#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Scratch;
use common::shop::{self, OUTLET, TERMINAL};
use mb_core::{AnyOrder, CustomerId, ItemId, Money, OrderId, StaffId, TableId, Timestamp};
use mb_db::repo::floor::DiningTable;
use mb_db::{Db, DbError, Repos, backup, export, locate};

/// THE ONE THAT MATTERS.
#[test]
fn t1_a_whole_shop_survives_delete_and_restore() {
    let scratch = Scratch::new("t1");
    let dir = shop::backup_dir(&scratch);

    let (before, order_ids) = {
        let db = scratch.open();
        let built = shop::build(&db);

        assert_orders_round_trip(&db, &built.orders);

        let before = shop::snapshot(&db);
        shop::take_and_verify(&db, &dir, "nightly.db");
        (before, built.orders)
    };

    // The disk dies.
    std::fs::remove_file(scratch.db_path()).expect("delete the database");
    for suffix in ["-wal", "-shm"] {
        let mut side = scratch.db_path().into_os_string();
        side.push(suffix);
        let _ = std::fs::remove_file(std::path::PathBuf::from(side));
    }
    assert!(!scratch.db_path().exists());

    let report = backup::restore(&dir.join("nightly.db"), &scratch.db_path()).expect("restore");
    assert!(!report.rolled_back, "the restore rolled back: {report:?}");

    let db = scratch.open();
    let after = shop::snapshot(&db);

    assert_eq!(
        before.len(),
        after.len(),
        "a table appeared or vanished across the restore"
    );
    for ((table, before_rows), (after_table, after_rows)) in before.iter().zip(&after) {
        assert_eq!(table, after_table);
        // Sync_outbox is deliberately different: a restore re-queues everything, because the
        // counter has no idea what the cloud has seen.
        if table == "sync_outbox" {
            assert_eq!(
                before_rows.len(),
                after_rows.len(),
                "the outbox lost or gained rows across the restore"
            );
            continue;
        }
        assert_eq!(
            before_rows, after_rows,
            "table {table} does not match after the restore"
        );
    }

    // And the whole point, again, on the restored file.
    assert_orders_round_trip(&db, &order_ids);
}

/// The private-field wall, asserted: save → read → `==`.
fn assert_orders_round_trip(db: &Db, ids: &[OrderId]) {
    let mut states = std::collections::BTreeMap::new();
    db.read(|_| Ok(())).expect("the database opens");

    for id in ids {
        let found = db
            .transaction(|tx| Repos::new(tx).orders().find(id))
            .expect("find")
            .unwrap_or_else(|| panic!("order {id} is missing"));

        let label = match &found {
            AnyOrder::Draft(_) => "draft",
            AnyOrder::Open(_) => "open",
            AnyOrder::Settled(_) => "settled",
            AnyOrder::Cancelled(_) => "cancelled",
            AnyOrder::Voided(_) => "voided",
        };
        *states.entry(label).or_insert(0_usize) += 1;

        // Read it a second time and assert the two agree.
        let again = db
            .transaction(|tx| Repos::new(tx).orders().find(id))
            .expect("find again")
            .expect("still there");
        assert_eq!(found, again, "order {id} does not read back the same twice");

        // And saving what we read must be a no-op, which is the real proof that nothing was
        // lost on the way out.
        db.transaction(|tx| Repos::new(tx).orders().save(OUTLET, TERMINAL, &found))
            .expect("re-save");
        let third = db
            .transaction(|tx| Repos::new(tx).orders().find(id))
            .expect("find a third time")
            .expect("still there");
        assert_eq!(found, third, "order {id} changed when it was written back");
    }

    // The fixture is only useful if it really covers every state.
    for state in ["draft", "open", "settled", "cancelled", "voided"] {
        assert!(
            states.get(state).copied().unwrap_or(0) > 0,
            "the fixture has no {state} order, so T1 does not test one"
        );
    }
}

