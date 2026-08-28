//! The wire, both ways: every outbox row a whole shop queues can be read for the cloud, and
//! what was read can be written into an empty shop and be the same shop.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests: expect is the assertion"
)]

mod common;

use std::collections::BTreeMap;

use mb_core::Timestamp;
use mb_db::repo::wire::{ROW_KEY, ROW_TABLE_KEY, Restored, WireRow};
use mb_db::Repos;

use common::{OUTLET, Scratch, shop};

/// Every pending outbox row of a shop, shaped for the cloud.
fn everything_on_the_wire(db: &mb_db::Db) -> Vec<WireRow> {
    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        let pending = repos.outbox().pending(10_000)?;
        assert!(!pending.is_empty(), "the built shop queued nothing");
        let mut out = Vec::new();
        for entry in &pending {
            let rows = repos
                .wire()
                .read(OUTLET, entry)
                .unwrap_or_else(|e| panic!("{} {} could not be read for the wire: {e}", entry.table_name, entry.row_id));
            out.extend(rows);
        }
        Ok(out)
    })
    .expect("read")
}

fn count(db: &mb_db::Db, table: &str) -> i64 {
    // Only what is settled is a fact: open, draft and cancelled orders never travel.
    let sql = match table {
        "orders" => "SELECT count(*) FROM orders WHERE state IN ('settled', 'voided')".to_owned(),
        "order_lines" => "SELECT count(*) FROM order_lines l JOIN orders o ON o.id = l.order_id WHERE o.state IN ('settled', 'voided')".to_owned(),
        other => format!("SELECT count(*) FROM {other}"),
    };
    db.read(|conn| Ok(conn.query_row(&sql, [], |r| r.get(0))?))
        .expect("count")
}

#[test]
fn every_queued_row_of_a_whole_shop_can_be_shaped_for_the_cloud() {
    let scratch = Scratch::new("wire_up");
    let db = scratch.open();
    shop::build(&db);

    let rows = everything_on_the_wire(&db);
    let mut by_table: BTreeMap<String, usize> = BTreeMap::new();
    for row in &rows {
        *by_table.entry(row.table.clone()).or_default() += 1;
        // Every row serialises to the protocol's five keys.
        let json = row.to_json();
        for key in ["table", "id", "updated_at", "deleted", "data"] {
            assert!(json.get(key).is_some(), "{} {} lacks {key}", row.table, row.id);
        }
        assert!(row.data.is_object(), "{} {} data is not an object", row.table, row.id);
    }
    // A settled order is a bill with everything a restore needs.
    let bills: Vec<&WireRow> = rows.iter().filter(|r| r.table == "orders").collect();
    assert!(!bills.is_empty(), "no bills travelled: {by_table:?}");
    for bill in &bills {
        for key in ["bill_number", "grand_total_paise", "lines", "payments", "tax_rows", "restore", "business_day"] {
            assert!(bill.data.get(key).is_some(), "bill {} lacks {key}", bill.id);
        }
        assert!(bill.data["lines"].as_array().is_some_and(|l| !l.is_empty()));
        assert!(bill.data["restore"]["bill_input"].is_object());
    }
    // The totals travel by day.
    assert!(by_table.contains_key("day_totals"), "{by_table:?}");
    assert!(by_table.contains_key("day_item_totals"), "{by_table:?}");
    // Typed master rows carry the whole counter row, named.
    for typed in ["items", "categories", "staff", "customers", "expenses"] {
        let Some(row) = rows.iter().find(|r| r.table == typed) else {
            continue;
        };
        let whole = row.data.get(ROW_KEY).unwrap_or_else(|| panic!("{typed} carries no whole row"));
        assert_eq!(whole[ROW_TABLE_KEY], typed);
        assert!(whole.get("id").is_some());
    }
    // And a staff row never carries the PIN hash inside its whole row.
    for staff in rows.iter().filter(|r| r.table == "staff") {
        assert!(staff.data[ROW_KEY].get("pin_hash").is_none(), "the PIN hash rode inside the whole row");
    }
    // Everything else went into the box, column for column, with its own key column.
    let boxed: Vec<&WireRow> = rows
        .iter()
        .filter(|r| {
            !["orders", "day_totals", "day_item_totals", "day_category_totals", "items", "categories", "staff", "roles",
              "customers", "expenses", "expense_categories", "cash_movements", "customer_ledger"]
                .contains(&r.table.as_str())
        })
        .collect();
    assert!(!boxed.is_empty(), "nothing went into the box: {by_table:?}");
    for row in boxed {
        assert!(
            row.data.get("id").is_some() || row.data.get("key").is_some() || row.data.get("outlet_id").is_some(),
            "box row {} {} has no key column: {}",
            row.table,
            row.id,
            row.data
        );
    }
}

