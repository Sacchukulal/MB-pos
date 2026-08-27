//! What the on-screen bill preview is handed — the fourth sink's input.

// Dots into percentages and dots back into characters.
#![allow(clippy::integer_division, reason = "dots and characters, not money")]

use mb_print::doc::Align;
use mb_print::layout::{Laid, LaidContent};
use mb_print::metrics::Metrics;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PreviewDoc {
    /// Printable dots across. The preview is exactly as wide as the paper, not "about right".
    pub dots: u32,
    /// Characters across at the body size — what the settings screen tells a shop it is
    /// choosing when it picks a size.
    pub columns: usize,
    pub lines: Vec<PreviewLine>,
    /// How much roll this costs.
    pub millimetres: u32,
    /// `raster` or `text` — which engine this preview is showing.
    pub engine: String,
    /// Anything the layout had to do that a person might want to know — a size that had to come
    /// down, an offset that was clamped.
    pub notes: Vec<String>,
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
    /// A drawn rule, not a row of characters.
    Rule {
        /// Dots from the left edge.
        indent: u32,
        /// Dots wide.
        width: u32,
        /// Dots of roll the whole row spends.
        row: u32,
        /// Dots thick, per stroke.
        thickness: u32,
        /// How many strokes — `Double` is two.
        strokes: u32,
        /// Dots between them. One word, so `serde` and `ts-rs` produce the same name on both
        /// sides without a rename attribute nobody would notice was missing.
        gap: u32,
        /// `on`/`off` dots for a dashed or dotted rule; `null` is continuous.
        dash: Option<Vec<u32>>,
    },
    /// The printer draws a real square; the screen draws one too, from the same payload,
    /// because a shop tuning its letterhead needs to see how much paper it takes.
    Qr {
        payload: String,
        indent: u32,
        row: u32,
        /// Dots across, so the square on screen is the square on paper.
        size: u32,
    },
    /// The printer draws the bars; the screen draws bars too.
    Barcode {
        payload: String,
        indent: u32,
        row: u32,
        /// Dots tall — `GS h` is set to 60 by `escpos`.
        height: u32,
    },
    /// A logo the raster sink will draw.
    Logo {
        indent: u32,
        row: u32,
        left: u32,
        width: u32,
        height: u32,
        /// One byte per dot, row by row, 1 is ink.
        ink: Option<Vec<u8>>,
    },
    /// The letterhead: a logo and the shop's name side by side.
    Band {
        row: u32,
        image: Box<PreviewLine>,
        lines: Vec<PreviewBandLine>,
    },
    Blank {
        row: u32,
    },
}

/// One line of a letterhead, already placed by the layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PreviewBandLine {
    pub text: String,
    /// Dots from the left edge of the paper.
    pub left: u32,
    /// Dots from the top of the band.
    pub top: u32,
    /// The box, in dots.
    pub width: u32,
    pub row: u32,
    pub cap: u16,
    pub bold: bool,
    pub align: String,
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

/// The one conversion.
#[must_use]
pub fn to_preview(laid: &Laid, metrics: &Metrics, engine: &str) -> PreviewDoc {
    let mut lines = Vec::with_capacity(laid.lines.len());

    for line in &laid.lines {
        let size = metrics.size(line.style);
        lines.push(match &line.content {
            LaidContent::Text { text } => PreviewLine::Text {
                text: text.clone(),
                indent: line.indent_dots,
                row: line.row_dots,
                cap: size.cap,
                advance: size.advance,
                scale: size.scale,
                bold: line.style.bold,
                segments: boxes(text, &line.segments),
            },
            LaidContent::Separator { pattern, width } => {
                let rule = mb_print::layout::Rule::of(*pattern);
                PreviewLine::Rule {
                    indent: line.indent_dots,
                    width: *width,
                    row: line.row_dots,
                    thickness: rule.thickness,
                    strokes: rule.strokes,
                    gap: rule.stroke_gap,
                    dash: rule.dash.map(|(on, off)| vec![on, off]),
                }
            }
            LaidContent::QrCode { payload, .. } => PreviewLine::Qr {
                payload: payload.clone(),
                indent: line.indent_dots,
                row: line.row_dots,
                size: line.row_dots,
            },
            LaidContent::Barcode { payload, .. } => PreviewLine::Barcode {
                payload: payload.clone(),
                indent: line.indent_dots,
                row: line.row_dots,
                height: 60,
            },
            LaidContent::Image {
                data,
                width_pct,
                align,
            } => {
                let usable = metrics.dots().saturating_sub(line.indent_dots).max(1);
                let width = (usable * u32::from((*width_pct).clamp(1, 100)) / 100).max(1);
                let picture = unpack(data, width);
                let drawn = picture.as_ref().map_or(0, |(w, _, _)| *w);
                let spare = usable.saturating_sub(drawn);
                PreviewLine::Logo {
                    indent: line.indent_dots,
                    row: line.row_dots,
                    left: line.indent_dots
                        + match align {
                            Align::Left => 0,
                            Align::Centre => spare / 2,
                            Align::Right => spare,
                        },
                    width: drawn,
                    height: picture.as_ref().map_or(0, |(_, h, _)| *h),
                    ink: picture.map(|(_, _, ink)| ink),
                }
            }
            LaidContent::Band {
                image,
                image_left,
                image_top,
                image_width,
                image_height,
                lines: band,
            } => {
                // The picture is placed inside the band, so the screen needs its top as well as
                // its left — the band draws both from the same origin.
                let top = *image_top;
                let picture = unpack(image, *image_width);
                PreviewLine::Band {
                    row: line.row_dots,
                    image: Box::new(PreviewLine::Logo {
                        indent: top,
                        row: *image_height,
                        left: *image_left,
                        width: *image_width,
                        height: *image_height,
                        ink: picture.map(|(_, _, ink)| ink),
                    }),
                    lines: band
                        .iter()
                        .map(|text| PreviewBandLine {
                            text: text.text.trim().to_owned(),
                            left: text.left,
                            top: text.top,
                            width: text.width,
                            row: metrics.size(text.style).row,
                            cap: metrics.size(text.style).cap,
                            bold: text.style.bold,
                            align: align_name(text.align).to_owned(),
                        })
                        .collect(),
                }
            }
            LaidContent::Blank => PreviewLine::Blank { row: line.row_dots },
        });
    }

    PreviewDoc {
        dots: metrics.dots(),
        columns: metrics.body().chars_across(metrics.dots()),
        lines,
        millimetres: laid.total_mm(),
        engine: engine.to_owned(),
        notes: laid.notes.iter().map(describe).collect(),
    }
}

/// The logo's dots, at the size it will print, one byte per dot.
fn unpack(data: &[u8], width: u32) -> Option<(u32, u32, Vec<u8>)> {
    let image = mb_print::image::Monochrome::decode(data).ok()?;
    let scaled = image.scaled_to(width.max(1));
    let mut ink = Vec::with_capacity((scaled.width * scaled.height) as usize);
    for y in 0..scaled.height {
        for x in 0..scaled.width {
            ink.push(u8::from(scaled.ink(x, y)));
        }
    }
    Some((scaled.width, scaled.height, ink))
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

#[cfg(test)]
mod tests {
    use super::*;
    use mb_print::doc::{Document, Style};
    use mb_print::layout::layout_for;
    use mb_print::paper::{Paper, PaperKind};

    fn metrics(kind: PaperKind) -> Metrics {
        let font = std::sync::Arc::new(mb_print::font::Font::builtin().expect("loads"));
        Metrics::face(Paper::new(kind), font)
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
}
