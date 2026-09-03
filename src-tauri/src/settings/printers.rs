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
    /// `same`: every kitchen ticket goes to the bill printer. `other`: each category may name
    /// its own printer; the rest go to the bill printer.
    pub kitchen_mode: String,
    /// `combined`: one ticket per printer. `category`: one ticket per category on it.
    pub ticket_style: String,
}

/// Where the two kitchen choices live: settings rows of the shop's own, read at print time.
const KITCHEN_MODE_KEY: &str = "printers.kitchen_mode";
const TICKET_STYLE_KEY: &str = "printers.ticket_style";

/// How this shop wants its kitchen tickets: which printers, and how many tickets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KitchenRouting {
    /// True when each category may have its own printer.
    pub separate_printers: bool,
    /// True when every category gets a ticket of its own.
    pub per_category: bool,
}

impl KitchenRouting {
    const fn mode_word(self) -> &'static str {
        if self.separate_printers { "other" } else { "same" }
    }

    const fn style_word(self) -> &'static str {
        if self.per_category { "category" } else { "combined" }
    }
}

/// Read the two choices. A shop that has never chosen prints one ticket per printer, and uses
/// other printers exactly when a category has been given one.
pub(crate) fn kitchen_routing(repos: &mb_db::Repos<'_>) -> Result<KitchenRouting, mb_db::DbError> {
    let mode: Option<String> = repos.settings().get(OUTLET, KITCHEN_MODE_KEY)?;
    let style: Option<String> = repos.settings().get(OUTLET, TICKET_STYLE_KEY)?;
    let separate_printers = match mode.as_deref() {
        Some("other") => true,
        Some(_) => false,
        None => !repos.settings().category_printers(OUTLET)?.is_empty(),
    };
    Ok(KitchenRouting {
        separate_printers,
        per_category: style.as_deref() == Some("category"),
    })
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
        let (printers, routes, routing) = shop
            .db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let printers = repos.settings().list_printers(OUTLET)?;
                let categories = repos.menu().list_categories(OUTLET)?;
                let mapped = repos.settings().category_printers(OUTLET)?;
                let routes = categories
                    .into_iter()
                    .filter(|category| category.is_active)
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
                let routing = kitchen_routing(&repos)?;
                Ok((printers, routes, routing))
            })
            .map_err(|e| words::from_db(&e))?;

        Ok(PrintersView {
            printers: printers.iter().map(row_view).collect(),
            windows,
            routes,
            kitchen_mode: routing.mode_word().to_owned(),
            ticket_style: routing.style_word().to_owned(),
        })
    })
}

/// The row a Windows printer already has here, if any.
fn row_for_windows_printer(existing: &[Printer], windows_name: &str) -> Option<Printer> {
    existing
        .iter()
        .find(|p| p.kind == "spooler" && p.address.as_deref() == Some(windows_name))
        .cloned()
}

/// A fresh row for a Windows printer: named after it, on the paper the bill printer uses.
fn windows_printer_edit(existing: &[Printer], windows_name: &str, role: &str, is_default: bool) -> PrinterEdit {
    let paper = existing
        .iter()
        .find(|p| p.is_default)
        .and_then(|p| u32::try_from(p.paper_mm).ok())
        .unwrap_or(80);
    PrinterEdit {
        id: String::new(),
        name: windows_name.to_owned(),
        kind: "spooler".to_owned(),
        address: windows_name.to_owned(),
        paper_mm: paper,
        is_default,
        role: role.to_owned(),
        engine: "raster".to_owned(),
        is_bold_dark: false,
        can_kick_drawer: false,
    }
}

fn all_printers(app: &App) -> UiResult<Vec<Printer>> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .map_err(|e| words::from_db(&e))
    })
}

/// Bills print on this Windows printer. An empty name means none yet: the stand-in takes the
/// bills again and prints nothing.
pub fn choose_bill_printer_on(app: &App, windows_name: String) -> UiResult<PrintersView> {
    guard::require(app, Permission::SettingsPrinter)?;
    let windows_name = windows_name.trim().to_owned();
    let existing = all_printers(app)?;
    if windows_name.is_empty() {
        return set_default_printer_on(app, crate::state::NO_PRINTER.to_owned());
    }
    match row_for_windows_printer(&existing, &windows_name) {
        Some(row) => {
            // A printer the kitchen already uses can print the bills too.
            if row.role == "kitchen" {
                save_printer_on(
                    app,
                    PrinterEdit {
                        id: row.id.clone(),
                        name: row.name.clone(),
                        kind: row.kind.clone(),
                        address: row.address.clone().unwrap_or_default(),
                        paper_mm: u32::try_from(row.paper_mm).unwrap_or(80),
                        is_default: true,
                        role: "both".to_owned(),
                        engine: row.engine.clone(),
                        is_bold_dark: row.is_bold_dark,
                        can_kick_drawer: row.can_kick_drawer,
                    },
                )
            } else {
                set_default_printer_on(app, row.id)
            }
        }
        None => save_printer_on(app, windows_printer_edit(&existing, &windows_name, "both", true)),
    }
}

