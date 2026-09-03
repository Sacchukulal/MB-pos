//! The bill — the only place in the product that knows what a receipt looks like.

use mb_core::{AnyOrder, Bill, PlaceOfSupply, PriceBasis};
use serde::{Deserialize, Serialize};

use crate::doc::{Align, BandLine, Block, Column, Document, Style};
use crate::error::PrintError;
use crate::metrics::Metrics;
use crate::settings::{LogoPosition, QrMode, ReceiptSettings};

/// Which copy this is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "copy", rename_all = "snake_case")]
pub enum Copy {
    Original,
    /// Counted, so the number means something.
    Duplicate {
        number: u32,
    },
    Voided {
        reason: String,
    },
    /// The bill a waiter carries to the table, before anyone has paid.
    NotPaid,
}

/// The shop, as the bill header needs it.
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
    pub upi_reference: Option<String>,
    pub registration: mb_core::Registration,
}

/// The customer, when there is one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillCustomer {
    pub name: String,
    pub phone: Option<String>,
    pub gstin: Option<String>,
}

/// 12, e-invoice, DESIGN.
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
    /// What the shop calls this table.
    pub table: Option<&'a str>,
    /// The time of day, already formatted — this crate owns no clock, so the caller writes
    /// `19:42` and this puts it beside the date.
    pub time: Option<&'a str>,
    /// X — who took the order, as against who took the money.
    pub waiter: Option<&'a str>,
    pub copy: Copy,
    pub einvoice: EInvoice,
    /// The shop's logo bytes, if it has one and the settings show it.
    pub logo: Option<Vec<u8>>,
}

/// Build the document for one bill.
pub fn bill_document(metrics: &Metrics, ctx: &BillContext<'_>) -> Result<Document, PrintError> {
    let s = ctx.settings;
    let mut doc = Document::new(metrics.paper());

    header(&mut doc, ctx);
    marks(&mut doc, ctx);
    meta(&mut doc, metrics, ctx)?;
    items(&mut doc, metrics, ctx);
    totals(&mut doc, ctx);
    // The setting decides, on a one-rate bill as much as a three-rate one — an owner who turned
    // it on wants the rate-wise table on every tax invoice, which is what a GST audit reads.
    if s.show.tax_summary {
        tax_summary(&mut doc, ctx);
    }
    payments(&mut doc, ctx);
    footer(&mut doc, ctx);

    Ok(doc)
}

