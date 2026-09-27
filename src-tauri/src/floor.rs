//! The floor — scope 14.1, 14.2, 14.3, and the three things you do to an order that is already
//! on a table (1.21, 1.22, 1.23).

use mb_auth::Permission;
use mb_core::{Qty, TableId, Timestamp};
use mb_db::repo::floor::{DiningTable, Range, Section};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::billing::{TableView, floor_view};
use crate::flows::{now, today};
use crate::guard;
use crate::log_info;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

/// Settings keys for the two thresholds (scope 14.2).
pub const WARN_KEY: &str = "floor.warn_minutes";
pub const LATE_KEY: &str = "floor.late_minutes";

/// What a shop gets before anybody opens the settings screen.
pub const DEFAULT_WARN_MINUTES: i64 = 20;
pub const DEFAULT_LATE_MINUTES: i64 = 45;

// What the screen sees.

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
    /// What this table PRINTS as, worked out by the one function that decides it
    /// (`mb_core::table`).
    pub printed: String,
    pub section_id: Option<String>,
    pub seats: u32,
    /// `None` when the table is not on the plan — which is every table until somebody drags
    /// one.
    pub x: Option<u32>,
    pub y: Option<u32>,
    pub is_active: bool,
    /// Whether an order is sitting on it right now.
    pub is_busy: bool,
    /// How many orders have ever pointed at it — the number the "hide it instead" refusal
    /// quotes.
    pub history: u32,
}

/// The whole floor in one answer: the tiles, the plan, the numbers, the thresholds that decided
/// the tile states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct FloorView {
    pub tiles: Vec<TableView>,
    pub sections: Vec<SectionView>,
    pub tables: Vec<TableRowView>,
    pub occupancy: OccupancyView,
    /// How many squares each way.
    pub grid: u32,
    pub warn_minutes: u32,
    pub late_minutes: u32,
    /// True once ANY table has been placed.
    pub has_layout: bool,
    /// Whether this person may change the room.
    pub can_arrange: bool,
    /// Whether the billing screen shows these tables — the switch on this screen.
    pub tables_on_counter: bool,
    /// A beep when a phone lands an order — the shop's switch under Settings › Billing.
    pub arrival_beep: bool,
    /// The cards beat on arrival — the switch beside it.
    pub arrival_beat: bool,
}

/// What a change to several tables at once did — and what it left alone, named, with the
/// reason. A bulk action that stopped at the first refusal used to undo the other thirty-nine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct FloorChangeView {
    pub floor: FloorView,
    /// The headline, in one line.
    pub said: String,
    /// One line per table that was left alone: its name, and why.
    pub kept: Vec<String>,
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

/// The two thresholds, with the shop's answer or the default.
pub fn thresholds(app: &App) -> UiResult<(i64, i64)> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let settings = mb_db::Repos::new(tx).settings();
                let warn = settings
                    .get::<i64>(OUTLET, WARN_KEY)?
                    .unwrap_or(DEFAULT_WARN_MINUTES);
                let late = settings
                    .get::<i64>(OUTLET, LATE_KEY)?
                    .unwrap_or(DEFAULT_LATE_MINUTES);
                // A late threshold below the warn one would make the amber state unreachable,
                // and the tile would jump straight to red.
                Ok((warn.max(1), late.max(warn.max(1) + 1)))
            })
            .map_err(|e| words::from_db(&e))
    })
}

