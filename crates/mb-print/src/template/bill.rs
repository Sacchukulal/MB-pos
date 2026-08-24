//! The bill — **the only place in the product that knows what a receipt looks
//! like.**
//!
//! Adding a field to a receipt is a change to this file and to nothing else.
//! That sentence is the whole point of the crate, and if it ever stops being
//! true, audit D1 has come back.
//!
//! No arithmetic happens here. If the template needs a number the [`Bill`] does
//! not carry, the `Bill` is wrong — a renderer that computes is a second money
//! path, and there is exactly one (R2, D2).
//!
//! # The 2026-08-23 redesign — P32
//!
//! The owner photographed a real bill. One dosa took **117 mm of print, of
//! which 26 % was separator rules and only 1.75 % was ink**, and the item line
//! read `105.00` above a subtotal of `100.00`. Three things changed here:
//!
//! * **The meta block is two rows, not five.** `Bill No`, `Date`, `Type`,
//!   `Table` and `Cashier` were five full-width label/amount rows; they are two
//!   three-column rows now, and they carry the **time** as well.
//! * **The item Amount column is the amount before tax**, so the column adds up
//!   to the subtotal exactly. The owner ruled on it. See [`totals`] for what
//!   that costs on a bill with inclusive prices, and how `Bill::gst_added`
//!   pays it.
//! * **A title.** A GST document has to say what it is, and none of them did.

use mb_core::{AnyOrder, Bill, OrderType, PlaceOfSupply, PriceBasis};
use serde::{Deserialize, Serialize};

use crate::doc::{Align, BandLine, Block, Column, Document, Style};
use crate::error::PrintError;
use crate::metrics::Metrics;
use crate::settings::{LogoPosition, QrMode, ReceiptSettings};

/// Which copy this is.
///
/// Audit D7: *"reprints and originals now go through the same code (good) — but
/// there is no mark on a reprint. A reprinted bill is indistinguishable from
/// the original, which is an obvious fraud opening."*
///
/// There is **no default**. The caller has to say, so forgetting is impossible
/// rather than merely discouraged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "copy", rename_all = "snake_case")]
pub enum Copy {
    Original,
    /// Scope 1.20 — counted, so the number means something.
    Duplicate { number: u32 },
    /// Scope 1.17.
    Voided { reason: String },
    /// **The bill a waiter carries to the table**, before anyone has paid.
    ///
    /// The owner asked for it on 2026-08-17 as a print button on the table
    /// tile, and it is how an Indian restaurant has always worked: the party
    /// asks for the bill, reads it, and *then* pays at the counter.
    ///
    /// **It has to be marked, and this is the whole reason it is a variant
    /// rather than [`Copy::Original`].** An open order prints no payment lines
    /// (there are none), so without a mark this paper is byte-for-byte a
    /// settled bill minus a section nobody counts — which is a slip a customer
    /// could reasonably hold up as proof of payment, and a shop could
    /// reasonably mistake for one. `should_kick` also refuses to open the cash
    /// drawer for anything that is not `Original`, which is the right answer
    /// here for free: no money is being taken yet.
    NotPaid,
}

/// The shop, as the bill header needs it.
///
/// Deliberately not `mb_db`'s `StoreProfile`: this crate does not depend on the
/// database, and the caller converts. That keeps a print template runnable in a
/// test with four lines of setup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    pub name: String,
    pub address: String,
    pub phone: Option<String>,
    pub gstin: Option<String>,
    pub fssai: Option<String>,
    pub state_code: Option<String>,
    pub upi_id: Option<String>,
    pub upi_merchant_name: Option<String>,
    /// Audit Part 3's third UPI field. It rides in the payload as `tn`, so the
    /// shop's bank statement says which counter a payment came from.
    pub upi_reference: Option<String>,
    pub registration: mb_core::Registration,
}

/// The customer, when there is one. Scope 2.6 — a B2B bill carries their GSTIN.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillCustomer {
    pub name: String,
    pub phone: Option<String>,
    pub gstin: Option<String>,
}

