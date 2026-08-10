//! Magic Bill — the counter.
//!
//! **The first thing in this rebuild that runs.** Phases 1 to 3 are four
//! library crates and 311 tests: a bill can be computed, numbered, settled,
//! stored, backed up, restored, rendered and printed, and none of it has ever
//! been on a screen. This is the shell that puts it there.
//!
//! # What lives here, and what deliberately does not
//!
//! This crate owns the window, the start-up order, the IPC boundary and the
//! state that outlives a screen. It owns **no business rule at all** — D1 and
//! audit E3: *"business rules live inside screen files… to answer 'what exactly
//! happens when a bill is settled?' you must read four files at once."* Every
//! rule is in mb-core, every row is in mb-db, every dot is in mb-print, and
//! this file is a hundred lines of wiring.

// A desktop app should not also open a console window behind itself. Debug
// builds keep it, because that is where a developer's output goes.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// *** TEMPORARY, AND IT COMES OUT WHEN P08'S SCREENS LAND. ***
//
// This crate is half of P08. The Rust shell is finished; the React side — the
// UI kit, the shell chrome, the gallery and the receipt preview — is not, and
// several things here exist precisely for it: `MoneyView` (what the cart will
// show), `startup::adopt` and `FoundCandidates::expected` (the "we found a
// database here" screen), `App::font` (the preview), `logging::Level::Debug`
// and `words::from_io`.
//
// Deleting them to satisfy the lint and writing them again in three hours is
// worse than one allow with a note saying when it goes. **If you are reading
// this after the screens exist, remove this line and fix what it hides.**
#![allow(dead_code, reason = "P08's Rust half landed before its React half")]

mod billing;
mod config;
mod corrections;
mod credit;
/// P18. **Closing the day** — requirement 9 of the ten, and audit B15: the
/// expected cash, the counted cash, the difference in words, and the lock.
mod dayclose;
mod floor;
mod expenses;
mod flows;
mod guard;
mod ipc;
/// P19. **The counter as a server** — D9: the phone talks to the till over the
/// shop's own WiFi, and the cloud is never the road an order travels.
mod lan;
mod logging;
mod menu;
mod preview;
mod orders;
mod push;
/// P18. **One shape for every report**, so one screen renders all of them and
/// adding a report never touches a `.tsx` file.
mod reports;
mod search;
mod session;
/// P17. **The one table that knows what a setting is**, and the load / save /
/// reset / export / import that fall out of it.
mod settings;
/// P11's flows are sequences, and a sequence that can only be checked by
/// clicking is a sequence that gets checked once. See the file's own note.
#[cfg(test)]
mod signin_tests;
/// B2 and B7 — the two of P10's five budgets that can be measured without a
/// screen. B1, B3 and B8 are re-assigned to P22; see the file, and
/// PERFORMANCE.md §4.
#[cfg(test)]
mod perf_tests;
/// P16's cash position is the figure P18 leans on, so the sequence that
/// produces it is driven with the real commands.
#[cfg(test)]
mod expense_tests;
/// P15's credit account is a sequence too: bill a regular, take money back,
/// and the statement has to add up at every step.
#[cfg(test)]
mod credit_tests;
/// P14 moves an order between tables, merges two and splits one. Every one of
/// those is a sequence on the money path that moves the kitchen ledger with it.
#[cfg(test)]
mod floor_tests;
/// P13's flows are sequences too — set a rate, watch every item on it move;
/// export, edit a cell, import back. Same reasoning as `signin_tests`.
#[cfg(test)]
mod menu_tests;
/// P17's T1 renders the paper twice for every setting on it, and its storage
/// tests need a real `settings` table to count rows in.
#[cfg(test)]
mod settings_tests;
/// P18's day close is a sequence a shop performs every night, and the lock it
/// produces has to be real — so it is driven with the real commands.
#[cfg(test)]
mod dayclose_tests;
/// P20 drives the intent applier against a real database: the counter is the
/// authority, so a test that stubs the counter is testing nothing.
#[cfg(test)]
mod order_tests;
mod startup;
mod state;
mod window;
mod words;

use tauri::Manager;

use config::AppConfig;
use startup::Startup;
use state::App;

fn main() {
    // Step 1, before anything that can fail. Audit E7: when the owner reports a
    // problem there must be something to read.
    let config_dir = AppConfig::directory();
    logging::start(&config_dir.join("logs"));
    log_info!("Magic Bill {} starting", env!("CARGO_PKG_VERSION"));

    // Step 2. The look, so the window is painted correctly the first time
    // rather than flashing light and then going dark.
    let app_config = AppConfig::load();

    let app_state = match App::new(app_config.clone()) {
        Ok(state) => state,
        Err(e) => {
            // The only way this fails is a font that will not load, which means
            // a corrupted install. There is no window yet, so the log is the
            // only place it can be said.
            log_error!("Magic Bill cannot start: {e}");
            return;
        }
    };

    // Steps 3 to 6. The order is load-bearing and `startup.rs` explains why: a
    // restore happens BEFORE anything opens the database (D27).
    match startup::run(&config_dir) {
        Startup::Ready { db, path, restored } => {
            if restored {
                log_info!("a backup was restored during start-up");
            }
            app_state.open_shop(*db, path);
        }
        Startup::FirstRun => log_info!("first run — the window opens to set-up"),
        Startup::FoundCandidates { candidates, .. } => {
            log_info!(
                "{} possible data file(s) found — the window opens and asks",
                candidates.len()
            );
        }
        Startup::Failed { error } => {
            // **The window still opens.** An error dialog on top of nothing is
            // the worst thing to show somebody whose shop will not start.
            log_error!("start-up failed: {error}");
        }
    }

    let window_state = app_config.window.clone();

    tauri::Builder::default()
        // Two counters on one PC fight over the database, the licence and the
        // numbering. The second one focuses the first rather than dying
        // quietly, because "it won't open" is the bug report that follows.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            window::focus_existing(app);
        }))
        .manage(app_state)
        .invoke_handler(commands!())
        .setup(move |app| {
            // Rust pushes; React subscribes (M4). Started before the window is
            // shown, so a job that was already parked when the app opened is on
            // screen at the first paint rather than at the first event.
            push::pump_print_queue(app.handle());
            push::emit_print_queue(app.handle());
            // **The idle lock** (P11). One sleeping thread; a React timer would
            // be a poll against M4 and would be bypassed by any screen that is
            // not open.
            push::watch_for_idle(app.handle());
            // P19. Last, because it is the only thing here that opens a socket
            // — and it never stops the window opening if it cannot.
            lan::start(app.handle());
            push::watch_for_pairing(app.handle());
            push::emit_session(app.handle());
            if let Some(main) = app.get_webview_window("main") {
                // Step 8. Restored and THEN shown, so the 800x600 flash audit
                // F7 describes cannot happen.
                window::restore_and_show(&main, &window_state);
                log_info!("the window is up");
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                if let Some(state) = window.app_handle().try_state::<App>()
                    && let Some(main) = window.get_webview_window("main")
                {
                    state.update_config(|config| window::remember(&main, config));
                }
            }
            tauri::WindowEvent::Destroyed => {
                if let Some(state) = window.app_handle().try_state::<App>() {
                    state.shutdown();
                }
                log_info!("Magic Bill is closing");
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            log_error!("the window could not be created: {e}");
        });
}
