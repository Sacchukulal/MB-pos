//! How big text is, in dots.
//!
//! ```text
//! chars = usable_columns * 24 / height
//! ```

// Dots, columns and cap heights.
#![allow(clippy::integer_division, reason = "dots and columns, not money")]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::doc::Style;
use crate::font::{Cell, Font};
use crate::paper::Paper;

/// What one size is worth, in dots, on this paper in this face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeMetrics {
    /// The cap height that will actually be drawn.
    pub cap: u16,
    /// One character's advance, in dots.
    pub advance: u32,
    /// Ink above the baseline, in dots.
    pub ascent: u32,
    /// Ink below the baseline, in dots.
    pub descent: u32,
    /// The whole row: ink plus leading.
    pub row: u32,
    /// The ESC/POS multiplier nearest this size, for the text engine.
    pub scale: u8,
}

impl SizeMetrics {
    /// How many characters of this size fit across `dots`.
    #[must_use]
    pub const fn chars_across(&self, dots: u32) -> usize {
        if self.advance == 0 {
            return 1;
        }
        let n = (dots / self.advance) as usize;
        if n == 0 { 1 } else { n }
    }
}

/// Everything the layout and the sinks need to know about how big text is.
#[derive(Clone)]
pub struct Metrics {
    paper: Paper,
    face: Option<Arc<Font>>,
    sizes: Arc<Mutex<BTreeMap<u16, SizeMetrics>>>,
}

impl std::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metrics")
            .field("paper", &self.paper.kind)
            .field(
                "face",
                &self.face.as_ref().map_or("the printer's own", |f| f.name()),
            )
            .finish()
    }
}

/// The capital height of the printer's own font at 1×.
const PRINTER_CAP: u16 = 17;

