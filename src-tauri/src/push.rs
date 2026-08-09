//! **Rust pushes. React subscribes. Nothing polls.**
//!
//! Budget **M4** — idle CPU under 1 % — and `PERFORMANCE.md` §5 rule 6:
//!
//! > *"No polling. Rust pushes state; React subscribes. A 250 ms poll loop is
//! > M4 gone before a single feature is written."*
//!
//! This is the pipe. The print queue's own `subscribe()` gives a
//! `Receiver<QueueEvent>` on a background thread (P07); this turns each event
//! into a snapshot and emits it to the window, and the screen re-renders. The
//! UI thread waits for nothing and asks for nothing.
//!
//! # Why a snapshot rather than the event
//!
//! A delta is a thing that can be missed — a screen that mounted late, a window
//! that was closed and reopened, an event dropped while the webview was
//! reloading. The queue holds at most a handful of jobs (D35: *"the spool is
//! not a log"*), so sending all of them costs nothing and cannot desynchronise.
//!
//! It is the same reasoning P07 used for `snapshot()` in the first place: *"a
//! screen that attached after the `Parked` event would otherwise be blind to
//! the one thing it exists to show."*

use tauri::{AppHandle, Emitter, Manager};

use crate::log_warn;
use crate::state::{App, Pushed};

/// The channel name. One event, one enum, one place to look.
pub const CHANNEL: &str = "mb://push";

/// Start forwarding the print queue's events to the window.
///
/// Spawns one thread that outlives the call. It ends when the queue's sender is
/// dropped, which happens on shutdown — so there is no stop handle to forget
/// and no way to leave a thread running against a queue that has gone.
pub fn pump_print_queue(app: &AppHandle) {
    let Some(state) = app.try_state::<App>() else {
        return;
    };
    let Some(events) = state.subscribe_to_queue() else {
        // No shop, so no queue yet. `App::open_shop` calls this again when one
        // opens; a first run simply has nothing to push.
        return;
    };

    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("mb-push".to_owned())
        .spawn(move || {
            // Every event is a reason to re-send the snapshot. The events
            // themselves are not forwarded: a screen does not need to know that
            // attempt three of five failed, it needs to know what is unfinished
            // right now.
            while events.recv().is_ok() {
                emit_print_queue(&handle);
            }
        });

    if let Err(e) = spawned {
        // Without this thread the indicator goes stale, which is audit D4 by
        // another route — so it is a warning and not a silence.
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
///
/// The same argument as the queue snapshot: the screen is told the state, never
/// the transition, so a screen that mounted late cannot be showing a stale
/// name over an unlocked till.
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

/// **A phone is asking to join** (P19, budget M4).
///
/// The panel does not poll. A phone can present its code at any moment, and
/// the person holding it is standing at the counter waiting for the name to
/// appear — so the counter pushes, exactly as it does for a print job and a
/// sign-in. The first version of the panel ran a `setInterval` while the code
/// was on screen, and `guards.test.ts` failed the build over it, which is
/// D40's rule working: the rules that erode are enforced by scripts.
pub fn emit_pairing(app: &AppHandle) {
    let Some(state) = app.try_state::<App>() else {
        return;
    };
    let waiting = state
        .network()
        .map_or(0, |n| u32::try_from(n.shared.desk.waiting().len()).unwrap_or(u32::MAX));
    if let Err(e) = app.emit(CHANNEL, Pushed::Pairing { waiting }) {
        log_warn!("the pairing panel could not be told: {e}");
    }
}

/// Watch the pairing desk while it is open.
///
/// **This is not a poll of the SCREEN** — nothing crosses to React unless the
/// number actually changes, and the thread sleeps whenever no code is being
/// shown. It exists because a phone arrives over a socket on another thread,
/// and a socket handler must not reach into Tauri's window (rule 2 of
/// `mb-lan`'s guarantee: a handler holds nothing the counter needs).
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
                // Asleep unless somebody is deliberately watching for a phone.
                if network.shared.desk.showing(crate::flows::now()).is_none() {
                    last = 0;
                    continue;
                }
                let waiting =
                    u32::try_from(network.shared.desk.waiting().len()).unwrap_or(u32::MAX);
                if waiting != last {
                    last = waiting;
                    emit_pairing(&handle);
                }
            }
        })
        .ok();
}

/// **The idle clock** (P11 item 8, budget M4).
///
/// One thread, asleep almost always, that locks the counter when nothing has
/// happened for [`crate::session::IDLE_LOCK`]. It is here rather than in React
/// for two reasons: a React timer is a poll, and a React timer is bypassed by
/// any screen that is not open.
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
                // P17: the shop's own limit, and **0 means never** — a terminal
                // in a locked back room has no use for a screen that keeps
                // asking who is there.
                let Some(limit) =
                    crate::session::idle_lock_for(state.shop_config().billing.idle_lock_minutes)
                else {
                    continue;
                };
                if !state.sessions().is_idle(crate::flows::now(), limit) {
                    continue;
                }
                if let Some(who) = state.sessions().end() {
                    crate::log_info!("the counter locked itself after {who} went quiet", who = who.name);
                    state.record_lock(&who);
                }
                emit_session(&handle);
            }
        });

    if let Err(e) = spawned {
        // A counter that never locks is audit C1 back again, so this is said
        // out loud rather than swallowed.
        log_warn!("the idle lock could not be started: {e}");
    }
}