#[test]
fn what_went_up_comes_back_down_as_the_same_shop() {
    let scratch = Scratch::new("wire_round_trip");
    let db = scratch.open();
    let built = shop::build(&db);
    let rows = everything_on_the_wire(&db);

    // A second, empty computer.
    let other = Scratch::new("wire_round_trip_down");
    let down = other.open();
    let mut written = 0;
    let mut skipped = 0;
    down.transaction(|tx| {
        tx.execute_batch("PRAGMA defer_foreign_keys = ON")?;
        let repos = Repos::new(tx);
        let wire = repos.wire();
        // The cloud hands the box back first, then the typed tables, then the bills, then the
        // totals — the order the restore reads them in.
        let typed = ["orders", "roles", "staff", "items", "categories", "customers", "expenses",
                     "expense_categories", "cash_movements", "customer_ledger",
                     "day_totals", "day_item_totals", "day_category_totals"];
        for row in rows.iter().filter(|r| !typed.contains(&r.table.as_str())) {
            if wire.write_boxed(&row.table, &row.data)? {
                written += 1;
            } else {
                skipped += 1;
            }
        }
        for row in rows.iter().filter(|r| typed.contains(&r.table.as_str()) && r.table != "orders") {
            let cloud_name = match row.table.as_str() {
                "items" => "menu_items",
                "categories" => "menu_categories",
                other => other,
            };
            match wire.restore_row(OUTLET, cloud_name, &row.id, row.updated_at, &row.data)? {
                Restored::Written => written += 1,
                Restored::Skipped => skipped += 1,
            }
        }
        for row in rows.iter().filter(|r| r.table == "orders") {
            match wire.restore_row(OUTLET, "bills", &row.id, row.updated_at, &row.data)? {
                Restored::Written => written += 1,
                Restored::Skipped => skipped += 1,
            }
        }
        repos.outbox().clear_backlog(Timestamp::from_millis(1))?;
        Ok(())
    })
    .expect("everything comes down");
    assert!(written > 0);
    // Only the per-item and per-category totals have nowhere to go.
    let group_totals = rows.iter().filter(|r| r.table == "day_item_totals" || r.table == "day_category_totals").count();
    assert_eq!(skipped, group_totals, "something with a place to go was skipped");

    // The same shop: the tables that travelled have the same rows.
    for table in ["orders", "order_lines", "bills", "bill_lines", "payments", "items", "categories", "staff", "roles",
                  "role_permissions", "customers", "expenses", "expense_categories", "cash_movements",
                  "customer_payments", "credit_adjustments", "dining_tables", "sections", "printers", "settings",
                  "tax_classes"] {
        assert_eq!(count(&down, table), count(&db, table), "{table} did not come back whole");
    }
    // Every settled order is a settled order again, with the same total.
    for id in &built.orders {
        let up = db
            .read_transaction(|tx| Repos::new(tx).orders().find(id))
            .expect("read")
            .expect("the order that was built");
        let back = down
            .read_transaction(|tx| Repos::new(tx).orders().find(id))
            .expect("read");
        match (&up, back) {
            (mb_core::AnyOrder::Settled(a), Some(mb_core::AnyOrder::Settled(b))) => {
                assert_eq!(a.bill.grand_total, b.bill.grand_total, "order {}", id.as_str());
                assert_eq!(a.bill_number.formatted, b.bill_number.formatted);
                assert_eq!(a.core.cart.lines().len(), b.core.cart.lines().len());
            }
            (mb_core::AnyOrder::Voided(a), Some(mb_core::AnyOrder::Voided(b))) => {
                assert_eq!(a.bill.grand_total, b.bill.grand_total, "order {}", id.as_str());
                assert_eq!(a.reason, b.reason);
            }
            // Open and draft orders do not travel: only what is settled is a fact.
            (mb_core::AnyOrder::Settled(_) | mb_core::AnyOrder::Voided(_), other) => {
                panic!("order {} came back as {other:?}", id.as_str())
            }
            _ => {}
        }
    }
    // The staff came back with their whole row — a column the typed shape does not carry.
    let (up_address, down_address): (Option<String>, Option<String>) = (
        db.read(|c| Ok(c.query_row("SELECT address FROM staff WHERE id = 'staff_1'", [], |r| r.get(0))?)).expect("up"),
        down.read(|c| Ok(c.query_row("SELECT address FROM staff WHERE id = 'staff_1'", [], |r| r.get(0))?)).expect("down"),
    );
    assert_eq!(up_address, down_address);
    // And the second computer has nothing to send: the cloud already has it all.
    assert_eq!(
        down.read_transaction(|tx| Repos::new(tx).outbox().pending_count()).expect("pending"),
        0
    );
}

