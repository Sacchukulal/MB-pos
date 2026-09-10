//! The kitchen ticket — a delta, not an order.

use mb_core::{LineIdentity, OrderType, Qty};
use serde::{Deserialize, Serialize};

use crate::doc::{Align, Block, Column, Document, Style};
use crate::error::PrintError;
use crate::paper::Paper;
use crate::settings::{KitchenSettings, TicketFormat};

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

    match s.format {
        TicketFormat::Classic => classic_head(&mut doc, ctx),
        TicketFormat::BigToken => big_token_head(&mut doc, ctx),
        TicketFormat::TableCard => table_card_head(&mut doc, ctx),
        TicketFormat::Slip => slip_head(&mut doc, ctx),
    }
    // Air before the food, so the cook's eye lands on the first line and not on the header.
    doc.air(s.row_height.section_air());

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

/// The word at the top of the ticket.
const fn title_of(ctx: &KitchenContext<'_>) -> &'static str {
    match ctx.kind {
        TicketKind::New => "KITCHEN",
        TicketKind::Cancellation => "*** CANCEL ***",
    }
}

/// The biggest text a ticket carries: the token or the table, read across the kitchen.
const BIGGEST: Style = Style {
    size: Style::LARGEST,
    bold: true,
};

/// `KOT 14`, when the ticket shows its number.
fn kot_of(ctx: &KitchenContext<'_>) -> Option<String> {
    ctx.kot_number
        .filter(|_| ctx.settings.show_kot_number)
        .map(|number| format!("KOT {number}"))
}

/// `TOKEN 7`, when the ticket shows it.
fn token_of(ctx: &KitchenContext<'_>) -> Option<String> {
    ctx.token
        .filter(|_| ctx.settings.show_token)
        .map(|token| format!("TOKEN {token}"))
}

/// `Table 6`, when the ticket shows it.
fn table_of(ctx: &KitchenContext<'_>) -> Option<String> {
    ctx.table
        .filter(|_| ctx.settings.show_table)
        .map(|table| format!("Table {table}"))
}

fn kind_of(ctx: &KitchenContext<'_>) -> Option<&'static str> {
    ctx.settings
        .show_order_type
        .then(|| super::order_type_label(ctx.order_type))
}

fn time_of<'a>(ctx: &KitchenContext<'a>) -> Option<&'a str> {
    ctx.time.filter(|_| ctx.settings.show_time)
}

/// A cancellation says so at the top whatever the format, and a reprint says so too: a ticket
/// cooked twice is food thrown away. The station line goes with them.
fn marks(doc: &mut Document, ctx: &KitchenContext<'_>) {
    let s = ctx.settings;
    if let Some(station) = ctx.station {
        doc.text(station, s.details, Align::Centre);
    }
    if ctx.reprint {
        doc.text("*** REPRINT ***", s.details, Align::Centre);
    }
}

/// Three cells across one row, when any of them has something in it.
fn three_across(doc: &mut Document, style: Style, cells: [String; 3]) {
    if cells.iter().all(String::is_empty) {
        return;
    }
    doc.push(Block::Columns {
        columns: vec![
            Column::fill(Align::Left),
            Column::fill(Align::Centre),
            Column::fill(Align::Right),
        ],
        rows: vec![cells.to_vec()],
        style,
    });
}

/// The bill number and the waiter are for a question, not for cooking, so they go on one
/// quiet row under the rest.
fn aside_row(doc: &mut Document, ctx: &KitchenContext<'_>) {
    let s = ctx.settings;
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
}

/// The title row and what goes with it, in the details size — the formats that make the
/// token or the table the big thing keep the title small.
fn small_title(doc: &mut Document, ctx: &KitchenContext<'_>) {
    let s = ctx.settings;
    let loud = ctx.kind == TicketKind::Cancellation;
    if s.show_title || loud {
        let style = if loud { s.title } else { s.details };
        match kot_of(ctx) {
            Some(kot) => {
                doc.row(title_of(ctx), kot, style);
            }
            None => {
                doc.text(title_of(ctx), style, Align::Centre);
            }
        }
    } else if let Some(kot) = kot_of(ctx) {
        doc.text(kot, s.details, Align::Right);
    }
    marks(doc, ctx);
    if s.separators.below_title {
        doc.separator(s.pattern);
    }
}

