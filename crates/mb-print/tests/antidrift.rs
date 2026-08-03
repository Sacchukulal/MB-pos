//! T1 and the tests that go with it: the sinks cannot disagree.
//!
//! > Audit D1: *"the same bill is drawn three separate times, by hand, in three
//! > places… every design change is triple work, and the three **will** drift
//! > apart. This is the single biggest source of 'the preview does not match
//! > the paper'."*
//!
//! The shared description was necessary and not sufficient — v1 had one and
//! drifted anyway. What makes drift impossible is that there is one traversal
//! and the renderers are sinks. This file is where that stops being an
//! assertion in a comment.

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

/// T1. **THE ANTI-DRIFT TEST — the reason this session exists.**
///
/// Render one bill that has everything on it through the recorder, through the
/// text sink and through the PDF sink, and assert that **every material value
/// the recorder saw appears in both outputs**.
///
/// When P07 adds the raster sink it joins this test and nothing else changes.
#[test]
fn t1_no_sink_can_drop_anything() {
    let fixture = Fixture::new();
    let ctx = fixture.context(Copy::Duplicate { number: 2 });
    let doc = bill_document(Paper::new(PaperKind::Mm80), &ctx).expect("builds");
    let laid = layout(&doc).expect("lays out");

    let mut recorder = Recorder::new();
    render(&laid, &mut recorder);

    let as_text = text::to_text(&laid);
    let as_pdf = String::from_utf8_lossy(&pdf::to_pdf(&laid)).into_owned();

    // Every line of text the traversal produced must be in both sinks.
    let seen = recorder.texts();
    assert!(seen.len() > 20, "the fixture is too thin to prove anything");

    // Compared with the line breaks removed. The claim is "nothing was
    // DROPPED", not "nothing was re-wrapped" — a sink is allowed to break a
    // 54-character UPI URI across 32-column paper, and indeed must, because an
    // overflow is the one thing R3 forbids.
    let flat_text = flatten(&as_text);
    for piece in &seen {
        assert!(
            flat_text.contains(&flatten(piece)),
            "the text sink dropped {piece:?}"
        );
    }

    // The PDF escapes brackets and backslashes, so compare against the escaped
    // form rather than pretending it does not.
    let flat_pdf = flatten(&as_pdf);
    for piece in &seen {
        let escaped = flatten(piece)
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        assert!(flat_pdf.contains(&escaped), "the PDF sink dropped {piece:?}");
    }

    // And the values that matter most, named explicitly, so a fixture that
    // silently stops containing one of them cannot make this test vacuous.
    for must in [
        "BIR/1207",                     // the bill number
        "Paneer",                       // an item name (wrapped)
        "Water 1L",                     // another
        "Beer",                         // the non-GST line
        "2201",                         // an HSN code
        "29ZYXWV9876K1Z2",              // the customer's GSTIN (2.6)
        "29ABCDE1234F1Z5",              // the shop's
        "DUPLICATE",                    // audit D7
        "TOTAL",
        "Non-GST value",                // scope 2.3 — the bar line
        "Tax summary",                  // scope 2.7
        "Card",                         // split payment (1.15)
        "Khata",
        "Sodexo",
        "Tip",
        "upi://pay",                    // the QR payload (8.2)
        "Thank you, visit again",
    ] {
        assert!(
            flat_text.contains(must),
            "the text sink is missing {must:?}"
        );
    }

    // The grand total, exactly as Money formats it (R2).
    let total = fixture.bill.grand_total.to_plain_string();
    assert!(as_text.contains(&total), "the printed total is missing");
    assert!(as_pdf.contains(&total), "the PDF total is missing");

    // The image reached the traversal even though neither sink can draw it.
    // That is the point of a sink: ignoring a block is a visible decision in
    // one file, not an omission nobody notices for a year.
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

/// One change to a settings block, for T11.
type Mutation = Box<dyn Fn(&mut mb_print::settings::ReceiptSettings)>;

/// T9. Amounts on paper are exactly `Money::to_plain_string`.
///
/// R2: a renderer that formats a number has become a second money path. This
/// scans the output for anything shaped like an amount and asserts it parses
/// back to the same paise.
#[test]
fn t9_every_amount_on_paper_round_trips_through_money() {
    let fixture = Fixture::new();
    let ctx = fixture.context(Copy::Original);
    let doc = bill_document(Paper::new(PaperKind::Mm80), &ctx).expect("builds");
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
    assert!(checked > 8, "only found {checked} amounts — the scan is broken");
}

/// T10. A reprint is visibly marked and an original is not. Both in one test,
/// so the assertion is a difference rather than a hope. Audit D7.
#[test]
fn t10_a_reprint_is_marked_and_an_original_is_not() {
    let fixture = Fixture::new();
    let paper = Paper::new(PaperKind::Mm80);

    let original = text::to_text(
        &layout(&bill_document(paper, &fixture.context(Copy::Original)).expect("builds"))
            .expect("lays out"),
    );
    let reprint = text::to_text(
        &layout(
            &bill_document(paper, &fixture.context(Copy::Duplicate { number: 3 })).expect("builds"),
        )
        .expect("lays out"),
    );
    let voided = text::to_text(
        &layout(
            &bill_document(
                paper,
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

/// T11. Every receipt setting changes the output.
///
/// A setting that changes nothing is either dead or broken, and v1 shipped one
/// of each — audit D5: *"a setting exists that you cannot change."*
#[test]
fn t11_every_setting_changes_the_output() {
    let fixture = Fixture::new();
    let paper = Paper::new(PaperKind::Mm80);

    let render_with = |settings: mb_print::settings::ReceiptSettings| {
        let ctx = mb_print::template::BillContext {
            settings: &settings,
            ..fixture.context(Copy::Original)
        };
        text::to_text(&layout(&bill_document(paper, &ctx).expect("builds")).expect("lays out"))
    };

    let base = render_with(fixture.settings.clone());

    // Each of these is a toggle from audit Part 3 or from the new tax engine.
    let mutations: Vec<(&str, Mutation)> = vec![
        ("show.token", Box::new(|s: &mut _| toggle(s, |s| &mut s.show.token))),
        ("show.gstin", Box::new(|s: &mut _| toggle(s, |s| &mut s.show.gstin))),
        ("show.fssai", Box::new(|s: &mut _| toggle(s, |s| &mut s.show.fssai))),
        ("show.address", Box::new(|s: &mut _| toggle(s, |s| &mut s.show.address))),
        ("show.phone", Box::new(|s: &mut _| toggle(s, |s| &mut s.show.phone))),
        ("show.cashier", Box::new(|s: &mut _| toggle(s, |s| &mut s.show.cashier))),
        ("show.hsn", Box::new(|s: &mut _| toggle(s, |s| &mut s.show.hsn))),
        ("show.tax_summary", Box::new(|s: &mut _| toggle(s, |s| &mut s.show.tax_summary))),
        ("show.payment_lines", Box::new(|s: &mut _| toggle(s, |s| &mut s.show.payment_lines))),
        ("sep.store_header", Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_store_header))),
        ("sep.meta", Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_meta))),
        ("sep.token", Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_token))),
        ("sep.column_names", Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_column_names))),
        ("sep.items", Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_items))),
        ("sep.subtotals", Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_subtotals))),
        ("sep.grand_total", Box::new(|s: &mut _| toggle(s, |s| &mut s.separators.below_grand_total))),
        ("pattern", Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
            s.pattern = mb_print::Pattern::Dotted;
        })),
        ("footer", Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
            s.footer = "Come back soon".to_owned();
        })),
        ("qr", Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
            s.qr = QrMode::Static;
        })),
        ("sections.store_name", Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
            s.sections.store_name = mb_print::Style::new(1, false);
        })),
        ("sections.token", Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
            s.sections.token = mb_print::Style::new(1, false);
        })),
        ("row_height", Box::new(|s: &mut mb_print::settings::ReceiptSettings| {
            s.row_height = mb_print::settings::RowHeight::Relaxed;
        })),
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

/// T14. The document survives JSON.
///
/// P08's preview is a sink on the other side of IPC, and D20 says nothing an
/// order can contain may serialise with a non-string map key. Prove the
/// document does not either.
#[test]
fn t14_the_document_crosses_ipc_unchanged() {
    let fixture = Fixture::new();
    let doc = bill_document(
        Paper::new(PaperKind::Mm80),
        &fixture.context(Copy::Original),
    )
    .expect("builds");

    let json = serde_json::to_string(&doc).expect("serialises");
    let back: mb_print::Document = serde_json::from_str(&json).expect("deserialises");
    assert_eq!(doc, back);

    // And the laid-out form too, since a preview may reasonably want that
    // rather than re-laying it out in JavaScript — which would be a second
    // layout engine, which is audit D1 again.
    let laid = layout(&doc).expect("lays out");
    let json = serde_json::to_string(&laid).expect("serialises");
    let back: mb_print::Laid = serde_json::from_str(&json).expect("deserialises");
    assert_eq!(laid, back);
}
