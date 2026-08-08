//! **The floor** — scope 14.1, 14.2, 14.3, and the three things you do to an
//! order that is already on a table (1.21, 1.22, 1.23).
//!
//! P09 built the tiles and the "No table" group. This is the master data
//! behind them, the layout, the timers that make a floor worth looking at, and
//! the operations.
//!
//! Bodies over `&App` (D46). Everything that changes the floor needs
//! `settings.tables`; everything that changes an ORDER needs the permission
//! that matches what it is really doing — moving an order is billing work, and
//! merging one away is close enough to a void that it is gated with one.
//!
//! # Two thresholds, and they come from settings
//!
//! `billing.rs` had `const LATE_AFTER_MINUTES: i64 = 30` — one threshold,
//! hard-coded. A dosa counter turns a table in eight minutes and a fine-dining
//! room in ninety; one constant is wrong for both. They are settings now, read
//! in Rust, and the tile state comes back **decided** so no screen ever
//! compares a number to a threshold (R8).

use mb_auth::Permission;
use mb_core::{BusinessDay, Money, Qty, TableId, Timestamp};
use mb_db::repo::floor::{DiningTable, Range, Section};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::billing::{TableView, floor_view};
use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::log_info;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

/// Settings keys for the two thresholds (scope 14.2). Spelled once.
pub const WARN_KEY: &str = "floor.warn_minutes";
pub const LATE_KEY: &str = "floor.late_minutes";

/// What a shop gets before anybody opens the settings screen.
///
/// Twenty and forty-five: long enough that an ordinary lunch never turns the
/// floor amber, short enough that both states are seen during a shift rather
/// than in theory.
pub const DEFAULT_WARN_MINUTES: i64 = 20;
pub const DEFAULT_LATE_MINUTES: i64 = 45;

// ---------------------------------------------------------------------------
// What the screen sees.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SectionView {
    pub id: String,
    pub name: String,
    pub sort_order: i32,
    pub is_active: bool,
    pub table_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TableRowView {
    pub id: String,
    pub label: String,
    /// What this table PRINTS as, worked out by the one function that decides
    /// it (`mb_core::table`). Shown in the master so an owner can see why two
    /// rows clash.
    pub printed: String,
    pub section_id: Option<String>,
    pub seats: u32,
    /// `None` when the table is not on the plan — which is every table until
    /// somebody drags one.
    pub x: Option<u32>,
    pub y: Option<u32>,
    pub is_active: bool,
    /// Whether an order is sitting on it right now. A table cannot be hidden
    /// or deleted while this is true, and the screen says so before the click.
    pub is_busy: bool,
    /// How many orders have ever pointed at it — the number the "hide it
    /// instead" refusal quotes.
    pub history: u32,
}

/// The whole floor in one answer: the tiles, the plan, the numbers, the
/// thresholds that decided the tile states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct FloorView {
    pub tiles: Vec<TableView>,
    pub sections: Vec<SectionView>,
    pub tables: Vec<TableRowView>,
    pub occupancy: OccupancyView,
    /// How many squares each way. The screen draws the grid from this rather
    /// than from a number of its own that could disagree.
    pub grid: u32,
    pub warn_minutes: u32,
    pub late_minutes: u32,
    /// True once ANY table has been placed. False means the section grid is
    /// what gets drawn — and that is not a degraded mode: no shop should have
    /// to draw a floor plan before it can bill.
    pub has_layout: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct OccupancyView {
    /// "6 of 22 tables busy" — assembled in Rust, because it is a sentence.
    pub busy: String,
    pub covers: String,
    pub turns: String,
    pub average: String,
}

/// What the screen sends to place a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TableEdit {
    pub id: String,
    pub label: String,
    pub section_id: Option<String>,
    pub seats: u32,
    pub is_active: bool,
}

// ---------------------------------------------------------------------------
// Reading.
// ---------------------------------------------------------------------------

