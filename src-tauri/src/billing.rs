//! The cart lives here, in Rust.

use mb_core::{
    AnyOrder, Bill, BillInput, BusinessDay, Cart, DiscountEntry, ItemSnapshot, Money, OrderCore,
    OrderId, OrderType, Placement, Settlement, StaffId, SubTable, TableId, Timestamp, compute_bill,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ipc::MoneyView;
use crate::words::{UiError, UiResult};

// The cart, as the process holds it.

/// The table a dine-in cart sits at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSeat {
    pub id: TableId,
    /// What the cashier calls it — "7", not "tbl_7".
    pub label: String,
    /// The 6A / 6B letter, when two parties share the table.
    pub seat: Option<SubTable>,
}

/// The order this cart already is on disk. Set once, when the cart is parked or an order is
/// opened; the time and the day never change after that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub id: OrderId,
    pub created_at: Timestamp,
    pub business_day: BusinessDay,
    pub opened_by: StaffId,
    pub baseline: Option<OrderCore>,
}

/// One counter's work in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartState {
    pub cart: Cart,
    // Private, with two setters, so a table can never sit on a parcel.
    order_type: OrderType,
    table: Option<TableSeat>,
    pub origin: Option<Origin>,
    /// The token the order took when it was parked — the number the screen tracks it by.
    pub token: Option<String>,
    /// The bill number, once the bill is paid.
    pub bill_number: Option<String>,
    pub settlement: Settlement,
    pub bill_discount: Option<DiscountEntry>,
    pub kitchen: mb_core::KitchenLedger,
    /// Whose account this bill went on, when it went on one.
    pub customer: Option<String>,
    /// How many are eating.
    pub covers: Option<u32>,
    /// A note on the whole order, printed on the bill.
    pub note: Option<String>,
    /// What the floor did while the cashier had this open.
    pub from_the_floor: Vec<crate::orders::FloorChange>,
    pub account: mb_core::BillingAccount,
}

impl Default for CartState {
    fn default() -> Self {
        CartState::new_order(OrderType::DineIn)
    }
}

impl CartState {
    /// An empty cart of this type — what the counter shows after a bill, keeping the type lock.
    #[must_use]
    pub fn new_order(order_type: OrderType) -> Self {
        CartState {
            cart: Cart::new(),
            order_type,
            table: None,
            origin: None,
            token: None,
            bill_number: None,
            settlement: Settlement::new(),
            bill_discount: None,
            kitchen: mb_core::KitchenLedger::new(),
            customer: None,
            covers: None,
            note: None,
            from_the_floor: Vec::new(),
            account: mb_core::BillingAccount::default(),
        }
    }

    /// A stored order, back in the cart.
    #[must_use]
    pub fn load(order: &AnyOrder, table_label: Option<String>) -> Self {
        let core = order.core();
        CartState {
            cart: core.cart.clone(),
            order_type: core.order_type(),
            table: core.table().map(|id| TableSeat {
                id: id.clone(),
                label: table_label.unwrap_or_else(|| id.as_str().to_owned()),
                seat: core.seat().cloned(),
            }),
            token: order.token().map(|t| t.formatted.clone()),
            bill_number: order.bill_number().map(|b| b.formatted.clone()),
            origin: Some(Origin {
                id: core.id.clone(),
                created_at: core.created_at,
                business_day: core.business_day,
                opened_by: core.created_by.clone(),
                baseline: Some(core.clone()),
            }),
            settlement: core.billing.settlement.clone(),
            bill_discount: core.billing.discount.clone(),
            kitchen: core.kitchen.clone(),
            customer: None,
            covers: core.covers,
            note: core.note.clone(),
            from_the_floor: Vec::new(),
            account: core.billing.clone(),
        }
    }

    #[must_use]
    pub const fn order_type(&self) -> OrderType {
        self.order_type
    }

    #[must_use]
    pub const fn table(&self) -> Option<&TableSeat> {
        self.table.as_ref()
    }

    #[must_use]
    pub fn table_id(&self) -> Option<&str> {
        self.table.as_ref().map(|t| t.id.as_str())
    }

    /// The letter, when the cart is a second party on its table.
    #[must_use]
    pub fn seat(&self) -> Option<&str> {
        self.table
            .as_ref()
            .and_then(|t| t.seat.as_ref())
            .map(mb_core::SubTable::as_str)
    }

    #[must_use]
    pub fn order_id(&self) -> Option<&str> {
        self.origin.as_ref().map(|o| o.id.as_str())
    }

    /// Change the type. Anything but dine-in leaves the table.
    pub fn set_order_type(&mut self, order_type: OrderType) {
        self.order_type = order_type;
        if order_type != OrderType::DineIn {
            self.table = None;
        }
    }

    /// Put the cart on a table, which makes it dine-in.
    pub fn place_on(&mut self, id: TableId, label: String, seat: Option<SubTable>) {
        self.order_type = OrderType::DineIn;
        self.table = Some(TableSeat { id, label, seat });
    }

    /// Where this order is — the one place a dine-in cart with no table is refused.
    pub fn placement(&self) -> UiResult<Placement> {
        Placement::new(
            self.order_type,
            self.table.as_ref().map(|t| t.id.clone()),
            self.table.as_ref().and_then(|t| t.seat.clone()),
        )
        .map_err(|_| {
            UiError::new(
                "bill.no_table",
                "This is a dine-in order with no table. Type the table number and \
                 press Enter, or change the order type.",
            )
        })
    }

    /// The order this cart is, as it would be written. An order that already exists keeps its
    /// id, its time and its day; a new one takes the clock.
    pub fn to_core(&self, now: Timestamp, by: &StaffId, till: &str) -> UiResult<OrderCore> {
        let placement = self.placement()?;
        let (id, created_at, business_day, created_by) = match &self.origin {
            Some(origin) => (
                origin.id.clone(),
                origin.created_at,
                origin.business_day,
                origin.opened_by.clone(),
            ),
            None => (
                OrderId::new(format!("{}_{till}", crate::newid::fresh_at("ord", now))),
                now,
                crate::flows::today(now),
                by.clone(),
            ),
        };
        Ok(OrderCore {
            id,
            business_day,
            created_at,
            placement,
            covers: self.covers,
            cart: self.cart.clone(),
            created_by,
            note: self.note.clone(),
            kitchen: self.kitchen.clone(),
            billing: mb_core::BillingAccount {
                discount: self.bill_discount.clone(),
                settlement: self.settlement.clone(),
                ..self.account.clone()
            },
        })
    }

