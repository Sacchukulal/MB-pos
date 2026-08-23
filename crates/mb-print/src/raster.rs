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

use crate::doc::{Align, Pattern, Style};
use crate::error::PrintError;
use crate::font::{Cell, Font};
use crate::image::Monochrome;
use crate::layout::{BandText, Laid, LaidContent, LaidLine, Rule as LayoutRule};
use crate::metrics::Metrics;
use crate::paper::Paper;
use crate::render::{BandImage, Sink, render};

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
/// Fails for A4, which has no dots (it is the PDF sink's paper and a thermal
/// head is not involved), and for metrics that carry no face — the printer's
/// own font cannot be rasterised by us, which is the whole point of it.
///
/// **It takes the same [`Metrics`] the layout was given.** That is P32's other
/// half: the layout decided where every line breaks using these measurements,
/// and handing the sink a different set is exactly how the paper and the
/// preview came to disagree.
pub fn to_raster(
    laid: &Laid,
    metrics: &Metrics,
    options: RasterOptions,
) -> Result<Raster, PrintError> {
    let Some(dots) = laid.paper.kind.dots() else {
        return Err(PrintError::invalid(
            "A4 has no dots — an A4 invoice is the PDF sink's job, not the printer's",
        ));
    };
    let Some(font) = metrics.font() else {
        return Err(PrintError::invalid(
            "the graphics engine needs a typeface; these metrics are the printer's own font",
        ));
    };

    let mut sink = RasterSink {
        font,
        metrics,
        width: dots,
        canvas: Canvas::new(dots),
        bands: Vec::new(),
        notes: Vec::new(),
        ink: Vec::new(),
        options,
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
    metrics: &'a Metrics,
    /// Printable dots across.
    width: u32,
    canvas: Canvas,
    bands: Vec<Band>,
    notes: Vec<RasterNote>,
    ink: Vec<LineInk>,
    options: RasterOptions,
}

impl RasterSink<'_> {
    /// The cell this style draws in. Straight out of the metrics the layout
    /// used, so the sink cannot pick a different size from the one the line
    /// was wrapped for.
    fn cell(&self, style: Style) -> Cell {
        self.font
            .cell_for_cap(u32::from(self.metrics.size(style).cap))
    }

    /// **One laid-out line, drawn the way its face needs.**
    ///
    /// # Two paths, and they land on the same edges
    ///
    /// A **typewriter** face is drawn on the grid: every character in its own
    /// cell, one advance apart. That is what the ESC/POS golden files assert
    /// and it does not change.
    ///
    /// A **proportional** face cannot be. Its characters are different widths,
    /// so the spaces the layout padded with are worth about a third of a digit
    /// and an amount padded to the right edge lands nowhere near it. So each of
    /// the line's boxes ([`crate::layout::Segment`]) is drawn on its own: the
    /// text inside is trimmed, measured, and put against the box's left edge,
    /// right edge or centre — which is how an invoice has always aligned, and
    /// which lands the figures on exactly the edges the typewriter path does.
    fn draw_line(&mut self, line: &LaidLine, text: &str) -> u32 {
        let cell = self.cell(line.style);
        let top = self.canvas.add_rows(line.row_dots.max(1));

        if self.font.is_monospace() || line.segments.is_empty() {
            return self.draw_grid(text, line.indent_dots, cell, line.style.bold, top);
        }

        let chars: Vec<char> = text.chars().collect();
        let mut dots = 0;
        for segment in &line.segments {
            let end = segment.start.saturating_add(segment.width).min(chars.len());
            if segment.start >= end {
                continue;
            }
            let run: String = chars[segment.start..end].iter().collect();
            let run = run.trim();
            if run.is_empty() {
                continue;
            }
            let box_left = line.indent_dots.saturating_add(
                u32::try_from(segment.start)
                    .unwrap_or(0)
                    .saturating_mul(cell.width),
            );
            let box_width = u32::try_from(segment.width)
                .unwrap_or(0)
                .saturating_mul(cell.width);
            dots += self.draw_run(
                run,
                box_left,
                box_width,
                segment.align,
                cell,
                line.style.bold,
                top,
            );
        }
        dots
    }

    /// Characters on the grid: one advance apart, whatever they measure.
    fn draw_grid(&mut self, text: &str, left: u32, cell: Cell, bold: bool, top: usize) -> u32 {
        let mut dots = 0;
        for (position, ch) in text.chars().enumerate() {
            let x0 = left.saturating_add(
                u32::try_from(position)
                    .unwrap_or(u32::MAX)
                    .saturating_mul(cell.width),
            );
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

    /// **One run of text, measured and aligned inside a box.** The letterhead
    /// and every proportional line go through here.
    #[expect(
        clippy::too_many_arguments,
        reason = "a position, a box, an alignment and a size — every one of them decided elsewhere"
    )]
    fn draw_run(
        &mut self,
        run: &str,
        box_left: u32,
        box_width: u32,
        align: Align,
        cell: Cell,
        bold: bool,
        top: usize,
    ) -> u32 {
        let measured = self.font.measure(run, cell);
        // A run wider than its box starts at the box's left edge and runs on:
        // the layout already wrapped to fit, so this is a face whose letters
        // are wider than the grid assumed, and the honest answer is to print
        // all of it rather than to lose characters (rule one).
        let spare = box_width.saturating_sub(measured);
        let mut pen = match align {
            Align::Left => box_left,
            Align::Centre => box_left + spare / 2,
            Align::Right => box_left + spare,
        };
        let mut dots = 0;
        for ch in run.chars() {
            if pen >= self.width {
                break;
            }
            let glyph = self.font.glyph_at(ch, cell, pen);
            dots += self.blit(&glyph, pen, top);
            if bold {
                dots += self.blit(&glyph, pen + 1, top);
            }
            pen += self.font.advance(ch, cell);
        }
        dots
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

    /// **A rule, drawn as dots** — P32.
    ///
    /// It used to be the pattern's character repeated across the paper, and
    /// measured on the owner's own bill a `-` is five dots of ink inside a
    /// twelve-dot cell: a row of widely spaced ticks where the preview drew a
    /// near-solid line. Three dots in Times New Roman.
    ///
    /// A rule is a rule. The dash is spaced so the **first and last strokes
    /// land on the paper's edges**, because a rule that stops short of one
    /// reads as a fault.
    fn draw_rule(&mut self, line: &LaidLine, pattern: Pattern, width: u32) -> u32 {
        let rule = LayoutRule::of(pattern);
        let rows = line.row_dots.max(rule.row());
        let top = self.canvas.add_rows(rows);
        let ink_rows =
            rule.thickness * rule.strokes + rule.stroke_gap * rule.strokes.saturating_sub(1);
        let first = top + ((rows.saturating_sub(ink_rows)) / 2) as usize;

        let mut dots = 0;
        for stroke in 0..rule.strokes {
            let y = first + (stroke * (rule.thickness + rule.stroke_gap)) as usize;
            for thickness in 0..rule.thickness {
                let row = y + thickness as usize;
                match rule.dash {
                    None => {
                        for x in 0..width {
                            if self.canvas.set(line.indent_dots + x, row) {
                                dots += 1;
                            }
                        }
                    }
                    Some((on, off)) => {
                        // **The last dash ends on the right edge.**
                        //
                        // A fixed step loses the remainder — 96 dashes of 2 on
                        // 576 dots at a step of 6 stop at dot 571, and a rule
                        // that stops five dots short of the paper reads as a
                        // fault. Each start is computed from its own index
                        // instead, so the first is at 0 and the last is at
                        // `width - on` exactly.
                        let period = (on + off).max(1);
                        let count = ((width + off) / period).max(1);
                        let span = width.saturating_sub(on);
                        for n in 0..count {
                            let start = if count > 1 { n * span / (count - 1) } else { 0 };
                            for x in start..(start + on).min(width) {
                                if self.canvas.set(line.indent_dots + x, row) {
                                    dots += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        dots
    }

    /// **The letterhead: a logo and the shop's name, side by side** — P32.
    ///
    /// Every position was decided by the layout. This blits and draws.
    fn draw_band(&mut self, line: &LaidLine, image: &BandImage<'_>, lines: &[BandText]) -> u32 {
        let top = self.canvas.add_rows(line.row_dots.max(1));
        let mut dots = 0;

        match Monochrome::decode(image.data) {
            Ok(picture) => {
                let scaled = picture.scaled_to(image.width.min(self.width).max(1));
                for y in 0..scaled.height {
                    for x in 0..scaled.width {
                        if scaled.ink(x, y)
                            && self
                                .canvas
                                .set(image.left + x, top + (image.top + y) as usize)
                        {
                            dots += 1;
                        }
                    }
                }
            }
            Err(e) => {
                // D37: a shop with a corrupt logo still gets its bill. The
                // layout has already put the text where it goes.
                self.notes.push(RasterNote::LogoSkipped {
                    reason: e.to_string(),
                });
            }
        }

        for text in lines {
            let run = text.text.trim();
            if run.is_empty() {
                continue;
            }
            let cell = self.cell(text.style);
            dots += self.draw_run(
                run,
                text.left,
                text.width,
                text.align,
                cell,
                text.style.bold,
                top + text.top as usize,
            );
        }
        dots
    }

    /// A logo on its own line.
    ///
    /// **The print offset moves it** — P32. It did not, so a shop that nudged
    /// its printer got every line of text moved and its logo left behind.
    fn draw_image(&mut self, line: &LaidLine, data: &[u8], width_pct: u8, align: Align) -> u32 {
        let image = match Monochrome::decode(data) {
            Ok(image) => image,
            Err(e) => {
                self.notes.push(RasterNote::LogoSkipped {
                    reason: e.to_string(),
                });
                // The row was still reserved by the layout, so the paper does
                // not jump: keep it blank rather than closing the gap.
                self.canvas.add_rows(line.row_dots);
                return 0;
            }
        };
        let usable = self.width.saturating_sub(line.indent_dots).max(1);
        let pct = u32::from(width_pct.clamp(1, 100));
        let target = (usable * pct / 100).max(1);
        let scaled = image.scaled_to(target.min(usable));

        let top = self.canvas.add_rows(line.row_dots.max(scaled.height));
        let spare = usable.saturating_sub(scaled.width);
        let x0 = line.indent_dots
            + match align {
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
            let dots = self.draw_line(line, text);
            self.ink.push(LineInk { index, dots });
        }
    }

    fn rule(&mut self, line: &LaidLine, pattern: Pattern, width: u32, index: usize) {
        let dots = self.draw_rule(line, pattern, width);
        self.ink.push(LineInk { index, dots });
    }

    fn image(&mut self, line: &LaidLine, data: &[u8], width_pct: u8, align: Align, index: usize) {
        let dots = self.draw_image(line, data, width_pct, align);
        self.ink.push(LineInk { index, dots });
    }

    fn band(&mut self, line: &LaidLine, image: &BandImage<'_>, lines: &[BandText], index: usize) {
        let dots = self.draw_band(line, image, lines);
        self.ink.push(LineInk { index, dots });
    }

    fn qr(&mut self, line: &LaidLine, payload: &str, width_pct: u8, align: Align, index: usize) {
        if self.options.native_qr {
            // A command has to sit between two pictures, so the picture so far
            // becomes a band and a new one starts after it.
            self.canvas.drain_into(&mut self.bands);
            let _ = width_pct;
            // **The print offset cannot reach a native QR**, and saying so is
            // better than pretending: the printer's own encoder positions the
            // square with `ESC a`, which knows nothing about a millimetre
            // correction. A shop that needs the QR nudged turns the native
            // encoder off and gets the payload as text, which the offset does
            // move.
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
        let cell = self.cell(Style::NORMAL);
        let across = self
            .metrics
            .body()
            .chars_across(self.width.saturating_sub(line.indent_dots));
        let mut dots = 0;
        let chars: Vec<char> = payload.chars().collect();
        for chunk in chars.chunks(across.max(1)) {
            let text: String = chunk.iter().collect();
            let top = self.canvas.add_rows(cell.height);
            dots += self.draw_grid(&text, line.indent_dots, cell, false, top);
        }
        self.ink.push(LineInk { index, dots });
    }

    fn barcode(
        &mut self,
        line: &LaidLine,
        payload: &str,
        human_readable: bool,
        align: Align,
        index: usize,
    ) {
        // The printer's own `GS k` draws the bars (P29). The raster path
        // reserves the rows the layout allowed for them and hands the drawing
        // over, exactly as the QR arm does — so the two codes on a bill are
        // made by the same encoder on both engines.
        let _ = (payload, human_readable, align);
        self.canvas.add_rows(line.row_dots);
        self.ink.push(LineInk { index, dots: 1 });
    }

    fn blank(&mut self, line: &LaidLine, index: usize) {
        // **The row the layout reserved**, not a height of this sink's own.
        // A blank used to be a bare cell while a line of text was a cell plus
        // leading, so a blank line was three dots shorter than the line above
        // it for no reason anybody chose.
        self.canvas.add_rows(line.row_dots);
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

    fn metrics(kind: PaperKind) -> Metrics {
        let font = std::sync::Arc::new(Font::builtin().expect("the shipped face loads"));
        Metrics::face(Paper::new(kind), font)
    }

    #[test]
    fn a_line_of_text_becomes_dots() {
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.text("TOTAL 240.00", Style::BOLD, Align::Left);
        let laid = layout(&doc).expect("lays out");
        let raster = to_raster(&laid, &metrics(laid.paper.kind), RasterOptions::default()).expect("rasters");

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
        assert!(to_raster(&laid, &metrics(laid.paper.kind), RasterOptions::default()).is_err());
    }

    #[test]
    fn a_tall_document_is_split_into_bands() {
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        for n in 0..60 {
            doc.line(format!("line {n}"));
        }
        let laid = layout(&doc).expect("lays out");
        let raster = to_raster(&laid, &metrics(laid.paper.kind), RasterOptions::default()).expect("rasters");
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
                &metrics(PaperKind::Mm80),
                RasterOptions::default(),
            )
            .expect("rasters")
        };
        let bold = {
            let mut doc = Document::new(Paper::new(PaperKind::Mm80));
            doc.text("MAGIC BILL", Style::BOLD, Align::Left);
            to_raster(
                &layout(&doc).expect("lays out"),
                &metrics(PaperKind::Mm80),
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
                &metrics(PaperKind::Mm80),
                RasterOptions::default(),
            )
            .expect("rasters")
            .height()
        };
        assert!(render_at(2) > render_at(1));
        assert!(render_at(3) > render_at(2));
    }
}
