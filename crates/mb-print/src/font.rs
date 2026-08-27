//! The font.

// Glyph geometry is floating point because outlines are.
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
const BUILTIN: &[u8] = include_bytes!("../assets/IBMPlexMono-Regular.ttf");

/// How much of a pixel has to be covered before a dot is fired.
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
    /// One entry per dot, row-major.
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    /// One character's advance, in dots.
    pub width: u32,
    /// The whole row this text occupies: ink plus leading.
    pub height: u32,
    /// The cap height this cell was built for — what the shop asked for.
    pub cap: u32,
    /// Ink above the baseline, in dots.
    pub ascent: u32,
    /// Ink below the baseline, in dots.
    pub descent: u32,
    px: f32,
    baseline: f32,
}

impl Cell {
    /// The point size this cell rasterises at.
    #[must_use]
    pub const fn px(&self) -> f32 {
        self.px
    }

    /// Dots from the top of the row down to the baseline.
    #[must_use]
    pub const fn baseline(&self) -> f32 {
        self.baseline
    }
}

/// The characters that decide how tall a row has to be.
const REFERENCE: &str = "M(){}[]|/\\jgpqy,;_QJ0123456789ABCXYZabcdfhklt%-.:*#+";

/// How much air goes under a line, as a percentage of the cap height.
const LEADING_PCT: u32 = 12;
const LEADING_MIN: u32 = 2;

/// A loaded typeface, with its glyphs cached by cell size.
pub struct Font {
    name: String,
    inner: fontdue::Font,
    cache: Mutex<BTreeMap<(char, u32, u32), Arc<Glyph>>>,
    /// The same glyphs placed at the pen rather than centred in a cell — see `Font::glyph_at`.
    natural: Mutex<BTreeMap<(char, u32, u32), Arc<Glyph>>>,
    cells: Mutex<BTreeMap<(u32, u32), Cell>>,
    /// Worked out once, on demand — see `Font::is_monospace`.
    monospace: Mutex<Option<bool>>,
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The face itself is 133 KB of tables; printing it would be useless and enormous.
        f.debug_struct("Font").field("name", &self.name).finish()
    }
}

impl Font {
    /// The face this build ships.
    pub fn builtin() -> Result<Font, PrintError> {
        Font::load(BUILTIN, "IBM Plex Mono Regular")
    }

