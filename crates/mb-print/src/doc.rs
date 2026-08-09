//! The one description of a printable document.
//!
//! **This module knows nothing about bills.** If a type in this file mentions
//! GST, it is in the wrong file — the bill lives in [`crate::template`], which
//! is the only place that knows what a receipt looks like.
//!
//! A flat list of blocks, not a tree. A receipt has no nesting and a tree would
//! only invite some.

use serde::{Deserialize, Serialize};

use crate::paper::Paper;

/// How big, and how heavy.
///
/// `scale` is 1, 2 or 3 — the ESC/POS multiplier P07 will emit, and the number
/// of columns a character occupies here. It is **capped by the layout** so text
/// can never overflow the paper (crown jewel 18), never rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    pub scale: u8,
    pub bold: bool,
}

impl Style {
    pub const NORMAL: Style = Style {
        scale: 1,
        bold: false,
    };
    pub const BOLD: Style = Style {
        scale: 1,
        bold: true,
    };

    #[must_use]
    pub const fn new(scale: u8, bold: bool) -> Self {
        Style { scale, bold }
    }

    /// Clamped to the range the printers actually support.
    #[must_use]
    pub const fn scale(self) -> u8 {
        if self.scale == 0 {
            1
        } else if self.scale > 3 {
            3
        } else {
            self.scale
        }
    }
}

impl Default for Style {
    fn default() -> Self {
        Style::NORMAL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Centre,
    Right,
}

/// The five separator patterns v1 had. Kept exactly, because a shop that has
/// tuned its receipt should not have to tune it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    #[default]
    Dashed,
    Dotted,
    Solid,
    Bold,
    Double,
}

impl Pattern {
    #[must_use]
    pub const fn glyph(self) -> char {
        match self {
            Pattern::Dashed => '-',
            Pattern::Dotted => '.',
            Pattern::Solid => '_',
            Pattern::Bold => '=',
            Pattern::Double => '=',
        }
    }

    /// `Double` is two rules, which is the only reason it differs from `Bold`.
    #[must_use]
    pub const fn lines(self) -> u8 {
        match self {
            Pattern::Double => 2,
            _ => 1,
        }
    }
}

// **`FontFamily` used to be here, and P17 deleted it — decision D71.**
//
// Audit Part 3 lists a font choice, v1 had one, and P06 modelled it. It was
// written onto every `Document` and **read by nothing**: `layout` does not
// carry it into `Laid`, and the raster sink draws with the ONE face the queue
// loaded at start-up (D33 — "one face for every printer, loaded once", because
// a cold glyph cache costs more than the whole raster). In `Engine::Text` mode
// the face is the printer's own, so ours could never apply there at all.
//
// P17's T1 is what found it: render the bill, change the setting, render again,
// and the two documents are identical. A setting that changes nothing is a lie
// on a screen, so the honest choice was to delete it rather than ship it.
//
// **It comes back at P23**, which has to ship a second face anyway for Kannada
// (D31 names that as a known gap needing a shaper). A font choice is a real
// choice on the day there is more than one font.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Width {
    /// Exactly this many columns.
    Fixed(usize),
    /// Whatever is left, shared between the `Fill` columns.
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub width: Width,
    pub align: Align,
}

impl Column {
    #[must_use]
    pub const fn fixed(width: usize, align: Align) -> Self {
        Column {
            width: Width::Fixed(width),
            align,
        }
    }

    #[must_use]
    pub const fn fill(align: Align) -> Self {
        Column {
            width: Width::Fill,
            align,
        }
    }
}

/// One thing on the paper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "block", rename_all = "snake_case")]
pub enum Block {
    Text {
        content: String,
        style: Style,
        align: Align,
    },
    /// `label .............. amount`
    ///
    /// The right side is an amount far more often than not, which is why the
    /// layout has a rule about it: see [`crate::layout`], "the money wins".
    Row {
        left: String,
        right: String,
        style: Style,
    },
    /// The item table.
    Columns {
        columns: Vec<Column>,
        rows: Vec<Vec<String>>,
        style: Style,
    },
    Separator {
        pattern: Pattern,
    },
    /// The shop's logo. Bytes, because this crate does not decode images —
    /// the sink that can draw one does.
    Image {
        data: Vec<u8>,
        width_pct: u8,
        align: Align,
    },
    /// Scope 8.2, the UPI QR. The payload is the UPI URI; making the picture
    /// is a sink's job.
    QrCode {
        payload: String,
        width_pct: u8,
        align: Align,
    },
    Spacer {
        lines: u8,
    },
}

/// A whole printable thing.
///
/// `Serialize`/`Deserialize` because **P08's on-screen preview is a fourth
/// sink** and it lives in React, so this crosses IPC as JSON. D20 applies:
/// nothing in here uses a map with a non-string key, and `t14` proves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub paper: Paper,
    pub blocks: Vec<Block>,
}

impl Document {
    #[must_use]
    pub fn new(paper: Paper) -> Self {
        Document {
            paper,
            blocks: Vec::new(),
        }
    }

    pub fn push(&mut self, block: Block) -> &mut Self {
        self.blocks.push(block);
        self
    }

    pub fn text(&mut self, content: impl Into<String>, style: Style, align: Align) -> &mut Self {
        self.push(Block::Text {
            content: content.into(),
            style,
            align,
        })
    }

    pub fn line(&mut self, content: impl Into<String>) -> &mut Self {
        self.text(content, Style::NORMAL, Align::Left)
    }

    pub fn row(
        &mut self,
        left: impl Into<String>,
        right: impl Into<String>,
        style: Style,
    ) -> &mut Self {
        self.push(Block::Row {
            left: left.into(),
            right: right.into(),
            style,
        })
    }

    pub fn separator(&mut self, pattern: Pattern) -> &mut Self {
        self.push(Block::Separator { pattern })
    }

    pub fn spacer(&mut self, lines: u8) -> &mut Self {
        self.push(Block::Spacer { lines })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paper::PaperKind;

    #[test]
    fn scale_is_clamped_to_what_a_printer_can_do() {
        assert_eq!(Style::new(0, false).scale(), 1);
        assert_eq!(Style::new(9, false).scale(), 3);
        assert_eq!(Style::new(2, false).scale(), 2);
    }

    #[test]
    fn a_document_round_trips_through_json() {
        // P08's preview is on the other side of IPC, so this is the wire.
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.text("ANNA KUTEERA", Style::new(2, true), Align::Centre)
            .separator(Pattern::Double)
            .row("Masala Dosa", "240.00", Style::NORMAL)
            .push(Block::QrCode {
                payload: "upi://pay?pa=anna@upi&am=240.00".to_owned(),
                width_pct: 40,
                align: Align::Centre,
            })
            .spacer(2);

        let json = serde_json::to_string(&doc).expect("serialises");
        let back: Document = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(doc, back);
    }
}
