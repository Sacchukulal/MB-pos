//! **The cart lives here, in Rust — and that is P09's real decision.**
//!
//! React holds no cart at all. Every change is a command that returns the whole
//! new [`CartView`], and there are three reasons, of which the third is the one
//! that matters:
//!
//! * **`Cart::add` merges lines** by `LineIdentity` — same item, same note,
//!   same modifiers. That rule was decided at P01 and lives in mb-core. A cart
//!   in TypeScript would be a second implementation of it, and the two would
//!   disagree the first time somebody adds "dosa, no onion" twice.
//! * **`compute_bill` is D4's eight-step pipeline, and it costs 14 µs** (budget
//!   B4). Recomputing the entire bill on every keystroke is free, so there is
//!   no performance argument for caching a total in React — and a cached total
//!   is a total that can be stale.
//! * **R8 stops being a rule somebody has to remember.** There is no money in
//!   TypeScript to do arithmetic *on*. `check-no-money.mjs` guards the letter;
//!   this guards the spirit.
//!
//! # Everything on this screen is a view model (D39)
//!
//! `CartView`, `TableView` and the rest are not mb-core's types. They are what
//! a screen renders: money already formatted by `Money::to_plain_string`,
//! states already turned into words, quantities already written the way a
//! shopkeeper writes them. The conversion is here, once.

use std::sync::Mutex;

