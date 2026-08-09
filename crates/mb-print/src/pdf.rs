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

/// Render a laid-out document as a PDF, **across as many pages as it needs.**
///
/// Until P18 this dropped everything past the first page, with a note saying
/// the day reports arrived was the day pagination had to be real. That day is
/// this one: a month of item sales is four hundred rows, and a report that
/// silently stops at row sixty-two is worse than no export at all — nothing on
/// the page says it is incomplete.
///
/// Every page carries "Page n of m" at the foot, so a printed stack that gets
/// dropped can be put back in order.
#[must_use]
pub fn to_pdf(laid: &Laid) -> Vec<u8> {
    let mut sink = PdfSink::default();
    render(laid, &mut sink);
    sink.finish_document()
}

/// One placed line: which page, where it starts, how big it is, what it says.
#[derive(Debug)]
struct Placed {
    page: usize,
    x: f64,
    y: f64,
    size: f64,
    text: String,
}

#[derive(Debug)]
struct PdfSink {
    placed: Vec<Placed>,
    /// Points down from the top margin **of the current page**. A cursor rather
    /// than a row index, because a 2× line is twice as tall and a row index
    /// cannot say that.
    cursor: f64,
    page: usize,
}

impl Default for PdfSink {
    fn default() -> Self {
        PdfSink {
            placed: Vec::new(),
            cursor: 0.0,
            page: 0,
        }
    }
}

/// How much of a page a line may occupy — the foot is kept clear for the page
/// number.
const BODY_HEIGHT: f64 = PAGE_HEIGHT - MARGIN - MARGIN - LINE_HEIGHT * 2.0;

impl PdfSink {
    /// **Scale is honoured here**, and the first version of this file did not
    /// honour it — a 2× heading came out the same size as the item lines while
    /// the text sink gave it twice the width. That is exactly the drift this
    /// crate exists to prevent, and it survived long enough to be caught by a
    /// test rather than by review, which is the argument for the test.
    fn place(&mut self, indent: usize, text: &str, scale: u8) {
        let scale = f64::from(scale.clamp(1, 3));
        let size = FONT_SIZE * scale;
        // The break happens BEFORE the line is placed, so a line is never cut in
        // half by the page edge.
        if self.cursor + LINE_HEIGHT * scale > BODY_HEIGHT {
            self.page += 1;
            self.cursor = 0.0;
        }
        if !text.trim().is_empty() {
            #[allow(clippy::cast_precision_loss)]
            let x = MARGIN + (indent as f64) * CHAR_WIDTH;
            self.placed.push(Placed {
                page: self.page,
                x,
                y: PAGE_HEIGHT - MARGIN - self.cursor - size,
                size,
                text: text.trim_end().to_owned(),
            });
        }
        self.cursor += LINE_HEIGHT * scale;
    }

    /// One page's content stream.
    fn stream_for(&self, page: usize, of: usize) -> String {
        let mut content = String::from("BT\n");
        let mut current = 0.0_f64;
        for line in self.placed.iter().filter(|l| l.page == page) {
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
        // The foot. Only when there is more than one page: a single-sheet
        // receipt with "Page 1 of 1" on it looks like a form.
        if of > 1 {
            let label = format!("Page {} of {of}", page + 1);
            #[allow(clippy::cast_precision_loss)]
            let x = (PAGE_WIDTH - (label.len() as f64) * CHAR_WIDTH) / 2.0;
            content.push_str(&format!(
                "/F1 {FONT_SIZE:.1} Tf\n1 0 0 1 {x:.2} {MARGIN:.2} Tm\n({}) Tj\n",
                escape(&label)
            ));
        }
        content.push_str("ET\n");
        content
    }

    fn finish_document(&self) -> Vec<u8> {
        let pages = self.page + 1;
        // Object numbering, and it has to be laid out before anything is
        // written: 1 catalog, 2 the page tree, 3 the font, then one page object
        // and one content object per page, interleaved so a page and its stream
        // are next to each other in the file.
        const FONT: usize = 3;
        let page_object = |i: usize| FONT + 1 + i * 2;
        let content_object = |i: usize| FONT + 2 + i * 2;

        let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", page_object(i))).collect();
        let mut objects: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!(
                "<< /Type /Pages /Kids [{}] /Count {pages} >>",
                kids.join(" ")
            ),
            // Courier is one of the base-14 fonts every reader carries, so
            // nothing has to be embedded.
            "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>"
                .to_owned(),
        ];
        for i in 0..pages {
            let stream = self.stream_for(i, pages);
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] \
                 /Resources << /Font << /F1 {FONT} 0 R >> >> /Contents {} 0 R >>",
                content_object(i)
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{stream}endstream",
                stream.len()
            ));
        }

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

    /// **P18.** A month of item sales is four hundred rows. Before this, row
    /// sixty-three onwards was silently dropped and nothing on the page said so.
    #[test]
    fn a_long_report_gets_more_pages_rather_than_a_shorter_report() {
        let mut doc = Document::new(Paper::new(PaperKind::A4));
        for n in 0..400 {
            doc.row(format!("Item number {n}"), "240.00", Style::NORMAL);
        }
        let pdf = to_pdf(&layout(&doc).expect("lays out"));
        let text = String::from_utf8_lossy(&pdf);

        // Roughly 62 lines fit on A4 at 10pt, so 400 rows is seven pages.
        assert!(text.contains("/Count 7"), "the page count is wrong");
        assert_eq!(text.matches("/Type /Page\n").count(), 0);
        assert_eq!(
            text.matches("/Type /Page ").count(),
            7,
            "there should be seven page objects"
        );
        // And the LAST row is really in the file — the whole point.
        assert!(text.contains("Item number 399"), "the last row was dropped");
        // Each sheet says where it belongs in the stack.
        assert!(text.contains("Page 1 of 7"));
        assert!(text.contains("Page 7 of 7"));
        // Every object the xref promises must actually be there, or a reader
        // rejects the file. 3 fixed + 2 per page.
        assert!(text.contains(&format!("xref\n0 {}\n", 3 + 7 * 2 + 1)));
    }

    /// A receipt is one page and must not grow a page number.
    #[test]
    fn a_short_document_is_still_one_clean_page() {
        let mut doc = Document::new(Paper::new(PaperKind::A4));
        doc.line("Masala Dosa");
        let text = String::from_utf8_lossy(&to_pdf(&layout(&doc).expect("lays out"))).into_owned();
        assert!(text.contains("/Count 1"));
        assert!(!text.contains("Page 1 of"), "a one-page slip got a page number");
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
