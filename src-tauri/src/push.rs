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

/// The phones: how many are live, how many are asking to join — the top bar's number and the
/// Phones panel's cue to re-read itself.
pub fn emit_phones(app: &AppHandle) {
    let Some(state) = app.try_state::<App>() else {
        return;
    };
    let (connected, waiting) = state.network().map_or((0, 0), |n| {
        (
            u32::try_from(n.shared.connected()).unwrap_or(u32::MAX),
            u32::try_from(n.shared.desk.waiting().len()).unwrap_or(u32::MAX),
        )
    });
    if let Err(e) = app.emit(CHANNEL, Pushed::Phones { connected, waiting }) {
        log_warn!("the phones panel could not be told: {e}");
    }
}

/// The shop's data tells the window what changed, after every commit, whoever wrote it — a
/// cashier, a phone, the kitchen, a timer.
pub fn watch_the_shop(app: &AppHandle) {
    let Some(db) = app.try_state::<App>().and_then(|state| state.shop_db()) else {
        return;
    };
    let handle = app.clone();
    let phones = start_phone_pusher(app.clone());
    db.watch(Arc::new(move |tables| {
        emit_changes(&handle, tables);
        tell_the_phones(&phones, tables);
    }));
}

/// What the phones are told about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhoneNews {
    Floor,
    Catalogue,
}

/// The phones' pusher: its own thread, fed from the commit callback, so building a floor
/// snapshot never runs on the thread that holds the shop — that is the nesting deadlock P18
/// and P20 both hit. It also collapses a burst (one settle touches four tables) into one push.
fn start_phone_pusher(app: AppHandle) -> std::sync::mpsc::Sender<PhoneNews> {
    let (tx, rx) = std::sync::mpsc::channel::<PhoneNews>();
    let spawned = std::thread::Builder::new()
        .name("mb-phones".to_owned())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                // A burst is one change.
                let mut news = BTreeSet::new();
                news.insert(first as u8);
                std::thread::sleep(std::time::Duration::from_millis(150));
                while let Ok(more) = rx.try_recv() {
                    news.insert(more as u8);
                }
                let Some(state) = app.try_state::<App>() else {
                    return;
                };
                if !crate::lan::phones_listening(&state) {
                    continue;
                }
                if news.contains(&(PhoneNews::Catalogue as u8))
                    && let Ok(catalogue) = crate::orders::catalogue(&state)
                {
                    crate::lan::push_to_phones(
                        &state,
                        "catalogue",
                        serde_json::json!({ "version": catalogue.version }),
                    );
                }
                if news.contains(&(PhoneNews::Floor as u8))
                    && let Ok(body) = crate::orders::floor_body(&state)
                {
                    crate::lan::push_to_phones(&state, "floor", body);
                }
            }
        });
    if let Err(e) = spawned {
        log_warn!("the phones' pusher could not be started: {e}");
    }
    tx
}

fn tell_the_phones(phones: &std::sync::mpsc::Sender<PhoneNews>, tables: &BTreeSet<String>) {
    const FLOOR: &[&str] = &["orders", "order_lines", "payments", "dining_tables", "sections"];
    const CATALOGUE: &[&str] = &["items", "categories", "item_variants", "dining_tables", "sections"];
    let touched = |names: &[&str]| names.iter().any(|name| tables.contains(*name));
    if touched(CATALOGUE) {
        let _ = phones.send(PhoneNews::Catalogue);
    }
    if touched(FLOOR) {
        let _ = phones.send(PhoneNews::Floor);
    }
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
            // What the panel last heard: how many are asking, and which code is on screen —
            // the code changes the moment a phone uses it, and the screen must follow.
            let mut last: Option<(usize, String)> = None;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(700));
                let Some(state) = handle.try_state::<App>() else {
                    return;
                };
                let Some(network) = state.network() else {
                    continue;
                };
                let showing = network.shared.desk.showing(crate::flows::now());
                let waiting = network.shared.desk.waiting().len();

                // Asleep unless somebody is deliberately watching for a phone — OR a phone is
                // already standing there.
                if waiting == 0 && showing.is_none() {
                    if last.is_some() {
                        // The code expired or was closed: the panel learns that too.
                        last = None;
                        emit_phones(&handle);
                    }
                    continue;
                }
                let now = Some((waiting, showing.map(|(_, code)| code).unwrap_or_default()));
                if now != last {
                    last = now;
                    emit_phones(&handle);
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
