//! The IPC boundary. One command per use case, named for the use case.

use mb_core::{BusinessDay, Timestamp};
use mb_print::printer::{PrinterConfig, Target};
use mb_print::queue::{Job, JobKind};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::state::{App, OUTLET, PrintJobView};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

// Money, as it crosses.

/// The only shape money takes on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MoneyView {
    pub paise: i64,
    pub text: String,
}

/// A rate from whole percent, for the demo shop's fixed menu.
#[cfg(debug_assertions)]
fn demo_rate(percent: u32) -> mb_core::TaxRate {
    mb_core::TaxRate::from_percent(percent).unwrap_or(mb_core::TaxRate::ZERO)
}

/// A stored count, made sendable.
#[must_use]
pub fn count(n: i64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<mb_core::Money> for MoneyView {
    fn from(money: mb_core::Money) -> Self {
        MoneyView {
            paise: money.paise(),
            text: money.to_plain_string(),
        }
    }
}

// What the app is, right now.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    /// False on a first run.
    pub has_shop: bool,
    pub shop_path: Option<String>,
    pub version: String,
    /// Where the logs are, so "send me the log" is a button and not a phone call about file
    /// paths.
    pub logs_path: Option<String>,
    /// The licence banner, or empty.
    pub licence: String,
    /// `ok`, `warn` or `danger`.
    pub licence_tone: String,
    /// Orders go to a Kitchen screen, so the shell shows one.
    pub kitchen_screen: bool,
}

#[tauri::command]
pub fn app_status(app: tauri::State<'_, App>) -> AppStatus {
    AppStatus {
        has_shop: app.has_shop(),
        shop_path: app
            .with_shop(|shop| Ok(shop.path.display().to_string()))
            .ok(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        logs_path: crate::logging::directory().map(|p| p.display().to_string()),
        licence: {
            let at = crate::flows::now();
            crate::words::licence_banner(&app.entitlement(), crate::flows::today(at))
                .unwrap_or_default()
        },
        licence_tone: crate::licensing::tone_for(app.entitlement().standing).to_owned(),
        kitchen_screen: app.shop_config().billing.kitchen_screen,
    }
}

/// A notice from Magic Bill, as the bell shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct NoticeView {
    pub id: String,
    pub title: String,
    pub body: String,
    /// "8 Aug, 4:32 pm".
    pub when: String,
    pub is_seen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct NoticesView {
    pub unseen: u32,
    pub notices: Vec<NoticeView>,
}

/// What the bell holds, newest first.
pub fn notices_on(app: &App) -> UiResult<NoticesView> {
    crate::guard::require_signed_in(app)?;
    let at = crate::flows::now();
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let notices = repos
                    .notices()
                    .list(crate::state::OUTLET, at)?
                    .into_iter()
                    .map(|n| NoticeView {
                        id: n.id,
                        title: n.title,
                        body: n.body,
                        when: crate::words::when(n.starts_at),
                        is_seen: n.is_seen,
                    })
                    .collect();
                Ok(NoticesView {
                    unseen: repos.notices().unseen(crate::state::OUTLET, at)?,
                    notices,
                })
            })
            .map_err(|e| crate::words::from_db(&e))
    })
}

/// The bell was opened: everything current is seen.
pub fn notices_seen_on(app: &App) -> UiResult<NoticesView> {
    crate::guard::require_signed_in(app)?;
    let at = crate::flows::now();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .notices()
                    .mark_all_seen(crate::state::OUTLET, at)
            })
            .map_err(|e| crate::words::from_db(&e))
    })?;
    let view = notices_on(app)?;
    app.push(crate::state::Pushed::Notices { unseen: view.unseen });
    Ok(view)
}

#[tauri::command]
pub fn notices(app: tauri::State<'_, App>) -> UiResult<NoticesView> {
    notices_on(&app)
}

#[tauri::command]
pub fn notices_seen(app: tauri::State<'_, App>) -> UiResult<NoticesView> {
    notices_seen_on(&app)
}

/// Ask the cloud for the people list and the notices now — the Staff screen and the bell.
#[tauri::command]
pub fn pull_from_cloud(app: tauri::State<'_, App>) -> UiResult<NoticesView> {
    crate::guard::require_signed_in(&app)?;
    match crate::sync::pull_once(&app) {
        Ok(_) => {}
        // A counter with no cloud yet, or none right now, still shows what it has.
        Err(e) if e.tone == crate::words::Tone::Notice || e.code == "licence.cloud" => {}
        Err(e) => return Err(e),
    }
    notices_on(&app)
}

/// "there is nothing to read".
#[tauri::command]
pub fn reveal_logs() -> UiResult<String> {
    let Some(dir) = crate::logging::directory() else {
        return Err(UiError::new(
            "logs.none",
            "There is no log folder yet. Restart Magic Bill and try again.",
        ));
    };
    Ok(dir.display().to_string())
}

/// A printer, as a settings screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PrinterView {
    pub id: String,
    pub name: String,
    pub connection: String,
    pub paper_mm: i64,
    pub is_default: bool,
    pub offset_x_mm: i64,
    pub offset_y_mm: i64,
}

/// The test print.
#[tauri::command]
pub fn print_test_page(app: tauri::State<'_, App>, printer_id: String) -> UiResult<String> {
    guard::require(&app, Permission::SettingsPrinter)?;
    let printer = find_printer(&app, &printer_id)?;
    let document = mb_print::testprint::test_document(&printer, None);
    let day = crate::flows::today(crate::flows::now());

    let job = Job::new(JobKind::Test, &printer.id, document, day).because("test print");

    // With a shop, the job is durable and survives a power cut.
    let queued = app.print(job.clone());

    match queued {
        Ok(id) => Ok(id),
        Err(e) if e.code == "shop.none" => {
            log_info!("test print with no shop open — using an in-memory queue");
            let queue = app.transient_queue(vec![printer]);
            // The one enqueue outside `App::print`, and it has to be: there is no shop, so
            // there is no chosen typeface to stamp on.
            let id = queue.enqueue(job).map_err(|e| words::from_print(&e))?;
            // The transient queue is dropped with the print in flight; the worker thread owns
            // the job and finishes it.
            std::mem::forget(queue);
            Ok(id)
        }
        Err(e) => Err(e),
    }
}

pub fn nudge_offset_on(
    app: &App,
    printer_id: String,
    dx_mm: i32,
    dy_mm: i32,
) -> UiResult<PrinterView> {
    guard::require(app, Permission::SettingsPrinter)?;
    app.with_shop(|shop| {
        let mut rows = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .map_err(|e| words::from_db(&e))?;
        let Some(row) = rows.iter_mut().find(|p| p.id == printer_id) else {
            return Err(UiError::new(
                "printer.unknown",
                "That printer is not set up any more. Choose another one.",
            ));
        };

        // Clamped by mb-print, so there is one rule rather than two: a nudge that could ever be
        // a real correction is +/- 20 mm.
        let mut config = PrinterConfig::new(&row.id, &row.name, Target::None);
        config.paper.offset = mb_print::paper::Offset::new(
            i32::try_from(row.offset_x_mm).unwrap_or(0),
            i32::try_from(row.offset_y_mm).unwrap_or(0),
        );
        mb_print::printer::nudge(&mut config, dx_mm, dy_mm);
        row.offset_x_mm = i64::from(config.paper.offset.x_mm);
        row.offset_y_mm = i64::from(config.paper.offset.y_mm);

        let row = row.clone();
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .settings()
                    .save_printer(OUTLET, &row, crate::flows::now())
            })
            .map_err(|e| words::from_db(&e))?;

        log_info!(
            "print offset for {} is now {} mm across, {} mm down",
            row.name,
            row.offset_x_mm,
            row.offset_y_mm
        );

        Ok(PrinterView {
            connection: String::new(),
            id: row.id,
            name: row.name,
            paper_mm: row.paper_mm,
            is_default: row.is_default,
            offset_x_mm: row.offset_x_mm,
            offset_y_mm: row.offset_y_mm,
        })
    })
}

/// What the shell's indicator shows.
#[tauri::command]
pub fn list_print_jobs(app: tauri::State<'_, App>) -> UiResult<Vec<PrintJobView>> {
    app.with_shop(|shop| Ok(shop.queue.snapshot().iter().map(to_view).collect()))
}

#[tauri::command]
pub fn retry_print_job(app: tauri::State<'_, App>, id: String) -> UiResult<()> {
    guard::require(&app, Permission::BillReprint)?;
    app.with_shop(|shop| shop.queue.retry(&id).map_err(|e| words::from_print(&e)))
}

#[tauri::command]
pub fn dismiss_print_job(app: tauri::State<'_, App>, id: String) -> UiResult<()> {
    guard::require(&app, Permission::BillReprint)?;
    app.with_shop(|shop| shop.queue.dismiss(&id).map_err(|e| words::from_print(&e)))
}

/// Every job that did not print, back in the queue in one press.
#[tauri::command]
pub fn retry_parked_print_jobs(app: tauri::State<'_, App>) -> UiResult<u32> {
    guard::require(&app, Permission::BillReprint)?;
    app.with_shop(|shop| {
        shop.queue
            .retry_parked()
            .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
            .map_err(|e| words::from_print(&e))
    })
}

/// Every job that has not printed, given up on in one press — whatever state it is in.
#[tauri::command]
pub fn dismiss_all_print_jobs(app: tauri::State<'_, App>) -> UiResult<u32> {
    guard::require(&app, Permission::BillReprint)?;
    app.with_shop(|shop| {
        shop.queue
            .dismiss_all()
            .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
            .map_err(|e| words::from_print(&e))
    })
}

/// The queue's vocabulary, turned into words.
pub fn to_view(status: &mb_print::queue::JobStatus) -> PrintJobView {
    use mb_print::queue::{JobKind as K, JobState as S};
    PrintJobView {
        id: status.id.clone(),
        printer: status.printer_name.clone(),
        what: match status.kind {
            K::Bill => "Bill",
            K::Kitchen => "Kitchen ticket",
            K::Label => "Label",
            K::Test => "Test print",
            K::Drawer => "Cash drawer",
            K::DayClose => "Closing slip",
            K::Delivery => "Delivery slip",
            // Named plainly, because this is the job a shopkeeper must notice if it fails: the
            // code on it is on screen for one dialog and nowhere else afterwards.
            K::Recovery => "Recovery code",
        }
        .to_owned(),
        state: match status.state {
            S::Pending => "Waiting".to_owned(),
            S::Printing => "Printing".to_owned(),
            S::Failed => format!("Did not print — trying again ({})", status.attempts),
            // The one a person has to act on, and it says so in words.
            S::Parked => "NOT PRINTED — needs you".to_owned(),
        },
        needs_attention: matches!(status.state, S::Parked),
        reason: status.reason.clone(),
        last_error: status.last_error.clone(),
    }
}

