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
    /// **The ticket's own running number** — P32.
    ///
    /// A cook could not say *"KOT 14"* because there was no such number. It is
    /// claimed per business day like the token and the bill number, so two
    /// tickets for one order are 14 and 15 and a kitchen can talk about them.
    pub kot_number: Option<&'a str>,
    pub order_type: OrderType,
    pub table: Option<&'a str>,
    /// Already formatted by the caller. This crate owns no clock (D5, D19).
    pub time: Option<&'a str>,
    /// Who called the order in. A cook with a question needs a person to ask.
    pub waiter: Option<&'a str>,
    /// Scope 3.1 — the station this ticket is going to, when the shop routes
    /// by category.
    pub station: Option<&'a str>,
    /// **A ticket printed again**, marked so the kitchen does not cook it
    /// twice. The bill has had `Copy::Duplicate` since P06; the ticket that
    /// actually causes food to be made had nothing.
    pub reprint: bool,
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

    // **The head, in one or two rows instead of six** — P32.
    //
    // A ticket is read across a hot room in a second. It used to be a centred
    // title, a centred token, and then `Bill`, `Type`, `Table` and `Time` as
    // four full-width label/value rows — six rows of paper before a cook saw a
    // single dish. The title and the ticket's own number share a line; what the
    // cook needs to place the order (table, type, time) shares another.
    if s.show_title {
        let title = match ctx.kind {
            TicketKind::New => "KITCHEN",
            TicketKind::Cancellation => "*** CANCEL ***",
        };
        match ctx.kot_number.filter(|_| s.show_kot_number) {
            Some(number) => {
                doc.row(title, format!("KOT {number}"), s.title);
            }
            None => {
                doc.text(title, s.title, Align::Centre);
            }
        }
        if let Some(station) = ctx.station {
            doc.text(station, s.details, Align::Centre);
        }
        if ctx.reprint {
            // The same weight the bill gives a duplicate, and for a bigger
            // reason: a ticket printed twice is food cooked twice.
            doc.text("*** REPRINT ***", s.details, Align::Centre);
        }
        if s.separators.below_title {
            doc.separator(s.pattern);
        }
    }

    if s.show_token && let Some(token) = ctx.token {
        doc.text(format!("TOKEN {token}"), s.title, Align::Centre);
        if s.separators.below_token {
            doc.separator(s.pattern);
        }
    }

    // One row: what to put it on, what kind of order it is, and when it was
    // called. Three columns rather than three rows.
    let table = match (s.show_table, ctx.table) {
        (true, Some(table)) => format!("Table {table}"),
        _ => String::new(),
    };
    let kind = if s.show_order_type {
        match ctx.order_type {
            OrderType::DineIn => "Dine In",
            OrderType::Parcel => "Parcel",
            OrderType::SelfService => "Self Service",
            OrderType::Delivery => "Delivery",
        }
    } else {
        ""
    };
    let time = match (s.show_time, ctx.time) {
        (true, Some(time)) => time,
        _ => "",
    };
    if !table.is_empty() || !kind.is_empty() || !time.is_empty() {
        doc.push(Block::Columns {
            columns: vec![
                Column::fill(Align::Left),
                Column::fill(Align::Centre),
                Column::fill(Align::Right),
            ],
            rows: vec![vec![table, kind.to_owned(), time.to_owned()]],
            style: s.details,
        });
    }
    // The bill number and the waiter are for a question, not for cooking, so
    // they go on one quiet row under the rest.
    let mut aside = Vec::new();
    if s.show_bill_number && let Some(number) = ctx.bill_number {
        aside.push(format!("Bill {number}"));
    }
    if let Some(waiter) = ctx.waiter {
        aside.push(waiter.to_owned());
    }
    if !aside.is_empty() {
        doc.text(aside.join("   "), s.details, Align::Left);
    }
    if s.separators.below_details {
        doc.separator(s.pattern);
    }

    if s.two_column {
        two_column_items(&mut doc, ctx);
    } else {
        one_column_items(&mut doc, ctx);
    }

    if s.separators.below_items {
        doc.separator(s.pattern);
    }
    // One blank row, not two: the job already feeds three lines before the
    // blade (P32).
    doc.spacer(1);

    Ok(doc)
}

