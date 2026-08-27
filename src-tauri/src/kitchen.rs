//! The kitchen display.

use std::collections::BTreeMap;

use mb_auth::Permission;
use mb_core::kitchen_delivery::{Action, Delivery, State};
use mb_core::{AnyOrder, OrderId, Qty, Timestamp};
use serde::Serialize;
use ts_rs::TS;

use crate::flows::now;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

/// The station a shop has until it says otherwise.
pub const DEFAULT_STATION: &str = "Kitchen";

/// One dish on a card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct KitchenLine {
    /// What identifies this line within its order, for a per-item bump.
    pub key: String,
    pub qty: String,
    pub name: String,
    /// "extra spicy", "no onion" — the thing a cook must not miss.
    pub note: Option<String>,
    /// Which course this dish belongs to.
    pub course: String,
    /// Added after the kitchen first saw this order.
    pub is_new: bool,
    /// A cook has ticked this dish off.
    pub is_done: bool,
}

/// One firing of one order, as the kitchen sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct KitchenTicket {
    pub id: String,
    pub order_id: String,
    pub station: String,
    /// "Table 7", "Parcel" — what a cook shouts across the kitchen.
    pub place: String,
    pub token: String,
    pub waiter: Option<String>,
    /// Minutes since the counter told the kitchen.
    pub waiting_minutes: u32,
    /// Already a sentence: "4 min", "11 min".
    pub waiting: String,
    /// "12 min" — what this firing was expected to take, or empty.
    pub expected: String,
    /// `new`, `cooking`, `late`, `printed` or `cancelled`.
    pub tone: String,
    /// What the tone means, in words.
    pub says: String,
    /// Which course this firing is, or empty.
    pub course: String,
    pub lines: Vec<KitchenLine>,
    /// A cancellation nobody has acknowledged.
    pub is_cancelled: bool,
    /// True when this went to paper because no screen drew it in time.
    pub was_printed: bool,
}

/// A course that has not been sent to the kitchen yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct WaitingCourse {
    pub order_id: String,
    pub place: String,
    pub course: String,
    /// "2 dishes" — how much is waiting.
    pub what: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct KitchenView {
    pub station: String,
    /// Every station this shop has.
    pub stations: Vec<String>,
    pub tickets: Vec<KitchenTicket>,
    /// "3 orders waiting" — or that nothing is.
    pub headline: String,
    /// How many need somebody right now, for the title and the sound.
    pub late: u32,
    /// Courses a waiter can still fire.
    pub waiting_courses: Vec<WaitingCourse>,
    /// The last card cleared at this station, so it can be brought back.
    pub last_cleared: Option<Cleared>,
}

/// The card the "Bring back" button would return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct Cleared {
    pub id: String,
    /// "Table 5 #12" — enough to know it is the right one before pressing.
    pub what: String,
}

/// When a ticket turns late, if the dishes on it have no target of their own.
const LATE_AFTER_MINUTES: u32 = 15;

/// What the kitchen should be looking at.
#[must_use]
pub fn look(app: &App, station: &str) -> KitchenView {
    look_at(app, station, now())
}

