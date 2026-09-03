//! What the on-screen bill preview is handed.
//!
//! Two pictures, for two engines. The graphics engine's preview is the printer's own raster —
//! `mb_print::raster::to_raster` on exactly the layout and metrics the queue prints with, every
//! dot — so the screen and the paper cannot differ. The text engine prints with the printer's
//! ROM font, which no screen has, so its preview stays a structured list of rows the browser
//! draws in a monospace face, with separators as the character row the printer prints.

// Dots into percentages and dots back into characters.
#![allow(clippy::integer_division, reason = "dots and characters, not money")]

use base64::Engine as _;
use mb_print::doc::Align;
use mb_print::layout::{Laid, LaidContent, LaidLine};
use mb_print::metrics::Metrics;
use mb_print::raster::{Band, RasterNote, RasterOptions, to_raster};
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PreviewDoc {
    /// Printable dots across. The preview is exactly as wide as the paper, not "about right".
    pub dots: u32,
    /// The roll's whole width in millimetres, and how much of it the head can reach. The
    /// screen draws the difference as the paper's own margin, so the preview is the receipt
    /// in the hand and not a strip of ink with no edge.
    pub paper_mm: u32,
    pub printable_mm: u32,
    /// Characters across at the body size — what the settings screen tells a shop it is
    /// choosing when it picks a size.
    pub columns: usize,
    /// The document as the text engine prints it. The screen draws these only when `raster`
    /// is absent; a test reads them for either engine.
    pub lines: Vec<PreviewLine>,
    /// How much roll this costs.
    pub millimetres: u32,
    /// `raster` or `text` — which engine this preview is showing.
    pub engine: String,
    /// Anything the layout or the sink had to do that a person might want to know — a size
    /// that had to come down, an offset that was clamped, a logo that would not read.
    pub notes: Vec<String>,
    /// The paper, dot for dot, when the graphics engine will print it.
    pub raster: Option<PreviewRaster>,
}

/// The printer's raster, as the screen draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PreviewRaster {
    /// Dots across — the paper's.
    pub width: u32,
    /// Dot rows, top to bottom.
    pub height: u32,
    /// The rows, packed one bit a dot — `ceil(width / 8)` bytes a row, most significant bit
    /// leftmost, a set bit is ink — which is exactly what `GS v 0` is sent, as base64. Eight
    /// times smaller than a byte a dot, and a third the size of the same bytes as a JSON array
    /// of numbers.
    pub bits: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PreviewLine {
    /// Already wrapped, already padded to its alignment, already offset.
    Text {
        text: String,
        /// Dots from the left edge of the paper.
        indent: u32,
        /// Dots of roll this row spends, top to bottom.
        row: u32,
        /// The height of a capital letter, in dots — what the shop chose and what the printer
        /// will draw.
        cap: u16,
        /// One character's advance, in dots.
        advance: u32,
        /// 1, 2 or 3 — what the TEXT print engine will emit.
        scale: u8,
        bold: bool,
        /// The aligned boxes on this line, in characters.
        segments: Vec<PreviewSegment>,
    },
    /// A separator, as the text engine prints it: the pattern's character, repeated across.
    Rule {
        glyphs: String,
        /// Dots from the left edge.
        indent: u32,
        /// Dots of roll the row spends — a character row, because that is what prints.
        row: u32,
        /// One character's advance, in dots.
        advance: u32,
    },
    /// The printer draws a real square; the screen draws a placeholder of the same size,
    /// because a shop tuning its letterhead needs to see how much paper it takes.
    Qr {
        payload: String,
        indent: u32,
        row: u32,
        /// Dots across, so the square on screen is the square on paper.
        size: u32,
    },
    /// The printer draws the bars; the screen draws a placeholder of the same height.
    Barcode {
        payload: String,
        indent: u32,
        row: u32,
        /// Dots tall.
        height: u32,
    },
    Blank {
        row: u32,
    },
}

/// One aligned box on a line — `mb_print::layout::Segment`, for the screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PreviewSegment {
    /// The text inside the box, already trimmed.
    pub text: String,
    /// How many characters wide the box is.
    pub width: usize,
    /// `left`, `right` or `centre`.
    pub align: String,
}

