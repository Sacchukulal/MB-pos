//! The PDF sink — scope 7.10, the B2B A4 invoice, and the file P18 will export
//! reports through.
//!
//! # Why there is no PDF crate (R6)
//!
//! A text-only PDF 1.4 over one of the base-14 fonts is a header, five objects,
//! a content stream of `Td`/`Tj`, and an xref table. That is what this file is,
//! and it opens in every reader that exists. A crate would be several hundred
//! kilobytes and a build-time cost for something a shop uses when a B2B
//! customer asks for an invoice.
//!
//! **The calculation changes the moment an embedded font or an image is
//! needed** — a logo on the invoice, or Kannada text — because both mean font
//! embedding and image compression, and neither is 150 lines. When that day
//! comes, a crate is the right answer and this module is the thing it replaces.
//! Say so here rather than discovering it as a surprise.
//!
//! # And the honest limitation
//!
//! Courier on a 96-column grid is **correct, filable and ugly**. A proper tax
//! invoice wants a proportional face and real table rules. This is a stated
//! compromise, not a design — see [`crate::paper::PaperKind::A4`].

// PDF's user space is points, and points are real numbers — there is no
// integer formulation of "10 pt Courier is 6 pt per character". D7's ban on
// floating point is about the MONEY path, and no money is computed here: an
// amount arrives as a string that `Money::to_plain_string` already produced,
// and this file only decides where to put it. `t9` asserts that every number
// which reaches paper round-trips through `Money::parse`, so if anything ever
// did arithmetic on an amount, that test is where it would surface.
#![allow(
    clippy::float_arithmetic,
    reason = "page coordinates in points, not money — see the note above"
)]

use crate::doc::{Align, Pattern};
use crate::layout::{Laid, LaidContent, LaidLine};
use crate::render::{Sink, render};

/// A4 at 72 dpi, which is what PDF's default user space is.
const PAGE_WIDTH: f64 = 595.0;
const PAGE_HEIGHT: f64 = 842.0;
const MARGIN: f64 = 36.0;
/// Courier is 0.6 em wide per character, so a 10 pt Courier column is 6 pt.
const FONT_SIZE: f64 = 10.0;
const CHAR_WIDTH: f64 = FONT_SIZE * 0.6;
const LINE_HEIGHT: f64 = FONT_SIZE * 1.25;

/// Render a laid-out document as a single-page PDF.
///
/// Content past one page is dropped at the bottom rather than paginated — a
/// receipt is one page by nature, and an invoice long enough to need two is a
/// real requirement that belongs with P18's report export, where multi-page
/// tables have to be solved anyway. Left as a named limitation rather than a
/// half-done pagination.
#[must_use]
pub fn to_pdf(laid: &Laid) -> Vec<u8> {
    let mut sink = PdfSink::default();
    render(laid, &mut sink);
    sink.finish_document()
}

/// One placed line: where it starts, how big it is, and what it says.
#[derive(Debug)]
struct Placed {
    x: f64,
    y: f64,
    size: f64,
    text: String,
}

#[derive(Debug)]
struct PdfSink {
    placed: Vec<Placed>,
    /// Points down from the top margin. A cursor rather than a row index,
    /// because a 2× line is twice as tall and a row index cannot say that.
    cursor: f64,
}

impl Default for PdfSink {
    fn default() -> Self {
        PdfSink {
            placed: Vec::new(),
            cursor: 0.0,
        }
    }
}

impl PdfSink {
    /// **Scale is honoured here**, and the first version of this file did not
    /// honour it — a 2× heading came out the same size as the item lines while
    /// the text sink gave it twice the width. That is exactly the drift this
    /// crate exists to prevent, and it survived long enough to be caught by a
    /// test rather than by review, which is the argument for the test.
    fn place(&mut self, indent: usize, text: &str, scale: u8) {
        let scale = f64::from(scale.clamp(1, 3));
        let size = FONT_SIZE * scale;
        if !text.trim().is_empty() {
            #[allow(clippy::cast_precision_loss)]
            let x = MARGIN + (indent as f64) * CHAR_WIDTH;
            self.placed.push(Placed {
                x,
                y: PAGE_HEIGHT - MARGIN - self.cursor - size,
                size,
                text: text.trim_end().to_owned(),
            });
        }
        self.cursor += LINE_HEIGHT * scale;
    }