/// `look`, at a stated time.
#[must_use]
pub fn look_at(app: &App, station: &str, at: Timestamp) -> KitchenView {
    let gathered = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let repos = mb_db::Repos::new(tx);
                    let tickets = repos.kitchen().outstanding(OUTLET, station)?;
                    let categories = repos.menu().list_categories(OUTLET)?;
                    let mut orders = BTreeMap::new();
                    for ticket in &tickets {
                        let id = OrderId::new(ticket.delivery.order_id.clone());
                        if let Some(order) = repos.orders().find(&id)? {
                            orders.insert(ticket.delivery.order_id.clone(), order);
                        }
                    }
                    // What can still be fired: every open order's courses that have no delivery
                    // yet.
                    let tables = repos.floor().list_tables(OUTLET)?;
                    let mut waiting = Vec::new();
                    for order in repos.orders().list_open(OUTLET)? {
                        let core = order.core();
                        let already = repos.kitchen().courses_fired(core.id.as_str())?;
                        for (course, count) in courses_of(&order) {
                            if course.is_empty() || already.covers(&course) {
                                continue;
                            }
                            waiting.push(WaitingCourse {
                                order_id: core.id.as_str().to_owned(),
                                place: place_and_token(&order, &tables),
                                what: words::count(count, "dish", "dishes"),
                                course,
                            });
                        }
                    }
                    let staff = repos.people().list_staff(OUTLET)?;
                    // The undo, and the order behind it so the bar can say which card it would
                    // bring back.
                    let cleared = repos.kitchen().last_bumped(OUTLET, station)?;
                    let cleared_order = match &cleared {
                        Some(t) => repos
                            .orders()
                            .find(&OrderId::new(t.delivery.order_id.clone()))?,
                        None => None,
                    };
                    Ok((
                        tickets,
                        orders,
                        categories,
                        waiting,
                        staff,
                        tables,
                        cleared.map(|t| (t, cleared_order)),
                    ))
                })
                .map_err(|e| words::from_db(&e))
        })
        .ok();

    let Some((tickets, orders, categories, waiting_courses, staff, tables, cleared)) = gathered
    else {
        return KitchenView {
            station: station.to_owned(),
            stations: vec![DEFAULT_STATION.to_owned()],
            tickets: Vec::new(),
            headline: "The shop's data could not be read. Print tickets on paper \
                       until this is sorted out."
                .to_owned(),
            late: 0,
            waiting_courses: Vec::new(),
            last_cleared: None,
        };
    };

    // Every station the shop has: the default, plus whatever categories name.
    let mut stations = vec![DEFAULT_STATION.to_owned()];
    for category in &categories {
        if let Some(name) = category.station.as_ref().filter(|s| !s.trim().is_empty())
            && !stations.iter().any(|s| s == name)
        {
            stations.push(name.clone());
        }
    }

    let cards: Vec<KitchenTicket> = tickets
        .iter()
        .map(|ticket| {
            let order = orders.get(&ticket.delivery.order_id);
            card(ticket, order, &staff, &tables, at)
        })
        .collect();

    let late = u32::try_from(cards.iter().filter(|c| c.tone == "late").count()).unwrap_or(0);
    let waiting = cards.len();

    KitchenView {
        station: station.to_owned(),
        stations,
        headline: if waiting == 0 {
            "Nothing waiting.".to_owned()
        } else {
            format!(
                "{} waiting.",
                words::count(i64::try_from(waiting).unwrap_or(0), "order", "orders")
            )
        },
        late,
        tickets: cards,
        waiting_courses,
        last_cleared: cleared.map(|(ticket, order)| {
            let place = order
                .as_ref()
                .map_or_else(String::new, |o| place_of(o, &tables));
            let token = order
                .as_ref()
                .and_then(AnyOrder::token)
                .map_or_else(String::new, |t| format!(" #{}", t.formatted));
            Cleared {
                id: ticket.delivery.id.clone(),
                // "Table 5 #12", or just the number if the order has gone — enough to know it
                // is the right one before pressing.
                what: match (place.is_empty(), token.is_empty()) {
                    (true, true) => "the last one".to_owned(),
                    _ => format!("{place}{token}").trim().to_owned(),
                },
            }
        }),
    }
}

/// "Table 7", "Parcel" — what a cook shouts across the kitchen.
fn place_of(order: &AnyOrder, tables: &[mb_db::repo::floor::DiningTable]) -> String {
    let core = order.core();
    core.table().map_or_else(
        || crate::billing::order_type_label(core.order_type()).to_owned(),
        |table| {
            let label = tables
                .iter()
                .find(|t| &t.id == table)
                .map_or_else(|| table.as_str().to_owned(), |t| t.label.clone());
            format!("Table {label}")
        },
    )
}

/// "Table 7 #12", "Parcel #13" — the place and the token, the way the counter says it.
pub(crate) fn place_and_token(
    order: &AnyOrder,
    tables: &[mb_db::repo::floor::DiningTable],
) -> String {
    match order.token() {
        Some(t) => format!("{} #{}", place_of(order, tables), t.formatted),
        None => place_of(order, tables),
    }
}