    /// The cart is now this order on disk.
    pub fn adopt(&mut self, core: &OrderCore) {
        // Clearing an unfinished entry may discard only money that was never saved.
        // A receipt parked before a failed settlement must remain paid on this counter.
        self.account = core.billing.clone();
        self.origin = Some(Origin {
            id: core.id.clone(),
            created_at: core.created_at,
            business_day: core.business_day,
            opened_by: core.created_by.clone(),
            baseline: Some(core.clone()),
        });
    }

    /// Compare the complete draft, including its receipts, with what was last read.
    pub fn has_local_changes(&self) -> UiResult<bool> {
        let Some(origin) = &self.origin else {
            return Ok(!self.cart.is_empty() || !self.settlement.payments().is_empty());
        };
        let Some(baseline) = &origin.baseline else { return Ok(true); };
        Ok(self.to_core(origin.created_at, &origin.opened_by, "")? != *baseline)
    }

    /// Rebase only independent edits. No menu lookup is allowed here: saved snapshots
    /// are the prices the guest ordered, and kitchen/receipt state is authoritative data.
    pub fn reconciled(&self, order: &AnyOrder, label: Option<String>) -> UiResult<Self> {
        let Some(origin) = &self.origin else { return Ok(self.clone()); };
        if origin.id != order.core().id { return Err(order_conflict()); }
        let Some(base) = &origin.baseline else { return Err(order_conflict()); };
        let local = self.to_core(origin.created_at, &origin.opened_by, "")?;
        let remote = order.core();
        if remote == base { return Ok(self.clone()); }
        let core = merge_order_core(base, &local, remote)?;
        let mut merged = Self::load(order, label);
        merged.cart = core.cart.clone();
        merged.order_type = core.order_type();
        merged.table = core.table().map(|id| TableSeat {
            id: id.clone(),
            label: merged.table.as_ref().map(|t| t.label.clone()).unwrap_or_else(|| id.as_str().to_owned()),
            seat: core.seat().cloned(),
        });
        merged.kitchen = core.kitchen;
        merged.covers = core.covers;
        merged.note = core.note;
        merged.bill_discount = core.billing.discount.clone();
        merged.settlement = core.billing.settlement.clone();
        merged.account = core.billing;
        // Cash typed at the counter remains an unsaved entry until park/settlement.
        // Clearing that entry must restore only the receipts actually on disk.
        merged.account.settlement = remote.billing.settlement.clone();
        merged.customer = self.customer.clone();
        merged.from_the_floor = self.from_the_floor.clone();
        // load() records the remote baseline, not the merged (still unsaved) draft.
        Ok(merged)
    }

    /// A combined bill still serves independent tables. Counter changes belong to its
    /// main table; quantities already assigned to a source table cannot be consumed.
    pub fn reconcile_service_from(&mut self, before: &CartState) -> UiResult<()> {
        let Some(service) = &before.account.service_cart else { return Ok(()); };
        if self.order_id() != before.order_id()
            || self.account.sources != before.account.sources
            || self.origin != before.origin
            || self.account.service_cart != before.account.service_cart
            || self.account.service_kitchen != before.account.service_kitchen
        { return Ok(()); }

        let invalid = || UiError::new("merge.source_items", "Only this table's items can be changed here; the other tables' items must stay on the combined bill.");
        let quantity_error = |e: mb_core::QtyError| UiError::new("cart.quantity", "That quantity is too large.").with_detail(e.to_string());
        let mut shapes: Vec<&mb_core::CartLine> = Vec::new();
        for line in before.cart.lines().iter().chain(self.cart.lines()) {
            if !shapes.iter().any(|known| same_service_line(known, line)) { shapes.push(line); }
        }
        let total = |lines: &[mb_core::CartLine], shape: &mb_core::CartLine| {
            lines.iter().filter(|line| same_service_line(line, shape))
                .try_fold(mb_core::Qty::ZERO, |sum, line| sum.add(line.qty))
                .map_err(quantity_error)
        };
        let mut lines = service.lines().to_vec();
        for shape in shapes {
            let change = total(self.cart.lines(), shape)?.sub(total(before.cart.lines(), shape)?).map_err(quantity_error)?;
            if change.is_zero() { continue; }
            let remaining = total(&lines, shape)?.add(change).map_err(quantity_error)?;
            if remaining.is_negative() { return Err(invalid()); }
            let template = lines.iter().find(|line| same_service_line(line, shape)).unwrap_or(shape).clone();
            lines.retain(|line| !same_service_line(line, shape));
            if remaining.is_positive() {
                lines.push(mb_core::CartLine { qty: remaining, ..template });
            }
        }
        let mut ledger = before.account.service_kitchen.clone().unwrap_or_default();
        let mut identities = Vec::new();
        for (identity, _) in before.kitchen.told().iter().chain(self.kitchen.told()) {
            if !identities.contains(identity) { identities.push(identity.clone()); }
        }
        for identity in identities {
            let change = self.kitchen.quantity_told(&identity).sub(before.kitchen.quantity_told(&identity)).map_err(quantity_error)?;
            let remaining = ledger.quantity_told(&identity).add(change).map_err(quantity_error)?;
            if remaining.is_negative() { return Err(invalid()); }
            ledger.set_told(&identity, remaining);
        }
        self.account.service_cart = Some(Cart::from_lines(lines).map_err(|e| UiError::new("cart.quantity", e.to_string()))?);
        self.account.service_kitchen = Some(ledger);
        Ok(())
    }

    /// Recompute from scratch. There is no incremental path and there must not be one.
    pub fn bill(&self, config: &crate::settings::ShopConfig) -> UiResult<Bill> {
        bill_for(
            &self.cart,
            self.order_type,
            self.bill_discount.clone(),
            config,
        )
    }
}