/// Whether a cash drawer hangs off the bill printer.
pub fn set_drawer_on(app: &App, on: bool) -> UiResult<PrintersView> {
    guard::require(app, Permission::SettingsPrinter)?;
    let target = crate::flows::default_printer(app)?.id;
    let Some(row) = all_printers(app)?.into_iter().find(|p| p.id == target) else {
        return Err(UiError::new("printer.unknown", "That printer is not set up any more."));
    };
    save_printer_on(
        app,
        PrinterEdit {
            id: row.id.clone(),
            name: row.name.clone(),
            kind: row.kind.clone(),
            address: row.address.clone().unwrap_or_default(),
            paper_mm: u32::try_from(row.paper_mm).unwrap_or(80),
            is_default: row.is_default,
            role: row.role.clone(),
            engine: row.engine.clone(),
            is_bold_dark: row.is_bold_dark,
            can_kick_drawer: on,
        },
    )
}

/// Write one of the two kitchen choices.
fn set_kitchen_choice(app: &App, key: &str, value: &str) -> UiResult<PrintersView> {
    let who = guard::require(app, Permission::SettingsPrinter)?;
    let at = crate::flows::now();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).settings().set(
                    OUTLET,
                    key,
                    &value.to_owned(),
                    at,
                    Some(who.staff_id.as_str()),
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    log_info!("{} set {key} to {value}", who.name);
    printers_on(app)
}

/// `same`: kitchen tickets print where bills do. `other`: categories may name their own printer.
pub fn set_kitchen_mode_on(app: &App, mode: String) -> UiResult<PrintersView> {
    if !["same", "other"].contains(&mode.as_str()) {
        return Err(UiError::new(
            "printer.kitchen_mode",
            "Kitchen tickets print on the bill printer, or on other printers.",
        ));
    }
    set_kitchen_choice(app, KITCHEN_MODE_KEY, &mode)
}

/// `combined`: one ticket per printer. `category`: one ticket per category.
pub fn set_ticket_style_on(app: &App, style: String) -> UiResult<PrintersView> {
    if !["combined", "category"].contains(&style.as_str()) {
        return Err(UiError::new(
            "printer.ticket_style",
            "A kitchen ticket is one for everything, or one per category.",
        ));
    }
    set_kitchen_choice(app, TICKET_STYLE_KEY, &style)
}

/// This category's kitchen tickets go to that Windows printer, which gets a row here if it has
/// none. An empty name sends them back to the bill printer.
pub fn route_category_to_on(
    app: &App,
    category_id: String,
    windows_name: String,
) -> UiResult<PrintersView> {
    guard::require(app, Permission::SettingsPrinter)?;
    let windows_name = windows_name.trim().to_owned();
    if windows_name.is_empty() {
        return route_category_on(app, category_id, String::new());
    }
    let existing = all_printers(app)?;
    let printer_id = match row_for_windows_printer(&existing, &windows_name) {
        Some(row) => row.id,
        None => {
            let saved = save_printer_on(
                app,
                windows_printer_edit(&existing, &windows_name, "kitchen", false),
            )?;
            saved
                .printers
                .iter()
                .find(|p| p.kind == "spooler" && p.address == windows_name)
                .map(|p| p.id.clone())
                .ok_or_else(|| {
                    UiError::new("printer.unknown", "That printer could not be set up.")
                })?
        }
    };
    route_category_on(app, category_id, printer_id)
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
pub fn choose_bill_printer(
    app: tauri::State<'_, App>,
    windows_name: String,
) -> UiResult<PrintersView> {
    choose_bill_printer_on(&app, windows_name)
}

#[tauri::command]
pub fn set_drawer(app: tauri::State<'_, App>, on: bool) -> UiResult<PrintersView> {
    set_drawer_on(&app, on)
}

#[tauri::command]
pub fn set_kitchen_mode(app: tauri::State<'_, App>, mode: String) -> UiResult<PrintersView> {
    set_kitchen_mode_on(&app, mode)
}

#[tauri::command]
pub fn set_ticket_style(app: tauri::State<'_, App>, style: String) -> UiResult<PrintersView> {
    set_ticket_style_on(&app, style)
}

#[tauri::command]
pub fn route_category_to(
    app: tauri::State<'_, App>,
    category_id: String,
    windows_name: String,
) -> UiResult<PrintersView> {
    route_category_to_on(&app, category_id, windows_name)
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
