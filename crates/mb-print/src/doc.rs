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
/// # Two ways of saying how big, because there are two engines
///
/// `scale` is 1, 2 or 3 — the ESC/POS multiplier, and the number of columns a
/// character occupies. It is **capped by the layout** so text can never
/// overflow the paper (crown jewel 18), never rejected. That is the whole of
/// what a thermal printer's *own* font can do, so it is what the **text**
/// engine emits and it is not going anywhere.
///
/// `px` is a height in dots, added 2026-08-17 when the owner asked for sizes
/// that step in twos rather than in multiples of the printer's cell:
///
/// > *"size of fonts now showing 24px, 48px, 72px only 3 and its completly
/// > wrong, i want like small changes also like 2px increasing… a number wise
/// > drop down selection for size."*
///
/// The **graphics** engine rasterises the receipt as a picture, so it can draw
/// any height it likes. `px` is `None` for a document that has not asked for
/// one, and then the height is `scale` cells exactly — which is why every
/// receipt tuned before this change still prints identically.
///
/// # Why both, rather than px replacing scale
///
/// Because the text engine cannot honour px and must not silently ignore it.
/// Keeping the multiplier means a shop on the Text engine gets the nearest
/// size its printer can actually form ([`Style::scale`]) instead of a setting
/// that appears to work and does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    /// **The height of this text in dots.**
    ///
    /// # Why it is still called `scale` on the wire
    ///
    /// It used to BE the multiplier — 1, 2 or 3 — and that name is the key a
    /// shop's tuned sizes are stored against
    /// (`receipt.sections.store_name.scale`). Renaming the field would rename
    /// the row, and every shop that had chosen a size would silently get the
    /// default back on upgrade. The name is history; the value is dots.
    ///
    /// [`Style::size_from_wire`] is what lets a value written by an older
    /// build still mean what it meant then.
    #[serde(rename = "scale", deserialize_with = "Style::size_from_wire")]
    pub size: u16,
    pub bold: bool,
}

impl Style {
    /// One cell of the printer's own font — 24 dots. What `scale: 1` was.
    pub const CELL: u16 = 24;

    pub const NORMAL: Style = Style {
        size: Style::CELL,
        bold: false,
    };
    pub const BOLD: Style = Style {
        size: Style::CELL,
        bold: true,
    };

    /// A size given as the ESC/POS multiplier, which is how every template in
    /// this crate still asks for one.
    #[must_use]
    pub const fn new(scale: u8, bold: bool) -> Self {
        Style {
            size: (scale as u16) * Style::CELL,
            bold,
        }
    }

    /// **A size in dots** — what a shop chooses on the settings screen since
    /// 2026-08-17. `base` is kept in the signature because the caller knows
    /// the printer's cell and this type should not have to assume it twice.
    #[must_use]
    pub const fn px(px: u16, bold: bool, base: u16) -> Self {
        let _ = base;
        Style { size: px, bold }
    }

    /// **The multiplier that comes closest to this size**, for the ESC/POS text
    /// engine — which has one font at 1×, 2× and 3× and can do nothing else.
    ///
    /// Nearest, not floor: 40 dots is closer to 2× (48) than to 1× (24), and a
    /// shop that asked for something large should not get small on the engine
    /// that cannot be exact.
    // Dots into cells, and the answer is clamped to 1..=3 three lines later —
    // so there is nothing a remainder or a narrowing cast could lose. The
    // workspace denies both because of D7, and D7 is about MONEY: no amount is
    // computed anywhere in this file.
    #[expect(
        clippy::integer_division,
        clippy::cast_possible_truncation,
        reason = "cells, not money — clamped to 1..=3 immediately below"
    )]
    #[must_use]
    pub const fn scale(self) -> u8 {
        let steps = (self.size + Style::CELL / 2) / Style::CELL;
        if steps == 0 {
            1
        } else if steps > 3 {
            3
        } else {
            steps as u8
        }
    }

    /// **How tall this text is in dots** — the graphics engine's question.
    ///
    /// `base` is what one multiplier step is worth, and is used only for a
    /// style that carries no size of its own (which cannot happen through the
    /// constructors, and is what a hand-built literal would leave).
    #[must_use]
    pub const fn height(self, base: u32) -> u32 {
        if self.size == 0 {
            base
        } else {
            self.size as u32
        }
    }

    /// The same style at a different multiplier. Used by the layout when it has
    /// to cap something to fit the paper.
    #[must_use]
    pub const fn at_scale(self, scale: u8, base: u16) -> Self {
        Style {
            size: (scale as u16) * base,
            bold: self.bold,
        }
    }

    /// **A size written by an older build still means what it meant then.**
    ///
    /// Before 2026-08-17 this field held the multiplier: 1, 2 or 3. It holds
    /// dots now. A shop's stored row and an exported configuration file both
    /// carry the old numbers, and reading `2` as two DOTS would print a bill
    /// nobody can see — so anything at or below 3 is read as the multiplier it
    /// was. No real size is that small: the smallest this product offers is 12.
    fn size_from_wire<'de, D>(d: D) -> Result<u16, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = u16::deserialize(d)?;
        Ok(if raw <= 3 {
            raw.max(1) * Style::CELL
        } else {
            raw
        })
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

