//! **The raster sink — crown jewel 17, and the seat D31 built for it.**
//!
//! > *"The graphics print engine. Printing the bill as a picture is what makes
//! > 'what you see is what you get' possible — and it is also what will make
//! > Kannada/Hindi receipts possible later."*
//!
//! This is the third sink. It is a [`Sink`] like the other two, it is called by
//! the same one traversal, and adding it changed nothing else in this crate —
//! which was the test D31 set for D29's shape.
//!
//! # It decides nothing about layout
//!
//! Every position it uses was already decided:
//!
//! * a column is `paper.dots_per_column()` dots at scale 1 — 12 on 58 mm and
//!   80 mm paper, 13 on 100 mm, and every thermal size divides evenly (there is
//!   a test in [`crate::paper`] that keeps it that way);
//! * a character at scale *n* occupies *n* columns and *n* rows of cell, and
//!   [`crate::layout`] has already halved or thirded the usable width to pay
//!   for it;
//! * **`LaidLine::indent` already contains the print offset** (scope 7.11).
//!   Applying it again would double the owner's correction, and that is the
//!   single likeliest bug in this session.
//!
//! If this file ever starts measuring text and deciding where to break it, stop
//! — that is a second layout engine, and a second layout engine is audit D1
//! coming back by a different route.
//!
//! # Bands, not one picture
//!
//! The output is a list of [`Band`]s rather than a single bitmap, for two
//! reasons that are both about the printer:
//!
//! * a QR code is printed by the printer's own encoder (D36), which means a
//!   command *between* two pictures;
//! * printers have a finite image buffer, and a receipt is up to two thousand
//!   dots tall. Every real ESC/POS driver sends rasters in slices.
//!
//! # And it never leaves this process
//!
//! Scope 17.11: a 576 × 1800 bitmap is about 130 KB, and thirteen of them would
//! be the whole 10 MB monthly egress budget. The bitmap exists between the
//! layout and the printer and nowhere else — not in the outbox, not in a
//! backup, not in a log.

// Dots into bytes, columns into dots. No amount is computed in this file; the
// templates hand `Money::to_plain_string` in as text and it is drawn, not
// arithmetic.
#![allow(
    clippy::integer_division,
    reason = "dots and columns, not money"
)]

use serde::{Deserialize, Serialize};

use crate::doc::{Align, Pattern};
use crate::error::PrintError;
use crate::font::{Cell, Font};
use crate::image::Monochrome;
use crate::layout::{Laid, LaidContent, LaidLine};
use crate::paper::Paper;
use crate::render::{Sink, render};

/// How many dot rows one `GS v 0` carries.
///
/// 512 rows on 80 mm paper is 36 KB, which every printer this product will meet
/// can hold. Splitting is free; a printer that runs out of buffer mid-image
/// prints half a bill and there is no way to find out that it did.
const MAX_BAND_ROWS: u32 = 512;

/// A dot image, packed the way `GS v 0` wants it: rows of bytes, most
/// significant bit leftmost, a set bit meaning "fire this dot".
pub type Bitmap = Monochrome;

/// One piece of a rasterised document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "band", rename_all = "snake_case")]
pub enum Band {
    /// Dots, to be sent as a raster image.
    Ink { image: Bitmap },
    /// A QR code the **printer** draws (D36). Carrying the payload rather than
    /// a picture is what saves this crate a QR encoder.
    Qr {
        payload: String,
        /// Module size in printer units, 1–16.
        module: u8,
        align: Align,
    },
}

/// Something the raster sink had to do that a person might want to know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "note", rename_all = "snake_case")]
pub enum RasterNote {
    /// A logo that could not be read. The bill still printed (D37).
    LogoSkipped { reason: String },
    /// The printer has no QR encoder, so the payload was printed as text — the
    /// same thing the text sink does, and a URI a customer can type is worth
    /// more than a blank square.
    QrAsText,
}

/// How much ink one line of the document produced.
///
/// This exists for the anti-drift test. A bitmap has no text to search, so the
/// claim "no sink can drop anything" is made where it is true: every line the
/// traversal handed over produced dots, and a line it called blank produced
/// none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineInk {
    pub index: usize,
    pub dots: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raster {
    pub paper: Paper,
    pub bands: Vec<Band>,
    pub notes: Vec<RasterNote>,
    pub ink: Vec<LineInk>,
}