pub fn floor_on(app: &App) -> UiResult<FloorView> {
    guard::require(app, Permission::BillCreate)?;
    // Asked, not assumed. `guard::require` returns the actor when it allows, so "may this
    // person arrange the room" is the same question the arranging commands ask, answered once
    // for the screen.
    let can_arrange = guard::require(app, Permission::TablesManage).is_ok();
    let at = now();
    let (warn, late) = thresholds(app)?;
    // Taken once, outside the transaction: a tile's running total is rounded and charged the
    // way the bill will be, and reading that inside the loop would be a lock per table.
    let config = app.shop_config();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let tables = repos.floor().list_tables(OUTLET)?;
                let sections = repos.floor().list_sections(OUTLET)?;
                let open = repos.orders().list_open(OUTLET)?;
                let numbers = repos.floor().occupancy(OUTLET, today(at))?;

                let mut tiles = floor_view(
                    &tables,
                    &sections,
                    &open,
                    crate::billing::Room {
                        // This screen has no cart, so it marks nothing.
                        cart_is_on: None,
                        now: at,
                        warn_after: warn,
                        late_after: late,
                        config: &config,
                    },
                );
                // The kitchen timer, who opened each order, what the phones asked — the same
                // call the billing grid makes.
                crate::billing::decorate(&mut tiles, &repos, at)?;

                let busy_ids: Vec<String> = open
                    .iter()
                    .filter_map(|o| o.core().table().map(|t| t.as_str().to_owned()))
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
                        turns: format!("{} today", words::count(numbers.turns, "turn", "turns")),
                        average: numbers
                            .average_minutes
                            .map_or_else(|| "—".to_owned(), |m| format!("{m} min at table")),
                    },
                    grid: crate::ipc::count(mb_db::repo::floor::GRID_CELLS),
                    has_layout: tables.iter().any(|t| t.pos.is_some()),
                    can_arrange,
                    tables_on_counter: config.billing.tables_on_counter,
                    arrival_beep: config.billing.arrival_beep,
                    arrival_beat: config.billing.arrival_beat,
                    warn_minutes: crate::ipc::count(warn),
                    late_minutes: crate::ipc::count(late),
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

pub(crate) fn minutes_between(from: Timestamp, to: Timestamp) -> i64 {
    (to.millis() - from.millis()).div_euclid(60_000).max(0)
}

// The master.

pub fn save_section_on(
    app: &App,
    id: String,
    name: String,
    sort_order: i64,
    is_active: bool,
) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    if name.trim().is_empty() {
        return Err(UiError::new(
            "floor.section_name",
            "A section needs a name — AC, Garden.",
        ));
    }
    let at = now();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).floor().save_section(
                    OUTLET,
                    &Section {
                        id: id.clone(),
                        name: name.trim().to_owned(),
                        sort_order,
                        is_active,
                    },
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
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .floor()
                    .delete_section(OUTLET, &id, at)
            })
            .map_err(|e| words::from_db(&e))
    })?;
    floor_on(app)
}

