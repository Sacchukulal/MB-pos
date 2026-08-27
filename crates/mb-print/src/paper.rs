//! The physical facts: how wide the paper is, and where the printer thinks its left edge is.

// Dots per column and millimetres per column.
#![allow(
    clippy::integer_division,
    reason = "printer geometry, not money — and the one case that matters is tested"
)]

use serde::{Deserialize, Serialize};

/// Dots to the millimetre, on every roll this product prints on.
pub const DOTS_PER_MM: u32 = 8;

/// What is in the printer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperKind {
    /// 58 mm, two inches.
    Mm58,
    /// 80 mm, three inches.
    #[default]
    Mm80,
    /// 100 mm, four inches.
    Mm100,
    /// 10, the B2B invoice.
    A4,
}

impl PaperKind {
    /// Characters across, at scale 1.
    #[must_use]
    pub const fn columns(self) -> usize {
        match self {
            PaperKind::Mm58 => 32,
            PaperKind::Mm80 => 48,
            PaperKind::Mm100 => 64,
            PaperKind::A4 => 96,
        }
    }

    #[must_use]
    pub const fn dots(self) -> Option<u32> {
        match self {
            PaperKind::Mm58 => Some(384),
            PaperKind::Mm80 => Some(576),
            PaperKind::Mm100 => Some(832),
            PaperKind::A4 => None,
        }
    }

    /// Printable width in millimetres.
    #[must_use]
    pub const fn printable_mm(self) -> u32 {
        match self {
            PaperKind::Mm58 => 48,
            PaperKind::Mm80 => 72,
            PaperKind::Mm100 => 104,
            PaperKind::A4 => 190,
        }
    }

    /// How many columns one millimetre is worth, as a rounded whole number of columns for a
    /// given millimetre shift.
    #[must_use]
    pub fn columns_for_mm(self, mm: i32) -> i32 {
        let columns = i64::from(u32::try_from(self.columns()).unwrap_or(u32::MAX));
        let width = i64::from(self.printable_mm());
        if width == 0 {
            return 0;
        }
        let scaled = i64::from(mm) * columns;
        // Round half away from zero, the same rule money.rs uses, so there is one rounding
        // convention in the product rather than two.
        let rounded = if scaled >= 0 {
            (scaled * 2 + width) / (width * 2)
        } else {
            (scaled * 2 - width) / (width * 2)
        };
        i32::try_from(rounded).unwrap_or(0)
    }
}

/// The print offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Offset {
    pub x_mm: i32,
    pub y_mm: i32,
}

impl Offset {
    #[must_use]
    pub const fn none() -> Self {
        Offset { x_mm: 0, y_mm: 0 }
    }

    #[must_use]
    pub const fn new(x_mm: i32, y_mm: i32) -> Self {
        Offset { x_mm, y_mm }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Paper {
    pub kind: PaperKind,
    pub offset: Offset,
}

impl Paper {
    #[must_use]
    pub const fn new(kind: PaperKind) -> Self {
        Paper {
            kind,
            offset: Offset::none(),
        }
    }

    #[must_use]
    pub const fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self
    }

    #[must_use]
    pub const fn columns(self) -> usize {
        self.kind.columns()
    }

    /// Dots per column at scale 1.
    #[must_use]
    pub fn dots_per_column(self) -> Option<u32> {
        let dots = self.kind.dots()?;
        let columns = u32::try_from(self.kind.columns()).ok()?;
        if columns == 0 {
            return None;
        }
        Some(dots / columns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_paper_has_a_whole_number_of_dots_per_column() {
        // If this ever stops being true the raster sink has to deal with fractional columns,
        // and it will deal with them differently from the text sink.
        for kind in [PaperKind::Mm58, PaperKind::Mm80, PaperKind::Mm100] {
            let paper = Paper::new(kind);
            let per = paper.dots_per_column().expect("thermal paper has dots");
            let columns = u32::try_from(kind.columns()).expect("small");
            assert_eq!(
                per * columns,
                kind.dots().expect("thermal paper has dots"),
                "{kind:?} does not divide evenly into columns"
            );
        }
    }

    /// `DOTS_PER_MM` is a constant, and this is what entitles it to be one.
    #[test]
    fn every_roll_is_eight_dots_to_the_millimetre() {
        for kind in [PaperKind::Mm58, PaperKind::Mm80, PaperKind::Mm100] {
            let dots = kind.dots().expect("thermal paper has dots");
            assert_eq!(
                dots / kind.printable_mm(),
                DOTS_PER_MM,
                "{kind:?} is not a 203 dpi head"
            );
            assert_eq!(dots % kind.printable_mm(), 0, "{kind:?} does not divide");
        }
    }

    #[test]
    fn a_millimetre_offset_becomes_whole_columns() {
        // 80 mm paper: 48 columns over 72 printable mm, so 1.5 mm per column.
        let p = PaperKind::Mm80;
        assert_eq!(p.columns_for_mm(0), 0);
        assert_eq!(p.columns_for_mm(2), 1); // 1.33 -> 1
        assert_eq!(p.columns_for_mm(3), 2); // 2.0  -> 2
        assert_eq!(p.columns_for_mm(-3), -2);
        assert_eq!(p.columns_for_mm(-2), -1);
    }

    #[test]
    fn a4_has_no_dots_and_that_is_not_an_error() {
        assert_eq!(PaperKind::A4.dots(), None);
        assert_eq!(Paper::new(PaperKind::A4).dots_per_column(), None);
        assert_eq!(PaperKind::A4.columns(), 96);
    }
}