use mb_core::{
    AnyOrder, Bill, BillInput, Cart, DiscountEntry, ItemSnapshot, Money, OrderType,
    PlaceOfSupply, RoundingMode, Settlement, TaxTreatment, Timestamp, compute_bill,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ipc::MoneyView;
use crate::words::{UiError, UiResult};

/// How long a table may sit before the floor is told about it.
///
/// Scope 14.2 makes this configurable and P17 owns the screen for it; until
/// then it is thirty minutes, which is roughly a South Indian lunch and short
/// enough that the state is visible during a shift rather than in theory.
pub const LATE_AFTER_MINUTES: i64 = 30;

// ---------------------------------------------------------------------------
// The cart, as the process holds it.
// ---------------------------------------------------------------------------

/// One counter's work in progress.
#[derive(Debug)]
pub struct CartState {
    pub cart: Cart,
    pub order_type: OrderType,
    /// The table this cart belongs to, when it is a dine-in order.
    pub table: Option<String>,
    /// What the cashier calls that table — "7", not "tbl_7". The id above is
    /// the key an order is saved against; this is the word on the screen.
    pub table_label: Option<String>,
    /// The order being edited, when an existing one was opened. `None` for a
    /// cart that has not been saved yet.
    pub order_id: Option<String>,
    pub settlement: Settlement,
    pub bill_discount: Option<DiscountEntry>,
    /// **Crown jewel 2 lives here while the order is being typed**, and it
    /// travels into the order when it is saved. What the kitchen was told is
    /// never a screen's memory.
    pub kitchen: mb_core::KitchenLedger,
    /// Scope 1.6 — which half of a shared table this order is (P14).
    pub sub_table: Option<mb_core::SubTable>,
    /// Scope 1.24 — how many are eating. `None` is honestly unknown.
    pub covers: Option<u32>,
    /// Scope 1.26 — a note on the whole order, printed on the bill.
    pub note: Option<String>,
    /// **Who put the first line on this bill** (P11).
    ///
    /// It becomes `orders.created_by`, while whoever is signed in when the
    /// money is taken becomes `orders.settled_by`. Those are two different
    /// people during a shift change, the schema has had both columns since P04,
    /// and this field is what finally makes them mean something.
    pub opened_by: Option<mb_core::StaffId>,
}

impl Default for CartState {
    fn default() -> Self {
        CartState {
            cart: Cart::new(),
            // Dine-in is what a restaurant counter does most of, and the
            // order-type LOCK (crown jewel 1) is what a parcel counter uses to
            // stop re-selecting it forty times an hour. The lock is the
            // screen's; the default is here.
            order_type: OrderType::DineIn,
            table: None,
            table_label: None,
            order_id: None,
            settlement: Settlement::new(),
            bill_discount: None,
            kitchen: mb_core::KitchenLedger::new(),
            sub_table: None,
            covers: None,
            note: None,
            opened_by: None,
        }
    }
}

impl CartState {
    /// Recompute from scratch. **There is no incremental path and there must
    /// not be one:** D4 fixes the order of operations, and a bill that was
    /// patched rather than recomputed is a bill nobody can reason about.
    pub fn bill(&self) -> UiResult<Bill> {
        let mut input = BillInput::new(&self.cart)
            .with_order_type(self.order_type)
            .with_place_of_supply(PlaceOfSupply::Intra)
            .with_rounding(RoundingMode::NearestRupee);
        if let Some(discount) = self.bill_discount.clone() {
            input = input.with_bill_discount(discount);
        }
        compute_bill(input).map_err(|e| {
            UiError::new(
                "bill.compute",
                "This bill could not be worked out. Nothing has been changed.",
            )
            .with_detail(e.to_string())
        })
    }
}

/// The whole cart region, in one value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CartView {
    pub lines: Vec<CartLineView>,
    pub bill: BillView,
    pub order_type: String,
    pub table: Option<String>,
    pub payments: Vec<PaymentView>,
    /// What has been taken so far.
    pub paid: MoneyView,
    /// What is still owed. Zero when the bill is covered.
    pub balance: MoneyView,
    /// What to hand back. Zero unless the customer over-paid in cash (1.16).
    pub change: MoneyView,
    pub is_empty: bool,
    /// Whether the kitchen has been told everything on this bill.
    ///
    /// **Decides what Enter on an empty box does** (audit 2.3): print the
    /// ticket, or complete the bill. It comes from the order's own ledger, so
    /// it is right after a merge and after a restart.
    pub kitchen_up_to_date: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CartLineView {
    pub index: usize,
    pub name: String,
    pub note: Option<String>,
    /// Written as a shopkeeper writes it: "2", "0.5", "1.333" (scope 1.10).
    /// `Qty` is thousandths; the formatting is mb-core's, not the screen's.
    pub qty: String,
    /// "5%", "18%", "Non-GST", "Exempt" — a label, never a number to compute
    /// with.
    pub rate_label: String,
    pub unit_price: MoneyView,
    pub gross: MoneyView,
    pub discount: MoneyView,
    /// What this line adds to the bill, tax included.
    pub amount: MoneyView,
    pub modifiers: Vec<String>,
}

/// The totals block — **a feature, not a footer.**
///
/// Audit **B11**: *"the tax report splits GST 50/50 into CGST/SGST always. No
/// IGST, no inter-state, no HSN summary, and nothing that can be filed
/// directly."* This is where a chartered accountant first sees whether the
/// product can be filed from, so it never collapses into one "GST" line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BillView {
    pub subtotal: MoneyView,
    pub line_discount: MoneyView,
    pub bill_discount: MoneyView,
    pub total_discount: MoneyView,
    /// **D15.** *"A discount that had to be capped says so; the flag reaches
    /// the bill."* It reaches `Bill`; if the screen dropped it, the flag would
    /// have travelled three phases to die on the last hop.
    pub discount_capped: bool,
    pub charges: Vec<ChargeView>,
    /// One row per rate (scope 2.7). Two rates means two rows, always.
    pub tax_rows: Vec<TaxRowView>,
    pub tax_total: MoneyView,
    /// Scope 2.3 — the liquor line that lets a bar bill at all. **Never inside
    /// a GST total.**
    pub non_gst_value: MoneyView,
    pub exempt_value: MoneyView,
    pub round_off: MoneyView,
    pub grand_total: MoneyView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ChargeView {
    pub name: String,
    pub amount: MoneyView,
    pub rate_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TaxRowView {
    /// "5%", "18%".
    pub rate_label: String,
    pub taxable: MoneyView,
    pub cgst: MoneyView,
    pub sgst: MoneyView,
    /// Zero on an intra-state bill; a row of its own when it is not (2.4).
    pub igst: MoneyView,
    pub is_interstate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PaymentView {
    pub index: usize,
    /// "Cash", "Card", "UPI", "Khata" — the label a report groups by.
    pub mode: String,
    pub amount: MoneyView,
    pub reference: Option<String>,
}

// ---------------------------------------------------------------------------
// The floor.
// ---------------------------------------------------------------------------

/// One tile in the grid — **the only view of open orders** (scope 1.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TableView {
    pub id: String,
    pub label: String,
    /// The section's name, or `None` for the "No table" group that holds open
    /// parcel and self-service orders — *"so no order is ever invisible"*.
    pub section: Option<String>,
    pub seats: i64,
    pub state: TableState,
    /// `None` when the table is free.
    pub total: Option<MoneyView>,
    /// How long it has been sitting. **Computed from the order's own
    /// timestamp**, never from a counter the screen keeps — a screen that
    /// counts loses the count when it re-renders, which is the same argument
    /// D5 makes about business days.
    pub minutes: Option<i64>,
    /// Whether the kitchen has been told (crown jewel 2's delta ledger).
    pub kitchen_told: bool,
    pub order_id: Option<String>,
}

/// **State is carried in form as well as colour** (UI_GUIDELINES §2 rule 2).
/// Grey-scale the screen and all four must still be distinguishable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "snake_case")]
pub enum TableState {
    /// Dashed outline, no fill. No colour at all.
    Free,
    /// Solid card, left stripe, amount in mono.
    Occupied,
    /// Past [`LATE_AFTER_MINUTES`]. Stripe plus an emphasised timer.
    ///
    /// §4: *"the single most useful thing a floor view can show, and no
    /// version of Magic Bill has ever had it. It is not optional."*
    Late,
    /// The order currently in the cart. Border plus a soft ring.
    Loaded,
}

/// A menu item, as the screen offers it. P13 owns the menu properly; this is
/// what the billing screen needs to put something in a cart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MenuItemView {
    pub id: String,
    pub name: String,
    pub price: MoneyView,
    pub rate_label: String,
    pub category: Option<String>,
}

// ---------------------------------------------------------------------------
// Building the views.
// ---------------------------------------------------------------------------

/// The whole cart region, from the cart and its freshly computed bill.
pub fn cart_view(state: &CartState) -> UiResult<CartView> {
    let bill = state.bill()?;
    let lines = state
        .cart
        .lines()
        .iter()
        .zip(bill.lines.iter())
        .enumerate()
        .map(|(index, (line, billed))| CartLineView {
            index,
            name: line.snapshot.name.clone(),
            note: line.note.clone(),
            qty: line.qty.to_string(),
            rate_label: rate_label(billed.rate, billed.treatment),
            unit_price: line.snapshot.unit_price.into(),
            gross: billed.gross.into(),
            discount: billed
                .line_discount
                .add(billed.bill_discount_share)
                .unwrap_or(Money::ZERO)
                .into(),
            amount: billed.gross_including_tax.into(),
            modifiers: line.modifiers.iter().map(|m| m.name.clone()).collect(),
        })
        .collect();

    let paid = state.settlement.total_paid().map_err(money_error)?;
    // **`balance`, not `amount_due`.** `amount_due` is what the bill ASKS for
    // (the total plus any tip); `balance` is what is LEFT after what has been
    // taken. Getting these the wrong way round showed "Balance 336.00" beside
    // "Cash 336.00" — found by paying a bill and looking at it.
    let due = state
        .settlement
        .balance(bill.grand_total)
        .map_err(money_error)?;
    let change = state
        .settlement
        .change_due(bill.grand_total)
        .map_err(money_error)?;

    Ok(CartView {
        lines,
        bill: bill_view(&bill)?,
        order_type: order_type_label(state.order_type).to_owned(),
        table: state.table_label.clone().or_else(|| state.table.clone()),
        payments: state
            .settlement
            .payments()
            .iter()
            .enumerate()
            .map(|(index, p)| PaymentView {
                index,
                mode: p.mode.report_label().to_owned(),
                amount: p.amount.into(),
                reference: p.reference.clone(),
            })
            .collect(),
        paid: paid.into(),
        balance: due.into(),
        change: change.into(),
        is_empty: state.cart.is_empty(),
        kitchen_up_to_date: state
            .kitchen
            .pending(&state.cart)
            .is_ok_and(|pending| pending.is_empty()),
    })
}

fn bill_view(bill: &Bill) -> UiResult<BillView> {
    let tax_rows = bill
        .summary
        .rows()
        .map(|row| TaxRowView {
            rate_label: row.rate.label(),
            taxable: row.taxable.into(),
            cgst: row.tax.cgst.into(),
            sgst: row.tax.sgst.into(),
            igst: row.tax.igst.into(),
            is_interstate: row.tax.igst.paise() > 0,
        })
        .collect();

    Ok(BillView {
        subtotal: bill.subtotal.into(),
        line_discount: bill.total_line_discount.into(),
        bill_discount: bill.total_bill_discount.into(),
        total_discount: bill.total_discount.into(),
        discount_capped: bill.bill_discount_capped,
        charges: bill
            .charges
            .iter()
            .map(|charge| ChargeView {
                name: charge.name.clone(),
                amount: charge.gross_including_tax.into(),
                rate_label: rate_label(charge.rate, charge.treatment),
            })
            .collect(),
        tax_rows,
        tax_total: bill.tax_total().map_err(money_error)?.into(),
        non_gst_value: bill.non_gst_value.into(),
        exempt_value: bill.exempt_value.into(),
        round_off: bill.round_off.into(),
        grand_total: bill.grand_total.into(),
    })
}

/// What a rate is called on screen. A **label**, never a number to compute
/// with — R8, and the treatments do not have a percentage at all.
fn rate_label(rate: mb_core::TaxRate, treatment: TaxTreatment) -> String {
    match treatment {
        TaxTreatment::NonGst => "Non-GST".to_owned(),
        TaxTreatment::Exempt => "Exempt".to_owned(),
        TaxTreatment::Inclusive => format!("{} incl.", rate.label()),
        TaxTreatment::Exclusive => rate.label(),
    }
}

pub const fn order_type_label(kind: OrderType) -> &'static str {
    match kind {
        OrderType::DineIn => "Dine in",
        OrderType::Parcel => "Parcel",
        OrderType::SelfService => "Self service",
        OrderType::Delivery => "Delivery",
    }
}

