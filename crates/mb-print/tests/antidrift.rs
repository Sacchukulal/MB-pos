#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Fixture;
use mb_core::Money;
use mb_print::layout::layout;
use mb_print::paper::{Paper, PaperKind};
use mb_print::render::{Call, Recorder, render};
use mb_print::settings::QrMode;
use mb_print::template::{Copy, bill_document};
use mb_print::{pdf, text};

/// THE ANTI-DRIFT TEST.
#[test]
fn t1_no_sink_can_drop_anything() {
    let fixture = Fixture::new();
    let ctx = fixture.context(Copy::Duplicate { number: 2 });
    let doc = bill_document(&common::metrics(PaperKind::Mm80), &ctx).expect("builds");
    let laid = layout(&doc).expect("lays out");

    let mut recorder = Recorder::new();
    render(&laid, &mut recorder);

    let as_text = text::to_text(&laid);
    let as_pdf = String::from_utf8_lossy(&pdf::to_pdf(&laid)).into_owned();

    // Every line of text the traversal produced must be in both sinks.
    let seen = recorder.texts();
    assert!(seen.len() > 20, "the fixture is too thin to prove anything");

    // Compared with the line breaks removed.
    let flat_text = flatten(&as_text);
    for piece in &seen {
        assert!(
            flat_text.contains(&flatten(piece)),
            "the text sink dropped {piece:?}"
        );
    }

    // The PDF escapes brackets and backslashes, so compare against the escaped form rather than
    // pretending it does not.
    let flat_pdf = flatten(&as_pdf);
    for piece in &seen {
        let escaped = flatten(piece)
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        assert!(
            flat_pdf.contains(&escaped),
            "the PDF sink dropped {piece:?}"
        );
    }

    // And the values that matter most, named explicitly, so a fixture that silently stops
    // containing one of them cannot make this test vacuous.
    for must in [
        "BIR/1207",        // the bill number
        "Paneer",          // an item name (wrapped)
        "Water 1L",        // another
        "Beer",            // the non-GST line
        "2201",            // an HSN code
        "29ZYXWV9876K1Z2", // the customer's GSTIN (2.6)
        "29ABCDE1234F1ZW", // the shop's
        "DUPLICATE",       // audit D7
        "TOTAL",
        "Non-GST value", // scope 2.3 — the bar line
        "Tax summary",   // scope 2.7
        "Card",          // split payment (1.15)
        "Credit",
        "Sodexo",
        "Tip",
        "upi://pay", // the QR payload (8.2)
        "Thank you, visit again",
    ] {
        assert!(
            flat_text.contains(must),
            "the text sink is missing {must:?}"
        );
    }

    // The grand total, exactly as Money formats it.
    let total = fixture.bill.grand_total.to_plain_string();
    assert!(as_text.contains(&total), "the printed total is missing");
    assert!(as_pdf.contains(&total), "the PDF total is missing");

    // The image reached the traversal even though neither sink can draw it.
    assert!(
        recorder
            .calls
            .iter()
            .any(|c| matches!(c, Call::Image { .. })),
        "the logo never reached the traversal"
    );
    assert!(
        recorder.calls.iter().any(|c| matches!(c, Call::Qr { .. })),
        "the QR never reached the traversal"
    );
}

/// Line breaks removed, so a re-wrapped payload still compares equal.
fn flatten(s: &str) -> String {
    s.replace(['\n', '\r'], "")
}

type Mutation = Box<dyn Fn(&mut mb_print::settings::ReceiptSettings)>;

/// Amounts on paper are exactly `Money::to_plain_string`.
#[test]
fn t9_every_amount_on_paper_round_trips_through_money() {
    let fixture = Fixture::new();
    let ctx = fixture.context(Copy::Original);
    let doc = bill_document(&common::metrics(PaperKind::Mm80), &ctx).expect("builds");
    let as_text = text::to_text(&layout(&doc).expect("lays out"));

    let mut checked = 0;
    for token in as_text.split_whitespace() {
        let candidate = token.trim_start_matches('-');
        if !candidate.contains('.') {
            continue;
        }
        if !candidate.chars().all(|c| c.is_ascii_digit() || c == '.') {
            continue;
        }
        // Two decimal places is what `to_plain_string` produces, always.
        let Some((_, fraction)) = candidate.split_once('.') else {
            continue;
        };
        assert_eq!(
            fraction.len(),
            2,
            "{token:?} is not two decimal places — something formatted a number \
             itself instead of using Money::to_plain_string"
        );
        let parsed = Money::parse(token).unwrap_or_else(|e| {
            panic!("{token:?} came off the paper and will not parse back: {e}")
        });
        assert_eq!(
            parsed.to_plain_string(),
            token.trim_start_matches('+'),
            "{token:?} does not round-trip"
        );
        checked += 1;
    }
    assert!(
        checked > 8,
        "only found {checked} amounts — the scan is broken"
    );
}