impl Raster {
    /// Total dot rows, across every ink band.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.bands
            .iter()
            .map(|b| match b {
                Band::Ink { image } => image.height,
                Band::Qr { .. } => 0,
            })
            .sum()
    }

    /// Ink produced by one line of the traversal.
    #[must_use]
    pub fn dots_for_line(&self, index: usize) -> Option<u32> {
        self.ink.iter().find(|l| l.index == index).map(|l| l.dots)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterOptions {
    /// Whether this printer has `GS ( k` — the built-in QR encoder. When it
    /// does not, the payload is drawn as text instead.
    pub native_qr: bool,
    /// Module size for that encoder.
    pub qr_module: u8,
}

impl Default for RasterOptions {
    fn default() -> Self {
        RasterOptions {
            native_qr: true,
            // 6 dots a module puts a 25×25 UPI QR at about 150 dots — a quarter
            // of 80 mm paper, and every phone camera reads it first time.
            qr_module: 6,
        }
    }
}

/// Draw a laid-out document as dots.
///
/// Fails only for A4, which has no dots: it is the PDF sink's paper and a
/// thermal head is not involved.
pub fn to_raster(
    laid: &Laid,
    font: &Font,
    options: RasterOptions,
) -> Result<Raster, PrintError> {
    let Some(dots) = laid.paper.kind.dots() else {
        return Err(PrintError::invalid(
            "A4 has no dots — an A4 invoice is the PDF sink's job, not the printer's",
        ));
    };
    let Some(per_column) = laid.paper.dots_per_column() else {
        return Err(PrintError::invalid("this paper has no column width in dots"));
    };

    let mut sink = RasterSink {
        font,
        width: dots,
        per_column,
        canvas: Canvas::new(dots),
        bands: Vec::new(),
        notes: Vec::new(),
        ink: Vec::new(),
        options,
        columns: laid.paper.columns(),
    };
    render(laid, &mut sink);

    Ok(Raster {
        paper: laid.paper,
        bands: sink.bands,
        notes: sink.notes,
        ink: sink.ink,
    })
}

// ---------------------------------------------------------------------------
// The canvas: rows of packed dots, grown a line at a time.
//
// Rows rather than one buffer, because a receipt's height is not known until
// the last block has been drawn, and re-allocating a 130 KB buffer per line is
// how P4 would have missed its budget.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Canvas {
    width: u32,
    stride: usize,
    rows: Vec<Vec<u8>>,
}

impl Canvas {
    fn new(width: u32) -> Canvas {
        Canvas {
            width,
            stride: Monochrome::stride(width),
            rows: Vec::new(),
        }
    }

    /// Adds `n` blank rows and returns the index of the first.
    fn add_rows(&mut self, n: u32) -> usize {
        let first = self.rows.len();
        for _ in 0..n {
            self.rows.push(vec![0; self.stride]);
        }
        first
    }

    fn set(&mut self, x: u32, y: usize) -> bool {
        if x >= self.width {
            return false;
        }
        let Some(row) = self.rows.get_mut(y) else {
            return false;
        };
        let index = (x as usize) / 8;
        let bit = 7 - ((x as usize) % 8);
        let Some(byte) = row.get_mut(index) else {
            return false;
        };
        let was = *byte >> bit & 1 == 1;
        *byte |= 1 << bit;
        !was
    }

    fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Splits into bands of at most [`MAX_BAND_ROWS`] rows and empties itself.
    fn drain_into(&mut self, bands: &mut Vec<Band>) {
        if self.rows.is_empty() {
            return;
        }
        let rows = std::mem::take(&mut self.rows);
        for chunk in rows.chunks(MAX_BAND_ROWS as usize) {
            let mut bits = Vec::with_capacity(chunk.len() * self.stride);
            for row in chunk {
                bits.extend_from_slice(row);
            }
            bands.push(Band::Ink {
                image: Monochrome {
                    width: self.width,
                    height: u32::try_from(chunk.len()).unwrap_or(0),
                    bits,
                },
            });
        }
    }
}

#[derive(Debug)]
struct RasterSink<'a> {
    font: &'a Font,
    /// Printable dots across.
    width: u32,
    /// Dots in one column at scale 1.
    per_column: u32,
    columns: usize,
    canvas: Canvas,
    bands: Vec<Band>,
    notes: Vec<RasterNote>,
    ink: Vec<LineInk>,
    options: RasterOptions,
}