/// The one conversion. `engine` is the word `Engine::name` gives for the printer this is a
/// preview of; the raster is drawn only for the graphics engine, because the text engine's
/// paper is drawn by the printer's own font and there is no raster to show.
#[must_use]
pub fn to_preview(laid: &Laid, metrics: &Metrics, engine: &str) -> PreviewDoc {
    let mut lines = Vec::with_capacity(laid.lines.len());
    let mut notes: Vec<String> = laid.notes.iter().map(describe).collect();

    for line in &laid.lines {
        match &line.content {
            LaidContent::Text { text } => {
                lines.push(text_line(line, text, &boxes(text, &line.segments), metrics));
            }
            LaidContent::Separator { pattern, width } => {
                // The text engine's row: the printer's own character, at the body size.
                let body = metrics.body();
                lines.push(PreviewLine::Rule {
                    glyphs: pattern
                        .glyph()
                        .to_string()
                        .repeat(laid.columns_of(*width).max(1)),
                    indent: line.indent_dots,
                    row: body.row,
                    advance: body.advance,
                });
            }
            LaidContent::QrCode {
                payload, width_pct, ..
            } => {
                let usable = metrics.dots().saturating_sub(line.indent_dots).max(1);
                lines.push(PreviewLine::Qr {
                    payload: payload.clone(),
                    indent: line.indent_dots,
                    row: line.row_dots,
                    size: mb_print::codes::qr_side(usable, *width_pct),
                });
            }
            LaidContent::Barcode { payload, .. } => lines.push(PreviewLine::Barcode {
                payload: payload.clone(),
                indent: line.indent_dots,
                row: line.row_dots,
                height: mb_print::codes::BAR_HEIGHT,
            }),
            // The printer's own font cannot draw a picture, and prints nothing for one. The
            // raster shows it as the dots it is.
            LaidContent::Image { .. } => {}
            // The letterhead's text, one row each, the way the text engine prints it — the
            // picture beside it is in the raster.
            LaidContent::Band { lines: band, .. } => {
                for text in band {
                    let usable = metrics.dots().saturating_sub(line.indent_dots).max(1);
                    let size = metrics.size(text.style);
                    let run = text.text.trim().to_owned();
                    let segments = vec![PreviewSegment {
                        text: run.clone(),
                        width: size.chars_across(usable),
                        align: align_name(text.align).to_owned(),
                    }];
                    lines.push(PreviewLine::Text {
                        text: run,
                        indent: line.indent_dots,
                        row: size.row,
                        cap: size.cap,
                        advance: size.advance,
                        scale: size.scale,
                        bold: text.style.bold,
                        segments,
                    });
                }
            }
            LaidContent::Blank => lines.push(PreviewLine::Blank { row: line.row_dots }),
        }
    }

    let raster = if engine == mb_print::printer::Engine::Raster.name() {
        raster_of(laid, metrics, &mut notes)
    } else {
        None
    };

    PreviewDoc {
        dots: metrics.dots(),
        paper_mm: laid.paper.kind.width_mm(),
        printable_mm: laid.paper.kind.printable_mm(),
        columns: metrics.body().chars_across(metrics.dots()),
        lines,
        millimetres: laid.total_mm(),
        engine: engine.to_owned(),
        notes,
        raster,
    }
}

fn text_line(
    line: &LaidLine,
    text: &str,
    segments: &[PreviewSegment],
    metrics: &Metrics,
) -> PreviewLine {
    let size = metrics.size(line.style);
    PreviewLine::Text {
        text: text.to_owned(),
        indent: line.indent_dots,
        row: line.row_dots,
        cap: size.cap,
        advance: size.advance,
        scale: size.scale,
        bold: line.style.bold,
        segments: segments.to_vec(),
    }
}

/// The printer's raster, with everything drawn as ink — the screen has no encoder to hand a
/// QR or a barcode to. On a printer without those encoders this IS the paper, byte for byte;
/// on one with them, every ink band is the same and the code is a command between two bands.
fn raster_of(laid: &Laid, metrics: &Metrics, notes: &mut Vec<String>) -> Option<PreviewRaster> {
    let raster = to_raster(laid, metrics, RasterOptions::drawn()).ok()?;
    let width = laid.paper.kind.dots()?;
    let mut bits = Vec::with_capacity(mb_print::image::Monochrome::stride(width) * 2_000);
    let mut height = 0;
    for band in &raster.bands {
        if let Band::Ink { image } = band {
            bits.extend_from_slice(&image.bits);
            height += image.height;
        }
    }
    notes.extend(raster.notes.iter().map(describe_ink));
    Some(PreviewRaster {
        width,
        height,
        bits: base64::engine::general_purpose::STANDARD.encode(bits),
    })
}

const fn align_name(align: Align) -> &'static str {
    match align {
        Align::Left => "left",
        Align::Centre => "centre",
        Align::Right => "right",
    }
}

