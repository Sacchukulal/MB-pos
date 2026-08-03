//! What the database actually does: T5, T6, T7, T13, T14, T15, T16, T22, T23.
//!
//! The schema tests read the declarations. These ones make the declarations
//! prove themselves.

// See clippy.toml (P01). The fixtures at the bottom of this file build rows,
// and the closures passed to `Db::read` / `Db::transaction` are not `#[test]`
// functions, so the exemption does not reach them. `integer_division` is
// allowed because splitting a grand total three ways for a split-payment
// fixture is arithmetic on a test amount, not on a customer's money.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    reason = "tests: expect is the assertion, and the fixtures split a fake total"
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::Scratch;
use mb_core::{
    Bill, BillInput, Cart, Charge, ChargeKind, CustomerId, Discount, DiscountEntry, ItemId,
    ItemSnapshot, LineIdentity, ModifierId, Money, OrderType, Payment, PaymentMode, PlaceOfSupply,
    Qty, RoundingMode, Settlement, TaxRate, TaxTreatment, compute_bill,
};
use mb_db::numbering::{self, CounterKind};
use mb_db::encode;
use rusqlite::Transaction;

/// `gross, line_discount, bill_discount_share, net, taxable, cgst, sgst, igst,`
/// `gross_including_tax, rate_bp, treatment` — D4's pipeline, in its own order.
type BillLineRow = (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, String);

/// `mode, customer_id, mode_label, amount, tip, settles_khata` — the tag, both
/// payloads, and the flag audit B12 says must not be a mode.
type PaymentRow = (String, Option<String>, Option<String>, i64, i64, i64);

/// T5. Foreign keys are enforced — **on a connection taken from the reader
/// pool**, not only on the writer.
///
/// `PRAGMA foreign_keys` is per-connection, is OFF by default, and is not
/// stored in the file. v1 never turned it on at all. A test that only checked
/// the writer would happily pass on that bug.
#[test]
fn t5_foreign_keys_are_enforced_on_every_connection() {
    let db = Scratch::new("t5").open();

    // The writer.
    let orphan = db.transaction(|tx| {
        tx.execute(
            "INSERT INTO items (id, outlet_id, category_id, name, unit_price,
                                created_at, updated_at)
             VALUES ('itm_x', 'outlet_default', 'cat_nope', 'Ghost', 100, 0, 0)",
            [],
        )
        .map_err(Into::into)
    });
    assert!(orphan.is_err(), "the writer allowed an orphan row");

    // And a reader, which is where the per-connection pragma actually bites.
    db.read(|conn| {
        let pragma: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
        assert_eq!(pragma, 1, "a pooled reader has foreign keys OFF");
        Ok(())
    })
    .expect("read the pragma");
}

/// T6. A transaction that fails leaves nothing behind, across every table it
/// touched.
#[test]
fn t6_a_rolled_back_transaction_leaves_nothing() {
    let db = Scratch::new("t6").open();

    let result: Result<(), mb_db::DbError> = db.transaction(|tx| {
        tx.execute_batch(common::STAFF_SQL)?;
        tx.execute_batch(common::FLOOR_SQL)?;
        tx.execute_batch(common::MENU_SQL)?;
        tx.execute_batch(
            "INSERT INTO expense_categories (id, outlet_id, name)
             VALUES ('exc_1', 'outlet_default', 'Gas');",
        )?;
        Err(mb_db::DbError::invariant("something went wrong at the counter"))
    });
    assert!(result.is_err());

    db.read(|conn| {
        for table in ["staff", "sections", "dining_tables", "categories", "items", "expense_categories"] {
            let n: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
            assert_eq!(n, 0, "{table} kept rows from a rolled-back transaction");
        }
        Ok(())
    })
    .expect("count");
}

