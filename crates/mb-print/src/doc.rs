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
/// # A size is the height of a capital letter, in dots — P32
///
/// Not a multiplier, not a nominal row height: **the thing a person can hold a
/// ruler against.** A thermal head is 8 dots to the millimetre, so size 15 is a
/// capital just under 2 mm tall, and size 26 is one about 3¼ mm tall.
///
/// # What it was, and why that had to change
///
/// It was a multiplier (1, 2, 3), then on 2026-08-17 it became a nominal
/// height in dots so a shop could ask for sizes between the printer's own three.
/// But **nothing drew that height.** The raster sink scaled the face until a
/// capital `M` fitted a 12-dot column and ignored the height altogether, so a
/// request for 24 dots put a **13-dot** capital (9 in Times New Roman) inside a
/// 27-dot row. The owner photographed the result on 2026-08-23:
///
/// > *"the printed real page is completely different then the setting i set"*
///
/// Worse, [`crate::layout`] capped the size down to make a table fit, and the
/// cap landed on the same number for the top five choices — five entries on a
/// dropdown that printed identically.
///
/// So the number means the letter now, [`crate::metrics`] measures what that
/// costs in advance and in row height, and **the layout never changes a size to
/// make something fit**. A name too long for its column wraps onto a second
/// line, which is the owner's own ruling:
///
/// > *"u can take 2 lines if item name is tooo long, then only, otherwise in
/// > the same line… so dont damage the design, font, styles etc."*
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    /// **The height of a capital letter, in dots.**
    ///
    /// # Why it is still called `scale` on the wire
    ///
    /// It used to BE the multiplier, and that name is the key a shop's tuned
    /// sizes are stored against (`receipt.sections.store_name.scale`).
    /// Renaming the field would rename the row, and every shop that had chosen
    /// a size would silently get the default back on upgrade. The name is
    /// history; the value is the cap height.
    ///
    /// [`Style::size_from_wire`] is what lets a value written by an older build
    /// still mean what it meant then.
    #[serde(rename = "scale", deserialize_with = "Style::size_from_wire")]
    pub size: u16,
    pub bold: bool,
}

impl Style {
    /// **The ten sizes the settings screen offers**, as cap heights in dots.
    ///
    /// The authority. `settings::catalog::SIZES` mirrors it for the screen and
    /// a test fails the build if the two ever disagree.
    ///
    /// **Deliberately disjoint from every value an older build stored**
    /// (1, 2, 3 and the nominal heights 16, 20, 24, 28, 32, 36, 40, 48, 60,
    /// 72), so a number read off a shop's disk can be told apart from one
    /// written today. That is what makes the upgrade in
    /// `settings::modernise` unambiguous instead of a guess.
    pub const LADDER: [u16; 10] = [9, 11, 13, 15, 17, 19, 22, 26, 33, 41];

    /// The body of a receipt — the fourth rung, and the default for every
    /// section that is not a heading.
    ///
    /// On 80 mm paper in the built-in face this fits 44 characters to a line,
    /// against the 48 the product shipped with, and draws a capital 15 dots
    /// tall against the 13 that was actually coming out. Slightly fewer
    /// characters, a visibly bigger letter, and a row that costs 21 dots
    /// instead of 27.
    pub const BODY: u16 = Style::LADDER[3];

    /// A heading — a shop's name, the grand total, the token. The eighth rung.
    pub const HEADING: u16 = Style::LADDER[7];

    pub const SMALLEST: u16 = Style::LADDER[0];
    pub const LARGEST: u16 = Style::LADDER[9];

    pub const NORMAL: Style = Style {
        size: Style::BODY,
        bold: false,
    };
    pub const BOLD: Style = Style {
        size: Style::BODY,
        bold: true,
    };

    /// A size given as the ESC/POS multiplier, which is how a template asks for
    /// "ordinary", "big" or "as big as this can go" without naming a number.
    #[must_use]
    pub const fn new(scale: u8, bold: bool) -> Self {
        let size = match scale {
            0 | 1 => Style::BODY,
            2 => Style::HEADING,
            _ => Style::LARGEST,
        };
        Style { size, bold }
    }

    /// **A cap height, straight.** What a shop's stored setting carries.
    #[must_use]
    pub const fn px(px: u16, bold: bool, base: u16) -> Self {
        let _ = base;
        Style { size: px, bold }
    }

    /// **The multiplier nearest this size**, for the ESC/POS text engine —
    /// which has one font at 1×, 2× and 3× and can do nothing else.
    ///
    /// The boundaries are the midpoints between [`Style::BODY`],
    /// [`Style::HEADING`] and [`Style::LARGEST`], which are the three sizes the
    /// three multipliers correspond to on the ladder.
    #[must_use]
    pub const fn scale(self) -> u8 {
        // Midpoint of HEADING (26) and LARGEST (41).
        if self.size >= 34 {
            3
        // Midpoint of BODY (15) and HEADING (26).
        } else if self.size >= 21 {
            2
        } else {
            1
        }
    }

    /// The cap height, with a floor for a style that carries none — which
    /// cannot happen through the constructors and is what a hand-built literal
    /// would leave.
    #[must_use]
    pub const fn height(self, base: u32) -> u32 {
        if self.size == 0 {
            base
        } else {
            self.size as u32
        }
    }