/// Scope 2.12, e-invoice, DESIGN.
///
/// Every field the IRP schema wants, carried on the model so that turning it on
/// later is a feature and not a migration. **Nothing is transmitted.** P04
/// already put the matching columns on `bills`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EInvoice {
    pub irn: Option<String>,
    pub ack_no: Option<String>,
    pub signed_qr: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BillContext<'a> {
    pub bill: &'a Bill,
    pub order: &'a AnyOrder,
    pub store: &'a Store,
    pub settings: &'a ReceiptSettings,
    pub customer: Option<&'a BillCustomer>,
    pub cashier: Option<&'a str>,
    /// **What the shop calls this table** — P32, and it is a label and never an
    /// id.
    ///
    /// The bill used to print `core.table.as_str()`, which is a `TableId`, so a
    /// real bill came out reading
    /// `Table  tbl_outlet_default_sec_mt48cjqf_h1_` — 36 of the 48 columns on
    /// 80 mm paper. The shop's own name for the table already existed
    /// (`flows::table_label`) and reached a toast and the kitchen screen and no
    /// piece of paper.
    ///
    /// `None` prints no table line at all. Falling back to the id would be
    /// exactly the bug again: a bill with no table line is honest, and a bill
    /// with a database key on it is not.
    pub table: Option<&'a str>,
    /// **The time of day, already formatted** — this crate owns no clock
    /// (D5, D19), so the caller writes `19:42` and this puts it beside the
    /// date. A restaurant bill without a time is not much of a record, and
    /// none of them had one until P32.
    pub time: Option<&'a str>,
    /// Scope 9.x — who took the order, as against who took the money. `None`
    /// prints nothing.
    pub waiter: Option<&'a str>,
    pub copy: Copy,
    pub einvoice: EInvoice,
    /// The shop's logo bytes, if it has one and the settings show it.
    pub logo: Option<Vec<u8>>,
}

/// Build the document for one bill.
///
/// # Why it takes the metrics — P32, second pass
///
/// **How many characters fit decides the SHAPE of the item table**, and only
/// [`Metrics`] knows how many fit: it depends on the shop's size, its typeface
/// and the roll. Found on the owner's own install — items set to size 10 in
/// Times New Roman leaves the dish name **three characters**, so every name
/// printed as a column of three-letter chunks and a bill came to 342 mm.
///
/// The template cannot answer "one line or two" without this, and the layout
/// cannot answer it at all: turning a four-column table into a two-line one is
/// a decision about what a bill looks like, and this file is the only place
/// that is allowed to know.
pub fn bill_document(
    metrics: &Metrics,
    ctx: &BillContext<'_>,
) -> Result<Document, PrintError> {
    let s = ctx.settings;
    let mut doc = Document::new(metrics.paper());

    header(&mut doc, ctx);
    marks(&mut doc, ctx);
    meta(&mut doc, metrics, ctx)?;
    items(&mut doc, metrics, ctx);
    totals(&mut doc, ctx);
    if s.show.tax_summary && more_than_one_rate(ctx.bill) {
        tax_summary(&mut doc, ctx);
    }
    payments(&mut doc, ctx);
    footer(&mut doc, ctx);

    Ok(doc)
}

/// The shop's own lines, in the order a letterhead reads.
///
/// One function, because they are drawn in two places now — stacked down the
/// paper, or beside the logo in a [`Block::Band`] — and two copies of this list
/// would be two letterheads that drift.
fn store_lines(ctx: &BillContext<'_>) -> Vec<BandLine> {
    let s = ctx.settings;
    let mut lines = vec![BandLine::new(
        &ctx.store.name,
        s.sections.store_name,
        Align::Centre,
    )];
    if s.show.address && !ctx.store.address.is_empty() {
        lines.push(BandLine::new(&ctx.store.address, s.sections.meta, Align::Centre));
    }
    if s.show.phone && let Some(phone) = &ctx.store.phone {
        lines.push(BandLine::new(
            format!("Ph {phone}"),
            s.sections.meta,
            Align::Centre,
        ));
    }
    if s.show.gstin && let Some(gstin) = &ctx.store.gstin {
        lines.push(BandLine::new(
            format!("GSTIN {gstin}"),
            s.sections.meta,
            Align::Centre,
        ));
    }
    if s.show.fssai && let Some(fssai) = &ctx.store.fssai {
        lines.push(BandLine::new(
            format!("FSSAI {fssai}"),
            s.sections.meta,
            Align::Centre,
        ));
    }
    lines
}

fn header(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;
    let lines = store_lines(ctx);

    match (s.logo, &ctx.logo) {
        // **The letterhead: logo on one side, the shop on the other** — P32.
        //
        // > *"if left right means the hotel name and address will cover 70% of
        // > 3 inch or 4 inch paper, remaining 30% width for logo, also logo
        // > correctly fit with that on ful size"*
        (LogoPosition::Left | LogoPosition::Right, Some(bytes)) => {
            doc.push(Block::Band {
                image: bytes.clone(),
                image_side: if s.logo == LogoPosition::Left {
                    Align::Left
                } else {
                    Align::Right
                },
                image_pct: s.logo_width_pct,
                text: lines,
            });
        }
        (LogoPosition::Top, Some(bytes)) => {
            doc.push(Block::Image {
                data: bytes.clone(),
                width_pct: s.logo_width_pct,
                align: Align::Centre,
            });
            for line in lines {
                doc.text(line.content, line.style, line.align);
            }
        }
        _ => {
            for line in lines {
                doc.text(line.content, line.style, line.align);
            }
        }
    }

    if s.separators.below_store_header {
        doc.separator(s.pattern);
    }
}