fn find_printer(app: &tauri::State<'_, App>, id: &str) -> UiResult<PrinterConfig> {
    // With a shop, use the configured printer.
    let configured = app.with_shop(|shop| {
        let rows = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .map_err(|e| words::from_db(&e))?;
        rows.iter()
            .find(|p| p.id == id)
            .map(crate::state::printer_config_for)
            .ok_or_else(|| {
                UiError::new(
                    "printer.unknown",
                    "That printer is not set up any more. Choose another one.",
                )
            })
    });

    match configured {
        Ok(printer) => Ok(printer),
        Err(e) if e.code == "shop.none" => {
            let path = crate::logging::directory()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("test-print.bin");
            log_warn!(
                "no shop is open, so the test print goes to {} instead of a printer",
                path.display()
            );
            Ok(PrinterConfig::new(
                "prn_none",
                "No printer set up yet",
                Target::File { path },
            ))
        }
        Err(e) => Err(e),
    }
}

/// Every command, in one place, so `main.rs` reads as a list rather than a macro nobody can
/// grep.
#[macro_export]
macro_rules! commands {
    () => {
        tauri::generate_handler![
            $crate::ipc::app_status,
            $crate::ipc::reveal_logs,
            $crate::ipc::print_test_page,
            $crate::ipc::list_print_jobs,
            $crate::ipc::retry_print_job,
            $crate::ipc::dismiss_print_job,
            $crate::ipc::retry_parked_print_jobs,
            $crate::ipc::dismiss_all_print_jobs,
            $crate::ipc::preview_test_page,
            $crate::ipc::preview_order,
            $crate::ipc::current_cart,
            $crate::ipc::cart_add,
            $crate::ipc::cart_set_qty,
            $crate::ipc::cart_step_qty,
            $crate::ipc::cart_remove,
            $crate::ipc::cart_clear,
            $crate::ipc::cart_set_order_type,
            $crate::ipc::cart_clear_payments,
            $crate::ipc::cart_cash_given,
            $crate::ipc::open_orders,
            $crate::ipc::menu_items,
            $crate::ipc::search_items,
            $crate::ipc::open_table,
            $crate::ipc::open_order,
            $crate::ipc::join_table,
            $crate::ipc::cart_set_discount,
            $crate::ipc::cart_clear_discount,
            $crate::flows::print_kitchen_ticket,
            // The cook lost the paper.
            $crate::flows::reprint_kitchen_ticket,
            $crate::flows::bill_pdf,
            $crate::flows::complete_bill,
            $crate::flows::print_open_bill,
            // The settle desk: what the phones asked, and the cashier's answer.
            $crate::orders::settle_requests,
            $crate::orders::settle_from_floor,
            $crate::orders::decline_settle,
            // Signing in, the people, the history.
            $crate::ipc::lock_state,
            $crate::ipc::login,
            $crate::ipc::lock_now,
            $crate::ipc::recover_with_code,
            $crate::ipc::list_staff,
            $crate::ipc::save_staff_member,
            $crate::ipc::set_staff_pin,
            $crate::ipc::list_roles,
            $crate::ipc::save_role,
            $crate::ipc::list_permissions,
            $crate::ipc::audit_trail,
            // The four ways a shop takes something back.
            $crate::corrections::list_bills,
            $crate::corrections::day_totals,
            $crate::corrections::reasons,
            $crate::corrections::void_bill,
            $crate::corrections::cancel_order,
            $crate::corrections::void_line,
            $crate::corrections::reprint_bill,
            $crate::corrections::refund_bill,
            // The menu.
            $crate::menu::menu_categories,
            $crate::menu::menu_rows,
            $crate::menu::save_menu_item,
            $crate::menu::set_item_available,
            $crate::menu::save_menu_category,
            $crate::menu::change_menu_prices,
            $crate::menu::plan_menu_import,
            $crate::menu::run_menu_import,
            $crate::menu::export_menu,
            // Settings › Tax — the one screen for tax.
            $crate::tax::tax_slabs,
            $crate::tax::tax_page,
            $crate::tax::save_tax_slab,
            $crate::tax::remove_tax_slab,
            $crate::tax::set_items_tax,
            $crate::tax::set_category_tax,
            $crate::menu::item_composition,
            $crate::menu::save_item_variant,
            $crate::menu::list_modifier_groups,
            $crate::menu::save_modifier_group,
            $crate::menu::attach_modifier_group,
            $crate::menu::list_combos,
            $crate::menu::save_combo,
            $crate::floor::floor_plan,
            $crate::floor::save_floor_section,
            $crate::floor::delete_floor_section,
            $crate::floor::save_dining_table,
            $crate::floor::add_dining_tables,
            $crate::floor::place_dining_table,
            $crate::floor::set_dining_tables_active,
            $crate::floor::delete_dining_table,
            $crate::floor::delete_dining_tables,
            $crate::floor::save_floor_thresholds,
            $crate::floor::move_order,
            $crate::floor::merge_orders,
            $crate::floor::split_order,
            $crate::credit::customers,
            $crate::credit::customer_account,
            $crate::credit::save_customer,
            $crate::credit::record_repayment,
            $crate::credit::save_credit_adjustment,
            $crate::credit::credit_headroom,
            $crate::credit::put_on_account,
            $crate::expenses::expenses,
            $crate::expenses::save_expense,
            $crate::expenses::delete_expense,
            $crate::expenses::save_cash_movement,
            $crate::expenses::save_expense_category,
            $crate::expenses::save_recurring_expense,
            $crate::expenses::confirm_recurring_expense,
            $crate::expenses::export_expenses,
            // The employment side: shifts, attendance, leave, salary and payroll.
            $crate::employment::employees,
            $crate::employment::save_employee,
            $crate::employment::attendance,
            $crate::employment::clock_in,
            $crate::employment::clock_out,
            $crate::employment::correct_attendance,
            $crate::employment::save_roster,
            $crate::employment::leave,
            $crate::employment::request_leave,
            $crate::employment::decide_leave,
            $crate::employment::adjust_leave,
            $crate::employment::salary,
            $crate::employment::save_salary,
            $crate::employment::give_advance,
            $crate::employment::payroll_runs,
            $crate::employment::payroll,
            $crate::employment::compute_payroll,
            $crate::employment::edit_payroll_line,
            $crate::employment::approve_payroll,
            $crate::employment::reverse_payroll,
            $crate::employment::staff_cost,
            $crate::employment::print_payslip,
            // Delivery, the riders, and the cash they are carrying.
            $crate::delivery::delivery_board,
            $crate::delivery::save_delivery,
            $crate::delivery::record_handback,
            $crate::delivery::set_rider,
            $crate::delivery::print_delivery_slip,
            // What the payment machines said, and what nobody has confirmed yet.
            $crate::payments::payments,
            $crate::payments::confirm_payment,
            // The device screen, the scale, the customer display and parcel labels.
            $crate::devices::device_manager,
            $crate::devices::scanned,
            $crate::devices::read_scale_once,
            $crate::devices::show_customer_display,
            $crate::devices::print_label,
            // The first run, and making a shop at all.
            $crate::firstrun::first_run,
            $crate::firstrun::create_shop,
            $crate::firstrun::use_existing_shop,
            // The logo, and the two Browse buttons that did not exist.
            $crate::logo::logo,
            $crate::logo::pick_a_logo,
            $crate::logo::save_logo,
            $crate::logo::remove_logo,
            $crate::logo::pick_a_folder,
            // The stock book.
            $crate::inventory::inventory,
            $crate::inventory::recipe,
            $crate::inventory::save_material,
            $crate::inventory::save_recipe,
            $crate::inventory::delete_recipe,
            $crate::inventory::record_stock_movement,
            $crate::inventory::rebuild_stock_balances,
            $crate::inventory::resolve_stock_problem,
            $crate::inventory::stock_variance,
            $crate::inventory::buy_list_text,
            // Buying, and the count.
            $crate::buying::buying,
            $crate::buying::supplier_account,
            $crate::buying::purchase,
            $crate::buying::save_supplier,
            $crate::buying::save_purchase,
            $crate::buying::cancel_purchase,
            $crate::buying::record_supplier_payment,
            $crate::buying::save_supplier_adjustment,
            $crate::buying::save_purchase_order,
            $crate::buying::set_order_state,
            $crate::buying::attach_photo,
            $crate::buying::purchase_photo,
            $crate::counting::stock_count,
            $crate::counting::open_stock_count,
            $crate::counting::record_count_line,
            $crate::counting::explain_count_line,
            $crate::counting::remove_count_line,
            $crate::counting::approve_stock_count,
            $crate::counting::abandon_stock_count,
            $crate::counting::count_sheet,
            $crate::terminals::tills,
            $crate::terminals::save_till,
            $crate::terminals::make_master,
            $crate::terminals::join_master,
            $crate::terminals::send_waiting_bills,
            $crate::share::share_report,
            // The settings.
            $crate::settings::ipc::settings_all,
            $crate::settings::ipc::reload_settings,
            $crate::settings::ipc::search_settings,
            $crate::settings::ipc::save_settings,
            $crate::settings::ipc::settings_defaults_for,
            $crate::settings::ipc::preview_settings,
            $crate::settings::printers::printer_setup,
            $crate::settings::printers::save_printer,
            $crate::settings::printers::delete_printer,
            $crate::settings::printers::route_category,
            $crate::settings::printers::set_default_printer,
            $crate::settings::printers::set_paper_size,
            $crate::settings::printers::print_sample_bill,
            $crate::settings::printers::nudge_printer,
            $crate::settings::backup::backup_status,
            $crate::settings::backup::back_up_now,
            $crate::settings::backup::verify_backup,
            $crate::settings::backup::request_restore,
            $crate::settings::backup::cancel_restore,
            $crate::settings::backup::find_shops,
            $crate::settings::ipc::export_settings,
            $crate::settings::ipc::plan_settings_import,
            $crate::settings::ipc::run_settings_import,
            $crate::settings::numbering::numbering,
            $crate::settings::numbering::save_counter,
            // Thirteen reports behind three commands, for the same reason the settings screen
            // is one component: the list is the screen.
            $crate::reports::report_list,
            $crate::reports::report,
            $crate::reports::report_csv,
            $crate::reports::report_pdf,
            $crate::reports::dashboard,
            // The business day: the gate, the Days screen, and the drawer count beside them.
            $crate::dayclose::day_state,
            $crate::dayclose::close_pending,
            $crate::dayclose::days,
            $crate::dayclose::close_day,
            $crate::dayclose::mark_holiday,
            $crate::dayclose::unmark_holiday,
            $crate::dayclose::reopen_day,
            $crate::dayclose::count_cash,
            $crate::dayclose::count_drawer,
            // The phones this counter serves.
            $crate::lan::network,
            $crate::lan::phones_now,
            $crate::lan::open_pairing,
            $crate::lan::allow_firewall,
            $crate::lan::close_pairing,
            $crate::lan::allow_device,
            $crate::lan::refuse_device,
            $crate::lan::revoke_device,
            // The floor's items, into the cashier's bill or not.
            $crate::orders::take_the_floors_items,
            $crate::orders::dismiss_the_floors_items,
            // The licence.
            $crate::licensing::account,
            $crate::licensing::activate,
            $crate::licensing::deactivate,
            $crate::licensing::transfer_here,
            $crate::licensing::use_emergency_code,
            $crate::licensing::refresh_licence,
            // Is this counter healthy, and what can we send to support.
            $crate::health::health,
            $crate::diagnostics::diagnostics_plan,
            $crate::diagnostics::write_diagnostics,
            // The update, and the way back.
            $crate::updates::look_for_an_update,
            $crate::updates::go_back_a_version,
            $crate::updates::install_update,
            // The cloud copy, and what comes back down it.
            $crate::firstrun::restore_from_cloud,
            $crate::ipc::notices,
            $crate::ipc::notices_seen,
            $crate::ipc::pull_from_cloud,
            // Deliberately Public: on a first run the stand-in is who is standing there, and a
            // set-up list that refuses to draw until somebody has a PIN is a list nobody can
            // use to create the PIN.
            $crate::setup::setup_list,
            // The kitchen screen.
            $crate::kitchen::kitchen,
            $crate::kitchen::kitchen_shown,
            $crate::kitchen::kitchen_bump,
            $crate::kitchen::kitchen_bump_line,
            $crate::kitchen::kitchen_recall,
            $crate::kitchen::kitchen_acknowledge,
            $crate::kitchen::kitchen_fire,
            // Development only — see its own documentation.
            #[cfg(debug_assertions)]
            $crate::ipc::seed_demo_shop,
        ]
    };
}