/// The title row, the token, the table row, the bill row.
fn classic_head(doc: &mut Document, ctx: &KitchenContext<'_>) {
    let s = ctx.settings;
    if s.show_title {
        match kot_of(ctx) {
            Some(kot) => {
                doc.row(title_of(ctx), kot, s.title);
            }
            None => {
                doc.text(title_of(ctx), s.title, Align::Centre);
            }
        }
        marks(doc, ctx);
        if s.separators.below_title {
            doc.separator(s.pattern);
        }
    }
    if let Some(token) = token_of(ctx) {
        doc.text(token, s.title, Align::Centre);
        if s.separators.below_token {
            doc.separator(s.pattern);
        }
    }
    three_across(
        doc,
        s.details,
        [
            table_of(ctx).unwrap_or_default(),
            kind_of(ctx).unwrap_or_default().to_owned(),
            time_of(ctx).unwrap_or_default().to_owned(),
        ],
    );
    aside_row(doc, ctx);
    if s.separators.below_details {
        doc.separator(s.pattern);
    }
}

/// The token as big as the paper allows, the rest small around it.
fn big_token_head(doc: &mut Document, ctx: &KitchenContext<'_>) {
    let s = ctx.settings;
    small_title(doc, ctx);
    if let Some(token) = token_of(ctx) {
        doc.text(token, BIGGEST, Align::Centre);
        if s.separators.below_token {
            doc.separator(s.pattern);
        }
    }
    three_across(
        doc,
        s.details,
        [
            table_of(ctx).unwrap_or_default(),
            kind_of(ctx).unwrap_or_default().to_owned(),
            time_of(ctx).unwrap_or_default().to_owned(),
        ],
    );
    aside_row(doc, ctx);
    if s.separators.below_details {
        doc.separator(s.pattern);
    }
}

/// The table as big as the paper allows; a parcel, with no table, shows its kind instead.
fn table_card_head(doc: &mut Document, ctx: &KitchenContext<'_>) {
    let s = ctx.settings;
    small_title(doc, ctx);
    let big = match ctx.table.filter(|_| s.show_table) {
        Some(table) => format!("TABLE {table}"),
        None => super::order_type_label(ctx.order_type).to_uppercase(),
    };
    doc.text(big, BIGGEST, Align::Centre);
    if s.separators.below_token {
        doc.separator(s.pattern);
    }
    three_across(
        doc,
        s.details,
        [
            token_of(ctx).unwrap_or_default(),
            kind_of(ctx).unwrap_or_default().to_owned(),
            time_of(ctx).unwrap_or_default().to_owned(),
        ],
    );
    aside_row(doc, ctx);
    if s.separators.below_details {
        doc.separator(s.pattern);
    }
}

/// One head row, then the dishes.
fn slip_head(doc: &mut Document, ctx: &KitchenContext<'_>) {
    let s = ctx.settings;
    if ctx.kind == TicketKind::Cancellation {
        doc.text(title_of(ctx), s.title, Align::Centre);
    }
    marks(doc, ctx);
    let left = kot_of(ctx).unwrap_or_else(|| {
        if s.show_title {
            title_of(ctx).to_owned()
        } else {
            String::new()
        }
    });
    let middle = table_of(ctx)
        .or_else(|| kind_of(ctx).map(str::to_owned))
        .unwrap_or_default();
    three_across(
        doc,
        Style {
            size: s.details.size,
            bold: true,
        },
        [left, middle, time_of(ctx).unwrap_or_default().to_owned()],
    );
    let mut second = Vec::new();
    if let Some(token) = token_of(ctx) {
        second.push(token);
    }
    if table_of(ctx).is_some()
        && let Some(kind) = kind_of(ctx)
    {
        second.push(kind.to_owned());
    }
    if s.show_bill_number
        && let Some(number) = ctx.bill_number
    {
        second.push(format!("Bill {number}"));
    }
    if let Some(waiter) = ctx.waiter {
        second.push(waiter.to_owned());
    }
    if !second.is_empty() {
        doc.text(second.join("  "), s.details, Align::Left);
    }
    if s.separators.below_details {
        doc.separator(s.pattern);
    }
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
        super::gap_before(&mut rows, n, gap, columns.len());
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
        super::gap_before(&mut rows, n, gap, columns.len());
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
