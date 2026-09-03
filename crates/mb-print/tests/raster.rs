#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    reason = "tests: expect is the assertion, and dots are not money"
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

fn metrics(paper: Paper) -> mb_print::metrics::Metrics {
    mb_print::metrics::Metrics::face(paper, std::sync::Arc::new(font()))
}

/// THE ANTI-DRIFT TEST GAINS ITS THIRD SINK.
#[test]
fn t1_the_raster_sink_cannot_drop_anything_either() {
    let fixture = Fixture::new();
    let ctx = fixture.context(Copy::Duplicate { number: 2 });
    let doc = bill_document(&common::metrics(PaperKind::Mm80), &ctx).expect("builds");
    let laid = layout(&doc).expect("lays out");

    let mut recorder = Recorder::new();
    render(&laid, &mut recorder);
    let raster = to_raster(&laid, &metrics(laid.paper), RasterOptions::default()).expect("rasters");

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

/// RASTER AND TEXT AGREE ABOUT THE GRID.
#[test]
fn t13_the_two_sinks_put_the_same_character_in_the_same_column() {
    let paper = Paper::new(PaperKind::Mm80);
    let mut doc = Document::new(paper);
    // Spaces in the middle on purpose: the columns that must be blank are the half of the claim
    // that catches an off-by-one.
    doc.text("AB  CD  240.00", Style::NORMAL, Align::Left);

    // One `Metrics` for both sinks, and the column is that face's own advance.
    let m = metrics(paper);
    let laid = mb_print::layout::layout_for(&doc, &m).expect("lays out");
    let text = to_text(&laid);
    let raster = to_raster(&laid, &m, RasterOptions::default()).expect("rasters");

    let per_column = m.body().advance;
    let line: Vec<char> = text.lines().next().expect("a line").chars().collect();

    let image = first_ink(&raster);
    for (column, ch) in line.iter().enumerate() {
        let start = u32::try_from(column).expect("small") * per_column;
        let inked = (start..start + per_column).any(|x| (0..image.height).any(|y| image.ink(x, y)));

        if *ch == ' ' {
            // Checked over the middle of the cell rather than all of it, so a glyph whose
            // stroke reaches the edge of its own column does not fail this.
            let middle = (start + 2..start + per_column - 2)
                .any(|x| (0..image.height).any(|y| image.ink(x, y)));
            assert!(
                !middle,
                "column {column} is a space in the text and ink in the picture"
            );
        } else {
            assert!(
                inked,
                "column {column} is {ch:?} in the text and blank in the picture"
            );
        }
    }
}

/// A BROKEN LOGO DOES NOT BREAK A BILL.
#[test]
fn t19_a_logo_that_cannot_be_read_is_skipped_and_the_bill_still_prints() {
    let fixture = Fixture::new();
    let doc = bill_document(
        &common::metrics(PaperKind::Mm80),
        &fixture.context(Copy::Original),
    )
    .expect("builds");
    let laid = layout(&doc).expect("lays out");
    let raster = to_raster(&laid, &metrics(laid.paper), RasterOptions::default()).expect("rasters");

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
    let raster = to_raster(&laid, &metrics(laid.paper), RasterOptions::default()).expect("rasters");
    assert!(raster.notes.is_empty(), "a good logo was refused");

    // 50 % of 576 dots is 288 wide, and a 64x32 source scales to 144 tall.
    assert_eq!(raster.height(), 144);
    let image = first_ink(&raster);
    let inked: u32 = (0..image.width)
        .map(|x| u32::from((0..image.height).any(|y| image.ink(x, y))))
        .sum();
    assert_eq!(inked, 288, "the logo is not half the width of the paper");
}

/// The QR goes to the printer's own encoder when it has one, and is drawn as modules when it
/// does not — the same square, at the same module size, either way.
#[test]
fn the_qr_is_the_printers_when_it_has_one_and_modules_when_it_does_not() {
    let payload = "upi://pay?pa=anna@upi&am=646.00";
    let mut doc = Document::new(Paper::new(PaperKind::Mm80));
    doc.push(mb_print::doc::Block::QrCode {
        payload: payload.to_owned(),
        width_pct: 40,
        align: Align::Centre,
    });
    let laid = layout(&doc).expect("lays out");

    let native = to_raster(&laid, &metrics(laid.paper), RasterOptions::default()).expect("rasters");
    let module = native
        .bands
        .iter()
        .find_map(|b| match b {
            Band::Qr { module, .. } => Some(*module),
            _ => None,
        })
        .expect("a printer with an encoder should be sent the payload, not a picture");
    // Two-fifths of 576 dots is 230; a 27-module symbol comes to 8 dots a module.
    let modules = mb_print::codes::qr(payload).expect("encodes");
    assert_eq!(module, modules.module_for(230));
    assert!(module > 6, "the setting's width was ignored: module {module}");

    let drawn = to_raster(&laid, &metrics(laid.paper), RasterOptions::drawn()).expect("rasters");
    assert!(drawn.notes.is_empty(), "{:?}", drawn.notes);
    assert!(
        drawn.bands.iter().all(|b| matches!(b, Band::Ink { .. })),
        "a screen has no encoder, so every band is ink"
    );
    // A real square: the top-left finder pattern is a solid 7×7 ring of modules, so its
    // corner dot is ink and the dot one module inside the ring is not.
    let side = modules.size * u32::from(module);
    let image = first_ink(&drawn);
    let left = (576 - side) / 2;
    let top = (image.height - side) / 2;
    assert!(image.ink(left, top), "the finder pattern's corner is blank");
    assert!(
        !image.ink(left + u32::from(module), top + u32::from(module)),
        "the finder pattern's inner ring is inked"
    );
    assert!(drawn.ink.iter().any(|l| l.dots > 1_000), "{:?}", drawn.ink);
}

/// The setting's width reaches the paper: a wider setting is a bigger module.
#[test]
fn the_qr_width_setting_changes_the_module_size_on_both_engines() {
    let payload = "upi://pay?pa=anna@upi&pn=Anna&am=646.00&cu=INR";
    let module_at = |pct: u8| {
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.push(mb_print::doc::Block::QrCode {
            payload: payload.to_owned(),
            width_pct: pct,
            align: Align::Centre,
        });
        let laid = layout(&doc).expect("lays out");
        let raster =
            to_raster(&laid, &metrics(laid.paper), RasterOptions::default()).expect("rasters");
        let native = raster
            .bands
            .iter()
            .find_map(|b| match b {
                Band::Qr { module, .. } => Some(*module),
                _ => None,
            })
            .expect("a QR band");
        // The text engine is told the same number.
        let text = mb_print::escpos::encode_text(
            &laid,
            &mb_print::printer::Capabilities::default(),
            &mb_print::escpos::JobOptions::default(),
        );
        let at = text
            .windows(7)
            .position(|w| w == [0x1D, 0x28, 0x6B, 3, 0, 49, 67])
            .expect("GS ( k 67 sets the module size");
        assert_eq!(text[at + 7], native, "the two engines disagree at {pct} %");
        // And the drawn square is that many dots a module.
        let drawn = to_raster(&laid, &metrics(laid.paper), RasterOptions::drawn()).expect("rasters");
        let modules = mb_print::codes::qr(payload).expect("encodes");
        assert_eq!(
            drawn.ink.first().map(|l| l.dots),
            Some(u32::from(native) * u32::from(native) * dark_count(&modules)),
            "the drawn square is not {native} dots a module"
        );
        native
    };
    assert!(module_at(20) < module_at(40));
    assert!(module_at(40) < module_at(80));
    assert_eq!(module_at(100), 16, "the printer's own limit");
}

fn dark_count(modules: &mb_print::codes::Modules) -> u32 {
    let mut n = 0;
    for y in 0..modules.size {
        for x in 0..modules.size {
            if modules.dark(x, y) {
                n += 1;
            }
        }
    }
    n
}

/// The barcode reaches the paper on the graphics engine — as the printer's own command when
/// it has one, and as bars when it does not.
#[test]
fn the_barcode_is_printed_by_the_raster_engine() {
    let mut doc = Document::new(Paper::new(PaperKind::Mm80));
    doc.line("TOTAL 646.00");
    doc.push(mb_print::doc::Block::Barcode {
        payload: "BIR1207".to_owned(),
        human_readable: true,
        align: Align::Centre,
    });
    doc.line("Thank you");
    let laid = layout(&doc).expect("lays out");

    let native = to_raster(&laid, &metrics(laid.paper), RasterOptions::default()).expect("rasters");
    assert!(
        native.bands.iter().any(|b| matches!(
            b,
            Band::Barcode { payload, human_readable: true, .. } if payload == "BIR1207"
        )),
        "the barcode band is missing: {:?}",
        native.bands.len()
    );
    // And it is sent: `GS k 73` with the payload, between the two ink bands.
    let bytes = mb_print::escpos::encode_raster(
        &native,
        &mb_print::printer::Capabilities::default(),
        &mb_print::escpos::JobOptions::default(),
    );
    let at = bytes
        .windows(4)
        .position(|w| w == [0x1D, 0x6B, 73, 9])
        .expect("GS k 73 is on the wire");
    assert_eq!(&bytes[at + 4..at + 6], b"{B");
    assert_eq!(&bytes[at + 6..at + 13], b"BIR1207");
    let rasters = bytes
        .windows(3)
        .filter(|w| *w == [0x1D, 0x76, 0x30])
        .count();
    assert_eq!(rasters, 2, "the text before and after the bars are two pictures");

    // A printer with no `GS k` gets bars drawn: Code 128 starts with a two-dot bar after the
    // quiet zone, and the number is under it.
    let drawn = to_raster(&laid, &metrics(laid.paper), RasterOptions::drawn()).expect("rasters");
    assert!(drawn.notes.is_empty(), "{:?}", drawn.notes);
    assert_eq!(drawn.bands.len(), 1, "everything is one picture");
    let bars = drawn.dots_for_line(1).expect("the barcode's ink");
    // Sixty rows of bars for ten symbols is thousands of dots; the text under them is a few
    // hundred more.
    assert!(bars > 3_000, "only {bars} dots of barcode");
    let image = first_ink(&drawn);
    let widths = mb_print::codes::code128("BIR1207").expect("encodes");
    let wide = mb_print::codes::barcode_width(&widths);
    let left = (576 - wide) / 2 + mb_print::codes::barcode_quiet();
    let top = laid.lines[0].row_dots;
    // Start B opens with a two-unit bar, and a unit is two dots.
    assert!(image.ink(left, top + 30), "the first bar is blank");
    assert!(image.ink(left + 3, top + 30), "the first bar is narrower than two units");
    assert!(!image.ink(left + 4, top + 30), "the first space is inked");
    assert!(!image.ink(left - 1, top + 30), "the quiet zone is inked");
}

/// A bill number a barcode cannot carry is printed as characters, and the sink says so.
#[test]
fn a_barcode_that_cannot_be_drawn_becomes_its_characters() {
    let mut doc = Document::new(Paper::new(PaperKind::Mm80));
    doc.push(mb_print::doc::Block::Barcode {
        payload: "BIR/\u{20B9}".to_owned(),
        human_readable: true,
        align: Align::Centre,
    });
    let laid = layout(&doc).expect("lays out");
    let drawn = to_raster(&laid, &metrics(laid.paper), RasterOptions::drawn()).expect("rasters");
    assert!(drawn.notes.contains(&RasterNote::BarcodeAsText));
    assert!(drawn.ink.iter().any(|l| l.dots > 0));
}

/// The queue and the preview resolve a printer through the one function.
#[test]
fn a_printer_is_drawn_the_same_way_wherever_it_is_asked() {
    use mb_print::printer::{Engine, PrinterConfig, Target};
    let faces = mb_print::font::OneFace::builtin().expect("loads");

    let mut printer = PrinterConfig::new("p1", "Counter", Target::None);
    let drawing = mb_print::drawing_for(&printer, Some("consolas"), &faces);
    assert_eq!(drawing.engine, Engine::Raster);
    assert!(drawing.metrics.font().is_some());
    assert_eq!(drawing.metrics.paper(), printer.paper);
    // A file or a null target has no encoders, so the sink draws the codes itself.
    assert_eq!(drawing.raster, RasterOptions::drawn());

    printer.caps.raster = false;
    let drawing = mb_print::drawing_for(&printer, None, &faces);
    assert_eq!(drawing.engine, Engine::Text);
    assert!(
        drawing.metrics.font().is_none(),
        "the text engine lays out with the printer's own font"
    );

    let real = PrinterConfig::new("p2", "Counter", Target::Network {
        host: "192.168.1.9".to_owned(),
        port: 9100,
    });
    assert_eq!(
        mb_print::drawing_for(&real, None, &faces).raster,
        RasterOptions::default()
    );
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
        let raster =
            to_raster(&laid, &metrics(laid.paper), RasterOptions::default()).expect("rasters");
        assert_eq!(first_ink(&raster).width, dots, "{kind:?}");
    }
}

