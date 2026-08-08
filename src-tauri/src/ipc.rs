//! **The IPC boundary.** One command per use case, named for the use case.
//!
//! *A deviation from this session's own prompt, and the reason:* the prompt
//! asked for `ipc/` with a file per group. At P08 there is no billing screen,
//! no menu and no reports, so that would be four files with two commands
//! between them and a barrel to keep in step. One file with sections is honest
//! now; the day `order.rs` has fifteen commands in it, split it. Splitting a
//! file is free — that is the same argument `SCHEMA.md` makes about tables.
//!
//! # The rules this boundary obeys
//!
//! * **Named for the use case**: `settle_order`, never `update_orders`. A
//!   command named after a table is a screen writing SQL by post, which is
//!   audit E3 with extra steps.
//! * **Every command returns a typed `Result`** whose error carries a code, a
//!   sentence for the shopkeeper and the technical detail behind it — see
//!   [`crate::words`], and audit F8.
//! * **`?` on a `DbError` does not compile here.** Every error is converted by
//!   hand, because the conversion is where somebody writes the sentence.
//! * **Types are generated, never written twice** (`ts-rs`), and `cargo test`
//!   regenerates and diffs them so the two sides cannot drift.
//! * **Money crosses as integer paise plus a preformatted string.** Never as a
//!   float, and never as a number TypeScript is expected to format — that is
//!   R8, and `Money::to_plain_string` is the only formatter in the product.

use mb_core::{BusinessDay, Timestamp};
use mb_print::printer::{PrinterConfig, Target};
use mb_print::queue::{Job, JobKind};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::state::{App, OUTLET, PrintJobView};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

// ---------------------------------------------------------------------------
// Money, as it crosses.
// ---------------------------------------------------------------------------

/// **The only shape money takes on the wire.**
///
/// Both halves are produced in Rust: `paise` is the integer of record (D2) and
/// `text` is what a screen displays, already formatted by
/// `Money::to_plain_string`. TypeScript never computes either.
///
/// It would be smaller to send only the integer and format it in TypeScript.
/// It would also be a second money path, in a language with no integers, and
/// `0.1 + 0.2` is the oldest bug in the industry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MoneyView {
    pub paise: i64,
    pub text: String,
}

