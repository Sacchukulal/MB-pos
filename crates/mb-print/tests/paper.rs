//! The tests that measure the paper.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    reason = "tests: expect is the assertion, and dots are not money"
)]

mod common;

use std::sync::Arc;

use mb_core::{
    AnyOrder, BillInput, BusinessDay, Cart, Claimed, DraftOrder, ItemId, ItemSnapshot, Money,
    OpenOrder, OrderId, OrderType, Payment, PaymentMode, Qty, Registration, Settlement, StaffId,
    TableId, Timestamp, compute_bill,
};
use mb_print::doc::{Align, Document, Pattern, Style};
use mb_print::font::Font;
use mb_print::layout::{Laid, LaidContent, layout_for};
use mb_print::metrics::Metrics;
use mb_print::paper::{DOTS_PER_MM, Paper, PaperKind};
use mb_print::raster::{Band, RasterOptions, to_raster};
use mb_print::settings::ReceiptSettings;
use mb_print::template::{BillContext, Copy, EInvoice, Store, bill_document};

const AT: Timestamp = Timestamp::from_millis(1_770_000_000_000);

fn metrics(kind: PaperKind) -> Metrics {
    Metrics::face(
        Paper::new(kind),
        Arc::new(Font::default_face().expect("the default face loads")),
    )
}

fn one_dosa() -> (mb_core::Bill, AnyOrder) {
    let mut cart = Cart::new();
    cart.add(
        ItemSnapshot::new(
            ItemId::new("itm"),
            "MASALA DOSE",
            Money::from_paise(10_000),
            common::pc(5),
        ),
        Qty::from_whole(1).expect("qty"),
        None,
        vec![],
    )
    .expect("adds");
    let bill = compute_bill(
        BillInput::new(&cart, Registration::Regular).with_order_type(OrderType::DineIn),
    )
    .expect("a bill");
    let day = BusinessDay::of(AT, mb_core::DayRule::DEFAULT, mb_core::UtcOffset::INDIA);
    let core = DraftOrder::new(
        OrderId::new("ord"),
        day,
        AT,
        mb_core::Placement::on_table(TableId::new("6")),
        StaffId::new("s"),
    )
    .core;
    let open = OpenOrder {
        core,
        token: Claimed {
            value: 3,
            formatted: "3".to_owned(),
            business_day: day,
        },
        bill_number: Claimed {
            value: 8,
            formatted: "0008".to_owned(),
            business_day: day,
        },
    };
    let mut settlement = Settlement::new();
    settlement
        .add(Payment::new(PaymentMode::Card, bill.grand_total).expect("a payment"))
        .expect("settles");
    let settled = open
        .settle(bill.clone(), settlement, AT, StaffId::new("s"))
        .expect("settles");
    (bill, AnyOrder::Settled(settled))
}

fn a_shop() -> Store {
    Store {
        name: "laptop test".to_owned(),
        address: "test address indian hotel".to_owned(),
        phone: Some("9000000009".to_owned()),
        ..Store::default()
    }
}

fn owners_bill(kind: PaperKind, settings: &ReceiptSettings) -> Laid {
    let (bill, order) = one_dosa();
    let store = a_shop();
    let doc = bill_document(
        &common::metrics(kind),
        &BillContext {
            bill: &bill,
            order: &order,
            store: &store,
            settings,
            customer: None,
            cashier: Some("test laptop"),
            table: Some("6"),
            time: Some("19:42"),
            waiter: None,
            copy: Copy::Original,
            einvoice: EInvoice::default(),
            logo: None,
        },
    )
    .expect("builds");
    layout_for(&doc, &metrics(kind)).expect("lays out")
}

// T-ink. The size a shop picks is the size that prints.

#[test]
fn a_capital_is_as_tall_as_the_size_says() {
    for family in mb_print::font::FAMILIES {
        let Some(font) = load(*family) else { continue };
        for cap in Style::LADDER {
            let cell = font.cell_for_cap(u32::from(cap));
            let glyph = font.glyph('M', cell);
            let mut top = u32::MAX;
            let mut bottom = 0;
            for y in 0..glyph.height {
                for x in 0..glyph.width {
                    if glyph.ink(x, y) {
                        let at = u32::try_from(glyph.top).unwrap_or(0) + y;
                        top = top.min(at);
                        bottom = bottom.max(at);
                    }
                }
            }
            assert!(top != u32::MAX, "{} drew no M at size {cap}", family.key);
            let drawn = bottom + 1 - top;
            assert!(
                drawn.abs_diff(u32::from(cap)) <= 1,
                "{}: size {cap} drew a {drawn}-dot capital",
                family.key
            );
        }
    }
}

/// A face this machine may not have.
fn load(family: mb_print::font::Family) -> Option<Font> {
    family.load().ok()
}

// T-rule. A rule is a rule.