/// The layout's boxes, with the text that is in each one.
fn boxes(text: &str, segments: &[mb_print::layout::Segment]) -> Vec<PreviewSegment> {
    let chars: Vec<char> = text.chars().collect();
    segments
        .iter()
        .map(|segment| {
            let end = segment.start.saturating_add(segment.width).min(chars.len());
            let run: String = if segment.start < end {
                chars[segment.start..end].iter().collect()
            } else {
                String::new()
            };
            PreviewSegment {
                text: run.trim().to_owned(),
                width: segment.width,
                align: align_name(segment.align).to_owned(),
            }
        })
        .collect()
}

/// A layout note, in words.
fn describe(note: &mb_print::layout::Note) -> String {
    use mb_print::layout::Note;
    match note {
        // Not "a heading": the same note comes from the item table, where the reason is that
        // the fixed columns had already eaten the paper.
        Note::ScaleCapped { asked, used } => format!(
            "Size {} does not fit this paper here, so it printed at {}.",
            crate::settings::size_label(*asked),
            crate::settings::size_label(*used)
        ),
        Note::OffsetClamped {
            asked_mm,
            used_dots,
        } => format!(
            "The print offset of {asked_mm:+} mm was too far, so it was limited \
             to {} mm.",
            used_dots / 8
        ),
        Note::LabelWrapped { label } => {
            format!("\"{label}\" was too long for one line, so it wrapped.")
        }
        Note::LogoUnreadable { reason } => {
            format!("Your logo could not be read, so it did not print: {reason}")
        }
    }
}