/// The two thresholds, with the shop's answer or the default.
pub fn thresholds(app: &App) -> UiResult<(i64, i64)> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let settings = mb_db::Repos::new(tx).settings();
                let warn = settings.get::<i64>(OUTLET, WARN_KEY)?.unwrap_or(DEFAULT_WARN_MINUTES);
                let late = settings.get::<i64>(OUTLET, LATE_KEY)?.unwrap_or(DEFAULT_LATE_MINUTES);
                // A late threshold below the warn one would make the amber
                // state unreachable, and the tile would jump straight to red.
                // Believing the pair as typed is worse than repairing it here.
                Ok((warn.max(1), late.max(warn.max(1) + 1)))
            })
            .map_err(|e| words::from_db(&e))
    })
}

pub fn floor_on(app: &App) -> UiResult<FloorView> {
    guard::require(app, Permission::BillCreate)?;
    let at = now();
    let (warn, late) = thresholds(app)?;
    let loaded = app.with_cart(|state| Ok(state.order_id.clone()))?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let tables = repos.floor().list_tables(OUTLET)?;
                let sections = repos.floor().list_sections(OUTLET)?;
                let open = repos.orders().list_open(OUTLET)?;
                let told = repos.events().last_for_each(mb_db::repo::events::KITCHEN_TICKET)?;

                let day = open
                    .first()
                    .map_or_else(|| today(at), |order| order.core().business_day);
                let numbers = repos.floor().occupancy(OUTLET, day)?;

                let mut tiles =
                    floor_view(&tables, &sections, &open, loaded.as_deref(), at, warn, late);
                // The second timer: minutes since the last kitchen ticket.
                for tile in &mut tiles {
                    if let Some(order_id) = &tile.order_id {
                        tile.kitchen_minutes = told
                            .iter()
                            .find(|(id, _)| id == order_id)
                            .map(|(_, when)| crate::ipc::count(minutes_between(*when, at)));
                    }
                }

                let busy_ids: Vec<String> = open
                    .iter()
                    .filter_map(|o| o.core().table.as_ref().map(|t| t.as_str().to_owned()))
                    .collect();

                let name_of = |id: &Option<String>| {
                    id.as_ref()
                        .and_then(|id| sections.iter().find(|s| &s.id == id))
                        .map(|s| s.name.clone())
                };

                let rows: Vec<TableRowView> = tables
                    .iter()
                    .map(|table| {
                        Ok(TableRowView {
                            id: table.id.as_str().to_owned(),
                            printed: mb_core::table::printed_name(
                                name_of(&table.section_id).as_deref(),
                                &table.label,
                            ),
                            label: table.label.clone(),
                            section_id: table.section_id.clone(),
                            seats: crate::ipc::count(table.seats),
                            x: table.pos.map(|(x, _)| crate::ipc::count(x)),
                            y: table.pos.map(|(_, y)| crate::ipc::count(y)),
                            is_active: table.is_active,
                            is_busy: busy_ids.iter().any(|id| id == table.id.as_str()),
                            history: crate::ipc::count(repos.floor().orders_against(&table.id)?),
                        })
                    })
                    .collect::<Result<_, mb_db::DbError>>()?;

                Ok(FloorView {
                    tiles,
                    sections: sections
                        .iter()
                        .map(|section| SectionView {
                            id: section.id.clone(),
                            name: section.name.clone(),
                            sort_order: i32::try_from(section.sort_order).unwrap_or(0),
                            is_active: section.is_active,
                            table_count: tables
                                .iter()
                                .filter(|t| t.section_id.as_ref() == Some(&section.id))
                                .count()
                                .try_into()
                                .unwrap_or(0),
                        })
                        .collect(),
                    tables: rows,
                    occupancy: OccupancyView {
                        busy: format!("{} of {} tables busy", numbers.busy, numbers.tables),
                        covers: match numbers.covers_now {
                            0 => "No cover count".to_owned(),
                            n => format!("{n} seated"),
                        },
                        turns: format!("{} turn(s) today", numbers.turns),
                        average: numbers
                            .average_minutes
                            .map_or_else(|| "—".to_owned(), |m| format!("{m} min at table")),
                    },
                    grid: crate::ipc::count(mb_db::repo::floor::GRID_CELLS),
                    has_layout: tables.iter().any(|t| t.pos.is_some()),
                    warn_minutes: crate::ipc::count(warn),
                    late_minutes: crate::ipc::count(late),
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

fn minutes_between(from: Timestamp, to: Timestamp) -> i64 {
    (to.millis() - from.millis()).div_euclid(60_000).max(0)
}

// ---------------------------------------------------------------------------
// The master.
// ---------------------------------------------------------------------------

pub fn save_section_on(
    app: &App,
    id: String,
    name: String,
    sort_order: i64,
    is_active: bool,
) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    if name.trim().is_empty() {
        return Err(UiError::new("floor.section_name", "A section needs a name — AC, Garden."));
    }
    let at = now();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).floor().save_section(
                    OUTLET,
                    &Section { id: id.clone(), name: name.trim().to_owned(), sort_order, is_active },
                    at,
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    floor_on(app)
}

pub fn delete_section_on(app: &App, id: String) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    let at = now();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).floor().delete_section(OUTLET, &id, at))
            .map_err(|e| words::from_db(&e))
    })?;
    floor_on(app)
}

