//! **Orders that leave on a bike** — P29, scope 14.5.
//!
//! # Where the pieces live
//!
//! | | |
//! |---|---|
//! | the state machine | [`mb_db::repo::delivery::DeliveryState`] |
//! | the rows and the arithmetic | [`mb_db::repo::delivery`] |
//! | the slip | [`mb_print::template::delivery`] |
//! | this file | the commands, the permission boundary, and the words |
//!
//! # The two facts a delivery screen has to keep apart
//!
//! An order has a state (`open`, `settled`) and a delivery has a state
//! (`out`, `delivered`). They are not the same fact and they do not move
//! together:
//!
//! * paid online, still on the road — settled, `out`;
//! * handed over at the door for cash — `delivered`, and the bill is settled
//!   at that moment or when the rider gets back;
//! * nobody was home — `failed`, and the bill is whatever it was.
//!
//! **The money question is the second column, never the first.**
//!
//! # What a rider is carrying, and why it is not stored
//!
//! ```text
//!   cash on his delivered orders today  −  what he has handed back today
//! ```
//!
//! Computed from rows every time (D120). A running figure on a person is a
//! number that can disagree with the rows that made it, and on the evening it
//! does, nobody can tell which one is lying — least of all the rider, who is
//! the person being accused.
//!
//! [`mb_db::repo::money::MoneyRepo::cash_position`] subtracts the same figure,
//! so the drawer stops being short all evening for a reason nobody can name.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use mb_auth::AuditEntry;
use mb_auth::Permission;
use mb_auth::audit::action;
use mb_core::businessday::BusinessDay;
use mb_core::money::Money;
use mb_db::repo::delivery::{Delivery, DeliveryState, RiderDay};

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

fn money(m: Money) -> MoneyView {
    MoneyView::from(m)
}

// ===========================================================================
// The view models
// ===========================================================================

/// One delivery, as the board shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DeliveryView {
    pub order_id: String,
    /// The bill number, or "not billed yet".
    pub reference: String,
    pub customer: String,
    pub phone: String,
    pub address: String,
    pub rider_id: String,
    pub rider_name: String,
    /// The tag: `pending`, `assigned`, `out`, `delivered`, `failed`.
    pub state: String,
    /// The same thing in words — "On the way".
    pub state_says: String,
    /// Why it did not arrive, when it did not.
    pub failure: String,
    pub total: MoneyView,
    /// What the rider has to collect at the door. Zero when it is paid.
    pub collect: MoneyView,
    /// True when the bill is settled — **and this is not the same as
    /// delivered**, which is the whole point of the two columns.
    pub paid: bool,
    /// What the board actually prints in the money column: "Collect ₹640.00",
    /// or "Paid".
    pub money_says: String,
}

/// One rider's evening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RiderDayView {
    pub id: String,
    pub name: String,
    pub out: u32,
    pub delivered: u32,
    pub failed: u32,
    pub collected: MoneyView,
    pub handed_back: MoneyView,
    pub carrying: MoneyView,
    /// "Carrying ₹900.00" or "Nothing outstanding" — the sentence the owner
    /// reads at eleven o'clock.
    pub says: String,
}

/// Somebody who could take an order out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RiderView {
    pub id: String,
    pub name: String,
}

/// The whole screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DeliveryBoardView {
    /// `2026-08-16`.
    pub day: String,
    pub deliveries: Vec<DeliveryView>,
    pub riders: Vec<RiderDayView>,
    /// Everybody flagged as a rider, for the assign menu.
    pub all_riders: Vec<RiderView>,
    /// The sum of what every rider is carrying.
    pub carrying: MoneyView,
    /// The headline sentence, in the shop's words.
    pub says: String,
    /// True when the signed-in person may dispatch. The screen still SHOWS the
    /// board to anybody signed in — reading where the food is is not a secret.
    pub may_dispatch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DeliveryEdit {
    pub order_id: String,
    pub address: String,
    pub customer_id: String,
    pub rider_id: String,
    /// One of the five state tags.
    pub state: String,
    pub failure: String,
}

// ===========================================================================
// Reading the board
// ===========================================================================

