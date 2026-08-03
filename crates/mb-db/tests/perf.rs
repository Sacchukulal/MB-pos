//! Budget **M5** — the database after one year of trading — and the fsync
//! measurement that settles `docs/PERFORMANCE.md` §5 rule 2.
//!
//! `docs/PERFORMANCE.md` §2.4: *M5 — database size after one year (~75,000
//! bills): **400 MB**, ceiling **800 MB**. Prompt: P04.*
//!
//! A schema decides this on the day it is written. Measuring it after a year of
//! real bills is measuring something nobody can change any more, so this test
//! builds a realistic shop, weighs it, and projects.
//!
//! §3.1's rules, obeyed: assertions in release only; every run prints the
//! number; the assert is against the CEILING, not the budget; `std::time` and
//! `std::fs` only, no criterion.
//!
//! ```text
//! cargo test -p mb-db --release --test perf -- --nocapture
//! ```

// A measuring harness, not shipped code — the same exemption mb-core's
// tests/perf.rs takes, and for the same reason. `expect` here IS the assertion;
// bytes over bills and elapsed over commits ARE the measurement; and reporting
// a megabyte figure to two decimal places is the one place in this workspace
// where a float is the right answer. D7 is about the money path, and no money
// goes anywhere near this file.
#![allow(
    clippy::expect_used,
    clippy::integer_division,
    clippy::float_arithmetic,
    reason = "a stopwatch and a weighing scale, not the money path"
)]

mod common;

use std::time::Instant;

use common::Scratch;
use mb_core::{BusinessDay, Money, Qty, TaxRate, Timestamp};
use mb_db::numbering::{self, CounterKind};
use mb_db::{Db, DbConfig, Synchronous, encode};
use rusqlite::Transaction;

/// One year of trading, from `PERFORMANCE.md`.
const BILLS_PER_YEAR: u64 = 75_000;
/// The M5 ceiling in bytes. The budget is 400 MB; §3.1 rule 3 says the assert
/// is against the ceiling, so a slightly fat run does not turn into a red
/// build.
const M5_CEILING_BYTES: u64 = 800 * 1024 * 1024;

/// How many bills to actually write. Enough that page overhead and index
/// fan-out are represented; small enough to run in a normal test suite.
const SAMPLE_BILLS: u64 = 2_000;

/// A realistic bill: eight lines, two tax rates, 1.5 payments on average.
const LINES_PER_BILL: u64 = 8;

#[test]
fn m5_a_year_of_trading_fits_the_budget() {
    let scratch = Scratch::new("m5");
    let db = scratch.open();

    let started = Instant::now();
    seed_a_shop(&db);
    let write_bills = Instant::now();
    for n in 0..SAMPLE_BILLS {
        write_one_bill(&db, n);
    }
    let writing = write_bills.elapsed();

    // The WAL holds the most recent commits. Weighing the main file alone would
    // under-report, and checkpointing first is also what P05's backup will have
    // to do before it copies anything.
    db.checkpoint().expect("checkpoint");

    let bytes = file_bytes(&scratch);
    let per_bill = bytes / SAMPLE_BILLS;
    let projected = per_bill * BILLS_PER_YEAR;

    println!("\n--- M5: database size after one year ---");
    println!("  bills written        {SAMPLE_BILLS}");
    println!("  lines per bill       {LINES_PER_BILL}");
    println!("  measured on disk     {:.2} MB", mb(bytes));
    println!("  per bill             {per_bill} bytes");
    println!("  projected at 75,000  {:.0} MB", mb(projected));
    println!("  budget / ceiling     400 MB / 800 MB");
    println!(
        "  wrote in             {writing:.2?}  (setup + write {:.2?})",
        started.elapsed()
    );
    println!();

    if cfg!(debug_assertions) {
        println!("  (debug build - measured and printed, not asserted)\n");
        return;
    }
    assert!(
        projected < M5_CEILING_BYTES,
        "projected {:.0} MB for a year of bills, over the {:.0} MB ceiling. \
         PERFORMANCE.md 3.4 applies in order: fix it (narrower rows, fewer \
         indexes, an archive policy for order_events), then make it invisible, \
         then record an exception with a name against it. The feature is never \
         dropped.",
        mb(projected),
        mb(M5_CEILING_BYTES)
    );
}