fn same_service_line(left: &mb_core::CartLine, right: &mb_core::CartLine) -> bool {
    if left.snapshot != right.snapshot || left.note != right.note { return false; }
    let mut left_modifiers = left.modifiers.clone();
    let mut right_modifiers = right.modifiers.clone();
    let key = |modifier: &mb_core::Modifier| (modifier.modifier_id.as_str().to_owned(), modifier.name.clone(), modifier.price_delta.paise());
    left_modifiers.sort_unstable_by_key(key);
    right_modifiers.sort_unstable_by_key(key);
    left_modifiers == right_modifiers
}

pub(crate) fn order_conflict() -> UiError {
    UiError::new("order.changed", "This order has conflicting changes at the counter and on another device. Your edits are still here. Undo the conflicting edit, or reload the saved order to discard your unsaved edits.")
}

/// An opaque reference to the exact line the cashier saw, including its order.
pub(crate) fn line_edit_token(state: &CartState, line: &mb_core::CartLine) -> String {
    serde_json::to_string(&(state.order_id(), line)).unwrap_or_default()
}

pub(crate) fn check_line_edit(state: &CartState, index: usize, expected: Option<&str>) -> UiResult<()> {
    if let Some(expected) = expected {
        let line = state.cart.lines().get(index).ok_or_else(order_conflict)?;
        if line_edit_token(state, line) != expected { return Err(order_conflict()); }
    }
    Ok(())
}

/// Quantity reductions keep a whole item count. Fractional quantities can still be
/// added or increased; removing the entire line is a separate, explicit operation.
pub(crate) fn validate_reduced_quantity(before: mb_core::Qty, after: mb_core::Qty) -> UiResult<()> {
    if after < before && (!after.is_positive() || after.thousandths() % mb_core::Qty::ONE.thousandths() != 0) {
        return Err(UiError::new("cart.qty_whole", "Use a whole quantity of 1 or more when reducing an item. To remove it completely, use Remove."));
    }
    Ok(())
}

fn merge_value<T: Clone + PartialEq>(base: &T, local: &T, remote: &T) -> UiResult<T> {
    if local == base || local == remote { Ok(remote.clone()) }
    else if remote == base { Ok(local.clone()) }
    else { Err(order_conflict()) }
}

fn merge_cart(base: &Cart, local: &Cart, remote: &Cart) -> UiResult<Cart> {
    if local == base { return Ok(remote.clone()); }
    if remote == base { return Ok(local.clone()); }
    let same = |a: &mb_core::CartLine, b: &mb_core::CartLine| {
        same_service_line(a, b) && a.line_discount == b.line_discount
    };
    let mut shapes = Vec::new();
    for cart in [base, local, remote] {
        for (index, line) in cart.lines().iter().enumerate() {
            // Without a stable line ID, duplicate identical rows cannot be matched safely.
            if cart.lines()[..index].iter().any(|other| same(other, line)) {
                return Err(order_conflict());
            }
            if !shapes.iter().any(|other| same(other, line)) { shapes.push(line.clone()); }
        }
    }
    let mut lines = Vec::new();
    for mut shape in shapes {
        let qty = |cart: &Cart| cart.lines().iter().find(|line| same(line, &shape)).map(|line| line.qty);
        let (before, ours, theirs) = (qty(base), qty(local), qty(remote));
        // Two simultaneous adds or quantity edits cannot be inferred from their final
        // numbers; even equal results might represent two separate additions.
        if ours != before && theirs != before { return Err(order_conflict()); }
        if let Some(quantity) = merge_value(&before, &ours, &theirs)? {
            shape.qty = quantity;
            lines.push(shape);
        }
    }
    Cart::from_lines(lines).map_err(|e| UiError::new("cart.quantity", e.to_string()))
}

fn merge_kitchen(base: &mb_core::KitchenLedger, local: &mb_core::KitchenLedger, remote: &mb_core::KitchenLedger) -> UiResult<mb_core::KitchenLedger> {
    if local == base { return Ok(remote.clone()); }
    if remote == base { return Ok(local.clone()); }
    let mut identities = Vec::new();
    for (identity, _) in base.told().iter().chain(local.told()).chain(remote.told()) {
        if !identities.contains(identity) { identities.push(identity.clone()); }
    }
    let mut merged = mb_core::KitchenLedger::new();
    for identity in identities {
        let quantity = merge_value(&base.quantity_told(&identity), &local.quantity_told(&identity), &remote.quantity_told(&identity))?;
        merged.set_told(&identity, quantity);
    }
    Ok(merged)
}

fn merge_order_core(base: &OrderCore, local: &OrderCore, remote: &OrderCore) -> UiResult<OrderCore> {
    if local == base { return Ok(remote.clone()); }
    if remote == base { return Ok(local.clone()); }
    // A concurrent combine/correction changes which bill owns money and food. It needs
    // explicit review rather than attaching this counter's edits to a different account.
    if remote.billing.revision != base.billing.revision
        || remote.billing.sources != base.billing.sources
        || remote.billing.billed_into != base.billing.billed_into
    { return Err(order_conflict()); }
    if local.billing.settlement != base.billing.settlement
        && remote.billing.settlement != base.billing.settlement
    { return Err(order_conflict()); }
    let mut merged = remote.clone();
    merged.cart = merge_cart(&base.cart, &local.cart, &remote.cart)?;
    merged.kitchen = merge_kitchen(&base.kitchen, &local.kitchen, &remote.kitchen)?;
    merged.placement = merge_value(&base.placement, &local.placement, &remote.placement)?;
    merged.covers = merge_value(&base.covers, &local.covers, &remote.covers)?;
    merged.note = merge_value(&base.note, &local.note, &remote.note)?;
    macro_rules! account_field {
        ($($field:ident),+ $(,)?) => { $(
            merged.billing.$field = merge_value(&base.billing.$field, &local.billing.$field, &remote.billing.$field)?;
        )+ };
    }
    account_field!(discount, settlement, revision, sources, source_labels, serving,
        billed_into, service_cart, service_kitchen, service_token, refund, refunds);
    // A remote kitchen send plus a local removal must not silently forget cooked food.
    let excess = merged.kitchen.over_told(&merged.cart)
        .map_err(|e| UiError::new("cart.quantity", e.to_string()))?;
    if !excess.is_empty() { return Err(order_conflict()); }
    Ok(merged)
}