/// Saving an order is atomic: a failure after the lines leaves nothing.
#[test]
fn t2_saving_an_order_is_atomic() {
    let scratch = Scratch::new("t2");
    let db = scratch.open();
    shop::build(&db);

    let id = OrderId::new("ord_partial");
    let order = db
        .transaction(|tx| Repos::new(tx).orders().find(&OrderId::new("ord_001")))
        .expect("find")
        .expect("present");
    let mut clone = order.clone();
    match &mut clone {
        AnyOrder::Settled(o) => o.core.id = id.clone(),
        _ => panic!("ord_001 should be settled"),
    }

    let result: Result<(), DbError> = db.transaction(|tx| {
        Repos::new(tx).orders().save(OUTLET, TERMINAL, &clone)?;
        // Everything is written. Now the caller fails, the way a print failure or a licence
        // check might.
        Err(DbError::invariant("something went wrong after the write"))
    });
    assert!(result.is_err());

    db.read(|conn| {
        for (table, column) in [
            ("orders", "id"),
            ("order_lines", "order_id"),
            ("bills", "order_id"),
            ("bill_lines", "order_id"),
            ("payments", "order_id"),
            ("kitchen_ledger", "order_id"),
        ] {
            let n: i64 = conn.query_row(
                &format!("SELECT count(*) FROM {table} WHERE {column} = ?1"),
                [id.as_str()],
                |r| r.get(0),
            )?;
            assert_eq!(n, 0, "{table} kept rows from a rolled-back save");
        }
        Ok(())
    })
    .expect("count");
}

/// A corrupted backup fails verification, two different ways, and is not restorable.
#[test]
fn t3_a_corrupted_backup_is_caught_and_refused() {
    let scratch = Scratch::new("t3");
    let dir = shop::backup_dir(&scratch);
    let db = scratch.open();
    shop::build(&db);

    // (a) The manifest lies about the row counts.
    let good = shop::take_and_verify(&db, &dir, "counts.db");
    let manifest = std::fs::read_to_string(good.manifest_path()).expect("read");
    let tampered = manifest.replace("count orders ", "count orders 99999 ignored ");
    std::fs::write(good.manifest_path(), tampered).expect("write");
    let report = backup::verify(&good.path).expect("verify");
    assert!(!report.is_ok(), "a lying manifest was accepted");
    assert!(
        !report.count_mismatches.is_empty(),
        "the row-count check did not fire: {report:?}"
    );
    assert!(
        backup::restore(&good.path, &scratch.db_path()).is_err(),
        "a backup that failed verification was restored anyway"
    );

    // (b) The file itself is damaged.
    let bytes = shop::take_and_verify(&db, &dir, "bytes.db");
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&bytes.path)
            .expect("open");
        let len = std::fs::metadata(&bytes.path).expect("meta").len();
        f.seek(SeekFrom::Start(len / 2)).expect("seek");
        f.write_all(&[0xAB; 2048]).expect("corrupt");
    }
    // Either outcome is the file being caught, and which one you get depends on which page the
    // damage lands in.
    match backup::verify(&bytes.path) {
        Ok(report) => assert!(!report.is_ok(), "a damaged file was accepted: {report:?}"),
        Err(_) => { /* refused outright, which is the same answer, louder */ }
    }
    assert!(
        backup::restore(&bytes.path, &scratch.db_path()).is_err(),
        "a damaged backup was restored anyway"
    );
}

/// When the restored database fails its own verification, the safety copy goes back.
#[test]
fn t4_a_failed_restore_rolls_back_to_the_safety_copy() {
    let scratch = Scratch::new("t4");
    let dir = shop::backup_dir(&scratch);

    // The handle is dropped before anything restores: a restore aimed at a database the app
    // still has open is refused (see `restore_files`), and the sanctioned flow is
    // request-then-restart.
    let before = {
        let db = scratch.open();
        shop::build(&db);
        shop::take_and_verify(&db, &dir, "good.db");
        shop::snapshot(&db)
    };

    // First, the check that comes BEFORE anything is touched: a backup with an orphaned row is
    // refused outright, and the shop is never opened up.
    let orphaned = dir.join("orphaned.db");
    std::fs::copy(dir.join("good.db"), &orphaned).expect("copy");
    {
        let conn = rusqlite::Connection::open(&orphaned).expect("open");
        // Foreign keys off, which is exactly how a bad row gets into a database in the real
        // world — and precisely why `verify` runs `foreign_key_check` separately from
        // `integrity_check`.
        conn.execute_batch("PRAGMA foreign_keys = OFF;")
            .expect("pragma");
        conn.execute(
            "INSERT INTO order_lines (id, order_id, seq, name, unit_price, tax_rate_bp,
                                      tax_kind, tax_basis, qty, was_discount_capped)
             VALUES ('orphan', 'no_such_order', 99, 'Ghost', 100, 0, 'exempt', 'exclusive', 1000, 0)",
            [],
        )
        .expect("insert an orphan");
    }
    remanifest(&orphaned, &dir.join("good.db"));
    assert!(
        backup::restore(&orphaned, &scratch.db_path()).is_err(),
        "a backup with orphaned rows was restored"
    );

    // Now the rollback path itself.
    let sabotaged = dir.join("sabotaged.db");
    std::fs::copy(dir.join("good.db"), &sabotaged).expect("copy");
    {
        let conn = rusqlite::Connection::open(&sabotaged).expect("open");
        conn.execute("DELETE FROM schema_version", [])
            .expect("wipe the ledger");
    }
    remanifest(&sabotaged, &dir.join("good.db"));

    let report = backup::restore(&sabotaged, &scratch.db_path()).expect("restore runs");
    assert!(
        report.rolled_back,
        "a broken restore was accepted: {report:?}"
    );
    assert!(report.failure.is_some(), "the rollback did not say why");
    assert!(report.safety_copy.is_some(), "no safety copy was taken");

    let db = scratch.open();
    assert_eq!(
        before,
        shop::snapshot(&db),
        "the rollback did not put the shop back"
    );
}