fn first_ink(raster: &mb_print::raster::Raster) -> &Monochrome {
    raster
        .bands
        .iter()
        .find_map(|b| match b {
            Band::Ink { image } => Some(image),
            _ => None,
        })
        .expect("something was drawn")
}

/// Not a test of the raster sink so much as of the claim in its module docs: laying a document
/// out never changes it, so two sinks over one `Laid` cannot be looking at different documents.
#[test]
fn laying_out_twice_gives_the_same_answer() {
    let fixture = Fixture::new();
    let doc = bill_document(
        &common::metrics(PaperKind::Mm80),
        &fixture.context(Copy::Original),
    )
    .expect("builds");
    let first: Laid = layout(&doc).expect("lays out");
    let second: Laid = layout(&doc).expect("lays out");
    assert_eq!(first, second);
}

// Sizes between the printer's own three.

/// Lay one line out at a size and report how tall the picture came out and how many characters
/// the layout let onto the line.
fn at_size(cap: u16, text: &str) -> (u32, usize) {
    let mut doc = Document::new(Paper::new(PaperKind::Mm80));
    doc.text(
        text,
        Style {
            size: cap,
            bold: false,
        },
        Align::Left,
    );
    let laid = layout(&doc).expect("lays out");
    let raster = to_raster(&laid, &metrics(laid.paper), RasterOptions::default()).expect("rasters");
    let longest = laid
        .lines
        .iter()
        .filter_map(|l| match &l.content {
            LaidContent::Text { text } => Some(text.trim_end().chars().count()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    (raster.height(), longest)
}

/// A size between the multiples draws between the multiples.
#[test]
fn every_size_draws_a_different_height() {
    let mut last = 0;
    for px in [12_u16, 14, 16, 18, 20, 22, 24, 32, 48] {
        let (rows, _) = at_size(px, "Masala Dosa");
        assert!(
            rows > last,
            "{px} px drew {rows} rows, no taller than the size below it ({last})"
        );
        last = rows;
    }
}

/// Words that wrap, so nothing is capped.
fn wrappable() -> String {
    "AAA ".repeat(60)
}

/// And a smaller size fits more on a line.
#[test]
fn a_smaller_size_fits_more_characters_across() {
    let (_, wide) = at_size(Style::LADDER[3], &wrappable());
    let (_, narrow) = at_size(Style::LADDER[0], &wrappable());

    assert!(
        narrow > wide,
        "12 px fitted {narrow} characters and 24 px fitted {wide} — the smaller \
         size did not fit more"
    );
    // Rung 1 is a 9-dot capital against rung 4's 15, so about half the width and about half
    // again as many characters.
    assert!(
        narrow * 3 >= wide * 4,
        "the small size fitted {narrow}, which is not meaningfully more than {wide}"
    );
}

#[test]
fn the_old_three_sizes_are_unchanged() {
    for (stored, rung, scale) in [(24_u16, 2_usize, 1_u8), (48, 7, 2), (72, 9, 3)] {
        let cap = Style::from_stored(stored);
        assert_eq!(cap, Style::LADDER[rung], "{stored} moved off its rung");
        assert_eq!(
            Style {
                size: cap,
                bold: false
            }
            .scale(),
            scale,
            "{stored} stopped being {scale}x for the text engine"
        );
    }
    // And the layout still gives them the widths they always had: 48 columns on 80 mm paper at
    // the body size, and fewer as the letter grows.
    for (stored, limit) in [(24_u16, 52_usize), (48, 26), (72, 16)] {
        let longest = at_size(Style::from_stored(stored), &wrappable()).1;
        assert!(
            longest <= limit && longest + 4 >= limit,
            "a stored {stored} filled {longest} of {limit} columns"
        );
    }
}

// Proportional faces.

/// Times New Roman, or `None` on a machine that does not have it.
fn proportional() -> Option<Font> {
    let path = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()),
    )
    .join("Fonts")
    .join("times.ttf");
    let bytes = std::fs::read(path).ok()?;
    Font::load(&bytes, "Times New Roman").ok()
}

/// The amount still ends where the paper ends.
#[test]
fn a_proportional_face_still_puts_the_amount_against_the_right_edge() {
    let Some(font) = proportional() else { return };
    let paper = Paper::new(PaperKind::Mm80);

    let mut doc = Document::new(paper);
    doc.row("Subtotal", "920.00", Style::NORMAL)
        .row("Grand Total", "1,240.00", Style::NORMAL);
    // One `Metrics` for the layout AND the sink.
    let metrics = mb_print::metrics::Metrics::face(paper, std::sync::Arc::new(font));
    let laid = mb_print::layout::layout_for(&doc, &metrics).expect("lays out");
    let raster = to_raster(&laid, &metrics, RasterOptions::default()).expect("rasters");

    // The rightmost dot of each line, which is the last digit of the amount.
    let edges: Vec<u32> = raster
        .bands
        .iter()
        .filter_map(|b| match b {
            Band::Ink { image } => Some(image),
            _ => None,
        })
        .flat_map(|image| {
            (0..image.height).filter_map(move |y| (0..image.width).rev().find(|x| image.ink(*x, y)))
        })
        .collect();

    let furthest = edges.iter().copied().max().expect("something was drawn");
    let dots = paper.kind.dots().expect("a thermal roll has dots");
    assert!(
        furthest + 12 >= dots && furthest < dots,
        "the amounts end at {furthest} of {dots} dots — not against the right edge"
    );
}

/// And it really is proportional, rather than the grid wearing a new face.
#[test]
fn a_proportional_face_is_drawn_proportionally() {
    let Some(font) = proportional() else { return };
    let font = std::sync::Arc::new(font);

    let ink_width = |text: &str| {
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.text(text, Style::NORMAL, Align::Left);
        let metrics = mb_print::metrics::Metrics::face(doc.paper, std::sync::Arc::clone(&font));
        let laid = mb_print::layout::layout_for(&doc, &metrics).expect("lays out");
        let raster = to_raster(&laid, &metrics, RasterOptions::default()).expect("rasters");
        raster
            .bands
            .iter()
            .filter_map(|b| match b {
                Band::Ink { image } => Some(image),
                _ => None,
            })
            .flat_map(|image| {
                (0..image.height)
                    .filter_map(move |y| (0..image.width).rev().find(|x| image.ink(*x, y)))
            })
            .max()
            .unwrap_or(0)
    };

    assert!(
        ink_width("iiiiii") < ink_width("MMMMMM"),
        "six thin letters took as much room as six fat ones — the face is being \
         drawn on a grid"
    );
}

/// A typewriter face is untouched.
#[test]
fn the_shipped_face_still_takes_the_grid_path() {
    assert!(
        font().is_monospace(),
        "the built-in face stopped being a typewriter one"
    );
    if let Some(times) = proportional() {
        assert!(
            !times.is_monospace(),
            "Times New Roman was taken for a typewriter face"
        );
    }
}