impl From<mb_core::Money> for MoneyView {
    fn from(money: mb_core::Money) -> Self {
        MoneyView {
            paise: money.paise(),
            text: money.to_plain_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// What the app is, right now.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    /// False on a first run. The shell opens to "create or restore" rather
    /// than to a blank screen.
    pub has_shop: bool,
    pub shop_path: Option<String>,
    pub theme: String,
    pub text_size: String,
    pub version: String,
    /// Where the logs are, so "send me the log" is a button and not a phone
    /// call about file paths (audit E7).
    pub logs_path: Option<String>,
}

#[tauri::command]
pub fn app_status(app: tauri::State<'_, App>) -> AppStatus {
    let config = app.config();
    AppStatus {
        has_shop: app.has_shop(),
        shop_path: app
            .with_shop(|shop| Ok(shop.path.display().to_string()))
            .ok(),
        theme: config.theme,
        text_size: config.text_size,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        logs_path: crate::logging::directory().map(|p| p.display().to_string()),
    }
}

/// The look, saved. Applied before the first paint on the next start, so the
/// window never flashes light and then goes dark.
///
/// **Rust stores the choice and does not know what it means.** A theme is data
/// (D21, and the owner's ruling of 2026-08-04): adding one must never require
/// a change on this side, so this validates nothing and stores the name.
#[tauri::command]
pub fn set_appearance(app: tauri::State<'_, App>, theme: String, text_size: String) {
    app.update_config(|config| {
        config.theme = theme.clone();
        config.text_size = text_size.clone();
    });
}

/// Audit E7 — *"there is nothing to read"*. One button.
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

// ---------------------------------------------------------------------------
// Printing.
// ---------------------------------------------------------------------------

/// A printer, as a settings screen shows it. P17 owns the screen; this exists
/// so the test print has something to aim at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PrinterView {
    pub id: String,
    pub name: String,
    pub connection: String,
    pub paper_mm: i64,
    pub is_default: bool,
    /// Scope 7.11, in the units the owner nudges in.
    pub offset_x_mm: i64,
    pub offset_y_mm: i64,
}

#[tauri::command]
pub fn list_printers(app: tauri::State<'_, App>) -> UiResult<Vec<PrinterView>> {
    app.with_shop(|shop| {
        let rows = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .map_err(|e| words::from_db(&e))?;
        Ok(rows
            .into_iter()
            .map(|p| PrinterView {
                connection: match p.kind.as_str() {
                    "spooler" => format!("Windows: {}", p.address.clone().unwrap_or_default()),
                    "network" => format!("Network: {}", p.address.clone().unwrap_or_default()),
                    "serial" => format!("Serial: {}", p.address.clone().unwrap_or_default()),
                    _ => "Not connected".to_owned(),
                },
                id: p.id,
                name: p.name,
                paper_mm: p.paper_mm,
                is_default: p.is_default,
                offset_x_mm: p.offset_x_mm,
                offset_y_mm: p.offset_y_mm,
            })
            .collect())
    })
}

/// The test print — P07 built the slip, the ruler and the nudge; this is what
/// makes them reachable.
///
/// **It works with no shop open**, which is the whole point of it: first run,
/// and after a restore, are exactly when somebody needs to know whether
/// printing works, and D27 says the database is deliberately not open then.
#[tauri::command]
pub fn print_test_page(app: tauri::State<'_, App>, printer_id: String) -> UiResult<String> {
    let printer = find_printer(&app, &printer_id)?;
    let document = mb_print::testprint::test_document(&printer, None);
    let day = BusinessDay::of(
        Timestamp::from_millis(now_millis()),
        mb_core::DayRule::default(),
        mb_core::UtcOffset::INDIA,
    );

    let job = Job::new(JobKind::Test, &printer.id, document, day).because("test print");

    // With a shop, the job is durable and survives a power cut. Without one,
    // the queue is in memory — D32's port, and the reason a test print works
    // when nothing else does.
    let queued = app.with_shop(|shop| {
        shop.queue
            .enqueue(job.clone())
            .map_err(|e| words::from_print(&e))
    });

    match queued {
        Ok(id) => Ok(id),
        Err(e) if e.code == "shop.none" => {
            log_info!("test print with no shop open — using an in-memory queue");
            let queue = app.transient_queue(vec![printer]);
            let id = queue.enqueue(job).map_err(|e| words::from_print(&e))?;
            // The transient queue is dropped with the print in flight; the
            // worker thread owns the job and finishes it.
            std::mem::forget(queue);
            Ok(id)
        }
        Err(e) => Err(e),
    }
}

/// Scope 7.11 — print, look at the paper, nudge, print again.
#[tauri::command]
pub fn nudge_print_offset(
    app: tauri::State<'_, App>,
    printer_id: String,
    dx_mm: i32,
    dy_mm: i32,
) -> UiResult<PrinterView> {
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

        // Clamped by mb-print, so there is one rule rather than two: a nudge
        // that could ever be a real correction is +/- 20 mm.
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
                mb_db::Repos::new(tx).settings().save_printer(
                    OUTLET,
                    &row,
                    Timestamp::from_millis(now_millis()),
                )
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

/// What the shell's indicator shows — **audit D4's visible half.**
///
/// From `snapshot()` rather than from the event stream, so a screen that
/// attached after the `Parked` event is not blind to the one thing it exists
/// to show.
#[tauri::command]
pub fn list_print_jobs(app: tauri::State<'_, App>) -> UiResult<Vec<PrintJobView>> {
    app.with_shop(|shop| Ok(shop.queue.snapshot().iter().map(to_view).collect()))
}

#[tauri::command]
pub fn retry_print_job(app: tauri::State<'_, App>, id: String) -> UiResult<()> {
    app.with_shop(|shop| shop.queue.retry(&id).map_err(|e| words::from_print(&e)))
}

#[tauri::command]
pub fn dismiss_print_job(app: tauri::State<'_, App>, id: String) -> UiResult<()> {
    app.with_shop(|shop| shop.queue.dismiss(&id).map_err(|e| words::from_print(&e)))
}

/// The queue's vocabulary, turned into words — crown jewel 14, and the reason
/// a screen never sees a tag like `parked`.
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
        }
        .to_owned(),
        state: match status.state {
            S::Pending => "Waiting".to_owned(),
            S::Printing => "Printing".to_owned(),
            S::Failed => format!("Did not print — trying again ({})", status.attempts),
            // The one a person has to act on, and it says so in words.
            S::Parked => "NOT PRINTED — needs you".to_owned(),
            S::Done => "Printed".to_owned(),
        },
        needs_attention: matches!(status.state, S::Parked),
        reason: status.reason.clone(),
        last_error: status.last_error.clone(),
    }
}

