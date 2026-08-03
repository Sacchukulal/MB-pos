//! The font — **the decision D31 deliberately left to this session.**
//!
//! P06 shipped the `Sink` trait, the one traversal and the two sinks that need
//! no font, and stopped there on purpose:
//!
//! > *"A raster renderer needs a rasterisable font; there is none in this
//! > repository; embedding one is a dependency **and** a licence decision
//! > **and** several hundred kilobytes against S4's 20 MB installer; and the
//! > right size and threshold depend on the printer's dots-per-mm and its
//! > raster command — all of which are P07's to decide."*
//!
//! # What was chosen, and what it cost
//!
//! | | | |
//! |---|---|---|
//! | rasteriser | `fontdue` 0.9, default features off | ~60 KB of binary |
//! | face | IBM Plex Mono Regular | 133 KB in `assets/` |
//! | licence | SIL Open Font License 1.1, copied to `assets/OFL.txt` | — |
//!
//! Roughly 200 KB against S4's 20 MB installer budget — one per cent.
//!
//! **Why a real TrueType face and not a baked bitmap font.** A 12 × 24 bitmap
//! font would be four kilobytes and would be a dead end: it can never become
//! crown jewel 17's Kannada path, and that is the whole reason D31 put this
//! decision beside the printer rather than beside the layout. A face is loaded
//! from bytes ([`Font::load`]), so a second script is a *value* and not a code
//! path.
//!
//! **Why monospace.** P06's model is a character grid — `paper.dots()` divided
//! by `paper.columns()` — and the text sink and the raster sink have to agree
//! column for column or the drift this crate exists to prevent comes back by a
//! different route. `tests/raster.rs` asserts that agreement.
//!
//! **Why one weight.** Bold is double-strike: the glyph drawn twice, one dot
//! apart. That is exactly what a thermal printer does for emphasised text, so
//! the raster path and the printer's own font path agree about what bold looks
//! like — and a second face would be another 133 KB to disagree with.
//!
//! # Crown jewel 17, honestly
//!
//! > *"The graphics print engine… is also what will make Kannada/Hindi receipts
//! > possible later."*
//!
//! The picture path now exists. **The Kannada path does not, and this file is
//! where that is said out loud rather than implied.**
//!
//! A character grid cannot lay out Kannada. The script reorders vowel signs,
//! stacks consonants into conjuncts, and has no fixed advance width — a
//! "character" in [`crate::layout`]'s sense is not a unit that exists in it.
//! Rendering it needs three things this session does not build:
//!
//! 1. **shaping** — a HarfBuzz-class library that turns a string into
//!    positioned glyph ids. `rustybuzz` is the pure-Rust one;
//! 2. **a face with the script in it** — IBM Plex Sans Devanagari for Hindi and
//!    Noto Sans Kannada, both SIL OFL like the one here, both loaded through
//!    [`Font::load`];
//! 3. **wrapping by measured width instead of by counted characters.**
//!
//! Only the third touches anything outside a font module, and it is one seam:
//! `layout` counts `chars()` to decide where a line breaks. P23 makes that a
//! measurement the caller supplies, defaulting to "one column per character" so
//! that nothing about a Latin receipt changes. **Name it, do not build it** —
//! a width-measuring layout with no shaper behind it is a second layout engine
//! with nothing to show for itself.

// Glyph geometry is floating point because outlines are. D7's ban is about the
// MONEY path — nothing in this file touches an amount, and the one place a
// rounding could matter (a column that is not a whole number of dots) has a
// test in paper.rs that fails rather than a rounding.
#![allow(
    clippy::float_arithmetic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::integer_division,
    reason = "glyph geometry, not money — see the note above"
)]

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::error::PrintError;

/// The face this build ships.
///
/// IBM Plex Mono Regular, SIL Open Font License 1.1. The licence is committed
/// beside it in `assets/OFL.txt`; the OFL requires it to travel with the font,
/// so an installer that ships the binary must ship that file too — S4's job,
/// noted here because it is the kind of thing that gets found at release.
const BUILTIN: &[u8] = include_bytes!("../assets/IBMPlexMono-Regular.ttf");

/// How much of a pixel has to be covered before a dot is fired.
///
/// A thermal dot is on or off; there is no grey. 40 % rather than 50 % biases
/// towards ink, because at twelve dots to a character a thin stroke that
/// vanishes is much worse than one that thickens — and the paper is already
/// unforgiving of thin.
const INK_THRESHOLD: u8 = 100;