/// T7. WAL — a long read does not block a write.
///
/// This is scope 16.6 and the whole reason the reader pool exists. Audit E1: "a
/// heavy report on a slow PC can make the search box stutter mid-rush."
#[test]
fn t7_a_long_read_does_not_block_a_write() {
    let scratch = Scratch::new("t7");
    let db = Arc::new(scratch.open());
    db.transaction(|tx| {
        tx.execute_batch(common::STAFF_SQL)?;
        Ok(())
    })
    .expect("seed");

    let writing = Arc::new(AtomicBool::new(false));
    let written = Arc::new(AtomicBool::new(false));

    std::thread::scope(|scope| {
        let reader_db = Arc::clone(&db);
        let reader_writing = Arc::clone(&writing);
        let reader_written = Arc::clone(&written);

        scope.spawn(move || {
            reader_db
                .read(|conn| {
                    // Hold a real read transaction open — a snapshot, exactly
                    // as a long report would.
                    conn.execute_batch("BEGIN DEFERRED; SELECT count(*) FROM staff;")?;
                    reader_writing.store(true, Ordering::SeqCst);
                    // Give the writer time to finish while this read is open.
                    for _ in 0..200 {
                        if reader_written.load(Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    conn.execute_batch("COMMIT;")?;
                    Ok(())
                })
                .expect("the long read");
        });

        while !writing.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }

        let started = std::time::Instant::now();
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO expense_categories (id, outlet_id, name)
                 VALUES ('exc_1', 'outlet_default', 'Gas')",
                [],
            )?;
            Ok(())
        })
        .expect("the write must not have been blocked out");
        let elapsed = started.elapsed();
        written.store(true, Ordering::SeqCst);

        assert!(
            elapsed < std::time::Duration::from_millis(1_000),
            "the write waited {elapsed:?} behind an open read — WAL is not on"
        );
    });

    // And the reverse: the reader's snapshot is still readable afterwards.
    db.read(|conn| {
        let n: i64 = conn.query_row("SELECT count(*) FROM expense_categories", [], |r| r.get(0))?;
        assert_eq!(n, 1);
        Ok(())
    })
    .expect("read after write");
}

/// T13. Ten thousand claims from several threads: no repeats, no gaps,
/// ascending.
///
/// This is B4. A `SELECT` followed by an `UPDATE` fails this test; one
/// statement passes it.
#[test]
fn t13_ten_thousand_claims_have_no_repeats_and_no_gaps() {
    const THREADS: usize = 4;
    const PER_THREAD: usize = 2_500;

    let scratch = Scratch::new("t13");
    let db = Arc::new(scratch.open());
    let day = mb_core::BusinessDay::from_ymd(2026, 8, 3);

    let claimed: Vec<u64> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let db = Arc::clone(&db);
                scope.spawn(move || {
                    let mut mine = Vec::with_capacity(PER_THREAD);
                    for _ in 0..PER_THREAD {
                        let c = db
                            .transaction(|tx| {
                                numbering::claim(
                                    tx,
                                    common::OUTLET,
                                    common::TERMINAL,
                                    CounterKind::Bill,
                                    day,
                                )
                            })
                            .expect("claim");
                        mine.push(c.value);
                    }
                    mine
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("thread"))
            .collect()
    });

    let mut sorted = claimed;
    sorted.sort_unstable();
    assert_eq!(sorted.len(), THREADS * PER_THREAD);
    for (i, value) in sorted.iter().enumerate() {
        let expected = u64::try_from(i + 1).expect("small");
        assert_eq!(
            *value, expected,
            "bill numbers must be 1..=n with no repeat and no gap"
        );
    }
}

/// T14. The reset happens **inside** the claim.
///
/// No restart, no settings visit, no separate reset call — which is exactly
/// audit B3: "the counter PC is set to never sleep and stays on for days… so if
/// the app is never closed, the token number never resets at midnight."
#[test]
fn t14_the_daily_reset_happens_inside_the_claim() {
    let scratch = Scratch::new("t14");
    let db = scratch.open();
    let day1 = mb_core::BusinessDay::from_ymd(2026, 8, 3);
    let day2 = day1.next();
    let day3 = day2.next();

    let claim = |day| {
        db.transaction(|tx| {
            numbering::claim(tx, common::OUTLET, common::TERMINAL, CounterKind::Token, day)
        })
        .expect("claim")
    };

    assert_eq!(claim(day1).value, 1);
    assert_eq!(claim(day1).value, 2);
    assert_eq!(claim(day1).value, 3, "it must not reset twice in a day");

    assert_eq!(claim(day2).value, 1, "a new day resets the token series");
    assert_eq!(claim(day2).value, 2);

    // Three days without the app ever restarting. This is the test that would
    // have caught B3.
    assert_eq!(claim(day3).value, 1);

    // And the bill series, which does NOT reset daily, keeps running.
    let bill = |day| {
        db.transaction(|tx| {
            numbering::claim(tx, common::OUTLET, common::TERMINAL, CounterKind::Bill, day)
        })
        .expect("claim")
    };
    assert_eq!(bill(day1).value, 1);
    assert_eq!(bill(day2).value, 2);
    assert_eq!(bill(day3).value, 3);
    // pad_width 4 on the bill counter, so the printed form is zero-padded.
    assert_eq!(bill(day3).formatted, "0004");
}