    fn finish_document(&self) -> Vec<u8> {
        let mut content = String::from("BT\n");
        let mut current = 0.0_f64;
        for line in &self.placed {
            if line.y < MARGIN {
                break;
            }
            if (line.size - current).abs() > f64::EPSILON {
                content.push_str(&format!("/F1 {:.1} Tf\n", line.size));
                current = line.size;
            }
            content.push_str(&format!(
                "1 0 0 1 {:.2} {:.2} Tm\n({}) Tj\n",
                line.x,
                line.y,
                escape(&line.text)
            ));
        }
        content.push_str("ET\n");

        let mut objects: Vec<String> = Vec::new();
        objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_owned());
        objects.push("<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned());
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] \
             /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
        ));
        // Courier is one of the base-14 fonts every reader carries, so nothing
        // has to be embedded.
        objects.push(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>"
                .to_owned(),
        );
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ));

        let mut out = String::from("%PDF-1.4\n");
        let mut offsets = Vec::with_capacity(objects.len());
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
        }

        let xref_at = out.len();
        out.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
        out.push_str("0000000000 65535 f \n");
        for offset in &offsets {
            out.push_str(&format!("{offset:010} 00000 n \n"));
        }
        out.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        ));

        out.into_bytes()
    }
}

/// PDF string escaping: backslash, and both parentheses.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            // WinAnsi covers Latin-1. Anything else — a rupee sign, Kannada —
            // is dropped here rather than written as mojibake, and that is one
            // of the reasons this module gets replaced when a font is embedded.
            c if (c as u32) < 256 => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

impl Sink for PdfSink {
    fn line(&mut self, line: &LaidLine, _index: usize) {
        if let LaidContent::Text { text } = &line.content {
            self.place(line.indent, text, line.style.scale());
        }
    }

    fn rule(&mut self, pattern: Pattern, width: usize, indent: usize, _index: usize) {
        let body: String = std::iter::repeat_n(pattern.glyph(), width).collect();
        self.place(indent, &body, 1);
    }

    fn image(&mut self, _data: &[u8], _width_pct: u8, _align: Align, _index: usize) {
        // An image needs an XObject and a compressed stream, which is where the
        // no-crate calculation stops working. Deliberately nothing, and the
        // module doc says why.
    }

    fn qr(&mut self, payload: &str, _width_pct: u8, _align: Align, _index: usize) {
        self.place(0, payload, 1);
    }

    fn blank(&mut self, _index: usize) {
        self.cursor += LINE_HEIGHT;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Document, Style};
    use crate::layout::layout;
    use crate::paper::{Paper, PaperKind};

    #[test]
    fn it_produces_something_a_reader_would_accept() {
        let mut doc = Document::new(Paper::new(PaperKind::A4));
        doc.text("TAX INVOICE", Style::new(2, true), Align::Centre)
            .separator(Pattern::Double)
            .row("Masala Dosa", "240.00", Style::NORMAL);
        let pdf = to_pdf(&layout(&doc).expect("lays out"));

        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"), "no PDF header");
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("/BaseFont /Courier"));
        assert!(text.trim_end().ends_with("%%EOF"), "no EOF marker");
        assert!(text.contains("startxref"));
        // Centring is padding, so the heading arrives with leading spaces.
        assert!(text.contains("TAX INVOICE)"), "the heading is missing");
        assert!(text.contains("240.00"), "the amount is missing");
    }

    #[test]
    fn a_bigger_heading_really_is_bigger() {
        // The first version of this sink ignored `scale` entirely: a 2x heading
        // came out the same size as the item lines while the text sink gave it
        // twice the width. That is the drift this crate exists to prevent, and
        // it survived until a test looked.
        let mut doc = Document::new(Paper::new(PaperKind::A4));
        doc.text("BIG", Style::new(2, true), Align::Left)
            .text("small", Style::NORMAL, Align::Left);
        let pdf = to_pdf(&layout(&doc).expect("lays out"));
        let text = String::from_utf8_lossy(&pdf);

        assert!(text.contains("/F1 20.0 Tf"), "the 2x heading is not 20pt");
        assert!(text.contains("/F1 10.0 Tf"), "the normal line is not 10pt");
    }

    #[test]
    fn parentheses_in_a_shop_name_do_not_break_the_file() {
        // "Anna Kuteera (Jayanagar)" is an entirely ordinary shop name, and an
        // unescaped bracket ends the PDF string early and corrupts the page.
        let mut doc = Document::new(Paper::new(PaperKind::A4));
        doc.line("Anna Kuteera (Jayanagar) \\ Branch");
        let pdf = to_pdf(&layout(&doc).expect("lays out"));
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("Anna Kuteera \\(Jayanagar\\) \\\\ Branch"));
    }
}