/// Each course on an order, and how many dishes are in it.
fn courses_of(order: &AnyOrder) -> Vec<(String, i64)> {
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for line in order.core().cart.lines() {
        let course = line.snapshot.course.clone().unwrap_or_default();
        *counts.entry(course).or_insert(0) += 1;
    }
    counts.into_iter().collect()
}

#[allow(
    clippy::integer_division,
    reason = "minutes from milliseconds, on a wall clock and not on money"
)]
fn card(
    ticket: &mb_db::repo::kitchen::Ticket,
    order: Option<&AnyOrder>,
    staff: &[mb_db::repo::people::StaffMember],
    tables: &[mb_db::repo::floor::DiningTable],
    at: Timestamp,
) -> KitchenTicket {
    let waited_ms = at
        .millis()
        .saturating_sub(ticket.delivery.sent_at.millis())
        .max(0);
    let minutes = u32::try_from(waited_ms / 60_000).unwrap_or(0);
    let target = ticket.expected_minutes.unwrap_or(LATE_AFTER_MINUTES);

    let (tone, says) = if ticket.needs_acknowledging() {
        ("cancelled", "CANCELLED — tell the cook".to_owned())
    } else if ticket.delivery.state == State::Printed {
        ("printed", "Already printed on paper".to_owned())
    } else if minutes >= target {
        ("late", "Late".to_owned())
    } else if minutes < 1 {
        ("new", "New".to_owned())
    } else {
        ("cooking", "Cooking".to_owned())
    };

    // The lines, from the ORDER — and which of them the kitchen had not seen when this firing
    // went out, which is what makes an addition obvious.
    let lines = order.map_or_else(Vec::new, |order| {
        let core = order.core();
        let told = core.kitchen.told();
        core.cart
            .lines()
            .iter()
            .filter(|line| {
                // A firing shows only its own course, so firing the mains does not re-show the
                // starters.
                match ticket.course.as_deref() {
                    None | Some("") => true,
                    Some(course) => line.snapshot.course.as_deref() == Some(course),
                }
            })
            .map(|line| {
                let identity = line.identity();
                let key = line_key(&identity);
                // From the ledger, not from timestamps.
                let seen = told
                    .iter()
                    .find(|(id, _)| id == &identity)
                    .map_or(Qty::ZERO, |(_, qty)| *qty);
                KitchenLine {
                    is_new: seen < line.qty,
                    is_done: ticket.bumped_lines.iter().any(|k| k == &key),
                    key,
                    qty: line.qty.to_string(),
                    name: line.snapshot.name.clone(),
                    note: identity.note.clone(),
                    course: line.snapshot.course.clone().unwrap_or_default(),
                }
            })
            .collect()
    });

    let (place, token, waiter) = order.map_or_else(
        || (String::new(), String::new(), None),
        |order| {
            let core = order.core();
            (
                place_of(order, tables),
                order
                    .token()
                    .map(|t| t.formatted.clone())
                    .unwrap_or_default(),
                staff
                    .iter()
                    .find(|s| s.id == core.created_by)
                    .map(|s| s.name.clone()),
            )
        },
    );

    KitchenTicket {
        id: ticket.delivery.id.clone(),
        order_id: ticket.delivery.order_id.clone(),
        station: ticket.delivery.station.clone(),
        place,
        token,
        waiter,
        waiting_minutes: minutes,
        waiting: format!("{minutes} min"),
        expected: ticket
            .expected_minutes
            .map(|m| format!("{m} min"))
            .unwrap_or_default(),
        tone: tone.to_owned(),
        says,
        course: ticket.course.clone().unwrap_or_default(),
        lines,
        is_cancelled: ticket.needs_acknowledging(),
        was_printed: ticket.delivery.state == State::Printed,
    }
}

/// How a line is named for a per-item bump.
fn line_key(identity: &mb_core::LineIdentity) -> String {
    match &identity.note {
        Some(note) if !note.is_empty() => {
            format!("{}|{note}", identity.item_id.as_str())
        }
        _ => identity.item_id.as_str().to_owned(),
    }
}

// What a cook can do.