pub fn order_type_from_label(label: &str) -> Option<OrderType> {
    match label {
        "Dine in" => Some(OrderType::DineIn),
        "Parcel" => Some(OrderType::Parcel),
        "Self service" => Some(OrderType::SelfService),
        "Delivery" => Some(OrderType::Delivery),
        _ => None,
    }
}

fn money_error(e: impl std::fmt::Display) -> UiError {
    UiError::new(
        "bill.money",
        "A figure on this bill could not be worked out. Nothing has been changed.",
    )
    .with_detail(e.to_string())
}

/// Build the floor: every table, plus the open orders that have no table.
///
/// Takes the open orders and the tables and joins them **here**, because the
/// join is a screen concern — a table with no order is a tile, and an order
/// with no table is also a tile, and neither of those is a row in a database.
pub fn floor_view(
    tables: &[mb_db::repo::floor::DiningTable],
    sections: &[mb_db::repo::floor::Section],
    open: &[AnyOrder],
    loaded_order: Option<&str>,
    now: Timestamp,
) -> Vec<TableView> {
    let mut out = Vec::with_capacity(tables.len() + open.len());

    for table in tables.iter().filter(|t| t.is_active) {
        let section = table.section_id.as_ref().and_then(|id| {
            sections
                .iter()
                .find(|s| &s.id == id)
                .map(|s| s.name.clone())
        });
        let order = open.iter().find(|o| {
            o.core()
                .table
                .as_ref()
                .is_some_and(|t| t.as_str() == table.id.as_str())
        });

        out.push(match order {
            Some(order) => tile_for(order, table.label.clone(), section, table.seats, loaded_order, now),
            None => TableView {
                id: table.id.as_str().to_owned(),
                label: table.label.clone(),
                section,
                seats: table.seats,
                state: TableState::Free,
                total: None,
                minutes: None,
                kitchen_told: false,
                order_id: None,
            },
        });
    }

    // The "No table" group — parcel and self-service orders, at the end.
    // §4: *"so no order is ever invisible."* T4 exists because this is exactly
    // the kind of order that goes missing.
    for order in open.iter().filter(|o| o.core().table.is_none()) {
        // No token accessor on AnyOrder (a draft has none), so a tile with no
        // table is labelled by its order type. P10 shows the token once an
        // order has been opened and numbered.
        let label = order_type_label(order.core().order_type).to_owned();
        out.push(tile_for(order, label, None, 0, loaded_order, now));
    }

    out
}

