//! **One traversal. Renderers are sinks.**
//!
//! This is the real answer to audit D1, and it is a stronger answer than the
//! one the prompt for this session originally asked for.
//!
//! > *"The same bill is drawn three separate times, by hand, in three places…
//! > The code even carries a note saying 'keep any layout change here in sync'.
//! > Every design change is triple work and the three **will** drift apart."*
//!
//! The obvious fix is "one description, three renderers". It is not enough, and
//! v1 is the proof: v1 already had a shared bill object and drifted anyway,
//! because the *drawing* was written three times. Three renderers means three
//! traversals, and a traversal is a thing that can quietly forget a block.
//!
//! So there is exactly one function that walks a laid-out document —
//! [`render`] — and every renderer is a [`Sink`] it calls. A sink cannot
//! forget: it is handed everything, in order. If it chooses to ignore a QR code
//! that is a visible decision in its own file, not an omission nobody notices
//! for a year.
//!
//! # What P07 inherits
//!
//! The raster sink is P07's, and adding it should change **nothing outside its
//! own file** — that is the test of whether this shape is right. What it needs
//! to know:
//!
//! * The grid is columns. A column is `paper.dots_per_column()` dots wide at
//!   scale 1, and every thermal paper size divides evenly (there is a test).
//! * `LaidLine::indent` already includes the print offset (scope 7.11). Do not
//!   apply it again.
//! * Crown jewel 17 says the raster path is also the Kannada/Hindi path, so the
//!   font P07 picks is the font P23 will live with.
//! * **A receipt raster never goes over the wire** (scope 17.11). A
//!   576 × 1800 bitmap is about 130 KB; thirteen of them would be the whole
//!   10 MB monthly egress budget. The bitmap exists between the layout and the
//!   printer and nowhere else.

use crate::doc::{Align, Pattern, Style};
use crate::layout::{BandText, Laid, LaidContent, LaidLine};

/// Something that can receive a laid-out document.
///
/// Every method has a default that does nothing, so a sink that only cares
/// about text does not have to write empty bodies for images — but the call
/// still happens, which is the point.
///
/// # Every method is handed the whole line — P32
///
/// It used to be handed the payload only, so a sink that needed to know how
/// tall a row was had to work it out for itself: `raster` computed one height
/// for text, a different one for a blank, and a third for a rule, and the three
/// disagreed by three dots each. The layout decides that now
/// ([`LaidLine::row_dots`]) and every sink is told, which is the same argument
/// as `indent` — a sink that recomputes a position is a sink that can disagree
/// about it.
pub trait Sink {
    /// A line of text, already wrapped, aligned, sized and indented.
    fn line(&mut self, line: &LaidLine, index: usize);

    fn rule(&mut self, line: &LaidLine, pattern: Pattern, width: u32, index: usize) {
        let _ = (line, pattern, width, index);
    }

    fn image(&mut self, line: &LaidLine, data: &[u8], width_pct: u8, align: Align, index: usize) {
        let _ = (line, data, width_pct, align, index);
    }

    /// **P32 — a logo and the shop's name in one band of rows.**
    ///
    /// A sink that cannot draw a picture still has to draw the text, so the
    /// lines are handed over separately from the image rather than buried in
    /// it. `escpos`'s text engine takes exactly that path.
    fn band(&mut self, line: &LaidLine, image: &BandImage<'_>, lines: &[BandText], index: usize) {
        let _ = (line, image, lines, index);
    }

    fn qr(&mut self, line: &LaidLine, payload: &str, width_pct: u8, align: Align, index: usize) {
        let _ = (line, payload, width_pct, align, index);
    }

    /// P29. A sink that cannot draw bars prints the characters instead — the
    /// same choice the QR arm makes, and for the same reason: a number a
    /// person can read beats a blank space.
    fn barcode(
        &mut self,
        line: &LaidLine,
        payload: &str,
        human_readable: bool,
        align: Align,
        index: usize,
    ) {
        let _ = (line, payload, human_readable, align, index);
    }

    fn blank(&mut self, line: &LaidLine, index: usize) {
        let _ = (line, index);
    }

    fn finish(&mut self) {}
}

/// Where a band's picture goes, in dots. Every number already decided.
#[derive(Debug, Clone, Copy)]
pub struct BandImage<'a> {
    pub data: &'a [u8],
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

