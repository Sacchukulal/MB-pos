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

use mb_core::{AnyOrder, Bill, OrderType, PlaceOfSupply};
use serde::{Deserialize, Serialize};

use crate::doc::{Align, Block, Column, Document, Style};
use crate::error::PrintError;
use crate::paper::Paper;
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
    pub is_composition: bool,
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
    pub copy: Copy,
    pub einvoice: EInvoice,
    /// The shop's logo bytes, if it has one and the settings show it.
    pub logo: Option<Vec<u8>>,
}

/// Build the document for one bill.
pub fn bill_document(paper: Paper, ctx: &BillContext<'_>) -> Result<Document, PrintError> {
    let s = ctx.settings;
    let mut doc = Document::new(paper);

    header(&mut doc, ctx);
    meta(&mut doc, ctx)?;
    items(&mut doc, ctx);
    totals(&mut doc, ctx);
    if s.show.tax_summary {
        tax_summary(&mut doc, ctx);
    }
    payments(&mut doc, ctx);
    footer(&mut doc, ctx);

    Ok(doc)
}

fn header(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;

    if s.logo == LogoPosition::Top
        && let Some(bytes) = &ctx.logo
    {
        doc.push(Block::Image {
            data: bytes.clone(),
            width_pct: s.logo_width_pct,
            align: Align::Centre,
        });
    }

    doc.text(&ctx.store.name, s.sections.store_name, Align::Centre);
    if s.show.address && !ctx.store.address.is_empty() {
        doc.text(&ctx.store.address, s.sections.meta, Align::Centre);
    }
    if s.show.phone && let Some(phone) = &ctx.store.phone {
        doc.text(format!("Ph: {phone}"), s.sections.meta, Align::Centre);
    }
    if s.show.gstin && let Some(gstin) = &ctx.store.gstin {
        doc.text(format!("GSTIN: {gstin}"), s.sections.meta, Align::Centre);
    }
    if s.show.fssai && let Some(fssai) = &ctx.store.fssai {
        doc.text(format!("FSSAI: {fssai}"), s.sections.meta, Align::Centre);
    }
    if s.separators.below_store_header {
        doc.separator(s.pattern);
    }

    // Audit D7, and it goes ABOVE the bill, not in a corner at the bottom.
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
    }
}

fn meta(doc: &mut Document, ctx: &BillContext<'_>) -> Result<(), PrintError> {
    let s = ctx.settings;
    let core = ctx.order.core();

    let number = ctx
        .order
        .bill_number()
        .ok_or_else(|| PrintError::invalid("a draft order has no bill number to print"))?;

    doc.row("Bill No", &number.formatted, s.sections.meta);
    doc.row("Date", core.business_day.to_string(), s.sections.meta);
    doc.row(
        "Type",
        match core.order_type {
            OrderType::DineIn => "Dine In",
            OrderType::Parcel => "Parcel",
            OrderType::SelfService => "Self Service",
            OrderType::Delivery => "Delivery",
        },
        s.sections.meta,
    );
    if let Some(table) = &core.table {
        doc.row("Table", table.as_str(), s.sections.meta);
    }
    if s.show.cashier && let Some(cashier) = ctx.cashier {
        doc.row("Cashier", cashier, s.sections.meta);
    }

    if let Some(customer) = ctx.customer {
        doc.row("Customer", &customer.name, s.sections.meta);
        // Scope 2.6. A B2B bill without the buyer's GSTIN is not a B2B bill.
        if let Some(gstin) = &customer.gstin {
            doc.row("Cust GSTIN", gstin, s.sections.meta);
        }
    }

    // Scope 2.12. Printed when it exists; absent until an IRP is wired up.
    if let Some(irn) = &ctx.einvoice.irn {
        doc.row("IRN", irn, s.sections.meta);
    }

    if s.separators.below_meta {
        doc.separator(s.pattern);
    }

    if s.show.token
        && let Some(open) = token_of(ctx.order)
    {
        doc.text(format!("TOKEN {open}"), s.sections.token, Align::Centre);
        if s.separators.below_token {
            doc.separator(s.pattern);
        }
    }
    Ok(())
}

fn token_of(order: &AnyOrder) -> Option<String> {
    match order {
        AnyOrder::Draft(_) => None,
        AnyOrder::Open(o) => Some(o.token.formatted.clone()),
        AnyOrder::Settled(o) => Some(o.token.formatted.clone()),
        AnyOrder::Cancelled(o) => Some(o.token.formatted.clone()),
        AnyOrder::Voided(o) => Some(o.token.formatted.clone()),
    }
}