/// One rasterised glyph, thresholded to dots, positioned inside its cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub width: u32,
    pub height: u32,
    /// Dots from the left of the cell.
    pub left: i32,
    /// Dots from the top of the cell.
    pub top: i32,
    /// One entry per dot, row-major. `true` is ink.
    pub on: Vec<bool>,
}

impl Glyph {
    #[must_use]
    pub fn ink(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let index = (y as usize) * (self.width as usize) + (x as usize);
        self.on.get(index).copied().unwrap_or(false)
    }

    #[must_use]
    pub fn is_blank(&self) -> bool {
        !self.on.iter().any(|on| *on)
    }
}

/// The size of one character cell, in dots, and where its baseline sits.
///
/// **The cell comes from the paper, not from the font.** `paper.dots()` divided
/// by `paper.columns()` is 12 dots on 58 mm and 80 mm paper and 13 on 100 mm;
/// the font is then scaled to fit that, which is what keeps the raster sink and
/// the text sink on the same grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub width: u32,
    pub height: u32,
    px: f32,
    baseline: f32,
}

impl Cell {
    /// The cell for a column at scale 1 on this paper.
    ///
    /// Height is twice the width, which is the proportion ESC/POS Font A has
    /// always used (12 × 24) and the proportion a receipt reads best at.
    #[must_use]
    pub const fn for_column(dots_per_column: u32) -> (u32, u32) {
        (dots_per_column, dots_per_column * 2)
    }
}

/// A loaded typeface, with its glyphs cached by cell size.
///
/// Cloning is cheap and shares the cache: the queue hands one `Arc<Font>` to
/// every printer's worker, and two printers on different paper sizes want
/// different cells out of the same face.
pub struct Font {
    name: String,
    inner: fontdue::Font,
    cache: Mutex<BTreeMap<(char, u32, u32), Arc<Glyph>>>,
    cells: Mutex<BTreeMap<(u32, u32), Cell>>,
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The face itself is 133 KB of tables; printing it would be useless and
        // enormous. The name is what a person debugging a receipt wants.
        f.debug_struct("Font").field("name", &self.name).finish()
    }
}

impl Font {
    /// The face this build ships.
    pub fn builtin() -> Result<Font, PrintError> {
        Font::load(BUILTIN, "IBM Plex Mono Regular")
    }