/// The normal ticket: quantity first, because the kitchen reads the number, not
/// the name.
fn one_column_items(doc: &mut Document, ctx: &KitchenContext<'_>) {
    let s = ctx.settings;
    // **The middle column is a gutter, and it has to be a column.**
    //
    // `lay_row` writes cells end to end with nothing between them, so a
    // right-aligned quantity butts straight into a left-aligned name: the
    // ticket read "2Paneer Butter Masala" from P06 until P17 put it on screen.
    // Putting a space at the front of the name cell does NOT work — `wrap`
    // splits on spaces and drops empty words, so a leading space disappears.
    // On a character grid a gap is a character, and a character in a row of
    // columns is a column.
    let columns = vec![
        Column::fixed(4, Align::Right),
        Column::fixed(1, Align::Left),
        Column::fill(Align::Left),
    ];

    if s.show_column_names {
        doc.push(Block::Columns {
            columns: columns.clone(),
            rows: vec![vec!["Qty".to_owned(), String::new(), "Item".to_owned()]],
            style: s.details,
        });
        if s.separators.below_column_names {
            doc.separator(s.pattern);
        }
    }

    let gap = usize::from(s.row_height.gap());
    let mut rows = Vec::with_capacity(ctx.lines.len());
    for (n, line) in ctx.lines.iter().enumerate() {
        // A blank row between dishes, not after the last one — trailing air is
        // what `doc.spacer` at the end is for, and doubling it wastes paper on
        // every ticket of the day.
        if n > 0 {
            for _ in 0..gap {
                rows.push(vec![String::new(), String::new(), String::new()]);
            }
        }
        rows.push(vec![
            line.qty.to_string(),
            String::new(),
            line.name.clone(),
        ]);
        for modifier in &line.modifiers {
            rows.push(vec![
                String::new(),
                String::new(),
                format!("+ {modifier}"),
            ]);
        }
        if let Some(note) = &line.note {
            rows.push(vec![String::new(), String::new(), format!("* {note}")]);
        }
    }

    doc.push(Block::Columns {
        columns,
        rows,
        style: s.items,
    });
}

/// v1's "2-column packing" — two dishes across, for a kitchen that would rather
/// have a short ticket than a readable one.
///
/// **The whole dish becomes one cell**, quantity, modifiers and note together.
/// A note on its own row cannot survive being packed beside somebody else's
/// dish: the ticket would read as though the note belonged to both.
fn two_column_items(doc: &mut Document, ctx: &KitchenContext<'_>) {
    let s = ctx.settings;
    let columns = vec![Column::fill(Align::Left), Column::fill(Align::Left)];

    if s.show_column_names {
        doc.push(Block::Columns {
            columns: columns.clone(),
            rows: vec![vec!["Item".to_owned(), "Item".to_owned()]],
            style: s.details,
        });
        if s.separators.below_column_names {
            doc.separator(s.pattern);
        }
    }

    let cells: Vec<String> = ctx
        .lines
        .iter()
        .map(|line| {
            let mut cell = format!("{} {}", line.qty, line.name);
            for modifier in &line.modifiers {
                cell.push_str(&format!(" +{modifier}"));
            }
            if let Some(note) = &line.note {
                cell.push_str(&format!(" *{note}"));
            }
            cell
        })
        .collect();

    let gap = usize::from(s.row_height.gap());
    let mut rows = Vec::with_capacity(cells.len().div_ceil(2));
    for (n, pair) in cells.chunks(2).enumerate() {
        if n > 0 {
            for _ in 0..gap {
                rows.push(vec![String::new(), String::new()]);
            }
        }
        rows.push(vec![
            pair.first().cloned().unwrap_or_default(),
            pair.get(1).cloned().unwrap_or_default(),
        ]);
    }

    doc.push(Block::Columns {
        columns,
        rows,
        style: s.items,
    });
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
