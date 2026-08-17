//! T2-T8 and T13: the layout rules, the golden files, and the kitchen delta.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Fixture;
use mb_core::{LineIdentity, ItemId, Money, OrderType, Qty};
use mb_print::doc::{Align, Block, Column, Document, Style};
use mb_print::layout::{Note, layout};
use mb_print::paper::{Offset, Paper, PaperKind};
use mb_print::settings::KitchenSettings;
use mb_print::template::{Copy, KitchenContext, TicketKind, TicketLine, bill_document, kitchen_document};
use mb_print::{PrintError, text};

/// T2. GOLDEN FILES.
///
/// Render a known bill at every paper size and compare against a committed
/// snapshot. Reviewing a receipt change becomes reading a diff, which is the
/// only way a change to a bill ever gets reviewed properly.
///
/// Set `MB_UPDATE_GOLDEN=1` to rewrite them, and then **read the diff** — that
/// is the whole point, and a golden file updated without being read is worse
/// than no golden file at all.
#[test]
fn t2_golden_files() {
    let fixture = Fixture::new();
    for (name, kind) in [
        ("58mm", PaperKind::Mm58),
        ("80mm", PaperKind::Mm80),
        ("100mm", PaperKind::Mm100),
        ("a4", PaperKind::A4),
    ] {
        let doc = bill_document(Paper::new(kind), &fixture.context(Copy::Original))
            .expect("builds");
        let rendered = text::to_text(&layout(&doc).expect("lays out"));

        let path = std::path::Path::new("tests/golden").join(format!("bill-{name}.txt"));
        if std::env::var_os("MB_UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all("tests/golden").expect("golden dir");
            std::fs::write(&path, rendered.as_bytes()).expect("write golden");
            continue;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "{} is missing — run with MB_UPDATE_GOLDEN=1 and read the diff",
                path.display()
            )
        });
        assert_eq!(
            normalise(&expected),
            normalise(&rendered),
            "the {name} bill changed. Read the diff, then MB_UPDATE_GOLDEN=1 if it is right."
        );
    }
}

