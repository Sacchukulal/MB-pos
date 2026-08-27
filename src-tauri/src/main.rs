//! Magic Bill — the counter.

// A desktop app should not also open a console window behind itself.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code, reason = "P08's Rust half landed before its React half")]

/// A whole trading day, and everything has to reconcile — where two figures are computed by
/// different code, they are asserted equal.
#[cfg(test)]
mod acceptance_tests;
mod billing;
/// Suppliers, the paper, the supplier ledger and purchase orders — and one rupee, one row.
mod buying;
#[cfg(test)]
mod buying_tests;
mod config;
mod corrections;
/// The physical stock count, which freezes the book and posts a delta rather than setting the
/// balance.
mod counting;
/// A crash writes a report whether or not anybody agreed to send it.
mod crash;
mod credit;
#[cfg(test)]
mod credit_tests;
/// Closing the day.
mod dayclose;
#[cfg(test)]
mod dayclose_tests;
/// Orders that leave on a bike, and the cash a rider is carrying.
mod delivery;
#[cfg(test)]
mod delivery_tests;
#[cfg(test)]
mod device_tests;
/// The things a counter is plugged into — and not one of them may ever stop a bill.
mod devices;
/// The button that turns a phone call into a fix, and the manifest a person sees before it
/// exists.
mod diagnostics;
mod employment;
/// A whole shop, so the look can be designed against a real screen.
#[cfg(test)]
mod employment_tests;
#[cfg(test)]
mod expense_tests;
mod expenses;
/// The first five minutes — and the command that was missing: nothing in this product could
/// create a shop.
mod firstrun;
mod floor;
#[cfg(test)]
mod floor_tests;
mod flows;
/// Bills travelling between tills â a settled bill is a FACT, and facts are copied rather
/// than reconciled.
mod forwarding;
mod guard;
/// "Is this counter healthy?", with a fix on every unhealthy row.
mod health;
#[cfg(test)]
mod health_tests;
/// The hygiene rules as tests — every allow says why, nothing unwraps on a user path, no
/// deferred note is left in the tree, no secret is committed, and no source file is
/// unreachable.
#[cfg(test)]
mod hygiene_tests;
/// The stock book — materials, recipes, wastage, the buy list and food cost.
mod inventory;
#[cfg(test)]
mod inventory_tests;
mod ipc;
/// The kitchen display — a screen on the kitchen wall instead of a paper ticket, and the paper
/// fallback that means the kitchen never goes blind.
mod kitchen;
#[cfg(test)]
mod kitchen_tests;
/// The counter as a server.
mod lan;
#[cfg(test)]
mod licence_tests;
/// The licence — one entitlement, decided in one place, and a gate that structurally cannot
/// reach billing.
mod licensing;
#[cfg(test)]
mod log_tests;
mod logging;
/// The shop's logo.
mod logo;
#[cfg(test)]
mod look_demo;
mod menu;
#[cfg(test)]
mod menu_tests;
/// Where a new row's id comes from.
mod newid;
#[cfg(test)]
mod order_tests;
mod orders;
#[cfg(test)]
mod payment_tests;
/// Did the money actually arrive?
mod payments;
#[cfg(test)]
mod perf_tests;
mod preview;
mod push;
/// A secret cannot be in the log, and this is the half a script enforces.
mod redact;
/// One shape for every report, so one screen renders all of them and adding a report never
/// touches a `.tsx` file.
mod reports;
mod search;
mod session;
/// The one table that knows what a setting is, and the load / save / reset / export / import
/// that fall out of it.
mod settings;
#[cfg(test)]
mod settings_tests;
/// Setting a shop up — a checklist that READS the shop rather than a wizard that remembers a
/// position, and never in front of the till.
mod setup;
mod share;
#[cfg(test)]
mod signin_tests;
mod startup;
mod state;
#[cfg(test)]
mod terminal_tests;
/// The tills â who this machine is, who the master is, and the series it issues under.
mod terminals;
/// Which typeface a bill and a kitchen ticket print in — the file system and the log half of
/// `mb_print::font::Typefaces`.
mod typefaces;
/// A shop must be able to go back.
mod updates;
mod window;
mod words;

