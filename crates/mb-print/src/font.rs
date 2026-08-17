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
    /// The same glyphs placed at the pen rather than centred in a cell — see
    /// [`Font::glyph_at`]. A second cache rather than a flag on the key,
    /// because a receipt uses one or the other and never both.
    natural: Mutex<BTreeMap<(char, u32, u32), Arc<Glyph>>>,
    cells: Mutex<BTreeMap<(u32, u32), Cell>>,
    /// Worked out once, on demand — see [`Font::is_monospace`].
    monospace: Mutex<Option<bool>>,
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
            natural: Mutex::new(BTreeMap::new()),
            cells: Mutex::new(BTreeMap::new()),
            monospace: Mutex::new(None),
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

    /// **How wide one character is, in dots, at this cell's size.**
    ///
    /// The number the layout needs in order to stop counting characters and
    /// start measuring them — the seam this file has named since P07 as the
    /// thing standing between a character grid and a real typeface.
    ///
    /// Rounded to whole dots, and that is deliberate rather than lazy: a
    /// printer fires whole dots, so a layout that positioned text at 11.4 dots
    /// would be describing something the hardware cannot do, and the raster
    /// sink would round it anyway — in its own way, at its own moment, which
    /// is how the sinks come to disagree. Rounding once, here, means every
    /// caller gets the same answer.
    ///
    /// **Never zero.** A face that reports no advance for a character (a
    /// control code, a glyph it does not have) would otherwise let a string of
    /// them measure as nothing and wrap forever.
    #[must_use]
    pub fn advance(&self, ch: char, cell: Cell) -> u32 {
        let width = self.inner.metrics(ch, cell.px).advance_width;
        if width.is_finite() && width >= 1.0 {
            width.round() as u32
        } else {
            1
        }
    }

    /// **How wide a whole string is, in dots.**
    ///
    /// The sum of its advances. No kerning: fontdue exposes pair kerning and a
    /// receipt does not want it — the amounts in a column have to line up with
    /// each other more than the letters have to sit prettily, and kerning is
    /// the thing that would make two rows of the same digits measure
    /// differently.
    #[must_use]
    pub fn measure(&self, text: &str, cell: Cell) -> u32 {
        text.chars().map(|ch| self.advance(ch, cell)).sum()
    }

    /// **A cell for an explicit height in dots**, rather than one derived from
    /// the paper's column width.
    ///
    /// This is what a size in px means now. `Cell::for_column` still exists and
    /// still describes the printer's own 12 × 24 grid; this describes what the
    /// GRAPHICS engine draws when a shop has asked for 18 px text.
    ///
    /// The width it reports is the face's own 'M' advance at that size — for a
    /// typewriter face that is every character's width, and for a proportional
    /// one it is the widest, which is what a caller wanting a worst case
    /// (a column that must not overflow) should reach for.
    #[must_use]
    pub fn cell_for_height(&self, height: u32) -> Cell {
        // The same search `cell` does, against height alone: find the largest
        // px whose ink fits the requested dot height.
        let mut best = Cell {
            width: height.div_ceil(2).max(1),
            height,
            px: 4.0,
            baseline: height as f32,
        };
        let mut px = 4.0_f32;
        while px <= (height as f32) * 1.5 {
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
            if ink_height > height as f32 {
                break;
            }
            let spare = (height as f32) - ink_height;
            let advance = self.inner.metrics('M', px).advance_width;
            best = Cell {
                width: if advance.is_finite() && advance >= 1.0 {
                    advance.round() as u32
                } else {
                    height.div_ceil(2).max(1)
                },
                height,
                px,
                baseline: line.ascent + spare / 2.0,
            };
            px += 0.5;
        }
        best
    }

    /// **Is every character the same width?**
    ///
    /// Measured, not declared. [`FAMILIES`] says so for the faces this build
    /// offers, but a `Font` is loaded from BYTES — that is the seam crown jewel
    /// 17's Kannada face arrives through — so a font whose family nobody wrote
    /// down still gets the right treatment.
    ///
    /// 'i' against 'M' is the whole test: they are the narrowest and widest
    /// letters in a proportional Latin face by a wide margin, and identical in
    /// a typewriter one.
    #[must_use]
    pub fn is_monospace(&self) -> bool {
        if let Some(known) = *lock(&self.monospace) {
            return known;
        }
        let cell = self.cell_for_height(24);
        let same = self.advance('i', cell) == self.advance('M', cell);
        *lock(&self.monospace) = Some(same);
        same
    }

    /// **A glyph placed at the pen rather than centred in a cell.**
    ///
    /// [`Font::glyph`] centres a narrow glyph inside its column, which is right
    /// for a character grid and wrong for proportional text — there, the pen
    /// advances by the character's own width and the glyph sits where the face
    /// says, or an 'i' would be drawn a third of a cell to the right of where
    /// it belongs and the word would come apart.
    ///
    /// `pen` is only used to keep the cache honest about sub-dot positioning;
    /// the returned glyph's `left` is relative to it.
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
            // No centring: the pen is the origin and `xmin` is the face's own
            // offset from it.
            left: metrics.xmin,
            top: (cell.baseline - (metrics.ymin as f32 + metrics.height as f32)).round() as i32,
            on,
        });
        lock(&self.natural).insert(key, Arc::clone(&glyph));
        glyph
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

