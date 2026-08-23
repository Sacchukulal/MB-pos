//! The physical facts: how wide the paper is, and where the printer thinks its
//! left edge is.

// Dots per column and millimetres per column. D7's ban on integer division is
// about the money path; these are printer geometry, and the one place a
// remainder would actually matter — dots not dividing evenly into columns —
// has a test that fails rather than a rounding.
#![allow(
    clippy::integer_division,
    reason = "printer geometry, not money — and the one case that matters is tested"
)]

use serde::{Deserialize, Serialize};

/// **Dots to the millimetre**, on every roll this product prints on.
///
/// A 203 dpi head, which is what every thermal receipt printer sold is. It is a
/// constant rather than a division because all three roll widths agree — 384
/// over 48 mm, 576 over 72 mm, 832 over 104 mm — and a test below says so, so
/// the print offset and "how long is this bill" have one answer instead of
/// three roundings.
pub const DOTS_PER_MM: u32 = 8;

/// What is in the printer.
///
/// **The grid is the model.** A thermal receipt is a character grid — that is
/// what the printer's own font path does, and matching it is what will make the
/// raster sink and the text sink agree when P07 builds the first of those.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperKind {
    /// 58 mm, two inches.
    Mm58,
    /// 80 mm, three inches. What most counters run.
    #[default]
    Mm80,
    /// 100 mm, four inches.
    Mm100,
    /// Scope 7.10, the B2B invoice.
    ///
    /// **A stated compromise, not a design.** A proper tax invoice wants a
    /// proportional face and real table rules; 96 columns of Courier is
    /// correct, filable and ugly. P18 exports reports through the same path and
    /// is welcome to argue for better.
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

    /// Printable dots across. `None` for A4, which is not a dot matrix.
    ///
    /// These are the real numbers for a 203 dpi head — 8 dots per millimetre —
    /// not round ones. 4-inch paper is 832 dots over 104 mm and **not** 800:
    /// 800 would not divide evenly into 64 columns, and a fractional
    /// dots-per-column is how the raster sink and the text sink would end up
    /// disagreeing about where a character sits. The test below is what caught
    /// that.
    #[must_use]
    pub const fn dots(self) -> Option<u32> {
        match self {
            PaperKind::Mm58 => Some(384),
            PaperKind::Mm80 => Some(576),
            PaperKind::Mm100 => Some(832),
            PaperKind::A4 => None,
        }
    }

    /// Printable width in millimetres. Less than the paper itself — every one
    /// of these printers leaves a margin it will not fire into.
    #[must_use]
    pub const fn printable_mm(self) -> u32 {
        match self {
            PaperKind::Mm58 => 48,
            PaperKind::Mm80 => 72,
            PaperKind::Mm100 => 104,
            PaperKind::A4 => 190,
        }
    }

    /// How many columns one millimetre is worth, as a rounded whole number of
    /// columns for a given millimetre shift.
    ///
    /// Rounded, because half a character is not a thing a text sink can do —
    /// and the two sinks have to agree or the offset has re-created the very
    /// drift this crate exists to prevent.
    #[must_use]
    pub fn columns_for_mm(self, mm: i32) -> i32 {
        let columns = i64::from(u32::try_from(self.columns()).unwrap_or(u32::MAX));
        let width = i64::from(self.printable_mm());
        if width == 0 {
            return 0;
        }
        let scaled = i64::from(mm) * columns;
        // Round half away from zero, the same rule money.rs uses, so there is
        // one rounding convention in the product rather than two.
        let rounded = if scaled >= 0 {
            (scaled * 2 + width) / (width * 2)
        } else {
            (scaled * 2 - width) / (width * 2)
        };
        i32::try_from(rounded).unwrap_or(0)
    }
}

/// Scope 7.11 — the print offset.
///
/// Thermal printers disagree about where the first dot sits relative to the
/// paper edge, so a document whose columns add up to exactly the paper width
/// can still come out 2–3 mm off-centre. This is the owner's correction for
/// that, in whole millimetres, signed.
///
/// It is applied **once**, in [`crate::layout`], so every sink inherits it and
/// none of them can disagree. P07 stores it per printer and makes it adjustable
/// from the test print; P17 puts it on a screen.
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

    /// Dots per column at scale 1 — what P07's raster sink will multiply by.
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
        // If this ever stops being true the raster sink has to deal with
        // fractional columns, and it will deal with them differently from the
        // text sink. Better to find out here.
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

    /// [`DOTS_PER_MM`] is a constant, and this is what entitles it to be one.
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
