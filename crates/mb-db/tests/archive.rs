//! The day file: sealing, the bytes, and every line read back as the same shop.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests: expect is the assertion"
)]

mod common;

use std::io::Read as _;

use mb_core::{AnyOrder, BusinessDay, OrderId, StaffId, Timestamp};
use mb_db::archive::{DayFile, SEAL_AFTER_DAYS, SealState};
use mb_db::repo::corrections::{Refund, RevertLine, RevertPayment, RevertRow};
use mb_db::repo::wire::{RestoreReport, WireRow, cloud_name};
use mb_db::{Repos, retention};

use common::{OUTLET, Scratch, TERMINAL, shop};

fn day(n: i32) -> BusinessDay {
    BusinessDay::from_days_since_epoch(20_600 + n)
}

fn at(n: i64) -> Timestamp {
    Timestamp::from_millis(1_770_000_000_000 + n * 60_000)
}

fn gunzip(gz: &[u8]) -> String {
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz).read_to_string(&mut text).expect("gunzip");
    text
}

fn build_file(db: &mb_db::Db, d: BusinessDay) -> DayFile {
    db.read_transaction(|tx| Repos::new(tx).archive().day_file(OUTLET, d, at(9_000), "test"))
        .expect("the day file")
}

#[test]
fn a_day_seals_three_days_on_or_when_it_is_locked_and_a_change_makes_it_dirty() {
    let scratch = Scratch::new("archive_seal");
    let db = scratch.open();
    shop::build(&db);

    db.read_transaction(|tx| {
        let archive = Repos::new(tx).archive();
        assert_eq!(archive.seal_state(OUTLET, day(0), day(0))?, SealState::Open);
        assert_eq!(archive.seal_state(OUTLET, day(0), day(SEAL_AFTER_DAYS - 1))?, SealState::Open);
        assert_eq!(archive.seal_state(OUTLET, day(0), day(SEAL_AFTER_DAYS))?, SealState::Sealed);
        Ok(())
    })
    .expect("seal rule");

    // Locked today: sealed at once.
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.days().lock(
            OUTLET,
            &mb_db::repo::DayRow {
                day: day(1),
                kind: mb_db::repo::DayKind::Trading,
                is_locked: true,
                closed_at: Some(at(500)),
                closed_by: Some(StaffId::new("staff_1")),
                reopened_at: None,
                reopened_by: None,
                note: None,
                bills: 0,
                net: mb_core::Money::ZERO,
                cash_taken: mb_core::Money::ZERO,
            },
        )?;
        assert_eq!(repos.archive().seal_state(OUTLET, day(1), day(1))?, SealState::Sealed);
        Ok(())
    })
    .expect("lock");

    // Pending: the sealed days with bills. Seen from day 3, days 0 (three days on) and 1
    // (locked) are pending; day 2 is still open.
    let pending = db
        .read_transaction(|tx| Repos::new(tx).archive().pending(OUTLET, day(3), 100))
        .expect("pending");
    assert_eq!(pending, vec![day(0), day(1)]);

    // Uploaded: gone from pending. Then a bill of that day saved again: dirty, pending again.
    let file = build_file(&db, day(0));
    db.transaction(|tx| Repos::new(tx).archive().record_upload(OUTLET, &file, at(600)))
        .expect("record");
    let pending = db
        .read_transaction(|tx| Repos::new(tx).archive().pending(OUTLET, day(3), 100))
        .expect("pending");
    assert_eq!(pending, vec![day(1)]);
    let a_bill_of_day_0 = db
        .read_transaction(|tx| Repos::new(tx).orders().find(&OrderId::new("ord_021")))
        .expect("read")
        .expect("ord_021");
    assert_eq!(a_bill_of_day_0.core().business_day, day(0));
    let AnyOrder::Settled(settled) = a_bill_of_day_0 else {
        panic!("ord_021 is settled in the fixture")
    };
    let voided = settled.void("late void", StaffId::new("staff_1"), at(700)).expect("void");
    db.transaction(|tx| Repos::new(tx).orders().save(OUTLET, TERMINAL, &AnyOrder::Voided(voided)))
        .expect("save the void");
    let pending = db
        .read_transaction(|tx| Repos::new(tx).archive().pending(OUTLET, day(3), 100))
        .expect("pending");
    assert_eq!(pending, vec![day(0), day(1)], "a void after the upload makes the day dirty");
    let ledger = db
        .read_transaction(|tx| Repos::new(tx).archive().ledger(OUTLET, day(0)))
        .expect("ledger")
        .expect("a row");
    assert_eq!(ledger.dirty_at, Some(at(700)));
    assert_eq!(ledger.uploaded_at, Some(at(600)));
}