/// The narrowest paper on which a name, a quantity, a rate and an amount all
/// fit on one line and the name is still readable.
///
/// Below this the table goes two-line — which is what every real 2-inch receipt
/// does, and the golden file is what proved it was needed: at 58 mm with an HSN
/// column the fixed columns left **three characters** for the item name, so
/// "Paneer Butter Masala" came out as a vertical column of three-letter
/// fragments. The layout was doing exactly what it was told; the template was
/// telling it something stupid.
const NARROW_BELOW: usize = 40;

fn items(doc: &mut Document, ctx: &BillContext<'_>) {
    if doc.paper.columns() < NARROW_BELOW {
        narrow_items(doc, ctx);
    } else {
        wide_items(doc, ctx);
    }
}

/// One line per item: name, qty, rate, amount across the paper.
fn wide_items(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;
    let with_hsn = s.show.hsn;

    let mut columns = vec![Column::fill(Align::Left)];
    if with_hsn {
        columns.push(Column::fixed(6, Align::Left));
    }
    columns.push(Column::fixed(4, Align::Right)); // qty
    columns.push(Column::fixed(9, Align::Right)); // rate
    columns.push(Column::fixed(10, Align::Right)); // amount

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
        row.push(line.gross_including_tax.to_plain_string());
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

    // No `below_column_names` rule here: there are no column names on this
    // paper, and drawing a rule "below" something that is not printed gave two
    // rules in a row — which the golden file showed and nobody would have
    // noticed in code.

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
            line.gross_including_tax.to_plain_string(),
            s.sections.items,
        );
    }

    if s.separators.below_items {
        doc.separator(s.pattern);
    }
}

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
        doc.row(
            &charge.name,
            charge.gross_including_tax.to_plain_string(),
            style,
        );
    }

    if !b.total_tax.cgst.is_zero() {
        doc.row("CGST", b.total_tax.cgst.to_plain_string(), style);
    }
    if !b.total_tax.sgst.is_zero() {
        doc.row("SGST", b.total_tax.sgst.to_plain_string(), style);
    }
    if !b.total_tax.igst.is_zero() {
        doc.row("IGST", b.total_tax.igst.to_plain_string(), style);
    }

    // Scope 2.3 — liquor. **Listed separately and never inside a GST total.**
    // This line is the difference between a bar being able to use this product
    // and not.
    if !b.non_gst_value.is_zero() {
        doc.row(
            "Non-GST value",
            b.non_gst_value.to_plain_string(),
            style,
        );
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

    if ctx.store.is_composition && !s.composition_note.is_empty() {
        doc.text(&s.composition_note, s.sections.meta, Align::Centre);
    }
}

/// Scope 2.7, and audit B11 is why it exists.
///
/// > *"The tax report splits GST 50/50 into CGST/SGST always. No IGST, no
/// > inter-state, no HSN summary, and nothing that can be filed directly."*
///
/// A chartered accountant looks at this block first.
fn tax_summary(doc: &mut Document, ctx: &BillContext<'_>) {
    let b = ctx.bill;
    if b.summary.rows().next().is_none() {
        return;
    }
    let s = ctx.settings;
    let inter = b.place_of_supply == PlaceOfSupply::Inter;

    doc.text("Tax summary", s.sections.subtotals, Align::Left);

    let columns = vec![
        Column::fixed(7, Align::Left),
        Column::fill(Align::Right),
        Column::fixed(9, Align::Right),
        Column::fixed(9, Align::Right),
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
                row.tax.igst.to_plain_string()
            } else {
                row.tax.cgst.to_plain_string()
            },
            if inter {
                String::new()
            } else {
                row.tax.sgst.to_plain_string()
            },
        ]);
    }
    doc.push(Block::Columns {
        columns,
        rows,
        style: s.sections.subtotals,
    });
    doc.separator(s.pattern);
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
    doc.separator(s.pattern);
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
    doc.spacer(2 + s.row_height.gap());
}

// **`Copy` deliberately has no `Default`.** clippy offered to derive one and it
// is wrong to accept: a default would mean a caller could print a bill without
// saying whether it is an original, and audit D7 is exactly that — *"a
// reprinted bill is indistinguishable from the original, which is an obvious
// fraud opening."* Making the caller say is the whole mechanism.
//
// There is also no `treatment_label` or `rule_if` helper here any more. Both
// were written speculatively and nothing called them, which is the same
// "no column nothing reads" rule D22 applies to the schema, applied to code.