pub fn look_on(app: &App, station: Option<String>) -> UiResult<KitchenView> {
    crate::guard::require(app, Permission::BillCreate)?;
    Ok(look(app, station.as_deref().unwrap_or(DEFAULT_STATION)))
}

/// The screen drew it.
pub fn shown_on(app: &App, id: String) -> UiResult<KitchenView> {
    crate::guard::require(app, Permission::BillCreate)?;
    let at = now();
    let station = with_ticket(app, &id, |ticket| {
        // A late ack on an already-printed ticket is REFUSED, and that refusal is the point —
        // the paper is in the kitchen already.
        let _ = ticket.delivery.shown(at);
        Ok(ticket.delivery.station.clone())
    })?;
    Ok(look(app, &station))
}

/// A cook cleared the whole card.
pub fn bump_on(app: &App, id: String) -> UiResult<KitchenView> {
    let who = crate::guard::require(app, Permission::BillCreate)?;
    let at = now();
    let station = with_ticket(app, &id, |ticket| {
        ticket
            .delivery
            .bump(at)
            .map_err(|e| UiError::new("kitchen.not_shown", format!("{e}.")))?;
        ticket.bumped_by = Some(who.staff_id.clone());
        Ok(ticket.delivery.station.clone())
    })?;
    crate::log_info!("kitchen: {id} was cleared by {}", who.name);
    Ok(look(app, &station))
}

/// A cook ticked off one dish.
pub fn bump_line_on(app: &App, id: String, key: String) -> UiResult<KitchenView> {
    crate::guard::require(app, Permission::BillCreate)?;
    let station = with_ticket(app, &id, |ticket| {
        // Toggling, not just adding: a cook who ticks the wrong dish presses it again.
        if let Some(index) = ticket.bumped_lines.iter().position(|k| k == &key) {
            ticket.bumped_lines.remove(index);
        } else {
            ticket.bumped_lines.push(key.clone());
        }
        Ok(ticket.delivery.station.clone())
    })?;
    Ok(look(app, &station))
}

/// A cook bumped the wrong ticket.
pub fn recall_on(app: &App, id: String) -> UiResult<KitchenView> {
    let who = crate::guard::require(app, Permission::BillCreate)?;
    let station = with_ticket(app, &id, |ticket| {
        ticket
            .delivery
            .recall()
            .map_err(|e| UiError::new("kitchen.not_bumped", format!("{e}.")))?;
        ticket.bumped_by = None;
        Ok(ticket.delivery.station.clone())
    })?;
    crate::log_info!("kitchen: {id} was brought back by {}", who.name);
    Ok(look(app, &station))
}

/// Somebody pressed "Got it" on a cancellation.
pub fn acknowledge_on(app: &App, id: String) -> UiResult<KitchenView> {
    let who = crate::guard::require(app, Permission::BillCreate)?;
    let at = now();
    let station = with_ticket(app, &id, |ticket| {
        ticket.acked_at = Some(at);
        // Seen, so it leaves the screen.
        ticket.delivery.close();
        Ok(ticket.delivery.station.clone())
    })?;
    crate::log_info!(
        "kitchen: {} acknowledged the cancellation of {id}",
        who.name
    );
    Ok(look(app, &station))
}

/// Fire a course — send the next part of an order to the kitchen when the table is ready for
/// it.
pub fn fire_on(app: &App, order_id: String, course: String) -> UiResult<KitchenView> {
    let who = crate::guard::require(app, Permission::BillCreate)?;

    // The kitchen is never told twice.
    let already = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).kitchen().courses_fired(&order_id))
            .map_err(|e| words::from_db(&e))
    })?;
    if already.covers(&course) {
        return Err(UiError::new(
            "kitchen.already_fired",
            format!("The kitchen already has the {course} for this table."),
        )
        .quietly());
    }

    let station = send(app, &order_id, Some(&course))?;
    crate::log_bill!(order_id, "{course} was fired by {}", who.name);
    Ok(look(app, &station))
}

