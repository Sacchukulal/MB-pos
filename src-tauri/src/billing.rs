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
}

/// One counter's work in progress.
#[derive(Debug)]
pub struct CartState {
    pub cart: Cart,
    // Private, with two setters, so a table can never sit on a parcel.
    order_type: OrderType,
    table: Option<TableSeat>,
    pub origin: Option<Origin>,
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
            settlement: Settlement::new(),
            bill_discount: None,
            kitchen: mb_core::KitchenLedger::new(),
            customer: None,
            covers: None,
            note: None,
            from_the_floor: Vec::new(),
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
            origin: Some(Origin {
                id: core.id.clone(),
                created_at: core.created_at,
                business_day: core.business_day,
                opened_by: core.created_by.clone(),
            }),
            settlement: Settlement::new(),
            bill_discount: None,
            kitchen: core.kitchen.clone(),
            customer: None,
            covers: core.covers,
            note: core.note.clone(),
            from_the_floor: Vec::new(),
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

    #[must_use]
    pub fn table_label(&self) -> Option<&str> {
        self.table.as_ref().map(|t| t.label.as_str())
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
        })
    }

    /// The cart is now this order on disk.
    pub fn adopt(&mut self, core: &OrderCore) {
        self.origin = Some(Origin {
            id: core.id.clone(),
            created_at: core.created_at,
            business_day: core.business_day,
            opened_by: core.created_by.clone(),
        });
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

/// The one way a cart becomes a bill, for the counter, the tile, the phone and the paper.
pub fn bill_for(
    cart: &Cart,
    order_type: OrderType,
    bill_discount: Option<DiscountEntry>,
    config: &crate::settings::ShopConfig,
) -> UiResult<Bill> {
    // A charge belongs to the ORDER TYPE: switching a table to a parcel drops the service
    // charge and adds the packing one.
    let charges = config.billing.charges_for(order_type);
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
    /// What the floor did to this order while the cashier had it open.
    pub from_the_floor: Vec<crate::orders::FloorChange>,
    /// A very long order, mentioned rather than refused.
    pub length_says: String,
    /// The shop always bills as one order type, so the switch is not shown.
    pub order_type_locked: bool,
    /// The shop has no kitchen ticket, so its buttons are not shown.
    pub kitchen_ticket_off: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CartLineView {
    pub index: usize,
    pub name: String,
    pub note: Option<String>,
    /// Written as a shopkeeper writes it: "2", "0.5", "1.333".
    pub qty: String,
    /// "5%", "18%", "Non-GST", "Exempt" — a label, never a number to compute with.
    pub rate_label: String,
    pub unit_price: MoneyView,
    pub gross: MoneyView,
    pub discount: MoneyView,
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
    pub id: String,
    pub label: String,
    /// The section's name, or `None` for the "No table" group that holds open parcel and
    /// self-service orders — "so no order is ever invisible".
    pub section: Option<String>,
    pub seats: u32,
    pub state: TableState,
    /// `None` when the table is free.
    pub total: Option<MoneyView>,
    /// How long it has been sitting.
    pub minutes: Option<u32>,
    /// Whether the kitchen has been told.
    pub kitchen_told: bool,
    /// Minutes since the last kitchen ticket went out — scope 14.2's second timer, and the one
    /// that catches a forgotten table: "food ordered 18 minutes ago and nothing since".
    pub kitchen_minutes: Option<u32>,
    pub order_id: Option<String>,
    /// The bill number this order has already claimed, formatted as it will be printed.
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
pub fn cart_view(state: &CartState, config: &crate::settings::ShopConfig) -> UiResult<CartView> {
    let bill = state.bill(config)?;
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
            rate_label: rate_label(billed.tax),
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
        table: state.table_label().map(str::to_owned),
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
        kitchen_told: !state.kitchen.told().is_empty(),
        covers: state.covers,
        order_id: state.order_id().map(str::to_owned),
        from_the_floor: state.from_the_floor.clone(),
        length_says: state.cart.length_says().unwrap_or_default(),
        order_type_locked: config.billing.lock_order_type,
        kitchen_ticket_off: config.billing.kitchen_ticket_off,
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

fn bill_view(bill: &Bill) -> UiResult<BillView> {
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
    let (loaded_order, loaded_table) =
        cart_is_on.map_or((None, None), |cart| (cart.order, cart.table));
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
                .table()
                .is_some_and(|t| t.as_str() == table.id.as_str())
        });

        // Decided here, where both halves are in scope, and nowhere else.
        let selected = loaded_table == Some(table.id.as_str())
            || order.is_some_and(|o| loaded_order == Some(o.core().id.as_str()));

        out.push(match order {
            Some(order) => tile_for(
                order,
                Seat {
                    label: table.label.clone(),
                    section,
                    seats: table.seats,
                    selected,
                    now,
                    warn_after,
                    late_after,
                    config,
                },
            ),
            None => TableView {
                id: table.id.as_str().to_owned(),
                label: table.label.clone(),
                section,
                seats: crate::ipc::count(table.seats),
                // A free table is free even while it is being looked at.
                state: TableState::Free,
                selected,
                total: None,
                minutes: None,
                kitchen_told: false,
                kitchen_minutes: None,
                order_id: None,
                bill_number: None,
            },
        });
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
                section: None,
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

/// Where a tile sits and how long a table may sit there — the four things that describe the
/// SEAT rather than the order in it, so `tile_for` takes two arguments instead of six.
struct Seat<'a> {
    label: String,
    section: Option<String>,
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
        // The delta ledger answers "is there anything the kitchen has not been told?".
        kitchen_told: core
            .kitchen
            .pending(&core.cart)
            .is_ok_and(|pending| pending.is_empty()),
        // Filled in by `floor::floor_on`, which is the only caller with the events table open.
        kitchen_minutes: None,
        order_id: Some(id.clone()),
        bill_number: order.bill_number().map(|claimed| claimed.formatted.clone()),
        id: core.table().map_or(id, |t| t.as_str().to_owned()),
        label,
        section,
        seats: crate::ipc::count(seats),
    }
}

/// What the tile shows as the running total.
pub(crate) fn running_total(
    order: &AnyOrder,
    config: &crate::settings::ShopConfig,
) -> Option<MoneyView> {
    let core = order.core();
    bill_for(&core.cart, core.order_type(), None, config)
        .ok()
        .map(|bill| bill.grand_total.into())
}

/// A menu item, from a row.
pub fn menu_view(item: &mb_db::repo::menu::MenuItem) -> MenuItemView {
    MenuItemView {
        id: item.id.as_str().to_owned(),
        name: item.name.clone(),
        price: item.unit_price.into(),
        rate_label: rate_label(item.tax),
        category: item.category_id.as_ref().map(|c| c.as_str().to_owned()),
    }
}

/// Turn a menu row into the snapshot a cart line is frozen from.
pub fn snapshot_for(item: &mb_db::repo::menu::MenuItem) -> ItemSnapshot {
    // The whole tax question moves across in one piece.
    let mut snapshot = ItemSnapshot::new(
        item.id.clone(),
        item.name.clone(),
        item.unit_price,
        item.tax.rate,
    )
    .with_tax(item.tax);
    if let Some(hsn) = item.hsn.clone() {
        snapshot = snapshot.with_hsn(hsn);
    }
    if let Some(category) = item.category_id.clone() {
        snapshot = snapshot.with_category(category);
    }
    snapshot.course = item.course.clone();
    // A negative or absurd number in the column becomes "no target" rather than a panic — the
    // timer is a help, never a reason a bill cannot be rung up.
    snapshot.prep_minutes = item.prep_minutes.and_then(|m| u32::try_from(m).ok());
    snapshot
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

        let view = cart_view(&state, &crate::settings::ShopConfig::default()).expect("view");
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
            cart_view(&state, &crate::settings::ShopConfig::default())
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

        let view = cart_view(&state, &crate::settings::ShopConfig::default()).expect("view");
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
            .bill(&crate::settings::ShopConfig::default())
            .expect("bill");
        let view = cart_view(&state, &crate::settings::ShopConfig::default()).expect("view");

        assert_eq!(view.bill.grand_total.paise, bill.grand_total.paise());
        assert_eq!(
            view.bill.grand_total.text,
            bill.grand_total.to_plain_string()
        );
        assert_eq!(view.bill.subtotal.paise, bill.subtotal.paise());
        assert_eq!(
            view.lines[0].amount.paise,
            bill.lines[0].gross_including_tax.paise()
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
            .bill(&crate::settings::ShopConfig::default())
            .expect("bill")
            .grand_total;
        assert_eq!(total.paise(), 10_500);
        assert_eq!(
            cart_view(&state, &crate::settings::ShopConfig::default())
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
        let view = cart_view(&state, &crate::settings::ShopConfig::default()).expect("view");
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
        let view = cart_view(&state, &crate::settings::ShopConfig::default()).expect("view");
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
            cart_view(&state, &crate::settings::ShopConfig::default())
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