fn remanifest(path: &std::path::Path, template: &std::path::Path) {
    let mut manifest_path = path.as_os_str().to_os_string();
    manifest_path.push(".manifest");
    let mut template_manifest = template.as_os_str().to_os_string();
    template_manifest.push(".manifest");
    let text = std::fs::read_to_string(std::path::PathBuf::from(template_manifest)).expect("read");

    // Recount and re-checksum by taking a fresh verify of the file and writing what it actually
    // contains.
    let conn = rusqlite::Connection::open(path).expect("open");
    let mut rebuilt = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("count ") {
            let table = rest.split_whitespace().next().unwrap_or_default();
            let n: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap_or(0);
            rebuilt.push_str(&format!("count {table} {n}\n"));
        } else if line.starts_with("checksum ") {
            rebuilt.push_str(&format!("checksum {}\n", fnv_file(path)));
        } else {
            rebuilt.push_str(line);
            rebuilt.push('\n');
        }
    }
    drop(conn);
    std::fs::write(std::path::PathBuf::from(manifest_path), rebuilt).expect("write");
}

fn fnv_file(path: &std::path::Path) -> String {
    use std::io::Read;
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut file = std::fs::File::open(path).expect("open");
    let mut hash = OFFSET;
    let mut buf = vec![0_u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).expect("read");
        if n == 0 {
            break;
        }
        for byte in &buf[..n] {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    format!("{hash:016x}")
}

/// A newer backup is refused; an older one is restored and migrated forward, which is the
/// ordinary case after an update.
#[test]
fn t5_newer_is_refused_and_older_is_migrated_forward() {
    let scratch = Scratch::new("t5");
    let dir = shop::backup_dir(&scratch);
    let taken = {
        let db = scratch.open();
        shop::build(&db);
        shop::take_and_verify(&db, &dir, "current.db")
    };

    // Pretend a future build made it.
    let future = dir.join("future.db");
    std::fs::copy(&taken.path, &future).expect("copy");
    {
        let conn = rusqlite::Connection::open(&future).expect("open");
        conn.execute(
            "INSERT INTO schema_version (version, name, checksum, applied_at, run_ms)
             VALUES (9999, '9999_from_the_future', 'f', 1, 0)",
            [],
        )
        .expect("insert");
    }
    remanifest(&future, &taken.path);

    match backup::restore(&future, &scratch.db_path()) {
        Err(DbError::NewerSchema { found, .. }) => assert_eq!(found, 9999),
        other => panic!("a newer backup was not refused: {other:?}"),
    }

    // And the ordinary case: the current one restores, and comes out at the version this build
    // knows.
    let report = backup::restore(&taken.path, &scratch.db_path()).expect("restore");
    assert!(!report.rolled_back);
    assert_eq!(report.migrated_to, mb_db::migrate::latest_version());
    let db = scratch.open();
    assert!(
        !shop::snapshot(&db).is_empty(),
        "the restored shop is readable"
    );
}

/// Retention keeps the newest N — and never none.
#[test]
fn t6_retention_keeps_the_newest_and_never_none() {
    const DAY_MS: i64 = 86_400_000;

    let scratch = Scratch::new("t6");
    let dir = shop::backup_dir(&scratch);
    let db = scratch.open();
    shop::build(&db);

    // 30 days of nightly backups, back-dated by rewriting each manifest.
    let now = 40 * DAY_MS;
    for n in 0..30_i64 {
        let taken = backup::take(&db, &dir.join(format!("b{n:02}.db")), "test").expect("take");
        backdate(&taken, now - (29 - n) * DAY_MS);
    }

    let pruned = backup::prune(&dir, 5).expect("prune");
    let left = backup::list(&dir).expect("list");
    assert_eq!(pruned.len(), 25);
    assert_eq!(left.len(), 5);
    assert!(
        left.iter().any(|b| b.path.ends_with("b29.db")),
        "the newest backup was pruned"
    );
    assert!(
        !left.iter().any(|b| b.path.ends_with("b00.db")),
        "the oldest survived"
    );

    // Asked to keep none, it keeps one: a shop is never left without a copy.
    backup::prune(&dir, 0).expect("prune again");
    assert_eq!(backup::list(&dir).expect("list").len(), 1);
}

fn backdate(taken: &backup::Backup, to_ms: i64) {
    let text = std::fs::read_to_string(taken.manifest_path()).expect("read");
    let rebuilt: String = text
        .lines()
        .map(|line| {
            if line.starts_with("taken_at_ms ") {
                format!("taken_at_ms {to_ms}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    std::fs::write(taken.manifest_path(), rebuilt).expect("write");
}

/// A backup taken while the shop is billing is consistent.
#[test]
fn t7_a_backup_during_active_billing_is_consistent() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let scratch = Scratch::new("t7");
    let dir = shop::backup_dir(&scratch);
    let db = Arc::new(scratch.open());
    shop::build(&db);

    let stop = Arc::new(AtomicBool::new(false));
    let taken = std::thread::scope(|scope| {
        let writer_db = Arc::clone(&db);
        let writer_stop = Arc::clone(&stop);
        scope.spawn(move || {
            let mut n = 1_000_i64;
            while !writer_stop.load(Ordering::SeqCst) {
                let id = OrderId::new(format!("ord_live_{n}"));
                let mut draft = mb_core::DraftOrder::new(
                    id,
                    mb_core::BusinessDay::from_days_since_epoch(20_600),
                    Timestamp::from_millis(1_770_000_000_000 + n),
                    mb_core::Placement::Parcel,
                    StaffId::new("staff_1"),
                );
                // Real lines, not an empty order: the assertion below is that no order was
                // copied WITHOUT its lines, and an order that never had any would make that
                // assertion vacuous.
                draft
                    .core
                    .cart
                    .add(
                        mb_core::ItemSnapshot::new(
                            ItemId::new("itm_dosa"),
                            "Masala Dosa",
                            Money::from_paise(12_000),
                            mb_core::TaxRate::from_percent(5).expect("5%"),
                        ),
                        mb_core::Qty::from_whole(2).expect("qty"),
                        None,
                        vec![],
                    )
                    .expect("add");
                let till = mb_db::Till::new(OUTLET, TERMINAL);
                if mb_db::settle::open_draft(&writer_db, till, draft).is_err() {
                    break;
                }
                n += 1;
            }
        });

        // Let the writer get going, then back up underneath it.
        std::thread::sleep(std::time::Duration::from_millis(30));
        let taken = backup::take(&db, &dir.join("during.db"), "test").expect("take");
        stop.store(true, Ordering::SeqCst);
        taken
    });

    let report = backup::verify(&taken.path).expect("verify");
    assert!(report.is_ok(), "{}", report.summary());

    // Consistency is more than integrity: every order in the copy must be whole.
    let conn = rusqlite::Connection::open(&taken.path).expect("open");
    let torn: i64 = conn
        .query_row(
            "SELECT count(*) FROM orders o
              WHERE NOT EXISTS (SELECT 1 FROM order_lines l WHERE l.order_id = o.id)
                AND o.state <> 'draft'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(torn, 0, "the backup caught an order without its lines");

    let billed_without_lines: i64 = conn
        .query_row(
            "SELECT count(*) FROM bills b
              WHERE NOT EXISTS (SELECT 1 FROM bill_lines l WHERE l.order_id = b.order_id)",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(
        billed_without_lines, 0,
        "a bill was copied without its lines"
    );
}

#[test]
fn t8_and_t9_export_import_round_trip_survives_nasty_text() {
    let scratch = Scratch::new("t8");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let mut nasty = shop_item();
        nasty.name = "Chicken \"Biryani\", Half\nSpecial".to_owned();
        repos
            .menu()
            .save_item(OUTLET, &nasty, Timestamp::from_millis(1))?;

        // A NULL note and an empty-string note on two otherwise identical rows.
        tx.execute(
            "UPDATE order_lines SET note = NULL WHERE id = 'ord_000_ln_0'",
            [],
        )?;
        tx.execute(
            "UPDATE order_lines SET note = '' WHERE id = 'ord_001_ln_0'",
            [],
        )?;
        Ok(())
    })
    .expect("plant the nasty rows");

    let before = shop::snapshot(&db);
    let folder = scratch.db_path().with_file_name("export");
    let report = export::export_all(&db, &folder, "test").expect("export");
    assert!(
        report.tables.iter().any(|(t, n)| t == "orders" && *n > 0),
        "the export wrote no orders"
    );
    assert!(
        report.database_copy.exists(),
        "the raw database was not exported"
    );

    // A dry run touches nothing.
    let fresh = Scratch::new("t8-import");
    let target = fresh.open();
    let dry = export::import_all(&target, &folder, true, false).expect("dry run");
    assert!(dry.dry_run);
    target
        .read(|conn| {
            let n: i64 = conn.query_row("SELECT count(*) FROM orders", [], |r| r.get(0))?;
            assert_eq!(n, 0, "a dry run wrote rows");
            Ok(())
        })
        .expect("count");

    export::import_all(&target, &folder, false, false).expect("import");
    let after = shop::snapshot(&target);

    for ((table, before_rows), (after_table, after_rows)) in before.iter().zip(&after) {
        assert_eq!(table, after_table);
        assert_eq!(
            before_rows, after_rows,
            "table {table} did not survive export and import"
        );
    }

    // And explicitly: NULL and "" are still different.
    target
        .read(|conn| {
            let null_note: Option<String> = conn.query_row(
                "SELECT note FROM order_lines WHERE id = 'ord_000_ln_0'",
                [],
                |r| r.get(0),
            )?;
            let empty_note: Option<String> = conn.query_row(
                "SELECT note FROM order_lines WHERE id = 'ord_001_ln_0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(null_note, None, "NULL came back as something else");
            assert_eq!(
                empty_note,
                Some(String::new()),
                "an empty string came back as NULL"
            );
            Ok(())
        })
        .expect("read the notes");

    // Refuses to import over a trading shop.
    assert!(
        export::import_all(&target, &folder, false, false).is_err(),
        "an import over a shop with orders in it was allowed"
    );
    export::import_all(&target, &folder, false, true).expect("force");
}

fn shop_item() -> mb_db::repo::menu::MenuItem {
    mb_db::repo::menu::MenuItem {
        id: ItemId::new("itm_nasty"),
        category_id: None,
        name: String::new(),
        unit_price: Money::from_paise(10_000),
        tax_class_id: mb_core::TaxClassId::new("tax_food_5"),
        price_basis: None,
        hsn: None,
        cost_price: None,
        short_code: None,
        prep_minutes: None,
        course: None,
        is_open_price: false,
        is_available: true,
        sort_order: 99,
    }
}

/// Money survives every round trip as exact paise.
#[test]
fn t10_money_is_the_same_integer_at_every_stage() {
    let scratch = Scratch::new("t10");
    let dir = shop::backup_dir(&scratch);
    let db = scratch.open();
    shop::build(&db);

    let totals = |db: &Db| -> Vec<(String, i64)> {
        db.read(|conn| {
            let mut stmt =
                conn.prepare("SELECT order_id, grand_total FROM bills ORDER BY order_id")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .expect("totals")
    };

    let original = totals(&db);
    assert!(!original.is_empty(), "the fixture has no bills");

    shop::take_and_verify(&db, &dir, "money.db");
    let folder = scratch.db_path().with_file_name("export");
    export::export_all(&db, &folder, "test").expect("export");
    drop(db);

    backup::restore(&dir.join("money.db"), &scratch.db_path()).expect("restore");
    let db = scratch.open();
    assert_eq!(original, totals(&db), "a total changed across the backup");

    let fresh = Scratch::new("t10-import");
    let target = fresh.open();
    export::import_all(&target, &folder, false, false).expect("import");
    assert_eq!(
        original,
        totals(&target),
        "a total changed across the export"
    );
}

/// Every write enqueues, in the same transaction.
#[test]
fn t11_every_write_enqueues_in_the_same_transaction() {
    let scratch = Scratch::new("t11");
    let db = scratch.open();
    shop::build(&db);

    // Every synced table must have produced outbox rows.
    db.read(|conn| {
        for table in [
            "orders",
            "items",
            "categories",
            "customers",
            "customer_payments",
            "expenses",
            "staff",
            "settings",
            "printers",
            "dining_tables",
            "day_closes",
        ] {
            let n: i64 = conn.query_row(
                "SELECT count(*) FROM sync_outbox WHERE table_name = ?1",
                [table],
                |r| r.get(0),
            )?;
            assert!(n > 0, "nothing was ever enqueued for {table}");
        }
        Ok(())
    })
    .expect("count the outbox");

    // And it is the SAME transaction: a rolled-back write leaves no outbox row.
    let before: i64 = db
        .transaction(|tx| Repos::new(tx).outbox().pending_count())
        .expect("count");
    let _ = db.transaction(|tx| -> Result<(), DbError> {
        Repos::new(tx).floor().save_table(
            OUTLET,
            &DiningTable {
                id: TableId::new("tbl_ghost"),
                section_id: None,
                label: "Ghost".to_owned(),
                seats: 2,
                pos: None,
                sort_order: 99,
                is_active: true,
            },
            Timestamp::from_millis(1),
        )?;
        Err(DbError::invariant("rolled back"))
    });
    let after: i64 = db
        .transaction(|tx| Repos::new(tx).outbox().pending_count())
        .expect("count");
    assert_eq!(
        before, after,
        "a rolled-back write left an outbox row behind"
    );
}

/// A restore re-queues the whole outbox, and the re-queue carries no payload.
#[test]
fn t12_a_restore_requeues_the_whole_outbox() {
    let scratch = Scratch::new("t12");
    let dir = shop::backup_dir(&scratch);
    let db = scratch.open();
    shop::build(&db);

    let total = db
        .transaction(|tx| {
            let outbox = Repos::new(tx).outbox();
            let pending = outbox.pending(usize::MAX)?;
            let ids: Vec<&str> = pending.iter().map(|r| r.id.as_str()).collect();
            outbox.mark_synced(&ids, Timestamp::from_millis(1))?;
            Ok(pending.len())
        })
        .expect("mark synced");
    assert!(total > 0);
    let still_pending = db
        .transaction(|tx| Repos::new(tx).outbox().pending_count())
        .expect("count");
    assert_eq!(still_pending, 0);

    shop::take_and_verify(&db, &dir, "synced.db");
    drop(db);
    backup::restore(&dir.join("synced.db"), &scratch.db_path()).expect("restore");

    let db = scratch.open();
    let after = db
        .transaction(|tx| Repos::new(tx).outbox().pending_count())
        .expect("count");
    assert_eq!(
        usize::try_from(after).expect("small"),
        total,
        "the restore did not re-queue the whole outbox"
    );

    // And it is still one narrow row per business row — no payload.
    db.read(|conn| {
        let with_payload: i64 = conn.query_row(
            "SELECT count(*) FROM sync_outbox WHERE op = 'upsert' AND tombstone IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(with_payload, 0, "an upsert is carrying a payload");
        Ok(())
    })
    .expect("check the payload");
}

/// A settle that fails does not burn a bill number.
#[test]
fn t13_a_failed_settle_returns_its_bill_number() {
    let scratch = Scratch::new("t13");
    let db = scratch.open();
    shop::build(&db);

    let next_number = |db: &Db| -> u64 {
        db.transaction(|tx| {
            mb_db::numbering::last_issued(tx, OUTLET, TERMINAL, mb_db::CounterKind::Bill)
        })
        .expect("last issued")
        .unwrap_or(0)
    };

    let before = next_number(&db);

    // A transaction that claims and then fails.
    let result: Result<(), DbError> = db.transaction(|tx| {
        mb_db::numbering::claim(
            tx,
            OUTLET,
            TERMINAL,
            mb_db::CounterKind::Bill,
            mb_core::BusinessDay::from_days_since_epoch(20_600),
        )?;
        Err(DbError::invariant("the printer caught fire"))
    });
    assert!(result.is_err());

    assert_eq!(
        next_number(&db),
        before,
        "a failed settle consumed a bill number"
    );
}

#[test]
fn t14_a_lost_config_finds_the_database_again() {
    let scratch = Scratch::new("t14");
    let db = scratch.open();
    shop::build(&db);
    drop(db);

    let config_dir = scratch.db_path().with_file_name("config");
    locate::write_config(&config_dir, &scratch.db_path()).expect("write config");
    assert_eq!(
        locate::read_config(&config_dir).expect("read"),
        Some(scratch.db_path())
    );

    std::fs::remove_file(locate::config_path(&config_dir)).expect("delete the config");
    assert_eq!(locate::read_config(&config_dir).expect("read"), None);

    // The live shop is still there and must be found, with enough about it to describe: "last
    // used on Tuesday, N bills".
    let found = locate::search_usual_places(&[scratch.db_path()]);
    let ours = found
        .iter()
        .find(|f| f.path == scratch.db_path())
        .expect("the live shop was not found (audit A5)");
    assert!(ours.orders > 0, "the found database reports no orders");
    assert!(ours.items > 0, "the found database reports no items");
    assert_eq!(ours.schema_version, mb_db::migrate::latest_version());

    // A stray file named shop.db is NOT a shop.
    let stray = scratch.db_path().with_file_name("stray-shop.db");
    std::fs::write(&stray, b"this is not a database").expect("write");
    assert!(
        locate::inspect(&stray).is_none(),
        "a stray file was offered to the owner as their shop"
    );

    // Nor is an unrelated SQLite file that happens to be in the folder.
    let empty = scratch.db_path().with_file_name("empty.db");
    rusqlite::Connection::open(&empty)
        .expect("open")
        .execute_batch("CREATE TABLE unrelated (x TEXT);")
        .expect("create");
    assert!(
        locate::inspect(&empty).is_none(),
        "an unrelated SQLite file was offered as a shop"
    );
}

/// A restore cannot be aimed at a live database, and the restore-at-startup seam works.
#[test]
fn t15_a_restore_is_requested_and_performed_before_the_database_opens() {
    let scratch = Scratch::new("t15");
    let dir = shop::backup_dir(&scratch);
    let config_dir = scratch.db_path().with_file_name("config");

    let db = scratch.open();
    shop::build(&db);
    let taken = shop::take_and_verify(&db, &dir, "chosen.db");
    drop(db);

    assert!(locate::read_config(&config_dir).expect("read").is_none());
    assert!(backup::pending_restore(&config_dir).is_none());

    backup::request_restore(&config_dir, &taken.path).expect("request");
    let pending = backup::pending_restore(&config_dir).expect("a restore is pending");
    assert_eq!(pending.from, taken.path);

    backup::restore(&pending.from, &scratch.db_path()).expect("restore");
    backup::clear_pending_restore(&config_dir).expect("clear");

    assert!(
        backup::pending_restore(&config_dir).is_none(),
        "the request survived its own attempt — that is a boot loop"
    );
    let db = scratch.open();
    assert!(!shop::snapshot(&db).is_empty());
}

/// The credit balance is a SUM, and there is no column to disagree with it.
#[test]
fn t16_the_credit_balance_is_computed_not_stored() {
    let scratch = Scratch::new("t16");
    let db = scratch.open();
    shop::build(&db);
    let customer = CustomerId::new("cus_1");

    let balance = |db: &Db| {
        db.transaction(|tx| Repos::new(tx).money().customer_balance(&customer))
            .expect("balance")
    };

    let before = balance(&db);
    db.transaction(|tx| {
        Repos::new(tx).money().record_credit_payment(
            OUTLET,
            &mb_db::repo::money::CreditPayment {
                id: "cpay_extra".to_owned(),
                customer_id: customer.clone(),
                amount: Money::from_paise(10_000),
                mode: "upi".to_owned(),
                reference: None,
                received_at: Timestamp::from_millis(2),
                received_by: None,
                business_day: mb_core::BusinessDay::from_days_since_epoch(20_601),
            },
        )
    })
    .expect("repay");

    assert_eq!(
        balance(&db).paise(),
        before.paise() - 10_000,
        "the balance did not follow the ledger"
    );

    // And there is no stored balance anywhere to drift from it.
    db.read(|conn| {
        for column in mb_db::schema::columns(conn, "customers")? {
            assert!(
                !column.name.contains("balance"),
                "customers.{} is a stored balance — two sources of truth",
                column.name
            );
        }
        Ok(())
    })
    .expect("inspect");
}

/// A sold item cannot be deleted, and the refusal says why in words.
#[test]
fn t17_a_sold_item_cannot_be_deleted_and_says_so() {
    let scratch = Scratch::new("t17");
    let db = scratch.open();
    shop::build(&db);

    let err = db
        .transaction(|tx| {
            Repos::new(tx).menu().delete_item(
                OUTLET,
                &ItemId::new("itm_dosa"),
                Timestamp::from_millis(1),
            )
        })
        .expect_err("a sold item was deleted");
    let message = err.to_string();
    assert!(
        message.contains("has been sold") && message.contains("take it off the menu"),
        "the refusal leaks a constraint instead of explaining: {message}"
    );

    // Taking it off the menu works, and the bills are untouched.
    db.transaction(|tx| {
        Repos::new(tx).menu().set_available(
            OUTLET,
            &ItemId::new("itm_dosa"),
            false,
            Timestamp::from_millis(2),
        )
    })
    .expect("take it off the menu");

    // An item nobody ever sold deletes normally.
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let mut typo = shop_item();
        typo.id = ItemId::new("itm_typo");
        repos
            .menu()
            .save_item(OUTLET, &typo, Timestamp::from_millis(3))?;
        repos
            .menu()
            .delete_item(OUTLET, &ItemId::new("itm_typo"), Timestamp::from_millis(4))
    })
    .expect("an unsold item must be deletable");
}

/// Typed settings: a mismatch is an error, a missing key is `None`.
#[test]
fn t18_settings_are_typed_and_never_default() {
    let scratch = Scratch::new("t18");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let settings = Repos::new(tx).settings();

        assert_eq!(
            settings.get::<i64>(OUTLET, "day.starts_at_minutes")?,
            Some(300)
        );
        assert_eq!(settings.get::<bool>(OUTLET, "bill.show_hsn")?, Some(true));
        assert_eq!(
            settings.get::<Money>(OUTLET, "day.opening_float")?,
            Some(Money::from_paise(200_000))
        );

        // A key nobody has set is None — not zero, not false, not "".
        assert_eq!(settings.get::<i64>(OUTLET, "nobody.set.this")?, None);
        assert_eq!(settings.get::<String>(OUTLET, "nobody.set.this")?, None);

        // Reading an int as a bool is an error.
        assert!(
            settings
                .get::<bool>(OUTLET, "day.starts_at_minutes")
                .is_err()
        );
        assert!(settings.get::<i64>(OUTLET, "bill.footer").is_err());
        Ok(())
    })
    .expect("settings");
}

/// A permission that is not a row cannot be granted.
#[test]
fn a_permission_typo_is_refused_not_silently_denied() {
    let scratch = Scratch::new("perms");
    let db = scratch.open();
    shop::build(&db);

    // Written past the repository, straight at the table.
    let err = db
        .transaction(|tx| {
            tx.execute(
                "INSERT INTO role_permissions (role_id, permission_code)
                 VALUES ('role_cashier', 'bill.craete')",
                [],
            )
            .map_err(mb_db::DbError::from)
        })
        .expect_err("a typo'd permission was accepted by the database");
    assert!(
        err.to_string().to_lowercase().contains("foreign key"),
        "the foreign key did not refuse it: {err}"
    );

    // And the real thing resolves through to the staff member, typed.
    let staff = db
        .transaction(|tx| Repos::new(tx).people().list_staff(OUTLET))
        .expect("staff");
    let ravi = staff
        .iter()
        .find(|s| s.id == StaffId::new("staff_1"))
        .expect("Ravi");
    assert!(ravi.permissions.has(mb_auth::Permission::BillCreate));
    assert!(!ravi.permissions.has(mb_auth::Permission::BillVoid));
}

/// A code the program does not know is an error at the row.
#[test]
fn a_permission_this_build_does_not_know_is_an_error_not_a_quiet_deny() {
    let scratch = Scratch::new("perm_unknown");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO permissions (code, description)
             VALUES ('fusion.reactor', 'Something a later version added')",
            [],
        )?;
        tx.execute(
            "INSERT INTO role_permissions (role_id, permission_code)
             VALUES ('role_cashier', 'fusion.reactor')",
            [],
        )?;
        Ok(())
    })
    .expect("a future permission");

    let err = db
        .transaction(|tx| Repos::new(tx).people().list_staff(OUTLET))
        .expect_err("an unknown permission was silently dropped");
    assert!(
        err.to_string().contains("fusion.reactor"),
        "the error does not name the permission: {err}"
    );
}