use tauri::Manager;

use config::AppConfig;
use startup::Startup;
use state::App;

fn main() {
    // Step 1, before anything that can fail.
    let config_dir = AppConfig::directory();
    logging::start(&config_dir.join("logs"));
    log_info!("Magic Bill {} starting", env!("CARGO_PKG_VERSION"));

    // Step 1b, immediately after.
    crash::install();

    // Count this attempt before anything can fail.
    let running = updates::Version::running();
    if updates::Starts::should_roll_back(&config_dir, running) {
        // The rollback itself is the installer we kept; running it is Phase 8's other half.
        log_error!(
            "version {running} has failed to start repeatedly — the previous \
             version should be restored (Settings > Go back)"
        );
    }
    updates::Starts::attempted(&config_dir, running);

    // The look, so the window is painted correctly the first time rather than flashing light
    // and then going dark.
    let app_config = AppConfig::load();

    let app_state = match App::new(app_config.clone()) {
        Ok(state) => state,
        Err(e) => {
            // The only way this fails is a font that will not load, which means a corrupted
            // install.
            log_error!("Magic Bill cannot start: {e}");
            return;
        }
    };

    // Steps 3 to 6. The order is load-bearing and `startup.rs` explains why: a restore happens
    // BEFORE anything opens the database.
    match startup::run(&config_dir) {
        Startup::Ready { db, path, restored } => {
            if restored {
                log_info!("a backup was restored during start-up");
            }
            app_state.open_shop(*db, path);
            // The counter is up.
            updates::Starts::healthy(&config_dir);
        }
        Startup::FirstRun => {
            log_info!("first run — the window opens to set-up");
            // A counter with no shop yet is still a counter that started, so this start counts
            // as healthy.
            updates::Starts::healthy(&config_dir);
        }
        Startup::FoundCandidates { candidates, .. } => {
            log_info!(
                "{} possible data file(s) found — the window opens and asks",
                candidates.len()
            );
        }
        Startup::Failed { error } => {
            // The window still opens.
            log_error!("start-up failed: {error}");
        }
    }

    let window_state = app_config.window.clone();

    let mut builder = tauri::Builder::default();
    // "Browse…", and its Rust half only — no JS permission is granted for it in
    // `capabilities/default.json`, so the screens reach it through `logo::pick_a_logo` and
    // `logo::pick_a_folder` like everything else.
    builder = builder.plugin(tauri_plugin_dialog::init());
    // Two counters on one PC fight over the database, the licence and the numbering.
    if config::AppConfig::directory() == config::AppConfig::windows_default() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            window::focus_existing(app);
        }));
    } else {
        log_info!(
            "this copy runs against {} rather than the usual folder, so it does \
             not take the one-copy lock",
            config::AppConfig::directory().display()
        );
    }

    builder
        .manage(app_state)
        .invoke_handler(commands!())
        .setup(move |app| {
            // Rust pushes; React subscribes.
            push::pump_print_queue(app.handle());
            push::emit_print_queue(app.handle());
            // The idle lock.
            push::watch_for_idle(app.handle());
            // The kitchen must never go blind.
            kitchen::watch_for_undrawn_tickets(app.handle());
            // Last, because it is the only thing here that opens a socket — and it never stops
            // the window opening if it cannot.
            lan::start(app.handle());
            // The bills a secondary is holding.
            forwarding::start_sender(app.handle());
            // A till that was switched off while somebody moved the main till finds out here,
            // on its own, because the machine that failed is exactly the one that could not be
            // told at the time.
            if let Some(state) = app.handle().try_state::<App>() {
                terminals::check_the_master_at_startup(&state);
            }
            push::watch_for_pairing(app.handle());
            push::emit_session(app.handle());
            if let Some(state) = app.handle().try_state::<App>() {
                let on = state.shop_config().devices.display_on;
                devices::sync_display(app.handle(), on);
            }
            if let Some(main) = app.get_webview_window("main") {
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
