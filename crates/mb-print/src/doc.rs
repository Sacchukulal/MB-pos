//! The one description of a printable document.

use serde::{Deserialize, Serialize};

use crate::paper::Paper;

/// How big, and how heavy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    /// The height of a capital letter, in dots.
    #[serde(rename = "scale", deserialize_with = "Style::size_from_wire")]
    pub size: u16,
    pub bold: bool,
}

impl Style {
    /// The ten sizes the settings screen offers, as cap heights in dots.
    pub const LADDER: [u16; 10] = [9, 11, 13, 15, 17, 19, 22, 26, 33, 41];

    /// The body of a receipt — the fourth rung, and the default for every section that is not a
    /// heading.
    pub const BODY: u16 = Style::LADDER[3];

    /// A heading — a shop's name, the grand total, the token.
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

    /// A size given as the ESC/POS multiplier, which is how a template asks for "ordinary",
    /// "big" or "as big as this can go" without naming a number.
    #[must_use]
    pub const fn new(scale: u8, bold: bool) -> Self {
        let size = match scale {
            0 | 1 => Style::BODY,
            2 => Style::HEADING,
            _ => Style::LARGEST,
        };
        Style { size, bold }
    }

    /// A cap height, straight.
    #[must_use]
    pub const fn px(px: u16, bold: bool, base: u16) -> Self {
        let _ = base;
        Style { size: px, bold }
    }

    /// The multiplier nearest this size, for the ESC/POS text engine — which has one font at
    /// 1×, 2× and 3× and can do nothing else.
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

    /// The cap height, with a floor for a style that carries none — which cannot happen through
    /// the constructors and is what a hand-built literal would leave.
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

    /// A size written by an older build still means what it meant then.
    fn size_from_wire<'de, D>(d: D) -> Result<u16, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Style::from_stored(u16::deserialize(d)?))
    }

    /// The table above, as a function.
    #[must_use]
    pub const fn from_stored(raw: u16) -> u16 {
        // The multiplier era. Matched rather than cast, so nothing has to reason about whether
        // a `u16` under four fits a `u8`.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Width {
    /// Exactly this many columns.
    Fixed(usize),
    /// Whatever is left, shared between the `Fill` columns.
    Fill,
}

/// One line of the text that sits beside a logo in a `Block::Band`.
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
    /// `label.............. amount`
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
    /// The alignment ruler on the test print.
    Ruler {
        marks: bool,
    },
    /// The shop's logo. Bytes, because this crate does not decode images — the sink that can
    /// draw one does.
    Image {
        data: Vec<u8>,
        width_pct: u8,
        align: Align,
    },
    /// A picture and a run of text lines, side by side.
    Band {
        image: Vec<u8>,
        /// Which side the picture is on — `Left` or `Right`.
        image_side: Align,
        /// The picture's share of the paper width, as a percentage.
        image_pct: u8,
        /// The text beside it, each line with its own size and alignment.
        text: Vec<BandLine>,
    },
    /// 2, the UPI QR.
    QrCode {
        payload: String,
        width_pct: u8,
        align: Align,
    },
    Barcode {
        payload: String,
        /// Whether to print the characters underneath.
        human_readable: bool,
        align: Align,
    },
    Spacer {
        lines: u8,
    },
    /// Air between one section and the next, in HALF body rows — a whole blank line between
    /// every section was a foot of paper, and none at all was one dense column.
    Air {
        halves: u8,
    },
}

/// A whole printable thing.
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

    /// Air, in half body rows. Nothing at all for zero, so a caller can pass a setting straight.
    pub fn air(&mut self, halves: u8) -> &mut Self {
        if halves == 0 {
            return self;
        }
        self.push(Block::Air { halves })
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