/// The claim's counterpart for the settings screen, and the proof that it
/// cannot be mistaken for one.
#[test]
fn the_settings_edit_reads_the_past_and_writes_the_future() {
    let scratch = Scratch::new("setnext");
    let db = scratch.open();
    let day = mb_core::BusinessDay::from_ymd(2026, 8, 3);

    db.transaction(|tx| {
        assert_eq!(
            numbering::last_issued(tx, common::OUTLET, common::TERMINAL, CounterKind::Bill)?,
            None,
            "nothing has been issued yet, which is not the same as zero"
        );
        let first = numbering::claim(tx, common::OUTLET, common::TERMINAL, CounterKind::Bill, day)?;
        assert_eq!(first.value, 1);
        assert_eq!(
            numbering::last_issued(tx, common::OUTLET, common::TERMINAL, CounterKind::Bill)?,
            Some(1)
        );

        // The owner types "the next bill should be 500".
        numbering::set_next(tx, common::OUTLET, common::TERMINAL, CounterKind::Bill, 500)?;
        let next = numbering::claim(tx, common::OUTLET, common::TERMINAL, CounterKind::Bill, day)?;
        assert_eq!(next.value, 500);
        Ok(())
    })
    .expect("settings round trip");
}

/// T15 and T16, on one bill, because building it is the expensive part.
///
/// **T15** proves requirement 7 of the ten — "the printed bill's lines always
/// sum to its printed total" — with a SQL statement and no Rust arithmetic, so
/// the same proof still works over a year of bills.
///
/// **T16** proves the type mapping in `encode.rs` is real: every value written
/// comes back identical, including the two payload-carrying payment modes, both
/// charge bases, the capped flag and the rounding mode.
#[test]
fn t15_and_t16_a_real_bill_reconciles_in_sql_and_survives_the_round_trip() {
    let scratch = Scratch::new("t15");
    let db = scratch.open();
    let day = mb_core::BusinessDay::from_ymd(2026, 8, 3);

    let (bill, settlement) = build_a_real_bill();

    db.transaction(|tx| {
        tx.execute_batch(common::STAFF_SQL)?;
        tx.execute_batch(common::FLOOR_SQL)?;
        tx.execute_batch(common::MENU_SQL)?;
        tx.execute_batch(
            "INSERT INTO customers (id, outlet_id, name, created_at, updated_at)
             VALUES ('cus_1', 'outlet_default', 'Regular', 0, 0);",
        )?;
        write_settled_order(tx, "ord_1", day, &bill, &settlement)
    })
    .expect("write the bill");

    // ---- T15: the reconciliation, in SQL. -------------------------------
    db.read(|conn| {
        let (lines, charges, round_off, grand): (i64, i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COALESCE(SUM(gross_including_tax), 0) FROM bill_lines   WHERE order_id = b.order_id),
                (SELECT COALESCE(SUM(gross_including_tax), 0) FROM bill_charges WHERE order_id = b.order_id),
                b.round_off,
                b.grand_total
             FROM bills b WHERE b.order_id = 'ord_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        assert_eq!(
            lines + charges + round_off,
            grand,
            "the stored lines and charges do not sum to the stored grand total"
        );

        // And the rate-wise summary ties to the stored tax totals, which is
        // what makes the GSTR-1 report (2.8) a GROUP BY instead of a
        // recomputation.
        let (taxable, cgst, sgst, igst): (i64, i64, i64, i64) = conn.query_row(
            "SELECT COALESCE(SUM(taxable),0), COALESCE(SUM(cgst),0),
                    COALESCE(SUM(sgst),0), COALESCE(SUM(igst),0)
               FROM bill_tax_rows WHERE order_id = 'ord_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let (b_taxable, b_cgst, b_sgst, b_igst): (i64, i64, i64, i64) = conn.query_row(
            "SELECT total_taxable, total_cgst, total_sgst, total_igst
               FROM bills WHERE order_id = 'ord_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        assert_eq!((taxable, cgst, sgst, igst), (b_taxable, b_cgst, b_sgst, b_igst));
        Ok(())
    })
    .expect("reconcile in SQL");

    // ---- T16: the round trip, value by value. ---------------------------
    db.read(|conn| {
        let (total_taxable, non_gst, round_off, grand, place, rounding, capped): (
            i64, i64, i64, i64, String, String, i64,
        ) = conn.query_row(
            "SELECT total_taxable, non_gst_value, round_off, grand_total,
                    place_of_supply, rounding_mode, was_bill_discount_capped
               FROM bills WHERE order_id = 'ord_1'",
            [],
            |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?,
                ))
            },
        )?;
        assert_eq!(encode::money_from_sql(total_taxable), bill.total_taxable);
        assert_eq!(encode::money_from_sql(non_gst), bill.non_gst_value);
        assert_eq!(encode::money_from_sql(round_off), bill.round_off);
        assert_eq!(encode::money_from_sql(grand), bill.grand_total);
        assert_eq!(
            encode::place_of_supply_from_sql(&place).expect("known"),
            bill.place_of_supply
        );
        assert_eq!(
            encode::rounding_mode_from_sql(&rounding).expect("known"),
            bill.rounding
        );
        assert_eq!(
            encode::bool_from_sql(capped, "bills.was_bill_discount_capped").expect("0 or 1"),
            bill.bill_discount_capped
        );

        // Every computed line, in order.
        let mut stmt = conn.prepare(
            "SELECT bl.gross, bl.line_discount, bl.bill_discount_share, bl.net,
                    bl.taxable, bl.cgst, bl.sgst, bl.igst, bl.gross_including_tax,
                    bl.rate_bp, bl.treatment
               FROM bill_lines bl
               JOIN order_lines ol ON ol.id = bl.order_line_id
              WHERE bl.order_id = 'ord_1'
              ORDER BY ol.seq",
        )?;
        let rows: Vec<BillLineRow> = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?,
                    r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        assert_eq!(rows.len(), bill.lines.len());
        for (row, line) in rows.iter().zip(&bill.lines) {
            assert_eq!(encode::money_from_sql(row.0), line.gross);
            assert_eq!(encode::money_from_sql(row.1), line.line_discount);
            assert_eq!(encode::money_from_sql(row.2), line.bill_discount_share);
            assert_eq!(encode::money_from_sql(row.3), line.net);
            assert_eq!(encode::money_from_sql(row.4), line.taxable);
            assert_eq!(encode::money_from_sql(row.5), line.tax.cgst);
            assert_eq!(encode::money_from_sql(row.6), line.tax.sgst);
            assert_eq!(encode::money_from_sql(row.7), line.tax.igst);
            assert_eq!(encode::money_from_sql(row.8), line.gross_including_tax);
            assert_eq!(
                encode::tax_rate_from_sql(row.9, "bill_lines.rate_bp").expect("in range"),
                line.rate
            );
            assert_eq!(
                encode::tax_treatment_from_sql(&row.10).expect("known"),
                line.treatment
            );
        }

        // Both charge bases.
        let mut stmt = conn.prepare(
            "SELECT kind, name, basis, basis_value, amount, gross_including_tax
               FROM bill_charges WHERE order_id = 'ord_1' ORDER BY seq",
        )?;
        let charges: Vec<(String, String, String, i64, i64, i64)> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?
            .collect::<Result<_, _>>()?;
        assert_eq!(charges.len(), bill.charges.len());
        for (row, charge) in charges.iter().zip(&bill.charges) {
            assert_eq!(
                encode::charge_kind_from_sql(&row.0, &row.1).expect("known"),
                charge.kind
            );
            assert_eq!(
                encode::charge_basis_from_sql(&row.2, row.3).expect("known"),
                charge.basis
            );
            assert_eq!(encode::money_from_sql(row.4), charge.amount);
            assert_eq!(encode::money_from_sql(row.5), charge.gross_including_tax);
        }

        // Both payload-carrying payment modes, and the khata flag that audit
        // B12 says must not be a mode.
        let mut stmt = conn.prepare(
            "SELECT mode, customer_id, mode_label, amount, tip, settles_khata
               FROM payments WHERE order_id = 'ord_1' ORDER BY seq",
        )?;
        let payments: Vec<PaymentRow> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?
            .collect::<Result<_, _>>()?;
        assert_eq!(payments.len(), settlement.payments().len());
        for (row, payment) in payments.iter().zip(settlement.payments()) {
            assert_eq!(
                encode::payment_mode_from_sql(&row.0, row.1.as_deref(), row.2.as_deref())
                    .expect("known"),
                payment.mode
            );
            assert_eq!(encode::money_from_sql(row.3), payment.amount);
            assert_eq!(
                encode::bool_from_sql(row.5, "payments.settles_khata").expect("0 or 1"),
                payment.settles_khata
            );
        }
        assert_eq!(
            encode::money_from_sql(payments.iter().map(|p| p.4).sum()),
            settlement.tip()
        );
        Ok(())
    })
    .expect("round trip");
}