pub fn save_table_on(app: &App, edit: TableEdit) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    if edit.label.trim().is_empty() {
        return Err(UiError::new(
            "floor.table_label",
            "A table needs a name — 6, G3, Counter.",
        ));
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

                // A table already on the plan keeps its square; a new one is given a free one,
                // so it lands somewhere VISIBLE rather than at (0,0) under another tile or
                // nowhere at all.
                let pos = match &existing {
                    Some(table) => table.pos,
                    None if repos
                        .floor()
                        .list_tables(OUTLET)?
                        .iter()
                        .any(|t| t.pos.is_some()) =>
                    {
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

pub fn place_table_on(
    app: &App,
    table_id: String,
    x: Option<i64>,
    y: Option<i64>,
) -> UiResult<FloorView> {
    guard::require(app, Permission::TablesManage)?;
    let at = now();
    // The drag is React's; the layout is Rust's.
    let pos = match (x, y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).floor().place(
                    OUTLET,
                    &TableId::new(table_id.clone()),
                    pos,
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
                mb_db::Repos::new(tx).floor().delete_table(
                    OUTLET,
                    &TableId::new(table_id.clone()),
                    at,
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    floor_on(app)
}

/// Several tables at once — one transaction still, but every table is judged on its own, so
/// one that cannot go no longer undoes the other thirty-nine.
pub fn delete_tables_on(app: &App, table_ids: Vec<String>) -> UiResult<FloorChangeView> {
    guard::require(app, Permission::TablesManage)?;
    let at = now();
    let (done, kept) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let floor = mb_db::Repos::new(tx).floor();
                let named = Named::read(&floor)?;
                let mut change = Change::default();
                for id in &table_ids {
                    let table = TableId::new(id.clone());
                    match floor.why_not_deleted(&table)? {
                        Some(why) => change.kept.push(named.line(id, &why)),
                        None => {
                            floor.delete_table(OUTLET, &table, at)?;
                            change.done += 1;
                        }
                    }
                }
                Ok((change.done, change.kept))
            })
            .map_err(|e| words::from_db(&e))
    })?;
    changed(app, done, kept, "deleted")
}

/// The same bargain for hiding and putting back — see `delete_tables_on`. Putting a table back
/// is never refused, so only hiding has anything to keep.
pub fn set_tables_active_on(
    app: &App,
    table_ids: Vec<String>,
    active: bool,
) -> UiResult<FloorChangeView> {
    guard::require(app, Permission::TablesManage)?;
    let at = now();
    let (done, kept) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let floor = mb_db::Repos::new(tx).floor();
                let named = Named::read(&floor)?;
                let mut change = Change::default();
                for id in &table_ids {
                    let table = TableId::new(id.clone());
                    let why = if active {
                        None
                    } else {
                        floor.why_not_hidden(&table)?
                    };
                    match why {
                        Some(why) => change.kept.push(named.line(id, &why)),
                        None => {
                            floor.set_active(OUTLET, &table, active, at)?;
                            change.done += 1;
                        }
                    }
                }
                Ok((change.done, change.kept))
            })
            .map_err(|e| words::from_db(&e))
    })?;
    changed(
        app,
        done,
        kept,
        if active {
            "put back"
        } else {
            "taken off the floor"
        },
    )
}

/// What a bulk change has managed so far, while it is still going.
#[derive(Default)]
struct Change {
    done: u32,
    kept: Vec<String>,
}

/// Every table's name as it prints, read once so a loop over forty of them does not ask
/// forty times.
struct Named {
    tables: Vec<DiningTable>,
    sections: Vec<Section>,
}

impl Named {
    fn read(floor: &mb_db::repo::floor::FloorRepo<'_>) -> Result<Self, mb_db::DbError> {
        Ok(Self {
            tables: floor.list_tables(OUTLET)?,
            sections: floor.list_sections(OUTLET)?,
        })
    }

    /// "AC 13 — this table has 3 order(s) against it…": the table a person can walk to, and
    /// then the reason.
    fn line(&self, id: &str, why: &str) -> String {
        let printed = self
            .tables
            .iter()
            .find(|t| t.id.as_str() == id)
            .map_or_else(
                || "A table".to_owned(),
                |table| {
                    let room = table
                        .section_id
                        .as_ref()
                        .and_then(|want| self.sections.iter().find(|s| &s.id == want));
                    mb_core::table::printed_name(room.map(|s| s.name.as_str()), &table.label)
                },
            );
        format!("{printed} — {why}")
    }
}

/// The floor as it is now, and one line saying what just happened to it.
fn changed(app: &App, done: u32, kept: Vec<String>, did: &str) -> UiResult<FloorChangeView> {
    let tables = |how: u32| words::count(i64::from(how), "table", "tables");
    let could_not = u32::try_from(kept.len()).unwrap_or(u32::MAX);
    let said = match (done, could_not) {
        (0, 0) => "Nothing was ticked.".to_owned(),
        (0, _) => format!("No table was {did}. {} could not be.", tables(could_not)),
        (_, 0) => format!("{} {did}.", tables(done)),
        (_, _) => format!(
            "{} {did}. {} could not be.",
            tables(done),
            tables(could_not)
        ),
    };
    Ok(FloorChangeView {
        floor: floor_on(app)?,
        said,
        kept,
    })
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

// The three operations.

/// Read one open order, or say why not in words rather than by an unwrap.
fn open_order(app: &App, id: &str) -> UiResult<mb_core::AnyOrder> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .orders()
                    .find_working(&mb_core::OrderId::new(id))?
                    .ok_or_else(|| mb_db::DbError::invariant("that order is not here any more"))
            })
            .map_err(|e| words::from_db(&e))
    })
}

