//! **The one place this program says no.**
//!
//! > Audit **C1**: *"There is no login on the POS at all. Anybody who walks
//! > behind the counter can open Reports and see the whole day's cash, change
//! > the bill number, delete menu items, delete credit customers, or deactivate
//! > the licence."*
//!
//! # Hiding a button is a courtesy; this is the control
//!
//! A permission check in TypeScript is decoration: the command is still there,
//! and `window.__TAURI__.invoke` is two lines away in a dev console. Every
//! guarded command therefore opens with one line —
//!
//! ```ignore
//! let who = guard::require(&app, Permission::BillVoid)?;
//! ```
//!
//! — and `require` does exactly three things: refuse when nobody is logged in,
//! refuse when this person may not, and **touch the session's idle clock**, so
//! that clock is fed by work rather than by mouse movement crossing the IPC
//! boundary.
//!
//! # The coverage table, and why it is a table
//!
//! There are twenty-eight commands today and there will be well over a hundred
//! by P30. A rule that every one of them is checked, enforced by everybody
//! remembering, is D40's definition of a rule that erodes — *"the rules that
//! erode are enforced by scripts, not by agreement"*.
//!
//! So [`COMMAND_ACCESS`] lists every command and what it needs, and a test
//! reads `ipc.rs` and `flows.rs`, finds every `#[tauri::command]`, and fails if
//! one is missing from the list. Adding a command without deciding is not a
//! review comment; it is a red build. Choosing [`Access::Public`] is allowed —
//! it just has to be chosen.

use mb_auth::{Actor, Permission};

use crate::state::App;
use crate::words::{UiError, UiResult};

/// What a command needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Anybody, including nobody: these work on the lock screen. Keep this list
    /// short and keep the reason with it.
    Public,
    Needs(Permission),
    /// **Any one of these opens the door** (P17).
    ///
    /// The settings screen is four screens in a trench coat: the shop's
    /// details, tax, printers and backup are four permissions, and a shop that
    /// gives one person the printers and another the tax rates is doing the
    /// normal thing. Requiring all four to *read* the screen would mean nobody
    /// but the owner could open it; requiring one particular one would mean the
    /// printer person needed `settings.store` to reach the printers.
    ///
    /// **It is a read-side answer only.** Every save re-checks the permission
    /// of the section it is writing, one by one, so this widens what can be
    /// looked at and never what can be changed.
    NeedsAny(&'static [Permission]),
}