/// The one way a cart becomes a bill, for the counter, the tile, the phone and the paper.
pub fn bill_for(
    cart: &Cart,
    order_type: OrderType,
    bill_discount: Option<DiscountEntry>,
    config: &crate::settings::ShopConfig,
) -> UiResult<Bill> {
    // A charge belongs to the ORDER TYPE: switching a table to a parcel drops the service
    // charge and adds the packing one. Its tax comes from the book, like an item's.
    let charges = config
        .billing
        .charges_for(order_type, &config.tax)
        .map_err(|e| {
            UiError::new(
                "bill.charge_slab",
                "A charge on this bill points at a tax slab the shop no longer has. \
                 Fix it under Settings › Tax.",
            )
            .with_detail(e.to_string())
        })?;
    let mut input = BillInput::new(cart, registration_of(config))
        .with_order_type(order_type)
        .with_rounding(config.billing.rounding)
        .with_charges(&charges);
    if let Some(discount) = bill_discount {
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
    /// What is still owed.
    pub balance: MoneyView,
    /// What to hand back.
    pub change: MoneyView,
    pub is_empty: bool,
    /// Whether the kitchen has been told everything on this bill.
    pub kitchen_up_to_date: bool,
    /// Whether the kitchen has been told ANYTHING on this order yet — which is a different
    /// question from `CartView::kitchen_up_to_date`, and the screen needs both.
    pub kitchen_told: bool,
    /// How many people are on this table.
    pub covers: Option<u32>,
    /// The order's id, once it has one.
    pub order_id: Option<String>,
    /// The token it took when it was parked, as it prints — what the screen calls it.
    pub token: Option<String>,
    /// The bill number, once the bill is paid.
    pub bill_number: Option<String>,
    /// What the floor did to this order while the cashier had it open.
    pub from_the_floor: Vec<crate::orders::FloorChange>,
    /// A very long order, mentioned rather than refused.
    pub length_says: String,
    /// The shop always bills as one order type, so the switch is not shown.
    pub order_type_locked: bool,
    /// The shop has no kitchen ticket, so its buttons are not shown.
    pub kitchen_ticket_off: bool,
    /// The billing screen shows the table grid; off, the orders being cooked take its room.
    pub tables_on_counter: bool,
    /// A beep when a phone lands an order — the shop's switch under Settings › Billing.
    pub arrival_beep: bool,
    /// The cards beat on arrival — the switch beside it.
    pub arrival_beat: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CartLineView {
    pub index: usize,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_token: Option<String>,
    pub name: String,
    pub note: Option<String>,
    /// Written as a shopkeeper writes it: "2", "0.5", "1.333".
    pub qty: String,
    /// "5%", "18%", "Non-GST", "Exempt" — a label, never a number to compute with.
    pub rate_label: String,
    pub unit_price: MoneyView,
    pub gross: MoneyView,
    /// Money off this line from both directions: its own discount and its share of the bill's.
    pub discount: MoneyView,
    /// This line's own discount alone, so the screen knows there is one to take off.
    pub line_discount: MoneyView,
    /// What this line adds to the bill, tax included.
    pub amount: MoneyView,
    pub modifiers: Vec<String>,
}

/// The totals block — a feature, not a footer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BillView {
    pub subtotal: MoneyView,
    pub line_discount: MoneyView,
    pub bill_discount: MoneyView,
    pub total_discount: MoneyView,
    /// "A discount that had to be capped says so; the flag reaches the bill." It reaches
    /// `Bill`; if the screen dropped it, the flag would have travelled three phases to die on
    /// the last hop.
    pub discount_capped: bool,
    pub charges: Vec<ChargeView>,
    /// One row per rate.
    pub tax_rows: Vec<TaxRowView>,
    pub tax_total: MoneyView,
    /// The liquor line that lets a bar bill at all.
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
    /// "Cash", "Card", "UPI", "Credit" — the label a report groups by.
    pub mode: String,
    pub amount: MoneyView,
    pub reference: Option<String>,
}

// The floor.

/// One tile in the grid — the only view of open orders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TableView {
    #[ts(optional)]
    pub billed_into: Option<String>,
    pub id: String,
    pub label: String,
    /// The section's name, or `None` for the "No table" group that holds open parcel and
    /// self-service orders — "so no order is ever invisible".
    pub section: Option<String>,
    /// Where the shop puts that room, so the billing screen groups its tiles in the SAME order
    /// the floor screen does. `None` goes last, with the "No table" group.
    pub section_order: Option<i32>,
    pub seats: u32,
    pub state: TableState,
    /// `None` when the table is free.
    pub total: Option<MoneyView>,
    /// How long it has been sitting.
    pub minutes: Option<u32>,
    /// The order's creation time in Unix milliseconds; `None` for a free table.
    #[ts(type = "number | null")]
    pub created_at: Option<i64>,
    /// Whether the kitchen has been told.
    pub kitchen_told: bool,
    /// Minutes since the last kitchen ticket went out — scope 14.2's second timer, and the one
    /// that catches a forgotten table: "food ordered 18 minutes ago and nothing since".
    pub kitchen_minutes: Option<u32>,
    /// A waiter asked for this table's bill from a phone, and it printed. The tile says so
    /// until the table is settled.
    pub bill_asked: bool,
    /// A waiter asked, from a phone, for this bill to be settled; the desk on the counter's
    /// screen has it until somebody there confirms or declines.
    pub settle_asked: bool,
    /// Who opened the order — the tile wears their colour, here and on the phones.
    pub by: Option<String>,
    pub by_id: Option<String>,
    pub order_id: Option<String>,
    /// The letter, when this tile is a second party on its table ("B" of "4B"). With no
    /// `order_id` beside it, it is the party the cart has just opened and not yet saved —
    /// pressing it re-joins the same seat.
    pub seat: Option<String>,
    /// The token this order took, formatted as it prints — the number on the tile.
    pub token: Option<String>,
    /// The bill number, once the bill is paid.
    pub bill_number: Option<String>,
    /// This is the tile the cashier is looking at — the cart is on it.
    pub selected: bool,
}