    /// The same style at one of the three multiplier sizes.
    #[must_use]
    pub const fn at_scale(self, scale: u8, base: u16) -> Self {
        let _ = base;
        Style {
            size: Style::new(scale, self.bold).size,
            bold: self.bold,
        }
    }

    /// **A size written by an older build still means what it meant then.**
    ///
    /// Three vocabularies have reached this field:
    ///
    /// | written | meaning | read as |
    /// |---|---|---|
    /// | 1, 2, 3 | the ESC/POS multiplier | the ladder rung that multiplier is |
    /// | 16 … 72 | a nominal row height (2026-08-17 to 2026-08-23) | the same rung of the new ladder |
    /// | 9 … 41 | a cap height | itself |
    ///
    /// The three sets do not overlap, which is why this can be exact rather
    /// than a guess — see [`Style::LADDER`]. A value in none of them is a
    /// config file somebody edited by hand; it is clamped to the ladder's ends
    /// and kept, because refusing a shop's own settings row is how a tuned
    /// receipt silently reverts to the default (that happened twice).
    fn size_from_wire<'de, D>(d: D) -> Result<u16, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Style::from_stored(u16::deserialize(d)?))
    }

    /// The table above, as a function. Public because the settings catalogue
    /// reads the same rows off the database and must agree exactly.
    #[must_use]
    pub const fn from_stored(raw: u16) -> u16 {
        // The multiplier era. Matched rather than cast, so nothing has to
        // reason about whether a `u16` under four fits a `u8` — D7 denies that
        // cast workspace-wide and it is right to.
        match raw {
            0 | 1 => return Style::BODY,
            2 => return Style::HEADING,
            3 => return Style::LARGEST,
            _ => {}
        }
        // The nominal-height era, rung for rung.
        let old = [16_u16, 20, 24, 28, 32, 36, 40, 48, 60, 72];
        let mut i = 0;
        while i < old.len() {
            if old[i] == raw {
                return Style::LADDER[i];
            }
            i += 1;
        }
        // Today's vocabulary, or something hand-edited.
        if raw < Style::SMALLEST {
            Style::SMALLEST
        } else if raw > Style::LARGEST {
            Style::LARGEST
        } else {
            raw
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

/// One line of the text that sits beside a logo in a [`Block::Band`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandLine {
    pub content: String,
    pub style: Style,
    pub align: Align,
}

impl BandLine {
    #[must_use]
    pub fn new(content: impl Into<String>, style: Style, align: Align) -> BandLine {
        BandLine {
            content: content.into(),
            style,
            align,
        }
    }
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
    /// **The alignment ruler on the test print** — scope 7.11, and expanded by
    /// the layout rather than by the template.
    ///
    /// It was built in [`crate::testprint`] as `paper.columns()` characters,
    /// which was right while a character's width was assumed and wrong the
    /// moment it was measured (P32): the ruler that exists to prove a print is
    /// aligned ran two characters off the edge of the paper.
    ///
    /// Only the layout knows how many characters fit, so only the layout can
    /// draw it. `marks` is the tick row; the other is the row of tens.
    Ruler {
        marks: bool,
    },
    /// The shop's logo. Bytes, because this crate does not decode images —
    /// the sink that can draw one does.
    Image {
        data: Vec<u8>,
        width_pct: u8,
        align: Align,
    },
    /// **A picture and a run of text lines, side by side** — P32.
    ///
    /// The only block in this crate that is two-dimensional, and it exists
    /// because a letterhead is two-dimensional and nothing else on a receipt
    /// is. The owner asked for it by name on 2026-08-23:
    ///
    /// > *"logo placement (left right, top, if left right means the hotel name
    /// > and address will cover 70% of 3 inch or 4 inch paper, remaining 30%
    /// > width for logo, also logo correctly fit with that on ful size"*
    ///
    /// Before this, a `Document` was a flat top-to-bottom list and **nothing
    /// could sit beside anything** — so a logo could only ever be centred above
    /// the shop's name, and `Block::Image`'s own `align` field was dead.
    ///
    /// The band's height is whichever of the two is taller, and the shorter one
    /// is centred against it. The layout decides where the text breaks, exactly
    /// as it does everywhere else; this block only says the two things share a
    /// band of rows.
    Band {
        image: Vec<u8>,
        /// Which side the picture is on — `Left` or `Right`. `Centre` is not a
        /// side and is treated as `Left`.
        image_side: Align,
        /// The picture's share of the paper width, as a percentage. 30 by the
        /// owner's ruling.
        image_pct: u8,
        /// The text beside it, each line with its own size and alignment.
        text: Vec<BandLine>,
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

    // **`snapped_to_cells` used to be here, and P32 deleted it.**
    //
    // It rewrote every size to a whole multiple of the printer's cell before
    // the layout measured anything, so the ESC/POS text engine's wrapping and
    // its emitted characters agreed. That work belongs to `metrics::Metrics`
    // now: `Metrics::printer_font` answers every question about size with one
    // of the three the hardware can form, so the layout is already working in
    // that engine's vocabulary and there is nothing left to snap. One
    // mechanism instead of two, and the document is no longer rewritten behind
    // the caller's back.

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