    /// Any TrueType or OpenType face, from bytes.
    ///
    /// This is the seam crown jewel 17 needs: P23's Kannada face arrives here
    /// and nothing else in the crate changes.
    pub fn load(bytes: &[u8], name: impl Into<String>) -> Result<Font, PrintError> {
        let settings = fontdue::FontSettings::default();
        let inner = fontdue::Font::from_bytes(bytes, settings)
            .map_err(|e| PrintError::invalid(format!("that font file cannot be read: {e}")))?;
        Ok(Font {
            name: name.into(),
            inner,
            cache: Mutex::new(BTreeMap::new()),
            cells: Mutex::new(BTreeMap::new()),
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The largest pixel size whose glyphs fit a `width × height` dot cell,
    /// with the baseline placed so the line is vertically centred.
    ///
    /// Searched rather than computed from the font's own metrics, because the
    /// metrics describe the design and what matters here is what actually
    /// rasterises inside twelve dots. Cached: this runs once per cell size per
    /// boot, not once per character.
    pub fn cell(&self, width: u32, height: u32) -> Cell {
        if let Some(found) = lock(&self.cells).get(&(width, height)) {
            return *found;
        }

        // 'M' is the widest glyph in a monospace face's Latin range, and every
        // glyph in it shares one advance — so fitting 'M' fits everything.
        let mut best = Cell {
            width,
            height,
            px: 4.0,
            baseline: height as f32,
        };
        let mut px = 4.0_f32;
        while px <= (height as f32) * 1.5 {
            let metrics = self.inner.metrics('M', px);
            let line = self
                .inner
                .horizontal_line_metrics(px)
                .unwrap_or(fontdue::LineMetrics {
                    ascent: px,
                    descent: 0.0,
                    line_gap: 0.0,
                    new_line_size: px,
                });
            let ink_height = line.ascent - line.descent;
            if metrics.advance_width > width as f32 || ink_height > height as f32 {
                break;
            }
            // Centre what is left over, so a 24-dot cell holding a 20-dot face
            // does not sit on the floor of its row.
            let spare = (height as f32) - ink_height;
            best = Cell {
                width,
                height,
                px,
                baseline: line.ascent + spare / 2.0,
            };
            px += 0.5;
        }

        lock(&self.cells).insert((width, height), best);
        best
    }

    /// One glyph, rasterised and thresholded for a cell of this size.
    ///
    /// Cached by `(character, cell width, cell height)`. A receipt uses about
    /// ninety distinct characters at two or three scales, so the cache fills in
    /// the first few bills and never grows again — which is why P4 measures the
    /// second render and not the first.
    pub fn glyph(&self, ch: char, cell: Cell) -> Arc<Glyph> {
        let key = (ch, cell.width, cell.height);
        if let Some(found) = lock(&self.cache).get(&key) {
            return Arc::clone(found);
        }

        let (metrics, coverage) = self.inner.rasterize(ch, cell.px);
        let mut on = Vec::with_capacity(coverage.len());
        for value in &coverage {
            on.push(*value >= INK_THRESHOLD);
        }

        // fontdue reports the bitmap's position relative to the pen: `xmin` is
        // its left edge and `ymin` its bottom edge, both measured up from the
        // baseline. The cell's own left edge is the pen, shifted right by
        // whatever the advance leaves spare, so a narrow glyph sits centred in
        // its column instead of hugging the left of it.
        let spare_x = (cell.width as f32 - metrics.advance_width) / 2.0;
        let left = (spare_x + metrics.xmin as f32).round() as i32;
        let top = (cell.baseline - (metrics.ymin as f32 + metrics.height as f32)).round() as i32;

        let glyph = Arc::new(Glyph {
            width: metrics.width as u32,
            height: metrics.height as u32,
            left,
            top,
            on,
        });
        lock(&self.cache).insert(key, Arc::clone(&glyph));
        glyph
    }
}

/// Takes a lock, recovering from poisoning rather than propagating it.
///
/// The same trade `mb-db`'s connection pool makes: a panicking worker thread
/// must not stop the counter printing for the rest of the shift, and a glyph
/// cache cannot be left in a broken state — the worst case is a glyph
/// rasterised twice.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_face_loads() {
        let font = Font::builtin().expect("the built-in face must load");
        assert_eq!(font.name(), "IBM Plex Mono Regular");
    }

    #[test]
    fn a_glyph_fits_its_cell_and_has_ink_in_it() {
        let font = Font::builtin().expect("loads");
        let (w, h) = Cell::for_column(12);
        let cell = font.cell(w, h);
        let glyph = font.glyph('M', cell);

        assert!(!glyph.is_blank(), "'M' rasterised to nothing");
        assert!(
            glyph.left >= 0 && glyph.width <= cell.width,
            "'M' does not fit a {}-dot column: left {} width {}",
            cell.width,
            glyph.left,
            glyph.width
        );
        assert!(
            glyph.top >= 0 && glyph.top as u32 + glyph.height <= cell.height,
            "'M' does not fit a {}-dot line: top {} height {}",
            cell.height,
            glyph.top,
            glyph.height
        );
    }

    #[test]
    fn a_space_is_blank_and_a_full_stop_is_not() {
        let font = Font::builtin().expect("loads");
        let (w, h) = Cell::for_column(12);
        let cell = font.cell(w, h);
        assert!(font.glyph(' ', cell).is_blank());
        assert!(!font.glyph('.', cell).is_blank());
    }

    #[test]
    fn the_cache_returns_the_same_glyph() {
        let font = Font::builtin().expect("loads");
        let cell = font.cell(12, 24);
        let first = font.glyph('7', cell);
        let second = font.glyph('7', cell);
        assert!(
            Arc::ptr_eq(&first, &second),
            "the glyph cache is not caching, and P4 will show it"
        );
    }

    #[test]
    fn every_thermal_cell_size_produces_a_usable_face() {
        // 12 dots at 58 mm and 80 mm, 13 at 100 mm, and the same again doubled
        // and trebled for scale 2 and 3.
        let font = Font::builtin().expect("loads");
        for dots in [12_u32, 13, 24, 26, 36, 39] {
            let (w, h) = Cell::for_column(dots);
            let cell = font.cell(w, h);
            let glyph = font.glyph('8', cell);
            assert!(!glyph.is_blank(), "'8' vanished at a {dots}-dot column");
            assert!(
                glyph.width <= cell.width && glyph.height <= cell.height,
                "'8' overflows a {dots}-dot cell"
            );
        }
    }
}