fn tile_for(
    order: &AnyOrder,
    label: String,
    section: Option<String>,
    seats: i64,
    loaded_order: Option<&str>,
    now: Timestamp,
) -> TableView {
    let core = order.core();
    let id = core.id.as_str().to_owned();
    let minutes = (now.millis() - core.created_at.millis()).div_euclid(60_000).max(0);
    let loaded = loaded_order == Some(id.as_str());

    TableView {
        state: if loaded {
            TableState::Loaded
        } else if minutes >= LATE_AFTER_MINUTES {
            TableState::Late
        } else {
            TableState::Occupied
        },
        total: running_total(order),
        minutes: Some(minutes),
                // The delta ledger (crown jewel 2) answers "is there anything the
        // kitchen has not been told?". An error reading it is not a reason to
        // claim the kitchen is up to date, so it reads as "not told".
        kitchen_told: core
            .kitchen
            .pending(&core.cart)
            .is_ok_and(|pending| pending.is_empty()),
        order_id: Some(id.clone()),
        id: core
            .table
            .as_ref()
            .map_or(id, |t| t.as_str().to_owned()),
        label,
        section,
        seats,
    }
}

/// What the tile shows as the running total.
///
/// Recomputed from the cart rather than stored: an open order has no bill yet,
/// and inventing one on the tile would be a second money path (R2).
fn running_total(order: &AnyOrder) -> Option<MoneyView> {
    let core = order.core();
    compute_bill(
        BillInput::new(&core.cart)
            .with_order_type(core.order_type)
            .with_place_of_supply(PlaceOfSupply::Intra)
            .with_rounding(RoundingMode::NearestRupee),
    )
    .ok()
    .map(|bill| bill.grand_total.into())
}