#[test]
fn a_day_file_is_deterministic_and_its_header_is_what_the_push_would_send() {
    let scratch = Scratch::new("archive_bytes");
    let db = scratch.open();
    shop::build(&db);

    let once = build_file(&db, day(0));
    let twice = build_file(&db, day(0));
    assert_eq!(once.gz, twice.gz, "the same day gave different bytes");
    assert_eq!(once.sha256, twice.sha256);
    assert_eq!(once.sha256.len(), 64);
    assert!(once.bills > 0);
    assert_eq!(DayFile::key("rid", day(0)), format!("rid/{}/{}.jsonl.gz", day(0).to_ymd().0, day(0)));
    assert_eq!(DayFile::day_of_key(&DayFile::key("rid", day(0))), Some(day(0)));

    let text = gunzip(&once.gz);
    let mut lines = text.lines();
    let header: serde_json::Value = serde_json::from_str(lines.next().expect("a header")).expect("json");
    assert_eq!(header["$kind"], "day");
    assert_eq!(header["business_day"], i64::from(day(0).days_since_epoch()));
    assert_eq!(header["schema"], mb_db::migrate::latest_version());
    assert_eq!(header["bills"], once.bills);
    assert!(header["day_item_totals"].as_array().is_some_and(|a| !a.is_empty()));

    // The header's totals are exactly the rows `mb_push` would carry for that day.
    let key = day(0).days_since_epoch().to_string();
    let pushed = db
        .read_transaction(|tx| {
            let repos = Repos::new(tx);
            let entry = |table: &str| mb_db::repo::OutboxRow {
                id: mb_db::repo::OutboxRepo::entry_id(table, &key),
                table_name: table.to_owned(),
                row_id: key.clone(),
                op: mb_db::repo::Op::Upsert,
                tombstone: None,
                created_at: at(9_000),
                attempts: 0,
            };
            let totals = repos.wire().read(OUTLET, &entry("day_totals"))?;
            let items = repos.wire().read(OUTLET, &entry("day_item_totals"))?;
            Ok((totals, items))
        })
        .expect("push rows");
    assert_eq!(header["day_totals"], pushed.0[0].data);
    let items: Vec<serde_json::Value> = pushed.1.iter().map(|r| r.data.clone()).collect();
    assert_eq!(header["day_item_totals"], serde_json::Value::Array(items));

    // Every other line is a wire row of a bill of that day, bills first by created_at.
    let mut bills = 0;
    let mut last_created = 0;
    for line in lines {
        let row = WireRow::from_json(&serde_json::from_str(line).expect("json")).expect("a wire row");
        if row.table == cloud_name("orders") {
            bills += 1;
            assert_eq!(row.data["business_day"], i64::from(day(0).days_since_epoch()));
            let created = row.data["created_at"].as_i64().expect("created_at");
            assert!(created >= last_created, "bills are not in order");
            last_created = created;
            assert!(row.data["restore"].get("settled_at").is_none(), "restore repeats a typed column");
        }
    }
    assert_eq!(bills, once.bills);
}

