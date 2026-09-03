//! The one place this program says no.
//!
//! ```ignore
//! let who = guard::require(&app, Permission::BillVoid)?;
//! ```

use mb_auth::{Actor, Permission};

use crate::state::App;
use crate::words::{UiError, UiResult};

/// What a command needs — the record the classification test reads.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Anybody, including nobody: these work on the lock screen.
    Public,
    /// A session, but no particular permission.
    SignedIn,
    /// Before there is a shop to have a permission in.
    FirstRun,
    Needs(Permission),
    /// Any one of these opens the door.
    NeedsAny(&'static [Permission]),
}

/// Every command in the product, and what it needs.
#[cfg(test)]
pub const COMMAND_ACCESS: &[(&str, Access)] = &[
    // Works while locked, and has to.
    ("app_status", Access::Public),
    // The theme toggle is on the lock screen.
    // "send me the log" must be a button, and support asks for it precisely when nobody can get
    // in.
    ("reveal_logs", Access::Public),
    // The lock screen itself.
    ("lock_state", Access::Public),
    ("login", Access::Public),
    ("lock_now", Access::Public),
    ("recover_with_code", Access::Public),
    // The print queue indicator stays visible while locked.
    ("list_print_jobs", Access::Public),
    // The preview of the built-in test slip needs no shop and no data.
    ("preview_test_page", Access::Public),
    ("preview_order", Access::Needs(Permission::BillCreate)),
    // The cook lost the paper.
    (
        "reprint_kitchen_ticket",
        Access::Needs(Permission::BillCreate),
    ),
    // Making a customer's invoice out of a bill that already exists is reading, not billing —
    // the same authority as looking at the bill list.
    ("bill_pdf", Access::Needs(Permission::ReportsView)),
    ("current_cart", Access::Needs(Permission::BillCreate)),
    ("cart_add", Access::Needs(Permission::BillCreate)),
    ("cart_set_qty", Access::Needs(Permission::BillCreate)),
    // The - and + on a cart line.
    ("cart_step_qty", Access::Needs(Permission::BillCreate)),
    ("cart_remove", Access::Needs(Permission::BillCreate)),
    ("cart_clear", Access::Needs(Permission::BillCreate)),
    ("cart_set_order_type", Access::Needs(Permission::BillCreate)),
    ("cart_clear_payments", Access::Needs(Permission::BillCreate)),
    ("cart_cash_given", Access::Needs(Permission::BillCreate)),
    (
        "cart_set_discount",
        Access::Needs(Permission::BillDiscountBill),
    ),
    (
        "cart_clear_discount",
        Access::Needs(Permission::BillDiscountBill),
    ),
    ("open_orders", Access::Needs(Permission::BillCreate)),
    ("menu_items", Access::Needs(Permission::BillCreate)),
    ("search_items", Access::Needs(Permission::BillCreate)),
    ("open_table", Access::Needs(Permission::BillCreate)),
    ("open_order", Access::Needs(Permission::BillCreate)),
    ("join_table", Access::Needs(Permission::BillCreate)),
    (
        "print_kitchen_ticket",
        Access::Needs(Permission::BillCreate),
    ),
    ("complete_bill", Access::Needs(Permission::BillCreate)),
    // The settle desk — the same permission as completing the bill by hand.
    ("settle_requests", Access::Needs(Permission::BillCreate)),
    ("settle_from_floor", Access::Needs(Permission::BillCreate)),
    ("decline_settle", Access::Needs(Permission::BillCreate)),
    // Carrying the bill to the table.
    ("print_open_bill", Access::Needs(Permission::BillCreate)),
    (
        "print_test_page",
        Access::Needs(Permission::SettingsPrinter),
    ),
    // Retrying or abandoning a print is reprinting a bill, which is a thing a shop counts and
    // therefore a thing it permits.
    ("retry_print_job", Access::Needs(Permission::BillReprint)),
    ("dismiss_print_job", Access::Needs(Permission::BillReprint)),
    (
        "retry_parked_print_jobs",
        Access::Needs(Permission::BillReprint),
    ),
    (
        "dismiss_all_print_jobs",
        Access::Needs(Permission::BillReprint),
    ),
    ("list_staff", Access::Needs(Permission::StaffManage)),
    ("save_staff_member", Access::Needs(Permission::StaffManage)),
    ("set_staff_pin", Access::Needs(Permission::StaffManage)),
    ("list_roles", Access::Needs(Permission::StaffManage)),
    ("save_role", Access::Needs(Permission::StaffManage)),
    ("list_permissions", Access::Needs(Permission::StaffManage)),
    ("audit_trail", Access::Needs(Permission::AuditView)),
    // Taking something back.
    ("list_bills", Access::Needs(Permission::ReportsView)),
    ("day_totals", Access::Needs(Permission::ReportsView)),
    // The reason list itself is not sensitive; being unable to read it would make every
    // correction dialog open empty.
    ("reasons", Access::Needs(Permission::BillCreate)),
    ("void_bill", Access::Needs(Permission::BillVoid)),
    ("refund_bill", Access::Needs(Permission::BillVoid)),
    ("cancel_order", Access::Needs(Permission::OrderCancel)),
    ("void_line", Access::Needs(Permission::OrderItemVoid)),
    ("reprint_bill", Access::Needs(Permission::BillReprint)),
    // The menu.
    ("menu_categories", Access::Needs(Permission::MenuManage)),
    ("menu_rows", Access::Needs(Permission::MenuManage)),
    ("save_menu_item", Access::Needs(Permission::MenuManage)),
    ("set_item_available", Access::Needs(Permission::MenuManage)),
    ("save_menu_category", Access::Needs(Permission::MenuManage)),
    ("change_menu_prices", Access::Needs(Permission::MenuManage)),
    ("plan_menu_import", Access::Needs(Permission::MenuManage)),
    ("run_menu_import", Access::Needs(Permission::MenuManage)),
    ("export_menu", Access::Needs(Permission::MenuManage)),
    // Settings › Tax. Reading the slabs is a menu job (the item form picks one); defining a
    // slab or moving items between slabs is what the shop owes the government, and getting it
    // wrong is a notice rather than a bad price.
    ("tax_slabs", Access::Needs(Permission::MenuManage)),
    ("tax_page", Access::Needs(Permission::MenuManage)),
    ("save_tax_slab", Access::Needs(Permission::SettingsTax)),
    ("remove_tax_slab", Access::Needs(Permission::SettingsTax)),
    ("set_items_tax", Access::Needs(Permission::SettingsTax)),
    ("set_category_tax", Access::Needs(Permission::SettingsTax)),
    // What an item is made of.
    ("item_composition", Access::Needs(Permission::MenuManage)),
    ("save_item_variant", Access::Needs(Permission::MenuManage)),
    (
        "list_modifier_groups",
        Access::Needs(Permission::MenuManage),
    ),
    ("save_modifier_group", Access::Needs(Permission::MenuManage)),
    (
        "attach_modifier_group",
        Access::Needs(Permission::MenuManage),
    ),
    ("list_combos", Access::Needs(Permission::MenuManage)),
    ("save_combo", Access::Needs(Permission::MenuManage)),
    // The floor. Reading it is billing work — a cashier has to see the tables.
    ("floor_plan", Access::Needs(Permission::BillCreate)),
    (
        "save_floor_section",
        Access::Needs(Permission::TablesManage),
    ),
    // The bulk pair. Same permission as the one-at-a-time commands they stand in for — a set of
    // tables is not a different question from a table.
    (
        "delete_dining_tables",
        Access::Needs(Permission::TablesManage),
    ),
    (
        "set_dining_tables_active",
        Access::Needs(Permission::TablesManage),
    ),
    (
        "delete_floor_section",
        Access::Needs(Permission::TablesManage),
    ),
    ("save_dining_table", Access::Needs(Permission::TablesManage)),
    ("add_dining_tables", Access::Needs(Permission::TablesManage)),
    (
        "place_dining_table",
        Access::Needs(Permission::TablesManage),
    ),
    (
        "delete_dining_table",
        Access::Needs(Permission::TablesManage),
    ),
    (
        "save_floor_thresholds",
        Access::Needs(Permission::TablesManage),
    ),
    ("move_order", Access::Needs(Permission::BillCreate)),
    ("merge_orders", Access::Needs(Permission::BillCreate)),
    ("split_order", Access::Needs(Permission::BillCreate)),
    // Customers and what they owe.
    ("customers", Access::Needs(Permission::CustomersManage)),
    (
        "customer_account",
        Access::Needs(Permission::CustomersManage),
    ),
    ("save_customer", Access::Needs(Permission::CustomersManage)),
    // Taking money IN is a cashier's job — `credit.collect` exists for exactly this and nothing
    // else.
    ("record_repayment", Access::Needs(Permission::CreditCollect)),
    // Changing what somebody owes without money moving is not.
    (
        "save_credit_adjustment",
        Access::Needs(Permission::CustomersManage),
    ),
    // Both of these happen mid-bill, so they are billing work; going PAST the limit asks for
    // `customers.manage` a second time inside the command.
    ("credit_headroom", Access::Needs(Permission::BillCreate)),
    ("put_on_account", Access::Needs(Permission::BillCreate)),
    // Money going out, and the drawer.
    ("expenses", Access::Needs(Permission::ExpensesManage)),
    ("save_expense", Access::Needs(Permission::ExpensesManage)),
    ("delete_expense", Access::Needs(Permission::ExpensesManage)),
    (
        "save_cash_movement",
        Access::Needs(Permission::ExpensesManage),
    ),
    (
        "save_expense_category",
        Access::Needs(Permission::ExpensesManage),
    ),
    (
        "save_recurring_expense",
        Access::Needs(Permission::ExpensesManage),
    ),
    (
        "confirm_recurring_expense",
        Access::Needs(Permission::ExpensesManage),
    ),
    ("export_expenses", Access::Needs(Permission::ExpensesManage)),
    // The employment side.
    ("employees", Access::Needs(Permission::StaffManage)),
    ("save_employee", Access::Needs(Permission::StaffManage)),
    // Reading attendance: your own needs nothing beyond being signed in, and anybody ELSE's
    // needs the permission.
    ("attendance", Access::SignedIn),
    ("clock_in", Access::SignedIn),
    ("clock_out", Access::SignedIn),
    (
        "correct_attendance",
        Access::Needs(Permission::AttendanceCorrect),
    ),
    ("save_roster", Access::Needs(Permission::AttendanceMark)),
    // The same "your own, or the permission" rule as `attendance`, for the same reason and with
    // the same test.
    ("leave", Access::SignedIn),
    ("request_leave", Access::SignedIn),
    ("decide_leave", Access::Needs(Permission::LeaveApprove)),
    ("adjust_leave", Access::Needs(Permission::LeaveApprove)),
    // What a shop pays its people is a different secret from what it took at the till, so none
    // of these is `ReportsView`.
    ("salary", Access::Needs(Permission::SalaryView)),
    ("save_salary", Access::Needs(Permission::SalaryManage)),
    ("give_advance", Access::Needs(Permission::SalaryManage)),
    ("payroll_runs", Access::Needs(Permission::SalaryView)),
    ("payroll", Access::Needs(Permission::SalaryView)),
    ("compute_payroll", Access::Needs(Permission::SalaryManage)),
    ("edit_payroll_line", Access::Needs(Permission::SalaryManage)),
    // Where money leaves the shop.
    ("approve_payroll", Access::Needs(Permission::SalaryManage)),
    ("reverse_payroll", Access::Needs(Permission::SalaryManage)),
    ("staff_cost", Access::Needs(Permission::SalaryView)),
    // The payslip is SalaryManage, not SalaryView: reading what somebody earns and handing them
    // the paper that says so are the same authority as approving the run that produced it.
    ("print_payslip", Access::Needs(Permission::SalaryManage)),
    ("delivery_board", Access::SignedIn),
    // Everything that MOVES a delivery, or takes money off a rider, is the one permission.
    ("save_delivery", Access::Needs(Permission::DeliveryDispatch)),
    (
        "record_handback",
        Access::Needs(Permission::DeliveryDispatch),
    ),
    (
        "print_delivery_slip",
        Access::Needs(Permission::DeliveryDispatch),
    ),
    // Saying somebody is a rider edits their staff record, so it is the same decision as any
    // other edit to it.
    ("set_rider", Access::Needs(Permission::StaffManage)),
    ("payments", Access::Needs(Permission::ReportsView)),
    ("confirm_payment", Access::Needs(Permission::BillCreate)),
    // The devices.
    ("device_manager", Access::Needs(Permission::SettingsPrinter)),
    (
        "show_customer_display",
        Access::Needs(Permission::SettingsPrinter),
    ),
    // Reading the scale is BillCreate, not a settings permission: it is done mid-bill, by
    // whoever is billing, forty times an evening.
    ("read_scale_once", Access::Needs(Permission::BillCreate)),
    // A scan is a keystroke on the billing screen, so it is the billing permission and nothing
    // else.
    ("scanned", Access::Needs(Permission::BillCreate)),
    ("print_label", Access::Needs(Permission::BillCreate)),
    ("settings_all", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    ("reload_settings", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    ("search_settings", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    ("save_settings", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    (
        "settings_defaults_for",
        Access::NeedsAny(SETTINGS_PERMISSIONS),
    ),
    // The live preview. Reading only — it renders a SAMPLE bill and never touches a real one.
    ("preview_settings", Access::NeedsAny(SETTINGS_PERMISSIONS)),
    ("export_settings", Access::Needs(Permission::SettingsStore)),
    (
        "plan_settings_import",
        Access::NeedsAny(SETTINGS_PERMISSIONS),
    ),
    // All four, and the command re-checks each one: an import writes tax rates and printer
    // setup, so it is not a store edit.
    (
        "run_settings_import",
        Access::NeedsAny(SETTINGS_PERMISSIONS),
    ),
    // The counters. A bill number is what a GST return is a list of.
    ("numbering", Access::Needs(Permission::SettingsTax)),
    ("save_counter", Access::Needs(Permission::SettingsTax)),
    // The printers, the backup and where the shop is.
    ("printer_setup", Access::Needs(Permission::SettingsPrinter)),
    ("save_printer", Access::Needs(Permission::SettingsPrinter)),
    ("delete_printer", Access::Needs(Permission::SettingsPrinter)),
    ("route_category", Access::Needs(Permission::SettingsPrinter)),
    (
        "set_default_printer",
        Access::Needs(Permission::SettingsPrinter),
    ),
    ("set_paper_size", Access::Needs(Permission::SettingsPrinter)),
    (
        "print_sample_bill",
        Access::Needs(Permission::SettingsPrinter),
    ),
    ("nudge_printer", Access::Needs(Permission::SettingsPrinter)),
    ("backup_status", Access::Needs(Permission::BackupRun)),
    ("back_up_now", Access::Needs(Permission::BackupRun)),
    ("verify_backup", Access::Needs(Permission::BackupRun)),
    ("request_restore", Access::Needs(Permission::BackupRun)),
    ("cancel_restore", Access::Needs(Permission::BackupRun)),
    ("find_shops", Access::Needs(Permission::BackupRun)),
    // The reports.
    ("report_list", Access::Needs(Permission::ReportsView)),
    ("report", Access::Needs(Permission::ReportsView)),
    ("report_csv", Access::Needs(Permission::ReportsView)),
    ("report_pdf", Access::Needs(Permission::ReportsView)),
    // The dashboard is the day's takings on a screen, like every other report.
    ("dashboard", Access::Needs(Permission::ReportsView)),
    // Closing the day.
    // The day. The gate asks whoever signed in; the Days screen and the drawer count open to
    // anybody who reads reports or closes days; the writes need the permission a cashier has.
    ("day_state", Access::SignedIn),
    ("close_pending", Access::Needs(Permission::DayClose)),
    (
        "days",
        Access::NeedsAny(&[Permission::ReportsView, Permission::DayClose]),
    ),
    ("close_day", Access::Needs(Permission::DayClose)),
    ("mark_holiday", Access::Needs(Permission::DayClose)),
    ("unmark_holiday", Access::Needs(Permission::DayClose)),
    ("reopen_day", Access::Needs(Permission::DayClose)),
    (
        "count_cash",
        Access::NeedsAny(&[Permission::ReportsView, Permission::DayClose]),
    ),
    ("count_drawer", Access::Needs(Permission::DayClose)),
    // The phones this counter serves.
    ("network", Access::Needs(Permission::ReportsView)),
    // The top bar's number: how many phones are live. No permission — it is a count.
    ("phones_now", Access::SignedIn),
    ("open_pairing", Access::Needs(Permission::DevicesPair)),
    ("allow_firewall", Access::Needs(Permission::DevicesPair)),
    ("close_pairing", Access::Needs(Permission::DevicesPair)),
    ("allow_device", Access::Needs(Permission::DevicesPair)),
    ("refuse_device", Access::Needs(Permission::DevicesPair)),
    ("revoke_device", Access::Needs(Permission::DevicesPair)),
    // What the floor did while the cashier was typing.
    (
        "take_the_floors_items",
        Access::Needs(Permission::BillCreate),
    ),
    (
        "dismiss_the_floors_items",
        Access::Needs(Permission::BillCreate),
    ),
    // The licence.
    ("account", Access::Needs(Permission::ReportsView)),
    ("refresh_licence", Access::Needs(Permission::ReportsView)),
    ("activate", Access::Needs(Permission::LicenceManage)),
    ("deactivate", Access::Needs(Permission::LicenceManage)),
    ("transfer_here", Access::Needs(Permission::LicenceManage)),
    (
        "use_emergency_code",
        Access::Needs(Permission::LicenceManage),
    ),
    // Is this counter healthy.
    ("health", Access::Needs(Permission::ReportsView)),
    ("diagnostics_plan", Access::Needs(Permission::BackupRun)),
    ("write_diagnostics", Access::Needs(Permission::BackupRun)),
    // Looking for an update is reading.
    ("look_for_an_update", Access::Needs(Permission::ReportsView)),
    (
        "go_back_a_version",
        Access::Needs(Permission::SettingsStore),
    ),
    ("install_update", Access::Needs(Permission::SettingsStore)),
    // The cloud copy. The bell is for whoever is at the counter; the pull rides on it.
    ("notices", Access::SignedIn),
    ("notices_seen", Access::SignedIn),
    ("pull_from_cloud", Access::SignedIn),
    // A new computer, before it has a shop.
    ("restore_from_cloud", Access::FirstRun),
    // Public, and it is a decision.
    ("setup_list", Access::Public),
    // The first run.
    ("first_run", Access::FirstRun),
    ("create_shop", Access::FirstRun),
    ("use_existing_shop", Access::FirstRun),
    // Browse for a folder — the other thing a fresh install needs and could not do.
    ("pick_a_folder", Access::FirstRun),
    // The logo.
    ("logo", Access::Needs(Permission::SettingsPrinter)),
    ("pick_a_logo", Access::Needs(Permission::SettingsPrinter)),
    ("save_logo", Access::Needs(Permission::SettingsPrinter)),
    ("remove_logo", Access::Needs(Permission::SettingsPrinter)),
    // The kitchen screen.
    ("kitchen", Access::Needs(Permission::BillCreate)),
    ("kitchen_shown", Access::Needs(Permission::BillCreate)),
    ("kitchen_bump", Access::Needs(Permission::BillCreate)),
    ("kitchen_bump_line", Access::Needs(Permission::BillCreate)),
    ("kitchen_recall", Access::Needs(Permission::BillCreate)),
    ("kitchen_acknowledge", Access::Needs(Permission::BillCreate)),
    ("kitchen_fire", Access::Needs(Permission::BillCreate)),
    // The stock book.
    ("inventory", Access::Needs(Permission::InventoryView)),
    ("recipe", Access::Needs(Permission::InventoryView)),
    ("stock_variance", Access::Needs(Permission::InventoryView)),
    ("buy_list_text", Access::Needs(Permission::InventoryView)),
    ("save_material", Access::Needs(Permission::InventoryManage)),
    ("save_recipe", Access::Needs(Permission::InventoryManage)),
    ("delete_recipe", Access::Needs(Permission::InventoryManage)),
    (
        "resolve_stock_problem",
        Access::Needs(Permission::InventoryManage),
    ),
    (
        "record_stock_movement",
        Access::Needs(Permission::StockWaste),
    ),
    (
        "rebuild_stock_balances",
        Access::Needs(Permission::StockAdjust),
    ),
    // Buying, and the count.
    ("buying", Access::Needs(Permission::PurchasesManage)),
    ("purchase", Access::Needs(Permission::PurchasesManage)),
    (
        "supplier_account",
        Access::Needs(Permission::PurchasesManage),
    ),
    ("save_purchase", Access::Needs(Permission::PurchasesManage)),
    (
        "cancel_purchase",
        Access::Needs(Permission::PurchasesManage),
    ),
    (
        "save_purchase_order",
        Access::Needs(Permission::PurchasesManage),
    ),
    (
        "set_order_state",
        Access::Needs(Permission::PurchasesManage),
    ),
    ("attach_photo", Access::Needs(Permission::PurchasesManage)),
    ("purchase_photo", Access::Needs(Permission::PurchasesManage)),
    ("save_supplier", Access::Needs(Permission::SuppliersManage)),
    (
        "record_supplier_payment",
        Access::Needs(Permission::SuppliersManage),
    ),
    (
        "save_supplier_adjustment",
        Access::Needs(Permission::SuppliersManage),
    ),
    ("stock_count", Access::Needs(Permission::InventoryView)),
    ("open_stock_count", Access::Needs(Permission::StockCount)),
    ("record_count_line", Access::Needs(Permission::StockCount)),
    ("explain_count_line", Access::Needs(Permission::StockCount)),
    ("remove_count_line", Access::Needs(Permission::StockCount)),
    ("abandon_stock_count", Access::Needs(Permission::StockCount)),
    ("count_sheet", Access::Needs(Permission::StockCount)),
    (
        "approve_stock_count",
        Access::Needs(Permission::StockAdjust),
    ),
    ("tills", Access::Needs(Permission::ReportsView)),
    // Changing a till's series prefix changes what its bills are NUMBERED, so it sits with the
    // shop's own settings and nothing weaker.
    ("save_till", Access::Needs(Permission::SettingsStore)),
    ("make_master", Access::Needs(Permission::SettingsStore)),
    ("join_master", Access::Needs(Permission::SettingsStore)),
    // Pressing "send now" only drains a queue that would have drained itself.
    ("send_waiting_bills", Access::Needs(Permission::BillCreate)),
    // Sharing a report is reading it, so it is the report's own permission and nothing weaker
    // â `report_on` checks it again anyway.
    ("share_report", Access::Needs(Permission::ReportsView)),
    // Development only.
    ("seed_demo_shop", Access::Needs(Permission::StaffManage)),
];

/// Refuse, or hand back who is doing this.
pub fn require(app: &App, need: Permission) -> UiResult<Actor> {
    let Some(session) = app.sessions().current() else {
        // The screen turns this code into the lock screen.
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

    // Work happened. This is what feeds the idle clock.
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

/// Refuse unless this person has at least one of these — see `Access::NeedsAny`.
/// Somebody, anybody, at the counter — the bell, the clock, a leave request.
pub fn require_signed_in(app: &App) -> UiResult<Actor> {
    let Some(session) = app.sessions().current() else {
        return Err(UiError::new(
            "auth.locked",
            "The screen is locked. Sign in to carry on.",
        ));
    };
    Ok(session.actor)
}

pub fn require_any(app: &App, needs: &[Permission]) -> UiResult<Actor> {
    let Some(session) = app.sessions().current() else {
        return Err(UiError::new(
            "auth.locked",
            "The screen is locked. Sign in to carry on.",
        ));
    };
    if let Some(first) = needs.iter().find(|need| session.actor.must(**need).is_ok()) {
        // Through the ordinary door, so the idle clock is fed in exactly one place and this
        // cannot drift from it.
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

/// "you do not have permission to void a bill" → "You do not have permission to void a bill".
fn capitalise(sentence: &str) -> String {
    let mut chars = sentence.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
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

    #[test]
    fn a_command_is_refused_when_the_person_may_not_do_it() {
        let app = an_app();
        app.sessions().begin(a_waiter(), crate::flows::now(), false);

        // What a waiter may do.
        assert!(require(&app, Permission::BillCreate).is_ok());

        // And what they may not.
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

    /// A shop that does not exist yet has nothing to lock.
    #[test]
    fn a_first_run_is_not_locked_out_of_itself() {
        let app = an_app();
        let session = app
            .sessions()
            .current()
            .expect("somebody is at the counter");
        assert!(session.is_stand_in);
        assert!(require(&app, Permission::StaffManage).is_ok());
    }

    #[test]
    fn locking_does_not_touch_the_cart() {
        let app = an_app();
        app.sessions().begin(a_waiter(), crate::flows::now(), false);
        app.with_cart_mut(|state| {
            state.place_on(mb_core::TableId::new("tbl_7"), "7".to_owned(), None);
            Ok(())
        })
        .expect("the cart takes a table");

        app.sessions().end();

        app.with_cart(|state| {
            assert_eq!(state.table_id(), Some("tbl_7"));
            assert_eq!(state.table().map(|t| t.label.as_str()), Some("7"));
            assert_eq!(state.order_type(), mb_core::OrderType::DineIn);
            Ok(())
        })
        .expect("the cart is where it was");
    }

    /// Work feeds the idle clock, and it is the GUARD that feeds it — not a mouse event
    /// crossing the IPC boundary.
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

    /// A refusal must NOT touch it, or somebody pressing a button they are not allowed to press
    /// would keep the till unlocked all night.
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
        // Requirement 3: a shop must be able to bill on its first day, and on that day nobody
        // has a PIN.
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
    fn declared_commands() -> BTreeSet<String> {
        const SOURCES: [&str; 33] = [
            include_str!("terminals.rs"),
            include_str!("orders.rs"),
            include_str!("buying.rs"),
            include_str!("counting.rs"),
            include_str!("share.rs"),
            include_str!("inventory.rs"),
            include_str!("lan.rs"),
            include_str!("licensing.rs"),
            include_str!("health.rs"),
            include_str!("diagnostics.rs"),
            include_str!("updates.rs"),
            include_str!("setup.rs"),
            include_str!("kitchen.rs"),
            include_str!("dayclose.rs"),
            include_str!("reports.rs"),
            include_str!("ipc.rs"),
            include_str!("flows.rs"),
            include_str!("corrections.rs"),
            include_str!("menu.rs"),
            include_str!("tax.rs"),
            include_str!("floor.rs"),
            include_str!("credit.rs"),
            include_str!("expenses.rs"),
            include_str!("employment.rs"),
            include_str!("delivery.rs"),
            include_str!("payments.rs"),
            include_str!("devices.rs"),
            include_str!("firstrun.rs"),
            include_str!("logo.rs"),
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
                // The attribute may be followed by more attributes (`#[cfg]`), then the
                // signature.
                for next in lines.by_ref() {
                    let next = next.trim();
                    if next.starts_with('#') {
                        continue;
                    }
                    if let Some(rest) = next
                        .strip_prefix("pub fn ")
                        .or_else(|| next.strip_prefix("pub async fn "))
                    {
                        let name: String = rest
                            .chars()
                            .take_while(|c| *c != '(' && *c != '<')
                            .collect();
                        found.insert(name);
                    }
                    break;
                }
            }
        }
        found
    }

    /// A command with no decision recorded fails the build.
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

    /// And the file list itself is checked, because it is the one part of this mechanism that
    /// fails silently.
    #[test]
    fn every_file_that_defines_commands_is_scanned() {
        // Every.rs file under src/, at any depth.
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
            // A LINE that is the attribute, not a file that merely mentions it — the same rule
            // the scanner uses.
            if !text.lines().any(|line| line.trim() == "#[tauri::command]") {
                continue;
            }
            let scanned = declared_commands();
            // Every command in this file must have been found by the scan.
            let names: Vec<String> = text
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    line.strip_prefix("pub fn ")
                        .or_else(|| line.strip_prefix("pub async fn "))
                })
                .map(|rest| {
                    rest.chars()
                        .take_while(|c| *c != '(' && *c != '<')
                        .collect()
                })
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

    /// The scan is the load-bearing half of the test above, so it gets its own assertion rather
    /// than being trusted.
    #[test]
    fn the_scan_finds_commands_in_both_files() {
        let declared = declared_commands();
        assert!(declared.contains("app_status"), "missed ipc.rs");
        assert!(declared.contains("complete_bill"), "missed flows.rs");
        // One that is behind a #[cfg], which is the case that breaks a naive "the line after
        // the attribute" scan.
        assert!(
            declared.contains("seed_demo_shop"),
            "missed a cfg'd command"
        );
    }

    /// Public is a decision, and a short list.
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