/// A menu item, from a row.
pub fn menu_view(item: &mb_db::repo::menu::MenuItem) -> MenuItemView {
    MenuItemView {
        id: item.id.as_str().to_owned(),
        name: item.name.clone(),
        price: item.unit_price.into(),
        rate_label: rate_label(item.tax_rate, item.tax_treatment),
        category: item.category_id.as_ref().map(|c| c.as_str().to_owned()),
    }
}

/// Turn a menu row into the snapshot a cart line is frozen from.
///
/// **Crown jewel 4** — *"frozen item snapshots on every order; old bills never
/// change when you change a price."* The snapshot is taken here, at the moment
/// of adding, and never looked up again.
pub fn snapshot_for(item: &mb_db::repo::menu::MenuItem) -> ItemSnapshot {
    let mut snapshot = ItemSnapshot::new(
        item.id.clone(),
        item.name.clone(),
        item.unit_price,
        item.tax_rate,
    )
    .with_treatment(item.tax_treatment);
    if let Some(hsn) = item.hsn.clone() {
        snapshot = snapshot.with_hsn(hsn);
    }
    if let Some(category) = item.category_id.clone() {
        snapshot = snapshot.with_category(category);
    }
    snapshot
}

/// The cart, held for the life of the process.
pub type Cart_ = Mutex<CartState>;


// ---------------------------------------------------------------------------
// Turning a cart into an order (P10).
// ---------------------------------------------------------------------------