// The preview — the fourth sink's input.

// There was a second `metrics_for` here, and it was the bug it exists to prevent.

/// Lay out a sample bill and hand it to the screen.
#[tauri::command]
pub fn preview_test_page(
    app: tauri::State<'_, App>,
    printer_id: Option<String>,
) -> UiResult<crate::preview::PreviewDoc> {
    let printer = match printer_id {
        Some(id) => find_printer(&app, &id)?,
        None => PrinterConfig::new("prn_preview", "Preview", Target::None),
    };
    let (metrics, engine) = app.metrics_for(mb_print::queue::JobKind::Test, &printer);
    let document = mb_print::testprint::test_document(&printer, None);
    let laid =
        mb_print::layout::layout_for(&document, &metrics).map_err(|e| words::from_print(&e))?;
    Ok(crate::preview::to_preview(&laid, &metrics, engine))
}

/// The REAL bill for the REAL order, before it prints.
#[tauri::command]
pub fn preview_order(
    app: tauri::State<'_, App>,
    order_id: Option<String>,
) -> UiResult<crate::preview::PreviewDoc> {
    crate::flows::preview_order_on(&app, order_id)
}

// The billing screen.

use crate::billing::{
    CartState, CartView, MenuItemView, TableView, cart_view, floor_view, menu_view,
    order_type_from_label, snapshot_for,
};
use mb_core::{OrderId, TableId};

/// What is in the cart right now.
#[tauri::command]
pub fn current_cart(app: tauri::State<'_, App>) -> UiResult<CartView> {
    guard::require(&app, Permission::BillCreate)?;
    app.with_cart(|state| cart_view(state, &app.shop_config()))
}

/// Put an item in.
pub fn cart_add_on(
    app: &App,
    item_id: String,
    qty: Option<String>,
    note: Option<String>,
) -> UiResult<CartView> {
    guard::require(app, Permission::BillCreate)?;
    let item = app.find_menu_item(&item_id)?;
    let qty = match qty {
        Some(text) => mb_core::Qty::parse(&text).map_err(|e| {
            UiError::new(
                "cart.qty",
                format!("\"{text}\" is not a quantity. Try 1, 2 or 0.5."),
            )
            .with_detail(e.to_string())
        })?,
        None => mb_core::Qty::from_whole(1).map_err(|e| {
            UiError::new("cart.qty", "A quantity of one could not be made.")
                .with_detail(e.to_string())
        })?,
    };

    // The tax is resolved here, once, from the book — and frozen with the line.
    let snapshot = snapshot_for(&item, &app.shop_config().tax)?;
    app.with_cart_mut(|state| {
        state
            .cart
            .add(snapshot, qty, note, vec![])
            .map_err(|e| {
                UiError::new("cart.add", "That item could not be added to the bill.")
                    .with_detail(e.to_string())
            })?;
        cart_view(state, &app.shop_config())
    })
}

#[tauri::command]
pub fn cart_set_qty(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    index: usize,
    qty: String,
) -> UiResult<CartView> {
    guard::require(&app, Permission::BillCreate)?;
    let parsed = mb_core::Qty::parse(&qty).map_err(|e| {
        UiError::new(
            "cart.qty",
            format!("\"{qty}\" is not a quantity. Try 1, 2 or 0.5."),
        )
        .with_detail(e.to_string())
    })?;
    let view = app.with_cart_mut(|state| {
        state.cart.set_qty(index, parsed).map_err(|e| {
            UiError::new("cart.qty", "That quantity could not be set.").with_detail(e.to_string())
        })?;
        cart_view(state, &app.shop_config())
    });
    shown(&handle, view)
}

/// One more, or one less — the − and + on a cart line.
#[tauri::command]
pub fn cart_step_qty(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    index: usize,
    by: i32,
) -> UiResult<CartView> {
    guard::require(&app, Permission::BillCreate)?;

    let step = mb_core::Qty::from_whole(i64::from(by)).map_err(|e| {
        UiError::new("cart.qty", "That step is too big.").with_detail(e.to_string())
    })?;

    let view = app.with_cart_mut(|state| {
        let now = state
            .cart
            .lines()
            .get(index)
            .ok_or_else(|| {
                UiError::new("cart.qty.gone", "That line is not on this bill any more.")
            })?
            .qty;
        let next = now.add(step).map_err(|e| {
            UiError::new("cart.qty", "That quantity could not be worked out.")
                .with_detail(e.to_string())
        })?;

        if next.is_positive() {
            state.cart.set_qty(index, next).map_err(|e| {
                UiError::new("cart.qty", "That quantity could not be set.")
                    .with_detail(e.to_string())
            })?;
        } else {
            state.cart.remove(index).map_err(|e| {
                UiError::new("cart.remove", "That line could not be removed.")
                    .with_detail(e.to_string())
            })?;
        }
        cart_view(state, &app.shop_config())
    });
    shown(&handle, view)
}

#[tauri::command]
pub fn cart_remove(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    index: usize,
) -> UiResult<CartView> {
    guard::require(&app, Permission::BillCreate)?;
    let view = app.with_cart_mut(|state| {
        state.cart.remove(index).map_err(|e| {
            UiError::new("cart.remove", "That line could not be removed.")
                .with_detail(e.to_string())
        })?;
        cart_view(state, &app.shop_config())
    });
    shown(&handle, view)
}

/// New order. Keeps the order type, because the type lock is what stops a parcel counter
/// re-selecting it forty times an hour.
pub fn cart_clear_on(app: &App, keep_type: bool) -> UiResult<CartView> {
    guard::require(app, Permission::BillCreate)?;
    let config = app.shop_config();
    app.with_cart_mut(|state| {
        let previous = if keep_type {
            state.order_type()
        } else {
            mb_core::OrderType::DineIn
        };
        *state = CartState::new_order(crate::billing::starting_order_type(&config, previous));
        cart_view(state, &config)
    })
}

#[tauri::command]
pub fn cart_set_order_type(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    order_type: String,
) -> UiResult<CartView> {
    guard::require(&app, Permission::BillCreate)?;
    let kind = order_type_from_label(&order_type).ok_or_else(|| {
        UiError::new(
            "cart.order_type",
            format!("\"{order_type}\" is not an order type."),
        )
    })?;
    let config = app.shop_config();
    if config.billing.lock_order_type && kind != config.billing.locked_order_type {
        return Err(UiError::new(
            "cart.order_type_locked",
            format!(
                "This counter always bills as {}. Change that under Settings › Billing.",
                crate::billing::order_type_label(config.billing.locked_order_type)
            ),
        )
        .quietly());
    }
    let view = app.with_cart_mut(|state| {
        state.set_order_type(kind);
        cart_view(state, &config)
    });
    shown(&handle, view)
}

/// Take a payment. A split is this more than once: the cash box, then the lit mode.
pub fn cart_add_payment_on(
    app: &App,
    mode: String,
    amount_paise: i64,
    reference: Option<String>,
) -> UiResult<CartView> {
    // One counter action at a time — see `App::begin_action`.
    let _one_at_a_time = app.begin_action();
    take_payment(app, mode, amount_paise, reference)
}

