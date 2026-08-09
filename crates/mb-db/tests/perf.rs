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

/// Budgets **R5** and **R6** — P05.
///
/// `PERFORMANCE.md` §2.3: *R5, local backup of a one-year database, 20 s /
/// 60 s. R6, restore and verify that backup, 60 s / 180 s.*
///
/// Building a real 317 MB database in a test would make the suite unrunnable,
/// so this measures **throughput** on a realistic sample and projects to the
/// 400 MB M5 budget — the same approach M5 itself takes above, and honest about
/// being a projection rather than the thing.
#[test]
fn r5_and_r6_backup_and_restore_fit_their_budgets() {
    const SAMPLE: u64 = 400;
    const PROJECT_TO_BYTES: f64 = 400.0 * 1024.0 * 1024.0;

    let scratch = Scratch::new("r5r6");
    let dir = scratch.db_path().with_file_name("backups");
    let backup_path = dir.join("measured.db");

    {
        let db = scratch.open();
        seed_a_shop(&db);
        for n in 0..SAMPLE {
            write_one_bill(&db, n);
        }
        db.checkpoint().expect("checkpoint");

        let live_bytes = file_bytes(&scratch);

        let started = Instant::now();
        mb_db::backup::take(&db, &backup_path, "perf").expect("take");
        let backup_secs = started.elapsed().as_secs_f64();

        let started = Instant::now();
        let report = mb_db::backup::verify(&backup_path).expect("verify");
        let verify_secs = started.elapsed().as_secs_f64();
        assert!(report.is_ok(), "{}", report.summary());

        let scale = PROJECT_TO_BYTES / mb(live_bytes).max(0.001) / (1024.0 * 1024.0);

        println!("\n--- R5: back up a one-year database ---");
        println!("  sample on disk       {:.2} MB", mb(live_bytes));
        println!("  backed up in         {:.3} s", backup_secs);
        println!("  verified in          {:.3} s", verify_secs);
        println!("  projected at 400 MB  {:.1} s", backup_secs * scale);
        println!("  budget / ceiling     20 s / 60 s");

        if !cfg!(debug_assertions) {
            let projected = backup_secs * scale;
            assert!(
                projected < 60.0,
                "a one-year backup projects to {projected:.1} s, over R5's 60 s ceiling"
            );
        }
    }

    // The restore runs with no handle on the database, which is the only way it
    // is allowed to run — see `backup::restore`.
    let started = Instant::now();
    let report = mb_db::backup::restore(&backup_path, &scratch.db_path()).expect("restore");
    let restore_secs = started.elapsed().as_secs_f64();
    assert!(!report.rolled_back, "the measured restore rolled back");

    let live_bytes = file_bytes(&scratch);
    let scale = PROJECT_TO_BYTES / mb(live_bytes).max(0.001) / (1024.0 * 1024.0);

    println!("\n--- R6: restore and verify ---");
    println!("  restored in          {:.3} s", restore_secs);
    println!("  projected at 400 MB  {:.1} s", restore_secs * scale);
    println!("  budget / ceiling     60 s / 180 s\n");

    if cfg!(debug_assertions) {
        return;
    }
    let projected = restore_secs * scale;
    assert!(
        projected < 180.0,
        "a one-year restore projects to {projected:.1} s, over R6's 180 s ceiling"
    );
}

