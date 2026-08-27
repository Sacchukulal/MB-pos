//! One traversal. Renderers are sinks.

use crate::doc::{Align, Pattern, Style};
use crate::layout::{BandText, Laid, LaidContent, LaidLine};

/// Something that can receive a laid-out document.
pub trait Sink {
    /// A line of text, already wrapped, aligned, sized and indented.
    fn line(&mut self, line: &LaidLine, index: usize);

    fn rule(&mut self, line: &LaidLine, pattern: Pattern, width: u32, index: usize) {
        let _ = (line, pattern, width, index);
    }

    fn image(&mut self, line: &LaidLine, data: &[u8], width_pct: u8, align: Align, index: usize) {
        let _ = (line, data, width_pct, align, index);
    }

    /// A logo and the shop's name in one band of rows.
    fn band(&mut self, line: &LaidLine, image: &BandImage<'_>, lines: &[BandText], index: usize) {
        let _ = (line, image, lines, index);
    }

    fn qr(&mut self, line: &LaidLine, payload: &str, width_pct: u8, align: Align, index: usize) {
        let _ = (line, payload, width_pct, align, index);
    }

    /// A sink that cannot draw bars prints the characters instead — the same choice the QR arm
    /// makes, and for the same reason: a number a person can read beats a blank space.
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

/// Where a band's picture goes, in dots.
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
    #[must_use]
    pub fn texts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for call in &self.calls {
            match call {
                Call::Line { text, .. } => out.push(text.trim().to_owned()),
                Call::Qr { payload, .. } | Call::Barcode { payload } => out.push(payload.clone()),
                // A band's letterhead is text on the paper like any other, and leaving it out
                // here would let a sink drop the shop's name without the anti-drift test
                // noticing.
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

    fn band(&mut self, _line: &LaidLine, image: &BandImage<'_>, lines: &[BandText], _index: usize) {
        self.calls.push(Call::Band {
            bytes: image.data.len(),
            lines: lines.iter().map(|l| l.text.clone()).collect(),
        });
    }

    fn qr(&mut self, _line: &LaidLine, payload: &str, width_pct: u8, _align: Align, _index: usize) {
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
