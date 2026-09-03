//! The plain-text sink — the printer's own font path.

use crate::doc::{Align, Pattern};
use crate::layout::{BandText, Laid, LaidContent, LaidLine};
use crate::render::{BandImage, Sink, render};

/// Render a laid-out document as plain lines.
#[must_use]
pub fn to_text(laid: &Laid) -> String {
    let mut sink = TextSink {
        out: String::new(),
        columns: laid.columns_of(laid.paper.kind.dots().unwrap_or(laid.base_advance)),
        laid,
    };
    render(laid, &mut sink);
    sink.out
}

#[derive(Debug)]
struct TextSink<'a> {
    out: String,
    columns: usize,
    /// For `Laid::columns_of` — an indent given in dots, as the spaces a fixed-pitch printer
    /// pads with.
    laid: &'a Laid,
}

impl TextSink<'_> {
    fn push(&mut self, indent: usize, body: &str) {
        for _ in 0..indent {
            self.out.push(' ');
        }
        self.out.push_str(body.trim_end());
        self.out.push('\n');
    }

    /// For text this sink produces itself rather than receiving from the layout — currently
    /// only the QR fallback.
    fn push_wrapped(&mut self, body: &str) {
        let width = self.columns.max(1);
        let chars: Vec<char> = body.chars().collect();
        for chunk in chars.chunks(width) {
            let line: String = chunk.iter().collect();
            self.push(0, &line);
        }
    }
}

impl Sink for TextSink<'_> {
    fn line(&mut self, line: &LaidLine, _index: usize) {
        if let LaidContent::Text { text } = &line.content {
            let indent = self.laid.columns_of(line.indent_dots);
            self.push(indent, text);
        }
    }

    fn rule(&mut self, line: &LaidLine, pattern: Pattern, width: u32, _index: usize) {
        // The one sink that still draws a rule out of characters, because it is the one sink
        // that cannot draw a dot.
        let indent = self.laid.columns_of(line.indent_dots);
        let across = self.laid.columns_of(width).max(1);
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
        // A text printer cannot draw a logo.
    }

    fn band(
        &mut self,
        _line: &LaidLine,
        _image: &BandImage<'_>,
        lines: &[BandText],
        _index: usize,
    ) {
        // The picture cannot be drawn; the shop's name still must be.
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
        // The payload is printed as text.
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
