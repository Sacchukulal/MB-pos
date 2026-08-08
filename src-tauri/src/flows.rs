//! The two flows that put an order on paper.
//!
//! # The order is saved first and queued second, in that order
//!
//! That sentence is the whole of audit **D4** — *"a failed print never loses
//! the order"* — and it is true **by construction** rather than by care:
//!
//! * `settle` commits (P05: one transaction, one fsync, budget B5), and the
//!   bill number is claimed inside that same transaction (D6), so two
//!   terminals cannot take the same one;
//! * only then does `enqueue` write its own durable row (P07, D35);
//! * a print that fails is retried, then **parked**, and shows in the shell's
//!   persistent indicator — which P08 built and which is D4's visible half.
//!
//! If the printer is on fire, the money is still recorded.

use mb_core::{BusinessDay, DayRule, StaffId, Timestamp, UtcOffset};
use mb_print::printer::PrinterConfig;
use mb_print::queue::{Job, JobKind};
use tauri::State;

use crate::billing::{CartState, TERMINAL};
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

pub fn now() -> Timestamp {
    Timestamp::from_millis(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0)),
    )
}

fn today(at: Timestamp) -> BusinessDay {
    BusinessDay::of(at, DayRule::default(), UtcOffset::INDIA)
}

/// Print what the kitchen has not seen — **the delta, never the order.**
///
/// > Crown jewel 2: *"only what the kitchen has not seen gets printed, and what
/// > was printed is remembered **in the database**, not in the screen's
/// > memory."*
#[tauri::command]
pub fn print_kitchen_ticket(app: State<'_, App>) -> UiResult<String> {
    let at = now();

    // 1. What is new, from the ledger that travels with the order — so this is
    //    right after a merge, after a restart, and on a second terminal.
    let (delta, lines, order_type, table) = app.with_cart(|state| {
        let delta = state.cart_pending()?;
        if delta.is_empty() {
            return Err(UiError::new(
                "kitchen.nothing",
                "The kitchen already has everything on this bill.",
            ));
        }
        let lines: Vec<mb_print::template::TicketLine> = delta
            .iter()
            .map(|(identity, qty)| mb_print::template::TicketLine {
                // The ledger stores ids; the name comes from the cart line's
                // frozen snapshot (crown jewel 4).
                name: state
                    .cart
                    .lines()
                    .iter()
                    .find(|line| &line.identity() == identity)
                    .map_or_else(|| "Item".to_owned(), |line| line.snapshot.name.clone()),
                qty: *qty,
                note: identity.note.clone(),
                modifiers: Vec::new(),
            })
            .collect();
        Ok((delta, lines, state.order_type, state.table.clone()))
    })?;

    // 2. Build it and hand it to the queue.
    let printer = default_printer(&app)?;
    let settings = mb_print::settings::KitchenSettings::default();
    let ctx = mb_print::template::KitchenContext {
        kind: mb_print::template::TicketKind::New,
        token: None,
        bill_number: None,
        order_type,
        table: table.as_deref(),
        time: None,
        station: None,
        lines: &lines,
        settings: &settings,
    };
    let document =
        mb_print::template::kitchen_document(printer.paper, &ctx).map_err(|e| words::from_print(&e))?;

    let reason = table
        .clone()
        .map_or_else(|| "kitchen ticket".to_owned(), |t| format!("table {t}"));
    let id = app.with_shop(|shop| {
        shop.queue
            .enqueue(
                Job::new(JobKind::Kitchen, &printer.id, document, today(at)).because(reason),
            )
            .map_err(|e| words::from_print(&e))
    })?;

    // 3. **Only once it is durably queued** do we remember it was told. Marking
    //    first and enqueuing second would lose the items silently if the
    //    enqueue failed — and the kitchen would never learn about them.
    app.with_cart_mut(|state| {
        state.kitchen.mark_printed(&delta).map_err(|e| {
            UiError::new(
                "kitchen.record",
                "The ticket was sent but could not be recorded. Check with the \
                 kitchen before sending it again.",
            )
            .with_detail(e.to_string())
        })
    })?;

    log_info!("kitchen ticket queued with {} line(s)", lines.len());
    Ok(id)
}