/// The fsync question, measured rather than argued.
///
/// `docs/PERFORMANCE.md` §5 rule 2 says `synchronous = NORMAL`. This crate ships
/// `FULL`, because in WAL mode NORMAL does not fsync on commit and a power cut
/// therefore loses the last committed bills — and requirement 1 of the ten is
/// that a failure loses NOTHING.
///
/// The trade is one fsync per commit against budget **B5** (settle a bill:
/// number claimed and written durably, 150 ms budget / 400 ms ceiling), and §5
/// rule 1 already says a settle is one transaction, so it is one fsync and not
/// four. This prints both, so the choice is a number rather than a preference.
#[test]
fn synchronous_full_costs_one_fsync_and_b5_can_afford_it() {
    const COMMITS: u32 = 200;

    let full = measure_commits(Synchronous::Full, COMMITS);
    let normal = measure_commits(Synchronous::Normal, COMMITS);

    println!("\n--- commit cost, WAL, {COMMITS} settle-sized transactions ---");
    println!("  synchronous = FULL     {full:.3} ms per commit");
    println!("  synchronous = NORMAL   {normal:.3} ms per commit");
    println!("  difference             {:.3} ms", full - normal);
    println!("  budget B5, whole settle: 150 ms, ceiling 400 ms");
    println!(
        "  NOTE: measured on this machine's disk. PERFORMANCE.md section 1 puts\n\
         \x20       an fsync on the reference machine's 5400 rpm HDD at 5-15 ms;\n\
         \x20       an SSD will not show that, so treat the FULL figure here as\n\
         \x20       a lower bound and re-measure on counter hardware at P30.\n"
    );

    if cfg!(debug_assertions) {
        return;
    }
    // Deliberately loose: this catches "FULL made a settle impossible", not
    // "FULL is slower", which it obviously is.
    assert!(
        full < 400.0,
        "a commit under synchronous=FULL took {full:.1} ms, which does not fit \
         inside B5's 400 ms ceiling for an entire settle"
    );
}

// ---------------------------------------------------------------------------

fn mb(bytes: u64) -> f64 {
    // Reporting only.
    #[allow(clippy::cast_precision_loss)]
    let bytes = bytes as f64;
    bytes / (1024.0 * 1024.0)
}

/// The main file plus its WAL and shared-memory files: what a backup would
/// actually have to carry.
fn file_bytes(scratch: &Scratch) -> u64 {
    let mut total = 0;
    for suffix in ["", "-wal", "-shm"] {
        let mut path = scratch.db_path().into_os_string();
        path.push(suffix);
        if let Ok(meta) = std::fs::metadata(&path) {
            total += meta.len();
        }
    }
    total
}

fn measure_commits(synchronous: Synchronous, commits: u32) -> f64 {
    let scratch = Scratch::new(match synchronous {
        Synchronous::Full => "sync-full",
        Synchronous::Normal => "sync-normal",
    });
    let mut config = DbConfig::new(scratch.db_path());
    config.synchronous = synchronous;
    let db = Db::open(&config).expect("open");
    seed_a_shop(&db);

    let started = Instant::now();
    for n in 0..u64::from(commits) {
        write_one_bill(&db, n);
    }
    let elapsed = started.elapsed();

    elapsed.as_secs_f64() * 1_000.0 / f64::from(commits)
}

fn seed_a_shop(db: &Db) {
    db.transaction(|tx| {
        tx.execute_batch(common::STAFF_SQL)?;
        tx.execute_batch(common::FLOOR_SQL)?;
        tx.execute_batch(common::MENU_SQL)?;
        // A menu of a realistic size, so the item table is not a rounding error
        // and the indexes on it have something to hold.
        for n in 0..300 {
            tx.execute(
                "INSERT INTO items (id, outlet_id, category_id, name, unit_price, tax_rate_bp,
                                    tax_treatment, hsn, short_code, created_at, updated_at)
                 VALUES (?1, 'outlet_default', 'cat_food', ?2, ?3, 500, 'exclusive', '2106', ?4, 0, 0)",
                rusqlite::params![
                    format!("itm_{n:04}"),
                    format!("Menu Item Number {n}"),
                    12_000 + i64::from(n),
                    format!("{n:04}"),
                ],
            )?;
        }
        tx.execute(
            "INSERT INTO expense_categories (id, outlet_id, name)
             VALUES ('exc_1', 'outlet_default', 'Gas')",
            [],
        )?;
        for n in 0..200 {
            tx.execute(
                "INSERT INTO customers (id, outlet_id, name, phone, created_at, updated_at)
                 VALUES (?1, 'outlet_default', ?2, '9000000000', 0, 0)",
                rusqlite::params![format!("cus_{n:04}"), format!("Customer {n}")],
            )?;
        }
        Ok(())
    })
    .expect("seed the shop");
}

