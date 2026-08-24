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
            "SELECT name, phone, gstin, fssai, state_code, upi_id, upi_reference, registration
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
            // P19. Which phones this counter serves, and — the column that
            // matters — that a revoked one is still HERE, as a state and not a
            // deletion (D47).
            "SELECT id, name, platform, staff_id, last_ip,
                    CASE WHEN revoked_at IS NULL THEN 'live' ELSE 'removed' END AS state,
                    substr(secret_hash, 1, 20) AS credential
               FROM lan_devices ORDER BY paired_at",
            // P20. The idempotency ledger — crown jewel 11. One row per intent,
            // carrying what the counter answered, so a retry gets the same
            // sentence and never a second order.
            "SELECT event_id, source, substr(result, 1, 55) AS answered
               FROM applied_events ORDER BY applied_at",
            "SELECT day_close_id, denomination, count
               FROM day_close_denominations ORDER BY denomination DESC",
            // P25. **The stock book, and the point of reading it back is that a
            // balance is DERIVED.** The second query sums the ledger; the third
            // is the cache. If those two ever disagree, D114 is broken and the
            // screen saying "1.712 bag" is not evidence of anything.
            "SELECT m.id, m.name, m.dimension, m.avg_cost AS paise_per_1000,
                    m.reorder_level, m.buy_from, m.is_active
               FROM materials m ORDER BY m.name",
            "SELECT u.material_id, u.name, u.base_per_unit, u.is_purchase_default
               FROM material_units u ORDER BY u.material_id",
            "SELECT r.owner_kind,
                    COALESCE(r.item_id, r.modifier_id, r.material_id) AS owner,
                    r.batch_yield, l.material_id, l.base_qty, l.yield_percent,
                    l.typed_qty || ' ' || l.typed_unit AS as_typed
               FROM recipes r LEFT JOIN recipe_lines l ON l.recipe_id = r.id
              ORDER BY r.id, l.seq",
            "SELECT material_id, kind, base_qty, typed_qty || ' ' || typed_unit AS as_typed,
                    unit_cost, total_cost, was_automatic, produced_for, order_id
               FROM stock_movements ORDER BY at, id LIMIT 40",
            "SELECT material_id, SUM(base_qty) AS summed_from_the_ledger
               FROM stock_movements GROUP BY material_id ORDER BY material_id",
            "SELECT material_id, base_qty AS the_cached_balance FROM material_balances
              ORDER BY material_id",
            "SELECT kind, subject, occurrences, substr(sentence, 1, 60) AS says
               FROM stock_problems WHERE resolved_at IS NULL ORDER BY occurrences DESC",

            // **P26 — the paper, and the four ledgers it moves** (D120). The
            // screen saying "saved" is the thing being tested, so it is not
            // evidence; these rows are.
            "SELECT id, name, terms_days, gstin, is_active FROM suppliers ORDER BY name",
            "SELECT p.id, p.kind, p.invoice_no, p.business_day, p.due_day, p.lines_value,
                    p.charges, p.tax_total, p.tax_creditable, p.total, p.is_cancelled,
                    p.cancel_reason, p.attachment_id
               FROM purchases p ORDER BY p.business_day, p.received_at LIMIT 40",
            // The landed cost is the number this whole module exists to get
            // right, so it is in the dump beside what was typed (D109, D123).
            "SELECT pl.purchase_id, pl.seq, pl.material_id,
                    pl.typed_qty || ' ' || pl.typed_unit AS as_typed,
                    pl.base_qty, pl.free_base_qty, pl.rate, pl.discount,
                    pl.charge_share, pl.tax_amount, pl.landed_value,
                    pl.landed_unit_cost AS paise_per_1000, pl.movement_id, pl.returns_seq
               FROM purchase_lines pl ORDER BY pl.purchase_id, pl.seq LIMIT 60",
            "SELECT supplier_id, amount, mode, purchase_id, business_day
               FROM supplier_payments ORDER BY business_day, id",
            "SELECT supplier_id, amount, increases, reason, business_day
               FROM supplier_adjustments ORDER BY business_day, id",
            // **The balance is a SUM and never a column** — this is the query
            // that proves it, exactly as P15's does for a customer.
            "SELECT s.name,
                    (SELECT COALESCE(SUM(CASE WHEN p.kind = 'purchase' THEN p.total
                                              ELSE -p.total END), 0)
                       FROM purchases p
                      WHERE p.supplier_id = s.id AND p.is_cancelled = 0)
                  - (SELECT COALESCE(SUM(amount), 0) FROM supplier_payments sp
                      WHERE sp.supplier_id = s.id)
                    AS owed_summed_from_the_rows
               FROM suppliers s ORDER BY s.name",
            "SELECT po.number, po.state, po.supplier_id, l.material_id,
                    l.typed_qty || ' ' || l.typed_unit AS as_typed, l.rate
               FROM purchase_orders po LEFT JOIN purchase_order_lines l ON l.po_id = po.id
              ORDER BY po.created_at, l.seq",
            // **D127 — the frozen book, and the delta it posted.** A count line
            // whose `book_qty` equals today's balance is the bug this decision
            // exists to prevent, and it is visible right here.
            "SELECT c.id, c.location, c.state, c.business_day, c.approved_at, c.ended_reason
               FROM stock_counts c ORDER BY c.business_day, c.opened_at",
            "SELECT l.count_id, l.material_id, l.book_qty AS book_when_counted,
                    l.counted_qty, l.variance_qty, l.variance_value, l.reason_id,
                    l.movement_id
               FROM stock_count_lines l ORDER BY l.count_id, l.seq",
            // D132: a row here with no file beside the database is a detectable
            // fact, which is the whole reason the metadata is in the database.
            "SELECT id, kind, subject_id, filename, byte_count FROM attachments
              ORDER BY created_at",
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