/// The payment itself, for a caller that already holds the counter — never take the action lock
/// here: a lock taken twice on one thread is a counter that freezes.
pub fn take_payment(
    app: &App,
    mode: String,
    amount_paise: i64,
    reference: Option<String>,
) -> UiResult<CartView> {
    guard::require(app, Permission::BillCreate)?;
    let mode = match mode.as_str() {
        "Cash" => mb_core::PaymentMode::Cash,
        "Card" => mb_core::PaymentMode::Card,
        "UPI" => mb_core::PaymentMode::Upi,
        other => mb_core::PaymentMode::Other(other.to_owned()),
    };
    let amount = mb_core::Money::from_paise(amount_paise);

    // The shape is checked before anything is asked.
    if !amount.is_positive() {
        return Err(
            UiError::new("payment.invalid", "That payment could not be taken.")
                .with_detail(mb_core::PaymentError::NonPositiveAmount.to_string()),
        );
    }

    let reference = reference
        .map(|r| r.trim().to_owned())
        .filter(|r| !r.is_empty());
    let order_id = app.with_cart(|state| Ok(state.order_id().map(str::to_owned)))?;

    let answer = crate::payments::ask_about(
        app,
        order_id.as_deref(),
        &mode,
        amount,
        reference.as_deref(),
    )?;

    if let mb_core::provider::Answer::Declined { because } = &answer {
        return Err(UiError::new(
            "payment.declined",
            format!("That payment was refused — {because}. Ask for another way to pay."),
        ));
    }

    let mut payment = mb_core::Payment::new(mode, amount).map_err(|e| {
        UiError::new("payment.invalid", "That payment could not be taken.")
            .with_detail(e.to_string())
    })?;
    // The provider's reference wins over the typed one when there is one: an approval code from
    // a machine is worth more than a number somebody read off a screen.
    let confirmed = answer.is_approved();
    if let mb_core::provider::Answer::Approved { reference: given } = &answer
        && !given.is_empty()
    {
        payment = payment.with_reference(given.clone());
    }
    if payment.reference.is_none()
        && let Some(reference) = reference
    {
        payment = payment.with_reference(reference);
    }
    let provider = app.provider();
    payment = payment.answered_by(provider.name(), confirmed);

    app.with_cart_mut(|state| {
        state.settlement.add(payment).map_err(|e| {
            UiError::new("payment.invalid", "That payment could not be taken.")
                .with_detail(e.to_string())
        })?;
        cart_view(state, &app.shop_config())
    })
}

#[tauri::command]
pub fn cart_clear_payments(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
) -> UiResult<CartView> {
    guard::require(&app, Permission::BillCreate)?;
    let view = app.with_cart_mut(|state| {
        state.settlement = mb_core::Settlement::new();
        cart_view(state, &app.shop_config())
    });
    shown(&handle, view)
}

/// The cash the customer handed over, as typed.
#[tauri::command]
pub fn cart_cash_given(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    amount: String,
) -> UiResult<CartView> {
    guard::require(&app, Permission::BillCreate)?;
    let typed = amount.trim().to_owned();
    let cleared = app.with_cart_mut(|state| {
        state.settlement = mb_core::Settlement::new();
        cart_view(state, &app.shop_config())
    })?;
    if typed.is_empty() {
        return shown(&handle, Ok(cleared));
    }
    let given = mb_core::Money::parse(&typed).map_err(|e| {
        UiError::new(
            "payment.cash",
            "Type how much cash was given, like 500 or 500.00.",
        )
        .with_detail(e.to_string())
    })?;
    if !given.is_positive() {
        return shown(&handle, Ok(cleared));
    }
    shown(
        &handle,
        cart_add_payment_on(&app, "Cash".to_owned(), given.paise(), None),
    )
}

// Money off a bill.

/// Take money off this bill.
pub fn cart_set_discount_on(
    app: &App,
    kind: String,
    value: String,
    reason: Option<String>,
) -> UiResult<CartView> {
    let who = guard::require(app, Permission::BillDiscountBill)?;
    let config = app.shop_config();

    let discount = match kind.as_str() {
        "percent" => {
            // Basis points, from text, without a float: "12.5" is 1250.
            let bp = percent_to_bp(&value)?;
            mb_core::Discount::percent_bp(bp).ok_or_else(|| {
                UiError::new("discount.too_much", "A discount cannot be more than 100%.")
            })?
        }
        "amount" => {
            let money = mb_core::Money::parse(value.trim()).map_err(|e| {
                UiError::new(
                    "discount.amount",
                    "Type how much to take off, like 50 or 50.00.",
                )
                .with_detail(e.to_string())
            })?;
            mb_core::Discount::amount(money).ok_or_else(|| {
                UiError::new(
                    "discount.negative",
                    "A discount takes money off. To add something on, use a charge.",
                )
            })?
        }
        _ => {
            return Err(UiError::new(
                "discount.kind",
                "A discount is a percentage or an amount.",
            ));
        }
    };

    let reason = reason
        .map(|r| r.trim().to_owned())
        .filter(|r| !r.is_empty());
    let mut entry = mb_core::DiscountEntry::new(discount);
    if let Some(reason) = reason {
        entry = entry.with_reason(reason);
    }
    entry = entry.authorised_by(who.staff_id.clone());

    app.with_cart_mut(|state| {
        // The base a percentage is taken off.
        let base = state.bill(&config)?.subtotal;
        who.discount_policy()
            .check(&entry, base)
            .map_err(|e| UiError::new("discount.refused", e.to_string()))?;
        state.bill_discount = Some(entry);
        cart_view(state, &config)
    })
}

/// Clear the discount. Separate from setting one so that "no discount" is never expressed as "a
/// discount of zero", which would print a zero line on the bill and read as a mistake.
pub fn cart_clear_discount_on(app: &App) -> UiResult<CartView> {
    guard::require(app, Permission::BillDiscountBill)?;
    app.with_cart_mut(|state| {
        state.bill_discount = None;
        cart_view(state, &app.shop_config())
    })
}

/// `"12.5"` to `1250` basis points, without a float.
fn percent_to_bp(typed: &str) -> UiResult<u32> {
    let typed = typed.trim();
    let wrong = || UiError::new("discount.percent", "Type the percentage, like 10 or 12.5.");
    if typed.is_empty() {
        return Err(wrong());
    }
    let (whole, fraction) = match typed.split_once('.') {
        Some((w, f)) => (w, f),
        None => (typed, ""),
    };
    if fraction.len() > 2 || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return Err(wrong());
    }
    let whole: u32 = whole.parse().map_err(|_| wrong())?;
    // "12.5" is 50 hundredths, not 5 — pad on the right, not the left.
    let hundredths: u32 = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u32>().map_err(|_| wrong())? * 10,
        _ => fraction.parse().map_err(|_| wrong())?,
    };
    whole
        .checked_mul(100)
        .and_then(|w| w.checked_add(hundredths))
        .ok_or_else(wrong)
}

#[tauri::command]
pub fn cart_set_discount(
    app: tauri::State<'_, App>,
    kind: String,
    value: String,
    reason: Option<String>,
) -> UiResult<CartView> {
    cart_set_discount_on(&app, kind, value, reason)
}

#[tauri::command]
pub fn cart_clear_discount(app: tauri::State<'_, App>) -> UiResult<CartView> {
    cart_clear_discount_on(&app)
}

/// The floor — the only view of open orders.
pub fn open_orders_on(app: &App) -> UiResult<Vec<TableView>> {
    guard::require(app, Permission::BillCreate)?;
    // All three halves of "where is the cashier" — see `TableView::selected`.
    let (loaded, on_table, on_seat) = app.with_cart(|state| {
        Ok((
            state.order_id().map(str::to_owned),
            state.table_id().map(str::to_owned),
            state.seat().map(str::to_owned),
        ))
    })?;
    // The same two thresholds the floor screen uses, from the same place — a billing grid and a
    // floor plan disagreeing about which table is late would be worse than neither of them
    // saying so.
    let (warn, late) = crate::floor::thresholds(app)?;
    let at = crate::flows::now();
    let config = app.shop_config();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let tables = repos.floor().list_tables(OUTLET)?;
                let sections = repos.floor().list_sections(OUTLET)?;
                let open = repos.orders().list_open(OUTLET)?;
                let mut tiles = floor_view(
                    &tables,
                    &sections,
                    &open,
                    crate::billing::Room {
                        // The billing grid IS the cart's view of the room, so it is the one
                        // screen that marks a tile.
                        cart_is_on: Some(crate::billing::CartIsOn {
                            order: loaded.as_deref(),
                            table: on_table.as_deref(),
                            seat: on_seat.as_deref(),
                        }),
                        now: at,
                        warn_after: warn,
                        late_after: late,
                        config: &config,
                    },
                );
                // The same facts the floor screen adds, so the two grids never disagree.
                crate::billing::decorate(&mut tiles, &repos, at)?;
                Ok(tiles)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// The menu, for putting something in the cart.
#[tauri::command]
pub fn menu_items(app: tauri::State<'_, App>) -> UiResult<Vec<MenuItemView>> {
    guard::require(&app, Permission::BillCreate)?;
    app.with_shop(|shop| {
        let items = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).menu().list_items(OUTLET, true))
            .map_err(|e| words::from_db(&e))?;
        let book = app.shop_config().tax;
        Ok(items.iter().map(|item| menu_view(item, &book)).collect())
    })
}

