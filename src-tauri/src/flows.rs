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

use mb_core::{BusinessDay, DayRule, Timestamp, UtcOffset};
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

pub fn today(at: Timestamp) -> BusinessDay {
    BusinessDay::of(at, DayRule::default(), UtcOffset::INDIA)
}

/// Print what the kitchen has not seen — **the delta, never the order.**
///
/// > Crown jewel 2: *"only what the kitchen has not seen gets printed, and what
/// > was printed is remembered **in the database**, not in the screen's
/// > memory."*
#[tauri::command]
pub fn print_kitchen_ticket(app: State<'_, App>) -> UiResult<String> {
    crate::guard::require(&app, mb_auth::Permission::BillCreate)?;
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
///
/// # Two people, two columns
///
/// `orders.created_by` is whoever put the first line on this bill;
/// `orders.settled_by` is whoever is signed in when the money is taken. During
/// a shift change those are different people, the schema has carried both
/// columns unused since P04, and P11 is where they start meaning something.
#[tauri::command]
pub fn complete_bill(app: State<'_, App>) -> UiResult<String> {
    let who = crate::guard::require(&app, mb_auth::Permission::BillCreate)?;
    let at = now();
    let settled_by = who.staff_id.clone();

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
        // Whoever opened it, and only this person if nobody else did. A cart
        // with no opener is a cart nothing was ever added to, which the
        // emptiness check above has already refused.
        let opened_by = state.opened_by.clone().unwrap_or_else(|| settled_by.clone());
        Ok((
            state.to_draft(at, opened_by)?,
            bill,
            state.settlement.clone(),
        ))
    })?;

    let (number, settled) = app.with_shop(|shop| {
        let till = mb_db::Till::new(OUTLET, TERMINAL);
        // Opening claims the token and the bill number atomically (D6).
        let open = mb_db::open_draft(&shop.db, till, draft).map_err(|e| words::from_db(&e))?;
        let formatted = open.bill_number.formatted.clone();
        // And settling writes the order, its lines, its payments and its ledger
        // in ONE commit (B5, and §5 rule 1).
        let settled = mb_db::settle(
            &shop.db,
            till,
            open,
            bill.clone(),
            settlement,
            at,
            settled_by.clone(),
        )
        .map_err(|e| words::from_db(&e))?;
        Ok((formatted, settled))
    })?;

    log_info!("bill {number} settled by {}", who.name);
    app.record(
        &mb_auth::AuditEntry::new(
            at,
            today(at),
            Some(settled_by),
            mb_auth::audit::action::BILL_SETTLED,
            "bill",
        )
        .about(number.clone())
        .with_after(serde_json::json!({ "total_paise": bill.grand_total.paise() })),
    );

    // THE ORDER IS ON DISK. Now, and only now, the paper.
    if let Err(e) = queue_bill_print(&app, &number, &settled, &bill, &who.name) {
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

/// **The real bill, with the real cashier's name on it** — audit C3.
///
/// > *"The bill always says 'Cashier: Admin'. Even though a staff list exists."*
///
/// Until P11 this queued P07's test slip, with a comment saying the real
/// document waited for P13 and P17. It could not wait: "the cashier's name is
/// on the bill" is not demonstrable on a document that has no cashier line, and
/// scope 9.4 belongs to this session.
///
/// **Where the shop has not filled something in, the bill says nothing.** It
/// does not invent a name, an address or a GSTIN — P17 gives the owner the
/// screen; this gives them a bill that is honest about being new.
pub(crate) fn queue_bill_print(
    app: &App,
    number: &str,
    settled: &mb_core::SettledOrder,
    bill: &mb_core::Bill,
    cashier: &str,
) -> UiResult<()> {
    let printer = default_printer(app)?;
    let store = store_for_the_bill(app);
    let settings = mb_print::settings::ReceiptSettings::default();
    let order = mb_core::AnyOrder::Settled(settled.clone());

    let document = mb_print::template::bill_document(
        printer.paper,
        &mb_print::template::BillContext {
            bill,
            order: &order,
            store: &store,
            settings: &settings,
            customer: None,
            cashier: Some(cashier),
            copy: mb_print::template::Copy::Original,
            einvoice: mb_print::template::EInvoice::default(),
            logo: None,
        },
    )
    .map_err(|e| words::from_print(&e))?;

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

/// mb-db's stored profile becomes mb-print's header — **the one place those two
/// vocabularies meet**, for the reason `state::printer_config_for` already
/// gives.
///
/// A shop that has not been set up yet gets an empty `Store`, and the template
/// already omits every line it has nothing for.
fn store_for_the_bill(app: &App) -> mb_print::template::Store {
    let profile = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| mb_db::Repos::new(tx).settings().store_profile(OUTLET))
                .map_err(|e| words::from_db(&e))
        })
        .unwrap_or(None);

    profile.map_or_else(mb_print::template::Store::default, |p| {
        mb_print::template::Store {
            name: p.name,
            address: p.address,
            phone: p.phone,
            gstin: p.gstin,
            fssai: p.fssai,
            state_code: p.state_code,
            upi_id: p.upi_id,
            upi_merchant_name: p.upi_merchant_name,
            is_composition: p.is_composition,
        }
    })
}

/// Where a job goes before P17 sets up routing.
fn default_printer(app: &App) -> UiResult<PrinterConfig> {
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