/// Read a ticket, change it, and write it back — in one transaction.
fn with_ticket<T>(
    app: &App,
    id: &str,
    change: impl FnOnce(&mut mb_db::repo::kitchen::Ticket) -> UiResult<T>,
) -> UiResult<T> {
    let taken = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mut ticket) = repos.kitchen().get(id)? else {
                    return Ok(None);
                };
                match change(&mut ticket) {
                    Ok(value) => {
                        repos.kitchen().save(&ticket)?;
                        Ok(Some(Ok(value)))
                    }
                    Err(e) => Ok(Some(Err(e))),
                }
            })
            .map_err(|e| words::from_db(&e))
    })?;
    match taken {
        Some(result) => result,
        None => Err(UiError::new(
            "kitchen.gone",
            "That ticket is no longer on this screen. Refresh and try again.",
        )),
    }
}

/// Send an order to the kitchen screen.
pub fn send(app: &App, order_id: &str, course: Option<&str>) -> UiResult<String> {
    let at = now();
    let config = app.shop_config();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| send_in(&mb_db::Repos::new(tx), &config, order_id, course, at))
            .map_err(|e| words::from_db(&e))
    })
}

/// The same, inside a transaction somebody else opened. A shop with no screen gets no
/// screen ticket: the paper the counter printed is the whole story.
pub fn send_in(
    repos: &mb_db::Repos<'_>,
    config: &crate::settings::ShopConfig,
    order_id: &str,
    course: Option<&str>,
    at: Timestamp,
) -> Result<String, mb_db::DbError> {
    if !config.billing.kitchen_screen {
        return Ok(DEFAULT_STATION.to_owned());
    }
    let id = format!("{}_{order_id}", crate::newid::fresh_at("kds", at));
    let order = repos.orders().find(&OrderId::new(order_id.to_owned()))?;
    let categories = repos.menu().list_categories(OUTLET)?;

    // The station comes from the CATEGORY of the food.
    let mut by_station: BTreeMap<String, Option<u32>> = BTreeMap::new();
    if let Some(order) = &order {
        for line in order.core().cart.lines() {
            if let Some(wanted) = course
                && line.snapshot.course.as_deref() != Some(wanted)
            {
                continue;
            }
            let station = line
                .snapshot
                .station
                .clone()
                .or_else(|| {
                    line.snapshot.category_id.as_ref().and_then(|c| {
                        categories
                            .iter()
                            .find(|cat| &cat.id == c)
                            .and_then(|cat| cat.station.clone())
                    })
                })
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_STATION.to_owned());
            // The target is the SLOWEST dish: the order is ready when the last
            // thing on it is.
            let slowest = by_station.entry(station).or_insert(None);
            if let Some(minutes) = line.snapshot.prep_minutes {
                *slowest = Some(slowest.unwrap_or(0).max(minutes));
            }
        }
    }
    if by_station.is_empty() {
        by_station.insert(DEFAULT_STATION.to_owned(), None);
    }

    let mut first = DEFAULT_STATION.to_owned();
    for (index, (station, expected)) in by_station.iter().enumerate() {
        if index == 0 {
            first = station.clone();
        }
        let delivery = Delivery::new(&format!("{id}_{station}"), order_id, station, at);
        // The order's OWN business day, not today's.
        let day = order
            .as_ref()
            .map_or_else(|| crate::flows::today(at), |o| o.core().business_day);
        repos
            .kitchen()
            .send(OUTLET, &delivery, course, *expected, day)?;
    }
    Ok(first)
}

/// The paper fallback. Called on a timer by `main`.
pub fn print_what_nobody_drew(app: &App) -> u32 {
    print_what_nobody_drew_at(app, now())
}