/// A solid rule has ink in every column, edge to edge.
#[test]
fn a_solid_rule_is_solid_from_edge_to_edge() {
    let m = metrics(PaperKind::Mm80);
    let mut doc = Document::new(Paper::new(PaperKind::Mm80));
    doc.separator(Pattern::Solid);
    let laid = layout_for(&doc, &m).expect("lays out");
    let raster = to_raster(&laid, &m, RasterOptions::default()).expect("rasters");

    let image = ink(&raster);
    let dots = PaperKind::Mm80.dots().expect("a roll has dots");
    for x in 0..dots {
        assert!(
            (0..image.height).any(|y| image.ink(x, y)),
            "column {x} of {dots} has no ink — the rule is not solid"
        );
    }
}

/// A dashed rule starts and ends on ink, so the line reaches both edges of the paper.
#[test]
fn a_dashed_rule_reaches_both_edges() {
    let m = metrics(PaperKind::Mm80);
    for pattern in [Pattern::Dashed, Pattern::Dotted] {
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.separator(pattern);
        let laid = layout_for(&doc, &m).expect("lays out");
        let raster = to_raster(&laid, &m, RasterOptions::default()).expect("rasters");

        let image = ink(&raster);
        let dots = PaperKind::Mm80.dots().expect("a roll has dots");
        assert!(
            (0..image.height).any(|y| image.ink(0, y)),
            "{pattern:?} does not start at the left edge"
        );
        assert!(
            (0..image.height).any(|y| image.ink(dots - 1, y)),
            "{pattern:?} does not reach the right edge"
        );
    }
}

/// `Double` really is two strokes, and `Bold` really is thicker.
#[test]
fn every_pattern_draws_something_different() {
    let m = metrics(PaperKind::Mm80);
    let inked = |pattern: Pattern| {
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.separator(pattern);
        let laid = layout_for(&doc, &m).expect("lays out");
        let raster = to_raster(&laid, &m, RasterOptions::default()).expect("rasters");
        raster.ink.iter().map(|l| l.dots).sum::<u32>()
    };
    let solid = inked(Pattern::Solid);
    assert_eq!(inked(Pattern::Double), solid * 2, "Double is two strokes");
    assert_eq!(inked(Pattern::Bold), solid * 2, "Bold is twice as thick as Solid");
    assert!(inked(Pattern::Dashed) < solid, "Dashed is not solid");
    assert!(
        inked(Pattern::Dotted) < inked(Pattern::Dashed),
        "Dotted is thinner than dashed"
    );
}

// T-length. The roll is a budget.

#[test]
fn a_one_item_bill_is_not_a_foot_of_paper() {
    let laid = owners_bill(PaperKind::Mm80, &ReceiptSettings::default());
    let mm = laid.total_mm();
    assert!(
        mm <= 85,
        "the owner's one-dosa bill is {mm} mm of roll — 117 before P32, and about 70 before the sections got their air (2026-09-03)"
    );
    // And it is a real bill, not an empty one.
    assert!(mm >= 40, "{mm} mm is too short to be a bill at all");
}

/// The same, on the other two rolls a shop can buy.
#[test]
fn every_roll_prints_a_short_bill() {
    for (kind, budget) in [(PaperKind::Mm58, 105), (PaperKind::Mm100, 80)] {
        let laid = owners_bill(kind, &ReceiptSettings::default());
        let mm = laid.total_mm();
        assert!(
            mm <= budget,
            "{kind:?} takes {mm} mm, over its {budget} mm budget"
        );
    }
}

/// A rule costs a fraction of a line, not a whole one.
#[test]
fn a_rule_costs_less_than_a_line_of_text() {
    let m = metrics(PaperKind::Mm80);
    let mut doc = Document::new(Paper::new(PaperKind::Mm80));
    doc.line("TOTAL").separator(Pattern::Dashed);
    let laid = layout_for(&doc, &m).expect("lays out");
    let text = laid.lines[0].row_dots;
    let rule = laid.lines[1].row_dots;
    assert!(
        rule * 2 < text,
        "a rule costs {rule} dots against a line's {text} — it used to cost the same"
    );
}

// T-sums. The printed lines add up, by eye.

/// The Amount column adds up to the printed Subtotal, exactly.
#[test]
fn the_amount_column_adds_up_to_the_subtotal() {
    let settings = ReceiptSettings::default();
    let laid = owners_bill(PaperKind::Mm80, &settings);
    let lines = laid.text_lines();

    let subtotal = money_on(&lines, "Subtotal").expect("a bill has a subtotal");
    let column = amount_column(&lines);
    assert_eq!(
        column, subtotal,
        "the item column comes to {column} paise and the subtotal says {subtotal}"
    );
}