/// Put a small shop in the database so the billing screen has something to render — development
/// only, and it cannot ship.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn seed_demo_shop(app: tauri::State<'_, App>) -> UiResult<String> {
    guard::require(&app, Permission::StaffManage)?;
    use mb_core::{CategoryId, ItemId, Money, TableId, TaxRate, TaxSpec};
    use mb_db::repo::floor::{DiningTable, Section};
    use mb_db::repo::menu::MenuItem;

    let at = crate::flows::now();

    if !app.has_shop() {
        let dir = crate::config::AppConfig::directory();
        let path = dir.join("demo-shop.db");
        let db = mb_db::Db::open(&mb_db::DbConfig::new(&path)).map_err(|e| words::from_db(&e))?;
        mb_db::locate::write_config(&dir, &path).map_err(|e| words::from_db(&e))?;
        crate::log_info!("dev: made a demo shop at {}", path.display());
        app.open_shop(db, path);
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);

                for (index, name) in ["Main Hall", "AC Section", "Terrace"].iter().enumerate() {
                    repos.floor().save_section(
                        OUTLET,
                        &Section {
                            id: format!("sec_{index}"),
                            name: (*name).to_owned(),
                            sort_order: index as i64,
                            is_active: true,
                        },
                        at,
                    )?;
                }

                // Twenty-two tables across three sections: enough that density is a real
                // question at 1366x768 rather than a theoretical one.
                let mut n = 0;
                for (section, count) in [("sec_0", 10), ("sec_1", 8), ("sec_2", 4)] {
                    for seat in 1..=count {
                        n += 1;
                        repos.floor().save_table(
                            OUTLET,
                            &DiningTable {
                                id: TableId::new(format!("tbl_{n}")),
                                section_id: Some(section.to_owned()),
                                label: format!("{n}"),
                                seats: if seat % 3 == 0 { 6 } else { 4 },
                                pos: None,
                                sort_order: n,
                                is_active: true,
                            },
                            at,
                        )?;
                    }
                }

                // The category has to exist before an item can point at it —
                // `items.category_id` references `categories(id)`, and the first run of this
                // seeder hit that constraint.
                repos.menu().save_category(
                    OUTLET,
                    &mb_db::repo::menu::Category {
                        id: CategoryId::new("cat_food"),
                        name: "Food".to_owned(),
                        sort_order: 0,
                        is_active: true,
                        // The demo shop has one kitchen screen, which is what a real small shop
                        // has.
                        station: None,
                        default_tax_class_id: None,
                    },
                    at,
                )?;

                // One `TaxSpec` per item, not a rate and a treatment side by side.
                let menu: [(&str, &str, i64, TaxSpec); 8] = [
                    (
                        "itm_dosa",
                        "Masala Dosa",
                        12_000,
                        mb_core::TaxSpec::gst(demo_rate(5)),
                    ),
                    (
                        "itm_idli",
                        "Idli Vada",
                        8_000,
                        mb_core::TaxSpec::gst(demo_rate(5)),
                    ),
                    (
                        "itm_pbm",
                        "Paneer Butter Masala (Half) - Extra Spicy",
                        31_500,
                        mb_core::TaxSpec::gst(demo_rate(5)),
                    ),
                    (
                        "itm_water",
                        "Water 1L",
                        2_000,
                        mb_core::TaxSpec::gst_inclusive(demo_rate(18)),
                    ),
                    (
                        "itm_cola",
                        "Cola 300ml",
                        4_000,
                        mb_core::TaxSpec::gst(demo_rate(18)),
                    ),
                    // The demo bar line.
                    (
                        "itm_beer",
                        "Beer 650ml",
                        22_000,
                        mb_core::TaxSpec::liquor(TaxRate::ZERO),
                    ),
                    (
                        "itm_rice",
                        "Curd Rice",
                        9_000,
                        mb_core::TaxSpec::gst(demo_rate(5)),
                    ),
                    (
                        "itm_sweet",
                        "Gulab Jamun (2 pc)",
                        6_000,
                        mb_core::TaxSpec::gst(demo_rate(5)),
                    ),
                ];
                for (index, (id, name, paise, tax)) in menu.iter().enumerate() {
                    repos.menu().save_item(
                        OUTLET,
                        &MenuItem {
                            id: ItemId::new(*id),
                            category_id: Some(CategoryId::new("cat_food")),
                            name: (*name).to_owned(),
                            unit_price: Money::from_paise(*paise),
                            // The demo shop points its items at the seeded slabs, so a rate
                            // change on the slab moves them — which is what a real shop does.
                            tax_class_id: mb_core::seeded_slab_for(tax.kind, tax.rate)
                                .unwrap_or_else(|| mb_core::TaxClassId::new("tax_food_5")),
                            // Only a tax-in item says so; the rest follow the slab and the shop.
                            price_basis: tax.basis.is_inclusive().then_some(tax.basis),
                            hsn: Some("2106".to_owned()),
                            cost_price: None,
                            short_code: None,
                            // The demo shop gets real prep times and courses, so the kitchen
                            // screen has something to show the first time somebody opens it.
                            prep_minutes: Some(if *paise > 8_000 { 12 } else { 4 }),
                            course: Some(
                                if *paise > 8_000 { "Main" } else { "Starter" }.to_owned(),
                            ),
                            is_open_price: false,
                            is_available: true,
                            sort_order: index as i64,
                        },
                        at,
                    )?;
                }
                Ok(())
            })
            .map_err(|e| words::from_db(&e))?;
        Ok("a demo shop is in place".to_owned())
    })
}

/// Ranked item search.
pub fn search_items_on(
    app: &App,
    text: String,
    mode: Option<crate::search::MatchMode>,
) -> UiResult<Vec<MenuItemView>> {
    guard::require(app, Permission::BillCreate)?;
    app.with_shop(|shop| {
        let items = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).menu().list_items(OUTLET, true))
            .map_err(|e| words::from_db(&e))?;
        // The shop's setting, not this function's default.
        let config = app.shop_config();
        Ok(crate::search::search(
            &items,
            &text,
            mode.unwrap_or(config.billing.search_mode),
            &config.tax,
        ))
    })
}

/// Press a table: its running order comes into the cart, or a new order starts on it.
/// The cart goes to a table. Lines typed at the counter and not yet in the books go with it —
/// onto a free table as its order, onto a busy table's bill as new items for the kitchen.
pub fn open_table_on(app: &App, table_id: String) -> UiResult<CartView> {
    guard::require(app, Permission::BillCreate)?;
    let table = TableId::new(table_id);
    let (label, found) = table_and_its_order(app, &table)?;
    let config = app.shop_config();

    app.with_cart_mut(|state| {
        let typed = state.order_id().is_none() && !state.cart.lines().is_empty();
        match found {
            Some(order) if typed => {
                // The kitchen ledger that comes with the order knows these lines are new.
                let mut joined = CartState::load(&order, Some(label));
                for line in state.cart.lines() {
                    joined.cart.push(line.clone()).map_err(|e| {
                        UiError::new(
                            "cart.join",
                            "These items could not go onto that table's bill.",
                        )
                        .with_detail(e.to_string())
                    })?;
                }
                *state = joined;
            }
            Some(order) => *state = CartState::load(&order, Some(label)),
            None if typed => state.place_on(table.clone(), label, None),
            // A free table: "press the table and start typing" is the flow.
            None => {
                let mut fresh = CartState::new_order(mb_core::OrderType::DineIn);
                fresh.place_on(table.clone(), label, None);
                *state = fresh;
            }
        }
        cart_view(state, &config)
    })
}

/// A table's label and the order on it, or a refusal if the table is gone.
fn table_and_its_order(
    app: &App,
    table: &TableId,
) -> UiResult<(String, Option<mb_core::AnyOrder>)> {
    let (label, found) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let label = repos.floor().find_table(table)?.map(|t| t.label);
                let found = match repos.floor().first_party_at(table)? {
                    Some(order_id) => repos.orders().find(&OrderId::new(order_id))?,
                    None => None,
                };
                Ok((label, found))
            })
            .map_err(|e| words::from_db(&e))
    })?;
    let Some(label) = label else {
        return Err(UiError::new(
            "table.unknown",
            "That table is not on the floor any more.",
        ));
    };
    Ok((label, found))
}

/// A second party on a table, with its own letter: the one given, or the next free one. What
/// was typed at the counter goes with it; a parked order in the cart stays where it is.
pub fn join_table_on(app: &App, table_id: String, seat: Option<String>) -> UiResult<CartView> {
    guard::require(app, Permission::BillCreate)?;
    let table = TableId::new(table_id);
    let (label, _) = table_and_its_order(app, &table)?;
    let config = app.shop_config();

    let taken: Vec<mb_core::SubTable> = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| mb_db::Repos::new(tx).orders().list_open(OUTLET))
                .map_err(|e| words::from_db(&e))
        })?
        .iter()
        .filter(|o| o.core().table() == Some(&table))
        .filter_map(|o| o.core().seat().cloned())
        .collect();

    let seat = match seat {
        Some(letter) => {
            let seat = mb_core::SubTable::parse(&letter).map_err(|e| {
                UiError::new("table.seat", "A seat is one letter, A to Z.")
                    .with_detail(e.to_string())
            })?;
            if taken.contains(&seat) {
                return Err(UiError::new(
                    "table.seat_taken",
                    format!(
                        "Table {label}{} already has a party. Choose another letter.",
                        seat.as_str()
                    ),
                )
                .quietly());
            }
            seat
        }
        // A is the table's own party; the next party takes the first free letter after it.
        None => ('B'..='Z')
            .filter_map(|letter| mb_core::SubTable::parse(&letter.to_string()).ok())
            .find(|candidate| !taken.contains(candidate))
            .ok_or_else(|| {
                UiError::new(
                    "table.full",
                    format!("Table {label} already has every letter from B to Z in use."),
                )
            })?,
    };

    app.with_cart_mut(|state| {
        // A parked order stays on its own table; the new party starts from nothing.
        if state.order_id().is_some() {
            *state = CartState::new_order(mb_core::OrderType::DineIn);
        }
        state.place_on(table.clone(), label.clone(), Some(seat));
        cart_view(state, &config)
    })
}

#[tauri::command]
pub fn join_table(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    table_id: String,
    seat: Option<String>,
) -> UiResult<CartView> {
    shown(&handle, join_table_on(&app, table_id, seat))
}

/// Open an order that has no table — a parcel or a self-service order on the floor.
pub fn open_order_on(app: &App, order_id: String) -> UiResult<CartView> {
    guard::require(app, Permission::BillCreate)?;
    let order = crate::flows::find_order(app, &OrderId::new(&order_id))?;
    let Some(order) =
        order.filter(|o| matches!(o, mb_core::AnyOrder::Open(_) | mb_core::AnyOrder::Draft(_)))
    else {
        return Err(UiError::new(
            "order.not_open",
            "That order is not on the floor any more.",
        ));
    };
    let label = order
        .core()
        .table()
        .and_then(|t| crate::flows::table_name(app, t));
    app.with_cart_mut(|state| {
        *state = CartState::load(&order, label);
        cart_view(state, &app.shop_config())
    })
}

// Signing in, the people, and the history.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission, Pin, PinHash, RoleShape};

use crate::guard;

/// What the lock screen needs, and the only command that answers while locked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct LockState {
    /// `None` when the screen is locked.
    pub signed_in_as: Option<String>,
    pub role: Option<String>,
    /// What this person may do, so the rail can hide what they cannot open.
    pub permissions: Vec<String>,
    /// True while nobody in this shop has a PIN.
    pub nobody_has_a_pin: bool,
    /// Everybody who could sign in.
    pub people: Vec<PersonView>,
    /// Whether this shop has a recovery code at all, so the lock screen only offers "forgotten
    /// your PIN?" when there is something to offer.
    pub can_recover: bool,
    /// Who the recovery code may set a PIN for, which is not a subset of `Self::people` — and
    /// the difference is a way to be locked out of your own shop for good.
    pub recoverable: Vec<PersonView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PersonView {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    pub status: String,
    pub has_pin: bool,
    /// Empty unless this person is locked out — then it is the sentence the screen shows,
    /// already counted down.
    pub locked_out: Option<String>,
    pub permissions: Vec<String>,
    pub max_discount_bp: Option<u32>,
    pub max_discount: Option<MoneyView>,
}

