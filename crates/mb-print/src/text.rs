//! The plain-text sink — the printer's own font path.
//!
//! Fast, and the only path that works on a printer with no raster support at
//! all. Size is not expressible in plain text, so a 2× line simply occupies
//! twice the columns per character — which the layout has already accounted
//! for, so this sink does no arithmetic of its own.
//!
//! P07 turns these lines into ESC/POS with the size multiplier attached; here
//! they are lines.
//!
//! # Columns, from dots — P32
//!
//! The layout speaks in **dots** now, because a character's width is measured
//! rather than assumed. This sink prints with the printer's own fixed font, so
//! it divides back by the base advance — which is exact, because
//! [`crate::metrics::Metrics::printer_font`] only ever answers in whole
//! multiples of it.

use crate::doc::{Align, Pattern};
use crate::layout::{BandText, Laid, LaidContent, LaidLine};
use crate::render::{BandImage, Sink, render};

/// Render a laid-out document as plain lines.
#[must_use]
pub fn to_text(laid: &Laid) -> String {
    let advance = laid.base_advance.max(1);
    let mut sink = TextSink {
        out: String::new(),
        columns: 0,
        advance,
    };
    sink.columns = sink.columns_of(laid.paper.kind.dots().unwrap_or(advance));
    render(laid, &mut sink);
    sink.out
}

#[derive(Debug)]
struct TextSink {
    out: String,
    columns: usize,
    advance: u32,
}

impl TextSink {
    /// An indent given in dots, as the spaces a fixed-pitch printer pads with.
    #[expect(
        clippy::integer_division,
        reason = "dots back into whole characters — the printer font's advance divides exactly"
    )]
    const fn columns_of(&self, dots: u32) -> usize {
        (dots / self.advance) as usize
    }

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
            let indent = self.columns_of(line.indent_dots);
            self.push(indent, text);
        }
    }

    fn rule(&mut self, line: &LaidLine, pattern: Pattern, width: u32, _index: usize) {
        // **The one sink that still draws a rule out of characters**, because
        // it is the one sink that cannot draw a dot. The other three draw a
        // real line (P32); this prints the pattern's glyph across the same
        // width, which is the closest the printer's own font can come.
        let indent = self.columns_of(line.indent_dots);
        let across = self.columns_of(width).max(1);
        let body: String = std::iter::repeat_n(pattern.glyph(), across).collect();
        self.push(indent, &body);
    }

    fn image(
        &mut self,
        _line: &LaidLine,
        _data: &[u8],
        _width_pct: u8,
        _align: Align,
        _index: usize,
    ) {
        // A text printer cannot draw a logo. Saying nothing is the right
        // output, and this empty body is the visible decision the `Sink` trait
        // exists to force — the alternative is a renderer that quietly skips a
        // block, which is audit D1.
    }

    fn band(
        &mut self,
        _line: &LaidLine,
        _image: &BandImage<'_>,
        lines: &[BandText],
        _index: usize,
    ) {
        // The picture cannot be drawn; **the shop's name still must be.**
        // Dropping the whole band because half of it is a logo would lose the
        // letterhead on every text-engine printer, which is exactly the kind of
        // silent omission the `Sink` trait exists to make impossible.
        for text in lines {
            self.push(0, text.text.trim());
        }
    }

    fn qr(
        &mut self,
        _line: &LaidLine,
        payload: &str,
        _width_pct: u8,
        _align: Align,
        _index: usize,
    ) {
        // The payload is printed as text. A UPI URI a customer can read and
        // type is worth more than a blank space, and P07's raster path prints
        // the real square.
        self.push_wrapped(payload);
    }

    fn barcode(
        &mut self,
        _line: &LaidLine,
        payload: &str,
        _human_readable: bool,
        _align: Align,
        _index: usize,
    ) {
        // Same argument as the QR: the characters a person can read beat a gap.
        self.push_wrapped(payload);
    }

    fn blank(&mut self, _line: &LaidLine, _index: usize) {
        self.out.push('\n');
    }
}
