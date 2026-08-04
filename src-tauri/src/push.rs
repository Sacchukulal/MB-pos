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