/// The shop's own lines, in the order a letterhead reads.
fn store_lines(ctx: &BillContext<'_>) -> Vec<BandLine> {
    let s = ctx.settings;
    let mut lines = vec![BandLine::new(
        &ctx.store.name,
        s.sections.store_name,
        Align::Centre,
    )];
    if s.show.address && !ctx.store.address.is_empty() {
        lines.push(BandLine::new(
            &ctx.store.address,
            s.sections.meta,
            Align::Centre,
        ));
    }
    if s.show.phone
        && let Some(phone) = &ctx.store.phone
    {
        lines.push(BandLine::new(
            format!("Ph {phone}"),
            s.sections.meta,
            Align::Centre,
        ));
    }
    if s.show.gstin
        && let Some(gstin) = &ctx.store.gstin
    {
        lines.push(BandLine::new(
            format!("GSTIN {gstin}"),
            s.sections.meta,
            Align::Centre,
        ));
    }
    if s.show.fssai
        && let Some(fssai) = &ctx.store.fssai
    {
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
        // The letterhead: logo on one side, the shop on the other.
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

/// What this piece of paper is not.
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

/// What this document is, in the words the law uses.
fn title_of(ctx: &BillContext<'_>) -> Option<&'static str> {
    if !ctx.settings.show.title || ctx.store.gstin.is_none() {
        return None;
    }
    ctx.store.registration.document_title()
}

/// The narrowest paper that can hold three columns of meta.
const NARROW_BELOW: usize = 40;

fn meta(doc: &mut Document, metrics: &Metrics, ctx: &BillContext<'_>) -> Result<(), PrintError> {
    let s = ctx.settings;
    let core = ctx.order.core();

    let number = ctx
        .order
        .bill_number()
        .ok_or_else(|| PrintError::invalid("a draft order has no bill number to print"))?;

    if let Some(title) = title_of(ctx) {
        doc.text(
            title,
            Style {
                size: s.sections.meta.size,
                bold: true,
            },
            Align::Centre,
        );
    }

    // An empty token prints no line, which is what a bill being previewed before it is an order
    // has: the number is claimed when the order is parked, and a `TOKEN -` placeholder would be
    // a number the paper will not have.
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
    let kind = super::order_type_label(core.order_type());
    let table = ctx.table.map(|t| format!("Table {t}"));
    let covers = match (s.show.covers, core.covers) {
        (true, Some(n)) if n > 0 => Some(format!("Covers {n}")),
        _ => None,
    };
    let cashier = match (s.show.cashier, ctx.cashier) {
        (true, Some(name)) => Some(format!("Cashier {name}")),
        _ => None,
    };
    // Not when it is the same person.
    let waiter = match (s.show.waiter, ctx.waiter) {
        (true, Some(name)) if Some(name) != ctx.cashier.filter(|_| s.show.cashier) => {
            Some(format!("Order {name}"))
        }
        _ => None,
    };

    if narrow_table(metrics, ctx) {
        // Two inches: three rows, and no word spent on a label.
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
        // Three columns, two rows.
        let columns = vec![
            Column::fill(Align::Left),
            Column::fixed(WHEN_COLUMNS, Align::Centre),
            Column::fill(Align::Right),
        ];
        let mut rows = vec![vec![
            format!("Bill {}", number.formatted),
            when,
            kind.to_owned(),
        ]];
        let second = vec![
            table.unwrap_or_default(),
            covers.unwrap_or_default(),
            cashier.or(waiter.clone()).unwrap_or_default(),
        ];
        if second.iter().any(|cell| !cell.is_empty()) {
            rows.push(second);
        }
        // A waiter as well as a cashier needs a third cell; it is rare enough that it earns its
        // own row rather than a fourth column on every bill.
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
        // A B2B bill without the buyer's GSTIN is not a B2B bill.
        if let Some(gstin) = &customer.gstin {
            doc.row("Cust GSTIN", gstin, s.sections.meta);
        }
    }

    // Where the supply happened, which is what decides CGST/SGST against IGST.
    if s.show.place_of_supply
        && let Some(code) = &ctx.store.state_code
    {
        doc.row(
            "Place of supply",
            match ctx.bill.place_of_supply {
                PlaceOfSupply::Intra => code.clone(),
                PlaceOfSupply::Inter => format!("{code} (inter-state)"),
            },
            s.sections.meta,
        );
    }

    // Printed when it exists; absent until an IRP is wired up.
    if let Some(irn) = &ctx.einvoice.irn {
        doc.row("IRN", irn, s.sections.meta);
    }

    if s.separators.below_meta {
        doc.separator(s.pattern);
    }
    Ok(())
}

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

/// What goes in the Amount column: the value before tax.
///
/// ```text
/// MASALA DOSE   1   100.00   105.00
/// Subtotal                   100.00
/// CGST                         2.50
/// SGST                         2.50
/// TOTAL                      105.00
/// ```
fn amount_of(line: &mb_core::BillLine) -> String {
    line.gross.to_plain_string()
}

/// The fewest characters a dish name may be given before the table gives way.
const MIN_NAME: usize = 10;

fn items(doc: &mut Document, metrics: &Metrics, ctx: &BillContext<'_>) {
    if narrow_table(metrics, ctx) {
        narrow_items(doc, ctx);
    } else {
        wide_items(doc, ctx);
    }
}

/// One line per item, or two?
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

/// The fixed columns of the item table.
const QTY: usize = 4;
const RATE: usize = 8;
const AMOUNT: usize = 9;
const HSN: usize = 6;

/// One line per item: name, qty, rate, amount across the paper.
fn wide_items(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;
    let with_hsn = s.show.hsn;

    // Tighter than they were.
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
        // Row height, and it is the only thing "row height" can mean on a device that cannot
        // vary leading.
        super::gap_before(&mut rows, n, gap, columns.len());
        let mut row = vec![line.snapshot.name.clone()];
        if with_hsn {
            row.push(line.snapshot.hsn.clone().unwrap_or_default());
        }
        row.push(line.qty.to_string());
        row.push(line.snapshot.unit_price.to_plain_string());
        row.push(amount_of(line));
        rows.push(row);

        // A per-line note is part of the bill, under its item.
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

/// 58 mm: the name gets a whole line, then `qty x rate` and the amount share the next one.
fn narrow_items(doc: &mut Document, ctx: &BillContext<'_>) {
    let s = ctx.settings;

    // `below_column_names` is the rule ABOVE the items on this paper.
    if s.separators.below_column_names {
        doc.separator(s.pattern);
    }

    for (n, line) in ctx.bill.lines.iter().enumerate() {
        if n > 0 {
            doc.spacer(s.row_height.gap());
        }
        let mut name = line.snapshot.name.clone();
        // On this paper the HSN rides with the name rather than taking six columns off it.
        if s.show.hsn
            && let Some(hsn) = &line.snapshot.hsn
        {
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

/// The totals, and they reconcile exactly.
///
/// ```text
/// subtotal − discount + charges + gst_added + round_off = grand_total
/// ```
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
        // A discount that could not do what was asked says so, all the way to the paper.
        if b.bill_discount_capped {
            doc.text("  (discount reduced to fit)", style, Align::Left);
        }
    }

    for charge in &b.charges {
        // The charge before its own tax, for the same reason the item column is: whatever tax
        // is on it is in the CGST/SGST lines below, and printing the tax-in figure here would
        // count it twice.
        doc.row(&charge.name, charge.amount.to_plain_string(), style);
    }

    let label = rate_label(b);
    if !b.gst_added.central.is_zero() {
        doc.row(
            format!("CGST{label}"),
            b.gst_added.central.to_plain_string(),
            style,
        );
    }
    if !b.gst_added.state.is_zero() {
        doc.row(
            format!("{}{label}", b.state_tax.label()),
            b.gst_added.state.to_plain_string(),
            style,
        );
    }
    if !b.gst_added.integrated.is_zero() {
        doc.row("IGST", b.gst_added.integrated.to_plain_string(), style);
    }
    // State VAT on liquor.
    if !b.vat_added.is_zero() {
        doc.row(
            vat_label(b),
            b.vat_added.into_money().to_plain_string(),
            style,
        );
    }
    // The other half of the split: tax the customer has already paid inside the prices above.
    // It is still CGST and SGST, and a tax invoice has to say so by name — "includes tax 4.76"
    // was the line the owner read as "the split is missing".
    if !b.gst_included.is_zero() && b.registration.charges_gst() {
        if !b.gst_included.central.is_zero() {
            doc.row(
                format!("Incl. CGST{label}"),
                b.gst_included.central.to_plain_string(),
                style,
            );
        }
        if !b.gst_included.state.is_zero() {
            doc.row(
                format!("Incl. {}{label}", b.state_tax.label()),
                b.gst_included.state.to_plain_string(),
                style,
            );
        }
        if !b.gst_included.integrated.is_zero() {
            doc.row(
                "Incl. IGST",
                b.gst_included.integrated.to_plain_string(),
                style,
            );
        }
    }
    if !b.vat_included.is_zero() {
        doc.text(
            format!(
                "  (includes VAT {})",
                b.vat_included.into_money().to_plain_string()
            ),
            style,
            Align::Left,
        );
    }

    if !b.non_gst_value.is_zero() {
        doc.row("Non-GST value", b.non_gst_value.to_plain_string(), style);
    }
    if !b.exempt_value.is_zero() {
        doc.row("Exempt value", b.exempt_value.to_plain_string(), style);
    }

    if !b.round_off.is_zero() {
        doc.row("Round off", b.round_off.to_plain_string(), style);
    }

    // The air around the one number the customer looks at.
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

    // The total in words, which a B2B customer's accounts department asks for.
    if s.show.amount_in_words {
        doc.text(
            format!("Rupees {}", b.grand_total.in_words()),
            s.sections.meta,
            Align::Left,
        );
    }

    // A bill of supply without the declaration is not one.
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

/// `" 2.5%"` when every taxed line on this bill is at one rate, and nothing when they are not.
fn rate_label(bill: &Bill) -> String {
    let mut rates = bill.summary.rows().filter(|r| !r.gst.is_zero());
    match (rates.next(), rates.next()) {
        (Some(row), None) => format!(" {}", row.rate.label()),
        _ => String::new(),
    }
}

fn tax_summary(doc: &mut Document, ctx: &BillContext<'_>) {
    let b = ctx.bill;
    // A shop that may not collect GST prints no GST table, zeroes included.
    if b.summary.rows().next().is_none() || !b.registration.charges_gst() {
        return;
    }
    let s = ctx.settings;
    let inter = b.place_of_supply == PlaceOfSupply::Inter;

    doc.text("Tax summary", s.sections.subtotals, Align::Left);

    // Narrower columns on a narrow roll.
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
    // A toggle in front of it.
    if s.separators.below_tax_summary {
        doc.separator(s.pattern);
    }
}

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
        doc.row(
            "Tip",
            settlement.tip().to_plain_string(),
            s.sections.subtotals,
        );
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

    if s.bill_barcode
        && let Some(number) = ctx.order.bill_number()
    {
        doc.push(Block::Barcode {
            payload: number.formatted.clone(),
            human_readable: true,
            align: Align::Centre,
        });
    }

    if !s.footer.is_empty() {
        doc.text(&s.footer, s.sections.footer, Align::Centre);
    }
    // One blank row, not four.
    doc.spacer(1);
}

/// Whether a line's price already contains its tax — used by nothing here, and kept as the one
/// place the question is spelled out for a reader wondering why `amount_of` does not have to
/// ask it.
const _: fn(PriceBasis) -> bool = PriceBasis::is_inclusive;

// `Copy` deliberately has no `Default`.
