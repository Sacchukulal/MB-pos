//! The test print — **and the print offset made usable by a shopkeeper.**
//!
//! v1's test print was a tiny slip that proved the printer was plugged in. This
//! one is a real-looking bill with sample data, so the owner can check
//! alignment, paper width, the font, the cut and the offset **before a customer
//! is standing there**.
//!
//! # It works when nothing else does
//!
//! That is the point of it, and it is why nothing here reads a database:
//!
//! * **first run.** The counter has just been installed and there is no shop
//!   yet;
//! * **after a restore.** D27 says a restore runs *before* `Db::open`, so at the
//!   moment somebody most wants to know whether printing works, the database is
//!   deliberately not open;
//! * **with no printer configured at all**, against the file transport, so
//!   "printing is broken" can be diagnosed without a printer.
//!
//! # Scope 7.11 — read the correction off the paper
//!
//! A ruler runs across the slip with every fifth column marked and every tenth
//! numbered, and the current offset is printed in both millimetres and columns.
//! Print, look at the paper, [`crate::printer::nudge`], print again. Guessing at
//! a number in a settings screen is how somebody spends an evening on a two
//! millimetre problem.

use mb_core::Money;

use crate::doc::{Align, Block, Document, Pattern, Style};
use crate::layout::{Note, layout};
use crate::printer::{Engine, PrinterConfig, Target};
use crate::template::Store;

/// A test slip for this printer, on its own paper, at its own offset.
///
/// `store` is optional: on a first run there is no shop profile yet, and the
/// slip still has to print.
#[must_use]
pub fn test_document(printer: &PrinterConfig, store: Option<&Store>) -> Document {
    let mut doc = Document::new(printer.paper);
    let columns = printer.paper.columns();

    doc.text(
        store.map_or("MAGIC BILL", |s| s.name.as_str()),
        Style::new(2, true),
        Align::Centre,
    );
    doc.text("TEST PRINT", Style::BOLD, Align::Centre);
    doc.separator(Pattern::Double);

    // Plain lines and not `Row`s, deliberately: D30 says the right-hand side of
    // a row is never shortened or wrapped, because it is an amount — so a row
    // whose right side is a sentence is an error waiting for the narrowest
    // paper. Labels that wrap are exactly what these want.
    doc.text(format!("Printer: {}", printer.name), Style::NORMAL, Align::Left);
    doc.text(
        format!("Connection: {}", describe(&printer.target)),
        Style::NORMAL,
        Align::Left,
    );
    doc.text(format!("Paper: {columns} columns"), Style::NORMAL, Align::Left);
    doc.text(
        format!(
            "Engine: {}",
            match printer.effective_engine() {
                Engine::Raster => "Picture",
                Engine::Text => "Printer font",
            }
        ),
        Style::NORMAL,
        Align::Left,
    );
    doc.separator(Pattern::Dashed);

    // ------------------------------------------------------------------
    // The ruler. Two lines: marks every fifth column, numbers every tenth.
    // ------------------------------------------------------------------
    doc.text("Alignment ruler", Style::BOLD, Align::Left);
    // **Drawn by the layout, at the width it measured** (P32). Built here from
    // `paper.columns()` it ran off the right edge the moment a character's
    // width stopped being an assumption.
    doc.push(Block::Ruler { marks: true });
    doc.push(Block::Ruler { marks: false });
    doc.text(
        "The first and last marks must sit at the edges of the paper.",
        Style::NORMAL,
        Align::Left,
    );
    doc.separator(Pattern::Dashed);

    // ------------------------------------------------------------------
    // Scope 7.11 — what the offset currently is, in both units.
    // ------------------------------------------------------------------
    let offset = printer.paper.offset;
    doc.text(
        format!("Offset across: {}", describe_mm(offset.x_mm, printer)),
        Style::NORMAL,
        Align::Left,
    );
    doc.text(
        format!("Offset down: {}", describe_down(offset.y_mm, printer)),
        Style::NORMAL,
        Align::Left,
    );

    // A setting that silently did less than it was asked is how somebody spends
    // an evening nudging a number that stopped moving three steps ago. `Laid`
    // already knows; this asks it.
    if let Some(clamped) = clamp_warning(&doc) {
        doc.text(&clamped, Style::BOLD, Align::Left);
    }
    doc.separator(Pattern::Dashed);

    // ------------------------------------------------------------------
    // A real-looking bill, so the shape on the paper is the shape a customer
    // will see. Sample amounts go through `Money` like every other amount on
    // paper does (R2) — a test print that formatted its own numbers would be
    // testing the wrong renderer.
    // ------------------------------------------------------------------
    doc.text("Sample bill", Style::BOLD, Align::Left);
    doc.row("Masala Dosa x2", Money::from_paise(24_000).to_plain_string(), Style::NORMAL);
    doc.row(
        "Paneer Butter Masala (Half) - Extra Spicy",
        Money::from_paise(31_500).to_plain_string(),
        Style::NORMAL,
    );
    doc.row("Water 1L x3", Money::from_paise(6_000).to_plain_string(), Style::NORMAL);
    doc.separator(Pattern::Dashed);
    doc.row("Subtotal", Money::from_paise(61_500).to_plain_string(), Style::NORMAL);
    doc.row("CGST 2.5%", Money::from_paise(1_538).to_plain_string(), Style::NORMAL);
    doc.row("SGST 2.5%", Money::from_paise(1_538).to_plain_string(), Style::NORMAL);
    doc.row("Round off", Money::from_paise(24).to_plain_string(), Style::NORMAL);
    doc.row(
        "TOTAL",
        Money::from_paise(64_600).to_plain_string(),
        Style::new(2, true),
    );
    doc.separator(Pattern::Bold);

    doc.text("If this looks right, printing works.", Style::NORMAL, Align::Centre);
    doc.spacer(2);
    doc
}

