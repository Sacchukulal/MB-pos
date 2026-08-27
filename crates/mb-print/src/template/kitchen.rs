//! The kitchen ticket — a delta, not an order.

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
    /// The modifier names, already resolved.
    pub modifiers: Vec<String>,
}

impl TicketLine {
    /// Build a line from a ledger delta plus the names the caller looked up.
    #[must_use]
    pub fn from_delta(
        identity: &LineIdentity,
        qty: Qty,
        name: String,
        modifiers: Vec<String>,
    ) -> Self {
        TicketLine {
            name,
            qty,
            note: identity.note.clone(),
            modifiers,
        }
    }
}

/// What kind of ticket this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketKind {
    #[default]
    New,
    /// Food the kitchen was told about that is no longer ordered.
    Cancellation,
}

#[derive(Debug, Clone)]
pub struct KitchenContext<'a> {
    pub kind: TicketKind,
    pub token: Option<&'a str>,
    pub bill_number: Option<&'a str>,
    /// The ticket's own running number.
    pub kot_number: Option<&'a str>,
    pub order_type: OrderType,
    pub table: Option<&'a str>,
    /// Already formatted by the caller.
    pub time: Option<&'a str>,
    /// Who called the order in.
    pub waiter: Option<&'a str>,
    /// The station this ticket is going to, when the shop routes by category.
    pub station: Option<&'a str>,
    /// A ticket printed again, marked so the kitchen does not cook it twice.
    pub reprint: bool,
    pub lines: &'a [TicketLine],
    pub settings: &'a KitchenSettings,
}

pub fn kitchen_document(paper: Paper, ctx: &KitchenContext<'_>) -> Result<Document, PrintError> {
    if ctx.lines.is_empty() {
        // An empty delta means the kitchen already knows everything.
        return Err(PrintError::invalid(
            "there is nothing new to tell the kitchen",
        ));
    }

    let s = ctx.settings;
    let mut doc = Document::new(paper);

    // The head, in one or two rows instead of six.
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
            // The same weight the bill gives a duplicate, and for a bigger reason: a ticket
            // printed twice is food cooked twice.
            doc.text("*** REPRINT ***", s.details, Align::Centre);
        }
        if s.separators.below_title {
            doc.separator(s.pattern);
        }
    }

    if s.show_token
        && let Some(token) = ctx.token
    {
        doc.text(format!("TOKEN {token}"), s.title, Align::Centre);
        if s.separators.below_token {
            doc.separator(s.pattern);
        }
    }

    // One row: what to put it on, what kind of order it is, and when it was called.
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
    // The bill number and the waiter are for a question, not for cooking, so they go on one
    // quiet row under the rest.
    let mut aside = Vec::new();
    if s.show_bill_number
        && let Some(number) = ctx.bill_number
    {
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
    // One blank row, not two: the job already feeds three lines before the blade.
    doc.spacer(1);

    Ok(doc)
}

/// The normal ticket: quantity first, because the kitchen reads the number, not the name.
fn one_column_items(doc: &mut Document, ctx: &KitchenContext<'_>) {
    let s = ctx.settings;
    // The middle column is a gutter, and it has to be a column.
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
        // A blank row between dishes, not after the last one — trailing air is what
        // `doc.spacer` at the end is for, and doubling it wastes paper on every ticket of the
        // day.
        if n > 0 {
            for _ in 0..gap {
                rows.push(vec![String::new(), String::new(), String::new()]);
            }
        }
        rows.push(vec![line.qty.to_string(), String::new(), line.name.clone()]);
        for modifier in &line.modifiers {
            rows.push(vec![String::new(), String::new(), format!("+ {modifier}")]);
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

/// A parcel label: a small document with its own paper.
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
