//! **The closing slip — the Z-report v1 never had.**
//!
//! > Audit **B15**: *"no opening cash, no closing cash, no expected vs actual,
//! > no Z-report. This is how every restaurant actually closes the day and it
//! > does not exist."*
//!
//! This is the paper a shop tears off at 11 pm, staples to the day's cash and
//! puts in a drawer. It is the record that says the money was counted, by whom,
//! and what was missing.
//!
//! Like every other template in this crate it does **no arithmetic**. The
//! variance arrives computed, the expected cash arrives computed, and every
//! figure arrives as a string `Money::to_plain_string` produced. A renderer that
//! computes is a second money path (R2, D2) — and on this slip in particular,
//! a second answer to "how much should be in the drawer?" would be the exact
//! disagreement the slip exists to settle.

use crate::doc::{Align, Block, Column, Document, Pattern, Style};
use crate::paper::Paper;

use super::bill::Store;

/// One counted denomination, as the slip prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountedNote {
    /// "500", "10", "50p" — written by the caller, because what a shop calls a
    /// coin is the shop's language and not this crate's.
    pub label: String,
    pub count: u32,
    pub total: String,
}

/// One line of the money summary: a label and an amount, already formatted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlipLine {
    pub label: String,
    pub amount: String,
}

/// Everything the slip prints. **Nothing here is a number**, deliberately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayCloseContext<'a> {
    pub store: &'a Store,
    /// "2026-08-09".
    pub day: &'a str,
    /// "Closed at 11:14 pm by Ravi" — one sentence, assembled by the caller
    /// which knows the shop's clock and the person's name.
    pub closed: &'a str,
    /// Bills, voids, refunds, discounts — the day in figures.
    pub takings: &'a [SlipLine],
    /// Cash in and out, ending at "expected in the drawer".
    pub drawer: &'a [SlipLine],
    pub counted: &'a [CountedNote],
    pub counted_total: &'a str,
    pub expected: &'a str,
    /// **The sentence, not the number.** "Short by 340.00" and "Over by 20.00"
    /// are different words, and a minus sign in front of an amount on a till
    /// slip is read wrong by somebody eventually.
    pub variance: &'a str,
    pub reason: Option<&'a str>,
    /// "Left in the drawer for tomorrow: 2,000.00", when the shop carries a
    /// float. `None` when it does not.
    pub carried: Option<&'a str>,
    /// A place for a signature, because this slip is evidence.
    pub sign_off: bool,
}