/// The terminal this counter is. One until P27 builds the second.
pub const TERMINAL: &str = "terminal_default";

impl CartState {
    /// Build the draft this cart represents.
    ///
    /// **Crown jewel 4 is already satisfied** by the time this runs: each line
    /// was frozen from an `ItemSnapshot` when it was added, so an order carries
    /// what the menu said *then* and *"old bills never change when you change a
    /// price."*
    /// Enough of an order for the kitchen templates to read — the cart, the
    /// type and the ledger.
    ///
    /// P12's cancellation slip needs an `OrderCore` to look line names up in,
    /// and the cart being mid-edit is exactly when it needs one. Nothing here
    /// is saved; the id and the times are placeholders, and `to_draft` is still
    /// the only thing that builds an order that reaches the disk.
    pub fn to_core_for_printing(&self) -> mb_core::OrderCore {
        mb_core::OrderCore {
            id: mb_core::OrderId::new(self.order_id.clone().unwrap_or_else(|| "unsaved".to_owned())),
            business_day: mb_core::BusinessDay::from_days_since_epoch(0),
            created_at: Timestamp::from_millis(0),
            order_type: self.order_type,
            table: self.table.clone().map(mb_core::TableId::new),
            sub_table: self.sub_table.clone(),
            covers: self.covers,
            cart: self.cart.clone(),
            created_by: self
                .opened_by
                .clone()
                .unwrap_or_else(|| mb_core::StaffId::new("unknown")),
            note: self.note.clone(),
            kitchen: self.kitchen.clone(),
        }
    }

    /// `by` is **who opened it**, not who is signed in now — the caller passes
    /// `opened_by` and falls back to the current person only for a cart that
    /// somehow has neither. See `flows::complete_bill`, which is the only
    /// caller, and P11 item 8 for why the two differ.
    pub fn to_draft(&self, at: Timestamp, by: mb_core::StaffId) -> UiResult<mb_core::DraftOrder> {
        let day = mb_core::BusinessDay::of(at, mb_core::DayRule::default(), mb_core::UtcOffset::INDIA);
        let id = mb_core::OrderId::new(self.order_id.clone().unwrap_or_else(|| {
            // A new order gets an id from the clock plus the terminal. D13 says
            // ids are text because two terminals collide on integers, and this
            // is that rule honoured rather than restated.
            format!("ord_{}_{}", at.millis(), TERMINAL)
        }));

        let mut draft = mb_core::DraftOrder::new(id, day, at, self.order_type, by);
        if let Some(table) = self.table.as_ref() {
            draft = draft.on_table(mb_core::TableId::new(table.clone()));
        }
        draft.core.cart = self.cart.clone();
        draft.core.kitchen = self.kitchen.clone();
        draft.core.note = self.note.clone();
        Ok(draft)
    }
}

/// What the kitchen has not been told about yet — **crown jewel 2.**
///
/// > *"The delta KOT. Only what the kitchen has not seen gets printed, and what
/// > was printed is remembered **in the database**, not in the screen's
/// > memory."*
///
/// The ledger travels with the order, so this is right after a merge, after a
/// restart, and on a second terminal. A screen-held delta would be wrong on all
/// three.
pub fn pending_for_kitchen(state: &CartState) -> UiResult<Vec<(mb_core::LineIdentity, mb_core::Qty)>> {
    state.cart_pending()
}

