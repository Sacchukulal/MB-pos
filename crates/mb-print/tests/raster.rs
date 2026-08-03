//! **T1 (extended), T13 and T19 — the third sink.**
//!
//! P06 shipped two sinks and left a seat for this one (D31). The claim it has to
//! join is D29's: *"a sink cannot forget: it is handed everything, in order."*
//! A bitmap has no text to search, so the claim is made where it is true —
//! every line the traversal handed over produced dots, and a line it called
//! blank produced none.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Fixture;
use mb_print::doc::{Align, Document, Style};
use mb_print::font::Font;
use mb_print::image::Monochrome;
use mb_print::layout::{Laid, LaidContent, layout};
use mb_print::paper::{Paper, PaperKind};
use mb_print::raster::{Band, RasterNote, RasterOptions, to_raster};
use mb_print::render::{Recorder, render};
use mb_print::template::{Copy, bill_document};
use mb_print::text::to_text;

fn font() -> Font {
    Font::builtin().expect("the shipped face loads")
}

/// T1. **THE ANTI-DRIFT TEST GAINS ITS THIRD SINK.**
///
/// Every block of a bill with everything on it reaches the raster sink, and
/// every one that has something to say produces ink.
#[test]
fn t1_the_raster_sink_cannot_drop_anything_either() {
    let fixture = Fixture::new();
    let ctx = fixture.context(Copy::Duplicate { number: 2 });
    let doc = bill_document(Paper::new(PaperKind::Mm80), &ctx).expect("builds");
    let laid = layout(&doc).expect("lays out");

    let mut recorder = Recorder::new();
    render(&laid, &mut recorder);
    let raster = to_raster(&laid, &font(), RasterOptions::default()).expect("rasters");

    // The traversal handed over every line, and the sink acknowledged each one.
    assert_eq!(
        raster.ink.len(),
        laid.lines.len(),
        "the raster sink was not handed every line — that is exactly the drift \
         D29 exists to make impossible"
    );
    assert_eq!(
        recorder.calls.len(),
        laid.lines.len(),
        "the recorder and the traversal disagree about how many blocks there are"
    );

    // And everything with something to say left a mark.
    let mut with_ink = 0;
    for (index, line) in laid.lines.iter().enumerate() {
        let dots = raster
            .dots_for_line(index)
            .unwrap_or_else(|| panic!("line {index} never reached the raster sink"));
        match &line.content {
            LaidContent::Text { text } if !text.trim().is_empty() => {
                assert!(dots > 0, "line {index} ({text:?}) printed nothing");
                with_ink += 1;
            }
            LaidContent::Separator { .. } => {
                assert!(dots > 0, "a separator printed nothing");
                with_ink += 1;
            }
            LaidContent::Blank => assert_eq!(dots, 0, "a blank line printed something"),
            _ => {}
        }
    }
    assert!(
        with_ink > 20,
        "only {with_ink} lines had ink — the fixture is too thin to prove anything"
    );

    // A receipt of this length is a real height of paper, not a stub.
    assert!(
        raster.height() > 500,
        "an 80 mm bill with everything on it came to {} dots",
        raster.height()
    );
}

/// T13. **RASTER AND TEXT AGREE ABOUT THE GRID.**
///
/// D29's claim, extended from characters to dots. If the two sinks ever
/// disagree about which column a character sits in, "the preview does not match
/// the paper" is back by a different route.
#[test]
fn t13_the_two_sinks_put_the_same_character_in_the_same_column() {
    let paper = Paper::new(PaperKind::Mm80);
    let mut doc = Document::new(paper);
    // Spaces in the middle on purpose: the columns that must be blank are the
    // half of the claim that catches an off-by-one.
    doc.text("AB  CD  240.00", Style::NORMAL, Align::Left);

    let laid = layout(&doc).expect("lays out");
    let text = to_text(&laid);
    let raster = to_raster(&laid, &font(), RasterOptions::default()).expect("rasters");

    let per_column = paper.dots_per_column().expect("thermal paper has dots");
    let line: Vec<char> = text.lines().next().expect("a line").chars().collect();

    let image = first_ink(&raster);
    for (column, ch) in line.iter().enumerate() {
        let start = u32::try_from(column).expect("small") * per_column;
        let inked = (start..start + per_column)
            .any(|x| (0..image.height).any(|y| image.ink(x, y)));

        if *ch == ' ' {
            // Checked over the middle of the cell rather than all of it, so a
            // glyph whose stroke reaches the edge of its own column does not
            // fail this. What must be true is that nothing was *drawn* here.
            let middle = (start + 2..start + per_column - 2)
                .any(|x| (0..image.height).any(|y| image.ink(x, y)));
            assert!(!middle, "column {column} is a space in the text and ink in the picture");
        } else {
            assert!(
                inked,
                "column {column} is {ch:?} in the text and blank in the picture"
            );
        }
    }
}