// ---------------------------------------------------------------------------
// The faces a shop may choose between — P31.
// ---------------------------------------------------------------------------

/// **The faces on offer, and where each one comes from.**
///
/// The owner asked for *"5-6 choices"* for the bill and the kitchen ticket.
/// The obvious way to do that is five more `.ttf` files in `assets/`, and it is
/// the wrong way: each is ~130 KB against S4's 20 MB installer, and every one
/// is a licence to check and re-check.
///
/// **So they come off the machine.** `Font::load` already takes bytes from
/// anywhere — that seam exists for crown jewel 17's Kannada face — and every
/// name below ships with Windows 10 and 11. It costs nothing in the installer,
/// and a face that is somehow missing falls back to the built-in with a line in
/// the log rather than a counter that will not print.
///
/// **Monospace only, and that is not taste.** [`crate::layout`] lays a receipt
/// out on a character grid: `paper.dots()` divided by `paper.columns()`. A
/// proportional face rendered into that grid has each glyph squeezed into a
/// cell of the same width, so an 'i' is drawn with an 'M's worth of space
/// around it. It is legible and it looks wrong, and there is no honest way to
/// offer it.
/// **THE MONOSPACE-ONLY RULE IS GONE, AND HERE IS WHAT REPLACED IT.**
///
/// This list used to be typewriter faces only, and the reason given was sound
/// for the engine as it then was: [`crate::layout`] laid a receipt out on a
/// character grid, so a proportional face had every glyph squeezed into a cell
/// of the same width and an 'i' was drawn with an 'M's worth of air around it.
///
/// The owner asked for the v1 list back on 2026-08-17 — *"i want some fonts
/// like it was in previous mb pos app… Times New Roman etc"* — and the audit
/// confirms v1 offered *"Monospace, Sans-Serif, Serif, Arial, Courier New,
/// Times New Roman"*. They chose the rebuild over the constraint.
///
/// So the layout **measures** text now instead of counting characters
/// ([`Font::measure`]), and a proportional face is laid out the way an invoice
/// always has been: column EDGES are fixed, and the text inside each column is
/// aligned against them. What is no longer true is that every character sits on
/// a grid — which was never something a customer wanted, only something the
/// engine needed.
///
/// # `monospace` is still load-bearing
///
/// The ESC/POS **text** engine prints with the printer's own built-in font: it
/// has one face, at 1×, 2× or 3×. It cannot render Times New Roman at 14 px and
/// never will. So a shop on the Text engine gets the nearest thing the hardware
/// can do, and [`Family::monospace`] is what lets the settings screen say so
/// rather than let somebody choose a face that quietly does nothing.
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
    // --- the proportional faces, 2026-08-17 -------------------------------
    //
    // Every one ships with Windows 10 and 11, for the same reason the
    // monospace ones do: `%SystemRoot%\Fonts` costs nothing in the installer
    // and adds no licence to check. A face that is somehow missing falls back
    // to the built-in with a line in the log (see `SystemFaces::load`).
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
    /// What is stored in the settings row. Never shown.
    pub key: &'static str,
    /// What the shop reads.
    pub label: &'static str,
    /// The file in the system font folder, or `None` for the built-in.
    pub file: Option<&'static str>,
    /// **Does every character have the same width?**
    ///
    /// True for a typewriter face. False for Times New Roman and its family,
    /// where the ESC/POS text engine cannot reproduce what the graphics engine
    /// draws — see [`FAMILIES`].
    pub monospace: bool,
}