pub fn save_table_on(app: &App, edit: TableEdit) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    if edit.label.trim().is_empty() {
        return Err(UiError::new("floor.table_label", "A table needs a name — 6, G3, Counter."));
    }
    let at = now();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let existing = repos
                    .floor()
                    .list_tables(OUTLET)?
                    .into_iter()
                    .find(|t| t.id.as_str() == edit.id);

                // A table already on the plan keeps its square; a new one is
                // given a free one, so it lands somewhere VISIBLE rather than
                // at (0,0) under another tile or nowhere at all.
                let pos = match &existing {
                    Some(table) => table.pos,
                    None if repos.floor().list_tables(OUTLET)?.iter().any(|t| t.pos.is_some()) => {
                        Some(repos.floor().first_free_cell(OUTLET)?)
                    }
                    None => None,
                };

                repos.floor().save_table(
                    OUTLET,
                    &DiningTable {
                        id: TableId::new(edit.id.clone()),
                        section_id: edit.section_id.clone(),
                        label: edit.label.trim().to_owned(),
                        seats: i64::from(edit.seats.max(1)),
                        pos,
                        sort_order: existing.map_or(0, |t| t.sort_order),
                        is_active: edit.is_active,
                    },
                    at,
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    floor_on(app)
}

pub fn add_tables_on(
    app: &App,
    section_id: Option<String>,
    prefix: String,
    from: i64,
    to: i64,
    seats: i64,
) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    let at = now();

    let made = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).floor().add_range(
                    OUTLET,
                    &Range {
                        section_id: section_id.clone(),
                        prefix: prefix.trim().to_owned(),
                        from,
                        to,
                        seats: seats.max(1),
                    },
                    at,
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!("{} tables added", made.len());
    floor_on(app)
}

pub fn place_table_on(app: &App, table_id: String, x: Option<i64>, y: Option<i64>) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    let at = now();
    // **The drag is React's; the layout is Rust's.** The screen reports which
    // square a tile was dropped on; this decides whether that is allowed.
    let pos = match (x, y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .floor()
                    .place(OUTLET, &TableId::new(table_id.clone()), pos, at)
            })
            .map_err(|e| words::from_db(&e))
    })?;
    floor_on(app)
}

pub fn set_table_active_on(app: &App, table_id: String, active: bool) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    let at = now();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).floor().set_active(
                    OUTLET,
                    &TableId::new(table_id.clone()),
                    active,
                    at,
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    floor_on(app)
}

pub fn delete_table_on(app: &App, table_id: String) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    let at = now();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .floor()
                    .delete_table(OUTLET, &TableId::new(table_id.clone()), at)
            })
            .map_err(|e| words::from_db(&e))
    })?;
    floor_on(app)
}

pub fn save_thresholds_on(app: &App, warn: i64, late: i64) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    if warn < 1 || late <= warn {
        return Err(UiError::new(
            "floor.thresholds",
            "The late time has to be longer than the warning time.",
        ));
    }
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let settings = mb_db::Repos::new(tx).settings();
                settings.set(OUTLET, WARN_KEY, &warn, now(), None)?;
                settings.set(OUTLET, LATE_KEY, &late, now(), None)
            })
            .map_err(|e| words::from_db(&e))
    })?;
    floor_on(app)
}

// ---------------------------------------------------------------------------
// The three operations.
// ---------------------------------------------------------------------------