#[test]
fn every_line_of_a_day_file_restores_the_same_bills_refunds_and_reverts() {
    let scratch = Scratch::new("archive_round_trip");
    let db = scratch.open();
    let built = shop::build(&db);

    // A refund and a revert on bills of day 0, so the file carries both.
    let voided_id = built
        .orders
        .iter()
        .find(|id| {
            db.read_transaction(|tx| Repos::new(tx).orders().find(id))
                .expect("read")
                .is_some_and(|o| matches!(&o, AnyOrder::Voided(v) if v.core.business_day == day(0)))
        })
        .expect("a voided bill on day 0")
        .clone();
    db.transaction(|tx| {
        Repos::new(tx).corrections().record_refund(
            OUTLET,
            &Refund {
                id: "rf_1".to_owned(),
                order_id: voided_id.clone(),
                amount: mb_core::Money::from_paise(1_000),
                mode: "cash".to_owned(),
                reason: "sorry".to_owned(),
                refunded_at: at(800),
                refunded_by: Some(StaffId::new("staff_1")),
            },
            day(0),
        )
    })
    .expect("refund");
    let settled_id = built
        .orders
        .iter()
        .find(|id| {
            db.read_transaction(|tx| Repos::new(tx).orders().find(id))
                .expect("read")
                .is_some_and(|o| matches!(&o, AnyOrder::Settled(s) if s.core.business_day == day(0)))
        })
        .expect("a settled bill on day 0")
        .clone();
    let AnyOrder::Settled(settled) = db
        .read_transaction(|tx| Repos::new(tx).orders().find(&settled_id))
        .expect("read")
        .expect("settled")
    else {
        panic!("settled")
    };
    let open = settled.clone().reopen();
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.orders().save(OUTLET, TERMINAL, &AnyOrder::Open(open.clone()))?;
        repos.corrections().record_revert(
            OUTLET,
            &RevertRow {
                id: "rvt_1".to_owned(),
                order_id: settled_id.clone(),
                business_day: day(0),
                reason: "wrong bill".to_owned(),
                reverted_at: at(810),
                reverted_by: Some(StaffId::new("staff_1")),
                before_total: settled.bill.grand_total,
                before_settled_at: settled.settled_at,
                before_settled_by: Some(settled.settled_by.clone()),
                approved_at: None,
                approved_by: None,
            },
            &[RevertLine {
                name: "Tea".to_owned(),
                qty: mb_core::Qty::from_thousandths(1_000),
                unit_price: mb_core::Money::from_paise(1_000),
                amount: mb_core::Money::from_paise(1_000),
            }],
            &[RevertPayment {
                mode: "Cash".to_owned(),
                amount: settled.bill.grand_total,
            }],
        )?;
        // And then cancelled: a billed order that was taken back and dropped travels as such.
        let cancelled = open.cancel("gave up", StaffId::new("staff_1"), at(820)).expect("cancel");
        repos.orders().save(OUTLET, TERMINAL, &AnyOrder::Cancelled(cancelled))?;
        Ok(())
    })
    .expect("revert");

    let file = build_file(&db, day(0));
    let text = gunzip(&file.gz);
    assert!(text.contains("\"table\":\"refunds\""), "the refund did not travel");
    assert!(text.contains("\"table\":\"bill_reverts\""), "the revert did not travel");
    assert!(text.contains("\"table\":\"bill_revert_lines\""), "the revert's lines did not travel");
    assert!(text.contains("\"table\":\"bill_revert_payments\""), "the revert's payments did not travel");
    assert!(text.contains("\"status\":\"cancelled\""), "the cancelled bill did not travel");

    // Down onto an empty computer that has the masters (a restore brings those first).
    let other = Scratch::new("archive_round_trip_down");
    let down = other.open();
    let masters = db
        .read_transaction(|tx| {
            let repos = Repos::new(tx);
            let mut rows = Vec::new();
            for entry in repos.outbox().pending(usize::MAX)? {
                if !["orders", "day_totals", "day_item_totals", "day_category_totals", "refunds", "bill_reverts"]
                    .contains(&entry.table_name.as_str())
                {
                    rows.extend(repos.wire().read(OUTLET, &entry)?);
                }
            }
            Ok(rows)
        })
        .expect("masters");
    let mut report = RestoreReport::default();
    down.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.wire().restore_rows(OUTLET, masters, &mut report)?;
        repos.archive().restore_text(OUTLET, &text, &mut report)?;
        repos.outbox().clear_backlog(at(0))?;
        Ok(())
    })
    .expect("restore");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(usize::try_from(report.bills).expect("small"), file.bills);
    assert_eq!(report.days, 1, "the header's totals came down");
    // The same file again — the window and the files overlap on a new PC — is one bill each.
    let mut again = RestoreReport::default();
    down.transaction(|tx| Repos::new(tx).archive().restore_text(OUTLET, &text, &mut again))
        .expect("again");
    assert_eq!(again.bills, 0, "{again:?}");
    assert_eq!(usize::try_from(again.unchanged).expect("small"), file.bills);

    // The same bills of that day, state for state and rupee for rupee.
    for id in &built.orders {
        let up = db.read_transaction(|tx| Repos::new(tx).orders().find(id)).expect("read").expect("built");
        if up.core().business_day != day(0) {
            continue;
        }
        let back = down.read_transaction(|tx| Repos::new(tx).orders().find(id)).expect("read");
        match (&up, back) {
            (AnyOrder::Settled(a), Some(AnyOrder::Settled(b))) => {
                assert_eq!(a.bill.grand_total, b.bill.grand_total);
                assert_eq!(a.bill_number.formatted, b.bill_number.formatted);
                assert_eq!(a.settled_at, b.settled_at, "settled_at read back from the typed column");
                assert_eq!(a.settled_by, b.settled_by);
            }
            (AnyOrder::Voided(a), Some(AnyOrder::Voided(b))) => {
                assert_eq!(a.bill.grand_total, b.bill.grand_total);
                assert_eq!(a.reason, b.reason);
            }
            (AnyOrder::Cancelled(a), Some(AnyOrder::Cancelled(b))) if a.bill_number.is_some() => {
                assert_eq!(a.reason, b.reason);
                assert_eq!(a.bill_number, b.bill_number);
            }
            (AnyOrder::Cancelled(a), None) if a.bill_number.is_none() => {}
            (AnyOrder::Settled(_) | AnyOrder::Voided(_), other) => panic!("{} came back as {other:?}", id.as_str()),
            _ => {}
        }
    }
    let refunds = down
        .read_transaction(|tx| Repos::new(tx).corrections().refunds_for(&voided_id))
        .expect("refunds");
    assert_eq!(refunds.len(), 1);
    assert_eq!(refunds[0].amount, mb_core::Money::from_paise(1_000));
    let (reverts, lines, payments) = down
        .read_transaction(|tx| {
            let c = Repos::new(tx).corrections();
            Ok((c.reverts_of(&settled_id)?, c.revert_lines("rvt_1")?, c.revert_payments("rvt_1")?))
        })
        .expect("reverts");
    assert_eq!(reverts.len(), 1);
    assert_eq!(lines.len(), 1);
    assert_eq!(payments.len(), 1);
    // And the day's totals are there for the day-wise report.
    let days = down
        .read_transaction(|tx| {
            Repos::new(tx)
                .wire()
                .cloud_days(OUTLET, mb_db::repo::reports::Period::one_day(day(0)))
        })
        .expect("cloud days");
    assert_eq!(days.len(), 1);

    // The fixture the phone's test reads, when the phone's repo is beside this one.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../MB-android/app/src/test/resources");
    if fixture.is_dir() {
        std::fs::write(fixture.join("day-file.jsonl.gz"), &file.gz).expect("write the fixture");
    }
}

