//! Rust pushes. React subscribes.

use std::collections::BTreeSet;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::log_warn;
use crate::state::{App, Pushed};

/// The channel name. One event, one enum, one place to look.
pub const CHANNEL: &str = "mb://push";

/// Start forwarding the print queue's events to the window.
pub fn pump_print_queue(app: &AppHandle) {
    let Some(state) = app.try_state::<App>() else {
        return;
    };
    let Some(events) = state.subscribe_to_queue() else {
        // No shop, so no queue yet.
        return;
    };

    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("mb-push".to_owned())
        .spawn(move || {
            // Every event is a reason to re-send the snapshot.
            while events.recv().is_ok() {
                emit_print_queue(&handle);
            }
        });

    if let Err(e) = spawned {
        log_warn!("the print queue's status thread could not be started: {e}");
    }
}

/// Send the queue's current state to the window.
pub fn emit_print_queue(app: &AppHandle) {
    let Some(state) = app.try_state::<App>() else {
        return;
    };
    let jobs = state.print_queue_snapshot();
    if let Err(e) = app.emit(CHANNEL, Pushed::PrintQueue { jobs }) {
        log_warn!("a print queue update could not be sent to the window: {e}");
    }
}

/// Tell the window who is at the counter — or that nobody is.
pub fn emit_session(app: &AppHandle) {
    let Some(state) = app.try_state::<App>() else {
        return;
    };
    let current = state.sessions().current();
    let message = Pushed::Session {
        who: current.as_ref().map(|s| s.actor.name.clone()),
        role: current.as_ref().and_then(|s| s.actor.role_name.clone()),
        stand_in: current.as_ref().is_some_and(|s| s.is_stand_in),
    };
    if let Err(e) = app.emit(CHANNEL, message) {
        log_warn!("the sign-in state could not be sent to the window: {e}");
    }
}

/// A phone is asking to join.
pub fn emit_pairing(app: &AppHandle) {
    let Some(state) = app.try_state::<App>() else {
        return;
    };
    let waiting = state.network().map_or(0, |n| {
        u32::try_from(n.shared.desk.waiting().len()).unwrap_or(u32::MAX)
    });
    if let Err(e) = app.emit(CHANNEL, Pushed::Pairing { waiting }) {
        log_warn!("the pairing panel could not be told: {e}");
    }
}

/// The shop's data tells the window what changed, after every commit, whoever wrote it — a
/// cashier, a phone, the kitchen, a timer.
pub fn watch_the_shop(app: &AppHandle) {
    let Some(db) = app.try_state::<App>().and_then(|state| state.shop_db()) else {
        return;
    };
    let handle = app.clone();
    db.watch(Arc::new(move |tables| emit_changes(&handle, tables)));
}

/// Which tables mean which screen.
fn emit_changes(app: &AppHandle, tables: &BTreeSet<String>) {
    const FLOOR: &[&str] = &[
        "orders",
        "order_lines",
        "payments",
        "dining_tables",
        "sections",
        "reservations",
        "waitlist",
    ];
    const KITCHEN: &[&str] = &["orders", "order_lines", "kitchen_deliveries"];
    let touched = |names: &[&str]| names.iter().any(|name| tables.contains(*name));
    if touched(FLOOR)
        && let Err(e) = app.emit(CHANNEL, Pushed::Floor)
    {
        log_warn!("the floor could not be told it changed: {e}");
    }
    if touched(KITCHEN)
        && let Err(e) = app.emit(CHANNEL, Pushed::Kitchen)
    {
        log_warn!("the kitchen could not be told it changed: {e}");
    }
}

/// The floor changed the order the cashier has open.
pub fn emit_floor_change(app: &AppHandle) {
    let Some(state) = app.try_state::<App>() else {
        return;
    };
    let waiting = state.floor_changes_waiting();
    if let Err(e) = app.emit(CHANNEL, Pushed::FloorChanged { waiting }) {
        log_warn!("the counter could not be told what the floor did: {e}");
    }
}

/// Watch the pairing desk while it is open.
pub fn watch_for_pairing(app: &AppHandle) {
    let handle = app.clone();
    std::thread::Builder::new()
        .name("mb-pairing".to_owned())
        .spawn(move || {
            let mut last = 0_u32;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(700));
                let Some(state) = handle.try_state::<App>() else {
                    return;
                };
                let Some(network) = state.network() else {
                    continue;
                };
                let waiting =
                    u32::try_from(network.shared.desk.waiting().len()).unwrap_or(u32::MAX);

                // Asleep unless somebody is deliberately watching for a phone — OR a phone is
                // already standing there.
                if waiting == 0 && network.shared.desk.showing(crate::flows::now()).is_none() {
                    last = 0;
                    continue;
                }
                if waiting != last {
                    last = waiting;
                    emit_pairing(&handle);
                }
            }
        })
        .ok();
}

/// The idle clock.
pub fn watch_for_idle(app: &AppHandle) {
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("mb-idle".to_owned())
        .spawn(move || {
            loop {
                std::thread::sleep(crate::session::IDLE_TICK);
                let Some(state) = handle.try_state::<App>() else {
                    return;
                };
                // The shop's own limit, and 0 means never — a terminal in a locked back room
                // has no use for a screen that keeps asking who is there.
                let Some(limit) =
                    crate::session::idle_lock_for(state.shop_config().billing.idle_lock_minutes)
                else {
                    continue;
                };
                if !state.sessions().is_idle(crate::flows::now(), limit) {
                    continue;
                }
                if let Some(who) = state.sessions().end() {
                    crate::log_info!(
                        "the counter locked itself after {who} went quiet",
                        who = who.name
                    );
                    state.record_lock(&who);
                }
                emit_session(&handle);
            }
        });

    if let Err(e) = spawned {
        log_warn!("the idle lock could not be started: {e}");
    }
}
