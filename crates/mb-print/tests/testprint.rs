#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use std::sync::Arc;

use mb_core::{BusinessDay, Money, Timestamp};
use mb_db::{Db, DbConfig, Repos};
use mb_print::layout::layout;
use mb_print::paper::{Offset, PaperKind};
use mb_print::printer::{PrinterConfig, Target, nudge};
use mb_print::queue::{Job, JobKind, MemoryStore, Queue, QueueConfig};
use mb_print::testprint::test_document;
use mb_print::text::to_text;

/// A test print works with nothing configured, all the way to bytes.
#[test]
fn t11_a_test_print_reaches_paper_with_no_database_and_no_printer() {
    let scratch = common::Scratch::new("testprint");
    let path = scratch.path("roll.bin");

    // No database anywhere in this test.
    let printer = PrinterConfig::new("prn_new", "Counter", Target::File { path: path.clone() });
    let queue = Queue::start(
        vec![printer.clone()],
        Arc::new(MemoryStore::new()),
        Arc::new(mb_print::font::OneFace::default_face().expect("the default face loads")),
        QueueConfig::default(),
    );

    let document = test_document(&printer, None);
    queue
        .enqueue(
            Job::new(
                JobKind::Test,
                "prn_new",
                document,
                BusinessDay::from_ymd(2026, 8, 3),
            )
            .because("test print"),
        )
        .expect("queued");

    let printed =
        common::until(|| path.exists() && std::fs::metadata(&path).is_ok_and(|m| m.len() > 0));
    assert!(printed, "the test print never reached the file");
    queue.shutdown();
}

/// The slip carries the ruler, the offset in both units, and sample money that round-trips like
/// every other amount on paper.
#[test]
fn t11_the_slip_says_everything_the_owner_needs_to_read_off_the_paper() {
    let printer = PrinterConfig::new("prn", "Counter", Target::None)
        .with_paper(PaperKind::Mm80)
        .with_offset(Offset::new(2, 0));
    let doc = test_document(&printer, None);
    let text = to_text(&layout(&doc).expect("lays out"));

    assert!(text.contains("TEST PRINT"));
    assert!(text.contains("Alignment ruler"));
    assert!(text.contains("Offset across"), "{text}");
    assert!(text.contains("+2 mm right"), "{text}");
    assert!(text.contains("Sample bill"));

    // Every amount on the slip is `Money::to_plain_string`, like every amount anywhere else.
    let mut checked = 0;
    for token in text.split_whitespace() {
        if !token.contains('.') || !token.chars().all(|c| c.is_ascii_digit() || c == '.') {
            continue;
        }
        let parsed = Money::parse(token)
            .unwrap_or_else(|e| panic!("{token:?} came off the slip and will not parse: {e}"));
        assert_eq!(parsed.to_plain_string(), token);
        checked += 1;
    }
    assert!(checked >= 5, "only found {checked} amounts on the slip");
}

/// The offset is adjustable, it moves everything, and it persists.
#[test]
fn t12_nudging_moves_the_print_and_the_value_survives_a_restart() {
    let scratch = common::Scratch::new("offset");
    let db = Db::open(&DbConfig::new(scratch.path("shop.db"))).expect("opens");
    common::seed_printer(&db, "prn_counter");

    let mut printer =
        PrinterConfig::new("prn_counter", "Counter", Target::None).with_paper(PaperKind::Mm80);

    let straight = layout(&test_document(&printer, None)).expect("lays out");

    // Two nudges of the kind an owner makes: print, look, nudge, print again.
    nudge(&mut printer, 2, 0);
    nudge(&mut printer, 1, 0);
    assert_eq!(printer.paper.offset, Offset::new(3, 0));

    let shifted = layout(&test_document(&printer, None)).expect("lays out");

    // 3 mm on 80 mm paper is two columns, and EVERY line starts two columns further in.
    assert!(straight.lines.iter().all(|l| l.indent_dots == 0));
    let moved = shifted.base_advance * 2;
    assert!(
        shifted.lines.iter().all(|l| l.indent_dots == moved),
        "the offset did not move every line by the same whole number of characters"
    );

    // Nothing was lost off the right edge.
    let printed = to_text(&shifted);
    let across = shifted
        .lines
        .iter()
        .filter_map(|l| match &l.content {
            mb_print::LaidContent::Separator { width, .. } => Some(*width),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    for line in printed.lines() {
        assert!(
            line.chars().count() <= PaperKind::Mm80.columns(),
            "{:?} is {} characters, and the rule is {} dots",
            line,
            line.chars().count(),
            across
        );
    }
    for must in ["TEST PRINT", "Alignment ruler", "Sample bill", "TOTAL"] {
        assert!(printed.contains(must), "{must} fell off the edge");
    }

    // And it persists: this is the half that makes 7.11 worth having.
    db.transaction(|tx| {
        Repos::new(tx).settings().save_printer(
            common::OUTLET,
            &mb_db::repo::settings::Printer {
                id: "prn_counter".to_owned(),
                name: "Counter".to_owned(),
                kind: "none".to_owned(),
                address: None,
                paper_mm: 80,
                is_default: true,
                can_kick_drawer: false,
                offset_x_mm: i64::from(printer.paper.offset.x_mm),
                offset_y_mm: i64::from(printer.paper.offset.y_mm),
                role: "both".to_owned(),
                engine: "raster".to_owned(),
                is_bold_dark: false,
            },
            Timestamp::from_millis(1_770_000_000_000),
        )
    })
    .expect("saves");
    drop(db);

    let reopened = Db::open(&DbConfig::new(scratch.path("shop.db"))).expect("reopens");
    let printers = reopened
        .transaction(|tx| Repos::new(tx).settings().list_printers(common::OUTLET))
        .expect("reads");
    assert_eq!(printers.len(), 1);
    assert_eq!(
        printers[0].offset_x_mm, 3,
        "the correction did not survive — the owner would have to make it again \
         every morning"
    );
}