/// **What this piece of paper is not.** Audit D7, and it goes above the bill
/// rather than in a corner at the bottom.
fn marks(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;
    match &ctx.copy {
        Copy::Original => {}
        Copy::Duplicate { number } => {
            doc.text(
                format!("DUPLICATE - REPRINT #{number}"),
                Style::new(2, true),
                Align::Centre,
            );
            doc.separator(s.pattern);
        }
        Copy::Voided { reason } => {
            doc.text("*** VOIDED ***", Style::new(2, true), Align::Centre);
            doc.text(format!("Reason: {reason}"), Style::BOLD, Align::Centre);
            doc.separator(s.pattern);
        }
        /* Double height and bold, the same weight VOIDED gets, because it is
           the same KIND of fact. The second line is for the customer holding
           it — "NOT PAID" alone reads as an accusation, and the point is that
           nothing has gone wrong yet. */
        Copy::NotPaid => {
            doc.text("*** NOT PAID ***", Style::new(2, true), Align::Centre);
            doc.text("Please pay at the counter", Style::BOLD, Align::Centre);
            doc.separator(s.pattern);
        }
    }
}

/// **What this document is, in the words the law uses.**
///
/// A composition dealer may not collect GST and issues a *bill of supply*;
/// everybody else registered issues a *tax invoice*. A shop with no GSTIN gets
/// no title at all rather than a wrong one — printing "TAX INVOICE" over a bill
/// from an unregistered shop is a worse fault than printing nothing.
fn title_of(ctx: &BillContext<'_>) -> Option<&'static str> {
    if !ctx.settings.show.title || ctx.store.gstin.is_none() {
        return None;
    }
    ctx.store.registration.document_title()
}

/// **The narrowest paper that can hold three columns of meta.**
///
/// Below this the bill goes two-line for its items and label/value for its
/// meta — which is what every real 2-inch receipt does.
const NARROW_BELOW: usize = 40;

fn meta(
    doc: &mut Document,
    metrics: &Metrics,
    ctx: &BillContext<'_>,
) -> Result<(), PrintError> {
    let s = ctx.settings;
    let core = ctx.order.core();

    let number = ctx
        .order
        .bill_number()
        .ok_or_else(|| PrintError::invalid("a draft order has no bill number to print"))?;

    if let Some(title) = title_of(ctx) {
        doc.text(title, Style { size: s.sections.meta.size, bold: true }, Align::Centre);
    }

    // **An empty token prints no line**, which is what a bill being previewed
    // before it is an order has: the number is claimed when the order is
    // parked, and a `TOKEN -` placeholder would be a number the paper will not
    // have. Found on the first real preview (P32).
    if s.show.token
        && let Some(open) = token_of(ctx.order).filter(|t| !t.is_empty())
    {
        doc.text(format!("TOKEN {open}"), s.sections.token, Align::Centre);
        if s.separators.below_token {
            doc.separator(s.pattern);
        }
    }

    let when = match (s.show.time, ctx.time) {
        (true, Some(time)) => format!("{} {time}", core.business_day),
        _ => core.business_day.to_string(),
    };
    let kind = match core.order_type {
        OrderType::DineIn => "Dine In",
        OrderType::Parcel => "Parcel",
        OrderType::SelfService => "Self Service",
        OrderType::Delivery => "Delivery",
    };
    let table = ctx.table.map(|t| format!("Table {t}"));
    let covers = match (s.show.covers, core.covers) {
        (true, Some(n)) if n > 0 => Some(format!("Covers {n}")),
        _ => None,
    };
    let cashier = match (s.show.cashier, ctx.cashier) {
        (true, Some(name)) => Some(format!("Cashier {name}")),
        _ => None,
    };
    // **Not when it is the same person** — P32, found on a real counter. A
    // one-person shop is one person, and `Order Ravi` under `Cashier Ravi` is a
    // row of paper on every bill of the day to say so twice.
    let waiter = match (s.show.waiter, ctx.waiter) {
        (true, Some(name)) if Some(name) != ctx.cashier.filter(|_| s.show.cashier) => {
            Some(format!("Order {name}"))
        }
        _ => None,
    };

    if narrow_table(metrics, ctx) {
        // **Two inches: three rows, and no word spent on a label.**
        //
        // Five full-width `label ... value` rows do not fit a roll this narrow
        // once the date carries a time as well — `Bill BIR/1207` plus
        // `2026-08-03 19:42` is thirty characters of twenty-nine. So the number
        // goes with the time, and the date goes with the type and the table:
        // nothing is lost and nothing wraps.
        doc.row(
            &number.formatted,
            ctx.time.filter(|_| s.show.time).unwrap_or(""),
            s.sections.meta,
        );
        let mut second = vec![core.business_day.to_string(), kind.to_owned()];
        second.extend(table.clone());
        second.extend(covers.clone());
        doc.text(second.join("  "), s.sections.meta, Align::Left);
        if waiter.is_some() || cashier.is_some() {
            doc.row(
                waiter.clone().unwrap_or_default(),
                cashier.clone().unwrap_or_default(),
                s.sections.meta,
            );
        }
    } else {
        // **Three columns, two rows.** This was five full-width rows and it is
        // the single biggest saving on the bill after the separators: 5 rows of
        // paper became 2.
        let columns = vec![
            Column::fill(Align::Left),
            Column::fixed(WHEN_COLUMNS, Align::Centre),
            Column::fill(Align::Right),
        ];
        let mut rows = vec![vec![format!("Bill {}", number.formatted), when, kind.to_owned()]];
        let second = vec![
            table.unwrap_or_default(),
            covers.unwrap_or_default(),
            cashier.or(waiter.clone()).unwrap_or_default(),
        ];
        if second.iter().any(|cell| !cell.is_empty()) {
            rows.push(second);
        }
        // A waiter as well as a cashier needs a third cell; it is rare enough
        // that it earns its own row rather than a fourth column on every bill.
        if let Some(waiter) = waiter.filter(|_| ctx.cashier.is_some() && s.show.cashier) {
            rows.push(vec![waiter, String::new(), String::new()]);
        }
        doc.push(Block::Columns {
            columns,
            rows,
            style: s.sections.meta,
        });
    }

    if let Some(customer) = ctx.customer {
        doc.row("Customer", &customer.name, s.sections.meta);
        // Scope 2.6. A B2B bill without the buyer's GSTIN is not a B2B bill.
        if let Some(gstin) = &customer.gstin {
            doc.row("Cust GSTIN", gstin, s.sections.meta);
        }
    }

    // Scope 2.4 — where the supply happened, which is what decides CGST/SGST
    // against IGST. Carried on the store since P06 and printed by nothing.
    if s.show.place_of_supply && let Some(code) = &ctx.store.state_code {
        doc.row(
            "Place of supply",
            match ctx.bill.place_of_supply {
                PlaceOfSupply::Intra => code.clone(),
                PlaceOfSupply::Inter => format!("{code} (inter-state)"),
            },
            s.sections.meta,
        );
    }

    // Scope 2.12. Printed when it exists; absent until an IRP is wired up.
    if let Some(irn) = &ctx.einvoice.irn {
        doc.row("IRN", irn, s.sections.meta);
    }

    if s.separators.below_meta {
        doc.separator(s.pattern);
    }
    Ok(())
}

/// How many characters `2026-08-23 19:42` needs, plus one of air.
const WHEN_COLUMNS: usize = 17;

fn token_of(order: &AnyOrder) -> Option<String> {
    match order {
        AnyOrder::Draft(_) => None,
        AnyOrder::Open(o) => Some(o.token.formatted.clone()),
        AnyOrder::Settled(o) => Some(o.token.formatted.clone()),
        AnyOrder::Cancelled(o) => Some(o.token.formatted.clone()),
        AnyOrder::Voided(o) => Some(o.token.formatted.clone()),
    }
}

/// **What goes in the Amount column: the value before tax** — P32, the owner's
/// ruling of 2026-08-23.
///
/// It was `gross_including_tax`, so a real bill printed
///
/// ```text
/// MASALA DOSE   1   100.00   105.00
/// Subtotal                   100.00
/// CGST                         2.50
/// SGST                         2.50
/// TOTAL                      105.00
/// ```
///
/// — a line that does not equal rate × quantity, above a subtotal that does not
/// equal the column, with the tax then added a second time. `gross` is
/// `unit price × qty` exactly, and `subtotal` is defined as the sum of it, so
/// the column adds up by construction rather than by care.
///
/// **No arithmetic here.** The figure is one the `Bill` already carries.
fn amount_of(line: &mb_core::BillLine) -> String {
    line.gross.to_plain_string()
}

/// **The fewest characters a dish name may be given before the table gives
/// way** — P32.
///
/// Ten, because that is about where a dish stops being recognisable: "Paneer
/// But" can be guessed at, "Pan" cannot. It is a **floor on the shape of the
/// table**, not on the shop's size — the size a shop picks is always used, and
/// what changes when the name will not fit is that the bill goes two-line, the
/// way every 2-inch receipt in the country already does.
///
/// This is the number whose absence produced a 342 mm bill on the owner's own
/// counter: items at size 10 in Times New Roman leave three characters for the
/// name, and "Paneer Butter Masala (Half) - Extra Spicy" became fourteen rows
/// of three letters.
const MIN_NAME: usize = 10;

fn items(doc: &mut Document, metrics: &Metrics, ctx: &BillContext<'_>) {
    if narrow_table(metrics, ctx) {
        narrow_items(doc, ctx);
    } else {
        wide_items(doc, ctx);
    }
}

/// **One line per item, or two?**
///
/// Two whenever the four-column table would leave the dish name less than
/// [`MIN_NAME`] characters — which is a measurement, not a guess about the
/// roll: the same 80 mm paper holds a four-column table at size 4 and cannot at
/// size 10.
fn narrow_table(metrics: &Metrics, ctx: &BillContext<'_>) -> bool {
    let across = metrics
        .size(ctx.settings.sections.items)
        .chars_across(metrics.dots());
    let mut fixed = QTY + RATE + AMOUNT;
    if ctx.settings.show.hsn {
        fixed += HSN;
    }
    across < fixed + MIN_NAME
}

/// The fixed columns of the item table. Named, because [`narrow_table`] has to
/// add them up and a second copy of the numbers is a second answer.
const QTY: usize = 4;
const RATE: usize = 8;
const AMOUNT: usize = 9;
const HSN: usize = 6;

/// One line per item: name, qty, rate, amount across the paper.
fn wide_items(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;
    let with_hsn = s.show.hsn;

    // **Tighter than they were.** Qty was 4 characters and rate 9 and amount
    // 10 — 23 of the paper's characters for three numbers, leaving the dish
    // name whatever was over. A quantity is rarely more than two digits and an
    // amount on a restaurant bill rarely more than six; the columns still
    // widen for anything bigger, because `fit_columns` gives the slack back to
    // the widest column and `lay_row` wraps rather than truncates.
    let mut columns = vec![Column::fill(Align::Left)];
    if with_hsn {
        columns.push(Column::fixed(HSN, Align::Left));
    }
    columns.push(Column::fixed(QTY, Align::Right));
    columns.push(Column::fixed(RATE, Align::Right));
    columns.push(Column::fixed(AMOUNT, Align::Right));

    let mut head = vec!["Item".to_owned()];
    if with_hsn {
        head.push("HSN".to_owned());
    }
    head.push("Qty".to_owned());
    head.push("Rate".to_owned());
    head.push("Amount".to_owned());

    doc.push(Block::Columns {
        columns: columns.clone(),
        rows: vec![head],
        style: s.sections.items,
    });
    if s.separators.below_column_names {
        doc.separator(s.pattern);
    }

    let gap = usize::from(s.row_height.gap());
    let mut rows = Vec::with_capacity(ctx.bill.lines.len());
    for (n, line) in ctx.bill.lines.iter().enumerate() {
        // Row height, and it is the only thing "row height" can mean on a
        // device that cannot vary leading. Between the rows, never after the
        // last one — the block below already ends with a rule.
        if n > 0 {
            for _ in 0..gap {
                rows.push(vec![String::new(); columns.len()]);
            }
        }
        let mut row = vec![line.snapshot.name.clone()];
        if with_hsn {
            row.push(line.snapshot.hsn.clone().unwrap_or_default());
        }
        row.push(line.qty.to_string());
        row.push(line.snapshot.unit_price.to_plain_string());
        row.push(amount_of(line));
        rows.push(row);

        // Scope 1.9 — a per-line note is part of the bill, under its item.
        if let Some(note) = &line.note {
            let mut note_row = vec![format!("  ({note})")];
            while note_row.len() < columns.len() {
                note_row.push(String::new());
            }
            rows.push(note_row);
        }
    }

    doc.push(Block::Columns {
        columns,
        rows,
        style: s.sections.items,
    });
    if s.separators.below_items {
        doc.separator(s.pattern);
    }
}

/// 58 mm: the name gets a whole line, then `qty x rate` and the amount share
/// the next one.
///
/// This is what a two-inch receipt looks like in every shop in the country, and
/// it is the only way a name is readable on 32 columns.
fn narrow_items(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;

    // **`below_column_names` is the rule ABOVE the items on this paper.**
    //
    // There are no column names on 32 columns, and drawing a rule "below"
    // something that is not printed once gave two rules in a row. But a bill
    // still needs the meta block separated from the food, and since P32 turned
    // `below_meta` off by default there was nothing between them at all — the
    // customer's GSTIN ran straight into the first dish. Same toggle, same
    // place on the paper, and the one it duplicated is now off.
    if s.separators.below_column_names {
        doc.separator(s.pattern);
    }

    for (n, line) in ctx.bill.lines.iter().enumerate() {
        if n > 0 {
            doc.spacer(s.row_height.gap());
        }
        let mut name = line.snapshot.name.clone();
        // Scope 2.5. On this paper the HSN rides with the name rather than
        // taking six columns off it.
        if s.show.hsn && let Some(hsn) = &line.snapshot.hsn {
            name.push_str(&format!(" [{hsn}]"));
        }
        doc.text(name, s.sections.items, Align::Left);

        if let Some(note) = &line.note {
            doc.text(format!("  ({note})"), s.sections.items, Align::Left);
        }

        doc.row(
            format!(
                "  {} x {}",
                line.qty,
                line.snapshot.unit_price.to_plain_string()
            ),
            amount_of(line),
            s.sections.items,
        );
    }

    if s.separators.below_items {
        doc.separator(s.pattern);
    }
}

/// **The totals, and they reconcile exactly.**
///
/// ```text
/// subtotal − discount + charges + gst_added + round_off = grand_total
/// ```
///
/// `gst_added` rather than `total_gst` is what makes it true on a bill that
/// mixes inclusive and exclusive prices: the tax already inside an inclusive
/// price is in the Amount column, so adding it again below would count it
/// twice. `Bill::gst_included` carries that part, printed as a memo, and
/// `mb_core` computes both — this file does no arithmetic (R2, D2).
///
/// **P33 gave the bill a second tax channel and this function does not print
/// it yet.** State VAT on alcohol now has its own `Bill::total_vat`, kept apart
/// from GST so a return can never file a beer as a supply. Adding a VAT row
/// here is a change to every piece of paper the product produces, so it belongs
/// to the one deliberate redesign pass (phase 6) rather than to a type port.
fn totals(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;
    let b = ctx.bill;
    let style = s.sections.subtotals;

    doc.row("Subtotal", b.subtotal.to_plain_string(), style);

    if !b.total_discount.is_zero() {
        doc.row(
            "Discount",
            format!("-{}", b.total_discount.to_plain_string()),
            style,
        );
        // D15: a discount that could not do what was asked says so, all the way
        // to the paper.
        if b.bill_discount_capped {
            doc.text("  (discount reduced to fit)", style, Align::Left);
        }
    }

    for charge in &b.charges {
        // **The charge before its own tax**, for the same reason the item
        // column is: whatever tax is on it is in the CGST/SGST lines below, and
        // printing the tax-in figure here would count it twice.
        doc.row(&charge.name, charge.amount.to_plain_string(), style);
    }

    let label = rate_label(b);
    if !b.gst_added.central.is_zero() {
        doc.row(format!("CGST{label}"), b.gst_added.central.to_plain_string(), style);
    }
    if !b.gst_added.state.is_zero() {
        doc.row(format!("{}{label}", b.state_tax.label()), b.gst_added.state.to_plain_string(), style);
    }
    if !b.gst_added.integrated.is_zero() {
        doc.row("IGST", b.gst_added.integrated.to_plain_string(), style);
    }
    // State VAT on liquor. Its own row, never inside a GST figure.
    if !b.vat_added.is_zero() {
        doc.row(vat_label(b), b.vat_added.into_money().to_plain_string(), style);
    }
    // The other half of the split: tax the customer has already paid inside the
    // prices above. A memo, not an addition — it is why the total still works.
    if !b.gst_included.is_zero()
        && let Ok(included) = b.gst_included.total()
    {
        doc.text(
            format!("  (includes tax {})", included.to_plain_string()),
            style,
            Align::Left,
        );
    }
    if !b.vat_included.is_zero() {
        doc.text(
            format!("  (includes VAT {})", b.vat_included.into_money().to_plain_string()),
            style,
            Align::Left,
        );
    }

    // Scope 2.3 — liquor. **Listed separately and never inside a GST total.**
    // This line is the difference between a bar being able to use this product
    // and not.
    if !b.non_gst_value.is_zero() {
        doc.row("Non-GST value", b.non_gst_value.to_plain_string(), style);
    }
    if !b.exempt_value.is_zero() {
        doc.row("Exempt value", b.exempt_value.to_plain_string(), style);
    }

    if !b.round_off.is_zero() {
        doc.row("Round off", b.round_off.to_plain_string(), style);
    }

    // The air around the one number the customer looks at. Compact says none.
    doc.spacer(s.row_height.section_gap());
    if s.separators.below_subtotals {
        doc.separator(s.pattern);
    }
    doc.row(
        "TOTAL",
        b.grand_total.to_plain_string(),
        s.sections.grand_total,
    );
    if s.separators.below_grand_total {
        doc.separator(s.pattern);
    }

    // Scope 2.9 — the total in words, which a B2B customer's accounts
    // department asks for. Off by default: it is two lines of paper on every
    // bill and a walk-in customer has never wanted it.
    if s.show.amount_in_words {
        doc.text(
            format!("Rupees {}", b.grand_total.in_words()),
            s.sections.meta,
            Align::Left,
        );
    }

    // A bill of supply without the declaration is not one. Blank falls back.
    if ctx.store.registration == mb_core::Registration::Composition {
        let note = if s.composition_note.is_empty() {
            "Composition taxable person, not eligible to collect tax on supplies"
        } else {
            &s.composition_note
        };
        doc.text(note, s.sections.meta, Align::Centre);
    }
}

/// `"VAT 20%"` when the liquor on this bill is all at one rate, else `"VAT"`.
fn vat_label(b: &Bill) -> String {
    let mut rows = b.summary.vat_rows();
    match (rows.next(), rows.next()) {
        (Some(only), None) => format!("VAT {}", only.rate.label()),
        _ => "VAT".to_owned(),
    }
}

/// `" 2.5%"` when every taxed line on this bill is at one rate, and nothing
/// when they are not.
///
/// A customer reading `CGST 2.50` cannot tell what it was charged on. A
/// customer reading `CGST 2.5%  2.50` can. On a mixed bill the rate belongs in
/// the summary block and not on the total, so this says nothing there.
fn rate_label(bill: &Bill) -> String {
    let mut rates = bill.summary.rows().filter(|r| !r.gst.is_zero());
    match (rates.next(), rates.next()) {
        (Some(row), None) => format!(" {}", row.rate.label()),
        _ => String::new(),
    }
}

fn more_than_one_rate(bill: &Bill) -> bool {
    bill.summary.rows().count() > 1
}

/// Scope 2.7, and audit B11 is why it exists.
///
/// > *"The tax report splits GST 50/50 into CGST/SGST always. No IGST, no
/// > inter-state, no HSN summary, and nothing that can be filed directly."*
///
/// A chartered accountant looks at this block first.
///
/// **Printed only when there is more than one rate on the bill** (P32). On a
/// single-rate bill every figure in it is already on the CGST/SGST lines above,
/// so it was three rows of paper repeating what was said a centimetre earlier.
/// `show.tax_summary` still turns it off entirely.
fn tax_summary(doc: &mut Document, ctx: &BillContext<'_>) {
    let b = ctx.bill;
    // A shop that may not collect GST prints no GST table, zeroes included.
    if b.summary.rows().next().is_none() || !b.registration.charges_gst() {
        return;
    }
    let s = ctx.settings;
    let inter = b.place_of_supply == PlaceOfSupply::Inter;

    doc.text("Tax summary", s.sections.subtotals, Align::Left);

    // **Narrower columns on a narrow roll** — P32. Fixed at 7/9/9 they took 25
    // of the 29 characters a 58 mm bill has, leaving four for "Taxable", which
    // printed as `Taxa` over `ble`. The layout was doing exactly what it was
    // told; the template was telling it something that could not work.
    let narrow = doc.paper.columns() < NARROW_BELOW;
    let (rate, money) = if narrow { (5, 7) } else { (7, 9) };
    let columns = vec![
        Column::fixed(rate, Align::Left),
        Column::fill(Align::Right),
        Column::fixed(money, Align::Right),
        Column::fixed(money, Align::Right),
    ];
    let mut rows = vec![vec![
        "Rate".to_owned(),
        "Taxable".to_owned(),
        if inter { "IGST" } else { "CGST" }.to_owned(),
        if inter { "" } else { "SGST" }.to_owned(),
    ]];
    for row in b.summary.rows() {
        rows.push(vec![
            row.rate.label(),
            row.taxable.to_plain_string(),
            if inter {
                row.gst.integrated.to_plain_string()
            } else {
                row.gst.central.to_plain_string()
            },
            if inter {
                String::new()
            } else {
                row.gst.state.to_plain_string()
            },
        ]);
    }
    doc.push(Block::Columns {
        columns,
        rows,
        style: s.sections.subtotals,
    });
    // **A toggle in front of it** — P32. This was a bare `doc.separator`, so a
    // shop could turn every rule off and still get this one. A rule nobody can
    // switch off is the same fault as a setting nobody reads.
    if s.separators.below_tax_summary {
        doc.separator(s.pattern);
    }
}

/// Scope 1.15. Audit B9: v1 was one bill, one payment mode, *"and today you
/// must lie about it."*
fn payments(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;
    if !s.show.payment_lines {
        return;
    }
    let Some(settlement) = settlement_of(ctx.order) else {
        return;
    };

    for payment in settlement.payments() {
        doc.row(
            payment.mode.report_label(),
            payment.amount.to_plain_string(),
            s.sections.subtotals,
        );
    }
    if !settlement.tip().is_zero() {
        doc.row("Tip", settlement.tip().to_plain_string(), s.sections.subtotals);
    }
    if let Ok(change) = settlement.change_due(ctx.bill.grand_total)
        && !change.is_zero()
    {
        doc.row("Change", change.to_plain_string(), s.sections.subtotals);
    }
    // The second rule with no toggle in front of it — see `tax_summary`.
    if s.separators.below_payments {
        doc.separator(s.pattern);
    }
}

fn settlement_of(order: &AnyOrder) -> Option<&mb_core::Settlement> {
    match order {
        AnyOrder::Settled(o) => Some(&o.settlement),
        AnyOrder::Voided(o) => Some(&o.settlement),
        _ => None,
    }
}

fn footer(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;

    match s.qr {
        QrMode::None => {}
        QrMode::Static | QrMode::Dynamic => {
            if let Some(upi) = &ctx.store.upi_id {
                let name = ctx
                    .store
                    .upi_merchant_name
                    .clone()
                    .unwrap_or_else(|| ctx.store.name.clone());
                let mut payload = if s.qr == QrMode::Dynamic {
                    format!(
                        "upi://pay?pa={upi}&pn={name}&am={}&cu=INR",
                        ctx.bill.grand_total.to_plain_string()
                    )
                } else {
                    format!("upi://pay?pa={upi}&pn={name}&cu=INR")
                };
                // Audit Part 3's "payment reference", and the reason it is a
                // setting at all: without it a shop's statement shows a column
                // of identical UPI credits and cannot be reconciled.
                if let Some(reference) = &ctx.store.upi_reference
                    && !reference.is_empty()
                {
                    payload.push_str(&format!("&tn={reference}"));
                }
                doc.push(Block::QrCode {
                    payload,
                    width_pct: s.qr_width_pct,
                    align: Align::Centre,
                });
            }
        }
    }

    // **P29, scope 7.6.** The bill's own number, in a form a scanner reads —
    // which is what makes "scan the bill to bring it back" possible. Below
    // the QR and above the thank-you, because it is for the shop rather than
    // for the customer.
    if s.bill_barcode && let Some(number) = ctx.order.bill_number() {
        doc.push(Block::Barcode {
            payload: number.formatted.clone(),
            human_readable: true,
            align: Align::Centre,
        });
    }

    if !s.footer.is_empty() {
        doc.text(&s.footer, s.sections.footer, Align::Centre);
    }
    // **One blank row, not four.** This was `2 + row_height.gap()`, on top of
    // the three lines the job feeds before the cut — 6 mm of roll on every bill
    // of the day, to clear a blade that was already clear.
    doc.spacer(1);
}

/// Whether a line's price already contains its tax — used by nothing here, and
/// kept as the one place the question is spelled out for a reader wondering why
/// [`amount_of`] does not have to ask it. `Bill::subtotal` is the sum of
/// `gross`, whatever the pricing basis, so the column adds up either way.
///
/// P33 split the old four-valued `TaxTreatment` into two questions, and it is
/// [`PriceBasis`] — *is the tax already inside the price?* — that this note was
/// ever about. The other half, what kind of supply a line is in law, has nothing
/// to do with what goes in the Amount column.
const _: fn(PriceBasis) -> bool = PriceBasis::is_inclusive;

// **`Copy` deliberately has no `Default`.** clippy offered to derive one and it
// is wrong to accept: a default would mean a caller could print a bill without
// saying whether it is an original, and audit D7 is exactly that — *"a
// reprinted bill is indistinguishable from the original, which is an obvious
// fraud opening."* Making the caller say is the whole mechanism.