impl RasterSink<'_> {
    /// The gap under a line of text.
    ///
    /// An eighth of the cell, which is what ESC/POS's default line spacing
    /// comes to for its own 24-dot font (30 dots for 24). Receipts are dense;
    /// this is the difference between dense and cramped.
    const fn leading(cell_height: u32) -> u32 {
        cell_height / 8
    }

    /// Draw one string of `scale`-sized characters starting at `indent`
    /// scale-1 columns from the left edge. Returns the dots it fired.
    fn draw_text(&mut self, text: &str, indent: usize, scale: u32, bold: bool) -> u32 {
        let cell_w = self.per_column * scale;
        let (_, base_height) = Cell::for_column(self.per_column);
        let cell_h = base_height * scale;
        let cell = self.font.cell(cell_w, cell_h);
        let top = self.add_line_rows(cell_h);

        let mut dots = 0;
        for (position, ch) in text.chars().enumerate() {
            let column = indent + position * (scale as usize);
            let x0 = u32::try_from(column)
                .unwrap_or(u32::MAX)
                .saturating_mul(self.per_column);
            if x0 >= self.width {
                // The layout guarantees this cannot happen; if it ever does,
                // stopping is better than wrapping a character onto the row
                // above, which is what writing past the edge would look like.
                break;
            }
            let glyph = self.font.glyph(ch, cell);
            dots += self.blit(&glyph, x0, top);
            if bold {
                // Double-strike: the same glyph one dot right. It is what the
                // printer's own emphasised mode does, so the two paths agree
                // about what bold looks like.
                dots += self.blit(&glyph, x0 + 1, top);
            }
        }
        dots
    }

    fn add_line_rows(&mut self, cell_h: u32) -> usize {
        self.canvas.add_rows(cell_h + RasterSink::leading(cell_h))
    }

    fn blit(&mut self, glyph: &crate::font::Glyph, x0: u32, top: usize) -> u32 {
        let mut dots = 0;
        for gy in 0..glyph.height {
            for gx in 0..glyph.width {
                if !glyph.ink(gx, gy) {
                    continue;
                }
                let x = i64::from(x0) + i64::from(glyph.left) + i64::from(gx);
                let y = i64::from(glyph.top) + i64::from(gy);
                if x < 0 || y < 0 {
                    continue;
                }
                let Ok(x) = u32::try_from(x) else { continue };
                let Ok(y) = usize::try_from(y) else { continue };
                if self.canvas.set(x, top + y) {
                    dots += 1;
                }
            }
        }
        dots
    }

    fn draw_image(&mut self, data: &[u8], width_pct: u8, align: Align) -> u32 {
        let image = match Monochrome::decode(data) {
            Ok(image) => image,
            Err(e) => {
                // D37: a shop with a corrupt logo still gets its bill.
                self.notes.push(RasterNote::LogoSkipped {
                    reason: e.to_string(),
                });
                return 0;
            }
        };
        let pct = u32::from(width_pct.clamp(1, 100));
        let target = (self.width * pct / 100).max(1);
        let scaled = image.scaled_to(target.min(self.width));

        let top = self.canvas.add_rows(scaled.height);
        let spare = self.width.saturating_sub(scaled.width);
        let x0 = match align {
            Align::Left => 0,
            Align::Centre => spare / 2,
            Align::Right => spare,
        };

        let mut dots = 0;
        for y in 0..scaled.height {
            for x in 0..scaled.width {
                if scaled.ink(x, y) && self.canvas.set(x0 + x, top + y as usize) {
                    dots += 1;
                }
            }
        }
        dots
    }
}