/// Budget **B5** — a whole settle, end to end, measured at p95.
///
/// *"Settle a bill: number claimed + written durably to disk — 150 ms, ceiling
/// 400 ms."* One transaction, one fsync, and the number claimed inside it.
///
/// **p95, not the mean.** B5 is written as a p95 and a mean would hide the one
/// flush in twenty that took 300 ms, which is the one the cashier notices.
#[test]
fn b5_a_whole_settle_is_one_durable_write() {
    const SETTLES: usize = 200;

    let scratch = Scratch::new("b5");
    let db = scratch.open();
    seed_a_shop(&db);

    let mut samples = Vec::with_capacity(SETTLES);
    for n in 0..SETTLES {
        let started = Instant::now();
        write_one_bill(&db, u64::try_from(n).expect("small"));
        samples.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    samples.sort_by(f64::total_cmp);

    let p95 = samples[samples.len() * 95 / 100];
    let median = samples[samples.len() / 2];
    let worst = samples[samples.len() - 1];

    println!("\n--- B5: settle a bill, one transaction, synchronous = FULL ---");
    println!("  settles              {SETTLES}");
    println!("  median               {median:.2} ms");
    println!("  p95                  {p95:.2} ms");
    println!("  worst                {worst:.2} ms");
    println!("  budget / ceiling     150 ms / 400 ms");
    println!(
        "  NOTE: an SSD. The reference machine is a 5400 rpm HDD where a single\n\
         \x20       fsync is 5-15 ms, so expect the p95 there to be tens of\n\
         \x20       milliseconds rather than this. Re-measure at P30.\n"
    );

    if cfg!(debug_assertions) {
        return;
    }
    assert!(
        p95 < 400.0,
        "the p95 settle took {p95:.1} ms, over B5's 400 ms ceiling"
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
             VALUES ('exc_perf', 'outlet_default', 'Gas')",
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
        "INSERT INTO payments (id, order_id, seq, mode, amount, tip, settles_credit,
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
            "INSERT INTO payments (id, order_id, seq, mode, amount, tip, settles_credit,
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

    // An expense every twentieth bill and a credit repayment every fiftieth, so
    // those tables are not zero in the projection.
    if n.is_multiple_of(20) {
        tx.execute(
            "INSERT INTO expenses (id, outlet_id, category_id, description, amount, mode,
                                   paid_at, paid_by, business_day)
             VALUES (?1, 'outlet_default', 'exc_gas', 'Gas cylinder', 180000, 'cash', ?2,
                     'staff_1', ?3)",
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

/// **M5's P11 addendum — what the audit trail costs.**
///
/// `m5_a_year_of_trading_fits_the_budget` above weighs bills, and it was
/// written before `audit_log` had anything in it. P11 fills that table on
/// every login, void, discount, price change and setting change, and adds
/// `seq`, `prev_hash` and `hash` to every row — 128 bytes of hex on top of the
/// content.
///
/// D35 asked exactly this question about the print spool and the answer was to
/// stop keeping one ("a spool, not a log"). It cannot be the answer here: an
/// audit trail that forgets is not an audit trail. So the number has to be
/// measured and it has to fit — and if it ever stops fitting, the fix is a
/// written retention rule executed by P18's day close, never a quieter trail.
#[test]
fn m5_the_audit_trail_fits_beside_a_year_of_bills() {
    use mb_auth::AuditEntry;

    /// A busy shop: logins, voids, discounts, price changes, day closes.
    const ACTIONS_PER_DAY: u64 = 200;
    const SAMPLE: u64 = 5_000;

    let scratch = Scratch::new("m5_audit");
    let db = scratch.open();

    // The main file PLUS its WAL: in WAL mode the rows are not in the main
    // file until a checkpoint, and the first version of this measured 0 bytes
    // per row because of it.
    let empty = file_bytes(&scratch);

    db.transaction(|tx| {
        tx.execute(
            "INSERT INTO staff (id, outlet_id, name, status, created_at, updated_at)
             VALUES ('staff_1', 'outlet_default', 'Cashier', 'active', 0, 0)",
            [],
        )?;
        Ok(())
    })
    .expect("somebody to blame");

    let start = Instant::now();
    db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        for n in 0..SAMPLE {
            // The widest realistic shape: a price change, which is the only
            // kind that carries both a before and an after.
            let entry = AuditEntry::new(
                Timestamp::from_millis(1_770_000_000_000 + i64::try_from(n).unwrap_or(0)),
                BusinessDay::from_days_since_epoch(20_600),
                Some(mb_core::StaffId::new("staff_1")),
                mb_auth::audit::action::PRICE_CHANGED,
                "menu_item",
            )
            .about(format!("itm_{n}"))
            .changed(
                serde_json::json!({ "unit_price": 12_000, "name": "Masala Dosa" }),
                serde_json::json!({ "unit_price": 9_000, "name": "Masala Dosa" }),
            );
            repos.audit().append("outlet_default", &entry)?;
        }
        Ok(())
    })
    .expect("write the history");
    let wrote_in = start.elapsed();

    let full = file_bytes(&scratch);
    let per_row = full.saturating_sub(empty) / SAMPLE;
    let per_year = per_row * ACTIONS_PER_DAY * 365;

    println!("\n--- M5 addendum: the audit trail (P11) ---");
    println!("  rows written         {SAMPLE}");
    println!("  per row              {per_row} bytes");
    println!("  at {ACTIONS_PER_DAY}/day, one year  {} MB", per_year / (1024 * 1024));
    println!("  M5 headroom          400 MB budget, bills project to 318 MB");
    println!("  wrote in             {wrote_in:.2?}");

    // The whole M5 budget is 400 MB and the bills project to 318. The trail has
    // to live in what is left, with room to spare — 40 MB is half the headroom.
    #[cfg(not(debug_assertions))]
    assert!(
        per_year < 40 * 1024 * 1024,
        "the audit trail projects to {per_year} bytes a year, which eats M5's headroom — \
         write a retention rule into PERFORMANCE.md and give it to P18's day close"
    );
}

// ---------------------------------------------------------------------------
// R1, R2, R3 — the report budgets (P18)
// ---------------------------------------------------------------------------

/// **R1 500 ms for today, R2 2.5 s for a year, R3 1.5 s for the day close.**
///
/// The reports are the one part of this product a shop runs *while* the
/// counter is billing, so `PERFORMANCE.md` §5's rules R1–R3 are what make the
/// numbers hold: they read on a reader connection, they never take the writer,
/// and every one of them is filtered by `idx_orders_day`.
///
/// **Three readings, not one.** The master plan records a single B4 run taken
/// straight after a build that read 23.3 µs and looked like a 55% regression;
/// three clean runs gave 15.5–15.8. A stopwatch started on a cold cache is
/// measuring the cache.
#[test]
fn r1_r2_r3_the_report_budgets() {
    use mb_db::repo::reports::{Period, SalesBy};

    let scratch = Scratch::new("perf-reports");
    let db = scratch.open();

    seed_a_shop(&db);
    // The same sample the M5 test uses, spread across a year, so the day index
    // has real cardinality rather than one hot page.
    for n in 0..SAMPLE_BILLS {
        write_one_bill(&db, n);
    }
    db.checkpoint().expect("checkpoint");

    let year = Period::new(
        BusinessDay::from_days_since_epoch(20_000),
        BusinessDay::from_days_since_epoch(20_000 + 365),
    );
    let today = Period::one_day(BusinessDay::from_days_since_epoch(20_100));

    // The projection: the sample is SAMPLE_BILLS, a year is BILLS_PER_YEAR.
    // Reporting the measured time AND the projection, because a 2,000-bill
    // database is not a year and pretending otherwise is the mistake M5's own
    // comment warns about.
    // Both are small round constants; the cast is exact and the lint is about
    // values this file does not have.
    #[allow(clippy::cast_precision_loss, reason = "2,000 and 75,000 are exact in f64")]
    let scale = BILLS_PER_YEAR as f64 / SAMPLE_BILLS as f64;

    let mut day_best = f64::MAX;
    let mut year_best = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        db.transaction(|tx| {
            let reports = mb_db::Repos::new(tx).reports();
            reports.sales_by(common::OUTLET, today, SalesBy::Item)?;
            reports.tax_by_rate(common::OUTLET, today)?;
            Ok(())
        })
        .expect("today's report");
        day_best = day_best.min(start.elapsed().as_secs_f64() * 1000.0);

        let start = Instant::now();
        db.transaction(|tx| {
            let reports = mb_db::Repos::new(tx).reports();
            reports.sales_by(common::OUTLET, year, SalesBy::Day)?;
            reports.sales_by(common::OUTLET, year, SalesBy::Item)?;
            reports.tax_by_rate(common::OUTLET, year)?;
            reports.tax_by_hsn(common::OUTLET, year)?;
            Ok(())
        })
        .expect("the year's report");
        year_best = year_best.min(start.elapsed().as_secs_f64() * 1000.0);
    }
    let projected_year = year_best * scale;

    println!("\n--- R1 / R2: the report budgets (P18) ---");
    println!("  bills in the sample  {SAMPLE_BILLS}");
    println!("  R1 today             {day_best:.1} ms   (budget 500, ceiling 1500)");
    println!("  R2 a year, measured  {year_best:.1} ms over {SAMPLE_BILLS} bills");
    println!("  R2 a year, projected {projected_year:.1} ms at {BILLS_PER_YEAR} bills");
    println!("     (budget 2500 ms, ceiling 6000)");

    // **R3 — the day close.** Everything the closing screen reads before a
    // person can type the first number: what the day took, what the drawer
    // should hold, and whether the day is already sealed. It is one paint of
    // one screen, so it is measured as one round trip rather than as three.
    let mut close_best = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        db.transaction(|tx| {
            let repos = mb_db::Repos::new(tx);
            repos.corrections().day_totals(common::OUTLET, today.from)?;
            repos.money().cash_position(common::OUTLET, today.from)?;
            repos.money().find_day_close(common::OUTLET, today.from)?;
            Ok(())
        })
        .expect("the day close");
        close_best = close_best.min(start.elapsed().as_secs_f64() * 1000.0);
    }
    println!("  R3 day close         {close_best:.1} ms   (budget 1500, ceiling 4000)");

    #[cfg(not(debug_assertions))]
    {
        assert!(day_best < 1_500.0, "R1's ceiling: today took {day_best:.1} ms");
        assert!(
            projected_year < 6_000.0,
            "R2's ceiling: a year projects to {projected_year:.1} ms"
        );
        assert!(
            close_best < 4_000.0,
            "R3's ceiling: the day close took {close_best:.1} ms"
        );
    }
}