/// The key the shop's recovery code hash is stored under.
const RECOVERY_KEY: &str = "auth.recovery_hash";

pub fn lock_state_on(app: &App) -> UiResult<LockState> {
    let current = app.sessions().current();
    let (people, can_recover) = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let repos = mb_db::Repos::new(tx);
                    let staff = repos.people().list_staff(OUTLET)?;
                    let mut people = Vec::new();
                    for member in staff {
                        let locked_out = lockout_message(&repos, &member)?;
                        people.push(person_view(&member, locked_out));
                    }
                    let recovery: Option<String> = repos.settings().get(OUTLET, RECOVERY_KEY)?;
                    Ok((people, recovery.is_some()))
                })
                .map_err(|e| words::from_db(&e))
        })
        .unwrap_or_else(|_| (Vec::new(), false));

    Ok(LockState {
        signed_in_as: current.as_ref().map(|s| s.actor.name.clone()),
        role: current.as_ref().and_then(|s| s.actor.role_name.clone()),
        permissions: current.as_ref().map_or_else(Vec::new, |s| {
            s.actor
                .permissions
                .iter()
                .map(|p| p.code().to_owned())
                .collect()
        }),
        nobody_has_a_pin: people.iter().all(|p| !p.has_pin),
        // Two lists off one read, and they are deliberately different.
        recoverable: people
            .iter()
            .filter(|p| {
                p.status == "active"
                    && p.permissions
                        .iter()
                        .any(|code| code == Permission::StaffManage.code())
            })
            .cloned()
            .collect(),
        // Only people who can actually sign in.
        people: people
            .into_iter()
            .filter(|p| p.has_pin && p.status == "active")
            .collect(),
        can_recover,
    })
}

fn person_view(
    member: &mb_db::repo::people::StaffMember,
    locked_out: Option<String>,
) -> PersonView {
    PersonView {
        id: member.id.as_str().to_owned(),
        name: member.name.clone(),
        role: member.role_name.clone(),
        status: status_word(member.status).to_owned(),
        has_pin: member.pin_hash.is_some(),
        locked_out,
        permissions: member
            .permissions
            .iter()
            .map(|p| p.code().to_owned())
            .collect(),
        max_discount_bp: member.max_discount_bp,
        max_discount: member.max_discount.map(MoneyView::from),
    }
}

const fn status_word(status: mb_db::repo::people::StaffStatus) -> &'static str {
    match status {
        mb_db::repo::people::StaffStatus::Active => "active",
        mb_db::repo::people::StaffStatus::Suspended => "suspended",
        mb_db::repo::people::StaffStatus::Left => "left",
    }
}

/// How long this person still has to wait, in words — or `None`.
fn lockout_message(
    repos: &mb_db::Repos<'_>,
    member: &mb_db::repo::people::StaffMember,
) -> Result<Option<String>, mb_db::DbError> {
    let failures = repos
        .audit()
        .failed_logins_since_success(OUTLET, member.id.as_str())?;
    let Some(wait) = mb_auth::lockout_after(failures) else {
        return Ok(None);
    };
    let Some(last) = repos
        .audit()
        .last_failed_login(OUTLET, member.id.as_str())?
    else {
        return Ok(None);
    };
    let elapsed = crate::flows::now().millis().saturating_sub(last.millis());
    let elapsed = std::time::Duration::from_millis(elapsed.max(0).unsigned_abs());
    match wait.checked_sub(elapsed) {
        Some(remaining) if !remaining.is_zero() => {
            Ok(Some(mb_auth::lockout::wait_message(remaining)))
        }
        _ => Ok(None),
    }
}

/// Sign in — one person, one verification.
pub fn login_on(app: &App, staff_id: String, pin: String) -> UiResult<LockState> {
    let at = crate::flows::now();
    let day = crate::flows::today(at);

    let typed = Pin::parse(&pin).map_err(|e| {
        UiError::new("auth.pin_shape", format!("{e}. Try again.")).with_detail(e.to_string())
    })?;

    let member = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).people().find_staff(OUTLET, &staff_id))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(member) = member else {
        return Err(UiError::new(
            "auth.unknown",
            "That person is not on this shop's staff list any more.",
        ));
    };

    // Somebody who has left keeps their history and loses their way in.
    if member.status != mb_db::repo::people::StaffStatus::Active {
        return Err(UiError::new(
            "auth.not_active",
            format!("{} is not signed on as staff here any more.", member.name),
        ));
    }

    // The lockout, computed from the history itself.
    let waiting = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| lockout_message(&mb_db::Repos::new(tx), &member))
            .map_err(|e| words::from_db(&e))
    })?;
    if let Some(message) = waiting {
        return Err(UiError::new("auth.locked_out", message));
    }

    let stored = member.pin().map_err(|e| words::from_db(&e))?;
    let Some(stored) = stored else {
        return Err(UiError::new(
            "auth.no_pin",
            format!(
                "{} has no PIN yet. Somebody who manages staff can set one.",
                member.name
            ),
        ));
    };

    if !mb_auth::verify_pin(&typed, &stored) {
        // The failure is written BEFORE the refusal is returned, because that row IS the
        // lockout counter (item 4) and a refusal that did not count would be no lockout at all.
        app.record(
            &AuditEntry::new(
                at,
                day,
                Some(member.id.clone()),
                action::LOGIN_FAILED,
                "staff",
            )
            .about(member.id.as_str()),
        );
        let waiting = app
            .with_shop(|shop| {
                shop.db
                    .transaction(|tx| lockout_message(&mb_db::Repos::new(tx), &member))
                    .map_err(|e| words::from_db(&e))
            })
            .unwrap_or(None);
        return Err(UiError::new(
            "auth.wrong_pin",
            waiting.unwrap_or_else(|| "Wrong PIN. Try again.".to_owned()),
        ));
    }

    app.sessions().begin(actor_for(&member), at, false);
    app.record(
        &AuditEntry::new(at, day, Some(member.id.clone()), action::LOGIN_OK, "staff")
            .about(member.id.as_str()),
    );
    log_info!("{} signed in", member.name);
    lock_state_on(app)
}

/// Mb-db's row becomes mb-auth's actor.
pub fn actor_for(member: &mb_db::repo::people::StaffMember) -> mb_auth::Actor {
    mb_auth::Actor {
        staff_id: member.id.clone(),
        name: member.name.clone(),
        role_id: member.role_id.clone(),
        role_name: member.role_name.clone(),
        permissions: member.permissions.clone(),
        max_discount_bp: member.max_discount_bp,
        max_discount: member.max_discount,
    }
}

/// Lock the screen — by the button, or by `Ctrl+L`.
pub fn lock_now_on(app: &App) -> UiResult<LockState> {
    // A shop with no PIN cannot be locked, because there would be no way back in.
    if !app.shop_has_a_pin() {
        return Err(UiError::new(
            "auth.nothing_to_unlock_with",
            "Nobody has a PIN yet, so there would be no way back in. Set a PIN \
             in Staff first — then the counter locks itself as well.",
        ));
    }

    if let Some(who) = app.sessions().end() {
        app.record(&AuditEntry::new(
            crate::flows::now(),
            crate::flows::today(crate::flows::now()),
            Some(who.staff_id.clone()),
            action::LOGOUT,
            "staff",
        ));
        log_info!("{} locked the counter", who.name);
    }
    lock_state_on(app)
}

/// The way back in when the PIN is gone.
pub fn recover_with_code_on(
    app: &App,
    code: String,
    staff_id: String,
    new_pin: String,
) -> UiResult<String> {
    let at = crate::flows::now();
    let day = crate::flows::today(at);
    let pin = Pin::parse(&new_pin)
        .map_err(|e| UiError::new("auth.pin_shape", format!("{e}.")).with_detail(e.to_string()))?;

    let stored: Option<String> = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).settings().get(OUTLET, RECOVERY_KEY))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(stored) = stored else {
        return Err(UiError::new(
            "auth.no_recovery",
            "This shop has no recovery code. Ring support, with your licence key to hand.",
        ));
    };
    let stored = PinHash::from_stored(&stored).map_err(|e| {
        UiError::new(
            "auth.recovery_unreadable",
            "This shop's recovery code could not be read. Ring support.",
        )
        .with_detail(e.to_string())
    })?;

    if !mb_auth::verify_recovery_code(&code, &stored) {
        return Err(UiError::new(
            "auth.recovery_wrong",
            "That is not this shop's recovery code. Check the slip it was printed on.",
        ));
    }

    let (fresh, fresh_hash) = mb_auth::new_recovery_code().map_err(|e| {
        UiError::new(
            "auth.recovery_failed",
            "A new recovery code could not be made.",
        )
        .with_detail(e.to_string())
    })?;
    let hashed = mb_auth::hash_pin(&pin).map_err(|e| {
        UiError::new("auth.pin_failed", "That PIN could not be saved.").with_detail(e.to_string())
    })?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mut member) = repos.people().find_staff(OUTLET, &staff_id)? else {
                    return Err(mb_db::DbError::invariant(
                        "that person is not on the staff list",
                    ));
                };
                // Only somebody who manages staff.
                if !member.permissions.has(Permission::StaffManage) {
                    return Err(mb_db::DbError::invariant(
                        "the recovery code can only set a PIN for somebody who manages staff",
                    ));
                }
                member.pin_hash = Some(hashed.as_str().to_owned());
                repos.people().save_staff(OUTLET, &member, at)?;
                repos.settings().set(
                    OUTLET,
                    RECOVERY_KEY,
                    &fresh_hash.as_str().to_owned(),
                    at,
                    None,
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(member.id.clone()),
                        action::RECOVERY_USED,
                        "staff",
                    )
                    .about(member.id.as_str()),
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(member.id.clone()),
                        action::RECOVERY_ISSUED,
                        "shop",
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_warn!("the recovery code was used, and a new one was issued");
    // On paper as well as on screen — see `print_the_recovery_slip`.
    print_the_recovery_slip(app, &fresh.to_print(), day, true);
    // Returned so the screen can show it once as well.
    Ok(fresh.to_print())
}

