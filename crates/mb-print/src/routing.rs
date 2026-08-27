//! Which printer gets which items.

use serde::{Deserialize, Serialize};

use crate::printer::PrinterConfig;
use crate::template::TicketLine;

/// One printer, or a printer per category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrinterMode {
    #[default]
    Single,
    Multiple,
}

/// One ticket per printer, or one per category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketStyle {
    #[default]
    Combined,
    CategoryWise,
}

/// A line, with the category it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedLine {
    pub line: TicketLine,
    pub category_id: Option<String>,
    pub category_name: Option<String>,
}

/// One ticket to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub printer_id: String,
    /// The station heading, when the shop prints category-wise.
    pub station: Option<String>,
    pub lines: Vec<TicketLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoutingTable {
    pub mode: PrinterMode,
    pub style: TicketStyle,
    /// Where anything unmapped goes.
    pub default_printer: String,
    /// `(category id, printer id)`.
    pub by_category: Vec<(String, String)>,
    /// Where a bill goes, when it is not the default printer.
    pub bill_printer: Option<String>,
}

impl RoutingTable {
    #[must_use]
    pub fn single(default_printer: impl Into<String>) -> RoutingTable {
        RoutingTable {
            mode: PrinterMode::Single,
            style: TicketStyle::Combined,
            default_printer: default_printer.into(),
            by_category: Vec::new(),
            bill_printer: None,
        }
    }

    /// Which printer this category's food goes to.
    #[must_use]
    pub fn printer_for(&self, category_id: Option<&str>) -> String {
        if matches!(self.mode, PrinterMode::Single) {
            return self.default_printer.clone();
        }
        let Some(category) = category_id else {
            return self.default_printer.clone();
        };
        self.by_category
            .iter()
            .find(|(id, _)| id == category)
            .map_or_else(
                || self.default_printer.clone(),
                |(_, printer)| printer.clone(),
            )
    }

    /// Where the bill goes.
    #[must_use]
    pub fn bill_printer(&self) -> String {
        self.bill_printer
            .clone()
            .unwrap_or_else(|| self.default_printer.clone())
    }
}

/// Split a delta into the tickets that have to be printed.
#[must_use]
pub fn route(lines: &[RoutedLine], table: &RoutingTable) -> Vec<Ticket> {
    // A Vec and a linear search rather than a map: there are at most a handful of tickets, and
    // insertion order IS the answer here.
    let mut tickets: Vec<Ticket> = Vec::new();

    for routed in lines {
        let printer_id = table.printer_for(routed.category_id.as_deref());
        let station = match table.style {
            TicketStyle::CategoryWise => routed.category_name.clone(),
            TicketStyle::Combined => None,
        };

        let existing = tickets
            .iter_mut()
            .find(|t| t.printer_id == printer_id && t.station == station);
        match existing {
            Some(ticket) => ticket.lines.push(routed.line.clone()),
            None => tickets.push(Ticket {
                printer_id,
                station,
                lines: vec![routed.line.clone()],
            }),
        }
    }

    tickets
}

/// Drop the tickets a printer is not allowed to receive, and say which.
#[must_use]
pub fn split_by_role(
    tickets: Vec<Ticket>,
    printers: &[PrinterConfig],
) -> (Vec<Ticket>, Vec<Ticket>) {
    let mut printable = Vec::new();
    let mut refused = Vec::new();
    for ticket in tickets {
        let allowed = printers
            .iter()
            .find(|p| p.id == ticket.printer_id)
            .is_some_and(|p| p.role.accepts_kitchen());
        if allowed {
            printable.push(ticket);
        } else {
            refused.push(ticket);
        }
    }
    (printable, refused)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_core::Qty;

    fn line(name: &str) -> TicketLine {
        TicketLine {
            name: name.to_owned(),
            qty: Qty::from_whole(1).expect("qty"),
            note: None,
            modifiers: Vec::new(),
        }
    }

    fn routed(name: &str, category: &str) -> RoutedLine {
        RoutedLine {
            line: line(name),
            category_id: Some(format!("cat_{category}")),
            category_name: Some(category.to_owned()),
        }
    }

    fn multiple() -> RoutingTable {
        RoutingTable {
            mode: PrinterMode::Multiple,
            style: TicketStyle::Combined,
            default_printer: "prn_kitchen".to_owned(),
            by_category: vec![("cat_tandoor".to_owned(), "prn_tandoor".to_owned())],
            bill_printer: Some("prn_counter".to_owned()),
        }
    }

    #[test]
    fn single_mode_sends_everything_to_the_default() {
        let table = RoutingTable::single("prn_kitchen");
        let tickets = route(
            &[routed("Dosa", "south"), routed("Naan", "tandoor")],
            &table,
        );
        assert_eq!(tickets.len(), 1);
        assert_eq!(tickets[0].printer_id, "prn_kitchen");
        assert_eq!(tickets[0].lines.len(), 2);
    }

    #[test]
    fn multiple_mode_splits_by_category() {
        let tickets = route(
            &[routed("Dosa", "south"), routed("Naan", "tandoor")],
            &multiple(),
        );
        assert_eq!(tickets.len(), 2);
        assert_eq!(tickets[0].printer_id, "prn_kitchen");
        assert_eq!(tickets[1].printer_id, "prn_tandoor");
    }

    #[test]
    fn a_category_nobody_mapped_still_prints() {
        // A shop that adds a category on a Friday evening has not mapped it, and silence is the
        // failure mode that loses food.
        let tickets = route(&[routed("Falooda", "desserts")], &multiple());
        assert_eq!(tickets.len(), 1);
        assert_eq!(tickets[0].printer_id, "prn_kitchen");
    }

    #[test]
    fn category_wise_makes_one_ticket_per_category_on_one_printer() {
        let mut table = RoutingTable::single("prn_kitchen");
        table.style = TicketStyle::CategoryWise;
        let tickets = route(
            &[
                routed("Dosa", "south"),
                routed("Naan", "tandoor"),
                routed("Idli", "south"),
            ],
            &table,
        );
        assert_eq!(tickets.len(), 2, "scope 1.8: one ticket per category");
        assert_eq!(tickets[0].station.as_deref(), Some("south"));
        assert_eq!(tickets[0].lines.len(), 2);
        assert_eq!(tickets[1].station.as_deref(), Some("tandoor"));
    }

    #[test]
    fn cart_order_survives_every_mode() {
        // The ticket has to read in the sequence the waiter called the items.
        let lines = [
            routed("Dosa", "south"),
            routed("Idli", "south"),
            routed("Vada", "south"),
        ];
        for table in [RoutingTable::single("prn_kitchen"), multiple()] {
            let tickets = route(&lines, &table);
            let names: Vec<&str> = tickets[0].lines.iter().map(|l| l.name.as_str()).collect();
            assert_eq!(names, ["Dosa", "Idli", "Vada"]);
        }
    }

    #[test]
    fn a_bill_only_printer_refuses_a_ticket() {
        use crate::printer::{Role, Target};

        let counter =
            PrinterConfig::new("prn_counter", "Counter", Target::None).with_role(Role::Bill);
        let tickets = route(
            &[routed("Dosa", "south")],
            &RoutingTable::single("prn_counter"),
        );
        let (printable, refused) = split_by_role(tickets, &[counter]);
        assert!(printable.is_empty());
        assert_eq!(refused.len(), 1, "refused, not silently dropped");
    }
}