/// `|....+....|` across the paper, with the ends at the edges.
#[must_use]
pub fn ruler_marks(columns: usize) -> String {
    (0..columns)
        .map(|i| {
            if i == 0 || i + 1 == columns || i % 10 == 0 {
                '|'
            } else if i % 5 == 0 {
                '+'
            } else {
                '.'
            }
        })
        .collect()
}

/// The tens digit under every tenth mark.
#[must_use]
pub fn ruler_numbers(columns: usize) -> String {
    let mut out = String::with_capacity(columns);
    for i in 0..columns {
        if i % 10 == 0 {
            // 0, 1, 2 … for columns 0, 10, 20. One character, so it cannot
            // push the ruler out of step with the marks above it.
            let tens = i.div_euclid(10) % 10;
            out.push(char::from_digit(u32::try_from(tens).unwrap_or(0), 10).unwrap_or('?'));
        } else {
            out.push(' ');
        }
    }
    out
}

/// "+2 mm right (2 characters)", or "none".
fn describe_mm(mm: i32, printer: &PrinterConfig) -> String {
    if mm == 0 {
        return "none".to_owned();
    }
    let columns = printer.paper.kind.columns_for_mm(mm);
    let direction = if mm > 0 { "right" } else { "left" };
    format!(
        "{mm:+} mm {direction} ({} characters)",
        columns.abs()
    )
}

fn describe_down(mm: i32, printer: &PrinterConfig) -> String {
    if mm == 0 {
        return "none".to_owned();
    }
    let lines = printer.paper.kind.columns_for_mm(mm);
    format!("{mm:+} mm ({} lines)", lines.abs())
}

/// Whether the layout had to reduce the offset, in words for the slip.
///
/// Laying the document out once to find out costs a fraction of a millisecond
/// and means there is exactly one clamping rule in the product — the layout's —
/// rather than a second copy of it here that can drift.
#[must_use]
pub fn clamp_warning(doc: &Document) -> Option<String> {
    let laid = layout(doc).ok()?;
    laid.notes.iter().find_map(|note| match note {
        Note::OffsetClamped { asked_mm, used_dots } => Some(format!(
            "NOTE: {asked_mm:+} mm was too far — it has been limited to \
             {} mm so the bill still fits the paper.",
            used_dots / i32::try_from(crate::paper::DOTS_PER_MM).unwrap_or(8)
        )),
        _ => None,
    })
}

fn describe(target: &Target) -> String {
    match target {
        Target::Spooler { name } => format!("Windows: {name}"),
        Target::Network { host, port } => format!("Network: {host}:{port}"),
        Target::Serial { port, baud } => format!("Serial: {port} at {baud}"),
        Target::File { path } => format!("File: {}", path.display()),
        Target::None => "None — nothing will be printed".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paper::{Offset, PaperKind};
    use crate::text::to_text;

    #[test]
    fn the_ruler_is_exactly_as_wide_as_the_line() {
        for columns in [32_usize, 48, 64] {
            assert_eq!(ruler_marks(columns).chars().count(), columns);
            assert_eq!(ruler_numbers(columns).chars().count(), columns);
        }
    }

    #[test]
    fn the_ruler_ends_at_both_edges() {
        let marks = ruler_marks(48);
        assert!(marks.starts_with('|'));
        assert!(marks.ends_with('|'));
    }

    #[test]
    fn a_test_print_works_with_no_shop_and_no_printer() {
        // First run, and after a restore: there is no database open.
        let printer = PrinterConfig::new("prn", "Counter", Target::None);
        let doc = test_document(&printer, None);
        let text = to_text(&layout(&doc).expect("lays out"));
        assert!(text.contains("TEST PRINT"));
        assert!(text.contains("MAGIC BILL"));
        assert!(text.contains("Alignment ruler"));
    }

    #[test]
    fn the_offset_is_printed_in_both_units() {
        let printer = PrinterConfig::new("prn", "Counter", Target::None)
            .with_paper(PaperKind::Mm80)
            .with_offset(Offset::new(3, 0));
        let doc = test_document(&printer, None);
        let text = to_text(&layout(&doc).expect("lays out"));
        // 3 mm on 80 mm paper is 2 columns — the owner has to be able to read
        // both, because they nudge in millimetres and see characters.
        assert!(text.contains("+3 mm right"), "{text}");
        assert!(text.contains("2 characters"), "{text}");
    }

    #[test]
    fn an_absurd_offset_says_it_was_limited() {
        let printer = PrinterConfig::new("prn", "Counter", Target::None)
            .with_paper(PaperKind::Mm58)
            .with_offset(Offset::new(20, 0));
        let doc = test_document(&printer, None);
        let text = to_text(&layout(&doc).expect("lays out"));
        assert!(
            text.contains("limited to"),
            "a clamp that says nothing is an evening wasted:\n{text}"
        );
    }
}
