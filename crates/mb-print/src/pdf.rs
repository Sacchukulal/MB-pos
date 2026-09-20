//! The PDF sink.

// PDF's user space is points, and points are real numbers — there is no integer formulation of
// "10 pt Courier is 6 pt per character".
#![allow(
    clippy::float_arithmetic,
    reason = "page coordinates in points, not money — see the note above"
)]

use crate::doc::{Align, Pattern};
use crate::layout::{BandText, Laid, LaidContent, LaidLine};
use crate::render::{BandImage, Sink, render};

const PAGE_WIDTH: f64 = 595.0;
const PAGE_HEIGHT: f64 = 842.0;
const MARGIN: f64 = 36.0;
/// The body size when the paper's columns leave room for it. Courier is 0.6 em wide per
/// character, so a 10 pt Courier column is 6 pt.
const LARGEST_FONT: f64 = 10.0;
const EM_PER_CHAR: f64 = 0.6;
const PRINTABLE_WIDTH: f64 = PAGE_WIDTH - MARGIN - MARGIN;

/// Render a laid-out document as a PDF, across as many pages as it needs.
#[must_use]
pub fn to_pdf(laid: &Laid) -> Vec<u8> {
    // The layout wrote every line as many characters wide as the paper says, so the face is
    // sized to fit that many between the margins. The first version of this file fixed the
    // face at 10 pt instead, and A4's 96 columns ran 17 pt past the edge of the sheet: the
    // last three characters of every total were off the page.
    #[allow(
        clippy::cast_precision_loss,
        reason = "a column count: no paper is 2^53 characters wide"
    )]
    let columns = laid.paper.columns().max(1) as f64;
    let widest_char = (PRINTABLE_WIDTH / columns).min(LARGEST_FONT * EM_PER_CHAR);
    // The stream spells the size to one decimal, so the size is what the stream will say —
    // rounded down, or the columns are placed for 9.08 pt and drawn at 9.1.
    let font_size = (widest_char / EM_PER_CHAR * 10.0).floor() / 10.0;
    let char_width = font_size * EM_PER_CHAR;
    let mut sink = PdfSink {
        placed: Vec::new(),
        laid,
        cursor: 0.0,
        page: 0,
        char_width,
        font_size,
        line_height: font_size * 1.25,
    };
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
struct PdfSink<'a> {
    placed: Vec<Placed>,
    /// For `Laid::columns_of` — the layout counts an indent in dots, and this page counts in
    /// columns of a fixed-pitch face.
    laid: &'a Laid,
    /// Points down from the top margin of the current page.
    cursor: f64,
    page: usize,
    /// One column of the paper, in points, and the face that fills it.
    char_width: f64,
    font_size: f64,
    line_height: f64,
}

