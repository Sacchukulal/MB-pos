//! The printer setup.

use mb_auth::Permission;
use mb_core::Timestamp;
use mb_db::repo::settings::Printer;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::guard;
use crate::state::{App, OUTLET, printer_config_for};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

// What the screen sees.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PrintersView {
    pub printers: Vec<PrinterRowView>,
    /// The real printers Windows knows about, for the "which one?" list.
    pub windows: Vec<String>,
    /// Which printer each category's kitchen tickets go to.
    pub routes: Vec<RouteView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PrinterRowView {
    pub id: String,
    pub name: String,
    /// `spooler`, `network`, `serial` or `none`.
    pub kind: String,
    /// The Windows name, the `host:port`, or the COM port.
    pub address: String,
    pub connection: String,
    pub paper_mm: u32,
    pub is_default: bool,
    /// `bill`, `kitchen` or `both`.
    pub role: String,
    /// `raster` or `text`.
    pub engine: String,
    pub is_bold_dark: bool,
    pub can_kick_drawer: bool,
    pub offset_x_mm: i32,
    pub offset_y_mm: i32,
    /// True for the stand-in row a shop has before it sets a printer up.
    pub is_stand_in: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RouteView {
    pub category_id: String,
    pub category: String,
    /// Empty means "wherever the kitchen tickets go by default".
    pub printer_id: String,
}

/// A printer on its way back from the screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PrinterEdit {
    /// Empty for a new one.
    pub id: String,
    pub name: String,
    pub kind: String,
    pub address: String,
    pub paper_mm: u32,
    pub is_default: bool,
    pub role: String,
    pub engine: String,
    pub is_bold_dark: bool,
    pub can_kick_drawer: bool,
}

fn connection_words(kind: &str, address: &str) -> String {
    match (kind, address) {
        ("spooler", "") => "Windows: not chosen yet".to_owned(),
        ("spooler", name) => format!("Windows: {name}"),
        ("network", "") => "Network: no address yet".to_owned(),
        ("network", address) => format!("Network: {address}"),
        ("serial", "") => "Serial: no port yet".to_owned(),
        ("serial", port) => format!("Serial: {port}"),
        _ => "Not connected — it takes jobs and prints nothing".to_owned(),
    }
}

fn row_view(printer: &Printer) -> PrinterRowView {
    PrinterRowView {
        connection: connection_words(&printer.kind, printer.address.as_deref().unwrap_or("")),
        is_stand_in: printer.id == crate::state::NO_PRINTER,
        id: printer.id.clone(),
        name: printer.name.clone(),
        kind: printer.kind.clone(),
        address: printer.address.clone().unwrap_or_default(),
        // Counts and small numbers cross as u32/i32, never i64.
        paper_mm: u32::try_from(printer.paper_mm).unwrap_or(80),
        is_default: printer.is_default,
        role: printer.role.clone(),
        engine: printer.engine.clone(),
        is_bold_dark: printer.is_bold_dark,
        can_kick_drawer: printer.can_kick_drawer,
        offset_x_mm: i32::try_from(printer.offset_x_mm).unwrap_or(0),
        offset_y_mm: i32::try_from(printer.offset_y_mm).unwrap_or(0),
    }
}

pub fn printers_on(app: &App) -> UiResult<PrintersView> {
    guard::require(app, Permission::SettingsPrinter)?;

    // Asked of Windows, not remembered.
    let windows = mb_winprint::list_printers().map_or_else(
        |e| {
            log_warn!("Windows would not list its printers ({e}); the list is empty");
            Vec::new()
        },
        |list| list.into_iter().map(|p| p.name).collect(),
    );

    app.with_shop(|shop| {
        let (printers, routes) = shop
            .db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let printers = repos.settings().list_printers(OUTLET)?;
                let categories = repos.menu().list_categories(OUTLET)?;
                let mapped = repos.settings().category_printers(OUTLET)?;
                let routes = categories
                    .into_iter()
                    .map(|category| RouteView {
                        printer_id: mapped
                            .iter()
                            .find(|(c, _)| *c == category.id.as_str())
                            .map(|(_, p)| p.clone())
                            .unwrap_or_default(),
                        category_id: category.id.as_str().to_owned(),
                        category: category.name,
                    })
                    .collect();
                Ok((printers, routes))
            })
            .map_err(|e| words::from_db(&e))?;

        Ok(PrintersView {
            printers: printers.iter().map(row_view).collect(),
            windows,
            routes,
        })
    })
}