impl CartState {
    pub fn cart_pending(&self) -> UiResult<Vec<(mb_core::LineIdentity, mb_core::Qty)>> {
        self.kitchen.pending(&self.cart).map_err(|e| {
            UiError::new(
                "kitchen.delta",
                "What the kitchen still needs could not be worked out. Nothing has been sent.",
            )
            .with_detail(e.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_core::{ItemId, Qty, TaxRate};

    fn item(id: &str, name: &str, paise: i64, rate: TaxRate, treatment: TaxTreatment) -> ItemSnapshot {
        ItemSnapshot::new(ItemId::new(id), name, Money::from_paise(paise), rate)
            .with_treatment(treatment)
    }

    fn one() -> Qty {
        Qty::from_whole(1).expect("qty")
    }

    /// **T9 — the cart is in Rust, and the merge rule is mb-core's.**
    ///
    /// Adding the same item twice, with the same note, is ONE line of quantity
    /// two. A cart in TypeScript would have re-implemented this and the two
    /// would have disagreed the first time somebody pressed dosa twice.
    #[test]
    fn adding_the_same_item_twice_merges_into_one_line() {
        let mut state = CartState::default();
        let dosa = item("itm_dosa", "Masala Dosa", 12_000, TaxRate::GST_5, TaxTreatment::Exclusive);
        state.cart.add(dosa.clone(), one(), None, vec![]).expect("add");
        state.cart.add(dosa, one(), None, vec![]).expect("add");

        let view = cart_view(&state).expect("view");
        assert_eq!(view.lines.len(), 1, "two presses of one item are one line");
        assert_eq!(view.lines[0].qty, "2");
    }

    /// A different note is a different line — the identity includes it, so
    /// "dosa" and "dosa, no onion" go to the kitchen as two things.
    #[test]
    fn a_different_note_is_a_different_line() {
        let mut state = CartState::default();
        let dosa = item("itm_dosa", "Masala Dosa", 12_000, TaxRate::GST_5, TaxTreatment::Exclusive);
        state.cart.add(dosa.clone(), one(), None, vec![]).expect("add");
        state
            .cart
            .add(dosa, one(), Some("no onion".to_owned()), vec![])
            .expect("add");
        assert_eq!(cart_view(&state).expect("view").lines.len(), 2);
    }

    /// **T5 — the totals block never collapses** (audit B10 and B11).
    ///
    /// A bill with 5%, 18% and non-GST on it shows **two rate rows and a
    /// separate non-GST value**. v1 showed one GST figure split 50/50 with no
    /// rates at all, which is why a chartered accountant could not file from it.
    #[test]
    fn a_mixed_rate_bill_keeps_every_rate_apart() {
        let mut state = CartState::default();
        state
            .cart
            .add(
                item("itm_dosa", "Masala Dosa", 12_000, TaxRate::GST_5, TaxTreatment::Exclusive),
                one(),
                None,
                vec![],
            )
            .expect("add");
        state
            .cart
            .add(
                item("itm_cola", "Cola", 4_000, TaxRate::GST_18, TaxTreatment::Exclusive),
                one(),
                None,
                vec![],
            )
            .expect("add");
        state
            .cart
            .add(
                item("itm_beer", "Beer", 22_000, TaxRate::ZERO, TaxTreatment::NonGst),
                one(),
                None,
                vec![],
            )
            .expect("add");

        let view = cart_view(&state).expect("view");
        assert_eq!(view.bill.tax_rows.len(), 2, "two rates means two rows, always");
        assert!(view.bill.tax_rows.iter().any(|r| r.rate_label == "5%"));
        assert!(view.bill.tax_rows.iter().any(|r| r.rate_label == "18%"));

        // Scope 2.3: the bar line, and it is NEVER inside a GST total.
        assert_eq!(view.bill.non_gst_value.paise, 22_000);
        for row in &view.bill.tax_rows {
            assert!(row.taxable.paise < 22_000, "alcohol leaked into a GST row");
        }
    }

    /// **Every figure the screen shows came from the bill Rust computed.**
    ///
    /// R8, checked rather than asserted: the view's grand total is the `Bill`'s
    /// grand total, to the paisa, and its text is what `Money` formatted.
    #[test]
    fn every_figure_is_the_cores_figure() {
        let mut state = CartState::default();
        state
            .cart
            .add(
                item("itm_pbm", "Paneer Butter Masala", 31_500, TaxRate::GST_5, TaxTreatment::Exclusive),
                one(),
                None,
                vec![],
            )
            .expect("add");

        let bill = state.bill().expect("bill");
        let view = cart_view(&state).expect("view");

        assert_eq!(view.bill.grand_total.paise, bill.grand_total.paise());
        assert_eq!(view.bill.grand_total.text, bill.grand_total.to_plain_string());
        assert_eq!(view.bill.subtotal.paise, bill.subtotal.paise());
        assert_eq!(view.lines[0].amount.paise, bill.lines[0].gross_including_tax.paise());
    }

    /// **Balance is what is LEFT, not what the bill asks for.**
    ///
    /// The regression this pins was found by paying a bill and looking at it:
    /// the panel said "Cash 336.00" and "Balance 336.00" at the same time,
    /// because both this view and `complete_bill` had called `amount_due` —
    /// which is the total plus tip and never falls as money comes in. The same
    /// mistake made Complete bill refuse every bill on earth.
    ///
    /// Every assertion below is wrong under `amount_due` and right under
    /// `balance`, which is the only reason to have it.
    #[test]
    fn paying_the_bill_brings_the_balance_down_to_nothing() {
        let mut state = CartState::default();
        state
            .cart
            .add(
                item("itm_dosa", "Masala Dosa", 10_000, TaxRate::GST_5, TaxTreatment::Exclusive),
                one(),
                None,
                vec![],
            )
            .expect("add");

        // 100.00 @ 5% exclusive = 105.00.
        let total = state.bill().expect("bill").grand_total;
        assert_eq!(total.paise(), 10_500);
        assert_eq!(cart_view(&state).expect("view").balance.paise, 10_500);

        // Part of it: still owed, and the panel must say how much.
        state
            .settlement
            .add(
                mb_core::Payment::new(mb_core::PaymentMode::Cash, Money::from_paise(5_000))
                    .expect("payment"),
            )
            .expect("add");
        let view = cart_view(&state).expect("view");
        assert_eq!(view.paid.paise, 5_000);
        assert_eq!(view.balance.paise, 5_500, "half paid is not paid");

        // And the rest: nothing left, and nothing to hand back.
        state
            .settlement
            .add(
                mb_core::Payment::new(mb_core::PaymentMode::Cash, Money::from_paise(5_500))
                    .expect("payment"),
            )
            .expect("add");
        let view = cart_view(&state).expect("view");
        assert_eq!(view.balance.paise, 0, "the bill is paid in full");
        assert_eq!(view.change.paise, 0);
        assert!(state.settlement.is_settled(total).expect("settled"));
    }

    /// **T7 — fractional quantity** (scope 1.10). `Qty` is thousandths; the
    /// screen shows what mb-core formatted, never its own rounding.
    #[test]
    fn a_fractional_quantity_reaches_the_screen_intact() {
        let mut state = CartState::default();
        state
            .cart
            .add(
                item("itm_sweet", "Kaju Katli", 90_000, TaxRate::GST_5, TaxTreatment::Exclusive),
                Qty::parse("0.5").expect("half a kilo"),
                None,
                vec![],
            )
            .expect("add");
        assert_eq!(cart_view(&state).expect("view").lines[0].qty, "0.5");
    }

    /// A rate is a **label**, and the treatments do not have a percentage at
    /// all — which is why nothing on the screen tries to compute with one.
    #[test]
    fn a_rate_is_a_label_not_a_number() {
        assert_eq!(rate_label(TaxRate::GST_5, TaxTreatment::Exclusive), "5%");
        assert_eq!(rate_label(TaxRate::GST_18, TaxTreatment::Inclusive), "18% incl.");
        assert_eq!(rate_label(TaxRate::ZERO, TaxTreatment::NonGst), "Non-GST");
        assert_eq!(rate_label(TaxRate::ZERO, TaxTreatment::Exempt), "Exempt");
    }

    /// The order type survives a round trip through the label, because the
    /// screen sends back what it was given.
    #[test]
    fn every_order_type_round_trips_through_its_label() {
        for kind in [
            OrderType::DineIn,
            OrderType::Parcel,
            OrderType::SelfService,
            OrderType::Delivery,
        ] {
            assert_eq!(order_type_from_label(order_type_label(kind)), Some(kind));
        }
    }
}