/// T19. **A BROKEN LOGO DOES NOT BREAK A BILL** (D37).
///
/// The shared fixture's "logo" is eight fake PNG bytes, so this is not
/// hypothetical: it is what a shop that uploaded the wrong thing has.
#[test]
fn t19_a_logo_that_cannot_be_read_is_skipped_and_the_bill_still_prints() {
    let fixture = Fixture::new();
    let doc = bill_document(
        Paper::new(PaperKind::Mm80),
        &fixture.context(Copy::Original),
    )
    .expect("builds");
    let laid = layout(&doc).expect("lays out");
    let raster = to_raster(&laid, &font(), RasterOptions::default()).expect("rasters");

    assert!(
        raster
            .notes
            .iter()
            .any(|n| matches!(n, RasterNote::LogoSkipped { .. })),
        "a logo that is not one of ours must say so"
    );
    assert!(
        raster.ink.iter().map(|l| l.dots).sum::<u32>() > 1_000,
        "the bill printed nothing because the logo was wrong"
    );
}

/// A logo that IS one of ours is drawn, scaled to the width it asked for.
#[test]
fn a_real_logo_is_drawn_at_the_width_it_asked_for() {
    let paper = Paper::new(PaperKind::Mm80);
    let mut logo = Monochrome::blank(64, 32);
    for y in 0..32 {
        for x in 0..64 {
            logo.set(x, y);
        }
    }

    let mut doc = Document::new(paper);
    doc.push(mb_print::doc::Block::Image {
        data: logo.encode(),
        width_pct: 50,
        align: Align::Centre,
    });

    let laid = layout(&doc).expect("lays out");
    let raster = to_raster(&laid, &font(), RasterOptions::default()).expect("rasters");
    assert!(raster.notes.is_empty(), "a good logo was refused");

    // 50 % of 576 dots is 288 wide, and a 64x32 source scales to 144 tall.
    assert_eq!(raster.height(), 144);
    let image = first_ink(&raster);
    let inked: u32 = (0..image.width)
        .map(|x| u32::from((0..image.height).any(|y| image.ink(x, y))))
        .sum();
    assert_eq!(inked, 288, "the logo is not half the width of the paper");
}

/// The QR goes to the printer's own encoder when it has one, and becomes text
/// when it does not (D36).
#[test]
fn the_qr_is_the_printers_when_it_has_one_and_text_when_it_does_not() {
    let mut doc = Document::new(Paper::new(PaperKind::Mm80));
    doc.push(mb_print::doc::Block::QrCode {
        payload: "upi://pay?pa=anna@upi&am=646.00".to_owned(),
        width_pct: 40,
        align: Align::Centre,
    });
    let laid = layout(&doc).expect("lays out");

    let native = to_raster(&laid, &font(), RasterOptions::default()).expect("rasters");
    assert!(
        native.bands.iter().any(|b| matches!(b, Band::Qr { .. })),
        "a printer with an encoder should be sent the payload, not a picture"
    );

    let drawn = to_raster(
        &laid,
        &font(),
        RasterOptions {
            native_qr: false,
            ..RasterOptions::default()
        },
    )
    .expect("rasters");
    assert!(
        drawn.notes.contains(&RasterNote::QrAsText),
        "a printer with no encoder should print the payload as text — a URI a \
         customer can type beats a blank space"
    );
    assert!(drawn.ink.iter().any(|l| l.dots > 0));
}

/// Every thermal paper size rasterises to its own dot width.
#[test]
fn each_paper_size_rasterises_to_its_own_width() {
    for (kind, dots) in [
        (PaperKind::Mm58, 384),
        (PaperKind::Mm80, 576),
        (PaperKind::Mm100, 832),
    ] {
        let mut doc = Document::new(Paper::new(kind));
        doc.line("TOTAL 646.00");
        let laid = layout(&doc).expect("lays out");
        let raster = to_raster(&laid, &font(), RasterOptions::default()).expect("rasters");
        assert_eq!(first_ink(&raster).width, dots, "{kind:?}");
    }
}

fn first_ink(raster: &mb_print::raster::Raster) -> &Monochrome {
    raster
        .bands
        .iter()
        .find_map(|b| match b {
            Band::Ink { image } => Some(image),
            Band::Qr { .. } => None,
        })
        .expect("something was drawn")
}

/// Not a test of the raster sink so much as of the claim in its module docs:
/// laying a document out never changes it, so two sinks over one `Laid` cannot
/// be looking at different documents.
#[test]
fn laying_out_twice_gives_the_same_answer() {
    let fixture = Fixture::new();
    let doc = bill_document(
        Paper::new(PaperKind::Mm80),
        &fixture.context(Copy::Original),
    )
    .expect("builds");
    let first: Laid = layout(&doc).expect("lays out");
    let second: Laid = layout(&doc).expect("lays out");
    assert_eq!(first, second);
}