/// `print_what_nobody_drew`, at a stated time — see `look_at` for why the clock is an argument.
pub fn print_what_nobody_drew_at(app: &App, at: Timestamp) -> u32 {
    // This runs on its own thread every five seconds, so it is the one caller that can
    // genuinely collide with a cashier.
    let _one_at_a_time = app.begin_action();

    let overdue = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| mb_db::Repos::new(tx).kitchen().awaiting_ack(OUTLET))
                .map_err(|e| words::from_db(&e))
        })
        .unwrap_or_default();

    let mut printed = 0;
    for ticket in overdue {
        if ticket
            .delivery
            .decide(at, mb_core::kitchen_delivery::ACK_SECONDS)
            != Action::PrintNow
        {
            continue;
        }
        // The paper first, then the mark.
        let sent = crate::flows::print_kitchen_ticket_for(app, &ticket.delivery.order_id);

        // An empty id means there was nothing left to send, because the counter had already put
        // this food on paper when the button was pressed.
        let nothing_left_to_send = matches!(&sent, Ok(id) if id.is_empty());

        let outcome = with_ticket(app, &ticket.delivery.id.clone(), |t| {
            t.delivery.printed();
            Ok(())
        });

        // And when the mark fails, say so.
        if let Err(e) = &outcome {
            crate::log_warn!(
                "order={} went to paper but could not be marked as printed ({e}). \
                 It will be tried again in a few seconds.",
                ticket.delivery.order_id,
            );
            continue;
        }

        if nothing_left_to_send {
            crate::log_info!(
                "order={} no kitchen screen drew this, but the counter had already \
                 printed it — nothing more was sent.",
                ticket.delivery.order_id,
            );
            continue;
        }

        printed += 1;
        crate::log_warn!(
            "order={} no kitchen screen drew this in time, so it went to paper \
             ({}). Check the screen at the {} station.",
            ticket.delivery.order_id,
            if sent.is_ok() {
                "printed"
            } else {
                "and the printer refused too"
            },
            ticket.delivery.station,
        );
    }
    printed
}

/// Watch for tickets nobody drew.
pub fn watch_for_undrawn_tickets(handle: &tauri::AppHandle) {
    use tauri::Manager as _;

    let ticking = handle.clone();
    std::thread::Builder::new()
        .name("mb-kitchen-fallback".to_owned())
        .spawn(move || {
            loop {
                // Well under `ACK_SECONDS`, so a ticket goes to paper close to the deadline
                // rather than up to a deadline late.
                std::thread::sleep(std::time::Duration::from_secs(5));
                let Some(app) = ticking.try_state::<App>() else {
                    return;
                };
                let printed = print_what_nobody_drew(&app);
                if printed > 0 {
                    // The counter says so out loud.
                    crate::push::emit_print_queue(&ticking);
                }
            }
        })
        .ok();
}

// The seats.

#[tauri::command]
pub fn kitchen(app: tauri::State<'_, App>, station: Option<String>) -> UiResult<KitchenView> {
    look_on(&app, station)
}

#[tauri::command]
pub fn kitchen_shown(app: tauri::State<'_, App>, id: String) -> UiResult<KitchenView> {
    shown_on(&app, id)
}

#[tauri::command]
pub fn kitchen_bump(app: tauri::State<'_, App>, id: String) -> UiResult<KitchenView> {
    bump_on(&app, id)
}

#[tauri::command]
pub fn kitchen_bump_line(
    app: tauri::State<'_, App>,
    id: String,
    key: String,
) -> UiResult<KitchenView> {
    bump_line_on(&app, id, key)
}

#[tauri::command]
pub fn kitchen_recall(app: tauri::State<'_, App>, id: String) -> UiResult<KitchenView> {
    recall_on(&app, id)
}

#[tauri::command]
pub fn kitchen_acknowledge(app: tauri::State<'_, App>, id: String) -> UiResult<KitchenView> {
    acknowledge_on(&app, id)
}