// The people screen.

pub fn list_staff_on(app: &App) -> UiResult<Vec<PersonView>> {
    guard::require(app, Permission::StaffManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut out = Vec::new();
                for member in repos.people().list_staff(OUTLET)? {
                    // The stand-in is not a person, and the staff list is "who works here".
                    if member.id.as_str() == crate::state::DEFAULT_STAFF {
                        continue;
                    }
                    let locked_out = lockout_message(&repos, &member)?;
                    out.push(person_view(&member, locked_out));
                }
                Ok(out)
            })
            .map_err(|e| words::from_db(&e))
    })
}

pub fn list_permissions_on(app: &App) -> UiResult<Vec<(String, String)>> {
    guard::require(app, Permission::StaffManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).people().permission_codes())
            .map_err(|e| words::from_db(&e))
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RoleView {
    pub id: String,
    pub name: String,
    pub is_builtin: bool,
    pub permissions: Vec<String>,
    /// A percentage as text, both ways — `"12.5%"` out, whatever was typed back in.
    pub max_discount_percent: Option<String>,
    pub max_discount: Option<MoneyView>,
}

pub fn list_roles_on(app: &App) -> UiResult<Vec<RoleView>> {
    guard::require(app, Permission::StaffManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).people().list_roles(OUTLET))
            .map_err(|e| words::from_db(&e))
            .map(|roles| roles.iter().map(role_view).collect())
    })
}

fn role_view(role: &RoleShape) -> RoleView {
    RoleView {
        id: role.id.clone(),
        name: role.name.clone(),
        is_builtin: role.is_builtin,
        permissions: role
            .permissions
            .iter()
            .map(|p| p.code().to_owned())
            .collect(),
        max_discount_percent: role.percent_label(),
        max_discount: role.max_discount.map(MoneyView::from),
    }
}

pub fn save_role_on(app: &App, role: RoleView) -> UiResult<Vec<RoleView>> {
    let who = guard::require(app, Permission::StaffManage)?;
    let at = crate::flows::now();
    let day = crate::flows::today(at);

    // BACKEND-G7 at the boundary.
    let permissions = mb_auth::PermissionSet::from_codes(&role.permissions)
        .map_err(|e| UiError::new("role.permission", format!("{e}. Reload and try again.")))?;

    let max_discount_bp =
        RoleShape::parse_percent(role.max_discount_percent.as_deref().unwrap_or_default())
            .map_err(|e| UiError::new("role.percent", e.to_string()))?;

    let shape = RoleShape {
        id: role.id.clone(),
        name: role.name.clone(),
        is_builtin: role.is_builtin,
        permissions,
        max_discount_bp,
        max_discount: role
            .max_discount
            .as_ref()
            .map(|m| mb_core::Money::from_paise(m.paise)),
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let before = repos
                    .people()
                    .list_roles(OUTLET)?
                    .into_iter()
                    .find(|r| r.id == shape.id);
                // Before and after, not just after — see the note in `save_staff_member_on`.
                let had_one = !repos.people().active_administrators(OUTLET)?.is_empty();
                repos.people().save_role(OUTLET, &shape, at)?;

                // The last-administrator rule. A shop nobody can administer can never be
                // repaired from its own counter.
                if had_one && repos.people().active_administrators(OUTLET)?.is_empty() {
                    return Err(mb_db::DbError::invariant(
                        "that would leave nobody able to manage staff — give somebody else \
                         that permission first",
                    ));
                }

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::ROLE_SAVED,
                        "role",
                    )
                    .about(&shape.id)
                    .changed(
                        before.as_ref().map_or(serde_json::Value::Null, role_json),
                        role_json(&shape),
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    list_roles_on(app)
}

fn role_json(role: &RoleShape) -> serde_json::Value {
    serde_json::json!({
        "name": role.name,
        "permissions": role.permissions.codes(),
        "max_discount_bp": role.max_discount_bp,
        "max_discount_paise": role.max_discount.map(mb_core::Money::paise),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct StaffEdit {
    pub id: String,
    pub name: String,
    pub role_id: Option<String>,
    /// "active", "suspended" or "left".
    pub status: String,
}

pub fn save_staff_member_on(app: &App, staff: StaffEdit) -> UiResult<Vec<PersonView>> {
    let who = guard::require(app, Permission::StaffManage)?;
    let at = crate::flows::now();
    let day = crate::flows::today(at);

    let status = match staff.status.as_str() {
        "active" => mb_db::repo::people::StaffStatus::Active,
        "suspended" => mb_db::repo::people::StaffStatus::Suspended,
        "left" => mb_db::repo::people::StaffStatus::Left,
        other => {
            return Err(UiError::new(
                "staff.status",
                format!("\"{other}\" is not something a staff member can be."),
            ));
        }
    };

    if staff.name.trim().is_empty() {
        return Err(UiError::new("staff.name", "A staff member needs a name."));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let existing = repos.people().find_staff(OUTLET, &staff.id)?;
                let before = existing.as_ref().map(staff_json);
                // Whether there WAS one, not only whether there is one.
                let had_one = !repos.people().active_administrators(OUTLET)?.is_empty();
                let member = mb_db::repo::people::StaffMember {
                    id: mb_core::StaffId::new(staff.id.clone()),
                    name: staff.name.trim().to_owned(),
                    role_id: staff.role_id.clone(),
                    role_name: None,
                    // A PIN is set by its own command.
                    pin_hash: existing.as_ref().and_then(|m| m.pin_hash.clone()),
                    status,
                    permissions: mb_auth::PermissionSet::new(),
                    max_discount_bp: None,
                    max_discount: None,
                };
                repos.people().save_staff(OUTLET, &member, at)?;

                if had_one && repos.people().active_administrators(OUTLET)?.is_empty() {
                    return Err(mb_db::DbError::invariant(
                        "that would leave nobody able to manage staff — this is the last person \
                         who can, so give somebody else that permission first",
                    ));
                }

                let after = repos
                    .people()
                    .find_staff(OUTLET, &staff.id)?
                    .as_ref()
                    .map_or(serde_json::Value::Null, staff_json);
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::STAFF_SAVED,
                        "staff",
                    )
                    .about(&staff.id)
                    .changed(before.unwrap_or(serde_json::Value::Null), after),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    list_staff_on(app)
}

/// Never the PIN itself.
fn staff_json(member: &mb_db::repo::people::StaffMember) -> serde_json::Value {
    serde_json::json!({
        "name": member.name,
        "role_id": member.role_id,
        "status": status_word(member.status),
        "has_pin": member.pin_hash.is_some(),
    })
}

/// Set or clear a PIN.
pub fn set_staff_pin_on(
    app: &App,
    staff_id: String,
    pin: Option<String>,
) -> UiResult<Option<String>> {
    let who = guard::require(app, Permission::StaffManage)?;
    let at = crate::flows::now();
    let day = crate::flows::today(at);
    // What the shop looked like BEFORE.
    let had_a_pin = app.shop_has_a_pin();

    let hashed = match pin.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(typed) => {
            let parsed = Pin::parse(typed).map_err(|e| {
                UiError::new("auth.pin_shape", format!("{e}.")).with_detail(e.to_string())
            })?;
            Some(mb_auth::hash_pin(&parsed).map_err(|e| {
                UiError::new("auth.pin_failed", "That PIN could not be saved.")
                    .with_detail(e.to_string())
            })?)
        }
        None => None,
    };

    let generated = match hashed {
        Some(_) => Some(mb_auth::new_recovery_code().map_err(|e| {
            UiError::new("auth.recovery_failed", "A recovery code could not be made.")
                .with_detail(e.to_string())
        })?),
        None => None,
    };

    let issued = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mut member) = repos.people().find_staff(OUTLET, &staff_id)? else {
                    return Err(mb_db::DbError::invariant(
                        "that person is not on the staff list",
                    ));
                };
                // A PIN with no role is somebody who can sign in and do nothing, which looks
                // like a broken app rather than a locked one.
                if hashed.is_some() && member.role_id.is_none() {
                    return Err(mb_db::DbError::invariant(
                        "give this person a role before setting their PIN",
                    ));
                }
                member.pin_hash = hashed.as_ref().map(|h| h.as_str().to_owned());
                repos.people().save_staff(OUTLET, &member, at)?;

                // The shop's first recovery code, the first time somebody who manages staff
                // gets a PIN.
                let existing: Option<String> = repos.settings().get(OUTLET, RECOVERY_KEY)?;
                let mut issued = None;
                if existing.is_none()
                    && member.permissions.has(Permission::StaffManage)
                    && let Some((code, hash)) = generated.as_ref()
                {
                    {
                        repos.settings().set(
                            OUTLET,
                            RECOVERY_KEY,
                            &hash.as_str().to_owned(),
                            at,
                            None,
                        )?;
                        repos.audit().append(
                            OUTLET,
                            &AuditEntry::new(
                                at,
                                day,
                                Some(who.staff_id.clone()),
                                action::RECOVERY_ISSUED,
                                "shop",
                            ),
                        )?;
                        issued = Some(code.to_print());
                    }
                }

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::PIN_SET,
                        "staff",
                    )
                    .about(&staff_id)
                    .with_after(serde_json::json!({ "has_pin": hashed.is_some() })),
                )?;
                Ok(issued)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // On paper as well as on screen, and BEFORE the relock below — a shop whose first PIN has
    // just been set is about to be looking at a lock screen, and the slip should already be
    // coming out of the printer by then.
    if let Some(code) = issued.as_deref() {
        print_the_recovery_slip(app, code, day, false);
    }

    // Setting the first PIN locks the app, here and now.
    app.relock_if_this_was_the_first_pin(had_a_pin);

    Ok(issued)
}