/// A reprint is visibly marked and an original is not.
#[test]
fn t10_a_reprint_is_marked_and_an_original_is_not() {
    let fixture = Fixture::new();
    let paper = Paper::new(PaperKind::Mm80);

    let original = text::to_text(
        &layout(
            &bill_document(
                &common::metrics(paper.kind),
                &fixture.context(Copy::Original),
            )
            .expect("builds"),
        )
        .expect("lays out"),
    );
    let reprint = text::to_text(
        &layout(
            &bill_document(
                &common::metrics(paper.kind),
                &fixture.context(Copy::Duplicate { number: 3 }),
            )
            .expect("builds"),
        )
        .expect("lays out"),
    );
    let voided = text::to_text(
        &layout(
            &bill_document(
                &common::metrics(paper.kind),
                &fixture.context(Copy::Voided {
                    reason: "wrong table".to_owned(),
                }),
            )
            .expect("builds"),
        )
        .expect("lays out"),
    );

    assert!(!original.contains("DUPLICATE"));
    assert!(!original.contains("VOIDED"));

    assert!(reprint.contains("DUPLICATE"));
    assert!(reprint.contains("REPRINT #3"), "the count is not printed");

    assert!(voided.contains("VOIDED"));
    assert!(voided.contains("wrong table"), "the reason is not printed");
}

/// The bill a waiter carries to the table says it has not been paid.
#[test]
fn a_bill_carried_to_the_table_says_it_is_not_paid() {
    let fixture = Fixture::new();
    let paper = Paper::new(PaperKind::Mm80);

    let carried = text::to_text(
        &layout(
            &bill_document(
                &common::metrics(paper.kind),
                &fixture.context(Copy::NotPaid),
            )
            .expect("builds"),
        )
        .expect("lays out"),
    );
    let original = text::to_text(
        &layout(
            &bill_document(
                &common::metrics(paper.kind),
                &fixture.context(Copy::Original),
            )
            .expect("builds"),
        )
        .expect("lays out"),
    );

    assert!(
        carried.contains("NOT PAID"),
        "the bill is not marked:\n{carried}"
    );
    assert!(
        carried.contains("pay at the counter"),
        "it does not say what to do next:\n{carried}"
    );
    // And it is not confused with the other two markings.
    assert!(!carried.contains("DUPLICATE"));
    assert!(!carried.contains("VOIDED"));
    // A settled bill never grows the mark.
    assert!(!original.contains("NOT PAID"));
}

/// Every receipt setting changes the output.
#[test]
fn t11_every_setting_changes_the_output() {
    let fixture = Fixture::new();
    let paper = Paper::new(PaperKind::Mm80);

    let render_with = |settings: mb_print::settings::ReceiptSettings| {
        let ctx = mb_print::template::BillContext {
            settings: &settings,
            ..fixture.context(Copy::Original)
        };
        text::to_text(
            &layout(&bill_document(&common::metrics(paper.kind), &ctx).expect("builds"))
                .expect("lays out"),
        )
    };

    let base = render_with(fixture.settings.clone());

    let mutations: Vec<(&str, Mutation)> = vec![
        (
            "show.token",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.show.token)),
        ),
        (
            "show.gstin",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.show.gstin)),
        ),
        (
            "show.fssai",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.show.fssai)),
        ),
        (
            "show.address",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.show.address)),
        ),
        (
            "show.phone",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.show.phone)),
        ),
        (
            "show.cashier",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.show.cashier)),
        ),
        (
            "show.hsn",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.show.hsn)),
        ),
        (
            "show.tax_summary",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.show.tax_summary)),
        ),
        (
            "show.payment_lines",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.show.payment_lines)),
        ),
        (
            "sep.store_header",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_store_header)),
        ),
        (
            "sep.meta",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_meta)),
        ),
        (
            "sep.token",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_token)),
        ),
        (
            "sep.column_names",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_column_names)),
        ),
        (
            "sep.items",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_items)),
        ),
        (
            "sep.subtotals",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_subtotals)),
        ),
        (
            "sep.grand_total",
            Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_grand_total)),
        ),
        (
            "pattern",
            Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
                s.pattern = mb_print::Pattern::Dotted;
            }),
        ),
        (
            "footer",
            Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
                s.footer = "Come back soon".to_owned();
            }),
        ),
        (
            "qr",
            Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
                s.qr = QrMode::Static;
            }),
        ),
        (
            "sections.store_name",
            Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
                s.sections.store_name = mb_print::Style::new(1, false);
            }),
        ),
        (
            "sections.token",
            Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
                s.sections.token = mb_print::Style::new(1, false);
            }),
        ),
        (
            "row_height",
            Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
                s.row_height = mb_print::settings::RowHeight::Relaxed;
            }),
        ),
    ];

    for (name, mutate) in mutations {
        let mut settings = fixture.settings.clone();
        mutate(&mut settings);
        let changed = render_with(settings);
        assert_ne!(
            base, changed,
            "changing {name} did not change the bill — the setting is dead or broken (audit D5)"
        );
    }
}

fn toggle(
    settings: &mut mb_print::settings::ReceiptSettings,
    pick: impl Fn(&mut mb_print::settings::ReceiptSettings) -> &mut bool,
) {
    let field = pick(settings);
    *field = !*field;
}

/// The document survives JSON.
#[test]
fn t14_the_document_crosses_ipc_unchanged() {
    let fixture = Fixture::new();
    let doc = bill_document(
        &common::metrics(PaperKind::Mm80),
        &fixture.context(Copy::Original),
    )
    .expect("builds");

    let json = serde_json::to_string(&doc).expect("serialises");
    let back: mb_print::Document = serde_json::from_str(&json).expect("deserialises");
    assert_eq!(doc, back);

    // And the laid-out form too, since a preview may reasonably want that rather than re-laying
    // it out in JavaScript.
    let laid = layout(&doc).expect("lays out");
    let json = serde_json::to_string(&laid).expect("serialises");
    let back: mb_print::Laid = serde_json::from_str(&json).expect("deserialises");
    assert_eq!(laid, back);
}