#[tauri::command]
pub fn kitchen_fire(
    app: tauri::State<'_, App>,
    order_id: String,
    course: String,
) -> UiResult<KitchenView> {
    fire_on(&app, order_id, course)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_ticket(state: State, sent_minutes_ago: i64) -> mb_db::repo::kitchen::Ticket {
        mb_db::repo::kitchen::Ticket {
            delivery: Delivery {
                id: "kds_1".to_owned(),
                order_id: "ord_1".to_owned(),
                station: DEFAULT_STATION.to_owned(),
                state,
                sent_at: Timestamp::from_millis(-sent_minutes_ago * 60_000),
                shown_at: None,
                bumped_at: None,
            },
            course: None,
            expected_minutes: None,
            bumped_by: None,
            bumped_on: None,
            bumped_lines: Vec::new(),
            cancelled_at: None,
            acked_at: None,
        }
    }

    fn drawn(ticket: &mb_db::repo::kitchen::Ticket) -> KitchenTicket {
        card(ticket, None, &[], &[], Timestamp::from_millis(0))
    }

    /// The states a cook has to tell apart from two metres.
    #[test]
    fn a_card_says_what_it_is_in_words_and_not_only_in_colour() {
        assert_eq!(drawn(&a_ticket(State::Pending, 0)).tone, "new");
        assert_eq!(drawn(&a_ticket(State::Shown, 5)).tone, "cooking");
        assert_eq!(drawn(&a_ticket(State::Shown, 20)).tone, "late");
        assert_eq!(drawn(&a_ticket(State::Printed, 2)).tone, "printed");

        for minutes in [0_i64, 5, 20] {
            let card = drawn(&a_ticket(State::Shown, minutes));
            assert!(!card.says.is_empty());
            assert!(!card.waiting.is_empty());
        }
    }

    /// A cancellation beats everything else on the card, including late.
    #[test]
    fn an_unacknowledged_cancellation_is_the_loudest_thing_on_the_screen() {
        let mut ticket = a_ticket(State::Shown, 30);
        ticket.cancelled_at = Some(Timestamp::from_millis(-60_000));
        let card = drawn(&ticket);
        assert_eq!(card.tone, "cancelled", "late won over a cancellation");
        assert!(card.is_cancelled);
        assert!(card.says.contains("CANCELLED"));

        // Once somebody presses "Got it", it stops shouting.
        ticket.acked_at = Some(Timestamp::from_millis(-30_000));
        let acked = drawn(&ticket);
        assert!(!acked.is_cancelled);
        assert_ne!(acked.tone, "cancelled");
    }

    #[test]
    fn a_printed_ticket_is_marked_rather_than_hidden() {
        let card = drawn(&a_ticket(State::Printed, 1));
        assert!(card.was_printed);
        assert_eq!(card.tone, "printed");
        assert!(card.says.contains("paper"));
    }

    /// The minutes are computed in Rust.
    #[test]
    fn the_screen_is_given_minutes_and_a_sentence() {
        let card = drawn(&a_ticket(State::Shown, 11));
        assert_eq!(card.waiting_minutes, 11);
        assert_eq!(card.waiting, "11 min");
    }

    /// The item's own prep time decides late, not one number for everything.
    #[test]
    fn a_dish_with_its_own_target_uses_it() {
        let mut quick = a_ticket(State::Shown, 6);
        quick.expected_minutes = Some(4);
        assert_eq!(drawn(&quick).tone, "late", "a 4-minute dish at 6 minutes");
        assert_eq!(drawn(&quick).expected, "4 min");

        let mut slow = a_ticket(State::Shown, 6);
        slow.expected_minutes = Some(20);
        assert_eq!(
            drawn(&slow).tone,
            "cooking",
            "a 20-minute dish at 6 minutes"
        );
    }

    /// A menu nobody has costed still gets a useful screen.
    #[test]
    fn a_dish_with_no_target_falls_back_to_a_named_number() {
        let at = Timestamp::from_millis(0);
        assert_eq!(
            card(
                &a_ticket(State::Shown, i64::from(LATE_AFTER_MINUTES) - 1),
                None,
                &[],
                &[],
                at
            )
            .tone,
            "cooking"
        );
        assert_eq!(
            card(
                &a_ticket(State::Shown, i64::from(LATE_AFTER_MINUTES)),
                None,
                &[],
                &[],
                at
            )
            .tone,
            "late"
        );
    }

    /// Two lines of the same dish are two jobs when one has a note.
    #[test]
    fn a_note_makes_a_line_its_own_job() {
        let plain = mb_core::LineIdentity {
            item_id: mb_core::ItemId::new("itm_paneer"),
            note: None,
            modifier_ids: Vec::new(),
        };
        let spicy = mb_core::LineIdentity {
            item_id: mb_core::ItemId::new("itm_paneer"),
            note: Some("extra spicy".to_owned()),
            modifier_ids: Vec::new(),
        };
        assert_ne!(
            line_key(&plain),
            line_key(&spicy),
            "ticking one would have ticked the other"
        );
        assert_eq!(line_key(&plain), "itm_paneer");
    }
}