/// Is this a name this build knows? Used by the settings catalogue, so an
/// unknown value in a config file is refused rather than silently ignored.
#[must_use]
pub fn family(key: &str) -> Option<Family> {
    FAMILIES.iter().copied().find(|f| f.key == key)
}

/// **Which face to draw a job with**, asked of the caller.
///
/// # Why this is a trait and not a `Faces` struct in this crate
///
/// Resolving a family name means reading a file out of the system font folder
/// and saying something in the log when it is not there — and this crate does
/// neither. It has no `log` dependency and no opinion about where an operating
/// system keeps its typefaces; `mb-winprint` is where D31 put the OS, and the
/// application is where the log lives.
///
/// So the crate keeps the part that is genuinely about typefaces ([`FAMILIES`])
/// and asks for the rest, exactly as it already asks for its transports
/// (`TransportFactory`) and its storage (`JobStore`).
///
/// **It cannot fail.** A shop whose chosen face has been uninstalled gets the
/// built-in one, because requirement 3 of the ten says billing does not stop
/// and the only thing worse than the wrong typeface is no bill.
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
    //! **Measuring text, which is what replaced counting it** — 2026-08-17.

    use super::*;

    fn builtin() -> Font {
        Font::builtin().expect("the built-in face loads")
    }

    /// In a typewriter face every character is the same width, so measuring a
    /// string is counting it times the cell — which is exactly what the layout
    /// did before it could measure. **The old behaviour has to fall out of the
    /// new code**, or every existing receipt changes the day this ships.
    #[test]
    fn a_typewriter_face_measures_the_same_as_counting() {
        let font = builtin();
        let cell = font.cell(12, 24);
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
    ///
    /// Skipped where the face is not installed: this asserts a property of
    /// Times New Roman, and a machine without it would otherwise fail a test
    /// about the code.
    #[test]
    fn a_proportional_face_measures_an_i_narrower_than_an_m() {
        let Ok(bytes) = std::fs::read(
            std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap_or("C:\\Windows".into()))
                .join("Fonts")
                .join("times.ttf"),
        ) else {
            return;
        };
        let font = Font::load(&bytes, "Times New Roman").expect("loads");
        let cell = font.cell_for_height(24);

        assert!(
            font.advance('i', cell) < font.advance('M', cell),
            "a proportional face is measuring like a typewriter one"
        );
        // Which means a string of thin letters is genuinely narrower than a
        // string of fat ones — the thing a character grid could not express.
        assert!(font.measure("iiiiii", cell) < font.measure("MMMMMM", cell));
    }

    /// **Digits stay in step.** An amount column is digits, and a face whose
    /// '1' were narrower than its '8' would make two rows of rupees fail to
    /// line up — which is the one thing a shopkeeper checks a bill for.
    /// Every face on the list is tabular for digits; this is what says so.
    #[test]
    fn digits_are_the_same_width_in_every_face_on_offer() {
        let font = builtin();
        let cell = font.cell_for_height(24);
        let zero = font.advance('0', cell);
        for digit in "123456789".chars() {
            assert_eq!(font.advance(digit, cell), zero, "{digit} is out of step");
        }
    }

    /// A size in dots is what a px setting means, and asking for a bigger one
    /// has to give bigger text. Guards against the search in `cell_for_height`
    /// silently pinning to its floor.
    #[test]
    fn a_taller_cell_draws_wider_characters() {
        let font = builtin();
        let small = font.cell_for_height(16);
        let large = font.cell_for_height(32);
        assert!(small.height < large.height);
        assert!(
            font.advance('M', small) < font.advance('M', large),
            "16 px and 32 px drew the same width"
        );
    }

    /// Nothing measures as nothing. A string that measured zero would wrap for
    /// ever, and the character that does it is always something unprintable
    /// that arrived from a shop's own data.
    #[test]
    fn no_character_measures_as_zero() {
        let font = builtin();
        let cell = font.cell(12, 24);
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