/// State is carried in form as well as colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "snake_case")]
pub enum TableState {
    /// Dashed outline, no fill.
    Free,
    /// Solid card, left stripe, amount in mono.
    Occupied,
    /// Past the WARN threshold.
    Waiting,
    /// Past the LATE threshold.
    Late,
}

// There was a fifth variant, `Loaded`, and removing it is the fix.

/// A menu item, as the screen offers it.
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

// Building the views.

/// The whole cart region, from the cart and its freshly computed bill.
/// The lines of a computed bill, as a screen lists them.
pub(crate) fn bill_lines(bill: &Bill) -> Vec<CartLineView> {
    bill.lines
        .iter()
        .enumerate()
        .map(|(index, billed)| CartLineView {
            index,
            edit_token: None,
            name: billed.snapshot.name.clone(),
            note: billed.note.clone(),
            qty: billed.qty.to_string(),
            rate_label: rate_label(billed.tax),
            unit_price: billed.snapshot.unit_price.into(),
            gross: billed.gross.into(),
            discount: billed
                .line_discount
                .add(billed.bill_discount_share)
                .unwrap_or(Money::ZERO)
                .into(),
            line_discount: billed.line_discount.into(),
            // What the line adds before its tax, so the lines add up to the Subtotal on the
            // same screen — for a tax-in price this IS the price paid.
            amount: billed.net.into(),
            modifiers: billed.modifiers.iter().map(|m| m.name.clone()).collect(),
        })
        .collect()
}

/// Every payment on a settlement, as a screen lists them.
pub(crate) fn payment_views(settlement: &Settlement) -> Vec<PaymentView> {
    settlement
        .payments()
        .iter()
        .enumerate()
        .map(|(index, p)| PaymentView {
            index,
            mode: p.mode.report_label().to_owned(),
            amount: p.amount.into(),
            reference: p.reference.clone(),
        })
        .collect()
}

pub fn cart_view(state: &CartState, config: &crate::settings::ShopConfig) -> UiResult<CartView> {
    let bill = state.bill(config)?;
    let mut lines = bill_lines(&bill);
    for (view, line) in lines.iter_mut().zip(state.cart.lines()) {
        view.edit_token = Some(line_edit_token(state, line));
    }

    let paid = state.settlement.total_paid().map_err(money_error)?;
    // `balance`, not `amount_due`. `amount_due` is what the bill ASKS for (the total plus any
    // tip); `balance` is what is LEFT after what has been taken.
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
        table: state
            .table()
            .map(|t| format!("{}{}", t.label, t.seat.as_ref().map_or("", |s| s.as_str()))),
        payments: payment_views(&state.settlement),
        paid: paid.into(),
        balance: due.into(),
        change: change.into(),
        is_empty: state.cart.is_empty(),
        kitchen_up_to_date: state
            .kitchen
            .pending(&state.cart)
            .is_ok_and(|pending| pending.is_empty()),
        kitchen_told: !state.kitchen.told().is_empty(),
        covers: state.covers,
        order_id: state.order_id().map(str::to_owned),
        token: state.token.clone(),
        bill_number: state.bill_number.clone(),
        from_the_floor: state.from_the_floor.clone(),
        length_says: state.cart.length_says().unwrap_or_default(),
        order_type_locked: config.billing.lock_order_type,
        kitchen_ticket_off: config.billing.kitchen_ticket_off,
        tables_on_counter: config.billing.tables_on_counter,
        arrival_beep: config.billing.arrival_beep,
        arrival_beat: config.billing.arrival_beat,
    })
}

/// What a fresh cart starts as: the locked type, or the one the counter was on.
#[must_use]
pub fn starting_order_type(
    config: &crate::settings::ShopConfig,
    previous: mb_core::OrderType,
) -> mb_core::OrderType {
    if config.billing.lock_order_type {
        config.billing.locked_order_type
    } else {
        previous
    }
}