/// Lay out the closing slip.
///
/// Infallible, unlike [`super::bill_document`]. There is nothing here that can
/// fail: no place-of-supply to reconcile, no QR payload to build, and the
/// narrowest paper this product supports is 32 columns, which the grid fits.
/// A `Result` with no error case would be a promise of checking that is not
/// happening.
#[must_use]
pub fn day_close_document(paper: Paper, context: &DayCloseContext<'_>) -> Document {
    let mut doc = Document::new(paper);
    doc.text(context.store.name.clone(), Style::new(2, true), Align::Centre);
    if !context.store.address.is_empty() {
        doc.text(context.store.address.clone(), Style::NORMAL, Align::Centre);
    }
    doc.spacer(1)
        .text("DAY CLOSE", Style::new(1, true), Align::Centre)
        .text(context.day.to_owned(), Style::NORMAL, Align::Centre)
        .separator(Pattern::Double);

    for line in context.takings {
        doc.row(line.label.clone(), line.amount.clone(), Style::NORMAL);
    }

    doc.separator(Pattern::Solid)
        .text("THE DRAWER", Style::new(1, true), Align::Left);
    for line in context.drawer {
        doc.row(line.label.clone(), line.amount.clone(), Style::NORMAL);
    }

    // The count. Three columns — what it is, how many, what that comes to —
    // because "we are always short of tens" is a thing an owner works out by
    // reading a stack of these.
    if !context.counted.is_empty() {
        doc.separator(Pattern::Solid)
            .text("COUNTED", Style::new(1, true), Align::Left)
            .push(Block::Columns {
                columns: vec![
                    Column::fixed(6, Align::Right),
                    Column::fixed(1, Align::Left), // the gutter, and it HAS to
                    // be a column of its own — `wrap` drops leading spaces, and
                    // that is how "2Paneer Butter Masala" reached a kitchen for
                    // eleven prompts (P17).
                    Column::fixed(5, Align::Right),
                    Column::fill(Align::Right),
                ],
                rows: context
                    .counted
                    .iter()
                    .map(|note| {
                        vec![
                            note.label.clone(),
                            String::new(),
                            format!("x {}", note.count),
                            note.total.clone(),
                        ]
                    })
                    .collect(),
                style: Style::NORMAL,
            });
    }

    doc.separator(Pattern::Solid)
        .row("Counted", context.counted_total.to_owned(), Style::new(1, true))
        .row("Expected", context.expected.to_owned(), Style::NORMAL)
        // The variance is the line a person looks for, so it is the big one.
        .text(context.variance.to_owned(), Style::new(2, true), Align::Centre);

    if let Some(reason) = context.reason {
        doc.spacer(1).line(format!("Reason: {reason}"));
    }
    if let Some(carried) = context.carried {
        doc.line(carried.to_owned());
    }

    doc.separator(Pattern::Double)
        .line(context.closed.to_owned());

    if context.sign_off {
        doc.spacer(2)
            .line("Counted by ______________________")
            .spacer(1)
            .line("Checked by ______________________");
    }
    doc.spacer(1);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::paper::PaperKind;
    use crate::text::to_text;

    fn a_store() -> Store {
        Store {
            name: "Anna Kuteera".to_owned(),
            address: "4th Block, Jayanagar".to_owned(),
            ..Store::default()
        }
    }

    fn a_slip(variance: &str, reason: Option<&str>) -> String {
        let store = a_store();
        let takings = [
            SlipLine { label: "Bills (48)".to_owned(), amount: "24,120.00".to_owned() },
            SlipLine { label: "Voided (2)".to_owned(), amount: "-620.00".to_owned() },
        ];
        let drawer = [
            SlipLine { label: "Opening float".to_owned(), amount: "2,000.00".to_owned() },
            SlipLine { label: "Cash sales".to_owned(), amount: "9,400.00".to_owned() },
        ];
        let counted = [
            CountedNote { label: "500".to_owned(), count: 20, total: "10,000.00".to_owned() },
            CountedNote { label: "10".to_owned(), count: 6, total: "60.00".to_owned() },
        ];
        let context = DayCloseContext {
            store: &store,
            day: "2026-08-09",
            closed: "Closed at 11:14 pm by Ravi",
            takings: &takings,
            drawer: &drawer,
            counted: &counted,
            counted_total: "11,060.00",
            expected: "11,400.00",
            variance,
            reason,
            carried: Some("Left in the drawer for tomorrow: 2,000.00"),
            sign_off: true,
        };
        let doc = day_close_document(Paper::new(PaperKind::Mm80), &context);
        to_text(&layout(&doc).expect("lays out"))
    }

    /// **T6.** The slip carries every figure a person needs to check the day,
    /// and the difference is words rather than a signed number.
    #[test]
    fn the_slip_says_what_is_missing_in_words() {
        let text = a_slip("SHORT BY 340.00", Some("paid the vegetable man from the drawer"));
        assert!(text.contains("DAY CLOSE"), "{text}");
        assert!(text.contains("2026-08-09"));
        assert!(text.contains("Anna Kuteera"));
        assert!(text.contains("Bills (48)"));
        assert!(text.contains("SHORT BY 340.00"), "{text}");
        // The reason is on the paper. A slip that records ₹340 missing without
        // recording why is the finding, not the fix.
        assert!(text.contains("paid the vegetable man"), "{text}");
        assert!(text.contains("Closed at 11:14 pm by Ravi"));
        assert!(text.contains("Counted by"));
    }

    /// The denomination grid keeps its gutter — P17's "2Paneer Butter Masala"
    /// bug was a missing column, and this grid has the same shape.
    #[test]
    fn the_note_count_has_a_space_between_its_columns() {
        let text = a_slip("MATCHES EXACTLY", None);
        assert!(text.contains("500"), "{text}");
        // "x 20", not "20x20" and not "500x 20".
        assert!(text.contains("x 20"), "{text}");
        assert!(text.contains("x 6"), "{text}");
        // No reason line when there is nothing to explain.
        assert!(!text.contains("Reason:"), "{text}");
    }

    /// **The narrowest paper a shop can own still holds the slip.** 32 columns
    /// is a 58 mm roll, which is what a small counter runs.
    #[test]
    fn it_fits_on_the_narrow_roll_too() {
        let store = a_store();
        let counted = [CountedNote {
            label: "500".to_owned(),
            count: 20,
            total: "10,000.00".to_owned(),
        }];
        let context = DayCloseContext {
            store: &store,
            day: "2026-08-09",
            closed: "Closed at 11:14 pm by Ravi",
            takings: &[],
            drawer: &[],
            counted: &counted,
            counted_total: "10,000.00",
            expected: "10,000.00",
            variance: "MATCHES EXACTLY",
            reason: None,
            carried: None,
            sign_off: false,
        };
        let doc = day_close_document(Paper::new(PaperKind::Mm58), &context);
        let text = to_text(&layout(&doc).expect("lays out"));
        for line in text.lines() {
            assert!(line.chars().count() <= 32, "too wide: {line:?}");
        }
        assert!(text.contains("MATCHES EXACTLY"), "{text}");
    }
}
