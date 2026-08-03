//! The kitchen ticket — **a delta, not an order.**
//!
//! Crown jewel 2, and the audit is emphatic about it:
//!
//! > *"The delta KOT. Only what the kitchen has not seen gets printed, and what
//! > was printed is remembered **in the database**, not in the screen's
//! > memory."*
//!
//! mb-core's `KitchenLedger::pending` decides what is new; mb-db stores what has
//! been told; this file only prints what it is handed. It does not compute a
//! delta and it must not — that rule exists once, in mb-core, and P03 spent an
//! item making sure of it.
//!
//! **In cart order.** Never sorted, never grouped, unless the shop has asked
//! for category-wise tickets — the ticket has to read in the sequence the
//! waiter called the items, or the kitchen loses its place.

use mb_core::{LineIdentity, OrderType, Qty};
use serde::{Deserialize, Serialize};

use crate::doc::{Align, Block, Column, Document, Style};
use crate::error::PrintError;
use crate::paper::Paper;
use crate::settings::KitchenSettings;

/// One line of the ticket: what to cook, how many, and what the waiter said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketLine {
    pub name: String,
    pub qty: Qty,
    pub note: Option<String>,
    /// The modifier names, already resolved. The ledger stores ids; whoever
    /// calls this has the menu and this crate does not.
    pub modifiers: Vec<String>,
}

impl TicketLine {
    /// Build a line from a ledger delta plus the names the caller looked up.
    #[must_use]
    pub fn from_delta(identity: &LineIdentity, qty: Qty, name: String, modifiers: Vec<String>) -> Self {
        TicketLine {
            name,
            qty,
            note: identity.note.clone(),
            modifiers,
        }
    }
}

/// What kind of ticket this is.
///
/// A cancellation slip is scope 1.19 and P12 owns the decision to send one —
/// but the shape belongs here, beside the ticket it mirrors, so P12 does not
/// invent a second one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketKind {
    #[default]
    New,
    /// Scope 1.19 — food the kitchen was told about that is no longer ordered.
    /// P03's `KitchenLedger::over_told` is what finds it.
    Cancellation,
}

#[derive(Debug, Clone)]
pub struct KitchenContext<'a> {
    pub kind: TicketKind,
    pub token: Option<&'a str>,
    pub bill_number: Option<&'a str>,
    pub order_type: OrderType,
    pub table: Option<&'a str>,
    /// Already formatted by the caller. This crate owns no clock (D5, D19).
    pub time: Option<&'a str>,
    /// Scope 3.1 — the station this ticket is going to, when the shop routes
    /// by category.
    pub station: Option<&'a str>,
    pub lines: &'a [TicketLine],
    pub settings: &'a KitchenSettings,
}

pub fn kitchen_document(paper: Paper, ctx: &KitchenContext<'_>) -> Result<Document, PrintError> {
    if ctx.lines.is_empty() {
        // An empty delta means the kitchen already knows everything. Printing a
        // blank ticket wastes paper and, worse, teaches the kitchen to ignore
        // tickets.
        return Err(PrintError::invalid(
            "there is nothing new to tell the kitchen",
        ));
    }

    let s = ctx.settings;
    let mut doc = Document::new(paper);

    if s.show_title {
        let title = match ctx.kind {
            TicketKind::New => "KITCHEN",
            TicketKind::Cancellation => "*** CANCEL ***",
        };
        doc.text(title, s.title, Align::Centre);
        if let Some(station) = ctx.station {
            doc.text(station, s.details, Align::Centre);
        }
        doc.separator(s.pattern);
    }

    if s.show_token && let Some(token) = ctx.token {
        doc.text(format!("TOKEN {token}"), s.title, Align::Centre);
    }
    if s.show_bill_number && let Some(number) = ctx.bill_number {
        doc.row("Bill", number, s.details);
    }
    if s.show_order_type {
        doc.row(
            "Type",
            match ctx.order_type {
                OrderType::DineIn => "Dine In",
                OrderType::Parcel => "Parcel",
                OrderType::SelfService => "Self Service",
                OrderType::Delivery => "Delivery",
            },
            s.details,
        );
    }
    if s.show_table && let Some(table) = ctx.table {
        doc.row("Table", table, s.details);
    }
    if s.show_time && let Some(time) = ctx.time {
        doc.row("Time", time, s.details);
    }
    doc.separator(s.pattern);

    // Quantity first: the kitchen reads the number, not the name.
    let columns = vec![Column::fixed(4, Align::Right), Column::fill(Align::Left)];
    let mut rows = Vec::with_capacity(ctx.lines.len());
    for line in ctx.lines {
        rows.push(vec![line.qty.to_string(), line.name.clone()]);
        for modifier in &line.modifiers {
            rows.push(vec![String::new(), format!("  + {modifier}")]);
        }
        if let Some(note) = &line.note {
            rows.push(vec![String::new(), format!("  * {note}")]);
        }
    }

    doc.push(Block::Columns {
        columns,
        rows,
        style: s.items,
    });
    doc.separator(s.pattern);
    doc.spacer(2);

    Ok(doc)
}

/// A parcel label (scope 7.9): a small document with its own paper.
#[derive(Debug, Clone)]
pub struct LabelContext<'a> {
    pub shop: &'a str,
    pub token: &'a str,
    pub line: &'a str,
    pub of: Option<(u32, u32)>,
}

pub fn label_document(paper: Paper, ctx: &LabelContext<'_>) -> Document {
    let mut doc = Document::new(paper);
    doc.text(ctx.shop, Style::new(1, true), Align::Centre);
    doc.text(
        format!("TOKEN {}", ctx.token),
        Style::new(2, true),
        Align::Centre,
    );
    doc.text(ctx.line, Style::NORMAL, Align::Centre);
    if let Some((n, total)) = ctx.of {
        doc.text(format!("{n} of {total}"), Style::NORMAL, Align::Centre);
    }
    doc
}