pub fn move_order_on(app: &App, order_id: String, to_table: String) -> UiResult<FloorView> {
    let _one_at_a_time = app.begin_action();
    let who = guard::require(app, Permission::BillCreate)?;
    let at = now();
    let target = TableId::new(to_table.clone());

    if app.with_cart(|state| Ok(state.order_id() == Some(order_id.as_str())))? {
        crate::flows::park_open_order(app)?;
    }
    let order = open_order(app, &order_id)?;
    if !matches!(order, mb_core::AnyOrder::Open(_)) {
        return Err(UiError::new("floor.finished", "Only an open order can move to another table."));
    }
    let from = order.core().table().cloned();
    if from.as_ref() == Some(&target) {
        return Err(UiError::new(
            "floor.same_table",
            "That order is already on that table.",
        ));
    }

    // Asked before the write, and answered in words.
    if let Some(called) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| Ok(mb_db::Repos::new(tx).orders().list_open(OUTLET)?
                .into_iter().find(|order| order.core().table() == Some(&target))))
            .map_err(|e| words::from_db(&e))
    })? {
        return Err(UiError::new(
            "floor.table_busy",
            format!(
                "There is already an order on that table ({}) — merge the two instead.",
                called.token().map_or("open order", |token| token.formatted.as_str())
            ),
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.orders().assert_working(&order)?;
                if repos.orders().list_open(OUTLET)?.iter().any(|order| order.core().table() == Some(&target)) {
                    return Err(mb_db::DbError::invariant("That table already has an order. Merge the bills instead."));
                }
                let moved = with_table(order.clone(), target.clone());
                repos.orders().save_working(OUTLET, app.terminal_id(), &moved)?;
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

    // A cart holding the moved order has to hear about it, or the screen would keep billing the
    // table the party has left.
    let label =
        crate::flows::table_name(app, &target).unwrap_or_else(|| target.as_str().to_owned());
    app.with_cart_mut(|state| {
        if state.order_id() == Some(order_id.as_str()) {
            state.place_on(target.clone(), label, None);
            if let Some(origin) = &mut state.origin {
                origin.baseline = Some(with_table(order.clone(), target.clone()).core().clone());
            }
        }
        Ok(())
    })?;

    log_info!("{} moved an order to {}", who.name, target.as_str());
    floor_on(app)
}

/// Put an order on a different table, which makes it dine-in.
fn with_table(mut order: mb_core::AnyOrder, table: TableId) -> mb_core::AnyOrder {
    order.core_mut().placement = mb_core::Placement::on_table(table);
    order
}

/// What to call an order in a sentence a person reads: the table it sits on, by the name the
/// shop gave the table, else the token the screen shows it by. Never its id.
fn order_label(app: &App, order: &mb_core::AnyOrder) -> String {
    if let Some(table) = order.core().table() {
        return crate::flows::table_name(app, table).unwrap_or_else(|| table.as_str().to_owned());
    }
    order
        .token()
        .map_or_else(|| "an order".to_owned(), |t| t.formatted.clone())
}

pub fn merge_orders_on(app: &App, from_order: String, into_order: String) -> UiResult<FloorView> {
    let _one_at_a_time = app.begin_action();
    let who = guard::require(app, Permission::BillCreate)?;
    let at = now();

    if from_order == into_order {
        return Err(UiError::new(
            "floor.same_order",
            "Those are the same order.",
        ));
    }
    if app.with_cart(|s| Ok(s.order_id() == Some(from_order.as_str()) || s.order_id() == Some(into_order.as_str())))? {
        crate::flows::park_open_order(app)?;
    }
    let absorbed_before = open_order(app, &from_order)?;
    let survivor_before = open_order(app, &into_order)?;
    for order in [&absorbed_before, &survivor_before] {
        if order.core().billing.billed_into.is_some() {
            return Err(UiError::new("merge.linked", "This order already belongs to a combined bill."));
        }
        if let Some(refusal) = crate::dayclose::day_refusal_on(app, order.core().business_day, "merge.closed", "combine these bills")? { return Err(refusal); }
        if matches!(order, mb_core::AnyOrder::Settled(_)) { guard::require(app, Permission::BillRevert)?; }
    }
    if absorbed_before.core().business_day != survivor_before.core().business_day {
        return Err(UiError::new("merge.day", "Choose bills from the same business day."));
    }
    let editable = |order: mb_core::AnyOrder| -> UiResult<mb_core::AnyOrder> { match order {
        mb_core::AnyOrder::Settled(paid) => Ok(mb_core::AnyOrder::Open(paid.reopen())),
        open @ mb_core::AnyOrder::Open(_) => Ok(open),
        _ => Err(UiError::new("merge.finished", "Only open or paid bills can be combined.")),
    }};
    let absorbed = editable(absorbed_before.clone())?;
    let survivor = editable(survivor_before.clone())?;

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
    mb_core::merge_into(&mut survivor_portion, absorbed_portion).map_err(|e| {
        UiError::new("floor.merge", "Those orders could not be merged.").with_detail(e.to_string())
    })?;

    let day = survivor.core().business_day;
    let absorbed_label = order_label(app, &absorbed);
    let survivor_label = order_label(app, &survivor);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);

                // The survivor takes the food.
                repos.orders().assert_working(&absorbed_before)?;
                repos.orders().assert_working(&survivor_before)?;
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
                let account = &mut merged.core_mut().billing;
                if account.service_cart.is_none() {
                    account.service_cart = Some(survivor.core().cart.clone());
                    account.service_kitchen = Some(survivor.core().kitchen.clone());
                }
                account.settlement.append(&absorbed.core().billing.settlement).map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
                account.sources.push(absorbed.core().id.clone());
                account.source_labels.push(absorbed.bill_number().map_or_else(|| format!("Order {absorbed_label}"), |n| format!("Bill {}", n.formatted)));
                account.source_labels.extend(absorbed.core().billing.source_labels.clone());
                account.sources.extend(absorbed.core().billing.sources.clone());
                account.serving.push(absorbed.core().placement.clone());
                account.serving.extend(absorbed.core().billing.serving.clone());
                // Preserve the money discounted on each source instead of expanding a
                // percentage onto the other guest's food.
                let discount = [&survivor, &absorbed].iter().try_fold(mb_core::Money::ZERO, |sum, order| {
                    crate::flows::bill_of(app, order).map_err(|e| mb_db::DbError::invariant(e.message))
                        .and_then(|b| sum.add(b.total_bill_discount).map_err(|e| mb_db::DbError::invariant(e.to_string())))
                })?;
                account.discount = discount.is_positive().then(|| mb_core::DiscountEntry::new(mb_core::Discount::Amount(discount)));
                repos.orders().save_working(OUTLET, app.terminal_id(), &merged)?;

                // An open source keeps serving its table; an issued source is replaced only
                // when the combined correction finishes.
                let mut serving = absorbed.clone();
                serving.core_mut().billing.billed_into = Some(merged.core().id.clone());
                if matches!(absorbed_before, mb_core::AnyOrder::Settled(_))
                    && let mb_core::AnyOrder::Open(open) = serving
                {
                    serving = mb_core::AnyOrder::Cancelled(open.cancel("Replaced by combined bill", who.staff_id.clone(), at).map_err(|e| mb_db::DbError::invariant(e.to_string()))?);
                }
                repos.orders().save_working(OUTLET, app.terminal_id(), &serving)?;
                // A previously combined source can itself join another bill. Every original
                // table must now point at the surviving account, including paid sources.
                for source in &absorbed.core().billing.sources {
                    let mut linked = repos.orders().find_working(source)?
                        .ok_or_else(|| mb_db::DbError::invariant("A combined source order is missing."))?;
                    linked.core_mut().billing.billed_into = Some(merged.core().id.clone());
                    repos.orders().save_working(OUTLET, app.terminal_id(), &linked)?;
                }
                // Settling a combined bill also creates a service-only order for its own
                // table. It carries no source payment, but must follow a later combination.
                for mut linked in repos.orders().list_open(OUTLET)? {
                    if linked.core().billing.billed_into.as_ref() == Some(&absorbed.core().id) {
                        linked.core_mut().billing.billed_into = Some(merged.core().id.clone());
                        repos.orders().save_working(OUTLET, app.terminal_id(), &linked)?;
                    }
                }
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

    // If either order is in the cart, the cart is now stale in a way the cashier cannot see.
    app.with_cart_mut(|state| {
        if state.order_id() == Some(from_order.as_str())
            || state.order_id() == Some(into_order.as_str())
        {
            *state = crate::billing::CartState::default();
        }
        Ok(())
    })?;

    log_info!(
        "{} merged {} into {}",
        who.name,
        absorbed_label,
        survivor_label
    );
    floor_on(app)
}

/// The same order tiles the counter knows, including issued bills from today's book.
#[tauri::command]
pub fn combine_candidates(app: tauri::State<'_, App>) -> UiResult<Vec<crate::billing::TableView>> {
    combine_candidates_on(&app)
}

pub fn combine_candidates_on(app: &App) -> UiResult<Vec<crate::billing::TableView>> {
    guard::require(app, Permission::BillCreate)?;
    let at = now();
    let config = app.shop_config();
    app.with_shop(|shop| shop.db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        let mut orders = repos.orders().list_for_day(OUTLET, today(at))?;
        // Paid sources can have a cancelled working draft while their issued sale remains
        // in reports. Candidate selection follows that draft too, not just open drafts.
        for order in &mut orders {
            if let Some(working) = repos.orders().find_working(&order.core().id)? {
                *order = working;
            }
        }
        for open in repos.orders().list_open(OUTLET)? {
            orders.retain(|o| o.core().id != open.core().id);
            orders.push(open);
        }
        orders.retain(|o| matches!(o, mb_core::AnyOrder::Open(_) | mb_core::AnyOrder::Settled(_)) && o.core().billing.billed_into.is_none());
        let mut tables = repos.floor().list_tables(OUTLET)?;
        // An issued bill remains a candidate even after its old table is hidden.
        for table in &mut tables { table.is_active = true; }
        let sections = repos.floor().list_sections(OUTLET)?;
        let mut tiles = Vec::with_capacity(orders.len());
        for order in &orders {
            let candidates = crate::billing::floor_view(&tables, &sections, std::slice::from_ref(order),
                crate::billing::Room { cart_is_on: None, now: at, warn_after: 60, late_after: 120, config: &config });
            for mut tile in candidates.into_iter().filter(|tile| tile.order_id.is_some()) {
                // Multiple paid bills can have the same physical table. This is a list of
                // orders, so its identity must also be the order.
                tile.id = order.core().id.as_str().to_owned();
                tiles.push(tile);
            }
        }
        Ok(tiles)
    }).map_err(|e| words::from_db(&e)))
}

