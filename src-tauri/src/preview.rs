//! What the on-screen bill preview is handed — **the fourth sink's input.**
//!
//! text (P06) · PDF (P06) · raster (P07) · screen (P08). All four render the
//! same `Laid`, which is D29: *"there is exactly one function that walks a
//! laid-out document, and every renderer is a `Sink` it calls."*
//!
//! # Why this is a view model and not `Laid` itself
//!
//! `Laid` could cross the wire — it is `Serialize` already, and P06 built it
//! that way on purpose. Sending it would mean `#[derive(TS)]` on ten types
//! across three files of mb-print, which is a dependency added to a library
//! crate for the benefit of a screen.
//!
//! So this converts, in one function, exactly as `PrintJobView` does for the
//! queue. The cost is this file; the gain is that mb-print's types can change
//! without a screen changing, and that the conversion is a single place to test.
//!
//! **And the conversion adds nothing.** No wrapping, no measuring, no
//! truncation, no arithmetic — every one of those was decided by
//! `mb_print::layout` before this saw anything. If this file ever starts
//! deciding where a line breaks, there are two layout engines and audit D1 is
//! back.

use mb_print::doc::Align;
use mb_print::layout::{Laid, LaidContent};
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PreviewDoc {
    /// Characters across. The preview is exactly as wide as the paper, not
    /// "about right".
    pub columns: usize,
    pub lines: Vec<PreviewLine>,
    /// Anything the layout had to do that a person might want to know — a
    /// heading that was capped (crown jewel 18), an offset that was clamped
    /// (scope 7.11). P17 shows these beside the setting that caused them.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PreviewLine {
    /// Already wrapped, already padded to its alignment, already offset.
    Text {
        text: String,
        indent: usize,
        /// 1, 2 or 3 — the ESC/POS multiplier, so the screen shows the same
        /// relative size the paper will.
        scale: u8,
        bold: bool,
    },
    /// The glyph is resolved **here** rather than on the screen, so the
    /// preview and the paper cannot disagree about what "dotted" looks like.
    Rule {
        glyph: String,
        width: usize,
        indent: usize,
    },
    /// The printer draws a real square (D36); on screen the payload is what a
    /// person can actually check — the same decision the text sink made.
    Qr { payload: String, indent: usize },
    /// A logo the raster sink will draw. The screen shows a placeholder: the
    /// bytes are a 1-bit bitmap (D37) and decoding one in the webview to show
    /// it smaller than a thumbnail is work for nothing.
    Logo { indent: usize },
    Blank,
}

/// The one conversion.
#[must_use]
pub fn to_preview(laid: &Laid) -> PreviewDoc {
    let mut lines = Vec::with_capacity(laid.lines.len());

    for line in &laid.lines {
        lines.push(match &line.content {
            LaidContent::Text { text } => PreviewLine::Text {
                text: text.clone(),
                indent: line.indent,
                scale: line.style.scale(),
                bold: line.style.bold,
            },
            LaidContent::Separator { pattern, width } => PreviewLine::Rule {
                glyph: pattern.glyph().to_string(),
                width: *width,
                indent: line.indent,
            },
            LaidContent::QrCode { payload, .. } => PreviewLine::Qr {
                payload: payload.clone(),
                indent: line.indent,
            },
            LaidContent::Image { .. } => PreviewLine::Logo {
                indent: line.indent,
            },
            LaidContent::Blank => PreviewLine::Blank,
        });
    }

    PreviewDoc {
        columns: laid.paper.columns(),
        lines,
        notes: laid.notes.iter().map(describe).collect(),
    }
}

/// A layout note, in words (crown jewel 14 again).
fn describe(note: &mb_print::layout::Note) -> String {
    use mb_print::layout::Note;
    match note {
        Note::ScaleCapped { asked, used } => format!(
            "A heading was too big for this paper, so it printed at {used}× \
             instead of {asked}×."
        ),
        Note::OffsetClamped {
            asked_mm,
            used_columns,
        } => format!(
            "The print offset of {asked_mm:+} mm was too far, so it was limited \
             to {used_columns} characters."
        ),
        Note::LabelWrapped { label } => {
            format!("\"{label}\" was too long for one line, so it wrapped.")
        }
    }
}

/// `Align` crosses nowhere yet; kept referenced so the import is honest about
/// what this module knows and a future alignment-aware preview has a hook.
const _: Option<Align> = None;

#[cfg(test)]
mod tests {
    use super::*;
    use mb_print::doc::{Document, Style};
    use mb_print::layout::layout;
    use mb_print::paper::{Paper, PaperKind};

    #[test]
    fn every_line_of_the_layout_reaches_the_preview() {
        // The sink property, at this boundary: nothing may be dropped on the
        // way to the screen either.
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.text("ANNA KUTEERA", Style::new(2, true), Align::Centre)
            .separator(mb_print::doc::Pattern::Double)
            .row("Masala Dosa", "240.00", Style::NORMAL)
            .spacer(1);

        let laid = layout(&doc).expect("lays out");
        let preview = to_preview(&laid);

        assert_eq!(preview.lines.len(), laid.lines.len());
        assert_eq!(preview.columns, 48);
    }

    #[test]
    fn the_preview_does_no_layout_of_its_own() {
        // Whatever the layout produced is what the preview carries, character
        // for character. If this ever needs a `trim` or a `slice`, something
        // has started deciding.
        let mut doc = Document::new(Paper::new(PaperKind::Mm58));
        doc.row("Paneer Butter Masala (Half) Extra Spicy", "1,240.00", Style::NORMAL);

        let laid = layout(&doc).expect("lays out");
        let preview = to_preview(&laid);

        let from_layout = laid.text_lines();
        let from_preview: Vec<String> = preview
            .lines
            .iter()
            .filter_map(|l| match l {
                PreviewLine::Text { text, indent, .. } => {
                    Some(format!("{}{text}", " ".repeat(*indent)))
                }
                _ => None,
            })
            .collect();
        assert_eq!(from_layout, from_preview);
    }

    #[test]
    fn a_capped_heading_is_explained_in_words() {
        let mut doc = Document::new(Paper::new(PaperKind::Mm58));
        doc.text(
            "ANNAPOORNESHWARI REFRESHMENTS",
            Style::new(3, true),
            Align::Centre,
        );
        let laid = layout(&doc).expect("lays out");
        let preview = to_preview(&laid);
        assert!(
            preview.notes.iter().any(|n| n.contains("too big")),
            "{:?}",
            preview.notes
        );
    }
}
