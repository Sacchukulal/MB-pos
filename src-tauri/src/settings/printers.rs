//! **The printer setup** — audit Part 3's "Printer Settings", every row.
//!
//! A printer is a **record**, not a scalar, which is why it is here and not in
//! the catalogue: a shop has none, one, or six of them, and keys like
//! `printer.3.paper_mm` would be audit E6 wearing a different hat.
//!
//! # The one paper size
//!
//! v1 had two, in two places, and they could disagree. There is one here, on
//! the printer, because paper is a property of a printer and not of a shop: a
//! counter with an 80 mm bill printer and a 58 mm kitchen printer is an
//! ordinary shop, and one shop-wide setting cannot describe it.
//!
//! # The test print is a BILL
//!
//! P07's slip proves the wire works. It does not answer *"is my bill centred,
//! is the logo the right size, does the total fit?"* — which is what somebody
//! standing at the printer is actually asking. So the test print is
//! `settings::sample`'s bill, the same document the preview shows, on this
//! shop's real settings.

use mb_auth::Permission;
use mb_core::Timestamp;
use mb_db::repo::settings::Printer;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::guard;
use crate::state::{App, OUTLET, printer_config_for};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

// ---------------------------------------------------------------------------
// What the screen sees.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PrintersView {
    pub printers: Vec<PrinterRowView>,
    /// **The real printers Windows knows about**, for the "which one?" list.
    /// Empty is a state, not a failure: a counter with no printer installed
    /// still has to be able to open this screen.
    pub windows: Vec<String>,
    /// Scope 3.1 — which printer each category's kitchen tickets go to.
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
    /// In words: "Windows: EPSON TM-T82", "Not connected yet".
    pub connection: String,
    pub paper_mm: u32,
    pub is_default: bool,
    /// `bill`, `kitchen` or `both`.
    pub role: String,
    /// `raster` or `text`.
    pub engine: String,
    pub is_bold_dark: bool,
    pub can_kick_drawer: bool,
    /// Scope 7.11, in the millimetres the owner nudges in.
    pub offset_x_mm: i32,
    pub offset_y_mm: i32,
    /// True for the stand-in row a shop has before it sets a printer up. It
    /// can be edited into a real one; it cannot be deleted, because the queue
    /// and the spool both hold a foreign key to it.
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

// ---------------------------------------------------------------------------
// Reading.
// ---------------------------------------------------------------------------

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
        // D58: counts and small numbers cross as u32/i32, never i64.
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

    // **Asked of Windows, not remembered.** A printer installed five minutes
    // ago has to appear, and one that was uninstalled has to stop appearing —
    // v1's "Refresh List" button existed because its list was a cache.
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

// ---------------------------------------------------------------------------
// Writing.
// ---------------------------------------------------------------------------

/// The three paper widths this product supports, and the reason a fourth is a
/// decision rather than a number: `mb_print::paper` knows the column counts.
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
    // **A network printer with no address prints nowhere and says it is fine.**
    if edit.kind == "network" && !edit.address.contains(':') {
        return Err(UiError::new(
            "printer.address",
            "A network printer needs an address and a port, like \
             192.168.1.50:9100.",
        ));
    }

    let at = crate::flows::now();
    let id = if edit.id.trim().is_empty() {
        format!("prn_{}", at.millis())
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
        // **The offset is NOT edited here.** It is nudged from the test print
        // (7.11), where the owner can see what they are correcting, and typing
        // a number into a box you cannot check is how it gets worse.
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
                // Keep the offset somebody has already nudged.
                let row = Printer {
                    offset_x_mm: before.map_or(0, |p| p.offset_x_mm),
                    offset_y_mm: before.map_or(0, |p| p.offset_y_mm),
                    ..row.clone()
                };
                repos.settings().save_printer(OUTLET, &row, at)?;

                // **One default, and it is the shop's answer to "where does a
                // bill go?".** Two would make that question ambiguous and
                // `flows::default_printer` would answer it by row order.
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

    // **The queue runs a thread per printer it was STARTED with**, so a
    // printer saved now has nowhere to send anything until the queue is built
    // again. This used to say so in the log and leave it there — and the owner
    // added their TVSE, pressed **Print a sample bill**, and was told "there is
    // no printer prn_…". Setting a printer up is the one moment somebody is
    // certain to test it, so it has to work then. P30.6.
    app.rebuild_queue();
    log_info!("{} saved the printer \"{}\"", who.name, row.name);

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
                // **A printer with paper in the spool cannot go.** `print_jobs`
                // holds a foreign key to it, and the refusal says how many
                // rather than showing a constraint name (audit F8) — the same
                // sentence shape as a table with bills against it (P14).
                let waiting = repos.print_jobs().count_for_printer(&id)?;
                if waiting > 0 {
                    return Err(mb_db::DbError::invariant(format!(
                        "there {} still {waiting} print job{} against this \
                         printer",
                        if waiting == 1 { "is" } else { "are" },
                        if waiting == 1 { "" } else { "s" }
                    )));
                }
                repos.settings().delete_printer(OUTLET, &id, crate::flows::now())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // Same as saving one: the queue is the printer list made real. P30.6.
    app.rebuild_queue();
    log_info!("{} removed a printer", who.name);
    printers_on(app)
}

/// Scope 3.1 — which printer this category's kitchen tickets go to.
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

/// **The test print, and it is a whole bill.**
///
/// P07's slip answers "does the wire work?". This answers the question
/// somebody standing at the printer is actually asking — is my bill centred,
/// is the logo the right size, does the total fit? — because it is the same
/// document the preview shows and the same one a customer will get.
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

    let (bill, order) = super::sample::sample_order().map_err(|e| words::from_print(&e))?;
    let store = config.store.to_print_store();
    let document = mb_print::template::bill_document(
        printer.paper,
        &mb_print::template::BillContext {
            bill: &bill,
            order: &order,
            store: &store,
            settings: &config.receipt,
            customer: None,
            cashier: Some(who.name.as_str()),
            copy: mb_print::template::Copy::Original,
            einvoice: mb_print::template::EInvoice::default(),
            logo: None,
        },
    )
    .map_err(|e| words::from_print(&e))?;

    let at = crate::flows::now();
    app.with_shop(|shop| {
        shop.queue
            .enqueue(
                mb_print::queue::Job::new(
                    mb_print::queue::JobKind::Test,
                    &printer.id,
                    document,
                    crate::flows::today(at),
                )
                .because("sample bill".to_owned()),
            )
            .map_err(|e| words::from_print(&e))
    })
}

/// Scope 7.11 — print, look at the paper, nudge, print again.
pub fn nudge_on(app: &App, printer_id: String, dx_mm: i32, dy_mm: i32) -> UiResult<PrintersView> {
    // The existing `ipc::nudge_print_offset` does the arithmetic and the
    // clamping; this is the settings screen's door to it, so there is one rule
    // and not two.
    crate::ipc::nudge_offset_on(app, printer_id, dx_mm, dy_mm)?;
    printers_on(app)
}

/// A timestamp, for the generated printer id.
const _: Option<Timestamp> = None;

// ---------------------------------------------------------------------------
// The seats.
// ---------------------------------------------------------------------------

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