impl Metrics {
    /// Measure a real typeface — the graphics engine and the preview.
    #[must_use]
    pub fn face(paper: Paper, face: Arc<Font>) -> Metrics {
        Metrics {
            paper,
            face: Some(face),
            sizes: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// The printer's own font: one face, three multipliers, nothing between.
    #[must_use]
    pub fn printer_font(paper: Paper) -> Metrics {
        Metrics {
            paper,
            face: None,
            sizes: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    #[must_use]
    pub const fn paper(&self) -> Paper {
        self.paper
    }

    #[must_use]
    pub fn dots(&self) -> u32 {
        match self.paper.kind.dots() {
            Some(dots) => dots,
            // No thermal head, so "dots" is a fiction.
            None => {
                let columns = u32::try_from(self.paper.columns()).unwrap_or(1);
                columns * self.body().advance.max(1)
            }
        }
    }

    /// One column of the printer's own grid, in dots.
    #[must_use]
    pub fn base_advance(&self) -> u32 {
        self.paper.dots_per_column().unwrap_or(12)
    }

    /// Is every character the same width?
    #[must_use]
    pub fn is_monospace(&self) -> bool {
        self.face.as_ref().is_none_or(|f| f.is_monospace())
    }

    /// The face, for a sink that has to draw glyphs.
    #[must_use]
    pub fn font(&self) -> Option<&Arc<Font>> {
        self.face.as_ref()
    }

    /// The cell to rasterise this size in.
    #[must_use]
    pub fn cell(&self, style: Style) -> Option<Cell> {
        let face = self.face.as_ref()?;
        Some(face.cell_for_cap(u32::from(self.size(style).cap)))
    }

    /// What this style is worth in dots.
    #[must_use]
    pub fn size(&self, style: Style) -> SizeMetrics {
        self.for_cap(style.size)
    }

    /// The same, for a bare cap height.
    #[must_use]
    pub fn for_cap(&self, cap: u16) -> SizeMetrics {
        let cap = cap.clamp(Style::SMALLEST, Style::LARGEST);
        if let Some(found) = lock(&self.sizes).get(&cap) {
            return *found;
        }
        let measured = self.measure(cap);
        lock(&self.sizes).insert(cap, measured);
        measured
    }

    /// The body size on this paper — what a plain line of text costs.
    #[must_use]
    pub fn body(&self) -> SizeMetrics {
        self.for_cap(Style::BODY)
    }

    fn measure(&self, cap: u16) -> SizeMetrics {
        let scale = Style {
            size: cap,
            bold: false,
        }
        .scale();
        match &self.face {
            None => {
                // The printer's own font.
                let n = u32::from(scale);
                let advance = self.base_advance() * n;
                let row = advance * 2;
                SizeMetrics {
                    cap: PRINTER_CAP * u16::from(scale),
                    advance,
                    // A 24-dot cell puts its baseline three quarters down, which is what Font A
                    // does.
                    ascent: row * 3 / 4,
                    descent: row / 4,
                    row,
                    scale,
                }
            }
            Some(face) => {
                let cell = face.cell_for_cap(u32::from(cap));
                SizeMetrics {
                    cap,
                    advance: cell.width.max(1),
                    ascent: cell.ascent,
                    descent: cell.descent,
                    row: cell.height.max(1),
                    scale,
                }
            }
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paper::PaperKind;

    fn face_metrics(kind: PaperKind) -> Metrics {
        let font = Arc::new(Font::builtin().expect("the shipped face loads"));
        Metrics::face(Paper::new(kind), font)
    }

    /// The size a shop picks is the size that gets drawn.
    #[test]
    fn a_cap_height_is_the_cap_height_that_prints() {
        let metrics = face_metrics(PaperKind::Mm80);
        let font = metrics.font().expect("a face").clone();
        for cap in Style::LADDER {
            let cell = font.cell_for_cap(u32::from(cap));
            let glyph = font.glyph('M', cell);
            let mut top = u32::MAX;
            let mut bottom = 0;
            for y in 0..glyph.height {
                for x in 0..glyph.width {
                    if glyph.ink(x, y) {
                        let at = u32::try_from(glyph.top).unwrap_or(0) + y;
                        top = top.min(at);
                        bottom = bottom.max(at);
                    }
                }
            }
            let drawn = bottom + 1 - top;
            assert!(
                drawn.abs_diff(u32::from(cap)) <= 1,
                "size {cap} drew a {drawn}-dot capital"
            );
        }
    }

    /// Every step up the ladder is visibly bigger.
    #[test]
    fn every_size_on_the_ladder_is_bigger_than_the_one_below() {
        let metrics = face_metrics(PaperKind::Mm80);
        let mut last: Option<SizeMetrics> = None;
        for cap in Style::LADDER {
            let now = metrics.for_cap(cap);
            if let Some(before) = last {
                assert!(now.cap > before.cap, "size {cap} is not taller");
                assert!(now.advance > before.advance, "size {cap} is not wider");
                assert!(now.row > before.row, "size {cap} does not take more paper");
            }
            last = Some(now);
        }
    }

    #[test]
    fn a_row_is_its_ink_plus_leading() {
        let metrics = face_metrics(PaperKind::Mm80);
        let body = metrics.body();
        assert_eq!(body.row, body.ascent + body.descent + leading_of(body));
        assert!(
            body.row < 27,
            "the body row is {} dots and it used to be 27 for a SMALLER letter",
            body.row
        );
    }

    fn leading_of(m: SizeMetrics) -> u32 {
        (u32::from(m.cap) * 12 / 100).max(2)
    }

    /// The printer's own font has three sizes, and asking for anything else gets the nearest —
    /// never a size the hardware cannot form.
    #[test]
    fn the_printer_font_has_only_three_answers() {
        let metrics = Metrics::printer_font(Paper::new(PaperKind::Mm80));
        let mut seen = std::collections::BTreeSet::new();
        for cap in Style::LADDER {
            let m = metrics.for_cap(cap);
            assert!((1..=3).contains(&m.scale));
            assert_eq!(m.advance, 12 * u32::from(m.scale));
            seen.insert(m.scale);
        }
        assert_eq!(seen.len(), 3, "the ladder must reach all three multipliers");
    }

    /// 80 mm paper at the body size is the density the product shipped with, so no shop's bill
    /// suddenly needs a second line for a name that fitted.
    #[test]
    fn the_body_size_keeps_a_usable_line_on_every_paper() {
        for (kind, least) in [
            (PaperKind::Mm58, 26),
            (PaperKind::Mm80, 40),
            (PaperKind::Mm100, 56),
        ] {
            let metrics = face_metrics(kind);
            let across = metrics.body().chars_across(metrics.dots());
            assert!(
                across >= least,
                "{kind:?} fits only {across} characters at the body size"
            );
        }
    }
}