#[tauri::command]
pub fn release_serving_table(app: tauri::State<'_, App>, order_id: String) -> UiResult<()> {
    release_serving_table_on(&app, order_id)
}

pub fn release_serving_table_on(app: &App, order_id: String) -> UiResult<()> {
    let _one_at_a_time = app.begin_action();
    let who = guard::require(app, Permission::BillCreate)?;
    app.with_shop(|shop| shop.db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        let Some(mb_core::AnyOrder::Open(open)) = repos.orders().find_working(&mb_core::OrderId::new(&order_id))? else { return Err(mb_db::DbError::invariant("This table is no longer occupied.")); };
        let Some(root) = &open.core.billing.billed_into else { return Err(mb_db::DbError::invariant("Settle this table's bill first.")); };
        if !matches!(repos.orders().find_working(root)?, Some(mb_core::AnyOrder::Settled(_))) { return Err(mb_db::DbError::invariant("The combined bill is still to be paid.")); }
        let closed = open.cancel("Service complete; paid on combined bill", who.staff_id.clone(), now()).map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
        repos.orders().save_working(OUTLET, app.terminal_id(), &mb_core::AnyOrder::Cancelled(closed))?;
        repos.kitchen().close_order(&order_id)?;
        Ok(())
    }).map_err(|e| words::from_db(&e)))
}

