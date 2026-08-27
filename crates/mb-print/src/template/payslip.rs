//! The payslip.

use crate::doc::{Align, Document, Pattern, Style};
use crate::paper::Paper;

/// One line of the arithmetic: a label and an amount, already formatted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaySlipLine {
    pub label: String,
    pub amount: String,
    /// True for a figure that comes OFF the pay.
    pub takes_away: bool,
}

/// What one person was paid, for one period.
#[derive(Debug, Clone)]
pub struct PayslipContext<'a> {
    pub shop: &'a str,
    pub person: &'a str,
    /// "Cook", "Counter" — whatever the shop calls the job.
    pub designation: Option<&'a str>,
    /// "1 August to 31 August 2026", formatted by the caller.
    pub period: &'a str,
    /// "₹18,000.00 a month", the basis this was worked out from.
    pub basis_says: &'a str,
    /// "26 days worked, 1 unpaid" — the attendance behind the figure.
    pub worked_says: &'a str,
    pub lines: &'a [PaySlipLine],
    pub net: &'a str,
    /// "Cash", "Bank".
    pub paid_by: &'a str,
    /// Set when a person changed a figure by hand before approving.
    pub edited: bool,
}

/// The slip.
#[must_use]
pub fn payslip_document(paper: Paper, ctx: &PayslipContext<'_>) -> Document {
    let mut doc = Document::new(paper);

    doc.text(ctx.shop, Style::new(1, true), Align::Centre);
    doc.text("PAYSLIP", Style::new(2, true), Align::Centre);
    doc.text(ctx.period, Style::NORMAL, Align::Centre);
    doc.separator(Pattern::Dashed);

    doc.text(ctx.person, Style::new(1, true), Align::Left);
    if let Some(designation) = ctx.designation {
        doc.text(designation, Style::NORMAL, Align::Left);
    }
    doc.row("On", ctx.basis_says, Style::NORMAL);
    doc.row("Worked", ctx.worked_says, Style::NORMAL);

    doc.separator(Pattern::Dashed);
    // The arithmetic, one step per line.
    for line in ctx.lines {
        let amount = if line.takes_away {
            format!("-{}", line.amount)
        } else {
            line.amount.clone()
        };
        doc.row(&line.label, &amount, Style::NORMAL);
    }

    doc.separator(Pattern::Solid);
    doc.row("NET PAY", ctx.net, Style::new(2, true));
    doc.row("Paid by", ctx.paid_by, Style::NORMAL);

    if ctx.edited {
        doc.spacer(1);
        doc.text(
            "One figure on this slip was changed by hand.",
            Style::NORMAL,
            Align::Left,
        );
    }

    // Two lines to sign on.
    doc.spacer(2);
    doc.row("Received", "____________________", Style::NORMAL);
    doc.row("Paid by", "____________________", Style::NORMAL);
    doc.spacer(1);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Block;
    use crate::paper::PaperKind;

    fn line(label: &str, amount: &str, takes_away: bool) -> PaySlipLine {
        PaySlipLine {
            label: label.to_owned(),
            amount: amount.to_owned(),
            takes_away,
        }
    }

    fn text_of(doc: &Document) -> String {
        doc.blocks
            .iter()
            .map(|b| match b {
                Block::Text { content, .. } => content.clone(),
                Block::Row { left, right, .. } => format!("{left} {right}"),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn ctx() -> PayslipContext<'static> {
        PayslipContext {
            shop: "Anand Bhavan",
            person: "Ravi",
            designation: Some("Head cook"),
            period: "1 August to 31 August 2026",
            basis_says: "\u{20b9}24,000.00 a month",
            worked_says: "26 days worked, 1 unpaid",
            lines: &[],
            net: "\u{20b9}21,500.00",
            paid_by: "Cash",
            edited: false,
        }
    }

    #[test]
    fn a_deduction_prints_with_a_minus_so_the_column_adds_up_by_eye() {
        let mut c = ctx();
        let lines = [
            line("Earned", "\u{20b9}23,000.00", false),
            line("Allowances", "\u{20b9}500.00", false),
            line("Advance recovered", "\u{20b9}2,000.00", true),
        ];
        c.lines = &lines;
        let text = text_of(&payslip_document(Paper::new(PaperKind::Mm80), &c));

        assert!(
            text.contains("Advance recovered -\u{20b9}2,000.00"),
            "{text}"
        );
        assert!(text.contains("Earned \u{20b9}23,000.00"), "{text}");
        assert!(text.contains("NET PAY \u{20b9}21,500.00"), "{text}");
    }

    #[test]
    fn an_edited_figure_is_on_the_paper() {
        // A slip that hides it is a slip that cannot be argued with.
        let mut c = ctx();
        c.edited = true;
        let text = text_of(&payslip_document(Paper::new(PaperKind::Mm80), &c));
        assert!(text.contains("changed by hand"), "{text}");
    }

    #[test]
    fn there_is_somewhere_to_sign() {
        let text = text_of(&payslip_document(Paper::new(PaperKind::Mm80), &ctx()));
        assert!(text.contains("Received"), "{text}");
    }
}