/// T22. The snapshot is frozen: renaming or deleting an item does not change a
/// bill that was already printed.
///
/// Audit Part 4's one compliment to v1: *"each order stores its items as a
/// frozen snapshot… If you rename an item or change its price tomorrow, old
/// bills do not change. This is correct and legally safer."*
#[test]
fn t22_the_item_snapshot_is_frozen() {
    let scratch = Scratch::new("t22");
    let db = scratch.open();
    let day = mb_core::BusinessDay::from_ymd(2026, 8, 3);
    let (bill, settlement) = build_a_real_bill();

    db.transaction(|tx| {
        tx.execute_batch(common::STAFF_SQL)?;
        tx.execute_batch(common::FLOOR_SQL)?;
        tx.execute_batch(common::MENU_SQL)?;
        tx.execute_batch(
            "INSERT INTO customers (id, outlet_id, name, created_at, updated_at)
             VALUES ('cus_1', 'outlet_default', 'Regular', 0, 0);",
        )?;
        write_settled_order(tx, "ord_1", day, &bill, &settlement)
    })
    .expect("write the bill");

    let before: (String, i64, i64) = db
        .read(|conn| {
            conn.query_row(
                "SELECT name, unit_price, (SELECT grand_total FROM bills WHERE order_id='ord_1')
                   FROM order_lines WHERE order_id = 'ord_1' AND seq = 0",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(Into::into)
        })
        .expect("read the line");

    // Tomorrow the owner renames the dish and puts the price up.
    db.transaction(|tx| {
        tx.execute(
            "UPDATE items SET name = 'Special Masala Dosa', unit_price = 15000
              WHERE id = 'itm_dosa'",
            [],
        )?;
        Ok(())
    })
    .expect("edit the menu");

    let after: (String, i64, i64) = db
        .read(|conn| {
            conn.query_row(
                "SELECT name, unit_price, (SELECT grand_total FROM bills WHERE order_id='ord_1')
                   FROM order_lines WHERE order_id = 'ord_1' AND seq = 0",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(Into::into)
        })
        .expect("read the line again");
    assert_eq!(before, after, "editing the menu changed a printed bill");

    // And an item that has been SOLD cannot be deleted at all.
    //
    // The snapshot protects the printed bill from a rename or a reprice, but it
    // does not make the sale disappear: `order_lines.item_id` is what
    // item-wise sales (scope 10.2) reads a year later, and `ON DELETE SET NULL`
    // would quietly turn last Diwali's best seller into an unattributed row.
    // So an item that has ever been billed is history, exactly like a staff
    // member who has left (scope 9.15). P13 removes it from the menu with
    // `is_available`, not with a DELETE.
    let sold = db.transaction(|tx| {
        tx.execute("DELETE FROM items WHERE id = 'itm_dosa'", [])
            .map_err(Into::into)
    });
    assert!(sold.is_err(), "an item that has been sold was deleted");

    // An item that was typed in by mistake and never sold deletes fine — the
    // constraint bites on history, not on housekeeping.
    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO items (id, outlet_id, name, unit_price, created_at, updated_at)
             VALUES ('itm_typo', 'outlet_default', 'Masalaa Dosa', 12000, 0, 0)",
            [],
        )?;
        tx.execute("DELETE FROM items WHERE id = 'itm_typo'", [])?;
        Ok(())
    })
    .expect("an unsold item must be deletable");

    let after_all: (String, i64, i64) = db
        .read(|conn| {
            conn.query_row(
                "SELECT name, unit_price, (SELECT grand_total FROM bills WHERE order_id='ord_1')
                   FROM order_lines WHERE order_id = 'ord_1' AND seq = 0",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(Into::into)
        })
        .expect("the bill must still read");
    assert_eq!(before, after_all, "the printed bill changed");
}