#[test]
fn a_day_the_counter_has_no_bills_for_is_read_from_the_cloud_totals() {
    use mb_db::repo::reports::{Period, SalesBy};

    let scratch = Scratch::new("wire_cloud_days");
    let db = scratch.open();
    shop::build(&db);

    let a_day_with_bills = db
        .read_transaction(|tx| {
            Repos::new(tx).reports().sales_by(
                OUTLET,
                Period::new(
                    mb_core::BusinessDay::from_days_since_epoch(20_000),
                    mb_core::BusinessDay::from_days_since_epoch(21_000),
                ),
                SalesBy::Day,
            )
        })
        .expect("report");
    assert!(!a_day_with_bills.is_empty());
    let known: i64 = a_day_with_bills[0].key.parse().expect("a day");

    // A year ago, only in the cloud; and the same day as a real one, which must not double.
    db.transaction(|tx| {
        let wire = Repos::new(tx).wire();
        for (day, gross) in [(known - 365, 123_400), (known, 999_999)] {
            wire.restore_row(
                OUTLET,
                "day_totals",
                &day.to_string(),
                Timestamp::from_millis(5),
                &serde_json::json!({ "business_day": day, "bills": 7, "gross_paise": gross, "discount_paise": 0, "tax_paise": 100 }),
            )?;
        }
        Ok(())
    })
    .expect("totals");

    let report = db
        .read_transaction(|tx| {
            Repos::new(tx).reports().sales_by(
                OUTLET,
                Period::new(
                    mb_core::BusinessDay::from_days_since_epoch(i32::try_from(known - 400).expect("fits")),
                    mb_core::BusinessDay::from_days_since_epoch(i32::try_from(known + 1).expect("fits")),
                ),
                SalesBy::Day,
            )
        })
        .expect("report");
    let old = report
        .iter()
        .find(|b| b.key == (known - 365).to_string())
        .expect("the old day is in the report");
    assert_eq!(old.gross.paise(), 123_400);
    assert_eq!(old.bills, 7);
    let same = report.iter().filter(|b| b.key == known.to_string()).count();
    assert_eq!(same, 1, "a day with bills of its own was doubled");
    let real = report.iter().find(|b| b.key == known.to_string()).expect("the real day");
    assert_ne!(real.gross.paise(), 999_999, "the cloud's figure replaced the counter's own bills");
    // Oldest first, as before.
    let keys: Vec<i64> = report.iter().map(|b| b.key.parse().unwrap()).collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted);
}

/// The cloud keeps the newest row per key, so a wire row stamped zero is a row the cloud
/// updates once and then never again — the owner's phone showed the day's first bill forever.
#[test]
fn no_wire_row_ever_carries_the_epoch_as_its_moment() {
    let scratch = Scratch::new("wire_moments");
    let db = scratch.open();
    shop::build(&db);
    for row in everything_on_the_wire(&db) {
        assert!(
            row.updated_at.millis() > 0,
            "{} {} went to the cloud stamped with the epoch",
            row.table,
            row.id
        );
    }
}