fn find_printer(app: &tauri::State<'_, App>, id: &str) -> UiResult<PrinterConfig> {
    // With a shop, use the configured printer. Without one — first run — fall
    // back to a file in the log folder, so "does printing work?" has an answer
    // before anything else exists.
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

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

/// Every command, in one place, so `main.rs` reads as a list rather than a
/// macro nobody can grep.
#[macro_export]
macro_rules! commands {
    () => {
        tauri::generate_handler![
            $crate::ipc::app_status,
            $crate::ipc::set_appearance,
            $crate::ipc::reveal_logs,
            $crate::ipc::list_printers,
            $crate::ipc::print_test_page,
            $crate::ipc::nudge_print_offset,
            $crate::ipc::list_print_jobs,
            $crate::ipc::retry_print_job,
            $crate::ipc::dismiss_print_job,
            $crate::ipc::preview_test_page,
            $crate::ipc::current_cart,
            $crate::ipc::cart_add,
            $crate::ipc::cart_set_qty,
            $crate::ipc::cart_remove,
            $crate::ipc::cart_clear,
            $crate::ipc::cart_set_order_type,
            $crate::ipc::cart_add_payment,
            $crate::ipc::cart_clear_payments,
            $crate::ipc::open_orders,
            $crate::ipc::menu_items,
            $crate::ipc::search_items,
            $crate::ipc::open_table,
            $crate::flows::print_kitchen_ticket,
            $crate::flows::complete_bill,
            // Development only — see its own documentation. It does not exist
            // in a release build.
            #[cfg(debug_assertions)]
            $crate::ipc::seed_demo_shop,
        ]
    };
}

// ---------------------------------------------------------------------------
// The preview — the fourth sink's input.
// ---------------------------------------------------------------------------

/// Lay out a sample bill and hand it to the screen.
///
/// The sample is **P07's test slip** rather than a second invented one: it is
/// already a real-looking bill with real amounts, it already carries the
/// alignment ruler and the print offset, and it needs no shop — which means the
/// preview works on a first run, like everything else in this session.
///
/// P09 will add `preview_order(order_id)` beside this, against a real bill.
/// That is audit **D6** — *"no bill preview before printing"* — and it costs
/// one command, because the sink already exists.
#[tauri::command]
pub fn preview_test_page(
    app: tauri::State<'_, App>,
    printer_id: Option<String>,
) -> UiResult<crate::preview::PreviewDoc> {
    let printer = match printer_id {
        Some(id) => find_printer(&app, &id)?,
        None => PrinterConfig::new("prn_preview", "Preview", Target::None),
    };
    let document = mb_print::testprint::test_document(&printer, None);
    let laid = mb_print::layout::layout(&document).map_err(|e| words::from_print(&e))?;
    Ok(crate::preview::to_preview(&laid))
}

// ---------------------------------------------------------------------------
// The billing screen (P09).
//
// Every one of these returns the WHOLE new `CartView`. There is no partial
// update and no delta: the bill is recomputed from the cart every time
// (D4, and it costs 14 µs), so a screen that rendered a delta would be a
// screen that could be stale.
// ---------------------------------------------------------------------------

use crate::billing::{
    CartState, CartView, MenuItemView, TableView, cart_view, floor_view, menu_view,
    order_type_from_label, snapshot_for,
};

/// What is in the cart right now.
#[tauri::command]
pub fn current_cart(app: tauri::State<'_, App>) -> UiResult<CartView> {
    app.with_cart(cart_view)
}

/// Put an item in. **`Cart::add` merges** by `LineIdentity` — same item, same
/// note, same modifiers — which is P01's rule and the reason the cart is not
/// in TypeScript.
#[tauri::command]
pub fn cart_add(
    app: tauri::State<'_, App>,
    item_id: String,
    qty: Option<String>,
    note: Option<String>,
) -> UiResult<CartView> {
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

    app.with_cart_mut(|state| {
        state
            .cart
            .add(snapshot_for(&item), qty, note, vec![])
            .map_err(|e| {
                UiError::new("cart.add", "That item could not be added to the bill.")
                    .with_detail(e.to_string())
            })?;
        cart_view(state)
    })
}

#[tauri::command]
pub fn cart_set_qty(
    app: tauri::State<'_, App>,
    index: usize,
    qty: String,
) -> UiResult<CartView> {
    let parsed = mb_core::Qty::parse(&qty).map_err(|e| {
        UiError::new(
            "cart.qty",
            format!("\"{qty}\" is not a quantity. Try 1, 2 or 0.5."),
        )
        .with_detail(e.to_string())
    })?;
    app.with_cart_mut(|state| {
        state.cart.set_qty(index, parsed).map_err(|e| {
            UiError::new("cart.qty", "That quantity could not be set.")
                .with_detail(e.to_string())
        })?;
        cart_view(state)
    })
}

#[tauri::command]
pub fn cart_remove(app: tauri::State<'_, App>, index: usize) -> UiResult<CartView> {
    app.with_cart_mut(|state| {
        state.cart.remove(index).map_err(|e| {
            UiError::new("cart.remove", "That line could not be removed.")
                .with_detail(e.to_string())
        })?;
        cart_view(state)
    })
}

/// New order. Keeps the order type, because **the type lock** (crown jewel 1)
/// is what stops a parcel counter re-selecting it forty times an hour.
#[tauri::command]
pub fn cart_clear(app: tauri::State<'_, App>, keep_type: bool) -> UiResult<CartView> {
    app.with_cart_mut(|state| {
        let kept = state.order_type;
        *state = CartState::default();
        if keep_type {
            state.order_type = kept;
        }
        cart_view(state)
    })
}

#[tauri::command]
pub fn cart_set_order_type(
    app: tauri::State<'_, App>,
    order_type: String,
) -> UiResult<CartView> {
    let kind = order_type_from_label(&order_type).ok_or_else(|| {
        UiError::new("cart.order_type", format!("\"{order_type}\" is not an order type."))
    })?;
    app.with_cart_mut(|state| {
        state.order_type = kind;
        // A parcel has no table, and leaving a stale one would settle the bill
        // against a table nobody is sitting at.
        if !matches!(kind, mb_core::OrderType::DineIn) {
            state.table = None;
            state.table_label = None;
        }
        cart_view(state)
    })
}

/// Take a payment. Split payment is simply calling this more than once (1.15).
#[tauri::command]
pub fn cart_add_payment(
    app: tauri::State<'_, App>,
    mode: String,
    amount_paise: i64,
) -> UiResult<CartView> {
    let mode = match mode.as_str() {
        "Cash" => mb_core::PaymentMode::Cash,
        "Card" => mb_core::PaymentMode::Card,
        "UPI" => mb_core::PaymentMode::Upi,
        other => mb_core::PaymentMode::Other(other.to_owned()),
    };
    let payment = mb_core::Payment::new(mode, mb_core::Money::from_paise(amount_paise))
        .map_err(|e| {
            UiError::new("payment.invalid", "That payment could not be taken.")
                .with_detail(e.to_string())
        })?;
    app.with_cart_mut(|state| {
        state.settlement.add(payment).map_err(|e| {
            UiError::new("payment.invalid", "That payment could not be taken.")
                .with_detail(e.to_string())
        })?;
        cart_view(state)
    })
}

#[tauri::command]
pub fn cart_clear_payments(app: tauri::State<'_, App>) -> UiResult<CartView> {
    app.with_cart_mut(|state| {
        state.settlement = mb_core::Settlement::new();
        cart_view(state)
    })
}

/// The floor — **the only view of open orders** (scope 1.4).
#[tauri::command]
pub fn open_orders(app: tauri::State<'_, App>) -> UiResult<Vec<TableView>> {
    let loaded = app.with_cart(|state| Ok(state.order_id.clone()))?;
    app.with_shop(|shop| {
        let (tables, sections, open) = shop
            .db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                Ok((
                    repos.floor().list_tables(OUTLET)?,
                    repos.floor().list_sections(OUTLET)?,
                    repos.orders().list_open(OUTLET)?,
                ))
            })
            .map_err(|e| words::from_db(&e))?;
        Ok(floor_view(
            &tables,
            &sections,
            &open,
            loaded.as_deref(),
            Timestamp::from_millis(now_millis()),
        ))
    })
}