impl Sink for RasterSink<'_> {
    fn line(&mut self, line: &LaidLine, index: usize) {
        if let LaidContent::Text { text } = &line.content {
            let scale = u32::from(line.style.scale());
            let dots = self.draw_text(text, line.indent, scale, line.style.bold);
            self.ink.push(LineInk { index, dots });
        }
    }

    fn rule(&mut self, pattern: Pattern, width: usize, indent: usize, index: usize) {
        // Drawn as the same repeated character the text sink writes, so the two
        // sinks cannot disagree about how long a separator is.
        let body: String = std::iter::repeat_n(pattern.glyph(), width).collect();
        let dots = self.draw_text(&body, indent, 1, false);
        self.ink.push(LineInk { index, dots });
    }

    fn image(&mut self, data: &[u8], width_pct: u8, align: Align, index: usize) {
        let dots = self.draw_image(data, width_pct, align);
        self.ink.push(LineInk { index, dots });
    }

    fn qr(&mut self, payload: &str, width_pct: u8, align: Align, index: usize) {
        if self.options.native_qr {
            // A command has to sit between two pictures, so the picture so far
            // becomes a band and a new one starts after it.
            self.canvas.drain_into(&mut self.bands);
            let _ = width_pct;
            self.bands.push(Band::Qr {
                payload: payload.to_owned(),
                module: self.options.qr_module.clamp(1, 16),
                align,
            });
            // A QR is not "no ink"; recording zero would make the anti-drift
            // test think the sink dropped it. One dot per module, near enough,
            // and the test only asks whether anything happened.
            self.ink.push(LineInk {
                index,
                dots: u32::from(self.options.qr_module),
            });
            return;
        }

        self.notes.push(RasterNote::QrAsText);
        let width = self.columns.max(1);
        let mut dots = 0;
        let chars: Vec<char> = payload.chars().collect();
        for chunk in chars.chunks(width) {
            let text: String = chunk.iter().collect();
            dots += self.draw_text(&text, 0, 1, false);
        }
        self.ink.push(LineInk { index, dots });
    }

    fn blank(&mut self, index: usize) {
        let (_, height) = Cell::for_column(self.per_column);
        self.canvas.add_rows(height);
        self.ink.push(LineInk { index, dots: 0 });
    }

    fn finish(&mut self) {
        if !self.canvas.is_empty() {
            self.canvas.drain_into(&mut self.bands);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Align, Document, Style};
    use crate::layout::layout;
    use crate::paper::PaperKind;

    fn font() -> Font {
        Font::builtin().expect("the shipped face loads")
    }

    #[test]
    fn a_line_of_text_becomes_dots() {
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.text("TOTAL 240.00", Style::BOLD, Align::Left);
        let laid = layout(&doc).expect("lays out");
        let raster = to_raster(&laid, &font(), RasterOptions::default()).expect("rasters");

        assert_eq!(raster.paper.kind, PaperKind::Mm80);
        assert!(raster.height() > 0, "nothing was drawn");
        assert!(
            raster.ink.iter().any(|l| l.dots > 0),
            "the line produced no ink"
        );
        for band in &raster.bands {
            if let Band::Ink { image } = band {
                assert_eq!(image.width, 576, "80 mm paper is 576 dots");
            }
        }
    }

    #[test]
    fn a4_has_no_dots_and_says_so_rather_than_guessing() {
        let mut doc = Document::new(Paper::new(PaperKind::A4));
        doc.line("INVOICE");
        let laid = layout(&doc).expect("lays out");
        assert!(to_raster(&laid, &font(), RasterOptions::default()).is_err());
    }

    #[test]
    fn a_tall_document_is_split_into_bands() {
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        for n in 0..60 {
            doc.line(format!("line {n}"));
        }
        let laid = layout(&doc).expect("lays out");
        let raster = to_raster(&laid, &font(), RasterOptions::default()).expect("rasters");
        assert!(
            raster.bands.len() > 1,
            "sixty lines is more than one band and a printer's buffer is finite"
        );
        for band in &raster.bands {
            if let Band::Ink { image } = band {
                assert!(image.height <= MAX_BAND_ROWS);
            }
        }
    }

    #[test]
    fn bold_fires_more_dots_than_plain() {
        let plain = {
            let mut doc = Document::new(Paper::new(PaperKind::Mm80));
            doc.text("MAGIC BILL", Style::NORMAL, Align::Left);
            to_raster(
                &layout(&doc).expect("lays out"),
                &font(),
                RasterOptions::default(),
            )
            .expect("rasters")
        };
        let bold = {
            let mut doc = Document::new(Paper::new(PaperKind::Mm80));
            doc.text("MAGIC BILL", Style::BOLD, Align::Left);
            to_raster(
                &layout(&doc).expect("lays out"),
                &font(),
                RasterOptions::default(),
            )
            .expect("rasters")
        };
        let sum = |r: &Raster| r.ink.iter().map(|l| l.dots).sum::<u32>();
        assert!(
            sum(&bold) > sum(&plain),
            "double-strike drew no extra dots, so bold is invisible on paper"
        );
    }

    #[test]
    fn a_bigger_scale_is_taller() {
        let render_at = |scale: u8| {
            let mut doc = Document::new(Paper::new(PaperKind::Mm80));
            doc.text("HELLO", Style::new(scale, false), Align::Left);
            to_raster(
                &layout(&doc).expect("lays out"),
                &font(),
                RasterOptions::default(),
            )
            .expect("rasters")
            .height()
        };
        assert!(render_at(2) > render_at(1));
        assert!(render_at(3) > render_at(2));
    }
}
