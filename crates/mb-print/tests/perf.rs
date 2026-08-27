//! Budgets P1, P2 and P3 — and they are new rows in the speed contract.
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
use mb_print::font::Font;
use mb_print::layout::layout;
use mb_print::paper::{Paper, PaperKind};
use mb_print::printer::PrinterConfig;
use mb_print::queue::sqlite::SqliteStore;
use mb_print::queue::{Job, JobKind, Queue, QueueConfig};
use mb_print::raster::{RasterOptions, to_raster};
use mb_print::settings::KitchenSettings;
use mb_print::template::{
    Copy, KitchenContext, TicketKind, TicketLine, bill_document, kitchen_document,
};
use mb_print::{pdf, text};

const RUNS: u32 = 200;

/// A forty-line bill, because that is what P1 says and the anti-drift fixture is deliberately
/// only three.
fn big_fixture() -> Fixture {
    use mb_core::{Cart, ItemSnapshot, Money, PriceBasis, TaxKind, TaxRate, TaxSpec};

    let mut cart = Cart::new();
    for n in 0..40_i64 {
        let rate = match n % 3 {
            0 => common::pc(5),
            1 => common::pc(12),
            _ => common::pc(18),
        };
        let tax = match n % 4 {
            0 => TaxSpec::gst(rate),
            1 => TaxSpec::gst_inclusive(rate),
            2 => TaxSpec::exempt(),
            _ => TaxSpec {
                kind: TaxKind::OutsideGst,
                rate: TaxRate::ZERO,
                basis: PriceBasis::Exclusive,
            },
        };
        let snapshot = ItemSnapshot::new(
            ItemId::new(format!("itm_{n:03}")),
            format!("Menu Item Number {n} With A Fairly Long Name"),
            Money::from_paise(12_000 + n * 137),
            rate,
        )
        .with_tax(tax)
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
    let _ = layout(&bill_document(&common::metrics(paper.kind), &ctx).expect("builds"))
        .expect("lays out");

    let started = Instant::now();
    for _ in 0..RUNS {
        let doc = bill_document(&common::metrics(paper.kind), &ctx).expect("builds");
        let laid = layout(&doc).expect("lays out");
        let out = text::to_text(&laid);
        std::hint::black_box(out);
    }
    let p1 = started.elapsed().as_secs_f64() * 1_000.0 / f64::from(RUNS);

    let started = Instant::now();
    for _ in 0..RUNS {
        let doc = bill_document(&common::metrics(paper.kind), &ctx).expect("builds");
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
        kot_number: Some("14"),
        order_type: OrderType::DineIn,
        table: Some("6"),
        time: Some("21:40"),
        waiter: Some("Suresh"),
        station: Some("TANDOOR"),
        reprint: false,
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

#[test]
fn p4_a_bill_becomes_dots_inside_its_budget() {
    let fixture = big_fixture();
    let paper = Paper::new(PaperKind::Mm80);
    let font = std::sync::Arc::new(Font::builtin().expect("the shipped face loads"));
    let doc = bill_document(
        &common::metrics(paper.kind),
        &fixture.context(Copy::Original),
    )
    .expect("builds");
    let laid = layout(&doc).expect("lays out");

    // Warmed once: the glyph cache fills on the first bill of the day and never grows again, so
    // the interesting number is the second bill and every one after it.
    let warm = to_raster(
        &laid,
        &mb_print::metrics::Metrics::face(laid.paper, std::sync::Arc::clone(&font)),
        RasterOptions::default(),
    )
    .expect("rasters");

    let runs = 50;
    let started = Instant::now();
    for _ in 0..runs {
        let doc = bill_document(
            &common::metrics(paper.kind),
            &fixture.context(Copy::Original),
        )
        .expect("builds");
        let laid = layout(&doc).expect("lays out");
        std::hint::black_box(
            to_raster(
                &laid,
                &mb_print::metrics::Metrics::face(laid.paper, std::sync::Arc::clone(&font)),
                RasterOptions::default(),
            )
            .expect("rasters"),
        );
    }
    let p4 = started.elapsed().as_secs_f64() * 1_000.0 / f64::from(runs);

    println!("\n--- P4: a bill becomes dots ---");
    println!("  lines                {}", laid.lines.len());
    println!("  dots                 576 x {}", warm.height());
    println!("  bands                {}", warm.bands.len());
    println!("  P4 build+lay+raster  {p4:.3} ms   budget 20 ms, ceiling 60 ms\n");

    if cfg!(debug_assertions) {
        return;
    }
    assert!(p4 < 60.0, "P4 took {p4:.2} ms, over the 60 ms ceiling");
}

/// A kitchen ticket handed to the print queue.
#[test]
fn b6_a_kitchen_ticket_reaches_the_queue_inside_its_budget() {
    let scratch = common::Scratch::new("b6");
    let db = std::sync::Arc::new(
        mb_db::Db::open(&mb_db::DbConfig::new(scratch.path("shop.db"))).expect("opens"),
    );
    common::seed_printer(&db, "prn_kitchen");

    let store = std::sync::Arc::new(SqliteStore::new(std::sync::Arc::clone(&db), common::OUTLET));
    let queue = Queue::start(
        vec![PrinterConfig::new(
            "prn_kitchen",
            "Kitchen",
            mb_print::printer::Target::None,
        )],
        store,
        std::sync::Arc::new(mb_print::font::OneFace::builtin().expect("the shipped face loads")),
        QueueConfig::default(),
    );

    let settings = KitchenSettings::default();
    let lines: Vec<TicketLine> = (0..20)
        .map(|n| TicketLine {
            name: format!("Menu Item Number {n}"),
            qty: Qty::from_whole(1 + i64::from(n % 3)).expect("qty"),
            note: None,
            modifiers: vec![],
        })
        .collect();
    let ctx = KitchenContext {
        kind: TicketKind::New,
        token: Some("42"),
        bill_number: Some("BIR/1207"),
        kot_number: Some("14"),
        order_type: OrderType::DineIn,
        table: Some("6"),
        time: Some("21:40"),
        waiter: Some("Suresh"),
        station: None,
        reprint: false,
        lines: &lines,
        settings: &settings,
    };
    let paper = Paper::new(PaperKind::Mm80);
    let day = mb_core::BusinessDay::from_ymd(2026, 8, 3);

    let runs = 50_u32;
    let mut worst = 0.0_f64;
    let started = Instant::now();
    for _ in 0..runs {
        let one = Instant::now();
        let doc = kitchen_document(paper, &ctx).expect("builds");
        queue
            .enqueue(Job::new(JobKind::Kitchen, "prn_kitchen", doc, day).because("table 6"))
            .expect("queued");
        worst = worst.max(one.elapsed().as_secs_f64() * 1_000.0);
    }
    let b6 = started.elapsed().as_secs_f64() * 1_000.0 / f64::from(runs);

    println!("\n--- B6: a kitchen ticket handed to the queue ---");
    println!("  store                SQLite, synchronous = FULL");
    println!("  B6 mean              {b6:.3} ms   budget 50 ms, ceiling 150 ms");
    println!("  B6 worst             {worst:.3} ms\n");

    queue.shutdown();

    if cfg!(debug_assertions) {
        return;
    }
    assert!(b6 < 150.0, "B6 took {b6:.2} ms, over the 150 ms ceiling");
}