/// T23. A repeated bill number is refused by the database.
///
/// mb-core's `Counter::claim` saturates at the top of its range rather than
/// wrapping, and its comment says the repeat "is caught by P04's uniqueness
/// constraint rather than silently reused". A promise in a comment in another
/// crate is not a constraint. This is the constraint.
#[test]
fn t23_a_repeated_bill_number_is_refused() {
    let db = Scratch::new("t23").open();

    db.transaction(|tx| {
        tx.execute_batch(common::STAFF_SQL)?;
        tx.execute_batch(common::FLOOR_SQL)?;
        tx.execute_batch(
            "INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
                                 created_by, order_type, table_id, token_value, token_formatted,
                                 bill_number_value, bill_number_formatted)
             VALUES ('ord_1', 'outlet_default', 'terminal_default', 'open', 20669, 1,
                     'staff_1', 'dine_in', 'tbl_1', 1, '1', 42, '0042');",
        )?;
        Ok(())
    })
    .expect("first order");

    let duplicate = db.transaction(|tx| {
        tx.execute_batch(
            "INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
                                 created_by, order_type, table_id, token_value, token_formatted,
                                 bill_number_value, bill_number_formatted)
             VALUES ('ord_2', 'outlet_default', 'terminal_default', 'open', 20669, 2,
                     'staff_1', 'dine_in', 'tbl_1', 2, '2', 42, '0042');",
        )
        .map_err(Into::into)
    });
    assert!(duplicate.is_err(), "a bill number was reused");

    // A token, by contrast, is unique only within its day — it resets.
    db.transaction(|tx| {
        tx.execute_batch(
            "INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
                                 created_by, order_type, table_id, token_value, token_formatted,
                                 bill_number_value, bill_number_formatted)
             VALUES ('ord_3', 'outlet_default', 'terminal_default', 'open', 20670, 3,
                     'staff_1', 'dine_in', 'tbl_1', 1, '1', 43, '0043');",
        )?;
        Ok(())
    })
    .expect("tomorrow's token 1 must be allowed");
}