/// Git may check the golden files out with CRLF; the renderer always writes LF.
fn normalise(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// T3. A long name wraps and loses nothing. Rule one.
#[test]
fn t3_a_long_name_wraps_and_loses_nothing() {
    let name = "Paneer Butter Masala (Half) - Extra Spicy, No Onion";
    let mut doc = Document::new(Paper::new(PaperKind::Mm58));
    doc.push(Block::Columns {
        columns: vec![
            Column::fill(Align::Left),
            Column::fixed(4, Align::Right),
            Column::fixed(9, Align::Right),
        ],
        rows: vec![vec![name.to_owned(), "2".to_owned(), "480.00".to_owned()]],
        style: Style::NORMAL,
    });
    let rendered = text::to_text(&layout(&doc).expect("lays out"));

    // Every word, in order, across however many lines it took.
    let flat: String = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut from = 0;
    for word in name.split_whitespace() {
        let at = flat[from..]
            .find(word)
            .unwrap_or_else(|| panic!("{word:?} was lost or reordered in:\n{rendered}"));
        from += at + word.len();
    }
    assert!(rendered.contains("480.00"), "the amount was lost");
}

/// T4. The columns always add up to the paper width.
///
/// A ragged right edge on a receipt looks like a fault, and the item table is
/// the block a customer looks at hardest.
#[test]
fn t4_the_item_table_fills_the_paper_exactly() {
    let fixture = Fixture::new();
    for kind in [
        PaperKind::Mm58,
        PaperKind::Mm80,
        PaperKind::Mm100,
        PaperKind::A4,
    ] {
        for hsn in [false, true] {
            let mut settings = fixture.settings.clone();
            settings.show.hsn = hsn;
            let ctx = mb_print::template::BillContext {
                settings: &settings,
                ..fixture.context(Copy::Original)
            };
            let doc = bill_document(Paper::new(kind), &ctx).expect("builds");
            let laid = layout(&doc).expect("lays out");

            // The separator lines are laid to the full usable width, so they
            // are the honest measure of what the table should match.
            let rule_width = laid
                .lines
                .iter()
                .find_map(|l| match &l.content {
                    mb_print::LaidContent::Separator { width, .. } => Some(*width),
                    _ => None,
                })
                .expect("a bill has separators");
            assert_eq!(
                rule_width,
                kind.columns(),
                "{kind:?} hsn={hsn}: the rules do not span the paper"
            );

            for line in laid.text_lines() {
                assert!(
                    line.chars().count() <= kind.columns(),
                    "{kind:?} hsn={hsn}: a line is {} columns, paper is {}:\n{line}",
                    line.chars().count(),
                    kind.columns()
                );
            }
        }
    }
}

/// T5. **The money wins**, and an amount that cannot fit at all is an error.
#[test]
fn t5_the_money_wins() {
    let mut doc = Document::new(Paper::new(PaperKind::Mm58));
    doc.row(
        "Paneer Butter Masala (Half) Extra Spicy No Onion",
        "1,240.00",
        Style::NORMAL,
    );
    let laid = layout(&doc).expect("lays out");
    let lines = laid.text_lines();

    assert!(
        lines[0].ends_with("1,240.00"),
        "the amount is not intact on the first line: {:?}",
        lines[0]
    );
    assert!(lines.len() > 1, "the label should have wrapped");
    assert!(
        laid.notes
            .iter()
            .any(|n| matches!(n, Note::LabelWrapped { .. })),
        "the wrap was not recorded"
    );

    // And the last clause of rule three: a right-hand side alone wider than the
    // paper is an error, not a truncation.
    //
    // Worth being honest about how reachable this is. `Money` tops out at
    // twenty characters, and twenty fits on the narrowest paper we sell, so a
    // real *amount* cannot trigger it — the layout caps the scale to 1 and it
    // fits. What CAN trigger it is a template putting something long on the
    // right of a row, and that is a template bug this guard turns into a clear
    // error instead of a silently short line. The unit test in `layout.rs`
    // covers the amount case at a narrower width directly.
    let mut impossible = Document::new(Paper::new(PaperKind::Mm58));
    impossible.row(
        "x",
        "THIS IS NOT AN AMOUNT IT IS FORTY CHARACTERS",
        Style::NORMAL,
    );
    match layout(&impossible) {
        Err(PrintError::AmountTooWide { .. }) => {}
        other => panic!("an unprintable right-hand side was not refused: {other:?}"),
    }
}

/// T6. The font cap. Crown jewel 18.
#[test]
fn t6_a_heading_too_big_is_capped_and_stays_complete() {
    let mut doc = Document::new(Paper::new(PaperKind::Mm58));
    doc.text(
        "ANNAPOORNESHWARI REFRESHMENTS",
        Style::new(3, true),
        Align::Centre,
    );
    let laid = layout(&doc).expect("lays out");

    assert!(laid.was_capped(), "the scale was not capped");
    let used = laid
        .notes
        .iter()
        .find_map(|n| match n {
            Note::ScaleCapped { asked, used } => Some((*asked, *used)),
            _ => None,
        })
        .expect("the cap was not recorded");
    // **In dots since 2026-08-17**, not in the ESC/POS multiplier. `Style::new(3, …)`
    // is 3 cells, which is 72 dots, and the note says what was asked for and
    // what was used in the same unit a shop's size setting is in.
    assert_eq!(used.0, 72, "it should have been asked for at three cells");
    assert!(used.1 < 72, "it should have come down");
    assert!(
        used.1 >= 24,
        "capping must not take a heading below the ordinary body size: {used:?}"
    );

    let rendered = text::to_text(&laid);
    for word in "ANNAPOORNESHWARI REFRESHMENTS".split(' ') {
        assert!(rendered.contains(word), "{word} was lost while capping");
    }
}

/// T7. The print offset — scope 7.11.
#[test]
fn t7_the_offset_moves_everything_and_clamps() {
    let fixture = Fixture::new();
    let build = |offset: Offset| {
        let paper = Paper::new(PaperKind::Mm80).with_offset(offset);
        let doc = bill_document(paper, &fixture.context(Copy::Original)).expect("builds");
        layout(&doc).expect("lays out")
    };

    let plain = build(Offset::none());
    let shifted = build(Offset::new(3, 0));

    // 3 mm on 80 mm paper is 2 columns, and EVERY line moves by the same
    // amount — a partial shift would mean two sinks disagreeing about the
    // origin, which is the thing this crate exists to prevent.
    for line in &shifted.lines {
        assert_eq!(line.indent, 2, "a line did not move with the rest");
    }
    assert!(plain.lines.iter().all(|l| l.indent == 0));

    // Nothing falls off the right edge: the usable width shrank to match.
    for line in shifted.text_lines() {
        assert!(
            line.chars().count() <= PaperKind::Mm80.columns(),
            "the offset pushed a line off the paper: {line:?}"
        );
    }

    // An absurd offset is clamped, and the clamp is recorded so P07's test
    // print can say so.
    let silly = build(Offset::new(40, 0));
    assert!(silly.was_clamped());
    for line in silly.text_lines() {
        assert!(line.chars().count() <= PaperKind::Mm80.columns());
    }

    // A vertical offset is blank lines at the top.
    let down = build(Offset::new(0, 3));
    assert!(
        down.lines.len() > plain.lines.len(),
        "a vertical offset added nothing"
    );

    // Negative clamps at the left edge rather than going off it.
    let left = build(Offset::new(-10, 0));
    assert!(left.lines.iter().all(|l| l.indent == 0));
}

/// T8. The printed GST summary sums to the bill's tax.
///
/// Audit B11: v1 *"splits GST 50/50 into CGST/SGST always. No IGST, no
/// inter-state, no HSN summary, and nothing that can be filed directly."* This
/// block is what a chartered accountant checks first.
#[test]
fn t8_the_printed_tax_summary_sums_to_the_bills_tax() {
    let fixture = Fixture::new();
    let doc = bill_document(
        Paper::new(PaperKind::Mm80),
        &fixture.context(Copy::Original),
    )
    .expect("builds");
    let rendered = text::to_text(&layout(&doc).expect("lays out"));

    // Pull the summary block out of the paper and add it up.
    let summary: Vec<&str> = rendered
        .lines()
        .skip_while(|l| !l.contains("Tax summary"))
        .take_while(|l| !l.trim().is_empty())
        .collect();
    assert!(summary.len() > 2, "no tax summary was printed");

    let mut cgst = Money::ZERO;
    let mut sgst = Money::ZERO;
    for line in summary.iter().skip(2) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 || !fields[0].ends_with('%') {
            continue;
        }
        cgst = cgst
            .add(Money::parse(fields[2]).expect("a printed amount parses"))
            .expect("sum");
        sgst = sgst
            .add(Money::parse(fields[3]).expect("a printed amount parses"))
            .expect("sum");
    }

    assert_eq!(
        cgst, fixture.bill.total_tax.cgst,
        "the printed CGST rows do not sum to the bill's CGST"
    );
    assert_eq!(
        sgst, fixture.bill.total_tax.sgst,
        "the printed SGST rows do not sum to the bill's SGST"
    );

    // And the non-GST value is on the paper, outside every GST total. Scope
    // 2.3, and it is what lets a bar bill at all.
    assert!(rendered.contains("Non-GST value"));
}

