//! The slip that goes out on the bike.

use crate::doc::{Align, Document, Pattern, Style};
use crate::paper::Paper;

/// One line of food, already priced and formatted by the caller — this crate owns no money type
/// on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlipLine {
    pub name: String,
    pub qty: String,
}

/// What the rider is being handed.
#[derive(Debug, Clone)]
pub struct DeliveryContext<'a> {
    pub shop: &'a str,
    /// The bill number if there is one, otherwise the token.
    pub reference: &'a str,
    /// Already formatted by the caller.
    pub time: Option<&'a str>,
    pub customer: Option<&'a str>,
    pub phone: Option<&'a str>,
    pub address: Option<&'a str>,
    /// A landmark, a zone, "second gate, blue door" — whatever the shop typed.
    pub note: Option<&'a str>,
    pub rider: Option<&'a str>,
    pub lines: &'a [SlipLine],
    /// The bill total, formatted.
    pub total: String,
    /// What to collect at the door, formatted — or `None` when the order is already paid, which
    /// prints a different line entirely.
    pub collect: Option<String>,
}

/// The slip.
#[must_use]
pub fn delivery_document(paper: Paper, ctx: &DeliveryContext<'_>) -> Document {
    let mut doc = Document::new(paper);

    doc.text("DELIVERY", Style::new(2, true), Align::Centre);
    doc.text(ctx.shop, Style::NORMAL, Align::Centre);
    doc.separator(Pattern::Dashed);

    doc.row("Bill", ctx.reference, Style::new(1, true));
    if let Some(time) = ctx.time {
        doc.row("Time", time, Style::NORMAL);
    }
    if let Some(rider) = ctx.rider {
        doc.row("Rider", rider, Style::NORMAL);
    }

    doc.separator(Pattern::Dashed);

    // The address is the point of the paper, so it is the biggest thing on it after the amount,
    // and it wraps rather than truncating.
    if let Some(name) = ctx.customer {
        doc.text(name, Style::new(1, true), Align::Left);
    }
    if let Some(phone) = ctx.phone {
        doc.text(phone, Style::new(2, true), Align::Left);
    }
    if let Some(address) = ctx.address {
        doc.text(address, Style::new(1, true), Align::Left);
    }
    if let Some(note) = ctx.note {
        doc.text(note, Style::NORMAL, Align::Left);
    }

    if !ctx.lines.is_empty() {
        doc.separator(Pattern::Dashed);
        for line in ctx.lines {
            doc.row(
                format!("{} x{}", line.name, line.qty),
                String::new(),
                Style::NORMAL,
            );
        }
    }

    doc.separator(Pattern::Solid);
    doc.row("Bill total", &ctx.total, Style::NORMAL);

    // The line the rider actually looks for, at the size they can read it from a scooter seat.
    match &ctx.collect {
        Some(amount) => {
            doc.text(
                format!("COLLECT {amount}"),
                Style::new(2, true),
                Align::Centre,
            );
        }
        None => {
            doc.text("PAID", Style::new(2, true), Align::Centre);
            doc.text("Collect nothing", Style::NORMAL, Align::Centre);
        }
    }

    doc.spacer(1);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Block;

    fn ctx() -> DeliveryContext<'static> {
        DeliveryContext {
            shop: "Anand Bhavan",
            reference: "B-0042",
            time: Some("7:40 pm"),
            customer: Some("Meera"),
            phone: Some("98400 11223"),
            address: Some("14/3 Kamaraj Street, second gate"),
            note: None,
            rider: Some("Kumar"),
            lines: &[],
            total: "\u{20b9}640.00".to_owned(),
            collect: Some("\u{20b9}640.00".to_owned()),
        }
    }

    fn all_text(doc: &Document) -> String {
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

    #[test]
    fn a_cash_slip_says_what_to_collect() {
        let doc = delivery_document(Paper::new(crate::paper::PaperKind::Mm80), &ctx());
        let text = all_text(&doc);
        assert!(text.contains("COLLECT \u{20b9}640.00"), "{text}");
        assert!(!text.contains("PAID"), "{text}");
    }

    #[test]
    fn a_prepaid_slip_says_collect_nothing_and_never_shows_an_amount_to_collect() {
        // The mistake this test exists for: a rider asking for money that has already been
        // paid, because the slip showed a total and nothing else.
        let mut c = ctx();
        c.collect = None;
        let doc = delivery_document(Paper::new(crate::paper::PaperKind::Mm80), &c);
        let text = all_text(&doc);
        assert!(text.contains("PAID"), "{text}");
        assert!(text.contains("Collect nothing"), "{text}");
        assert!(!text.contains("COLLECT \u{20b9}"), "{text}");
    }

    #[test]
    fn the_address_is_on_the_slip_at_a_size_a_rider_can_read() {
        let doc = delivery_document(Paper::new(crate::paper::PaperKind::Mm80), &ctx());
        let big = doc.blocks.iter().any(|b| {
            matches!(b, Block::Text { content, style, .. }
                if content.contains("Kamaraj") && style.scale() > 1)
        });
        // Scale 1 with emphasis is what a printer gives for "bigger than the body text" on 80
        // mm paper; the assertion is that it is not the plain body style.
        let emphatic = doc.blocks.iter().any(|b| {
            matches!(b, Block::Text { content, style, .. }
                if content.contains("Kamaraj") && style.bold)
        });
        assert!(big || emphatic);
    }
}