fn state_from_tag(tag: &str) -> UiResult<DeliveryState> {
    match tag {
        "pending" => Ok(DeliveryState::Pending),
        "assigned" => Ok(DeliveryState::Assigned),
        "out" => Ok(DeliveryState::Out),
        "delivered" => Ok(DeliveryState::Delivered),
        "failed" => Ok(DeliveryState::Failed),
        other => Err(UiError::new(
            "delivery.state",
            format!("\"{other}\" is not a delivery state."),
        )),
    }
}

fn blank_to_none(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn view_of(row: &Delivery) -> DeliveryView {
    let paid = row.order_state == "settled";
    // **What is left to collect at the door.** The bill total less what has
    // already been paid against it — and a settled bill has nothing left,
    // whichever way it was paid.
    let collect = if paid {
        Money::ZERO
    } else {
        row.total
    };
    let money_says = if paid {
        "Paid".to_owned()
    } else if collect == Money::ZERO {
        "Nothing to collect".to_owned()
    } else {
        format!("Collect {}", collect.to_indian_string())
    };

    DeliveryView {
        order_id: row.order_id.clone(),
        reference: row
            .bill_number
            .clone()
            .unwrap_or_else(|| "Not billed yet".to_owned()),
        customer: row.customer_name.clone().unwrap_or_default(),
        phone: row.phone.clone().unwrap_or_default(),
        address: row.address.clone().unwrap_or_default(),
        rider_id: row.rider_id.clone().unwrap_or_default(),
        rider_name: row.rider_name.clone().unwrap_or_default(),
        state: row.state.as_sql().to_owned(),
        state_says: row.state.words().to_owned(),
        failure: row.failure.clone().unwrap_or_default(),
        total: money(row.total),
        collect: money(collect),
        paid,
        money_says,
    }
}

fn rider_view_of(row: &RiderDay) -> RiderDayView {
    let says = if row.carrying == Money::ZERO {
        "Nothing outstanding".to_owned()
    } else {
        format!("Carrying {}", row.carrying.to_indian_string())
    };
    RiderDayView {
        id: row.rider_id.clone(),
        name: row.rider_name.clone(),
        out: crate::ipc::count(row.out),
        delivered: crate::ipc::count(row.delivered),
        failed: crate::ipc::count(row.failed),
        collected: money(row.collected),
        handed_back: money(row.handed_back),
        carrying: money(row.carrying),
        says,
    }
}

pub fn board_on(app: &App, day: Option<String>) -> UiResult<DeliveryBoardView> {
    // Reading the board needs no permission beyond being signed in: knowing
    // where the food is is the floor's business, and a counter that has to ask
    // the owner to see it is a counter that keeps the list on paper instead.
    let who = app
        .sessions()
        .current()
        .ok_or_else(|| UiError::new("auth.locked", "The screen is locked. Sign in to carry on."))?
        .actor;
    let at = now();
    let day = match day {
        Some(text) if !text.trim().is_empty() => parse_day(&text)?,
        _ => today(at),
    };

    let config = app.shop_config();
    let (rows, riders, all_riders) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let d = repos.delivery();
                let mut rows = d.deliveries_on(OUTLET, day)?;
                // **An order that has not been settled has no bill row.**
                //
                // Without this the board shows a rider 0.00 and "nothing to
                // collect" on an order nobody has paid for — which is the
                // single most expensive thing this screen could get wrong.
                // The figure comes from the same function the floor tile uses,
                // so the two cannot disagree about what an open order is worth.
                for row in &mut rows {
                    if row.order_state != "settled"
                        && let Some(order) =
                            repos.orders().find(&mb_core::OrderId::new(row.order_id.clone()))?
                        && let Some(total) = crate::billing::running_total(&order, &config)
                    {
                        row.total = Money::from_paise(total.paise);
                    }
                }
                Ok((rows, d.rider_day(OUTLET, day)?, d.riders(OUTLET)?))
            })
            .map_err(|e| words::from_db(&e))
    })?;

    let carrying = riders
        .iter()
        .try_fold(Money::ZERO, |sum, r| sum.add(r.carrying))
        .unwrap_or(Money::ZERO);

    let out_now = rows
        .iter()
        .filter(|r| matches!(r.state, DeliveryState::Out | DeliveryState::Assigned))
        .count();
    let says = match (out_now, carrying == Money::ZERO) {
        (0, true) => "Everything is back.".to_owned(),
        (0, false) => format!("{} is still with the riders.", carrying.to_indian_string()),
        (n, true) => format!("{n} on the road, no cash outstanding."),
        (n, false) => format!(
            "{n} on the road, {} with the riders.",
            carrying.to_indian_string()
        ),
    };

    Ok(DeliveryBoardView {
        day: day.to_string(),
        deliveries: rows.iter().map(view_of).collect(),
        riders: riders.iter().map(rider_view_of).collect(),
        all_riders: all_riders
            .into_iter()
            .map(|(id, name)| RiderView { id, name })
            .collect(),
        carrying: money(carrying),
        says,
        may_dispatch: who.can(Permission::DeliveryDispatch),
    })
}