impl PdfSink<'_> {
    /// How much of a page a line may occupy — the foot is kept clear for the page number.
    fn body_height(&self) -> f64 {
        PAGE_HEIGHT - MARGIN - MARGIN - self.line_height * 2.0
    }

    /// Scale is honoured here, and the first version of this file did not honour it — a 2×
    /// heading came out the same size as the item lines while the text sink gave it twice the
    /// width.
    fn place(&mut self, indent: usize, text: &str, scale: u8) {
        let scale = f64::from(scale.clamp(1, 3));
        let size = self.font_size * scale;
        // The break happens BEFORE the line is placed, so a line is never cut in half by the
        // page edge.
        if self.cursor + self.line_height * scale > self.body_height() {
            self.page += 1;
            self.cursor = 0.0;
        }
        if !text.trim().is_empty() {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a column index on a page: no page is 2^53 characters wide"
            )]
            let x = MARGIN + (indent as f64) * self.char_width;
            self.placed.push(Placed {
                page: self.page,
                x,
                y: PAGE_HEIGHT - MARGIN - self.cursor - size,
                size,
                text: text.trim_end().to_owned(),
            });
        }
        self.cursor += self.line_height * scale;
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
        // The foot. Only when there is more than one page: a single-sheet receipt with "Page 1
        // of 1" on it looks like a form.
        if of > 1 {
            let label = format!("Page {} of {of}", page + 1);
            #[allow(
                clippy::cast_precision_loss,
                reason = "the length of \"Page 1 of 2\": a dozen characters"
            )]
            let x = (PAGE_WIDTH - (label.len() as f64) * self.char_width) / 2.0;
            content.push_str(&format!(
                "/F1 {:.1} Tf\n1 0 0 1 {x:.2} {MARGIN:.2} Tm\n({}) Tj\n",
                self.font_size,
                escape(&label)
            ));
        }
        content.push_str("ET\n");
        content
    }

    fn finish_document(&self) -> Vec<u8> {
        let pages = self.page + 1;
        // Object numbering, and it has to be laid out before anything is written: 1 catalog, 2
        // the page tree, 3 the font, then one page object and one content object per page,
        // interleaved so a page and its stream are next to each other in the file.
        const FONT: usize = 3;
        let page_object = |i: usize| FONT + 1 + i * 2;
        let content_object = |i: usize| FONT + 2 + i * 2;

        let kids: Vec<String> = (0..pages)
            .map(|i| format!("{} 0 R", page_object(i)))
            .collect();
        let mut objects: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!(
                "<< /Type /Pages /Kids [{}] /Count {pages} >>",
                kids.join(" ")
            ),
            // Courier is one of the base-14 fonts every reader carries, so nothing has to be
            // embedded.
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
            // WinAnsi covers Latin-1. Anything else — a rupee sign, Kannada — is dropped here
            // rather than written as mojibake, and that is one of the reasons this module gets
            // replaced when a font is embedded.
            c if (c as u32) < 256 => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

impl Sink for PdfSink<'_> {
    fn line(&mut self, line: &LaidLine, _index: usize) {
        if let LaidContent::Text { text } = &line.content {
            let indent = self.laid.columns_of(line.indent_dots);
            self.place(indent, text, line.style.scale());
        }
    }

    fn rule(&mut self, line: &LaidLine, pattern: Pattern, width: u32, _index: usize) {
        // A real line, like the raster sink draws.
        let indent = self.laid.columns_of(line.indent_dots);
        let across = self.laid.columns_of(width).max(1);
        let rule = crate::layout::Rule::of(pattern);
        for _ in 0..rule.strokes {
            self.place(indent, &"_".repeat(across), 1);
        }
    }

    fn image(
        &mut self,
        _line: &LaidLine,
        _data: &[u8],
        _width_pct: u8,
        _align: Align,
        _index: usize,
    ) {
        // An image needs an XObject and a compressed stream, which is where the no-crate
        // calculation stops working.
    }

    fn band(
        &mut self,
        _line: &LaidLine,
        _image: &BandImage<'_>,
        lines: &[BandText],
        _index: usize,
    ) {
        // The picture cannot be drawn here either; the letterhead still must be.
        for text in lines {
            self.place(0, text.text.trim(), text.style.scale());
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
        self.place(0, payload, 1);
    }

    fn barcode(
        &mut self,
        _line: &LaidLine,
        payload: &str,
        _human_readable: bool,
        _align: Align,
        _index: usize,
    ) {
        self.place(0, payload, 1);
    }

    fn blank(&mut self, _line: &LaidLine, _index: usize) {
        self.cursor += self.line_height;
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

    /// Every column the paper promises lands between the margins. The first version fixed the
    /// face at 10 pt, and A4's 96 columns ran off the right edge of the sheet: "Total" read
    /// "Tot" and every figure under it lost its paise.
    #[test]
    fn every_column_of_the_paper_is_on_the_page() {
        for kind in [PaperKind::A4, PaperKind::Mm80] {
            let paper = Paper::new(kind);
            let mut doc = Document::new(paper);
            doc.separator(Pattern::Double);
            let laid = layout(&doc).expect("lays out");
            let pdf = to_pdf(&laid);
            let text = String::from_utf8_lossy(&pdf);
            let size: f64 = text
                .lines()
                .find_map(|l| l.strip_prefix("/F1 ")?.strip_suffix(" Tf")?.parse().ok())
                .expect("a font size");
            #[allow(clippy::cast_precision_loss, reason = "a column count")]
            let right_edge = MARGIN + size * EM_PER_CHAR * paper.columns() as f64;
            assert!(
                right_edge <= PAGE_WIDTH - MARGIN + 0.01,
                "{kind:?}: {} columns at {size} pt reach {right_edge} pt",
                paper.columns()
            );
            assert!(
                size <= LARGEST_FONT,
                "{kind:?}: {size} pt is bigger than the body"
            );
        }
    }

    #[test]
    fn a_bigger_heading_really_is_bigger() {
        // The first version of this sink ignored `scale` entirely: a 2x heading came out the
        // same size as the item lines while the text sink gave it twice the width.
        // 80 mm is 48 columns, which a 10 pt face fits with room to spare, so the body is
        // the full 10 pt here; A4's 96 columns are fitted a little smaller.
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.text("BIG", Style::new(2, true), Align::Left)
            .text("small", Style::NORMAL, Align::Left);
        let pdf = to_pdf(&layout(&doc).expect("lays out"));
        let text = String::from_utf8_lossy(&pdf);

        assert!(text.contains("/F1 20.0 Tf"), "the 2x heading is not 20pt");
        assert!(text.contains("/F1 10.0 Tf"), "the normal line is not 10pt");
    }

    /// A month of item sales is four hundred rows.
    #[test]
    fn a_long_report_gets_more_pages_rather_than_a_shorter_report() {
        let mut doc = Document::new(Paper::new(PaperKind::A4));
        for n in 0..400 {
            doc.row(format!("Item number {n}"), "240.00", Style::NORMAL);
        }
        let pdf = to_pdf(&layout(&doc).expect("lays out"));
        let text = String::from_utf8_lossy(&pdf);

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
        // Every object the xref promises must actually be there, or a reader rejects the file.
        assert!(text.contains(&format!("xref\n0 {}\n", 3 + 7 * 2 + 1)));
    }

    /// A receipt is one page and must not grow a page number.
    #[test]
    fn a_short_document_is_still_one_clean_page() {
        let mut doc = Document::new(Paper::new(PaperKind::A4));
        doc.line("Masala Dosa");
        let text = String::from_utf8_lossy(&to_pdf(&layout(&doc).expect("lays out"))).into_owned();
        assert!(text.contains("/Count 1"));
        assert!(
            !text.contains("Page 1 of"),
            "a one-page slip got a page number"
        );
    }

    #[test]
    fn parentheses_in_a_shop_name_do_not_break_the_file() {
        // "Anna Kuteera (Jayanagar)" is an entirely ordinary shop name, and an unescaped
        // bracket ends the PDF string early and corrupts the page.
        let mut doc = Document::new(Paper::new(PaperKind::A4));
        doc.line("Anna Kuteera (Jayanagar) \\ Branch");
        let pdf = to_pdf(&layout(&doc).expect("lays out"));
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("Anna Kuteera \\(Jayanagar\\) \\\\ Branch"));
    }
}