/// **Every command in the product, and what it needs.**
pub const COMMAND_ACCESS: &[(&str, Access)] = &[
    // --- works while locked, and has to ------------------------------------
    // The shell reads its own state before anybody has logged in.
    ("app_status", Access::Public),
    // The theme toggle is on the lock screen (UI_GUIDELINES §0: light and dark
    // both ship, with a toggle the user can press — including this user).
    ("set_appearance", Access::Public),
    // Audit E7: "send me the log" must be a button, and support asks for it
    // precisely when nobody can get in.
    ("reveal_logs", Access::Public),
    // The lock screen itself.
    ("lock_state", Access::Public),
    ("login", Access::Public),
    ("lock_now", Access::Public),
    ("recover_with_code", Access::Public),
    // The print queue indicator stays visible while locked — audit D4: a bill
    // that printed wrong while the screen was locked is still the shop's
    // problem, and a queue nobody can see is the finding itself.
    ("list_print_jobs", Access::Public),
    // The preview of the built-in test slip needs no shop and no data.
    ("preview_test_page", Access::Public),

    // --- billing ------------------------------------------------------------
    ("current_cart", Access::Needs(Permission::BillCreate)),
    ("cart_add", Access::Needs(Permission::BillCreate)),
    ("cart_set_qty", Access::Needs(Permission::BillCreate)),
    ("cart_remove", Access::Needs(Permission::BillCreate)),
    ("cart_clear", Access::Needs(Permission::BillCreate)),
    ("cart_set_order_type", Access::Needs(Permission::BillCreate)),
    ("cart_add_payment", Access::Needs(Permission::BillCreate)),
    ("cart_clear_payments", Access::Needs(Permission::BillCreate)),
    ("open_orders", Access::Needs(Permission::BillCreate)),
    ("menu_items", Access::Needs(Permission::BillCreate)),
    ("search_items", Access::Needs(Permission::BillCreate)),
    ("open_table", Access::Needs(Permission::BillCreate)),
    ("print_kitchen_ticket", Access::Needs(Permission::BillCreate)),
    ("complete_bill", Access::Needs(Permission::BillCreate)),

    // --- paper --------------------------------------------------------------
    ("list_printers", Access::Needs(Permission::SettingsPrinter)),
    ("print_test_page", Access::Needs(Permission::SettingsPrinter)),
    ("nudge_print_offset", Access::Needs(Permission::SettingsPrinter)),
    // Retrying or abandoning a print is reprinting a bill, which is a thing a
    // shop counts (scope 1.20) and therefore a thing it permits.
    ("retry_print_job", Access::Needs(Permission::BillReprint)),
    ("dismiss_print_job", Access::Needs(Permission::BillReprint)),

    // --- people -------------------------------------------------------------
    ("list_staff", Access::Needs(Permission::StaffManage)),
    ("save_staff_member", Access::Needs(Permission::StaffManage)),
    ("set_staff_pin", Access::Needs(Permission::StaffManage)),
    ("list_roles", Access::Needs(Permission::StaffManage)),
    ("save_role", Access::Needs(Permission::StaffManage)),
    ("list_permissions", Access::Needs(Permission::StaffManage)),
    ("audit_trail", Access::Needs(Permission::AuditView)),

    // --- taking something back (P12) ----------------------------------------
    // The day's takings on a screen, which is audit C1's first example of what
    // anybody could see — so it is `reports.view`, not `bill.create`.
    ("list_bills", Access::Needs(Permission::ReportsView)),
    ("day_totals", Access::Needs(Permission::ReportsView)),
    // The reason list itself is not sensitive; being unable to read it would
    // make every correction dialog open empty.
    ("reasons", Access::Needs(Permission::BillCreate)),
    ("void_bill", Access::Needs(Permission::BillVoid)),
    // A refund is the money half of a void, so it is the same permission: a
    // shop that let a cashier hand cash back without letting them void would
    // have a hole shaped exactly like the one B5 describes.
    ("refund_bill", Access::Needs(Permission::BillVoid)),
    ("cancel_order", Access::Needs(Permission::OrderCancel)),
    ("void_line", Access::Needs(Permission::OrderItemVoid)),
    ("reprint_bill", Access::Needs(Permission::BillReprint)),

    // --- the menu (P13) -----------------------------------------------------
    ("menu_tax_classes", Access::Needs(Permission::MenuManage)),
    ("menu_categories", Access::Needs(Permission::MenuManage)),
    ("menu_rows", Access::Needs(Permission::MenuManage)),
    ("save_menu_item", Access::Needs(Permission::MenuManage)),
    ("set_item_available", Access::Needs(Permission::MenuManage)),
    ("save_menu_category", Access::Needs(Permission::MenuManage)),
    // A tax rate is not a menu edit: it is what the shop owes the
    // government, and getting it wrong is a notice rather than a bad price.
    ("save_tax_class", Access::Needs(Permission::SettingsTax)),
    ("change_menu_prices", Access::Needs(Permission::MenuManage)),
    ("plan_menu_import", Access::Needs(Permission::MenuManage)),
    ("run_menu_import", Access::Needs(Permission::MenuManage)),
    ("export_menu", Access::Needs(Permission::MenuManage)),
    // What an item is made of (scope 6.1–6.3). All `menu.manage`: a size and a
    // modifier are both prices, and a combo is a price with arithmetic in it.
    ("item_composition", Access::Needs(Permission::MenuManage)),
    ("save_item_variant", Access::Needs(Permission::MenuManage)),
    ("list_modifier_groups", Access::Needs(Permission::MenuManage)),
    ("save_modifier_group", Access::Needs(Permission::MenuManage)),
    ("attach_modifier_group", Access::Needs(Permission::MenuManage)),
    ("list_combos", Access::Needs(Permission::MenuManage)),
    ("save_combo", Access::Needs(Permission::MenuManage)),
    // The floor (P14). Reading it is billing work — a cashier has to see the
    // tables. CHANGING the room is `tables.manage`, and the three operations
    // are gated by what they really do: moving an order is billing, merging
    // one away destroys a bill number and is gated like a void.
    ("floor_plan", Access::Needs(Permission::BillCreate)),
    ("save_floor_section", Access::Needs(Permission::TablesManage)),
    ("delete_floor_section", Access::Needs(Permission::TablesManage)),
    ("save_dining_table", Access::Needs(Permission::TablesManage)),
    ("add_dining_tables", Access::Needs(Permission::TablesManage)),
    ("place_dining_table", Access::Needs(Permission::TablesManage)),
    ("set_dining_table_active", Access::Needs(Permission::TablesManage)),
    ("delete_dining_table", Access::Needs(Permission::TablesManage)),
    ("save_floor_thresholds", Access::Needs(Permission::TablesManage)),
    ("move_order", Access::Needs(Permission::BillCreate)),
    ("merge_orders", Access::Needs(Permission::BillVoid)),
    ("split_order", Access::Needs(Permission::BillCreate)),
    ("even_split", Access::Needs(Permission::BillCreate)),
    ("set_covers", Access::Needs(Permission::BillCreate)),

    // --- customers and what they owe (P15) --------------------------------
    // The owner renamed this from "khata" on 2026-08-08.
    ("who_owes", Access::Needs(Permission::CustomersManage)),
    ("customers", Access::Needs(Permission::CustomersManage)),
    ("customer_account", Access::Needs(Permission::CustomersManage)),
    ("save_customer", Access::Needs(Permission::CustomersManage)),
    // Taking money IN is a cashier's job — `credit.collect` exists for exactly
    // this and nothing else.
    ("record_repayment", Access::Needs(Permission::CreditCollect)),
    // Changing what somebody owes without money moving is not. It is the one
    // door in this account that could make money disappear.
    ("save_credit_adjustment", Access::Needs(Permission::CustomersManage)),
    // Both of these happen mid-bill, so they are billing work; going PAST the
    // limit asks for `customers.manage` a second time inside the command.
    ("credit_headroom", Access::Needs(Permission::BillCreate)),
    ("put_on_account", Access::Needs(Permission::BillCreate)),

    // --- money going out, and the drawer (P16) ------------------------------
    // All `expenses.manage`. A cashier who records a 40-rupee milk purchase
    // mid-service is doing the thing this feature exists for, so the
    // permission is on the role rather than on the till.
    ("expenses", Access::Needs(Permission::ExpensesManage)),
    ("save_expense", Access::Needs(Permission::ExpensesManage)),
    ("delete_expense", Access::Needs(Permission::ExpensesManage)),
    ("save_cash_movement", Access::Needs(Permission::ExpensesManage)),
    ("save_expense_category", Access::Needs(Permission::ExpensesManage)),
    ("save_recurring_expense", Access::Needs(Permission::ExpensesManage)),
    ("confirm_recurring_expense", Access::Needs(Permission::ExpensesManage)),
    ("export_expenses", Access::Needs(Permission::ExpensesManage)),

    // --- settings (P17) -----------------------------------------------------
    // Reading is `NeedsAny`: the shop's details, tax, printers and backup are
    // four permissions and a shop may well split them between two people.
    // **Every save re-checks the section it is writing** — see
    // `settings::ipc::permission_for`, which both sides call.
    ("settings_all", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    ("reload_settings", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    ("search_settings", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    ("save_settings", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    ("settings_defaults_for", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    // The live preview. Reading only â it renders a SAMPLE bill and never
    // touches a real one.
    ("preview_settings", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    ("export_settings", Access::Needs(Permission::SettingsStore)),
    ("plan_settings_import", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    // **All four**, and the command re-checks each one: an import writes tax
    // rates and printer setup, so it is not a store edit.
    ("run_settings_import", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    // The counters. A bill number is what a GST return is a list of.
    ("numbering", Access::Needs(Permission::SettingsTax)),
    ("save_counter", Access::Needs(Permission::SettingsTax)),

    // --- the printers, the backup and where the shop is (P17 part 4) -------
    ("printer_setup", Access::Needs(Permission::SettingsPrinter)),
    ("save_printer", Access::Needs(Permission::SettingsPrinter)),
    ("delete_printer", Access::Needs(Permission::SettingsPrinter)),
    ("route_category", Access::Needs(Permission::SettingsPrinter)),
    ("print_sample_bill", Access::Needs(Permission::SettingsPrinter)),
    ("nudge_printer", Access::Needs(Permission::SettingsPrinter)),
    // Backup is its own permission, and audit A1 is why: the person who may
    // REPLACE a shop's whole database is not automatically the person who may
    // change its footer message.
    ("backup_status", Access::Needs(Permission::BackupRun)),
    ("back_up_now", Access::Needs(Permission::BackupRun)),
    ("verify_backup", Access::Needs(Permission::BackupRun)),
    ("request_restore", Access::Needs(Permission::BackupRun)),
    ("cancel_restore", Access::Needs(Permission::BackupRun)),
    ("find_shops", Access::Needs(Permission::BackupRun)),

    // --- development only ---------------------------------------------------
    // `#[cfg(debug_assertions)]` already keeps it out of a release build. It
    // still needs a permission, because a dev build is what a support engineer
    // runs on a shop's own machine.
    ("seed_demo_shop", Access::Needs(Permission::StaffManage)),
];

/// Refuse, or hand back who is doing this.
pub fn require(app: &App, need: Permission) -> UiResult<Actor> {
    let Some(session) = app.sessions().current() else {
        // The screen turns this code into the lock screen. It is not an error
        // the cashier has done anything about — they just need to sign in.
        return Err(UiError::new(
            "auth.locked",
            "The screen is locked. Sign in to carry on.",
        ));
    };

    if let Err(denied) = session.actor.must(need) {
        return Err(UiError::new(
            "auth.denied",
            format!(
                "{}, {}. Ask somebody who can.",
                capitalise(&denied.to_string()),
                session.actor.name
            ),
        )
        .with_detail(format!("needs {}", need.code())));
    }

    // Work happened. This is what feeds the idle clock — see `session`.
    app.sessions().touch(crate::flows::now());
    Ok(session.actor)
}

/// The four permissions the settings screen is built out of.
pub const SETTINGS_PERMISSIONS: &[Permission] = &[
    Permission::SettingsStore,
    Permission::SettingsTax,
    Permission::SettingsPrinter,
    Permission::BackupRun,
];

/// Refuse unless this person has **at least one** of these — see
/// [`Access::NeedsAny`].
pub fn require_any(app: &App, needs: &[Permission]) -> UiResult<Actor> {
    let Some(session) = app.sessions().current() else {
        return Err(UiError::new(
            "auth.locked",
            "The screen is locked. Sign in to carry on.",
        ));
    };
    if let Some(first) = needs.iter().find(|need| session.actor.must(**need).is_ok()) {
        // Through the ordinary door, so the idle clock is fed in exactly one
        // place and this cannot drift from it.
        return require(app, *first);
    }
    Err(UiError::new(
        "auth.denied",
        format!(
            "{}, you cannot change any of this shop's settings. Ask somebody \
             who can.",
            session.actor.name
        ),
    )
    .with_detail(format!(
        "needs one of {}",
        needs
            .iter()
            .map(|p| p.code())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

/// "you do not have permission to void a bill" → "You do not have permission to
/// void a bill". The sentence is built here rather than stored capitalised
/// because `AuthError`'s `Display` is also a log line.
fn capitalise(sentence: &str) -> String {
    let mut chars = sentence.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// What a command needs, or `None` if nobody has said.
#[must_use]
pub fn access_for(command: &str) -> Option<Access> {
    COMMAND_ACCESS
        .iter()
        .find(|(name, _)| *name == command)
        .map(|(_, access)| *access)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use mb_auth::PermissionSet;

    use crate::state::App;

    fn an_app() -> App {
        App::new(crate::config::AppConfig::default()).expect("the font loads")
    }

    fn a_waiter() -> mb_auth::Actor {
        mb_auth::Actor {
            staff_id: mb_core::StaffId::new("staff_waiter"),
            name: "Priya".to_owned(),
            role_id: Some("role_waiter".to_owned()),
            role_name: Some("Waiter".to_owned()),
            permissions: [Permission::BillCreate].into_iter().collect(),
            max_discount_bp: Some(0),
            max_discount: None,
        }
    }

    /// **T1 — and this is the test audit C1 is about.**
    ///
    /// The command is CALLED, not hidden. Every guarded command in the product
    /// funnels through `require`, and `every_command_is_classified` below proves
    /// there is no command that does not — so these two tests together are the
    /// claim that a permission cannot be got past from a dev console.
    #[test]
    fn a_command_is_refused_when_the_person_may_not_do_it() {
        let app = an_app();
        app.sessions()
            .begin(a_waiter(), crate::flows::now(), false);

        // What a waiter may do.
        assert!(require(&app, Permission::BillCreate).is_ok());

        // And what they may not. Audit C1 by name: "anybody who walks behind
        // the counter can open Reports and see the whole day's cash."
        for refused in [
            Permission::ReportsView,
            Permission::BillVoid,
            Permission::DrawerOpen,
            Permission::StaffManage,
            Permission::BackupRun,
        ] {
            let error = require(&app, refused).expect_err("it was allowed");
            assert_eq!(error.code, "auth.denied", "{refused:?}");
            assert!(
                error.message.contains("Priya"),
                "the refusal should name who is signed in: {}",
                error.message
            );
            assert!(
                error.detail.is_some_and(|d| d.contains(refused.code())),
                "the detail should carry the permission for a support call"
            );
        }
    }

    #[test]
    fn everything_is_refused_while_the_screen_is_locked() {
        let app = an_app();
        app.sessions().end();
        for need in Permission::ALL {
            let error = require(&app, *need).expect_err("it was allowed");
            assert_eq!(error.code, "auth.locked");
        }
    }

    /// **A shop that does not exist yet has nothing to lock.**
    ///
    /// Found by running it: the first version opened a first run straight onto
    /// the lock screen, with an empty staff list and no way past — nobody could
    /// create the shop that would hold the PIN that would let them in.
    #[test]
    fn a_first_run_is_not_locked_out_of_itself() {
        let app = an_app();
        let session = app.sessions().current().expect("somebody is at the counter");
        assert!(session.is_stand_in);
        assert!(require(&app, Permission::StaffManage).is_ok());
    }

    /// **T7's half that lives here.** Locking touches the session and nothing
    /// else — a shift change at 9 pm cannot cost a table its order.
    #[test]
    fn locking_does_not_touch_the_cart() {
        let app = an_app();
        app.sessions().begin(a_waiter(), crate::flows::now(), false);
        app.with_cart_mut(|state| {
            state.table = Some("tbl_7".to_owned());
            state.table_label = Some("7".to_owned());
            state.order_type = mb_core::OrderType::Parcel;
            Ok(())
        })
        .expect("the cart takes a table");

        app.sessions().end();

        app.with_cart(|state| {
            assert_eq!(state.table.as_deref(), Some("tbl_7"));
            assert_eq!(state.table_label.as_deref(), Some("7"));
            assert_eq!(state.order_type, mb_core::OrderType::Parcel);
            Ok(())
        })
        .expect("the cart is where it was");
    }

    /// Work feeds the idle clock, and it is the GUARD that feeds it — not a
    /// mouse event crossing the IPC boundary.
    #[test]
    fn a_guarded_command_keeps_the_screen_awake() {
        let app = an_app();
        let long_ago = mb_core::Timestamp::from_millis(0);
        app.sessions().begin(a_waiter(), long_ago, false);
        assert!(
            app.sessions()
                .is_idle(crate::flows::now(), crate::session::IDLE_LOCK),
            "it should have gone idle"
        );

        require(&app, Permission::BillCreate).expect("allowed");
        assert!(
            !app.sessions()
                .is_idle(crate::flows::now(), crate::session::IDLE_LOCK),
            "taking an order did not count as being at the counter"
        );
    }

    /// A refusal must NOT touch it, or somebody pressing a button they are not
    /// allowed to press would keep the till unlocked all night.
    #[test]
    fn a_refused_command_does_not() {
        let app = an_app();
        let long_ago = mb_core::Timestamp::from_millis(0);
        app.sessions().begin(a_waiter(), long_ago, false);
        let _ = require(&app, Permission::ReportsView);
        assert!(
            app.sessions()
                .is_idle(crate::flows::now(), crate::session::IDLE_LOCK),
            "a refusal counted as work"
        );
    }

    #[test]
    fn the_stand_in_may_do_everything_on_a_shop_with_no_pin() {
        // Requirement 3: a shop must be able to bill on its first day, and on
        // that day nobody has a PIN. See `session::stand_in_actor`.
        let app = an_app();
        app.sessions().begin(
            crate::session::stand_in_actor("Counter", "staff_default"),
            crate::flows::now(),
            true,
        );
        for need in Permission::ALL {
            assert!(require(&app, *need).is_ok(), "{need:?} was refused");
        }
        assert_eq!(PermissionSet::everything().len(), Permission::ALL.len());
    }

    /// Every `#[tauri::command]` in the two files that define them.
    ///
    /// `include_str!` rather than reading the file at runtime: the sources are
    /// baked in at compile time, so this cannot pass because somebody ran the
    /// test from the wrong directory.
    fn declared_commands() -> BTreeSet<String> {
        // **Every file that defines commands must be in this list**, and P12
        // proved why it is a risk worth naming: a new module's commands would
        // otherwise be invisible to the very test that exists to see them, and
        // the coverage check would pass while covering nothing.
        const SOURCES: [&str; 11] = [
            include_str!("ipc.rs"),
            include_str!("flows.rs"),
            include_str!("corrections.rs"),
            include_str!("menu.rs"),
            include_str!("floor.rs"),
            include_str!("credit.rs"),
            include_str!("expenses.rs"),
            include_str!("settings/ipc.rs"),
            include_str!("settings/printers.rs"),
            include_str!("settings/backup.rs"),
            include_str!("settings/numbering.rs"),
        ];
        let mut found = BTreeSet::new();
        for source in SOURCES {
            let mut lines = source.lines().peekable();
            while let Some(line) = lines.next() {
                if line.trim() != "#[tauri::command]" {
                    continue;
                }
                // The attribute may be followed by more attributes (`#[cfg]`),
                // then the signature.
                for next in lines.by_ref() {
                    let next = next.trim();
                    if next.starts_with('#') {
                        continue;
                    }
                    if let Some(rest) = next.strip_prefix("pub fn ") {
                        let name: String =
                            rest.chars().take_while(|c| *c != '(' && *c != '<').collect();
                        found.insert(name);
                    }
                    break;
                }
            }
        }
        found
    }

    /// **T11.** A command with no decision recorded fails the build.
    #[test]
    fn every_command_is_classified() {
        let declared = declared_commands();
        assert!(
            declared.len() > 20,
            "the scan found only {} commands, so it is broken rather than the code being clean",
            declared.len()
        );

        let classified: BTreeSet<String> = COMMAND_ACCESS
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect();

        let undecided: Vec<&String> = declared.difference(&classified).collect();
        assert!(
            undecided.is_empty(),
            "these commands exist with no access decision — add them to COMMAND_ACCESS, \
             choosing Access::Public on purpose if that is what you mean: {undecided:?}"
        );

        let ghosts: Vec<&String> = classified.difference(&declared).collect();
        assert!(
            ghosts.is_empty(),
            "these are classified but no longer exist — delete the line: {ghosts:?}"
        );
    }

    /// **And the file list itself is checked**, because it is the one part of
    /// this mechanism that fails silently.
    ///
    /// P12 found the hole: it added `corrections.rs` with eight commands, and
    /// `SOURCES` did not know about it — so the coverage test above would have
    /// passed while covering none of them. A guard that can be bypassed by
    /// adding a file is a guard with a hole shaped like a new feature.
    ///
    /// `CARGO_MANIFEST_DIR` is resolved at compile time, so this cannot pass by
    /// being run from the wrong directory.
    #[test]
    fn every_file_that_defines_commands_is_scanned() {
        // **Every .rs file under src/, at any depth.** P17 put commands in
        // `src/settings/ipc.rs`, and a scan that only read the top level would
        // have missed all five of them — which is the same hole P12 found by
        // adding `corrections.rs`, one directory deeper.
        fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("a readable directory") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    rust_files(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let mut missing = Vec::new();
        for path in files {
            let text = std::fs::read_to_string(&path).expect("a source file");
            // **A LINE that is the attribute**, not a file that merely mentions
            // it — the same rule the scanner uses. Matching on `contains` found
            // this file itself, whose scanner compares against that string.
            if !text.lines().any(|line| line.trim() == "#[tauri::command]") {
                continue;
            }
            let scanned = declared_commands();
            // Every command in this file must have been found by the scan.
            let names: Vec<String> = text
                .lines()
                .filter_map(|line| line.trim().strip_prefix("pub fn "))
                .map(|rest| rest.chars().take_while(|c| *c != '(' && *c != '<').collect())
                .collect();
            if !names.iter().any(|n: &String| scanned.contains(n)) {
                missing.push(path.display().to_string());
            }
        }
        assert!(
            missing.is_empty(),
            "these files define commands and are not in SOURCES, so the coverage \
             test cannot see them: {missing:?}"
        );
    }

    /// The scan is the load-bearing half of the test above, so it gets its own
    /// assertion rather than being trusted.
    #[test]
    fn the_scan_finds_commands_in_both_files() {
        let declared = declared_commands();
        assert!(declared.contains("app_status"), "missed ipc.rs");
        assert!(declared.contains("complete_bill"), "missed flows.rs");
        // One that is behind a #[cfg], which is the case that breaks a naive
        // "the line after the attribute" scan.
        assert!(declared.contains("seed_demo_shop"), "missed a cfg'd command");
    }

    /// Public is a decision, and a short list. If this number grows, somebody
    /// is making things public to get past the build.
    #[test]
    fn the_public_list_stays_short_and_deliberate() {
        let public = COMMAND_ACCESS
            .iter()
            .filter(|(_, access)| *access == Access::Public)
            .count();
        assert!(
            public <= 10,
            "{public} public commands is too many to still be a decision"
        );
    }

    #[test]
    fn nothing_is_classified_twice() {
        let mut seen = BTreeSet::new();
        for (name, _) in COMMAND_ACCESS {
            assert!(seen.insert(*name), "{name} is classified twice");
        }
    }

    #[test]
    fn a_refusal_names_the_person_and_says_what_to_do() {
        let sentence = capitalise("you do not have permission to void a bill");
        assert!(sentence.starts_with("You do not"));
    }
}
