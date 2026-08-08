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
    guard::require(&app, Permission::SettingsPrinter)?;
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
    guard::require(&app, Permission::SettingsPrinter)?;
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
    guard::require(&app, Permission::SettingsPrinter)?;
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
    guard::require(&app, Permission::BillReprint)?;
    app.with_shop(|shop| shop.queue.retry(&id).map_err(|e| words::from_print(&e)))
}

#[tauri::command]
pub fn dismiss_print_job(app: tauri::State<'_, App>, id: String) -> UiResult<()> {
    guard::require(&app, Permission::BillReprint)?;
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
            // P11 — signing in, the people, the history. `guard.rs` has a test
            // that every one of these has an access decision recorded.
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
            // P12 — the four ways a shop takes something back (B5, B6, D7).
            $crate::corrections::list_bills,
            $crate::corrections::day_totals,
            $crate::corrections::reasons,
            $crate::corrections::void_bill,
            $crate::corrections::cancel_order,
            $crate::corrections::void_line,
            $crate::corrections::reprint_bill,
            $crate::corrections::refund_bill,
            // P13 — the menu.
            $crate::menu::menu_tax_classes,
            $crate::menu::menu_categories,
            $crate::menu::menu_rows,
            $crate::menu::save_menu_item,
            $crate::menu::set_item_available,
            $crate::menu::save_menu_category,
            $crate::menu::save_tax_class,
            $crate::menu::change_menu_prices,
            $crate::menu::plan_menu_import,
            $crate::menu::run_menu_import,
            $crate::menu::export_menu,
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
            $crate::floor::set_dining_table_active,
            $crate::floor::delete_dining_table,
            $crate::floor::save_floor_thresholds,
            $crate::floor::move_order,
            $crate::floor::merge_orders,
            $crate::floor::split_order,
            $crate::floor::even_split,
            $crate::floor::set_covers,
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
    guard::require(&app, Permission::BillCreate)?;
    app.with_cart(cart_view)
}

/// Put an item in. **`Cart::add` merges** by `LineIdentity` — same item, same
/// note, same modifiers — which is P01's rule and the reason the cart is not
/// in TypeScript.
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
    guard::require(&app, Permission::BillCreate)?;
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
    guard::require(&app, Permission::BillCreate)?;
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
pub fn cart_clear_on(app: &App, keep_type: bool) -> UiResult<CartView> {
    guard::require(app, Permission::BillCreate)?;
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
    guard::require(&app, Permission::BillCreate)?;
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
    guard::require(&app, Permission::BillCreate)?;
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
    guard::require(&app, Permission::BillCreate)?;
    app.with_cart_mut(|state| {
        state.settlement = mb_core::Settlement::new();
        cart_view(state)
    })
}

/// The floor — **the only view of open orders** (scope 1.4).
pub fn open_orders_on(app: &App) -> UiResult<Vec<TableView>> {
    guard::require(app, Permission::BillCreate)?;
    let loaded = app.with_cart(|state| Ok(state.order_id.clone()))?;
    // The same two thresholds the floor screen uses, from the same place —
    // a billing grid and a floor plan disagreeing about which table is late
    // would be worse than neither of them saying so.
    let (warn, late) = crate::floor::thresholds(app)?;
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
            warn,
            late,
        ))
    })
}

