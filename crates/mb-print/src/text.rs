//! The plain-text sink — the printer's own font path.
//!
//! Fast, and the only path that works on a printer with no raster support at
//! all. Scale is not expressible in plain text, so a 2× line simply occupies
//! twice the columns per character — which the layout has already accounted
//! for, so this sink does no arithmetic of its own.
//!
//! P07 turns these lines into ESC/POS with the size multiplier attached; here
//! they are lines.

use crate::doc::{Align, Pattern};
use crate::layout::{Laid, LaidContent, LaidLine};
use crate::render::{Sink, render};

/// Render a laid-out document as plain lines.
#[must_use]
pub fn to_text(laid: &Laid) -> String {
    let mut sink = TextSink {
        out: String::new(),
        columns: laid.paper.columns(),
    };
    render(laid, &mut sink);
    sink.out
}

#[derive(Debug)]
struct TextSink {
    out: String,
    columns: usize,
}

impl TextSink {
    fn push(&mut self, indent: usize, body: &str) {
        for _ in 0..indent {
            self.out.push(' ');
        }
        self.out.push_str(body.trim_end());
        self.out.push('\n');
    }

    /// For text this sink produces itself rather than receiving from the
    /// layout — currently only the QR fallback.
    ///
    /// It has to wrap, because a 54-character UPI URI on 32-column paper is an
    /// overflow, and an overflow is the one thing R3 forbids. The golden file
    /// is what caught it: the layout was doing its job and this sink was
    /// quietly writing past the edge.
    fn push_wrapped(&mut self, body: &str) {
        let width = self.columns.max(1);
        let chars: Vec<char> = body.chars().collect();
        for chunk in chars.chunks(width) {
            let line: String = chunk.iter().collect();
            self.push(0, &line);
        }
    }
}

impl Sink for TextSink {
    fn line(&mut self, line: &LaidLine, _index: usize) {
        if let LaidContent::Text { text } = &line.content {
            self.push(line.indent, text);
        }
    }

    fn rule(&mut self, pattern: Pattern, width: usize, indent: usize, _index: usize) {
        let body: String = std::iter::repeat_n(pattern.glyph(), width).collect();
        self.push(indent, &body);
    }

    fn image(&mut self, _data: &[u8], _width_pct: u8, _align: Align, _index: usize) {
        // A text printer cannot draw a logo. Saying nothing is the right
        // output, and this empty body is the visible decision the `Sink` trait
        // exists to force — the alternative is a renderer that quietly skips a
        // block, which is audit D1.
    }

    fn qr(&mut self, payload: &str, _width_pct: u8, _align: Align, _index: usize) {
        // The payload is printed as text. A UPI URI a customer can read and
        // type is worth more than a blank space, and P07's raster path prints
        // the real square.
        self.push_wrapped(payload);
    }

    fn blank(&mut self, _index: usize) {
        self.out.push('\n');
    }
}