/// Put the shop's recovery code on paper.
fn print_the_recovery_slip(app: &App, code: &str, day: BusinessDay, replaces_an_older: bool) {
    let printed = crate::flows::default_printer(app).and_then(|printer| {
        let config = app.shop_config();
        let store = config.store.to_print_store();
        let document = mb_print::template::recovery_document(
            printer.paper,
            &mb_print::template::RecoveryContext {
                code,
                // A PIN can be set before the shop profile is finished, and a slip with no name
                // on it is better than no slip.
                store: (!store.name.trim().is_empty()).then_some(&store),
                issued_on: &day_in_words(day),
                replaces_an_older_code: replaces_an_older,
            },
        );
        app.print(
            Job::new(JobKind::Recovery, &printer.id, document, day)
                .because("recovery code".to_owned()),
        )
    });

    if let Err(cause) = printed {
        // Loud, because the shop is now one closed dialog away from having no way back in — and
        // this is the line a support call starts from.
        log_warn!(
            "the recovery slip could not be printed ({}) — the code is on screen only",
            cause.message
        );
    }
}

fn day_in_words(day: BusinessDay) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let (year, month, d) = day.to_ymd();
    // `month` is 1..=12 from `to_ymd`; this arrives at the right name without a cast that D7
    // would have to argue about, and at "" if it ever does not.
    let name = usize::try_from(month)
        .ok()
        .and_then(|month| month.checked_sub(1))
        .and_then(|index| MONTHS.get(index))
        .copied()
        .unwrap_or_default();
    format!("{d} {name} {year}")
}

// The history.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AuditView {
    pub entries: Vec<AuditEntryView>,
    /// The sentence to show when the chain is broken.
    pub tampered: Option<String>,
    /// Every action this build knows, for the filter — from the list, not from whatever happens
    /// to be in this shop's data.
    pub actions: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AuditEntryView {
    pub seq: i64,
    pub when: String,
    pub who: String,
    pub what: String,
    pub about: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
}

pub fn audit_trail_on(
    app: &App,
    staff_id: Option<String>,
    action_code: Option<String>,
    days: Option<i32>,
) -> UiResult<AuditView> {
    guard::require(app, Permission::AuditView)?;
    let today = crate::flows::today(crate::flows::now());
    let from_day = days.map(|d| BusinessDay::from_days_since_epoch(today.days_since_epoch() - d));

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let rows = repos.audit().list(
                    OUTLET,
                    &mb_db::repo::AuditFilter {
                        from_day,
                        to_day: None,
                        staff_id,
                        action: action_code,
                        limit: 200,
                    },
                )?;
                // Whatever an entry is about is named, never shown as an id.
                let mut names: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for person in repos.people().list_staff(OUTLET)? {
                    names.insert(person.id.as_str().to_owned(), person.name);
                }
                for item in repos.menu().list_items(OUTLET, false)? {
                    names.insert(item.id.as_str().to_owned(), item.name);
                }
                for table in repos.floor().list_tables(OUTLET)? {
                    names.insert(
                        table.id.as_str().to_owned(),
                        format!("Table {}", table.label),
                    );
                }
                for customer in repos.money().list_customers(OUTLET)? {
                    names.insert(customer.id.as_str().to_owned(), customer.name);
                }
                for material in repos.stock().materials(OUTLET, true)? {
                    names.insert(material.id.as_str().to_owned(), material.name);
                }
                let tampered = repos.audit().verify(OUTLET)?.err().map(|b| {
                    format!(
                        "This shop's history has been changed outside Magic Bill — {b}. \
                         Treat everything after that point with care."
                    )
                });
                Ok(AuditView {
                    entries: rows.iter().map(|row| entry_view(row, &names)).collect(),
                    tampered,
                    actions: action::ALL
                        .iter()
                        .map(|a| ((*a).to_owned(), action::words(a).to_owned()))
                        .collect(),
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

fn entry_view(
    row: &mb_auth::AuditRow,
    names: &std::collections::HashMap<String, String>,
) -> AuditEntryView {
    AuditEntryView {
        seq: row.seq,
        when: crate::words::when(Timestamp::from_millis(row.at)),
        who: row
            .staff_name
            .clone()
            .unwrap_or_else(|| "Somebody not on the staff list".to_owned()),
        what: action::words(&row.action).to_owned(),
        about: row
            .entity_id
            .as_ref()
            .map(|id| names.get(id).cloned().unwrap_or_else(|| id.clone())),
        before: row.before_json.as_deref().map(readable_change),
        after: row.after_json.as_deref().map(readable_change),
    }
}

/// A change, in words a shopkeeper reads.
fn readable_change(json: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        // Unparseable: show it as it is rather than hiding a row of history.
        return json.to_owned();
    };
    let Some(fields) = value.as_object() else {
        return value.to_string();
    };

    let mut parts: Vec<String> = Vec::new();
    for (key, field) in fields {
        // A settings change is keyed by its setting's KEY, and the screen must not show one.
        if let Some(entry) = crate::settings::catalog::find(key) {
            let shown = match field {
                serde_json::Value::String(s) if s.is_empty() => "—".to_owned(),
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => "—".to_owned(),
                other => other.to_string(),
            };
            parts.push(format!("{} {shown}", entry.label));
            continue;
        }
        let label = key.replace('_', " ");
        let label = label.strip_suffix(" paise").unwrap_or(&label);
        // A key like `total_paise` names the unit, not the reader's business.
        let shown = if key.ends_with("_paise") {
            field.as_i64().map_or_else(
                || field.to_string(),
                |paise| mb_core::Money::from_paise(paise).to_plain_string(),
            )
        } else {
            match field {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Bool(b) => if *b { "yes" } else { "no" }.to_owned(),
                serde_json::Value::Null => "—".to_owned(),
                other => other.to_string(),
            }
        };
        let mut label = label.to_owned();
        if let Some(first) = label.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        parts.push(format!("{label} {shown}"));
    }
    parts.join(", ")
}

// The command wrappers.

#[tauri::command]
pub fn lock_state(app: tauri::State<'_, App>) -> UiResult<LockState> {
    lock_state_on(&app)
}

#[tauri::command]
pub fn login(app: tauri::State<'_, App>, staff_id: String, pin: String) -> UiResult<LockState> {
    login_on(&app, staff_id, pin)
}

#[tauri::command]
pub fn lock_now(app: tauri::State<'_, App>) -> UiResult<LockState> {
    lock_now_on(&app)
}

#[tauri::command]
pub fn recover_with_code(
    app: tauri::State<'_, App>,
    code: String,
    staff_id: String,
    new_pin: String,
) -> UiResult<String> {
    recover_with_code_on(&app, code, staff_id, new_pin)
}

#[tauri::command]
pub fn list_staff(app: tauri::State<'_, App>) -> UiResult<Vec<PersonView>> {
    list_staff_on(&app)
}

#[tauri::command]
pub fn list_permissions(app: tauri::State<'_, App>) -> UiResult<Vec<(String, String)>> {
    list_permissions_on(&app)
}

#[tauri::command]
pub fn list_roles(app: tauri::State<'_, App>) -> UiResult<Vec<RoleView>> {
    list_roles_on(&app)
}

#[tauri::command]
pub fn save_role(app: tauri::State<'_, App>, role: RoleView) -> UiResult<Vec<RoleView>> {
    save_role_on(&app, role)
}

#[tauri::command]
pub fn save_staff_member(
    app: tauri::State<'_, App>,
    staff: StaffEdit,
) -> UiResult<Vec<PersonView>> {
    save_staff_member_on(&app, staff)
}

#[tauri::command]
pub fn set_staff_pin(
    app: tauri::State<'_, App>,
    staff_id: String,
    pin: Option<String>,
) -> UiResult<Option<String>> {
    set_staff_pin_on(&app, staff_id, pin)
}

#[tauri::command]
pub fn audit_trail(
    app: tauri::State<'_, App>,
    staff_id: Option<String>,
    action_code: Option<String>,
    days: Option<i32>,
) -> UiResult<AuditView> {
    audit_trail_on(&app, staff_id, action_code, days)
}

/// The floor, as a screen shows it.
#[tauri::command]
pub fn open_orders(app: tauri::State<'_, App>) -> UiResult<Vec<TableView>> {
    open_orders_on(&app)
}

/// Ranked item search.
#[tauri::command]
pub fn search_items(
    app: tauri::State<'_, App>,
    text: String,
    mode: Option<crate::search::MatchMode>,
) -> UiResult<Vec<MenuItemView>> {
    search_items_on(&app, text, mode)
}

// Bodies over `&App`, so the billing budgets can be measured and the correction sequences
// driven without a window.

#[tauri::command]
pub fn open_table(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    table_id: String,
) -> UiResult<CartView> {
    shown(&handle, open_table_on(&app, table_id))
}

#[tauri::command]
pub fn open_order(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    order_id: String,
) -> UiResult<CartView> {
    shown(&handle, open_order_on(&app, order_id))
}

fn shown(handle: &tauri::AppHandle, view: UiResult<CartView>) -> UiResult<CartView> {
    if let Ok(cart) = &view {
        crate::devices::show_bill(handle, cart);
    }
    view
}

#[tauri::command]
pub fn cart_add(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    item_id: String,
    qty: Option<String>,
    note: Option<String>,
) -> UiResult<CartView> {
    shown(&handle, cart_add_on(&app, item_id, qty, note))
}

#[tauri::command]
pub fn cart_clear(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    keep_type: bool,
) -> UiResult<CartView> {
    shown(&handle, cart_clear_on(&app, keep_type))
}

#[cfg(test)]
mod change_words {
    use super::readable_change;

    #[test]
    fn a_change_reads_as_words_and_rupees() {
        let said =
            readable_change(r#"{"reason":"Billed twice","state":"voided","total_paise":39300}"#);
        assert_eq!(said, "Reason Billed twice, State voided, Total 393.00");
        assert!(!said.contains('{'), "{said}");
        assert!(!said.contains("paise"), "{said}");
    }

    #[test]
    fn money_is_formatted_by_rust_and_only_by_rust() {
        // TypeScript never divides by a hundred, here or anywhere.
        assert_eq!(readable_change(r#"{"amount_paise":5}"#), "Amount 0.05");
        assert_eq!(
            readable_change(r#"{"amount_paise":100000}"#),
            "Amount 1000.00"
        );
    }

    #[test]
    fn a_flag_reads_as_a_word() {
        assert_eq!(readable_change(r#"{"has_pin":true}"#), "Has pin yes");
        assert_eq!(readable_change(r#"{"has_pin":false}"#), "Has pin no");
    }

    #[test]
    fn something_unparseable_is_shown_rather_than_hidden() {
        // A trail with a gap in it is worth less than an ugly line in it.
        assert_eq!(readable_change("not json"), "not json");
    }
}