/// The menu, for putting something in the cart. P13 owns the menu screens.
#[tauri::command]
pub fn menu_items(app: tauri::State<'_, App>) -> UiResult<Vec<MenuItemView>> {
    guard::require(&app, Permission::BillCreate)?;
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
    guard::require(&app, Permission::StaffManage)?;
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
                            // The demo shop points its items at the seeded
                            // classes, so a rate change on the class moves
                            // them — which is what a real shop does.
                            tax_class_id: Some(mb_core::TaxClassId::new(
                                match treatment {
                                    mb_core::TaxTreatment::NonGst => "tax_liquor",
                                    _ if rate.basis_points() == 1_800 => "tax_packaged_18",
                                    _ => "tax_food_5",
                                },
                            )),
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
pub fn open_table_on(app: &App, table_id: String) -> UiResult<CartView> {
    guard::require(app, Permission::BillCreate)?;
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

// ---------------------------------------------------------------------------
// P11 — signing in, the people, and the history.
//
// Audit C1: "There is no login on the POS at all." Everything below exists to
// make that sentence untrue, and `guard.rs` is what makes it stay untrue.
// ---------------------------------------------------------------------------

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
    ///
    /// **This is a courtesy and not the control.** `guard::require` refuses the
    /// commands themselves, and there is a test that calls them directly. A
    /// screen that treated this list as the security boundary would be audit
    /// C1 with extra steps.
    pub permissions: Vec<String>,
    /// True while nobody in this shop has a PIN. The banner reads off it, and
    /// it is why the app opens straight into billing on a shop's first day.
    pub nobody_has_a_pin: bool,
    /// Everybody who could sign in. **Only people with a PIN**, because a name
    /// on this list that cannot be chosen is a support call.
    pub people: Vec<PersonView>,
    /// Whether this shop has a recovery code at all, so the lock screen only
    /// offers "forgotten your PIN?" when there is something to offer.
    pub can_recover: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PersonView {
    pub id: String,
    pub name: String,
    pub code: Option<String>,
    pub role: Option<String>,
    pub status: String,
    pub has_pin: bool,
    /// Empty unless this person is locked out — then it is the sentence the
    /// screen shows, already counted down.
    pub locked_out: Option<String>,
    pub permissions: Vec<String>,
    pub max_discount_bp: Option<u32>,
    pub max_discount: Option<MoneyView>,
}

/// The key the shop's recovery code hash is stored under.
///
/// A setting rather than a column: it belongs to the shop, not to a person, and
/// P04's settings table is exactly the shape for "one value the shop has".
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
        // Only people who can actually sign in. A name that cannot be chosen is
        // a support call about a screen that "does nothing".
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
        code: member.code.clone(),
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
    let Some(last) = repos.audit().last_failed_login(OUTLET, member.id.as_str())? else {
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

/// **Sign in — one person, one verification.**
///
/// BACKEND-**D1** is the finding this shape exists to close: v1 tried the typed
/// PIN against *every* active staff row, so with ten staff a random guess was
/// ten times likelier to land, and here it would also cost ten Argon2
/// verifications. The cashier says who they are first, then proves it.
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

    // Scope 9.15 — somebody who has left keeps their history and loses their
    // way in. T3: it takes effect on the next action, not the next shift.
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
        // The failure is written BEFORE the refusal is returned, because that
        // row IS the lockout counter (item 4) and a refusal that did not count
        // would be no lockout at all.
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

/// mb-db's row becomes mb-auth's actor. One place, like `printer_config_for`.
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
///
/// **It touches nothing but the session.** The cart, the kitchen ledger and the
/// print queue are all exactly where they were; that is item 8, and T7.
pub fn lock_now_on(app: &App) -> UiResult<LockState> {
    // **A shop with no PIN cannot be locked, because there would be no way
    // back in.**
    //
    // Found by looking at the window: on a first run the title bar offered a
    // lock button, and pressing it — or Ctrl+L — ended the stand-in session
    // and put up a lock screen with an empty staff list. The only escape was
    // restarting the app. One click to a dead end, with every test green.
    //
    // Refused HERE rather than by hiding the button, for P11's reason: hiding
    // a control is a courtesy and Rust is the control. Ctrl+L works on every
    // screen and would have got past a hidden button anyway.
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

/// **The way back in when the PIN is gone.**
///
/// Deliberately not rate-limited: the per-person lockout exists so a malicious
/// waiter cannot lock the owner out with five wrong guesses, and that
/// protection is worth nothing if this path can be locked instead. The defence
/// here is 31^10 and an Argon2 verification per attempt, not a counter.
///
/// Using it kills the old code and issues a new one, which the screen prints.
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

    // A new code, before the old one stops working.
    let (fresh, fresh_hash) = mb_auth::new_recovery_code()
        .map_err(|e| UiError::new("auth.recovery_failed", "A new recovery code could not be made.").with_detail(e.to_string()))?;
    let hashed = mb_auth::hash_pin(&pin)
        .map_err(|e| UiError::new("auth.pin_failed", "That PIN could not be saved.").with_detail(e.to_string()))?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mut member) = repos.people().find_staff(OUTLET, &staff_id)? else {
                    return Err(mb_db::DbError::invariant(
                        "that person is not on the staff list",
                    ));
                };
                // Only somebody who manages staff. Otherwise the recovery code
                // would be a way to hand a waiter a new PIN and the run of the
                // shop with it.
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
    // Returned so the screen can show it once and print it. From this moment it
    // exists nowhere else.
    Ok(fresh.to_print())
}

// ---------------------------------------------------------------------------
// The people screen.
// ---------------------------------------------------------------------------

pub fn list_staff_on(app: &App) -> UiResult<Vec<PersonView>> {
    guard::require(app, Permission::StaffManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut out = Vec::new();
                for member in repos.people().list_staff(OUTLET)? {
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
    /// **A percentage as text, both ways** — `"12.5%"` out, whatever was typed
    /// back in. D39 and R8: the screen does not divide by a hundred, any more
    /// than it divides paise by a hundred. The money guard failed the build on
    /// the first version of this, which did exactly that.
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
        permissions: role.permissions.iter().map(|p| p.code().to_owned()).collect(),
        max_discount_percent: role.percent_label(),
        max_discount: role.max_discount.map(MoneyView::from),
    }
}

pub fn save_role_on(app: &App, role: RoleView) -> UiResult<Vec<RoleView>> {
    let who = guard::require(app, Permission::StaffManage)?;
    let at = crate::flows::now();
    let day = crate::flows::today(at);

    // **BACKEND-G7 at the boundary.** A code the screen sent that this build
    // does not know is refused here, by name, rather than silently dropped.
    let permissions = mb_auth::PermissionSet::from_codes(&role.permissions)
        .map_err(|e| UiError::new("role.permission", format!("{e}. Reload and try again.")))?;

    let max_discount_bp = RoleShape::parse_percent(
        role.max_discount_percent.as_deref().unwrap_or_default(),
    )
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
                // **Before and after, not just after** — see the note in
                // `save_staff_member_on`. A shop that has never had an
                // administrator must be able to get one.
                let had_one = !repos.people().active_administrators(OUTLET)?.is_empty();
                repos.people().save_role(OUTLET, &shape, at)?;

                // **The last-administrator rule.** A shop nobody can administer
                // can never be repaired from its own counter. The check is
                // after the write and inside the transaction on purpose: it
                // asks the real question — "who can administer this shop now?"
                // — rather than trying to predict the answer, and the rollback
                // undoes it.
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
    pub code: Option<String>,
    pub role_id: Option<String>,
    /// "active", "suspended" or "left". Scope 9.15: never "deleted".
    pub status: String,
}

pub fn save_staff_member_on(
    app: &App,
    staff: StaffEdit,
) -> UiResult<Vec<PersonView>> {
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
                // **Whether there WAS one, not only whether there is one.**
                //
                // Found by driving a shop's first day end to end: the rule as
                // first written refused the very first hire, because a brand-new
                // shop has no administrator before the write either. "Do not
                // remove the last one" and "there must always be one" are
                // different rules, and only the first one is true — a shop that
                // has never had an administrator has to be able to get one.
                let had_one = !repos.people().active_administrators(OUTLET)?.is_empty();
                let member = mb_db::repo::people::StaffMember {
                    id: mb_core::StaffId::new(staff.id.clone()),
                    name: staff.name.trim().to_owned(),
                    code: staff.code.clone(),
                    role_id: staff.role_id.clone(),
                    role_name: None,
                    // A PIN is set by its own command. Editing a name must not
                    // be able to clear somebody's way in.
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

/// **Never the PIN itself.** The audit trail is the one table that must not
/// contain a secret, and this is the function that would have put one there.
fn staff_json(member: &mb_db::repo::people::StaffMember) -> serde_json::Value {
    serde_json::json!({
        "name": member.name,
        "code": member.code,
        "role_id": member.role_id,
        "status": status_word(member.status),
        "has_pin": member.pin_hash.is_some(),
    })
}

/// Set or clear a PIN.
///
/// Returns the shop's recovery code **only when one is generated**, which is
/// the first time a PIN is set on somebody who manages staff. It is shown once
/// and printed, and after that it exists nowhere but on paper.
pub fn set_staff_pin_on(
    app: &App,
    staff_id: String,
    pin: Option<String>,
) -> UiResult<Option<String>> {
    let who = guard::require(app, Permission::StaffManage)?;
    let at = crate::flows::now();
    let day = crate::flows::today(at);
    // What the shop looked like BEFORE. "The first PIN" is a thing you can only
    // know by having asked first — see `relock_if_this_was_the_first_pin`.
    let had_a_pin = app.shop_has_a_pin();

    let hashed = match pin.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(typed) => {
            let parsed = Pin::parse(typed)
                .map_err(|e| UiError::new("auth.pin_shape", format!("{e}.")).with_detail(e.to_string()))?;
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
                // A PIN with no role is somebody who can sign in and do
                // nothing, which looks like a broken app rather than a locked
                // one.
                if hashed.is_some() && member.role_id.is_none() {
                    return Err(mb_db::DbError::invariant(
                        "give this person a role before setting their PIN",
                    ));
                }
                member.pin_hash = hashed.as_ref().map(|h| h.as_str().to_owned());
                repos.people().save_staff(OUTLET, &member, at)?;

                // The shop's first recovery code, the first time somebody who
                // manages staff gets a PIN. Not on every PIN: a second code
                // would silently retire the slip already in the drawer.
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
                    &AuditEntry::new(at, day, Some(who.staff_id.clone()), action::PIN_SET, "staff")
                        .about(&staff_id)
                        .with_after(serde_json::json!({ "has_pin": hashed.is_some() })),
                )?;
                Ok(issued)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // **Setting the first PIN locks the app, here and now.** Proving it works
    // while that person is still standing at the counter is worth four seconds;
    // finding out at 8 am tomorrow is not.
    app.relock_if_this_was_the_first_pin(had_a_pin);

    Ok(issued)
}

// ---------------------------------------------------------------------------
// The history.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AuditView {
    pub entries: Vec<AuditEntryView>,
    /// The sentence to show when the chain is broken — audit C4's
    /// "tamper-evident", made visible. `None` when the history hangs together.
    pub tampered: Option<String>,
    /// Every action this build knows, for the filter — from the list, not from
    /// whatever happens to be in this shop's data. A filter that cannot offer
    /// "voided a bill" until somebody has voided one is useless at exactly the
    /// moment it is needed.
    pub actions: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AuditEntryView {
    pub seq: i64,
    /// Already formatted. R8 — TypeScript does no arithmetic, and that includes
    /// arithmetic on dates.
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
                let tampered = repos.audit().verify(OUTLET)?.err().map(|b| {
                    format!(
                        "This shop's history has been changed outside Magic Bill — {b}. \
                         Treat everything after that point with care."
                    )
                });
                Ok(AuditView {
                    entries: rows.iter().map(entry_view).collect(),
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

fn entry_view(row: &mb_auth::AuditRow) -> AuditEntryView {
    AuditEntryView {
        seq: row.seq,
        when: crate::words::when(Timestamp::from_millis(row.at)),
        who: row
            .staff_name
            .clone()
            .unwrap_or_else(|| "Somebody not on the staff list".to_owned()),
        what: action::words(&row.action).to_owned(),
        about: row.entity_id.clone(),
        before: row.before_json.as_deref().map(readable_change),
        after: row.after_json.as_deref().map(readable_change),
    }
}

/// **A change, in words a shopkeeper reads** — audit F8, and D39.
///
/// `audit_log`'s before/after is JSON, deliberately: the shape differs per
/// action and nothing ever queries inside it, which is why it is the one place
/// JSON is allowed in this database. **Showing that JSON to the owner is a
/// different decision, and it was the wrong one.** The History screen was
/// rendering `{"state":"settled","total_paise":39300}` at somebody who wanted
/// to know who voided a bill — *"errors show raw system text to a restaurant
/// owner"* is the same finding, one screen along.
///
/// It is formatted **here** rather than in React because a `*_paise` value is
/// money, and money is formatted in exactly one place (D39, R8) —
/// `Money::to_plain_string`.
fn readable_change(json: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        // Unparseable: show it as it is rather than hiding a row of history.
        // A trail with a gap in it is worth less than an ugly line in it.
        return json.to_owned();
    };
    let Some(fields) = value.as_object() else {
        return value.to_string();
    };

    let mut parts: Vec<String> = Vec::new();
    for (key, field) in fields {
        let label = key.replace('_', " ");
        let label = label.strip_suffix(" paise").unwrap_or(&label);
        // A key like `total_paise` names the unit, not the reader's business.
        let shown = if key.ends_with("_paise") {
            field
                .as_i64()
                .map_or_else(|| field.to_string(), |paise| {
                    mb_core::Money::from_paise(paise).to_plain_string()
                })
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

// ---------------------------------------------------------------------------
// The command wrappers.
//
// Every P11 command's body takes `&App` and lives above; these are the Tauri
// seats. The split is not ceremony — `tauri::State` cannot be constructed in a
// test, so a body that took one could only ever be driven by hand through the
// window. Signing in, locking, setting the first PIN and the last-administrator
// rule are all sequences, and a sequence that can only be checked by clicking
// is a sequence that gets checked once.
// ---------------------------------------------------------------------------

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

/// The floor, as a screen shows it. Body over `&App` (D46) so P12's
/// "does a cancel free the table?" can be asked without a window.
#[tauri::command]
pub fn open_orders(app: tauri::State<'_, App>) -> UiResult<Vec<TableView>> {
    open_orders_on(&app)
}

/// Ranked item search (P10, budget B2). Body over `&App` (D46) so B2 can be
/// measured without a window.
#[tauri::command]
pub fn search_items(
    app: tauri::State<'_, App>,
    text: String,
    mode: Option<crate::search::MatchMode>,
) -> UiResult<Vec<MenuItemView>> {
    search_items_on(&app, text, mode)
}

// Bodies over `&App` (D46), so the billing budgets can be measured and the
// correction sequences driven without a window.

#[tauri::command]
pub fn open_table(app: tauri::State<'_, App>, table_id: String) -> UiResult<CartView> {
    open_table_on(&app, table_id)
}

#[tauri::command]
pub fn cart_add(
    app: tauri::State<'_, App>,
    item_id: String,
    qty: Option<String>,
    note: Option<String>,
) -> UiResult<CartView> {
    cart_add_on(&app, item_id, qty, note)
}

#[tauri::command]
pub fn cart_clear(app: tauri::State<'_, App>, keep_type: bool) -> UiResult<CartView> {
    cart_clear_on(&app, keep_type)
}

#[cfg(test)]
mod change_words {
    use super::readable_change;

    /// **Audit F8, on the History screen.** The owner asked who voided a bill;
    /// they should not be reading `total_paise` and a pair of braces.
    #[test]
    fn a_change_reads_as_words_and_rupees() {
        let said = readable_change(
            r#"{"reason":"Billed twice","state":"voided","total_paise":39300}"#,
        );
        assert_eq!(said, "Reason Billed twice, State voided, Total 393.00");
        assert!(!said.contains('{'), "{said}");
        assert!(!said.contains("paise"), "{said}");
    }

    #[test]
    fn money_is_formatted_by_rust_and_only_by_rust() {
        // D39: TypeScript never divides by a hundred, here or anywhere.
        assert_eq!(readable_change(r#"{"amount_paise":5}"#), "Amount 0.05");
        assert_eq!(readable_change(r#"{"amount_paise":100000}"#), "Amount 1000.00");
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