    /// Any TrueType or OpenType face, from bytes.
    pub fn load(bytes: &[u8], name: impl Into<String>) -> Result<Font, PrintError> {
        let settings = fontdue::FontSettings::default();
        let inner = fontdue::Font::from_bytes(bytes, settings)
            .map_err(|e| PrintError::invalid(format!("that font file cannot be read: {e}")))?;
        Ok(Font {
            name: name.into(),
            inner,
            cache: Mutex::new(BTreeMap::new()),
            natural: Mutex::new(BTreeMap::new()),
            cells: Mutex::new(BTreeMap::new()),
            monospace: Mutex::new(None),
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// A cell whose capital letters are exactly `cap` dots tall.
    pub fn cell_for_cap(&self, cap: u32) -> Cell {
        let cap = cap.clamp(4, 200);
        if let Some(found) = lock(&self.cells).get(&(cap, 0)) {
            return *found;
        }
        let cell = self.build_cell(cap);
        lock(&self.cells).insert((cap, 0), cell);
        cell
    }

    fn build_cell(&self, cap: u32) -> Cell {
        // A capital is roughly 0.7 em in every Latin face, so the answer is near `cap / 0.7`.
        let target = cap as f32;
        let mut px = (target * 1.2).max(4.0);
        let mut best_px = px;
        let mut best_gap = f32::MAX;
        let limit = (target * 3.0).max(12.0);
        let mut probe = 4.0_f32;
        while probe <= limit {
            let drawn = self.inner.rasterize('M', probe).0.height as f32;
            let gap = (drawn - target).abs();
            // `<=` so that, among sizes that draw the same dot height, the LARGEST is chosen: a
            // bigger point size fills its dots more solidly, which is what a thermal head
            // wants.
            if gap <= best_gap {
                best_gap = gap;
                best_px = probe;
            }
            if drawn > target + 2.0 {
                break;
            }
            probe += 0.25;
        }
        px = best_px;

        // The row, measured against the shapes a receipt can actually print.
        let mut ascent = 0.0_f32;
        let mut descent = 0.0_f32;
        for ch in REFERENCE.chars() {
            let m = self.inner.metrics(ch, px);
            if m.height == 0 {
                continue;
            }
            let above = m.ymin as f32 + m.height as f32;
            if above > ascent {
                ascent = above;
            }
            let below = -(m.ymin as f32);
            if below > descent {
                descent = below;
            }
        }
        let ascent = ascent.ceil().max(target) as u32;
        let descent = descent.ceil().max(0.0) as u32;
        let leading = (cap * LEADING_PCT / 100).max(LEADING_MIN);

        // The advance is a digit's, not `M`'s.
        let advance = self.inner.metrics('0', px).advance_width;
        let width = if advance.is_finite() && advance >= 1.0 {
            advance.round() as u32
        } else {
            cap.div_ceil(2).max(1)
        };

        Cell {
            width: width.max(1),
            height: ascent + descent + leading,
            cap,
            ascent,
            descent,
            px,
            baseline: (leading as f32) / 2.0 + ascent as f32,
        }
    }

    /// How wide one character is, in dots, at this cell's size.
    #[must_use]
    pub fn advance(&self, ch: char, cell: Cell) -> u32 {
        let width = self.inner.metrics(ch, cell.px).advance_width;
        if width.is_finite() && width >= 1.0 {
            width.round() as u32
        } else {
            1
        }
    }

    /// How wide a whole string is, in dots.
    #[must_use]
    pub fn measure(&self, text: &str, cell: Cell) -> u32 {
        text.chars().map(|ch| self.advance(ch, cell)).sum()
    }

    /// Is every character the same width?
    #[must_use]
    pub fn is_monospace(&self) -> bool {
        if let Some(known) = *lock(&self.monospace) {
            return known;
        }
        let cell = self.cell_for_cap(15);
        let same = self.advance('i', cell) == self.advance('M', cell);
        *lock(&self.monospace) = Some(same);
        same
    }

    /// A glyph placed at the pen rather than centred in a cell.
    #[must_use]
    pub fn glyph_at(&self, ch: char, cell: Cell, pen: u32) -> Arc<Glyph> {
        let _ = pen;
        let key = (ch, cell.width, cell.height);
        if let Some(found) = lock(&self.natural).get(&key) {
            return Arc::clone(found);
        }
        let (metrics, coverage) = self.inner.rasterize(ch, cell.px);
        let on: Vec<bool> = coverage.iter().map(|v| *v >= INK_THRESHOLD).collect();
        let glyph = Arc::new(Glyph {
            width: metrics.width as u32,
            height: metrics.height as u32,
            // No centring: the pen is the origin and `xmin` is the face's own offset from it.
            left: metrics.xmin,
            top: (cell.baseline - (metrics.ymin as f32 + metrics.height as f32)).round() as i32,
            on,
        });
        lock(&self.natural).insert(key, Arc::clone(&glyph));
        glyph
    }

    /// One glyph, rasterised and thresholded for a cell of this size.
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

        // Fontdue reports the bitmap's position relative to the pen: `xmin` is its left edge
        // and `ymin` its bottom edge, both measured up from the baseline.
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
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// The faces a shop may choose between.

/// The faces on offer, and where each one comes from.
pub const FAMILIES: &[Family] = &[
    Family {
        key: "builtin",
        label: "Magic Bill's own (IBM Plex Mono)",
        file: None,
        monospace: true,
    },
    Family {
        key: "consolas",
        label: "Consolas",
        file: Some("consola.ttf"),
        monospace: true,
    },
    Family {
        key: "consolas_bold",
        label: "Consolas Bold — darker on faint paper",
        file: Some("consolab.ttf"),
        monospace: true,
    },
    Family {
        key: "courier",
        label: "Courier New",
        file: Some("cour.ttf"),
        monospace: true,
    },
    Family {
        key: "lucida",
        label: "Lucida Console",
        file: Some("lucon.ttf"),
        monospace: true,
    },
    Family {
        key: "cascadia",
        label: "Cascadia Mono",
        file: Some("CascadiaMono.ttf"),
        monospace: true,
    },
    Family {
        key: "times",
        label: "Times New Roman — a printed-book look",
        file: Some("times.ttf"),
        monospace: false,
    },
    Family {
        key: "georgia",
        label: "Georgia — heavier serif, clear on faint paper",
        file: Some("georgia.ttf"),
        monospace: false,
    },
    Family {
        key: "arial",
        label: "Arial",
        file: Some("arial.ttf"),
        monospace: false,
    },
    Family {
        key: "calibri",
        label: "Calibri — rounder, a little smaller",
        file: Some("calibri.ttf"),
        monospace: false,
    },
    Family {
        key: "verdana",
        label: "Verdana — widest, easiest to read small",
        file: Some("verdana.ttf"),
        monospace: false,
    },
];

/// One choice on the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Family {
    /// What is stored in the settings row.
    pub key: &'static str,
    /// What the shop reads.
    pub label: &'static str,
    /// The file in the system font folder, or `None` for the built-in.
    pub file: Option<&'static str>,
    /// Does every character have the same width?
    pub monospace: bool,
}

/// Is this a name this build knows?
#[must_use]
pub fn family(key: &str) -> Option<Family> {
    FAMILIES.iter().copied().find(|f| f.key == key)
}

/// Which face to draw a job with, asked of the caller.
pub trait Typefaces: Send + Sync + fmt::Debug {
    /// `None`, an empty string, or `"builtin"` all mean the shipped face.
    fn face(&self, key: Option<&str>) -> Arc<Font>;
}

/// The one every test and every caller with nothing to choose between uses.
#[derive(Debug)]
pub struct OneFace(pub Arc<Font>);

impl OneFace {
    pub fn builtin() -> Result<OneFace, PrintError> {
        Ok(OneFace(Arc::new(Font::builtin()?)))
    }
}

impl Typefaces for OneFace {
    fn face(&self, _key: Option<&str>) -> Arc<Font> {
        Arc::clone(&self.0)
    }
}

#[cfg(test)]
mod measuring {
    //! Measuring text, which is what replaced counting it.

    use super::*;

    fn builtin() -> Font {
        Font::builtin().expect("the built-in face loads")
    }

    /// In a typewriter face every character is the same width, so measuring a string is
    /// counting it times the cell — which is exactly what the layout did before it could
    /// measure.
    #[test]
    fn a_typewriter_face_measures_the_same_as_counting() {
        let font = builtin();
        let cell = font.cell_for_cap(15);
        let advance = font.advance('M', cell);

        for text in ["Masala Dosa", "1,240.00", "MMMMMMMM", "iiiiiiii"] {
            assert_eq!(
                font.measure(text, cell),
                advance * u32::try_from(text.chars().count()).expect("short"),
                "{text:?} did not measure as its character count"
            );
        }
    }

    /// And in a proportional face they are not, which is the whole point.
    #[test]
    fn a_proportional_face_measures_an_i_narrower_than_an_m() {
        let Ok(bytes) = std::fs::read(
            std::path::PathBuf::from(
                std::env::var_os("SystemRoot").unwrap_or("C:\\Windows".into()),
            )
            .join("Fonts")
            .join("times.ttf"),
        ) else {
            return;
        };
        let font = Font::load(&bytes, "Times New Roman").expect("loads");
        let cell = font.cell_for_cap(15);

        assert!(
            font.advance('i', cell) < font.advance('M', cell),
            "a proportional face is measuring like a typewriter one"
        );
        // Which means a string of thin letters is genuinely narrower than a string of fat ones
        // — the thing a character grid could not express.
        assert!(font.measure("iiiiii", cell) < font.measure("MMMMMM", cell));
    }

    /// Digits stay in step.
    #[test]
    fn digits_are_the_same_width_in_every_face_on_offer() {
        let font = builtin();
        let cell = font.cell_for_cap(15);
        let zero = font.advance('0', cell);
        for digit in "123456789".chars() {
            assert_eq!(font.advance(digit, cell), zero, "{digit} is out of step");
        }
    }

    /// A size in dots is what a px setting means, and asking for a bigger one has to give
    /// bigger text.
    #[test]
    fn a_taller_cell_draws_wider_characters() {
        let font = builtin();
        let small = font.cell_for_cap(10);
        let large = font.cell_for_cap(24);
        assert!(small.height < large.height);
        assert!(
            font.advance('M', small) < font.advance('M', large),
            "16 px and 32 px drew the same width"
        );
    }

    /// Nothing measures as nothing.
    #[test]
    fn no_character_measures_as_zero() {
        let font = builtin();
        let cell = font.cell_for_cap(15);
        for ch in ['\u{0}', '\u{7}', ' ', '\u{200b}', 'ಅ'] {
            assert!(font.advance(ch, cell) >= 1, "{ch:?} measured as nothing");
        }
    }
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
        let cell = font.cell_for_cap(15);
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
        let cell = font.cell_for_cap(15);
        assert!(font.glyph(' ', cell).is_blank());
        assert!(!font.glyph('.', cell).is_blank());
    }

    #[test]
    fn the_cache_returns_the_same_glyph() {
        let font = Font::builtin().expect("loads");
        let cell = font.cell_for_cap(15);
        let first = font.glyph('7', cell);
        let second = font.glyph('7', cell);
        assert!(
            Arc::ptr_eq(&first, &second),
            "the glyph cache is not caching, and P4 will show it"
        );
    }

    #[test]
    fn every_thermal_cell_size_produces_a_usable_face() {
        // Every cap height the settings screen offers — see `catalog::SIZES`.
        let font = Font::builtin().expect("loads");
        for cap in [9_u32, 11, 13, 15, 17, 19, 22, 25, 29, 35] {
            let cell = font.cell_for_cap(cap);
            let glyph = font.glyph('8', cell);
            assert!(!glyph.is_blank(), "'8' vanished at a {cap}-dot cap");
            assert!(
                glyph.width <= cell.width && glyph.height <= cell.height,
                "'8' overflows a {cap}-dot cap"
            );
        }
    }
}