fn parse_day(text: &str) -> UiResult<BusinessDay> {
    let parts: Vec<&str> = text.trim().split('-').collect();
    let bad = || {
        UiError::new(
            "delivery.day",
            "Type the date as 2026-08-16 — the year, the month, then the day.",
        )
    };
    if parts.len() != 3 {
        return Err(bad());
    }
    let year: i32 = parts[0].parse().map_err(|_| bad())?;
    let month: u32 = parts[1].parse().map_err(|_| bad())?;
    let dom: u32 = parts[2].parse().map_err(|_| bad())?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&dom) {
        return Err(bad());
    }
    Ok(BusinessDay::from_ymd(year, month, dom))
}

// ===========================================================================
// Moving a delivery along
// ===========================================================================

pub fn save_delivery_on(app: &App, edit: DeliveryEdit) -> UiResult<DeliveryBoardView> {
    let who = guard::require(app, Permission::DeliveryDispatch)?;
    let at = now();
    let day = today(at);
    let state = state_from_tag(&edit.state)?;

    // A rider is compulsory from `assigned` onwards. The screen can offer the
    // step, but the counter is not allowed to send food out with nobody.
    let rider = blank_to_none(&edit.rider_id);
    if rider.is_none()
        && matches!(
            state,
            DeliveryState::Assigned | DeliveryState::Out | DeliveryState::Delivered
        )
    {
        return Err(UiError::new(
            "delivery.rider",
            "Choose who is taking it.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.delivery().set_delivery(
                    OUTLET,
                    &edit.order_id,
                    blank_to_none(&edit.address).as_deref(),
                    blank_to_none(&edit.customer_id).as_deref(),
                    rider.as_deref(),
                    state,
                    blank_to_none(&edit.failure).as_deref(),
                )?;

                // R11 — the audit row is in the SAME transaction as the change.
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::DELIVERY_SET,
                        "order",
                    )
                    .about(edit.order_id.clone())
                    .with_after(serde_json::json!({
                        "state": state.as_sql(),
                        "rider": rider,
                        "failure": edit.failure,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    board_on(app, None)
}

// ===========================================================================
// The money coming back
// ===========================================================================

pub fn record_handback_on(
    app: &App,
    rider_id: String,
    amount: String,
    note: String,
) -> UiResult<DeliveryBoardView> {
    let who = guard::require(app, Permission::DeliveryDispatch)?;
    let at = now();
    let day = today(at);

    // **The screen sends what was typed** and Rust parses it (D39). One money
    // parser in the product, and JavaScript is not allowed near it.
    let amount = crate::menu::parse_money_public(&amount)?;
    if amount <= Money::ZERO {
        return Err(UiError::new(
            "delivery.handback",
            "Type how much the rider handed over.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let d = repos.delivery();

                // **Before and after, in the audit row.** One permission covers
                // dispatching and receipting, and this is the control that
                // makes that safe: a handback that did not happen is visible as
                // a rider whose carrying figure dropped with nobody at the
                // till.
                let before = d
                    .rider_day(OUTLET, day)?
                    .into_iter()
                    .find(|r| r.rider_id == rider_id)
                    .map(|r| r.carrying)
                    .unwrap_or(Money::ZERO);

                d.record_handback(
                    OUTLET,
                    &format!("hbk_{}", at.millis()),
                    &rider_id,
                    amount,
                    at,
                    day,
                    Some(who.staff_id.as_str()),
                    blank_to_none(&note).as_deref(),
                )?;

                let after = d
                    .rider_day(OUTLET, day)?
                    .into_iter()
                    .find(|r| r.rider_id == rider_id)
                    .map(|r| r.carrying)
                    .unwrap_or(Money::ZERO);

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::RIDER_HANDBACK,
                        "staff",
                    )
                    .about(rider_id.clone())
                    .changed(
                        serde_json::json!({ "carrying": before.paise() }),
                        serde_json::json!({
                            "carrying": after.paise(),
                            "handed_back": amount.paise(),
                            "note": note,
                        }),
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    board_on(app, None)
}

/// Say that somebody does, or does not, take orders out.
///
/// `StaffManage` and not `DeliveryDispatch`: this edits a person's record, and
/// who is on the shop's staff is the same decision whether it is about riding
/// or about anything else.
pub fn set_rider_on(app: &App, staff_id: String, is_rider: bool) -> UiResult<DeliveryBoardView> {
    let who = guard::require(app, Permission::StaffManage)?;
    let at = now();
    let day = today(at);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.delivery().set_rider_flag(OUTLET, &staff_id, is_rider, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::STAFF_SAVED,
                        "staff",
                    )
                    .about(staff_id.clone())
                    .with_after(serde_json::json!({ "is_rider": is_rider })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    board_on(app, None)
}

// ===========================================================================
// The paper
// ===========================================================================

/// The slip that goes out with the rider.
///
/// **A printer that is missing must not stop a delivery** (the rule for this
/// whole session): the job is queued exactly like a bill, so a shop whose
/// printer is off prints it when the printer comes back, and the food still
/// goes out.
pub fn print_delivery_slip_on(app: &App, order_id: String) -> UiResult<String> {
    guard::require(app, Permission::DeliveryDispatch)?;
    let at = now();
    let printer = crate::flows::default_printer(app)?;
    let shop_name = app.shop_config().store.name.clone();

    let row = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).delivery().delivery(OUTLET, &order_id))
            .map_err(|e| words::from_db(&e))
    })?;
    let Some(row) = row else {
        return Err(UiError::new(
            "delivery.unknown",
            "That delivery is not on this counter any more.",
        ));
    };
    let view = view_of(&row);

    let ctx = mb_print::template::DeliveryContext {
        shop: &shop_name,
        reference: &view.reference,
        time: None,
        customer: blank(&view.customer),
        phone: blank(&view.phone),
        address: blank(&view.address),
        note: None,
        rider: blank(&view.rider_name),
        lines: &[],
        total: row.total.to_indian_string(),
        collect: if view.paid {
            None
        } else {
            Some(view.collect.text.clone())
        },
    };
    let document = mb_print::template::delivery_document(printer.paper, &ctx);

    app.print(mb_print::queue::Job::new(
                    mb_print::queue::JobKind::Delivery,
                    &printer.id,
                    document,
                    today(at),
                )
                .because(format!("delivery {}", view.reference)),)
}

fn blank(text: &str) -> Option<&str> {
    if text.trim().is_empty() { None } else { Some(text) }
}

// ===========================================================================
// The commands
// ===========================================================================

#[tauri::command]
pub fn delivery_board(
    app: tauri::State<'_, App>,
    day: Option<String>,
) -> UiResult<DeliveryBoardView> {
    board_on(&app, day)
}

#[tauri::command]
pub fn save_delivery(
    app: tauri::State<'_, App>,
    edit: DeliveryEdit,
) -> UiResult<DeliveryBoardView> {
    save_delivery_on(&app, edit)
}

#[tauri::command]
pub fn record_handback(
    app: tauri::State<'_, App>,
    rider_id: String,
    amount: String,
    note: String,
) -> UiResult<DeliveryBoardView> {
    record_handback_on(&app, rider_id, amount, note)
}

#[tauri::command]
pub fn set_rider(
    app: tauri::State<'_, App>,
    staff_id: String,
    is_rider: bool,
) -> UiResult<DeliveryBoardView> {
    set_rider_on(&app, staff_id, is_rider)
}

#[tauri::command]
pub fn print_delivery_slip(app: tauri::State<'_, App>, order_id: String) -> UiResult<String> {
    print_delivery_slip_on(&app, order_id)
}