/// A raster sink's note, in words.
fn describe_ink(note: &RasterNote) -> String {
    match note {
        RasterNote::LogoSkipped { reason } => {
            format!("Your logo could not be read, so it did not print: {reason}")
        }
        RasterNote::QrAsText => {
            "The QR code's contents are too long for a QR code, so they printed as text."
                .to_owned()
        }
        RasterNote::BarcodeAsText => {
            "The bill number has a character a barcode cannot carry, so it printed as text."
                .to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_print::doc::{Block, Document, Style};
    use mb_print::layout::layout_for;
    use mb_print::paper::{Paper, PaperKind};

    fn metrics(kind: PaperKind) -> Metrics {
        let font = std::sync::Arc::new(mb_print::font::Font::default_face().expect("loads"));
        Metrics::face(Paper::new(kind), font)
    }

    /// A bill with everything the raster has to draw itself.
    fn everything(kind: PaperKind) -> Document {
        let mut doc = Document::new(Paper::new(kind));
        doc.text("ANNA KUTEERA", Style::new(2, true), Align::Centre)
            .separator(mb_print::doc::Pattern::Double)
            .row("Masala Dosa", "240.00", Style::NORMAL)
            .push(Block::Image {
                data: mb_print::image::Monochrome::blank(40, 20).encode(),
                width_pct: 30,
                align: Align::Centre,
            })
            .push(Block::QrCode {
                payload: "upi://pay?pa=anna@upi&am=240.00&cu=INR".to_owned(),
                width_pct: 40,
                align: Align::Centre,
            })
            .push(Block::Barcode {
                payload: "BIR1207".to_owned(),
                human_readable: true,
                align: Align::Centre,
            })
            .spacer(1);
        doc
    }

    #[test]
    fn every_line_of_the_layout_reaches_the_preview() {
        // The sink property, at this boundary: nothing may be dropped on the way to the screen
        // either.
        let m = metrics(PaperKind::Mm80);
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.text("ANNA KUTEERA", Style::new(2, true), Align::Centre)
            .separator(mb_print::doc::Pattern::Double)
            .row("Masala Dosa", "240.00", Style::NORMAL)
            .spacer(1);

        let laid = layout_for(&doc, &m).expect("lays out");
        let preview = to_preview(&laid, &m, "raster");

        assert_eq!(preview.lines.len(), laid.lines.len());
        assert_eq!(preview.dots, 576);
        assert!(preview.millimetres > 0);
    }

    #[test]
    fn the_preview_does_no_layout_of_its_own() {
        // Whatever the layout produced is what the preview carries, character for character.
        let m = metrics(PaperKind::Mm58);
        let mut doc = Document::new(Paper::new(PaperKind::Mm58));
        doc.row(
            "Paneer Butter Masala (Half) Extra Spicy",
            "1,240.00",
            Style::NORMAL,
        );

        let laid = layout_for(&doc, &m).expect("lays out");
        let preview = to_preview(&laid, &m, "raster");

        let from_layout = laid.text_lines();
        let from_preview: Vec<String> = preview
            .lines
            .iter()
            .filter_map(|l| match l {
                PreviewLine::Text { text, indent, .. } => {
                    Some(format!("{}{text}", " ".repeat(laid.columns_of(*indent))))
                }
                _ => None,
            })
            .collect();
        assert_eq!(from_layout, from_preview);
    }

    /// The preview carries the size the paper will draw, in the same unit.
    #[test]
    fn the_preview_carries_the_paper_s_own_dots() {
        let m = metrics(PaperKind::Mm80);
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.text("TOTAL", Style::new(2, true), Align::Left);
        let laid = layout_for(&doc, &m).expect("lays out");
        let preview = to_preview(&laid, &m, "raster");

        let PreviewLine::Text {
            cap, row, advance, ..
        } = &preview.lines[0]
        else {
            panic!("not text");
        };
        let expected = m.size(Style::new(2, true));
        assert_eq!(u32::from(*cap), u32::from(expected.cap));
        assert_eq!(*row, expected.row);
        assert_eq!(*advance, expected.advance);
        assert_eq!(*row, laid.lines[0].row_dots);
    }

    #[test]
    fn a_capped_size_is_explained_in_words() {
        let m = metrics(PaperKind::Mm58);
        let mut doc = Document::new(Paper::new(PaperKind::Mm58));
        doc.text(
            "ANNAPOORNESHWARIREFRESHMENTS",
            Style {
                size: Style::LARGEST,
                bold: true,
            },
            Align::Centre,
        );
        let laid = layout_for(&doc, &m).expect("lays out");
        let preview = to_preview(&laid, &m, "raster");
        assert!(
            preview.notes.iter().any(|n| n.contains("does not fit")),
            "{:?}",
            preview.notes
        );
    }

    /// The anti-drift property at its strongest: the screen's raster IS the printer's raster,
    /// byte for byte, for the same layout and the same metrics — QR, barcode and logo
    /// included, drawn by the one sink.
    #[test]
    fn the_raster_preview_is_the_printers_raster_byte_for_byte() {
        for kind in [PaperKind::Mm58, PaperKind::Mm80, PaperKind::Mm100] {
            let m = metrics(kind);
            let laid = layout_for(&everything(kind), &m).expect("lays out");
            let preview = to_preview(&laid, &m, "raster");
            let raster = preview.raster.expect("the graphics engine has a raster");

            // What the queue would send a printer with no encoders of its own.
            let printed = to_raster(&laid, &m, RasterOptions::drawn()).expect("rasters");
            let mut bytes = Vec::new();
            for band in &printed.bands {
                let Band::Ink { image } = band else {
                    panic!("a drawn raster has only ink");
                };
                assert_eq!(image.width, raster.width);
                bytes.extend_from_slice(&image.bits);
            }
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&raster.bits)
                .expect("base64");
            assert_eq!(decoded, bytes, "{kind:?}: the screen's dots are not the paper's");
            assert_eq!(raster.height, printed.height(), "{kind:?}");
            assert_eq!(
                decoded.len(),
                mb_print::image::Monochrome::stride(raster.width) * raster.height as usize
            );
            // And the QR and the bars are in it: a real square is a lot more ink than a row
            // of text.
            let ink: u32 = decoded.iter().map(|b| b.count_ones()).sum();
            assert!(ink > 4_000, "{kind:?}: only {ink} dots — the codes were not drawn");
        }
    }

    /// The text engine has no raster; it has the printer's character row where a rule is.
    #[test]
    fn the_text_engine_previews_rows_of_the_printers_own_characters() {
        let paper = Paper::new(PaperKind::Mm80);
        let m = Metrics::printer_font(paper);
        let laid = layout_for(&everything(PaperKind::Mm80), &m).expect("lays out");
        let preview = to_preview(&laid, &m, "text");
        assert!(preview.raster.is_none());

        let rule = preview
            .lines
            .iter()
            .find_map(|l| match l {
                PreviewLine::Rule { glyphs, row, .. } => Some((glyphs.clone(), *row)),
                _ => None,
            })
            .expect("the separator reaches the screen");
        // Forty-eight columns of `=`, one character row tall — what `encode_text` prints.
        assert_eq!(rule.0, "=".repeat(48));
        assert_eq!(rule.1, m.body().row);
        // The square the setting asked for, and the bars at their height.
        assert!(preview.lines.iter().any(|l| matches!(l, PreviewLine::Qr { size, .. } if *size == 230)));
        assert!(
            preview
                .lines
                .iter()
                .any(|l| matches!(l, PreviewLine::Barcode { height, .. } if *height == 60))
        );
    }

    /// A raster for the text engine would be a lie: it is not what that printer draws.
    #[test]
    fn the_text_engine_is_never_given_a_raster() {
        let m = metrics(PaperKind::Mm80);
        let laid = layout_for(&everything(PaperKind::Mm80), &m).expect("lays out");
        assert!(to_preview(&laid, &m, "text").raster.is_none());
        assert!(to_preview(&laid, &m, "raster").raster.is_some());
    }
}