/// The one traversal.
pub fn render(laid: &Laid, sink: &mut dyn Sink) {
    for (index, line) in laid.lines.iter().enumerate() {
        match &line.content {
            LaidContent::Text { .. } => sink.line(line, index),
            LaidContent::Separator { pattern, width } => {
                sink.rule(line, *pattern, *width, index);
            }
            LaidContent::Image {
                data,
                width_pct,
                align,
            } => sink.image(line, data, *width_pct, *align, index),
            LaidContent::Band {
                image,
                image_left,
                image_top,
                image_width,
                image_height,
                lines,
            } => sink.band(
                line,
                &BandImage {
                    data: image,
                    left: *image_left,
                    top: *image_top,
                    width: *image_width,
                    height: *image_height,
                },
                lines,
                index,
            ),
            LaidContent::QrCode {
                payload,
                width_pct,
                align,
            } => sink.qr(line, payload, *width_pct, *align, index),
            LaidContent::Barcode {
                payload,
                human_readable,
                align,
            } => sink.barcode(line, payload, *human_readable, *align, index),
            LaidContent::Blank => sink.blank(line, index),
        }
    }
    sink.finish();
}

/// Everything a traversal did, in order.
///
/// Test-only in spirit, public because the anti-drift test lives in `tests/`.
/// This is what makes that test an equation rather than a hope: whatever the
/// recorder saw, every other sink must also have been handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Line {
        text: String,
        style: Style,
        indent: u32,
    },
    Rule {
        pattern: Pattern,
        width: u32,
    },
    Image {
        bytes: usize,
        width_pct: u8,
    },
    Band {
        bytes: usize,
        lines: Vec<String>,
    },
    Qr {
        payload: String,
        width_pct: u8,
    },
    Barcode {
        payload: String,
    },
    Blank,
}

#[derive(Debug, Clone, Default)]
pub struct Recorder {
    pub calls: Vec<Call>,
}

impl Recorder {
    #[must_use]
    pub fn new() -> Self {
        Recorder::default()
    }

    /// Every piece of text the document contains, in order.
    ///
    /// What the anti-drift test checks the other sinks against.
    #[must_use]
    pub fn texts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for call in &self.calls {
            match call {
                Call::Line { text, .. } => out.push(text.trim().to_owned()),
                Call::Qr { payload, .. } | Call::Barcode { payload } => out.push(payload.clone()),
                // A band's letterhead is text on the paper like any other, and
                // leaving it out here would let a sink drop the shop's name
                // without the anti-drift test noticing.
                Call::Band { lines, .. } => {
                    out.extend(lines.iter().map(|l| l.trim().to_owned()));
                }
                Call::Image { .. } | Call::Rule { .. } | Call::Blank => {}
            }
        }
        out.retain(|t| !t.is_empty());
        out
    }
}

impl Sink for Recorder {
    fn line(&mut self, line: &LaidLine, _index: usize) {
        if let LaidContent::Text { text } = &line.content {
            self.calls.push(Call::Line {
                text: text.clone(),
                style: line.style,
                indent: line.indent_dots,
            });
        }
    }

    fn rule(&mut self, _line: &LaidLine, pattern: Pattern, width: u32, _index: usize) {
        self.calls.push(Call::Rule { pattern, width });
    }

    fn image(
        &mut self,
        _line: &LaidLine,
        data: &[u8],
        width_pct: u8,
        _align: Align,
        _index: usize,
    ) {
        self.calls.push(Call::Image {
            bytes: data.len(),
            width_pct,
        });
    }

    fn band(
        &mut self,
        _line: &LaidLine,
        image: &BandImage<'_>,
        lines: &[BandText],
        _index: usize,
    ) {
        self.calls.push(Call::Band {
            bytes: image.data.len(),
            lines: lines.iter().map(|l| l.text.clone()).collect(),
        });
    }

    fn qr(
        &mut self,
        _line: &LaidLine,
        payload: &str,
        width_pct: u8,
        _align: Align,
        _index: usize,
    ) {
        self.calls.push(Call::Qr {
            payload: payload.to_owned(),
            width_pct,
        });
    }

    fn barcode(
        &mut self,
        _line: &LaidLine,
        payload: &str,
        _human_readable: bool,
        _align: Align,
        _index: usize,
    ) {
        self.calls.push(Call::Barcode {
            payload: payload.to_owned(),
        });
    }

    fn blank(&mut self, _line: &LaidLine, _index: usize) {
        self.calls.push(Call::Blank);
    }
}