pub(crate) fn bill_view(bill: &Bill) -> UiResult<BillView> {
    let tax_rows = bill
        .summary
        .rows()
        .map(|row| TaxRowView {
            rate_label: row.rate.label(),
            taxable: row.taxable.into(),
            cgst: row.gst.central.into(),
            sgst: row.gst.state.into(),
            igst: row.gst.integrated.into(),
            is_interstate: row.gst.integrated.paise() > 0,
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
                rate_label: rate_label(charge.tax),
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

/// Who this shop is, for the tax pipeline.
pub fn registration_of(config: &crate::settings::ShopConfig) -> mb_core::Registration {
    config.store.registration()
}

/// What a line's tax is called on screen.
fn rate_label(tax: mb_core::TaxSpec) -> String {
    match tax.kind {
        // A liquor line with no rate reads as it always did.
        mb_core::TaxKind::OutsideGst => {
            if tax.rate.is_zero() {
                "Non-GST".to_owned()
            } else {
                format!("VAT {}", tax.rate.label())
            }
        }
        mb_core::TaxKind::Exempt => "Exempt".to_owned(),
        mb_core::TaxKind::Untaxed => "No tax".to_owned(),
        mb_core::TaxKind::Gst => match tax.basis {
            mb_core::PriceBasis::Inclusive => format!("{} incl.", tax.rate.label()),
            mb_core::PriceBasis::Exclusive => tax.rate.label(),
        },
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
pub struct Room<'a> {
    /// Where the cashier's cart is — or `None` when the screen asking has no cart behind it.
    pub cart_is_on: Option<CartIsOn<'a>>,
    pub now: Timestamp,
    /// Both thresholds come from settings.
    pub warn_after: i64,
    pub late_after: i64,
    /// The round-off mode and the default charges a running total is computed with.
    pub config: &'a crate::settings::ShopConfig,
}

/// Which tile the cart is on, in the two ways it can be said.
pub struct CartIsOn<'a> {
    /// The saved order the cart is holding, once it has one.
    pub order: Option<&'a str>,
    /// The table the cart is on, order or no order.
    pub table: Option<&'a str>,
    /// And the letter, when it is a second party there — the table's own tile is not it.
    pub seat: Option<&'a str>,
}

pub fn floor_view(
    tables: &[mb_db::repo::floor::DiningTable],
    sections: &[mb_db::repo::floor::Section],
    open: &[AnyOrder],
    room: Room<'_>,
) -> Vec<TableView> {
    let Room {
        cart_is_on,
        now,
        warn_after,
        late_after,
        config,
    } = room;
    // Split out once. A screen with no cart marks nothing, and every comparison below is
    // against `None`, which nothing matches.
    let (loaded_order, loaded_table, loaded_seat) =
        cart_is_on.map_or((None, None, None), |cart| (cart.order, cart.table, cart.seat));
    let mut out = Vec::with_capacity(tables.len() + open.len());

    for table in tables.iter().filter(|t| t.is_active) {
        let section = room_of(table, sections);
        // Every party on this table. The one with no letter is the table's own tile; every
        // lettered one is its own tile beside it — and stays so when the first party has paid.
        let mut here: Vec<&AnyOrder> = open
            .iter()
            .filter(|o| {
                o.core()
                    .table()
                    .is_some_and(|t| t.as_str() == table.id.as_str())
            })
            .collect();
        here.sort_by_key(|o| o.core().seat().map(|s| s.as_str().to_owned()));
        let order = here.iter().find(|o| o.core().seat().is_none()).copied();
        let cart_here = loaded_table == Some(table.id.as_str());

        // Decided here, where both halves are in scope, and nowhere else. A cart on 2B is on
        // table 2, but it is not on table 2's own tile.
        let selected = (cart_here && loaded_seat.is_none())
            || order.is_some_and(|o| loaded_order == Some(o.core().id.as_str()));

        // The table's own tile first — "4" before "4B", the way the room reads.
        out.push(match order {
            Some(order) => tile_for(
                order,
                Seat {
                    label: table.label.clone(),
                    section: section.clone(),
                    seats: table.seats,
                    selected,
                    now,
                    warn_after,
                    late_after,
                    config,
                },
            ),
            None => free_tile(
                table.id.as_str(),
                table.label.clone(),
                &section,
                table.seats,
                selected,
                None,
            ),
        });

        // Then its parties, in letter order.
        for party in here.iter().filter(|o| o.core().seat().is_some()) {
            let seat = party.core().seat().map_or("", |s| s.as_str());
            out.push(tile_for(
                party,
                Seat {
                    label: format!("{}{seat}", table.label),
                    // A second party sits in the same room as the table it is beside.
                    section: section.clone(),
                    seats: 0,
                    selected: loaded_order == Some(party.core().id.as_str()),
                    now,
                    warn_after,
                    late_after,
                    config,
                },
            ));
        }

        // The party the cart has just opened here and not yet saved: "+" was pressed and no
        // ticket has gone. There is no order to draw it from, so it is drawn from where the cart
        // is — the same way a pressed free table is drawn from the table alone — and it leaves
        // the grid the moment the cart does.
        if cart_here
            && let Some(letter) = loaded_seat
            && !here
                .iter()
                .any(|o| o.core().seat().is_some_and(|s| s.as_str() == letter))
        {
            out.push(free_tile(
                table.id.as_str(),
                format!("{}{letter}", table.label),
                &section,
                0,
                true,
                Some(letter.to_owned()),
            ));
        }
    }

    // The "No table" group — parcel and self-service orders, at the end.
    for order in open.iter().filter(|o| o.core().table().is_none()) {
        // No token accessor on AnyOrder (a draft has none), so a tile with no table is labelled
        // by its order type.
        let label = order_type_label(order.core().order_type()).to_owned();
        // A parcel or self-service order has no table, so the order is the only thing there is
        // to match on.
        let selected = loaded_order == Some(order.core().id.as_str());
        out.push(tile_for(
            order,
            Seat {
                label,
                section: InRoom::default(),
                seats: 0,
                selected,
                now,
                warn_after,
                late_after,
                config,
            },
        ));
    }

    out
}

/// A tile with no order on it: a free table, or the party the cart has opened and not yet
/// saved. Free even while it is being looked at — `selected` is the ring, not a state.
fn free_tile(
    table_id: &str,
    label: String,
    section: &InRoom,
    seats: i64,
    selected: bool,
    seat: Option<String>,
) -> TableView {
    TableView {
        id: table_id.to_owned(),
        billed_into: None,
        label,
        section: section.name.clone(),
        section_order: section.order,
        seats: crate::ipc::count(seats),
        state: TableState::Free,
        selected,
        total: None,
        minutes: None,
        created_at: None,
        kitchen_told: false,
        kitchen_minutes: None,
        bill_asked: false,
        settle_asked: false,
        by: None,
        by_id: None,
        order_id: None,
        seat,
        token: None,
        bill_number: None,
    }
}

/// Where a tile sits and how long a table may sit there — the four things that describe the
/// SEAT rather than the order in it, so `tile_for` takes two arguments instead of six.
struct Seat<'a> {
    label: String,
    section: InRoom,
    seats: i64,
    /// Already decided by `floor_view` — see `TableView::selected`.
    selected: bool,
    now: Timestamp,
    warn_after: i64,
    late_after: i64,
    config: &'a crate::settings::ShopConfig,
}

fn tile_for(order: &AnyOrder, seat: Seat<'_>) -> TableView {
    let Seat {
        label,
        section,
        seats,
        selected,
        now,
        warn_after,
        late_after,
        config,
    } = seat;
    let core = order.core();
    let id = core.id.as_str().to_owned();
    let minutes = (now.millis() - core.created_at.millis())
        .div_euclid(60_000)
        .max(0);

    TableView {
        // Being selected no longer costs the table its state.
        billed_into: core.billing.billed_into.as_ref().map(|id| id.as_str().to_owned()),
        state: if minutes >= late_after {
            TableState::Late
        } else if minutes >= warn_after {
            TableState::Waiting
        } else {
            TableState::Occupied
        },
        selected,
        total: running_total(order, config),
        minutes: Some(crate::ipc::count(minutes)),
        created_at: Some(core.created_at.millis()),
        // The delta ledger answers "is there anything the kitchen has not been told?".
        kitchen_told: core
            .kitchen
            .pending(&core.cart)
            .is_ok_and(|pending| pending.is_empty()),
        // Filled in by `floor::floor_on`, which is the only caller with the events table open.
        kitchen_minutes: None,
        bill_asked: false,
        settle_asked: false,
        by: None,
        by_id: Some(core.created_by.as_str().to_owned()),
        order_id: Some(id.clone()),
        seat: core.seat().map(|s| s.as_str().to_owned()),
        token: order.token().map(|claimed| claimed.formatted.clone()),
        bill_number: order.bill_number().map(|claimed| claimed.formatted.clone()),
        // A table's tile is the table; a second party's tile, and a tile with no table, is the
        // order itself.
        id: match core.table() {
            Some(table) if core.seat().is_none() => table.as_str().to_owned(),
            _ => id,
        },
        label,
        section: section.name,
        section_order: section.order,
        seats: crate::ipc::count(seats),
    }
}

/// The room a tile sits in: its name, and where the shop puts it. The two always travel
/// together — a name carried without its place is what left the billing screen sorting rooms
/// by the alphabet while the floor screen used the shop's own order.
#[derive(Debug, Clone, Default)]
struct InRoom {
    name: Option<String>,
    order: Option<i32>,
}

/// Which room this table is in, read from the shop's own list of them.
fn room_of(
    table: &mb_db::repo::floor::DiningTable,
    sections: &[mb_db::repo::floor::Section],
) -> InRoom {
    let Some(found) = table
        .section_id
        .as_ref()
        .and_then(|id| sections.iter().find(|s| &s.id == id))
    else {
        return InRoom::default();
    };
    InRoom {
        name: Some(found.name.clone()),
        // The same narrowing `SectionView` does, so the two never disagree.
        order: Some(i32::try_from(found.sort_order).unwrap_or(0)),
    }
}

/// The facts a tile carries that only the events table and the people list know: the kitchen
/// timer, who opened the order, and what the phones asked. BOTH screens call this, so the
/// billing grid and the floor plan can never disagree about a table.
pub(crate) fn decorate(
    tiles: &mut [TableView],
    repos: &mb_db::Repos<'_>,
    at: Timestamp,
) -> Result<(), mb_db::DbError> {
    let told = repos
        .events()
        .last_for_each(mb_db::repo::events::KITCHEN_TICKET)?;
    let asked = repos
        .events()
        .last_for_each(mb_db::repo::events::BILL_ASKED)?;
    let settles = repos.events().latest_of_two(
        mb_db::repo::events::SETTLE_ASKED,
        mb_db::repo::events::SETTLE_DECLINED,
    )?;
    let people = repos.people().list_staff(crate::state::OUTLET)?;
    for tile in tiles.iter_mut() {
        let Some(order_id) = tile.order_id.clone() else {
            continue;
        };
        tile.kitchen_minutes = told
            .iter()
            .find(|(id, _)| id == &order_id)
            .map(|(_, when)| crate::ipc::count(crate::floor::minutes_between(*when, at)));
        tile.bill_asked = asked.iter().any(|(id, _)| id == &order_id);
        tile.settle_asked = settles
            .iter()
            .any(|e| e.order_id == order_id && e.event == mb_db::repo::events::SETTLE_ASKED);
        tile.by = tile
            .by_id
            .as_deref()
            .and_then(|id| people.iter().find(|p| p.id.as_str() == id))
            .map(|p| p.name.clone());
    }
    Ok(())
}

/// What the tile shows as the running total.
pub(crate) fn running_total(
    order: &AnyOrder,
    config: &crate::settings::ShopConfig,
) -> Option<MoneyView> {
    let core = order.core();
    bill_for(&core.cart, core.order_type(), core.billing.discount.clone(), config)
        .ok()
        .map(|bill| bill.grand_total.into())
}

/// A menu item, from a row. The tax words come from the book, the only place tax lives.
pub fn menu_view(item: &mb_db::repo::menu::MenuItem, book: &mb_core::TaxBook) -> MenuItemView {
    MenuItemView {
        id: item.id.as_str().to_owned(),
        name: item.name.clone(),
        price: item.unit_price.into(),
        // An item whose slab is gone still shows on the counter, and says so.
        rate_label: book
            .spec_for(&item.tax_class_id, item.price_basis)
            .map_or_else(|_| "No tax slab".to_owned(), rate_label),
        category: item.category_id.as_ref().map(|c| c.as_str().to_owned()),
    }
}

/// Turn a menu row into the snapshot a cart line is frozen from. The tax is resolved HERE, once,
/// and frozen with the line; nothing downstream asks the book again.
pub fn snapshot_for(
    item: &mb_db::repo::menu::MenuItem,
    book: &mb_core::TaxBook,
) -> UiResult<ItemSnapshot> {
    item.snapshot(book).map_err(|e| {
        UiError::new(
            "menu.slab",
            format!(
                "{} points at a tax slab this shop no longer has. Give it one under Settings › Tax.",
                item.name
            ),
        )
        .with_detail(e.to_string())
    })
}

impl CartState {}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_core::{ItemId, Qty, TaxRate};

    fn item(id: &str, name: &str, paise: i64, tax: mb_core::TaxSpec) -> ItemSnapshot {
        ItemSnapshot::new(ItemId::new(id), name, Money::from_paise(paise), tax.rate).with_tax(tax)
    }

    fn one() -> Qty {
        Qty::from_whole(1).expect("qty")
    }

    /// A registered shop — a blank GST number bills without GST, by design.
    fn regular() -> crate::settings::ShopConfig {
        let mut config = crate::settings::ShopConfig::default();
        config.store.gstin = "29ABCDE1234F1Z5".to_owned();
        config.store.state_code = "29".to_owned();
        config
    }

    /// The cart is in Rust, and the merge rule is mb-core's.
    #[test]
    fn adding_the_same_item_twice_merges_into_one_line() {
        let mut state = CartState::default();
        let dosa = item(
            "itm_dosa",
            "Masala Dosa",
            12_000,
            mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
        );
        state
            .cart
            .add(dosa.clone(), one(), None, vec![])
            .expect("add");
        state.cart.add(dosa, one(), None, vec![]).expect("add");

        let view = cart_view(&state, &regular()).expect("view");
        assert_eq!(view.lines.len(), 1, "two presses of one item are one line");
        assert_eq!(view.lines[0].qty, "2");
    }

    /// A different note is a different line — the identity includes it, so "dosa" and "dosa, no
    /// onion" go to the kitchen as two things.
    #[test]
    fn a_different_note_is_a_different_line() {
        let mut state = CartState::default();
        let dosa = item(
            "itm_dosa",
            "Masala Dosa",
            12_000,
            mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
        );
        state
            .cart
            .add(dosa.clone(), one(), None, vec![])
            .expect("add");
        state
            .cart
            .add(dosa, one(), Some("no onion".to_owned()), vec![])
            .expect("add");
        assert_eq!(
            cart_view(&state, &regular())
                .expect("view")
                .lines
                .len(),
            2
        );
    }

    /// The totals block never collapses.
    #[test]
    fn a_mixed_rate_bill_keeps_every_rate_apart() {
        let mut state = CartState::default();
        state
            .cart
            .add(
                item(
                    "itm_dosa",
                    "Masala Dosa",
                    12_000,
                    mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
                ),
                one(),
                None,
                vec![],
            )
            .expect("add");
        state
            .cart
            .add(
                item(
                    "itm_cola",
                    "Cola",
                    4_000,
                    mb_core::TaxSpec::gst(TaxRate::from_percent(18).expect("18%")),
                ),
                one(),
                None,
                vec![],
            )
            .expect("add");
        state
            .cart
            .add(
                item(
                    "itm_beer",
                    "Beer",
                    22_000,
                    mb_core::TaxSpec::liquor(mb_core::TaxRate::ZERO),
                ),
                one(),
                None,
                vec![],
            )
            .expect("add");

        let view = cart_view(&state, &regular()).expect("view");
        assert_eq!(
            view.bill.tax_rows.len(),
            2,
            "two rates means two rows, always"
        );
        assert!(view.bill.tax_rows.iter().any(|r| r.rate_label == "5%"));
        assert!(view.bill.tax_rows.iter().any(|r| r.rate_label == "18%"));

        // The bar line, and it is NEVER inside a GST total.
        assert_eq!(view.bill.non_gst_value.paise, 22_000);
        for row in &view.bill.tax_rows {
            assert!(row.taxable.paise < 22_000, "alcohol leaked into a GST row");
        }
    }

    /// Every figure the screen shows came from the bill Rust computed.
    #[test]
    fn every_figure_is_the_cores_figure() {
        let mut state = CartState::default();
        state
            .cart
            .add(
                item(
                    "itm_pbm",
                    "Paneer Butter Masala",
                    31_500,
                    mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
                ),
                one(),
                None,
                vec![],
            )
            .expect("add");

        let bill = state
            .bill(&regular())
            .expect("bill");
        let view = cart_view(&state, &regular()).expect("view");

        assert_eq!(view.bill.grand_total.paise, bill.grand_total.paise());
        assert_eq!(
            view.bill.grand_total.text,
            bill.grand_total.to_plain_string()
        );
        assert_eq!(view.bill.subtotal.paise, bill.subtotal.paise());
        assert_eq!(
            view.lines[0].amount.paise,
            bill.lines[0].net.paise()
        );
    }

    /// Balance is what is LEFT, not what the bill asks for.
    #[test]
    fn paying_the_bill_brings_the_balance_down_to_nothing() {
        let mut state = CartState::default();
        state
            .cart
            .add(
                item(
                    "itm_dosa",
                    "Masala Dosa",
                    10_000,
                    mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
                ),
                one(),
                None,
                vec![],
            )
            .expect("add");

        // 100.00 @ 5% exclusive = 105.00.
        let total = state
            .bill(&regular())
            .expect("bill")
            .grand_total;
        assert_eq!(total.paise(), 10_500);
        assert_eq!(
            cart_view(&state, &regular())
                .expect("view")
                .balance
                .paise,
            10_500
        );

        // Part of it: still owed, and the panel must say how much.
        state
            .settlement
            .add(
                mb_core::Payment::new(mb_core::PaymentMode::Cash, Money::from_paise(5_000))
                    .expect("payment"),
            )
            .expect("add");
        let view = cart_view(&state, &regular()).expect("view");
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
        let view = cart_view(&state, &regular()).expect("view");
        assert_eq!(view.balance.paise, 0, "the bill is paid in full");
        assert_eq!(view.change.paise, 0);
        assert!(state.settlement.is_settled(total).expect("settled"));
    }

    /// Fractional quantity.
    #[test]
    fn a_fractional_quantity_reaches_the_screen_intact() {
        let mut state = CartState::default();
        state
            .cart
            .add(
                item(
                    "itm_sweet",
                    "Kaju Katli",
                    90_000,
                    mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
                ),
                Qty::parse("0.5").expect("half a kilo"),
                None,
                vec![],
            )
            .expect("add");
        assert_eq!(
            cart_view(&state, &regular())
                .expect("view")
                .lines[0]
                .qty,
            "0.5"
        );
    }

    /// A rate is a label, and the treatments do not have a percentage at all — which is why
    /// nothing on the screen tries to compute with one.
    #[test]
    fn a_rate_is_a_label_not_a_number() {
        let pc = |n: u32| TaxRate::from_percent(n).expect("a real rate");
        assert_eq!(rate_label(mb_core::TaxSpec::gst(pc(5))), "5%");
        assert_eq!(
            rate_label(mb_core::TaxSpec::gst_inclusive(pc(18))),
            "18% incl."
        );
        assert_eq!(
            rate_label(mb_core::TaxSpec::liquor(TaxRate::ZERO)),
            "Non-GST"
        );
        assert_eq!(rate_label(mb_core::TaxSpec::exempt()), "Exempt");
        assert_eq!(rate_label(mb_core::TaxSpec::liquor(pc(20))), "VAT 20%");
    }

    /// The order type survives a round trip through the label, because the screen sends back
    /// what it was given.
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