/// The same claim on a bill that mixes every kind of line there is — an exclusive one, an
/// inclusive one, one outside GST, a discount and two charges.
#[test]
fn a_mixed_bill_still_adds_up() {
    let fixture = common::Fixture::new();
    let doc = bill_document(&metrics(PaperKind::Mm100), &fixture.context(Copy::Original))
        .expect("builds");
    let laid = layout_for(&doc, &metrics(PaperKind::Mm100)).expect("lays out");
    let lines = laid.text_lines();

    let subtotal = money_on(&lines, "Subtotal").expect("a subtotal");
    assert_eq!(amount_column(&lines), subtotal, "{lines:#?}");

    // And the whole thing reconciles: subtotal − discount + charges + tax added + round-off is
    // the printed total.
    let bill = &fixture.bill;
    let mut running = bill.subtotal;
    running = running.sub(bill.total_discount).expect("a discount");
    for charge in &bill.charges {
        running = running.add(charge.amount).expect("a charge");
    }
    running = running
        .add(bill.gst_added.total().expect("tax"))
        .expect("tax");
    running = running.add(bill.round_off).expect("round off");
    assert_eq!(
        running, bill.grand_total,
        "the printed lines do not sum to the printed total"
    );
}

/// Requirement 7, as an equation on the model itself: the two halves of the tax always make the
/// whole.
#[test]
fn the_tax_split_is_exhaustive() {
    let fixture = common::Fixture::new();
    let bill = &fixture.bill;
    assert_eq!(
        bill.gst_included.add(bill.gst_added).expect("adds"),
        bill.total_gst,
        "some GST is in neither half"
    );
    assert_eq!(
        bill.vat_included.add(bill.vat_added).expect("adds"),
        bill.total_vat,
        "some VAT is in neither half"
    );
}

// T-band. The letterhead.

/// 30 % picture, 70 % text, on every roll.
#[test]
fn the_letterhead_gives_the_logo_thirty_per_cent() {
    for kind in [PaperKind::Mm58, PaperKind::Mm80, PaperKind::Mm100] {
        let m = metrics(kind);
        let mut settings = ReceiptSettings::default();
        settings.logo = mb_print::settings::LogoPosition::Left;
        settings.logo_width_pct = 30;

        let (bill, order) = one_dosa();
        let store = a_shop();
        let doc = bill_document(
            &m,
            &BillContext {
                bill: &bill,
                order: &order,
                store: &store,
                settings: &settings,
                customer: None,
                cashier: None,
                table: Some("6"),
                time: Some("19:42"),
                waiter: None,
                copy: Copy::Original,
                einvoice: EInvoice::default(),
                logo: Some(mb_print::image::Monochrome::blank(120, 60).encode()),
            },
        )
        .expect("builds");
        let laid = layout_for(&doc, &m).expect("lays out");

        let LaidContent::Band {
            image_left,
            image_width,
            lines,
            ..
        } = &laid.lines[0].content
        else {
            panic!("{kind:?}: the letterhead is not a band");
        };
        let dots = m.dots();
        assert_eq!(*image_left, 0, "{kind:?}: the logo is not on the left");
        assert_eq!(
            *image_width,
            dots * 30 / 100,
            "{kind:?}: the logo is not 30 %"
        );
        for line in lines {
            assert_eq!(
                line.left, *image_width,
                "{kind:?}: the text overlaps the logo"
            );
            assert_eq!(
                line.width,
                dots - image_width,
                "{kind:?}: the text is not the other 70 %"
            );
            assert_eq!(
                line.align,
                Align::Centre,
                "{kind:?}: the owner asked for centred"
            );
        }
        assert!(
            lines.iter().any(|l| l.text.contains("laptop test")),
            "{kind:?}: the shop's name is not in the letterhead"
        );
    }
}

fn ink(raster: &mb_print::raster::Raster) -> &mb_print::image::Monochrome {
    raster
        .bands
        .iter()
        .find_map(|b| match b {
            Band::Ink { image } => Some(image),
            _ => None,
        })
        .expect("something was drawn")
}

/// The amount on a labelled row, in paise.
fn money_on(lines: &[String], label: &str) -> Option<i64> {
    lines
        .iter()
        .find(|l| l.trim_start().starts_with(label))
        .and_then(|l| paise(l.split_whitespace().last()?))
}

/// The Amount column, summed out of the characters on the paper.
fn amount_column(lines: &[String]) -> i64 {
    let mut total = 0;
    let mut inside = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("Item ") || trimmed == "Item" {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if trimmed.starts_with("Subtotal") {
            break;
        }
        // A wrapped name or a note has no amount on it, and neither does a blank row between
        // items.
        if let Some(last) = trimmed.split_whitespace().last()
            && let Some(amount) = paise(last)
            // A wrapped continuation ending in a number would be a dish called "Beer 650" — the
            // amount column always has two decimal places.
            && last.contains('.')
        {
            total += amount;
        }
    }
    total
}

fn paise(text: &str) -> Option<i64> {
    let (rupees, paise) = text.split_once('.')?;
    let rupees: i64 = rupees.parse().ok()?;
    let paise: i64 = paise.parse().ok()?;
    Some(rupees * 100 + if rupees < 0 { -paise } else { paise })
}

/// The roll is eight dots to the millimetre, and every number above depends on it.
#[test]
fn the_head_is_the_one_these_numbers_assume() {
    assert_eq!(DOTS_PER_MM, 8);
}