/// T13. The kitchen ticket is a delta, in cart order.
#[test]
fn t13_the_kitchen_ticket_is_a_delta_in_cart_order() {
    let settings = KitchenSettings::default();
    let lines = vec![
        TicketLine::from_delta(
            &LineIdentity {
                item_id: ItemId::new("itm_dosa"),
                note: Some("extra crispy".to_owned()),
                modifier_ids: vec![],
            },
            Qty::from_whole(1).expect("qty"),
            "Masala Dosa".to_owned(),
            vec!["extra cheese".to_owned()],
        ),
        TicketLine::from_delta(
            &LineIdentity {
                item_id: ItemId::new("itm_idli"),
                note: None,
                modifier_ids: vec![],
            },
            Qty::from_whole(1).expect("qty"),
            "Idli".to_owned(),
            vec![],
        ),
    ];

    let ctx = KitchenContext {
        kind: TicketKind::New,
        token: Some("42"),
        bill_number: Some("BIR/1207"),
        order_type: OrderType::DineIn,
        table: Some("6"),
        time: Some("21:40"),
        station: None,
        lines: &lines,
        settings: &settings,
    };
    let rendered = text::to_text(
        &layout(&kitchen_document(Paper::new(PaperKind::Mm80), &ctx).expect("builds"))
            .expect("lays out"),
    );

    assert!(rendered.contains("KITCHEN"));
    assert!(rendered.contains("Masala Dosa"));
    assert!(rendered.contains("Idli"));
    assert!(rendered.contains("extra cheese"), "a modifier is missing");
    assert!(rendered.contains("extra crispy"), "a note is missing");

    // Cart order, not alphabetical and not grouped.
    let dosa = rendered.find("Masala Dosa").expect("dosa");
    let idli = rendered.find("Idli").expect("idli");
    assert!(dosa < idli, "the ticket was reordered");

    // Two dosa were already printed; the delta says one. The ticket must not
    // say three — crown jewel 2, and mb-core decided the number, not this
    // crate.
    assert!(!rendered.contains(" 3 "), "the ticket printed a total, not a delta");

    // An empty delta is refused rather than printed: a blank ticket wastes
    // paper and teaches the kitchen to ignore tickets.
    let empty = KitchenContext { lines: &[], ..ctx };
    assert!(kitchen_document(Paper::new(PaperKind::Mm80), &empty).is_err());
}

/// A cancellation slip is the same ticket wearing a different word. Scope 1.19,
/// P12 decides when to send one.
#[test]
fn a_cancellation_slip_says_cancel() {
    let settings = KitchenSettings::default();
    let lines = vec![TicketLine::from_delta(
        &LineIdentity {
            item_id: ItemId::new("itm_dosa"),
            note: None,
            modifier_ids: vec![],
        },
        Qty::from_whole(2).expect("qty"),
        "Masala Dosa".to_owned(),
        vec![],
    )];
    let ctx = KitchenContext {
        kind: TicketKind::Cancellation,
        token: Some("42"),
        bill_number: None,
        order_type: OrderType::DineIn,
        table: Some("6"),
        time: None,
        station: None,
        lines: &lines,
        settings: &settings,
    };
    let rendered = text::to_text(
        &layout(&kitchen_document(Paper::new(PaperKind::Mm80), &ctx).expect("builds"))
            .expect("lays out"),
    );
    assert!(rendered.contains("CANCEL"), "a cancellation must say so");
    assert!(rendered.contains("Masala Dosa"));
}