/// Settle the bill — **P05's `settle`, which is one transaction.**
#[tauri::command]
pub fn complete_bill(app: State<'_, App>) -> UiResult<String> {
    let at = now();
    let staff = StaffId::new(crate::state::DEFAULT_STAFF);

    let (draft, bill, settlement) = app.with_cart(|state| {
        if state.cart.is_empty() {
            return Err(UiError::new("bill.empty", "There is nothing on this bill yet."));
        }
        // mb-db refuses this at `open_draft` (audit 2.3), which is the right
        // place for the rule but the wrong moment for the person: they have
        // already taken the money. Say it here, in words that say what to do.
        if state.order_type == mb_core::OrderType::DineIn && state.table.is_none() {
            return Err(UiError::new(
                "bill.no_table",
                "This is a dine-in order with no table. Type the table number and \
                 press Enter, or change the order type.",
            ));
        }
        let bill = state.bill()?;
        // **`is_settled`, not `amount_due`.** `amount_due` is what the bill asks
        // for and is positive on every bill that has anything on it — so
        // checking it refused paid bills too, which is what running this found.
        // `is_settled` is the rule itself, and it allows an overpayment (change
        // is due, and the bill is still paid).
        let settled = state.settlement.is_settled(bill.grand_total).map_err(|e| {
            UiError::new("bill.money", "This bill's balance could not be worked out.")
                .with_detail(e.to_string())
        })?;
        if !settled {
            let left = state
                .settlement
                .balance(bill.grand_total)
                .map_err(|e| UiError::new("bill.money", "This bill's balance could not be worked out.")
                    .with_detail(e.to_string()))?;
            return Err(UiError::new(
                "bill.unpaid",
                format!("{} is still to pay on this bill.", left.to_plain_string()),
            ));
        }
        Ok((state.to_draft(at, staff.clone())?, bill, state.settlement.clone()))
    })?;

    let number = app.with_shop(|shop| {
        let till = mb_db::Till::new(OUTLET, TERMINAL);
        // Opening claims the token and the bill number atomically (D6).
        let open = mb_db::open_draft(&shop.db, till, draft).map_err(|e| words::from_db(&e))?;
        let formatted = open.bill_number.formatted.clone();
        // And settling writes the order, its lines, its payments and its ledger
        // in ONE commit (B5, and §5 rule 1).
        mb_db::settle(&shop.db, till, open, bill, settlement, at, staff)
            .map_err(|e| words::from_db(&e))?;
        Ok(formatted)
    })?;

    log_info!("bill {number} settled");

    // THE ORDER IS ON DISK. Now, and only now, the paper.
    if let Err(e) = queue_bill_print(&app, &number) {
        // Deliberately not fatal: the money is recorded. The failure belongs in
        // the print queue's indicator, which is where a cashier looks.
        log_warn!("bill {number} settled but could not be queued to print: {e}");
    }

    app.with_cart_mut(|state| {
        // A new order, keeping the type — the lock's whole purpose.
        let kept = state.order_type;
        *state = CartState {
            order_type: kept,
            ..CartState::default()
        };
        Ok(())
    })?;

    Ok(number)
}

fn queue_bill_print(app: &State<'_, App>, number: &str) -> UiResult<()> {
    // P06's bill template wants a settled order, a store profile and receipt
    // settings, which P13 and P17 fill in. Until then the slip is P07's test
    // document, so the QUEUE path is exercised end to end rather than mocked —
    // and what matters here is that a failure lands in the indicator.
    let printer = default_printer(app)?;
    let document = mb_print::testprint::test_document(&printer, None);
    app.with_shop(|shop| {
        shop.queue
            .enqueue(
                Job::new(JobKind::Bill, &printer.id, document, today(now()))
                    .because(format!("bill {number}")),
            )
            .map_err(|e| words::from_print(&e))
    })?;
    Ok(())
}

/// Where a job goes before P17 sets up routing.
fn default_printer(app: &State<'_, App>) -> UiResult<PrinterConfig> {
    let rows = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .map_err(|e| words::from_db(&e))
    })?;

    rows.iter()
        .find(|p| p.is_default)
        .or_else(|| rows.first())
        .map(crate::state::printer_config_for)
        // Start-up saves a stand-in when the shop has none (see
        // `state::fallback_row`), so this is genuinely unreachable — and it
        // returns an error rather than inventing a printer, because a printer
        // only this function believes in is exactly the bug that produced
        // "there is no printer prn_none" and then "FOREIGN KEY constraint
        // failed". If the row is missing, saying so is the honest outcome.
        .ok_or_else(|| {
            UiError::new(
                "print.no_printer",
                "This shop has no printer set up, and the stand-in is missing. \
                 Add a printer in Settings.",
            )
        })
}