/// The three paper widths this product supports, and the reason a fourth is a decision rather
/// than a number: `mb_print::paper` knows the column counts.
const PAPERS: [u32; 3] = [58, 80, 100];

pub fn save_printer_on(app: &App, edit: PrinterEdit) -> UiResult<PrintersView> {
    let who = guard::require(app, Permission::SettingsPrinter)?;

    if edit.name.trim().is_empty() {
        return Err(UiError::new(
            "printer.name",
            "Give this printer a name you will recognise — \"Counter\", \
             \"Kitchen\".",
        ));
    }
    if !PAPERS.contains(&edit.paper_mm) {
        return Err(UiError::new(
            "printer.paper",
            "Paper is 58 mm, 80 mm or 100 mm wide.",
        ));
    }
    if !["spooler", "network", "serial", "none"].contains(&edit.kind.as_str()) {
        return Err(UiError::new(
            "printer.kind",
            "A printer is connected through Windows, over the network, or by a \
             serial cable.",
        ));
    }
    if !["bill", "kitchen", "both"].contains(&edit.role.as_str()) {
        return Err(UiError::new(
            "printer.role",
            "A printer prints bills, kitchen tickets, or both.",
        ));
    }
    // A network printer with no address prints nowhere and says it is fine.
    if edit.kind == "network" && !edit.address.contains(':') {
        return Err(UiError::new(
            "printer.address",
            "A network printer needs an address and a port, like \
             192.168.1.50:9100.",
        ));
    }

    let at = crate::flows::now();
    let id = if edit.id.trim().is_empty() {
        crate::newid::fresh_at("prn", at)
    } else {
        edit.id.clone()
    };

    let row = Printer {
        id: id.clone(),
        name: edit.name.trim().to_owned(),
        kind: edit.kind.clone(),
        address: if edit.address.trim().is_empty() {
            None
        } else {
            Some(edit.address.trim().to_owned())
        },
        paper_mm: i64::from(edit.paper_mm),
        is_default: edit.is_default,
        can_kick_drawer: edit.can_kick_drawer,
        // The offset is NOT edited here.
        offset_x_mm: 0,
        offset_y_mm: 0,
        role: edit.role.clone(),
        engine: edit.engine.clone(),
        is_bold_dark: edit.is_bold_dark,
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let existing = repos.settings().list_printers(OUTLET)?;
                let before = existing.iter().find(|p| p.id == id);

                /*
                  **THE STAND-IN GIVES WAY, AND THIS IS THE BUG THE OWNER
                  REPORTED ON 2026-08-17.**

                  > *"even if we added a printer again its not printig real
                  > bill."*

                  Every shop starts with `state::fallback_row` — a real row,
                  `kind = 'none'`, `is_default = true`, whose whole job is to
                  accept jobs and print nothing so a shop can bill on its first
                  day (requirement 3). It has to be default, because on day one
                  it is the only printer there is.

                  Nothing ever took that back. The owner installed their TVSE,
                  filled the dialog in, saved, and the stand-in was **still**
                  the default — so `flows::default_printer` kept answering
                  "prn_none" and every bill was rendered, queued, marked
                  printed, and thrown away. No error anywhere, because
                  printing nothing is exactly what that row promises to do.
                  The only way to escape it was a checkbox at the bottom of
                  the dialog reading "Bills go here unless something says
                  otherwise", which does not contain the word default.

                  So: **a real printer arriving where the only default is the
                  stand-in becomes the default.** Not "always become the
                  default" — a shop adding its second and third printers has
                  already made that choice and this must not overrule it. Only
                  the placeholder is ever displaced, which is what a
                  placeholder is for.
                */
                let real = row.kind != "none";
                let default_is_a_placeholder = existing
                    .iter()
                    .find(|p| p.is_default)
                    .is_none_or(|p| p.id == crate::state::NO_PRINTER);

                let row = Printer {
                    // Keep the offset somebody has already nudged.
                    offset_x_mm: before.map_or(0, |p| p.offset_x_mm),
                    offset_y_mm: before.map_or(0, |p| p.offset_y_mm),
                    is_default: row.is_default || (real && default_is_a_placeholder),
                    ..row.clone()
                };
                repos.settings().save_printer(OUTLET, &row, at)?;

                // One default, and it is the shop's answer to "where does a bill go?".
                if row.is_default {
                    for other in existing.iter().filter(|p| p.id != id && p.is_default) {
                        repos.settings().save_printer(
                            OUTLET,
                            &Printer {
                                is_default: false,
                                ..other.clone()
                            },
                            at,
                        )?;
                    }
                }
                repos.audit().append(
                    OUTLET,
                    &mb_auth::AuditEntry::new(
                        at,
                        crate::flows::today(at),
                        Some(who.staff_id.clone()),
                        mb_auth::audit::action::SETTING_CHANGED,
                        "printers",
                    )
                    .about(id.clone())
                    .with_after(serde_json::json!({
                        "Printer": row.name,
                        "Connected by": row.kind,
                        "Paper": format!("{} mm", row.paper_mm),
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // The queue runs a thread per printer it was STARTED with, so a printer saved now has
    // nowhere to send anything until the queue is built again.
    app.rebuild_queue();
    log_info!("{} saved the printer \"{}\"", who.name, row.name);

    printers_on(app)
}

/// Choose where bills print.
pub fn set_default_printer_on(app: &App, printer_id: String) -> UiResult<PrintersView> {
    let who = guard::require(app, Permission::SettingsPrinter)?;
    let at = crate::flows::now();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let existing = repos.settings().list_printers(OUTLET)?;
                if !existing.iter().any(|p| p.id == printer_id) {
                    return Err(mb_db::DbError::invariant(
                        "that printer is not set up any more",
                    ));
                }
                for printer in &existing {
                    let wanted = printer.id == printer_id;
                    if printer.is_default != wanted {
                        repos.settings().save_printer(
                            OUTLET,
                            &Printer {
                                is_default: wanted,
                                ..printer.clone()
                            },
                            at,
                        )?;
                    }
                }
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // The queue is the printer list made real — same reason as saving one.
    app.rebuild_queue();
    log_info!("{} chose where bills print", who.name);
    printers_on(app)
}

/// Which paper this shop's bills print on — 58 mm (2 inch), 80 mm (3 inch) or 100 mm (4 inch).
pub fn set_paper_on(app: &App, mm: u32) -> UiResult<PrintersView> {
    let who = guard::require(app, Permission::SettingsPrinter)?;
    if !PAPERS.contains(&mm) {
        return Err(UiError::new(
            "printer.paper",
            "Paper is 58 mm (2 inch), 80 mm (3 inch) or 100 mm (4 inch).",
        ));
    }
    let at = crate::flows::now();

    // The printer bills actually go to — the same answer `flows::default_printer` gives, so
    // changing "the paper" here changes the paper of the roll a customer's bill comes out on
    // rather than of whichever row is first.
    let target = crate::flows::default_printer(app)?.id;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(printer) = repos
                    .settings()
                    .list_printers(OUTLET)?
                    .into_iter()
                    .find(|p| p.id == target)
                else {
                    return Err(mb_db::DbError::invariant("that printer is gone"));
                };
                repos.settings().save_printer(
                    OUTLET,
                    &Printer {
                        paper_mm: i64::from(mm),
                        ..printer
                    },
                    at,
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // The queue holds each printer's paper in its worker, so it has to be told.
    app.rebuild_queue();
    log_info!("{} set the bill paper to {mm} mm", who.name);
    printers_on(app)
}

pub fn delete_printer_on(app: &App, id: String) -> UiResult<PrintersView> {
    let who = guard::require(app, Permission::SettingsPrinter)?;

    if id == crate::state::NO_PRINTER {
        return Err(UiError::new(
            "printer.stand_in",
            "This is the stand-in every shop starts with. Change it into your \
             real printer instead of removing it.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                // A printer with paper in the spool cannot go.
                let waiting = repos.print_jobs().count_for_printer(&id)?;
                if waiting > 0 {
                    return Err(mb_db::DbError::invariant(format!(
                        "there {} still {waiting} print job{} against this \
                         printer",
                        if waiting == 1 { "is" } else { "are" },
                        if waiting == 1 { "" } else { "s" }
                    )));
                }
                repos
                    .settings()
                    .delete_printer(OUTLET, &id, crate::flows::now())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // Same as saving one: the queue is the printer list made real.
    app.rebuild_queue();
    log_info!("{} removed a printer", who.name);
    printers_on(app)
}

/// Which printer this category's kitchen tickets go to.
pub fn route_category_on(
    app: &App,
    category_id: String,
    printer_id: String,
) -> UiResult<PrintersView> {
    guard::require(app, Permission::SettingsPrinter)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).settings().route_category(
                    OUTLET,
                    &category_id,
                    if printer_id.is_empty() {
                        None
                    } else {
                        Some(printer_id.as_str())
                    },
                    crate::flows::now(),
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    printers_on(app)
}

/// The test print, and it is a whole bill.
pub fn print_sample_on(app: &App, printer_id: String) -> UiResult<String> {
    let who = guard::require(app, Permission::SettingsPrinter)?;
    let config = app.shop_config();

    let printer = app.with_shop(|shop| {
        let rows = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .map_err(|e| words::from_db(&e))?;
        rows.iter()
            .find(|p| p.id == printer_id)
            .map(printer_config_for)
            .ok_or_else(|| {
                UiError::new(
                    "printer.unknown",
                    "That printer is not set up any more. Choose another one.",
                )
            })
    })?;

    let (bill, order) = super::sample::sample_order(config.store.registration())
        .map_err(|e| words::from_print(&e))?;
    let store = config.store.to_print_store();
    let table = crate::flows::first_table_label(app);
    let time = crate::flows::clock_time(crate::flows::now());
    let (metrics, _) = app.metrics_for(mb_print::queue::JobKind::Test, &printer);
    let document = mb_print::template::bill_document(
        &metrics,
        &mb_print::template::BillContext {
            bill: &bill,
            order: &order,
            store: &store,
            settings: &config.receipt,
            customer: None,
            cashier: Some(who.name.as_str()),
            // The same values a real bill resolves.
            table: table.as_deref(),
            time: Some(time.as_str()),
            waiter: Some(who.name.as_str()),
            copy: mb_print::template::Copy::Original,
            einvoice: mb_print::template::EInvoice::default(),
            // The same letterhead a real bill will carry.
            logo: crate::logo::stored(app),
        },
    )
    .map_err(|e| words::from_print(&e))?;

    let at = crate::flows::now();
    app.print(
        mb_print::queue::Job::new(
            mb_print::queue::JobKind::Test,
            &printer.id,
            document,
            crate::flows::today(at),
        )
        .because("sample bill".to_owned()),
    )
}

/// Print, look at the paper, nudge, print again.
pub fn nudge_on(app: &App, printer_id: String, dx_mm: i32, dy_mm: i32) -> UiResult<PrintersView> {
    // The existing `ipc::nudge_print_offset` does the arithmetic and the clamping; this is the
    // settings screen's door to it, so there is one rule and not two.
    crate::ipc::nudge_offset_on(app, printer_id, dx_mm, dy_mm)?;
    printers_on(app)
}

/// A timestamp, for the generated printer id.
const _: Option<Timestamp> = None;

// The seats.

#[tauri::command]
pub fn printer_setup(app: tauri::State<'_, App>) -> UiResult<PrintersView> {
    printers_on(&app)
}

#[tauri::command]
pub fn save_printer(app: tauri::State<'_, App>, edit: PrinterEdit) -> UiResult<PrintersView> {
    save_printer_on(&app, edit)
}

#[tauri::command]
pub fn delete_printer(app: tauri::State<'_, App>, id: String) -> UiResult<PrintersView> {
    delete_printer_on(&app, id)
}

#[tauri::command]
pub fn set_default_printer(
    app: tauri::State<'_, App>,
    printer_id: String,
) -> UiResult<PrintersView> {
    set_default_printer_on(&app, printer_id)
}

#[tauri::command]
pub fn set_paper_size(app: tauri::State<'_, App>, mm: u32) -> UiResult<PrintersView> {
    set_paper_on(&app, mm)
}

#[tauri::command]
pub fn route_category(
    app: tauri::State<'_, App>,
    category_id: String,
    printer_id: String,
) -> UiResult<PrintersView> {
    route_category_on(&app, category_id, printer_id)
}

#[tauri::command]
pub fn print_sample_bill(app: tauri::State<'_, App>, printer_id: String) -> UiResult<String> {
    print_sample_on(&app, printer_id)
}

#[tauri::command]
pub fn nudge_printer(
    app: tauri::State<'_, App>,
    printer_id: String,
    dx_mm: i32,
    dy_mm: i32,
) -> UiResult<PrintersView> {
    nudge_on(&app, printer_id, dx_mm, dy_mm)
}