/// One settled bill, in ONE transaction — `PERFORMANCE.md` §5 rule 1, which is
/// also what makes the fsync measurement above mean anything.
fn write_one_bill(db: &Db, n: u64) {
    // Spread the bills over a year so the day indexes have real cardinality.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let day = BusinessDay::from_days_since_epoch(20_000 + ((n / 205) as i32));
    #[allow(clippy::cast_possible_wrap)]
    let at = Timestamp::from_millis(1_770_000_000_000 + (n as i64) * 60_000);
    let order_id = format!("ord_{n:07}");

    db.transaction(|tx| write_bill_rows(tx, &order_id, day, at, n))
        .expect("write a bill");
}

fn write_bill_rows(
    tx: &Transaction<'_>,
    order_id: &str,
    day: BusinessDay,
    at: Timestamp,
    n: u64,
) -> Result<(), mb_db::DbError> {
    let day_sql = encode::business_day_to_sql(day);
    let at_sql = encode::timestamp_to_sql(at);

    let bill_no = numbering::claim(tx, common::OUTLET, common::TERMINAL, CounterKind::Bill, day)?;
    let token = numbering::claim(tx, common::OUTLET, common::TERMINAL, CounterKind::Token, day)?;

    tx.execute(
        "INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
                             created_by, order_type, table_id, covers, customer_id,
                             token_value, token_formatted, bill_number_value,
                             bill_number_formatted, settled_at, settled_by)
         VALUES (?1, 'outlet_default', 'terminal_default', 'settled', ?2, ?3, 'staff_1',
                 'dine_in', 'tbl_1', 4, ?4, ?5, ?6, ?7, ?8, ?3, 'staff_1')",
        rusqlite::params![
            order_id,
            day_sql,
            at_sql,
            format!("cus_{:04}", n % 200),
            i64::try_from(token.value).unwrap_or(i64::MAX),
            token.formatted,
            i64::try_from(bill_no.value).unwrap_or(i64::MAX),
            bill_no.formatted,
        ],
    )?;

    let mut grand = 0_i64;
    for seq in 0..LINES_PER_BILL {
        let line_id = format!("{order_id}_l{seq}");
        let item = format!("itm_{:04}", (n + seq) % 300);
        let qty = Qty::from_whole(1 + i64::try_from(seq % 3).unwrap_or(0)).expect("qty");
        let unit = Money::from_paise(12_000 + i64::try_from(seq).unwrap_or(0) * 500);
        let gross = qty.extend(unit).expect("extend");
        let taxable = gross;
        let half = Money::from_paise(taxable.paise() * 5 / 200);
        let including = Money::from_paise(taxable.paise() + half.paise() * 2);
        grand += including.paise();

        tx.execute(
            "INSERT INTO order_lines (id, order_id, seq, item_id, name, unit_price, tax_rate_bp,
                                      tax_treatment, hsn, qty, note, was_discount_capped)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 500, 'exclusive', '2106', ?7, ?8, 0)",
            rusqlite::params![
                line_id,
                order_id,
                i64::try_from(seq).unwrap_or(0),
                item,
                format!("Menu Item Number {}", (n + seq) % 300),
                encode::money_to_sql(unit),
                encode::qty_to_sql(qty),
                if seq == 0 { Some("no onion") } else { None },
            ],
        )?;
        tx.execute(
            "INSERT INTO bill_lines (order_line_id, order_id, gross, line_discount,
                                     bill_discount_share, net, taxable, cgst, sgst, igst,
                                     gross_including_tax, rate_bp, treatment)
             VALUES (?1, ?2, ?3, 0, 0, ?3, ?4, ?5, ?5, 0, ?6, 500, 'exclusive')",
            rusqlite::params![
                line_id,
                order_id,
                encode::money_to_sql(gross),
                encode::money_to_sql(taxable),
                encode::money_to_sql(half),
                encode::money_to_sql(including),
            ],
        )?;
        tx.execute(
            "INSERT INTO kitchen_ledger (order_id, identity_key, item_id, note, qty_told, updated_at)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5)
             ON CONFLICT (order_id, identity_key) DO UPDATE SET qty_told = qty_told + excluded.qty_told",
            rusqlite::params![
                order_id,
                format!("{item}\u{1f}"),
                item,
                encode::qty_to_sql(qty),
                at_sql,
            ],
        )?;
    }

    tx.execute(
        "INSERT INTO bills (order_id, subtotal, total_line_discount, total_bill_discount,
                            total_discount, total_charges, was_bill_discount_capped,
                            total_taxable, total_cgst, total_sgst, total_igst,
                            non_gst_value, exempt_value, round_off, grand_total,
                            place_of_supply, rounding_mode, computed_at)
         VALUES (?1, ?2, 0, 0, 0, 0, 0, ?2, ?3, ?3, 0, 0, 0, 0, ?4, 'intra', 'nearest_rupee', ?5)",
        rusqlite::params![order_id, grand, grand * 5 / 210, grand, at_sql],
    )?;

    // Two rate rows, as a mixed-rate shop really has.
    for rate in [TaxRate::GST_5, TaxRate::GST_18] {
        tx.execute(
            "INSERT INTO bill_tax_rows (order_id, rate_bp, taxable, cgst, sgst, igst)
             VALUES (?1, ?2, ?3, ?4, ?4, 0)",
            rusqlite::params![
                order_id,
                encode::tax_rate_to_sql(rate),
                grand / 2,
                grand / 40,
            ],
        )?;
    }

    // Every bill has one payment; every other bill has a second (scope 1.15,
    // split payment).
    tx.execute(
        "INSERT INTO payments (id, order_id, seq, mode, amount, tip, settles_khata,
                               received_at, received_by, business_day)
         VALUES (?1, ?2, 0, 'cash', ?3, 0, 0, ?4, 'staff_1', ?5)",
        rusqlite::params![
            format!("{order_id}_p0"),
            order_id,
            grand / 2,
            at_sql,
            day_sql,
        ],
    )?;
    if n.is_multiple_of(2) {
        tx.execute(
            "INSERT INTO payments (id, order_id, seq, mode, amount, tip, settles_khata,
                                   received_at, received_by, business_day)
             VALUES (?1, ?2, 1, 'upi', ?3, 0, 0, ?4, 'staff_1', ?5)",
            rusqlite::params![
                format!("{order_id}_p1"),
                order_id,
                grand - grand / 2,
                at_sql,
                day_sql,
            ],
        )?;
    }

    // The trail, and the outbox entry that A1 / A2 / A3 exist for.
    tx.execute(
        "INSERT INTO order_events (id, order_id, at, business_day, event, staff_id)
         VALUES (?1, ?2, ?3, ?4, 'settled', 'staff_1')",
        rusqlite::params![format!("{order_id}_ev"), order_id, at_sql, day_sql],
    )?;
    tx.execute(
        "INSERT INTO sync_outbox (id, outlet_id, table_name, row_id, op, created_at)
         VALUES (?1, 'outlet_default', 'orders', ?2, 'upsert', ?3)",
        rusqlite::params![format!("{order_id}_ob"), order_id, at_sql],
    )?;

    // An expense every twentieth bill and a khata repayment every fiftieth, so
    // those tables are not zero in the projection.
    if n.is_multiple_of(20) {
        tx.execute(
            "INSERT INTO expenses (id, outlet_id, category_id, description, amount, is_cash,
                                   paid_at, paid_by, business_day)
             VALUES (?1, 'outlet_default', 'exc_1', 'Gas cylinder', 180000, 1, ?2, 'staff_1', ?3)",
            rusqlite::params![format!("exp_{n:07}"), at_sql, day_sql],
        )?;
    }
    if n.is_multiple_of(50) {
        tx.execute(
            "INSERT INTO customer_payments (id, outlet_id, customer_id, amount, mode,
                                            received_at, received_by, business_day)
             VALUES (?1, 'outlet_default', ?2, 50000, 'cash', ?3, 'staff_1', ?4)",
            rusqlite::params![
                format!("cpay_{n:07}"),
                format!("cus_{:04}", n % 200),
                at_sql,
                day_sql
            ],
        )?;
    }

    Ok(())
}