/// The CHECK constraints that carry mb-core's type-level rules down to the
/// disk: a cancelled order has a reason, a dine-in order past draft has a
/// table, and a khata payment names its customer.
#[test]
fn the_orders_table_refuses_what_mb_core_refuses() {
    let db = Scratch::new("checks").open();
    db.transaction(|tx| {
        tx.execute_batch(common::STAFF_SQL)?;
        tx.execute_batch(common::FLOOR_SQL)?;
        Ok(())
    })
    .expect("seed");

    let base = "INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
                                    created_by, order_type, table_id, token_value, token_formatted,
                                    bill_number_value, bill_number_formatted, cancel_reason)";

    // A cancelled order with no reason (audit B6 says the reason is compulsory).
    let no_reason = db.transaction(|tx| {
        tx.execute_batch(&format!(
            "{base} VALUES ('o1','outlet_default','terminal_default','cancelled',20669,1,
                            'staff_1','parcel',NULL,1,'1',1,'0001',NULL);"
        ))
        .map_err(Into::into)
    });
    assert!(no_reason.is_err(), "a cancelled order was stored without a reason");

    // A whitespace-only reason is not a reason.
    let blank_reason = db.transaction(|tx| {
        tx.execute_batch(&format!(
            "{base} VALUES ('o2','outlet_default','terminal_default','cancelled',20669,1,
                            'staff_1','parcel',NULL,1,'1',1,'0001','   ');"
        ))
        .map_err(Into::into)
    });
    assert!(blank_reason.is_err(), "'   ' was accepted as a reason");

    // A dine-in order past draft with no table (audit 2.3).
    let no_table = db.transaction(|tx| {
        tx.execute_batch(
            "INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
                                 created_by, order_type, token_value, token_formatted,
                                 bill_number_value, bill_number_formatted)
             VALUES ('o3','outlet_default','terminal_default','open',20669,1,
                     'staff_1','dine_in',1,'1',1,'0001');",
        )
        .map_err(Into::into)
    });
    assert!(no_table.is_err(), "a dine-in order was opened without a table");

    // But a DRAFT dine-in order may sit there without one while the cashier
    // is still typing.
    db.transaction(|tx| {
        tx.execute_batch(
            "INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
                                 created_by, order_type)
             VALUES ('o4','outlet_default','terminal_default','draft',20669,1,
                     'staff_1','dine_in');",
        )?;
        Ok(())
    })
    .expect("a draft may be incomplete");
}

// ---------------------------------------------------------------------------
// Fixtures.
//
// These build rows with hand-written SQL and the encoders from `encode.rs`, and
// that is the P04/P05 seam working as intended: P04 owns the value mapping, so
// its tests can write a row; P05 owns `save_settled_order`, so this is not it.
// ---------------------------------------------------------------------------

/// A bill with everything on it: three rates including a non-GST line, a line
/// discount, a bill discount, a percentage charge and a flat one, round-off on.
fn build_a_real_bill() -> (Bill, Settlement) {
    let mut cart = Cart::new();

    let dosa = ItemSnapshot::new(ItemId::new("itm_dosa"), "Masala Dosa", Money::from_paise(12_000), TaxRate::GST_5)
        .with_hsn("2106");
    let water = ItemSnapshot::new(ItemId::new("itm_water"), "Water", Money::from_paise(2_000), TaxRate::GST_18)
        .with_treatment(TaxTreatment::Inclusive)
        .with_hsn("2201");
    let beer = ItemSnapshot::new(ItemId::new("itm_beer"), "Beer", Money::from_paise(22_000), TaxRate::ZERO)
        .with_treatment(TaxTreatment::NonGst);

    cart.add(dosa, Qty::from_whole(2).expect("qty"), Some("extra crispy".to_owned()), vec![])
        .expect("add");
    cart.add(water, Qty::from_whole(3).expect("qty"), None, vec![])
        .expect("add");
    cart.add(beer, Qty::from_whole(1).expect("qty"), None, vec![])
        .expect("add");

    // A line discount that gets capped, so `was_capped` has something to say.
    cart.set_line_discount(
        1,
        Some(
            DiscountEntry::new(Discount::amount(Money::from_paise(100_000)).expect("valid"))
                .with_reason("goodwill"),
        ),
    )
    .expect("line discount");

    let charges = [
        Charge::percent(ChargeKind::Service, "Service Charge", 500, TaxRate::GST_5),
        Charge::flat(ChargeKind::Packing, "Packing", Money::from_paise(1_500), TaxRate::GST_18),
    ];

    let bill = compute_bill(
        BillInput::new(&cart)
            .with_bill_discount(DiscountEntry::new(
                Discount::percent_bp(500).expect("valid"),
            ))
            .with_charges(&charges)
            .with_place_of_supply(PlaceOfSupply::Intra)
            .with_order_type(OrderType::DineIn)
            .with_rounding(RoundingMode::NearestRupee),
    )
    .expect("compute");

    // Split payment: part cash, part khata, part something the shop calls
    // "Sodexo". The khata one carries a customer; the other one a label.
    let mut settlement = Settlement::with_tip(Money::from_paise(2_000)).expect("tip");
    let half = Money::from_paise(bill.grand_total.paise() / 2);
    let quarter = Money::from_paise(bill.grand_total.paise() / 4);
    let rest = Money::from_paise(bill.grand_total.paise() - half.paise() - quarter.paise());
    settlement
        .add(Payment::new(PaymentMode::Card, half).expect("payment"))
        .expect("add");
    settlement
        .add(
            Payment::new(PaymentMode::Other("Sodexo".to_owned()), quarter)
                .expect("payment")
                .with_reference("SDX-9911"),
        )
        .expect("add");
    settlement
        .add(
            Payment::new(PaymentMode::Credit(CustomerId::new("cus_1")), rest)
                .expect("payment")
                .settling_khata(),
        )
        .expect("add");

    (bill, settlement)
}

