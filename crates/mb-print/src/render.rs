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
use crate::layout::{Laid, LaidContent, LaidLine};

/// Something that can receive a laid-out document.
///
/// Every method has a default that does nothing, so a sink that only cares
/// about text does not have to write empty bodies for images — but the call
/// still happens, which is the point.
pub trait Sink {
    /// A line of text, already wrapped, aligned, scaled and indented.
    fn line(&mut self, line: &LaidLine, index: usize);

    fn rule(&mut self, pattern: Pattern, width: usize, indent: usize, index: usize) {
        let _ = (pattern, width, indent, index);
    }

    fn image(&mut self, data: &[u8], width_pct: u8, align: Align, index: usize) {
        let _ = (data, width_pct, align, index);
    }

    fn qr(&mut self, payload: &str, width_pct: u8, align: Align, index: usize) {
        let _ = (payload, width_pct, align, index);
    }

    fn blank(&mut self, index: usize) {
        let _ = index;
    }

    fn finish(&mut self) {}
}

/// The one traversal.
pub fn render(laid: &Laid, sink: &mut dyn Sink) {
    for (index, line) in laid.lines.iter().enumerate() {
        match &line.content {
            LaidContent::Text { .. } => sink.line(line, index),
            LaidContent::Separator { pattern, width } => {
                sink.rule(*pattern, *width, line.indent, index);
            }
            LaidContent::Image {
                data,
                width_pct,
                align,
            } => sink.image(data, *width_pct, *align, index),
            LaidContent::QrCode {
                payload,
                width_pct,
                align,
            } => sink.qr(payload, *width_pct, *align, index),
            LaidContent::Blank => sink.blank(index),
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
        indent: usize,
    },
    Rule {
        pattern: Pattern,
        width: usize,
    },
    Image {
        bytes: usize,
        width_pct: u8,
    },
    Qr {
        payload: String,
        width_pct: u8,
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
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::Line { text, .. } => Some(text.trim().to_owned()),
                Call::Qr { payload, .. } => Some(payload.clone()),
                _ => None,
            })
            .filter(|t| !t.is_empty())
            .collect()
    }
}

impl Sink for Recorder {
    fn line(&mut self, line: &LaidLine, _index: usize) {
        if let LaidContent::Text { text } = &line.content {
            self.calls.push(Call::Line {
                text: text.clone(),
                style: line.style,
                indent: line.indent,
            });
        }
    }

    fn rule(&mut self, pattern: Pattern, width: usize, _indent: usize, _index: usize) {
        self.calls.push(Call::Rule { pattern, width });
    }

    fn image(&mut self, data: &[u8], width_pct: u8, _align: Align, _index: usize) {
        self.calls.push(Call::Image {
            bytes: data.len(),
            width_pct,
        });
    }

    fn qr(&mut self, payload: &str, width_pct: u8, _align: Align, _index: usize) {
        self.calls.push(Call::Qr {
            payload: payload.to_owned(),
            width_pct,
        });
    }

    fn blank(&mut self, _index: usize) {
        self.calls.push(Call::Blank);
    }
}