/// What the screen sends to split an order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SplitRequest {
    pub order_id: String,
    /// `(line index, quantity as typed)` — the lines going to the new bill.
    pub lines: Vec<(usize, String)>,
    /// Where the new order sits.
    pub to_table: Option<String>,
    /// The letter for the new order when it stays at the same table.
    pub seat: Option<String>,
}

/// The unlettered party occupies A; subsequent parties take the first free letter.
pub(crate) fn next_free_seat(open: &[mb_core::AnyOrder], table: &TableId) -> Option<mb_core::SubTable> {
    ('B'..='Z').find_map(|letter| {
        let seat = mb_core::SubTable::parse(&letter.to_string()).ok()?;
        (!open.iter().any(|order| order.core().table() == Some(table)
            && order.core().seat() == Some(&seat))).then_some(seat)
    })
}

pub fn split_order_on(app: &App, request: SplitRequest) -> UiResult<FloorView> {
    let _one_at_a_time = app.begin_action();
    let who = guard::require(app, Permission::BillCreate)?;
    let at = now();

    if app.with_cart(|s| Ok(s.order_id() == Some(request.order_id.as_str())))? {
        crate::flows::park_open_order(app)?;
    }
    let order = open_order(app, &request.order_id)?;
    if order.core().billing.billed_into.is_some() || !order.core().billing.sources.is_empty() {
        return Err(UiError::new("split.combined", "Open the original orders before changing how this combined bill is split."));
    }
    if !order.core().billing.settlement.is_empty() {
        return Err(UiError::new("split.paid", "This bill has payments; finish its correction before splitting it."));
    }
    if let Some(refusal) = crate::dayclose::day_refusal_on(app, order.core().business_day, "split.closed", "split this bill")? { return Err(refusal); }
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

    let moved = mb_core::take_lines(&mut origin, &picks)
        .map_err(|e| UiError::new("floor.split", format!("That split is not possible: {e}")))?;

    let seat = request
        .seat
        .as_deref()
        .map(mb_core::SubTable::parse)
        .transpose()
        .map_err(|e| {
            UiError::new("floor.seat", "A seat is one letter, A to Z.").with_detail(e.to_string())
        })?;

    let day = order.core().business_day;
    let table = match &request.to_table {
        Some(id) => Some(TableId::new(id.clone())),
        None => order.core().table().cloned(),
    };
    let placement = match table {
        Some(table) => mb_core::Placement::DineIn { table, seat },
        None => order.core().placement.clone(),
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);

                let mut placement = placement.clone();
                if let mb_core::Placement::DineIn { table, seat } = &mut placement {
                    let open = repos.orders().list_open(OUTLET)?;
                    if !repos.floor().list_tables(OUTLET)?.iter().any(|row| &row.id == table && row.is_active) {
                        return Err(mb_db::DbError::invariant("Choose an active table for the split."));
                    }
                    if seat.is_none() && (order.core().table() == Some(table)
                        || open.iter().any(|order| order.core().table() == Some(table) && order.core().seat().is_none())) {
                        *seat = Some(next_free_seat(&open, table)
                            .ok_or_else(|| mb_db::DbError::invariant("All seat letters on that table are in use."))?);
                    }
                    if open.iter().any(|order| order.core().table() == Some(table) && order.core().seat() == seat.as_ref()) {
                        return Err(mb_db::DbError::invariant("That seat already has an order. Choose another letter."));
                    }
                }

                // What stays.
                repos.orders().assert_working(&order)?;
                let mut kept = order.clone();
                let (cart, kitchen) = origin.clone().into_parts();
                match &mut kept {
                    mb_core::AnyOrder::Open(o) => {
                        o.core.cart = cart;
                        o.core.kitchen = kitchen;
                    }
                    _ => {
                        return Err(mb_db::DbError::invariant("only an open order can be split"));
                    }
                }
                let discount = crate::flows::bill_of(app, &order).map_err(|e| mb_db::DbError::invariant(e.message))?.total_bill_discount;
                let nets = [&origin.cart, &moved.cart].iter().map(|cart| crate::billing::bill_for(cart, order.core().order_type(), None, &app.shop_config()).map(|b| b.subtotal.sub(b.total_line_discount).unwrap_or(mb_core::Money::ZERO))).collect::<Result<Vec<_>, _>>().map_err(|e| mb_db::DbError::invariant(e.message))?;
                let shares = mb_core::transfer::split_bill_discount(discount, &nets).map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
                kept.core_mut().billing.discount = shares[0].is_positive().then(|| mb_core::DiscountEntry::new(mb_core::Discount::Amount(shares[0])));
                repos.orders().save_working(OUTLET, app.terminal_id(), &kept)?;

                // And what leaves: a new order with its own numbers, claimed against the
                // ORIGINAL's business day so a split at 00:15 does not jump to tomorrow's
                // series.
                let (moved_cart, moved_kitchen) = moved.clone().into_parts();
                let mut fresh = mb_core::DraftOrder::new(
                    mb_core::OrderId::new(crate::newid::fresh_at("ord", at)),
                    day,
                    at,
                    placement.clone(),
                    who.staff_id.clone(),
                );
                fresh.core.cart = moved_cart;
                fresh.core.kitchen = moved_kitchen;
                fresh.core.billing.discount = shares[1].is_positive().then(|| mb_core::DiscountEntry::new(mb_core::Discount::Amount(shares[1])));

                // Its own token, claimed in THIS transaction so a failure cannot consume one —
                // the same rule `open_draft` follows, and claimed against the ORIGINAL order's
                // business day so a split at 00:15 does not jump to tomorrow. Its bill number
                // comes with its bill, like any other order's.
                let token = mb_db::numbering::claim(
                    tx,
                    OUTLET,
                    app.terminal_id(),
                    mb_db::numbering::CounterKind::Token,
                    day,
                )?;
                let opened = mb_core::OpenOrder {
                    core: fresh.core,
                    token,
                    bill_number: None,
                };
                let new_id = opened.core.id.as_str().to_owned();
                repos
                    .orders()
                    .save(OUTLET, app.terminal_id(), &mb_core::AnyOrder::Open(opened))?;

                crate::kitchen::split_in(&repos, &request.order_id, &new_id, &origin, &moved, day)?;

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
        if state.order_id() == Some(request.order_id.as_str()) {
            *state = crate::billing::CartState::default();
        }
        Ok(())
    })?;

    log_info!("{} split an order", who.name);
    floor_on(app)
}

// The seats.

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
pub fn delete_dining_table(app: tauri::State<'_, App>, table_id: String) -> UiResult<FloorView> {
    delete_table_on(&app, table_id)
}

#[tauri::command]
pub fn delete_dining_tables(
    app: tauri::State<'_, App>,
    table_ids: Vec<String>,
) -> UiResult<FloorChangeView> {
    delete_tables_on(&app, table_ids)
}

#[tauri::command]
pub fn set_dining_tables_active(
    app: tauri::State<'_, App>,
    table_ids: Vec<String>,
    active: bool,
) -> UiResult<FloorChangeView> {
    set_tables_active_on(&app, table_ids, active)
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