// **`FontFamily` used to be here, P17 deleted it (D71), and P31 put the choice
// back — NOT here.**
//
// Audit Part 3 lists a font choice, v1 had one, and P06 modelled it on the
// document. It was written onto every `Document` and **read by nothing**:
// `layout` does not carry it into `Laid`, and the raster sink drew with the ONE
// face the queue loaded at start-up (D33 — "one face for every printer, loaded
// once", because a cold glyph cache costs more than the whole raster). P17's T1
// found it — render, change the setting, render again, identical — and deleting
// a control wired to nothing was right.
//
// **Where it lives now is the JOB, and that is the whole difference.** A
// `Document` is a shape; which typeface draws it is a fact about the printing,
// like the paper width and the cut. So `queue::Job` carries a family key,
// `Shared` holds a `font::Typefaces`, and the raster sink is handed the face for
// that job. A bill and a kitchen ticket going to the same printer can be
// different faces — which is what the owner asked for at P31, and what a field
// on the document could never have given.
//
// The faces are not in the installer either: `typefaces.rs` reads them out of
// the system font folder, so five more choices cost nothing (S4).
//
// In `Engine::Text` mode the face is still the printer's own and ours cannot
// apply, exactly as it never could. That is why the setting is described as the
// face the RASTER path draws with.
//
// Kannada is still not this (D31, crown jewel 17): a second face is not a
// shaper, and `layout` still counts characters. `font.rs` says so at length.

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
    /// **P29, scope 7.6 — the bill's own number, in a form a scanner can
    /// read.** Recalling a printed bill by scanning it is the one thing a
    /// scanner does that nothing else in this product can do at all.
    ///
    /// CODE 128, which every scanner sold reads and which takes letters as
    /// well as digits — a bill number is `B-0042`, not a number.
    Barcode {
        payload: String,
        /// Whether to print the characters underneath. Yes on a bill: a
        /// barcode a scanner will not read is a bill somebody still has to
        /// find by hand.
        human_readable: bool,
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

    /// **The same document with every size rounded to a whole cell** — for the
    /// ESC/POS text engine, which has the printer's own font at 1×, 2× and 3×
    /// and cannot draw anything between.
    ///
    /// See `layout::Grid`. Snapping here, before anything is measured, is what
    /// keeps the wrapping, the column widths and the emitted characters
    /// agreeing with each other on that engine.
    #[must_use]
    pub fn snapped_to_cells(&self) -> Document {
        let snap = |style: &Style| Style {
            size: u16::from(style.scale()) * Style::CELL,
            bold: style.bold,
        };
        Document {
            paper: self.paper,
            blocks: self
                .blocks
                .iter()
                .map(|block| match block {
                    Block::Text { content, style, align } => Block::Text {
                        content: content.clone(),
                        style: snap(style),
                        align: *align,
                    },
                    Block::Row { left, right, style } => Block::Row {
                        left: left.clone(),
                        right: right.clone(),
                        style: snap(style),
                    },
                    Block::Columns { columns, rows, style } => Block::Columns {
                        columns: columns.clone(),
                        rows: rows.clone(),
                        style: snap(style),
                    },
                    other => other.clone(),
                })
                .collect(),
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
