//! A read-only look at what a run actually wrote.
//!
//! Not a test and not shipped. It exists because "the bill is on disk" is a
//! claim about the disk, and reading it back is the only way to check it — the
//! screen that says "settled" is the thing under test.
//!
//!     cargo run -p mb-db --example peek -- <path-to.db>

#![allow(
    clippy::expect_used,
    reason = "a developer's one-shot reader: it takes a path on the command \
              line and its only sensible response to a bad one is to say so and \
              stop. Nothing ships it, and nothing calls it."
)]

fn main() {
    let path = std::env::args().nth(1).expect("usage: peek <db>");
    let db = mb_db::Db::open(&mb_db::DbConfig::new(std::path::Path::new(&path))).expect("open");
    db.transaction(|tx| {
        for sql in [
            "SELECT id, state, bill_number_formatted, token_formatted, order_type, table_id, created_by, terminal_id FROM orders",
            "SELECT order_id, seq, name, qty, unit_price FROM order_lines",
            "SELECT order_id, mode, amount FROM payments",
            "SELECT id, kind, printer_id, state, attempts, reason FROM print_jobs",
            // P11 and P12. The master plan tells every session to read the disk
            // back with this example, and until now it could not show who a
            // bill was voided by, whether the history hangs together, or
            // whether money went back — which is most of what those two
            // sessions built.
            "SELECT state, bill_number_formatted, void_reason, voided_by, cancel_reason, cancelled_by FROM orders WHERE state IN ('voided','cancelled')",
            "SELECT seq, action, entity, entity_id, staff_id, substr(hash,1,12) AS hash, substr(COALESCE(prev_hash,'-'),1,12) AS prev FROM audit_log ORDER BY seq",
            "SELECT id, name, status, CASE WHEN pin_hash IS NULL THEN 'no PIN' ELSE 'has a PIN' END AS pin, role_id FROM staff",
            "SELECT id, name, is_builtin, max_discount_bp, max_discount_paise FROM roles",
            "SELECT order_id, amount, mode, reason, refunded_by FROM refunds",
            "SELECT order_id, printed_at, printed_by, reason FROM reprints",
            "SELECT kind, COUNT(*) AS how_many FROM reasons GROUP BY kind",
            // P14. A merge and a split are invisible above: the merged-away
            // order shows only as `cancelled`, and the new half of a split
            // looks like any other open order. These two are what tell them
            // apart — and this file has now been extended three times for
            // exactly this reason, which is the argument for extending it.
            "SELECT id, state, bill_number_formatted, table_id, sub_table, covers, merged_into \
             FROM orders WHERE merged_into IS NOT NULL OR sub_table IS NOT NULL OR covers IS NOT NULL",
            "SELECT order_id, at, event, staff_id, detail FROM order_events ORDER BY at",
            "SELECT id, section_id, label, seats, pos_x, pos_y, is_active FROM dining_tables \
             WHERE pos_x IS NOT NULL OR is_active = 0",
            // P15. The credit account, from its three sources — and the
            // balance is deliberately NOT here, because there is no balance
            // column to read: it is the sum of these rows.
            "SELECT id, name, phone, phone_key, credit_limit FROM customers",
            "SELECT p.customer_id, o.bill_number_formatted, p.amount, o.state
               FROM payments p JOIN orders o ON o.id = p.order_id
              WHERE p.mode = 'credit'",
            "SELECT customer_id, amount, mode, reference, business_day FROM customer_payments",
            "SELECT customer_id, amount, increases, reason, made_by FROM credit_adjustments",
            // P16. Money going out, and the drawer. There is no cash-movement
            // row for a cash expense on purpose — the position is a query.
            "SELECT id, category_id, description, amount, mode, paid_to, gst_amount \n               FROM expenses",
            "SELECT kind, amount, reason, business_day FROM cash_movements",
            "SELECT id, description, amount, every, next_due, is_active FROM recurring_expenses",
            // P17. The two places a setting is kept, and reading them back is
            // the only proof that the settings screen did anything — the
            // screen saying "Saved" is the thing being tested.
            "SELECT name, phone, gstin, fssai, state_code, upi_id, upi_reference,
                    is_composition, default_place_of_supply
               FROM store_profile",
            "SELECT key, value, value_type, updated_by FROM settings ORDER BY key",
            "SELECT kind, terminal_id, prefix, pad_width, reset_daily, start, last_issued
               FROM counters",
            // P18. The night's record: what the till expected, what a person
            // counted, the difference, why, and whether the day is sealed. The
            // screen saying "Closed" is the thing being tested (audit B15).
            "SELECT business_day, opening_float, expected_cash, counted_cash, variance,
                    is_locked, closed_by, note
               FROM day_closes ORDER BY business_day",
            "SELECT day_close_id, denomination, count
               FROM day_close_denominations ORDER BY denomination DESC",
        ] {
            let mut stmt = tx.prepare(sql).expect("prepare");
            let cols = stmt.column_count();
            let rows: Vec<String> = stmt
                .query_map([], |row| {
                    Ok((0..cols)
                        .map(|i| match row.get_ref(i) {
                            Ok(rusqlite::types::ValueRef::Null) => "-".to_owned(),
                            Ok(rusqlite::types::ValueRef::Integer(n)) => n.to_string(),
                            Ok(rusqlite::types::ValueRef::Real(f)) => f.to_string(),
                            Ok(rusqlite::types::ValueRef::Text(t)) => String::from_utf8_lossy(t).into_owned(),
                            Ok(rusqlite::types::ValueRef::Blob(b)) => format!("{} bytes", b.len()),
                            Err(_) => "?".to_owned(),
                        })
                        .collect::<Vec<_>>()
                        .join(" | "))
                })
                .expect("query")
                .filter_map(Result::ok)
                .collect();
            println!("{sql}");
            if rows.is_empty() {
                println!("  (nothing)");
            } else {
                println!("  {}", rows.join("
  "));
            }
            println!();
        }
        Ok(())
    })
    .expect("read");
}