/// Read one open order, or say why not in words rather than by an unwrap.
fn open_order(app: &App, id: &str) -> UiResult<mb_core::AnyOrder> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .orders()
                    .find(&mb_core::OrderId::new(id))?
                    .ok_or_else(|| mb_db::DbError::invariant("that order is not here any more"))
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// **MOVE** — scope 1.23. The party changed seats.
///
/// Only `table_id` changes. The order keeps its id, its bill number, its cart,
/// its covers and its kitchen ledger, because it is the same order in a
/// different chair — and a move that mints a new order abandons a bill number.
pub fn move_order_on(app: &App, order_id: String, to_table: String) -> UiResult<FloorView> {
    let who = guard::require(app, Permission::BillCreate)?;
    let at = now();
    let target = TableId::new(to_table.clone());

    let order = open_order(app, &order_id)?;
    let from = order.core().table.clone();
    if from.as_ref() == Some(&target) {
        return Err(UiError::new("floor.same_table", "That order is already on that table."));
    }

    // **Asked before the write, and answered in words.** `DbError::Invariant`
    // deliberately does not reach a shopkeeper (`words::from_db` says why), so
    // a rule a cashier can act on gets its own guard and its own sentence here.
    if let Some(called) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).floor().open_order_at(&target))
            .map_err(|e| words::from_db(&e))
    })? {
        return Err(UiError::new(
            "floor.table_busy",
            format!(
                "There is already an order on that table ({}) — merge the two instead.",
                called.1
            ),
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let moved = with_table(order.clone(), Some(target.clone()));
                repos.orders().save(OUTLET, crate::billing::TERMINAL, &moved)?;
                repos.events().record(
                    &order_id,
                    at,
                    moved.core().business_day,
                    mb_db::repo::events::MOVED,
                    Some(&who.staff_id),
                    Some(target.as_str()),
                )?;
                repos.audit().append(
                    OUTLET,
                    &mb_auth::AuditEntry::new(
                        at,
                        today(at),
                        Some(who.staff_id.clone()),
                        mb_auth::audit::action::ORDER_MOVED,
                        "order",
                    )
                    .about(order_id.clone())
                    .with_after(serde_json::json!({
                        "from": from.as_ref().map(TableId::as_str),
                        "to": target.as_str(),
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // A cart holding the moved order has to hear about it, or the screen would
    // keep billing the table the party has left.
    app.with_cart_mut(|state| {
        if state.order_id.as_deref() == Some(order_id.as_str()) {
            state.table = Some(target.as_str().to_owned());
        }
        Ok(())
    })?;

    log_info!("{} moved an order to {}", who.name, target.as_str());
    floor_on(app)
}

/// Put an order on a different table without rebuilding it by hand.
fn with_table(order: mb_core::AnyOrder, table: Option<TableId>) -> mb_core::AnyOrder {
    let mut order = order;
    match &mut order {
        mb_core::AnyOrder::Draft(o) => o.core.table = table,
        mb_core::AnyOrder::Open(o) => o.core.table = table,
        mb_core::AnyOrder::Settled(o) => o.core.table = table,
        mb_core::AnyOrder::Cancelled(o) => o.core.table = table,
        mb_core::AnyOrder::Voided(o) => o.core.table = table,
    }
    order
}

/// **MERGE** — scope 1.22. Two tables, one bill.
///
/// The absorbed order is **cancelled with a link**, never deleted (D47), and
/// the reasoning is in the schema comment on `orders.merged_into`: its food
/// was sold on the other bill, so counting it in the day's gross would count
/// it twice.
pub fn merge_orders_on(app: &App, from_order: String, into_order: String) -> UiResult<FloorView> {
    let who = guard::require(app, Permission::BillVoid)?;
    let at = now();

    if from_order == into_order {
        return Err(UiError::new("floor.same_order", "Those are the same order."));
    }
    let absorbed = open_order(app, &from_order)?;
    let survivor = open_order(app, &into_order)?;

    let (mut survivor_portion, absorbed_portion) = (
        mb_core::Portion {
            cart: survivor.core().cart.clone(),
            kitchen: survivor.core().kitchen.clone(),
        },
        mb_core::Portion {
            cart: absorbed.core().cart.clone(),
            kitchen: absorbed.core().kitchen.clone(),
        },
    );
    mb_core::merge_into(&mut survivor_portion, absorbed_portion)
        .map_err(|e| UiError::new("floor.merge", "Those orders could not be merged.").with_detail(e.to_string()))?;

    let day = survivor.core().business_day;
    let absorbed_label = absorbed
        .core()
        .table
        .as_ref()
        .map_or_else(|| "an order".to_owned(), |t| t.as_str().to_owned());

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);

                // The survivor takes the food.
                let mut merged = survivor.clone();
                let (cart, kitchen) = survivor_portion.clone().into_parts();
                match &mut merged {
                    mb_core::AnyOrder::Draft(o) => {
                        o.core.cart = cart;
                        o.core.kitchen = kitchen;
                    }
                    mb_core::AnyOrder::Open(o) => {
                        o.core.cart = cart;
                        o.core.kitchen = kitchen;
                    }
                    _ => {
                        return Err(mb_db::DbError::invariant(
                            "only an open order can take another one's food",
                        ));
                    }
                }
                repos.orders().save(OUTLET, crate::billing::TERMINAL, &merged)?;

                // And the absorbed one is closed with a link rather than a hole.
                let closed = match absorbed.clone() {
                    mb_core::AnyOrder::Open(o) => o
                        .cancel(
                            &format!("merged into {into_order}"),
                            who.staff_id.clone(),
                            at,
                        )
                        .map_err(|e| mb_db::DbError::invariant(e.to_string()))?,
                    _ => {
                        return Err(mb_db::DbError::invariant(
                            "only an open order can be merged away",
                        ));
                    }
                };
                repos
                    .orders()
                    .save(OUTLET, crate::billing::TERMINAL, &mb_core::AnyOrder::Cancelled(closed))?;
                repos.floor().record_merge(&from_order, &into_order)?;
                repos.events().record(
                    &from_order,
                    at,
                    day,
                    mb_db::repo::events::MERGED,
                    Some(&who.staff_id),
                    Some(&into_order),
                )?;
                repos.audit().append(
                    OUTLET,
                    &mb_auth::AuditEntry::new(
                        at,
                        today(at),
                        Some(who.staff_id.clone()),
                        mb_auth::audit::action::ORDER_MERGED,
                        "order",
                    )
                    .about(from_order.clone())
                    .with_after(serde_json::json!({ "into": into_order })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // If either order is in the cart, the cart is now stale in a way the
    // cashier cannot see. Emptying it is the honest answer: the food is on the
    // other bill, and the screen reloads it from disk.
    app.with_cart_mut(|state| {
        if state.order_id.as_deref() == Some(from_order.as_str())
            || state.order_id.as_deref() == Some(into_order.as_str())
        {
            *state = crate::billing::CartState::default();
        }
        Ok(())
    })?;

    log_info!("{} merged {} into another bill", who.name, absorbed_label);
    floor_on(app)
}

/// What the screen sends to split an order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SplitRequest {
    pub order_id: String,
    /// `(line index, quantity as typed)` — the lines going to the new bill.
    pub lines: Vec<(usize, String)>,
    /// Where the new order sits. `None` leaves it on the same table as a
    /// second seat (`6A` / `6B`, scope 1.6).
    pub to_table: Option<String>,
    /// The letter for the new order when it stays at the same table.
    pub seat: Option<String>,
}

/// **SPLIT** — scope 1.21. Two guests, two bills.
///
/// The money is `transfer::take_lines`' problem and the reasoning lives there.
/// This is the plumbing: a second order, its own bill number, and the ledger
/// arithmetic that stops the kitchen being told twice.
pub fn split_order_on(app: &App, request: SplitRequest) -> UiResult<FloorView> {
    let who = guard::require(app, Permission::BillCreate)?;
    let at = now();

    let order = open_order(app, &request.order_id)?;
    let mut origin = mb_core::Portion {
        cart: order.core().cart.clone(),
        kitchen: order.core().kitchen.clone(),
    };

    let mut picks = Vec::new();
    for (index, qty) in &request.lines {
        let qty = Qty::parse(qty.trim()).map_err(|e| {
            UiError::new("floor.split_qty", format!("\"{qty}\" is not a quantity."))
                .with_detail(e.to_string())
        })?;
        picks.push(mb_core::Pick { index: *index, qty });
    }

    let moved = mb_core::take_lines(&mut origin, &picks).map_err(|e| {
        UiError::new("floor.split", format!("That split is not possible: {e}"))
    })?;

    let seat = request
        .seat
        .as_deref()
        .map(mb_core::SubTable::parse)
        .transpose()
        .map_err(|e| UiError::new("floor.seat", "A seat is one letter, A to Z.").with_detail(e.to_string()))?;

    let day = order.core().business_day;
    let table = match &request.to_table {
        Some(id) => Some(TableId::new(id.clone())),
        None => order.core().table.clone(),
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);

                // What stays.
                let mut kept = order.clone();
                let (cart, kitchen) = origin.clone().into_parts();
                match &mut kept {
                    mb_core::AnyOrder::Open(o) => {
                        o.core.cart = cart;
                        o.core.kitchen = kitchen;
                    }
                    _ => {
                        return Err(mb_db::DbError::invariant(
                            "only an open order can be split",
                        ));
                    }
                }
                repos.orders().save(OUTLET, crate::billing::TERMINAL, &kept)?;

                // And what leaves: a new order with its own numbers, claimed
                // against the ORIGINAL's business day (D5) so a split at 00:15
                // does not jump to tomorrow's series.
                let (moved_cart, moved_kitchen) = moved.clone().into_parts();
                let mut fresh = mb_core::DraftOrder::new(
                    mb_core::OrderId::new(format!("ord_{}", at.millis())),
                    day,
                    at,
                    order.core().order_type,
                    who.staff_id.clone(),
                );
                fresh.core.cart = moved_cart;
                fresh.core.kitchen = moved_kitchen;
                fresh.core.table = table.clone();
                fresh.core.sub_table = seat.clone();

                // Its own token and bill number, claimed in THIS transaction so a
                // failure cannot consume one — the same rule `open_draft`
                // follows, and claimed against the ORIGINAL order's business
                // day (D5) so a split at 00:15 does not jump to tomorrow.
                let token = mb_db::numbering::claim(
                    tx,
                    OUTLET,
                    crate::billing::TERMINAL,
                    mb_db::numbering::CounterKind::Token,
                    day,
                )?;
                let bill_number = mb_db::numbering::claim(
                    tx,
                    OUTLET,
                    crate::billing::TERMINAL,
                    mb_db::numbering::CounterKind::Bill,
                    day,
                )?;
                let opened = mb_core::OpenOrder { core: fresh.core, token, bill_number };
                let new_id = opened.core.id.as_str().to_owned();
                repos
                    .orders()
                    .save(OUTLET, crate::billing::TERMINAL, &mb_core::AnyOrder::Open(opened))?;

                repos.events().record(
                    &request.order_id,
                    at,
                    day,
                    mb_db::repo::events::SPLIT,
                    Some(&who.staff_id),
                    Some(&new_id),
                )?;
                repos.audit().append(
                    OUTLET,
                    &mb_auth::AuditEntry::new(
                        at,
                        today(at),
                        Some(who.staff_id.clone()),
                        mb_auth::audit::action::ORDER_SPLIT,
                        "order",
                    )
                    .about(request.order_id.clone())
                    .with_after(serde_json::json!({
                        "into": new_id,
                        "lines": request.lines.len(),
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    app.with_cart_mut(|state| {
        if state.order_id.as_deref() == Some(request.order_id.as_str()) {
            *state = crate::billing::CartState::default();
        }
        Ok(())
    })?;

    log_info!("{} split an order", who.name);
    floor_on(app)
}

/// What an even split comes to, per guest — scope 1.21's other half.
///
/// **This answers a question; it does not create bills.** Splitting the money
/// n ways without splitting the food is what a group asking "what do we each
/// owe?" actually wants, and inventing n orders to answer it would litter the
/// day with bills nobody asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct EvenSplitView {
    pub total: MoneyView,
    pub ways: u32,
    pub shares: Vec<MoneyView>,
    /// "₹33.34 each, and one of you pays a paisa more" — said out loud,
    /// because a remainder nobody mentions looks like a rounding bug.
    pub note: String,
}

pub fn even_split_on(app: &App, ways: u32) -> UiResult<EvenSplitView> {
    guard::require(app, Permission::BillCreate)?;
    let total = app.with_cart(|state| Ok(state.bill()?.grand_total))?;

    let shares = mb_core::even_shares(total, ways)
        .map_err(|e| UiError::new("floor.even_split", format!("That split is not possible: {e}")))?;

    let biggest = shares.first().copied().unwrap_or(Money::ZERO);
    let smallest = shares.last().copied().unwrap_or(Money::ZERO);
    let note = if biggest == smallest {
        format!("{} each", biggest.to_plain_string())
    } else {
        format!(
            "{} each — the first {} pay {} so nothing is lost",
            smallest.to_plain_string(),
            shares.iter().filter(|s| **s == biggest).count(),
            biggest.to_plain_string(),
        )
    };

    Ok(EvenSplitView {
        total: MoneyView::from(total),
        ways,
        shares: shares.into_iter().map(MoneyView::from).collect(),
        note,
    })
}

/// Scope 1.24 — how many are eating. Never compulsory.
pub fn set_covers_on(app: &App, covers: Option<u32>) -> UiResult<()> {
    guard::require(app, Permission::BillCreate)?;
    app.with_cart_mut(|state| {
        state.covers = covers;
        Ok(())
    })
}

// --- the seats -------------------------------------------------------------

#[tauri::command]
pub fn floor_plan(app: tauri::State<'_, App>) -> UiResult<FloorView> {
    floor_on(&app)
}

#[tauri::command]
pub fn save_floor_section(
    app: tauri::State<'_, App>,
    id: String,
    name: String,
    sort_order: i64,
    is_active: bool,
) -> UiResult<FloorView> {
    save_section_on(&app, id, name, sort_order, is_active)
}

#[tauri::command]
pub fn delete_floor_section(app: tauri::State<'_, App>, id: String) -> UiResult<FloorView> {
    delete_section_on(&app, id)
}

#[tauri::command]
pub fn save_dining_table(app: tauri::State<'_, App>, edit: TableEdit) -> UiResult<FloorView> {
    save_table_on(&app, edit)
}

#[tauri::command]
pub fn add_dining_tables(
    app: tauri::State<'_, App>,
    section_id: Option<String>,
    prefix: String,
    from: i64,
    to: i64,
    seats: i64,
) -> UiResult<FloorView> {
    add_tables_on(&app, section_id, prefix, from, to, seats)
}

#[tauri::command]
pub fn place_dining_table(
    app: tauri::State<'_, App>,
    table_id: String,
    x: Option<i64>,
    y: Option<i64>,
) -> UiResult<FloorView> {
    place_table_on(&app, table_id, x, y)
}

#[tauri::command]
pub fn set_dining_table_active(
    app: tauri::State<'_, App>,
    table_id: String,
    active: bool,
) -> UiResult<FloorView> {
    set_table_active_on(&app, table_id, active)
}

#[tauri::command]
pub fn delete_dining_table(app: tauri::State<'_, App>, table_id: String) -> UiResult<FloorView> {
    delete_table_on(&app, table_id)
}

#[tauri::command]
pub fn save_floor_thresholds(
    app: tauri::State<'_, App>,
    warn: i64,
    late: i64,
) -> UiResult<FloorView> {
    save_thresholds_on(&app, warn, late)
}

#[tauri::command]
pub fn move_order(
    app: tauri::State<'_, App>,
    order_id: String,
    to_table: String,
) -> UiResult<FloorView> {
    move_order_on(&app, order_id, to_table)
}

#[tauri::command]
pub fn merge_orders(
    app: tauri::State<'_, App>,
    from_order: String,
    into_order: String,
) -> UiResult<FloorView> {
    merge_orders_on(&app, from_order, into_order)
}

#[tauri::command]
pub fn split_order(app: tauri::State<'_, App>, request: SplitRequest) -> UiResult<FloorView> {
    split_order_on(&app, request)
}

#[tauri::command]
pub fn even_split(app: tauri::State<'_, App>, ways: u32) -> UiResult<EvenSplitView> {
    even_split_on(&app, ways)
}

#[tauri::command]
pub fn set_covers(app: tauri::State<'_, App>, covers: Option<u32>) -> UiResult<()> {
    set_covers_on(&app, covers)
}

/// Unused today; here so the day the floor needs a business day it does not
/// invent a second way of asking for one.
#[allow(dead_code, reason = "the floor's own day helper, used by occupancy tests")]
fn business_day_now() -> BusinessDay {
    today(now())
}