/// Writes a settled order: the header, the typed lines, the computed lines, the
/// charges, the rate summary, the payments and the kitchen ledger.
fn write_settled_order(
    tx: &Transaction<'_>,
    order_id: &str,
    day: mb_core::BusinessDay,
    bill: &Bill,
    settlement: &Settlement,
) -> Result<(), mb_db::DbError> {
    let claimed = numbering::claim(tx, common::OUTLET, common::TERMINAL, CounterKind::Bill, day)?;
    let token = numbering::claim(tx, common::OUTLET, common::TERMINAL, CounterKind::Token, day)?;
    let day_sql = encode::business_day_to_sql(day);

    tx.execute(
        "INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
                             created_by, order_type, table_id, token_value, token_formatted,
                             bill_number_value, bill_number_formatted, settled_at, settled_by)
         VALUES (?1, ?2, ?3, 'settled', ?4, ?5, 'staff_1', ?6, 'tbl_1', ?7, ?8, ?9, ?10, ?5, 'staff_1')",
        rusqlite::params![
            order_id,
            common::OUTLET,
            common::TERMINAL,
            day_sql,
            encode::timestamp_to_sql(mb_core::Timestamp::from_millis(1_770_000_000_000)),
            encode::order_type_to_sql(bill.order_type),
            i64::try_from(token.value).unwrap_or(i64::MAX),
            token.formatted,
            i64::try_from(claimed.value).unwrap_or(i64::MAX),
            claimed.formatted,
        ],
    )?;

    tx.execute(
        "INSERT INTO bills (order_id, subtotal, total_line_discount, total_bill_discount,
                            total_discount, total_charges, was_bill_discount_capped,
                            bill_discount_kind, bill_discount_value,
                            total_taxable, total_cgst, total_sgst, total_igst,
                            non_gst_value, exempt_value, round_off, grand_total,
                            place_of_supply, rounding_mode, computed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'percent', 500, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16, ?17, ?18)",
        rusqlite::params![
            order_id,
            encode::money_to_sql(bill.subtotal),
            encode::money_to_sql(bill.total_line_discount),
            encode::money_to_sql(bill.total_bill_discount),
            encode::money_to_sql(bill.total_discount),
            encode::money_to_sql(bill.total_charges),
            encode::bool_to_sql(bill.bill_discount_capped),
            encode::money_to_sql(bill.total_taxable),
            encode::money_to_sql(bill.total_tax.cgst),
            encode::money_to_sql(bill.total_tax.sgst),
            encode::money_to_sql(bill.total_tax.igst),
            encode::money_to_sql(bill.non_gst_value),
            encode::money_to_sql(bill.exempt_value),
            encode::money_to_sql(bill.round_off),
            encode::money_to_sql(bill.grand_total),
            encode::place_of_supply_to_sql(bill.place_of_supply),
            encode::rounding_mode_to_sql(bill.rounding),
            encode::timestamp_to_sql(mb_core::Timestamp::from_millis(1_770_000_000_000)),
        ],
    )?;

    for (seq, line) in bill.lines.iter().enumerate() {
        let line_id = format!("{order_id}_ln_{seq}");
        let seq = i64::try_from(seq).unwrap_or(i64::MAX);
        tx.execute(
            "INSERT INTO order_lines (id, order_id, seq, item_id, name, unit_price, tax_rate_bp,
                                      tax_treatment, hsn, qty, note, was_discount_capped)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0)",
            rusqlite::params![
                line_id,
                order_id,
                seq,
                line.snapshot.item_id.as_str(),
                line.snapshot.name,
                encode::money_to_sql(line.snapshot.unit_price),
                encode::tax_rate_to_sql(line.snapshot.tax_rate),
                encode::tax_treatment_to_sql(line.snapshot.tax_treatment),
                line.snapshot.hsn,
                encode::qty_to_sql(line.qty),
                line.note,
            ],
        )?;
        tx.execute(
            "INSERT INTO bill_lines (order_line_id, order_id, gross, line_discount,
                                     bill_discount_share, net, taxable, cgst, sgst, igst,
                                     gross_including_tax, rate_bp, treatment)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                line_id,
                order_id,
                encode::money_to_sql(line.gross),
                encode::money_to_sql(line.line_discount),
                encode::money_to_sql(line.bill_discount_share),
                encode::money_to_sql(line.net),
                encode::money_to_sql(line.taxable),
                encode::money_to_sql(line.tax.cgst),
                encode::money_to_sql(line.tax.sgst),
                encode::money_to_sql(line.tax.igst),
                encode::money_to_sql(line.gross_including_tax),
                encode::tax_rate_to_sql(line.rate),
                encode::tax_treatment_to_sql(line.treatment),
            ],
        )?;
    }

    for (seq, charge) in bill.charges.iter().enumerate() {
        let (basis, basis_value) = encode::charge_basis_to_sql(charge.basis);
        tx.execute(
            "INSERT INTO bill_charges (id, order_id, seq, kind, name, basis, basis_value, amount,
                                       taxable, cgst, sgst, igst, gross_including_tax, rate_bp,
                                       treatment)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                format!("{order_id}_chg_{seq}"),
                order_id,
                i64::try_from(seq).unwrap_or(i64::MAX),
                encode::charge_kind_to_sql(&charge.kind),
                charge.name,
                basis,
                basis_value,
                encode::money_to_sql(charge.amount),
                encode::money_to_sql(charge.taxable),
                encode::money_to_sql(charge.tax.cgst),
                encode::money_to_sql(charge.tax.sgst),
                encode::money_to_sql(charge.tax.igst),
                encode::money_to_sql(charge.gross_including_tax),
                encode::tax_rate_to_sql(charge.rate),
                encode::tax_treatment_to_sql(charge.treatment),
            ],
        )?;
    }

    for row in bill.summary.rows() {
        tx.execute(
            "INSERT INTO bill_tax_rows (order_id, rate_bp, taxable, cgst, sgst, igst)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                order_id,
                encode::tax_rate_to_sql(row.rate),
                encode::money_to_sql(row.taxable),
                encode::money_to_sql(row.tax.cgst),
                encode::money_to_sql(row.tax.sgst),
                encode::money_to_sql(row.tax.igst),
            ],
        )?;
    }

    for (seq, payment) in settlement.payments().iter().enumerate() {
        let cols = encode::payment_mode_to_sql(&payment.mode);
        // The tip belongs to the settlement, not to one payment; it is put on
        // the first row so the settlement's total is recoverable.
        let tip = if seq == 0 { settlement.tip() } else { Money::ZERO };
        tx.execute(
            "INSERT INTO payments (id, order_id, seq, mode, customer_id, mode_label, amount, tip,
                                   reference, settles_khata, received_at, received_by, business_day)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'staff_1', ?12)",
            rusqlite::params![
                format!("{order_id}_pay_{seq}"),
                order_id,
                i64::try_from(seq).unwrap_or(i64::MAX),
                cols.mode,
                cols.customer_id,
                cols.mode_label,
                encode::money_to_sql(payment.amount),
                encode::money_to_sql(tip),
                payment.reference,
                encode::bool_to_sql(payment.settles_khata),
                encode::timestamp_to_sql(mb_core::Timestamp::from_millis(1_770_000_000_000)),
                day_sql,
            ],
        )?;
    }

    // The kitchen was told about everything on the order.
    for line in &bill.lines {
        let identity = LineIdentity {
            item_id: line.snapshot.item_id.clone(),
            note: line.note.clone(),
            modifier_ids: line.modifiers.iter().map(|m| m.modifier_id.clone()).collect::<Vec<ModifierId>>(),
        };
        tx.execute(
            "INSERT INTO kitchen_ledger (order_id, identity_key, item_id, note, qty_told, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (order_id, identity_key)
             DO UPDATE SET qty_told = qty_told + excluded.qty_told",
            rusqlite::params![
                order_id,
                encode::line_identity_key(&identity),
                identity.item_id.as_str(),
                identity.note,
                encode::qty_to_sql(line.qty),
                encode::timestamp_to_sql(mb_core::Timestamp::from_millis(1_770_000_000_000)),
            ],
        )?;
    }

    Ok(())
}
