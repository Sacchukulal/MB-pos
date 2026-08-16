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
/// P26. Suppliers, the paper, the supplier ledger and purchase orders — and
/// **one rupee, one row** (D120).
mod buying;
/// P26. The physical stock count, which freezes the book and posts a delta
/// rather than setting the balance (D127).
mod counting;
mod config;
mod corrections;
mod credit;
/// P18. **Closing the day** — requirement 9 of the ten, and audit B15: the
/// expected cash, the counted cash, the difference in words, and the lock.
mod dayclose;
mod floor;
mod employment;
/// P29. **Orders that leave on a bike**, and the cash a rider is carrying.
mod delivery;
/// P29. **Did the money actually arrive?** — the payment provider seam, the
/// attempts ledger and the unconfirmed list.
mod payments;
/// P29. **The things a counter is plugged into** — and not one of them may
/// ever stop a bill.
mod devices;
/// P30.5. **The first five minutes** — and the command that was missing:
/// nothing in this product could create a shop.
mod firstrun;
mod expenses;
/// P27. **Bills travelling between tills** â a settled bill is a FACT, and
/// facts are copied rather than reconciled (D136).
mod forwarding;
mod flows;
mod guard;
/// P25. **The stock book** — materials, recipes, wastage, the buy list and
/// food cost. Every unit conversion and every sentence is made here, because a
/// screen that divided a quantity by a pack size would be a second answer to
/// D108.
mod inventory;
/// P25 tests: the units worked through, the sale that always completes, and
/// the cache that has to agree with the ledger.
#[cfg(test)]
mod inventory_tests;
mod ipc;
/// P31. **The shop's logo** — the half D37 left for a later session and no
/// session took: picking a file, keeping the dots, and handing them to the
/// bill. Also the folder picker, because they are the same missing thing.
mod logo;
/// P19. **The counter as a server** — D9: the phone talks to the till over the
/// shop's own WiFi, and the cloud is never the road an order travels.
mod lan;
/// P21. **The licence** — one entitlement, decided in one place, and a gate
/// that structurally cannot reach billing (D86).
mod licensing;
/// P21's T1 and T10. Five bills in five states of the licence, and every gated
/// command called directly rather than hidden.
#[cfg(test)]
mod licence_tests;
mod logging;
mod menu;
mod preview;
/// P24. **The kitchen display** — a screen on the kitchen wall instead of a
/// paper ticket, and the paper fallback that means the kitchen never goes
/// blind.
mod kitchen;
/// P24 tests: the right station, both failure directions, courses and the
/// cancellation that cannot be waved away.
#[cfg(test)]
mod kitchen_tests;
/// P22. **A secret cannot be in the log**, and this is the half a script
/// enforces (D93). Audit E7 asks a shopkeeper to email us that file.
mod redact;
/// P22. A crash writes a report whether or not anybody agreed to send it
/// (D95) — audit E8.
mod crash;
/// P22. The button that turns a phone call into a fix, and the manifest a
/// person sees before it exists (D94) — audit E7.
mod diagnostics;
/// P22. "Is this counter healthy?", with a fix on every unhealthy row (D100).
mod health;
/// P22. **A shop must be able to go back** — audit E9, I1 and ANDROID-G2/G4.
mod updates;
/// P22. Setting a shop up — a checklist that READS the shop rather than a
/// wizard that remembers a position (D102), and never in front of the till.
mod setup;
/// P22's T5: drive everything that touches a secret, then read the log back.
#[cfg(test)]
mod log_tests;
/// P22's T7 and T8: break each thing in turn and read the bundle back out of
/// a real zip.
#[cfg(test)]
mod health_tests;
mod orders;
mod push;
/// P18. **One shape for every report**, so one screen renders all of them and
/// adding a report never touches a `.tsx` file.
mod reports;
mod search;
mod session;
/// P26, scope 10.13. Sharing a summary through the operating system, with the
/// limit printed on the screen (D134). It does not need a channel; it needs an
/// honest one.
mod share;
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
/// P26 drives the buying commands in sequence: a delivery moves four ledgers
/// at once, and the count has to survive a delivery arriving in the middle of
/// it (D127).
#[cfg(test)]
mod buying_tests;
/// P20 drives the intent applier against a real database: the counter is the
/// authority, so a test that stubs the counter is testing nothing.
#[cfg(test)]
mod order_tests;
/// P27.5. **A whole shop, so the look can be designed against a real screen.**
/// Not a test of anything — a seeder, ignored by the suite (D55).
/// P28 driven end to end: a payroll month with an advance, and the two things
/// that must be refused (approving twice, correcting your own hours).
#[cfg(test)]
mod employment_tests;
/// P29 driven end to end: a rider's evening, and the cash that comes back.
#[cfg(test)]
mod delivery_tests;
/// P29 driven end to end: the unconfirmed list, a declined card, and the three
/// places a tip must never appear.
#[cfg(test)]
mod payment_tests;
/// P29: T1 and T2 — every device absent, and every device present but not
/// answering. The sale completes either way.
#[cfg(test)]
mod device_tests;
/// P30. The hygiene rules as tests — every allow says why, nothing unwraps on
/// a user path, no deferred note is left in the tree, no secret is committed,
/// and no source file is unreachable.
#[cfg(test)]
mod hygiene_tests;
/// P30. **A whole trading day, and everything has to reconcile** — where two
/// figures are computed by different code, they are asserted equal.
#[cfg(test)]
mod acceptance_tests;
#[cfg(test)]
mod look_demo;
mod startup;
mod state;
/// P31. **Which typeface a bill and a kitchen ticket print in** — the file
/// system and the log half of `mb_print::font::Typefaces`.
mod typefaces;
/// P27. **The tills** â who this machine is, who the master is, and the
/// series it issues under (D135, D139).
mod terminals;
/// P27's eleven. Two tills with two databases, hammered concurrently, split for
/// half an hour and healed — because what is only provable here is what happens
/// to a shop's MONEY when two machines write bills at once.
#[cfg(test)]
mod terminal_tests;
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

    // Step 1b, immediately after — audit E8. A panic before this point has the
    // log and nothing else, which is why logging goes first and this second.
    crash::install();

    // Step 1c. **D98: count this attempt before anything can fail.**
    //
    // Incremented here and cleared only once the counter is genuinely up
    // (`window` + a shop, or the first-run state). Two attempts on one version
    // with no clear in between means the new release does not start on this
    // machine, and the shop gets its old one back rather than a Saturday night
    // with no till.
    let running = updates::Version::running();
    if updates::Starts::should_roll_back(&config_dir, running) {
        // The rollback itself is the installer we kept; running it is Phase
        // 8's other half. What matters here is that the state is DETECTED and
        // said out loud rather than the counter looping on a broken version.
        log_error!(
            "version {running} has failed to start repeatedly — the previous \
             version should be restored (Settings > Go back)"
        );
    }
    updates::Starts::attempted(&config_dir, running);

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
            // **The counter is up.** D98's clear, and it is deliberately here
            // rather than after the window paints: a build that opens a window
            // and then cannot read the shop's data is a build that must roll
            // back, and clearing on paint alone would let it keep starting.
            updates::Starts::healthy(&config_dir);
        }
        Startup::FirstRun => {
            log_info!("first run — the window opens to set-up");
            // A counter with no shop yet is still a counter that started, so
            // this start counts as healthy (D98). Withholding it would roll
            // back a perfectly good release on a brand-new machine.
            updates::Starts::healthy(&config_dir);
        }
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

    let mut builder = tauri::Builder::default();
    // P31. **"Browse…"**, and its Rust half only — no JS permission is granted
    // for it in `capabilities/default.json`, so the screens reach it through
    // `logo::pick_a_logo` and `logo::pick_a_folder` like everything else.
    builder = builder.plugin(tauri_plugin_dialog::init());
    // Two counters on one PC fight over the database, the licence and the
    // numbering. The second one focuses the first rather than dying quietly,
    // because "it won't open" is the bug report that follows.
    //
    // **Unless somebody has deliberately pointed this copy somewhere else**
    // (P27). The lock's job is "one copy per SHOP", and a machine running
    // against a config folder Windows did not choose is a second shop — two
    // tills, two databases, nothing shared. A shopkeeper never has that:
    // `APPDATA` is per-user and fixed, so setting it is a deliberate act.
    //
    // Without this, D55's two-process check is impossible: the second till is
    // swallowed by the first's window and P27 can only ever be proved by tests
    // that never leave one process.
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
            // Rust pushes; React subscribes (M4). Started before the window is
            // shown, so a job that was already parked when the app opened is on
            // screen at the first paint rather than at the first event.
            push::pump_print_queue(app.handle());
            push::emit_print_queue(app.handle());
            // **The idle lock** (P11). One sleeping thread; a React timer would
            // be a poll against M4 and would be bypassed by any screen that is
            // not open.
            push::watch_for_idle(app.handle());
            // P24. **The kitchen must never go blind.** One sleeping thread
            // that prints any ticket no screen drew in time. Not a React timer
            // — a screen that is not open would not run one, and the whole
            // point is that this works when the screen is dead.
            kitchen::watch_for_undrawn_tickets(app.handle());
            // P19. Last, because it is the only thing here that opens a socket
            // — and it never stops the window opening if it cannot.
            lan::start(app.handle());
            // P27. **The bills a secondary is holding** (D136). One sleeping
            // thread that does nothing at all on the main till, and that never
            // touches the settle path — it only reads a queue the settle
            // already wrote.
            forwarding::start_sender(app.handle());
            // D139. A till that was switched off while somebody moved the main
            // till finds out here, on its own, because the machine that failed
            // is exactly the one that could not be told at the time.
            if let Some(state) = app.handle().try_state::<App>() {
                terminals::check_the_master_at_startup(&state);
            }
            push::watch_for_pairing(app.handle());
            push::emit_session(app.handle());
            // P29, scope 7.8. Open the customer window if this shop asked for
            // one. It cannot fail into anything: a second monitor that has been
            // unplugged since last night is a log line and a counter that still
            // bills.
            if let Some(state) = app.handle().try_state::<App>() {
                let on = state.shop_config().devices.display_on;
                devices::sync_display(app.handle(), on);
            }
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