#[test]
fn a_corrupt_line_is_counted_and_the_rest_of_the_file_lands() {
    let scratch = Scratch::new("archive_corrupt");
    let db = scratch.open();
    shop::build(&db);
    let file = build_file(&db, day(0));
    let text = gunzip(&file.gz);
    let mut lines: Vec<&str> = text.lines().collect();
    lines.insert(1, "this is not json");
    lines.insert(2, r#"{"table":"bills","id":"ghost","updated_at":1,"deleted":false,"data":{"restore":{"placement":"parcel"}}}"#);
    let broken = lines.join("\n");

    let other = Scratch::new("archive_corrupt_down");
    let down = other.open();
    let mut report = RestoreReport::default();
    // No masters: every bill fails on its own (no staff, no terminal), the totals still land,
    // and nothing is half-written.
    down.transaction(|tx| Repos::new(tx).archive().restore_text(OUTLET, &broken, &mut report))
        .expect("the file does not abort");
    assert_eq!(report.days, 1);
    assert_eq!(report.bills, 0);
    assert!(report.failed.len() >= 2, "{:?}", report.failed);
    assert!(report.failed.iter().any(|f| f.contains("ghost")));
    let orders: i64 = down
        .read(|c| Ok(c.query_row("SELECT count(*) FROM orders", [], |r| r.get(0))?))
        .expect("count");
    assert_eq!(orders, 0, "a failed bill left rows behind");
}

#[test]
fn a_shop_queued_whole_leaves_sealed_days_to_their_files() {
    let scratch = Scratch::new("archive_queue_shop");
    let db = scratch.open();
    shop::build(&db);
    db.transaction(|tx| {
        let outbox = Repos::new(tx).outbox();
        let all = outbox.pending(usize::MAX)?;
        let ids: Vec<&str> = all.iter().map(|r| r.id.as_str()).collect();
        outbox.mark_synced(&ids, at(0))
    })
    .expect("drain");
    // Seen from day 4: day 0 is sealed, days 1 and 2 are not.
    let unsealed_from = day(4 - SEAL_AFTER_DAYS + 1);
    db.transaction(|tx| Repos::new(tx).outbox().queue_shop(OUTLET, unsealed_from, at(1)))
        .expect("queue");
    let queued = db
        .read_transaction(|tx| Repos::new(tx).outbox().pending(usize::MAX))
        .expect("pending");
    let days_queued: std::collections::BTreeSet<String> = queued
        .iter()
        .filter(|e| e.table_name == "day_totals")
        .map(|e| e.row_id.clone())
        .collect();
    assert!(!days_queued.contains(&day(0).days_since_epoch().to_string()), "a sealed day was queued for the push");
    assert!(days_queued.contains(&day(2).days_since_epoch().to_string()));
    assert!(queued.iter().any(|e| e.table_name == "items"));
    assert!(queued.iter().any(|e| e.table_name == "counters"));
    let sealed_orders = queued.iter().filter(|e| e.table_name == "orders").count();
    assert!(sealed_orders > 0);
}

#[test]
fn retention_prunes_only_the_four_logs_and_only_past_their_days() {
    let scratch = Scratch::new("retention");
    let db = scratch.open();
    shop::build(&db);
    let now = at(0).add_millis(200 * 86_400_000).expect("later");

    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO applied_events (event_id, outlet_id, applied_at, source, result) VALUES ('old', ?1, ?2, 'phone', NULL), ('new', ?1, ?3, 'phone', NULL)",
            rusqlite::params![OUTLET, now.millis() - 8 * 86_400_000, now.millis() - 6 * 86_400_000],
        )?;
        Ok(())
    })
    .expect("seed");
    let before: i64 = db
        .read(|c| Ok(c.query_row("SELECT count(*) FROM orders", [], |r| r.get(0))?))
        .expect("orders");
    let ledger_before: i64 = db
        .read(|c| Ok(c.query_row("SELECT count(*) FROM kitchen_ledger", [], |r| r.get(0))?))
        .expect("ledger");
    assert!(ledger_before > 0);

    let pruned = retention::prune(&db, now).expect("prune");
    let names: Vec<&str> = pruned.rows.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(names, ["applied_events", "order_events", "payment_attempts", "kitchen_ledger"]);
    assert_eq!(pruned.rows[0].1, 1, "only the eight-day-old event");
    let left: Vec<String> = db
        .read(|c| {
            let mut stmt = c.prepare("SELECT event_id FROM applied_events ORDER BY 1")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .expect("left");
    assert_eq!(left, ["new"]);
    // The fixture's kitchen rows are 200 days old: gone. The money rows are all still here.
    let ledger_after: i64 = db
        .read(|c| Ok(c.query_row("SELECT count(*) FROM kitchen_ledger", [], |r| r.get(0))?))
        .expect("ledger");
    assert_eq!(ledger_after, 0);
    let after: i64 = db
        .read(|c| Ok(c.query_row("SELECT count(*) FROM orders", [], |r| r.get(0))?))
        .expect("orders");
    assert_eq!(before, after);
    // A money table can never be named: the list is the rule.
    assert!(!retention::RETENTION.iter().any(|(t, _, _)| {
        ["orders", "order_lines", "bills", "payments", "refunds", "bill_reverts", "customer_payments", "credit_adjustments"].contains(t)
    }));
}