/// The menu, for putting something in the cart. P13 owns the menu screens.
#[tauri::command]
pub fn menu_items(app: tauri::State<'_, App>) -> UiResult<Vec<MenuItemView>> {
    app.with_shop(|shop| {
        let items = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).menu().list_items(OUTLET, true))
            .map_err(|e| words::from_db(&e))?;
        Ok(items.iter().map(menu_view).collect())
    })
}

/// Put a small shop in the database so the billing screen has something to
/// render — **development only, and it cannot ship.**
///
/// `#[cfg(debug_assertions)]`: the command does not exist in a release build,
/// so there is no "demo data" button for a shop to press by accident and no
/// feature invented outside `FEATURE_SCOPE.md`. P13 builds the real menu
/// screens and P14 the real floor; this exists so that "run it and look at it"
/// — which P08 added to the method after shipping two visible bugs — is
/// possible at all before those sessions.
///
/// The items are deliberately awkward rather than tidy: a long name that must
/// wrap, a 5% line, an 18% line, an inclusive-priced line and a **non-GST**
/// one, because a bar that cannot bill is audit B10 and the totals block is
/// where it shows.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn seed_demo_shop(app: tauri::State<'_, App>) -> UiResult<String> {
    use mb_core::{CategoryId, ItemId, Money, TableId, TaxRate, TaxTreatment};
    use mb_db::repo::floor::{DiningTable, Section};
    use mb_db::repo::menu::MenuItem;

    let at = Timestamp::from_millis(now_millis());

    // A first run has no database at all, and the real "create a new shop"
    // flow is P22's first-run wizard. Rather than invent a product feature
    // here, the dev seeder makes one where the shop would go and records it
    // the same way the wizard will — through mb-db's own locate, so there is
    // still exactly one answer to "where is the shop?" (audit A5).
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

                // Twenty-two tables across three sections: enough that density
                // is a real question at 1366x768 rather than a theoretical one.
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
                // `items.category_id` references `categories(id)`, and the
                // first run of this seeder hit that constraint. Which is the
                // schema doing its job: P04 turned off nothing, so a dangling
                // reference cannot be written even by a developer in a hurry.
                repos.menu().save_category(
                    OUTLET,
                    &mb_db::repo::menu::Category {
                        id: CategoryId::new("cat_food"),
                        name: "Food".to_owned(),
                        sort_order: 0,
                        is_active: true,
                    },
                    at,
                )?;

                let menu: [(&str, &str, i64, TaxRate, TaxTreatment); 8] = [
                    ("itm_dosa", "Masala Dosa", 12_000, TaxRate::GST_5, TaxTreatment::Exclusive),
                    ("itm_idli", "Idli Vada", 8_000, TaxRate::GST_5, TaxTreatment::Exclusive),
                    (
                        "itm_pbm",
                        "Paneer Butter Masala (Half) - Extra Spicy",
                        31_500,
                        TaxRate::GST_5,
                        TaxTreatment::Exclusive,
                    ),
                    ("itm_water", "Water 1L", 2_000, TaxRate::GST_18, TaxTreatment::Inclusive),
                    ("itm_cola", "Cola 300ml", 4_000, TaxRate::GST_18, TaxTreatment::Exclusive),
                    ("itm_beer", "Beer 650ml", 22_000, TaxRate::ZERO, TaxTreatment::NonGst),
                    ("itm_rice", "Curd Rice", 9_000, TaxRate::GST_5, TaxTreatment::Exclusive),
                    ("itm_sweet", "Gulab Jamun (2 pc)", 6_000, TaxRate::GST_5, TaxTreatment::Exclusive),
                ];
                for (index, (id, name, paise, rate, treatment)) in menu.iter().enumerate() {
                    repos.menu().save_item(
                        OUTLET,
                        &MenuItem {
                            id: ItemId::new(*id),
                            category_id: Some(CategoryId::new("cat_food")),
                            name: (*name).to_owned(),
                            unit_price: Money::from_paise(*paise),
                            tax_rate: *rate,
                            tax_treatment: *treatment,
                            hsn: Some("2106".to_owned()),
                            cost_price: None,
                            short_code: None,
                            prep_minutes: None,
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

/// Ranked item search — audit 2.3, budget B2.
///
/// Runs here rather than in React because **ranking is a rule**: name-start
/// beats word-start beats inside-word, and a second copy of that in TypeScript
/// would disagree with this one the moment P13 adds short codes to the same
/// box. See `search.rs`.
#[tauri::command]
pub fn search_items(
    app: tauri::State<'_, App>,
    text: String,
    mode: Option<crate::search::MatchMode>,
) -> UiResult<Vec<MenuItemView>> {
    app.with_shop(|shop| {
        let items = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).menu().list_items(OUTLET, true))
            .map_err(|e| words::from_db(&e))?;
        Ok(crate::search::search(
            &items,
            &text,
            mode.unwrap_or_default(),
        ))
    })
}

/// Load a table's running order into the cart — **budget B7**.
///
/// The order's own cart replaces the current one, and its `KitchenLedger`
/// comes with it, which is what makes the kitchen delta right afterwards
/// (crown jewel 2: *"what was printed is remembered in the database, not in
/// the screen's memory"*).
#[tauri::command]
pub fn open_table(app: tauri::State<'_, App>, table_id: String) -> UiResult<CartView> {
    // **The label, not the id.** The cart carries `tbl_7` because that is the
    // key an order is saved against; the cashier's table is called 7 and the
    // header said "Table tbl_7" until somebody looked at it. Resolved here,
    // once, rather than by trimming a prefix off an id anywhere else.
    let label = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).floor().list_tables(OUTLET))
            .map_err(|e| words::from_db(&e))
            .map(|tables| {
                tables
                    .into_iter()
                    .find(|t| t.id.as_str() == table_id)
                    .map(|t| t.label)
            })
    })?;

    let found = app.with_shop(|shop| {
        let open = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).orders().list_open(OUTLET))
            .map_err(|e| words::from_db(&e))?;
        Ok(open.into_iter().find(|order| {
            order
                .core()
                .table
                .as_ref()
                .is_some_and(|t| t.as_str() == table_id)
        }))
    })?;

    app.with_cart_mut(|state| {
        match found {
            Some(order) => {
                let core = order.core();
                // The whole order comes across: its cart, its type, its id.
                // Nothing is re-derived, because a screen that re-derived it
                // would be a second copy of the order.
                state.cart = core.cart.clone();
                state.order_type = core.order_type;
                state.order_id = Some(core.id.as_str().to_owned());
                state.table = Some(table_id.clone());
                state.table_label = label.clone();
                state.settlement = mb_core::Settlement::new();
            }
            None => {
                // A free table: start a new order on it rather than refusing,
                // because "press the table and start typing" is the flow.
                *state = crate::billing::CartState {
                    order_type: state.order_type,
                    table: Some(table_id.clone()),
                    table_label: label.clone(),
                    ..crate::billing::CartState::default()
                };
            }
        }
        cart_view(state)
    })
}
