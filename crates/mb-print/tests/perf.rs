//! Budgets **P1, P2 and P3** — and they are new rows in the speed contract.
//!
//! P06 owned no budget in `docs/PERFORMANCE.md`, which was a gap: **B6** gives
//! P07 50 ms (ceiling 150) to get a kitchen ticket into the print queue, and
//! rendering the document happens inside that. A session that produces the
//! artifact another session is measured on should carry a number.
//!
//! | | what | budget | ceiling |
//! |---|---|---|---|
//! | P1 | Lay out and render a 40-line bill to text | 2 ms | 10 ms |
//! | P2 | Lay out and render the same bill to PDF | 5 ms | 20 ms |
//! | P3 | Lay out and render a 20-item kitchen ticket | 1 ms | 5 ms |
//!
//! The three ceilings together are a third of B6, which leaves P07 the rest for
//! the spooler.
//!
//! §3.1's rules: release-only assertions, print every run, assert the ceiling
//! rather than the budget, `std::time::Instant` only.
//!
//! ```text
//! cargo test -p mb-print --release --test perf -- --nocapture
//! ```

#![allow(
    clippy::expect_used,
    clippy::float_arithmetic,
    clippy::integer_division,
    reason = "a stopwatch, not the money path"
)]

mod common;

use std::time::Instant;

use common::Fixture;
use mb_core::{ItemId, LineIdentity, OrderType, Qty};
use mb_print::layout::layout;
use mb_print::paper::{Paper, PaperKind};
use mb_print::settings::KitchenSettings;
use mb_print::template::{Copy, KitchenContext, TicketKind, TicketLine, bill_document, kitchen_document};
use mb_print::{pdf, text};

const RUNS: u32 = 200;

/// A forty-line bill, because that is what P1 says and the anti-drift fixture
/// is deliberately only three.
///
/// Forty is a real table of eight ordering properly, and it is the same figure
/// budget B4 uses for `compute_bill`, so the two numbers are about the same
/// bill.
fn big_fixture() -> Fixture {
    use mb_core::{Cart, ItemSnapshot, Money, TaxRate, TaxTreatment};

    let mut cart = Cart::new();
    for n in 0..40_i64 {
        let treatment = match n % 4 {
            0 => TaxTreatment::Exclusive,
            1 => TaxTreatment::Inclusive,
            2 => TaxTreatment::Exempt,
            _ => TaxTreatment::NonGst,
        };
        let rate = match n % 3 {
            0 => TaxRate::GST_5,
            1 => TaxRate::GST_12,
            _ => TaxRate::GST_18,
        };
        let snapshot = ItemSnapshot::new(
            ItemId::new(format!("itm_{n:03}")),
            format!("Menu Item Number {n} With A Fairly Long Name"),
            Money::from_paise(12_000 + n * 137),
            rate,
        )
        .with_treatment(treatment)
        .with_hsn("2106");
        cart.add(
            snapshot,
            Qty::from_whole(1 + n % 3).expect("qty"),
            (n % 5 == 0).then(|| "no onion".to_owned()),
            vec![],
        )
        .expect("add");
    }

    let bill = common::bill(&cart);
    let settlement = common::settlement(&bill);
    let order = common::order(bill.clone(), settlement);
    let mut fixture = Fixture::new();
    fixture.bill = bill;
    fixture.order = order;
    fixture
}

#[test]
fn p1_and_p2_a_bill_renders_well_inside_b6() {
    let fixture = big_fixture();
    let ctx = fixture.context(Copy::Original);
    let paper = Paper::new(PaperKind::Mm80);

    // Warm the allocator so the first run is not the measurement.
    let _ = layout(&bill_document(paper, &ctx).expect("builds")).expect("lays out");

    let started = Instant::now();
    for _ in 0..RUNS {
        let doc = bill_document(paper, &ctx).expect("builds");
        let laid = layout(&doc).expect("lays out");
        let out = text::to_text(&laid);
        std::hint::black_box(out);
    }
    let p1 = started.elapsed().as_secs_f64() * 1_000.0 / f64::from(RUNS);

    let started = Instant::now();
    for _ in 0..RUNS {
        let doc = bill_document(paper, &ctx).expect("builds");
        let laid = layout(&doc).expect("lays out");
        let out = pdf::to_pdf(&laid);
        std::hint::black_box(out);
    }
    let p2 = started.elapsed().as_secs_f64() * 1_000.0 / f64::from(RUNS);

    println!("\n--- P1 / P2: rendering a bill ---");
    println!("  lines on the bill    {}", fixture.bill.lines.len());
    println!("  P1 build+lay+text    {p1:.3} ms   budget 2 ms, ceiling 10 ms");
    println!("  P2 build+lay+pdf     {p2:.3} ms   budget 5 ms, ceiling 20 ms");
    println!("  B6 (P07, whole queue hand-off) is 50 ms / 150 ms\n");

    if cfg!(debug_assertions) {
        return;
    }
    assert!(p1 < 10.0, "P1 took {p1:.2} ms, over the 10 ms ceiling");
    assert!(p2 < 20.0, "P2 took {p2:.2} ms, over the 20 ms ceiling");
}

#[test]
fn p3_a_kitchen_ticket_renders_in_almost_no_time() {
    let settings = KitchenSettings::default();
    let lines: Vec<TicketLine> = (0..20)
        .map(|n| {
            TicketLine::from_delta(
                &LineIdentity {
                    item_id: ItemId::new(format!("itm_{n}")),
                    note: (n % 3 == 0).then(|| "no onion".to_owned()),
                    modifier_ids: vec![],
                },
                Qty::from_whole(1 + i64::from(n % 3)).expect("qty"),
                format!("Menu Item Number {n}"),
                if n % 4 == 0 {
                    vec!["extra cheese".to_owned()]
                } else {
                    vec![]
                },
            )
        })
        .collect();

    let ctx = KitchenContext {
        kind: TicketKind::New,
        token: Some("42"),
        bill_number: Some("BIR/1207"),
        order_type: OrderType::DineIn,
        table: Some("6"),
        time: Some("21:40"),
        station: Some("TANDOOR"),
        lines: &lines,
        settings: &settings,
    };
    let paper = Paper::new(PaperKind::Mm80);

    let _ = kitchen_document(paper, &ctx).expect("builds");

    let started = Instant::now();
    for _ in 0..RUNS {
        let doc = kitchen_document(paper, &ctx).expect("builds");
        let laid = layout(&doc).expect("lays out");
        std::hint::black_box(text::to_text(&laid));
    }
    let p3 = started.elapsed().as_secs_f64() * 1_000.0 / f64::from(RUNS);

    println!("\n--- P3: rendering a kitchen ticket ---");
    println!("  items                {}", lines.len());
    println!("  P3 build+lay+text    {p3:.3} ms   budget 1 ms, ceiling 5 ms\n");

    if cfg!(debug_assertions) {
        return;
    }
    assert!(p3 < 5.0, "P3 took {p3:.2} ms, over the 5 ms ceiling");
}
