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

use crate::billing::CartState;
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

/// **The shop's day rule, as one number, for the whole process** — decision
/// D70.
///
/// D5 says the business day is stored and never derived, and P17 is where the
/// owner finally gets to choose when it starts. The question was how to get
/// their answer to the forty places that ask "which day is this?".
///
/// Threading it through every one of them means a `&App` on functions that have
/// no other reason to want one, and a lock acquired on the billing path — which
/// budget **B5** (150 ms to settle) exists to keep clear. Reading it from the
/// database at each call site is worse still.
///
/// So it is a static. Honestly: **this is one counter serving one shop**, the
/// rule is a single `u16`, and every caller in the process wants the same
/// answer. `App` sets it when the shop's configuration is loaded, and again
/// whenever it is saved.
static DAY_START_MINUTES: std::sync::atomic::AtomicU16 =
    std::sync::atomic::AtomicU16::new(DayRule::DEFAULT.starts_at_minutes());

/// Called by `App` when the shop's configuration is read or written.
pub fn set_day_rule(rule: DayRule) {
    DAY_START_MINUTES.store(
        rule.starts_at_minutes(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

#[must_use]
pub fn day_rule() -> DayRule {
    DayRule::new(DAY_START_MINUTES.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or(DayRule::DEFAULT)
}

pub fn today(at: Timestamp) -> BusinessDay {
    BusinessDay::of(at, day_rule(), UtcOffset::INDIA)
}

/// **Print a kitchen ticket for an order that is already on disk** — P24's
/// paper fallback.
///
/// `print_kitchen_ticket_on` prints the CART's delta, which is right when a
/// cashier presses the button. This prints a whole saved order, which is what
/// is needed when no kitchen screen drew a ticket in time: there is no cart
/// involved and the cashier may be three orders further on.
///
/// **The kitchen must never go blind.** That is the whole reason this exists,
/// and it is why it takes an order id rather than looking at the screen.
pub fn print_kitchen_ticket_for(app: &App, order_id: &str) -> UiResult<String> {
    let at = now();
    let printer = default_printer(app)?;
    let settings = app.shop_config().kitchen;

    let order = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .orders()
                    .find(&mb_core::OrderId::new(order_id.to_owned()))
            })
            .map_err(|e| words::from_db(&e))
    })?;
    let Some(order) = order else {
        return Err(UiError::new(
            "kitchen.no_order",
            "That order is no longer on this counter.",
        ));
    };
    let core = order.core();

    let lines: Vec<mb_print::template::TicketLine> = core
        .cart
        .lines()
        .iter()
        .map(|line| mb_print::template::TicketLine {
            name: line.snapshot.name.clone(),
            qty: line.qty,
            note: line.identity().note.clone(),
            modifiers: Vec::new(),
        })
        .collect();

    let table = core.table.as_ref().map(|t| t.as_str().to_owned());
    let ctx = mb_print::template::KitchenContext {
        kind: mb_print::template::TicketKind::New,
        token: order.token().map(|t| t.formatted.as_str()),
        bill_number: None,
        order_type: core.order_type,
        table: table.as_deref(),
        time: None,
        station: None,
        lines: &lines,
        settings: &settings,
    };
    let document = mb_print::template::kitchen_document(printer.paper, &ctx)
        .map_err(|e| words::from_print(&e))?;

    app.with_shop(|shop| {
        shop.queue
            .enqueue(
                Job::new(JobKind::Kitchen, &printer.id, document, today(at))
                    .because("no kitchen screen drew this in time".to_owned()),
            )
            .map_err(|e| words::from_print(&e))
    })
}

/// Print what the kitchen has not seen — **the delta, never the order.**
///
/// > Crown jewel 2: *"only what the kitchen has not seen gets printed, and what
/// > was printed is remembered **in the database**, not in the screen's
/// > memory."*
pub fn print_kitchen_ticket_on(app: &App) -> UiResult<String> {
    crate::guard::require(app, mb_auth::Permission::BillCreate)?;
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
    let printer = default_printer(app)?;
    // P17: the shop's own, not this crate's idea of a sensible one.
    let settings = app.shop_config().kitchen;
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

    // 2b. **The order goes on disk BEFORE the paper.** Same rule as
    //     `complete_bill`, and the same reason (D4): if the order is not saved
    //     first, a kitchen ticket exists for something the shop has no record
    //     of — and, until P12, there was no Open order on disk at all, so audit
    //     B6's "cancel the order" had nothing to cancel and the floor grid had
    //     nothing to show (scope 1.4).
    park_open_order(app)?;

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

    // 4. **And the ledger goes to disk too.** Crown jewel 2 is that what the
    //    kitchen was told is remembered *in the database, not in the screen's
    //    memory* — so parking before the paper (step 2b) is only half of it.
    //
    //    Found by cancelling an order: the slip listed nothing, because the
    //    parked row had been written before `mark_printed` and its ledger was
    //    empty. The order was on disk and what the kitchen knew was not.
    if let Err(e) = park_open_order(app) {
        log_warn!("the kitchen ledger could not be saved: {e}");
    }

    // 4b. **And the kitchen SCREEN is told too** (P24).
    //
    // Here, beside the paper, so screen and printer can never disagree about
    // what the kitchen knows. A shop with no screen has one ticket sitting
    // pending, nobody draws it, and twenty seconds later the counter prints it
    // — which is exactly the behaviour a shop with no screen wants, and it
    // costs one row.
    //
    // Not fatal: a kitchen screen that cannot be told is a shop that still has
    // paper, and the print above has already happened.
    if let Some(order_id) = app.with_cart(|state| Ok(state.order_id.clone()))?
        && let Err(e) = crate::kitchen::send(app, &order_id, None)
    {
        log_warn!("order={order_id} the kitchen screen could not be told: {e}");
    }

    // 5. **And WHEN it was told** (P14, scope 14.2). The floor's second timer
    //    is "food ordered 18 minutes ago and nothing since", which is the
    //    number that catches a forgotten table — and it cannot be read off the
    //    ledger, whose rows are rewritten whenever anything on the order
    //    changes. An event happened at a moment and nothing later moves it.
    if let Some(order_id) = app.with_cart(|state| Ok(state.order_id.clone()))? {
        let day = today(at);
        let recorded = app.with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    mb_db::Repos::new(tx).events().record(
                        &order_id,
                        at,
                        day,
                        mb_db::repo::events::KITCHEN_TICKET,
                        None,
                        None,
                    )
                })
                .map_err(|e| words::from_db(&e))
        });
        if let Err(e) = recorded {
            // The paper is already out. Losing the timestamp costs a timer on
            // the floor, not a ticket, so it is logged rather than raised.
            log_warn!("the kitchen ticket time could not be recorded: {e}");
        }
    }

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
pub fn complete_bill_on(app: &App) -> UiResult<String> {
    let who = crate::guard::require(app, mb_auth::Permission::BillCreate)?;
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
        let bill = state.bill(&app.shop_config())?;
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
            state.to_draft(at, opened_by, app.terminal_id())?,
            bill,
            state.settlement.clone(),
        ))
    })?;

    let (number, settled) = app.with_shop(|shop| {
        let till = mb_db::Till::new(OUTLET, app.terminal_id());
        // **An order the kitchen already knows about is already on disk**, with
        // its numbers claimed (see `park_open_order`). Claiming again would
        // burn a bill number per kitchen ticket and leave the parked row
        // behind, open, on a table nobody is sitting at.
        let existing = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&draft.core.id))
            .map_err(|e| words::from_db(&e))?;

        let open = match existing {
            Some(mb_core::AnyOrder::Open(mut open)) => {
                // The cart may have moved on since the ticket printed.
                open.core = draft.core.clone();
                open
            }
            // Opening claims the token and the bill number atomically (D6).
            _ => mb_db::open_draft(&shop.db, till, draft).map_err(|e| words::from_db(&e))?,
        };
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

    // P22: a line about a bill carries the order id, so a support call that
    // begins "the bill I printed at about half past eight" can reach a line in
    // a file. See `log_bill!`.
    crate::log_bill!(settled.core.id, "settled as {number} by {}", who.name);
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
    if let Err(e) = queue_bill_print(app, &number, &settled, &bill, &who.name) {
        // Deliberately not fatal: the money is recorded. The failure belongs in
        // the print queue's indicator, which is where a cashier looks.
        log_warn!(
            "order={} bill {number} settled but could not be queued to print: {e}",
            settled.core.id
        );
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
    let config = app.shop_config();
    let store = config.store.to_print_store();
    let settings = &config.receipt;
    let order = mb_core::AnyOrder::Settled(settled.clone());

    let document = mb_print::template::bill_document(
        printer.paper,
        &mb_print::template::BillContext {
            bill,
            order: &order,
            store: &store,
            settings,
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

// **`store_for_the_bill` used to be here, and its going is the point of P17.**
//
// It read the shop's profile out of the database on every print, and every
// OTHER setting on that path was `ReceiptSettings::default()` — so a shop could
// change its name and nothing else. The whole configuration is now loaded once
// into `App` (`settings::load`), and `settings::Store::to_print_store` is the
// single place mb-db's vocabulary becomes mb-print's, exactly as
// `state::printer_config_for` is for a printer.

/// Where a job goes before P17 sets up routing.
pub(crate) fn default_printer(app: &App) -> UiResult<PrinterConfig> {
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

/// **One named printer** — P29, for the label printer, which is chosen by id
/// rather than by being the default.
pub(crate) fn printer_by_id(app: &App, id: &str) -> UiResult<PrinterConfig> {
    let rows = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .map_err(|e| words::from_db(&e))
    })?;
    rows.iter()
        .find(|p| p.id == id)
        .map(crate::state::printer_config_for)
        .ok_or_else(|| {
            UiError::new(
                "print.no_printer",
                "That printer is not set up any more. Choose another one in                  Settings, under Devices.",
            )
        })
}

/// **One template, another argument** — P12's reprint, and audit D7.
///
/// `bill_document` already takes a [`Copy`](mb_print::template::Copy), so a
/// duplicate and a voided copy are the same call with a different value. There
/// is deliberately no second rendering path: *"v1's diverged because it had its
/// own copy of the layout"*, and the fix is that there is nothing to diverge
/// from.
pub(crate) fn queue_bill_copy(
    app: &App,
    order: &mb_core::AnyOrder,
    bill: &mb_core::Bill,
    cashier: &str,
    copy: mb_print::template::Copy,
) -> UiResult<()> {
    let printer = default_printer(app)?;
    let config = app.shop_config();
    let store = config.store.to_print_store();
    let settings = &config.receipt;
    let because = match &copy {
        mb_print::template::Copy::Original => "bill".to_owned(),
        mb_print::template::Copy::Duplicate { number } => format!("copy {number}"),
        mb_print::template::Copy::Voided { .. } => "voided copy".to_owned(),
    };

    let document = mb_print::template::bill_document(
        printer.paper,
        &mb_print::template::BillContext {
            bill,
            order,
            store: &store,
            settings,
            customer: None,
            cashier: Some(cashier),
            copy,
            einvoice: mb_print::template::EInvoice::default(),
            logo: None,
        },
    )
    .map_err(|e| words::from_print(&e))?;

    app.with_shop(|shop| {
        shop.queue
            .enqueue(Job::new(JobKind::Bill, &printer.id, document, today(now())).because(because))
            .map_err(|e| words::from_print(&e))
    })?;
    Ok(())
}

/// **Put the open order on disk** — scope 1.4, and the thing audit B6's fix
/// needs in order to have anything to cancel.
///
/// # Why here, and why now
///
/// Until P12 an order lived in the cart's memory until it was settled, so:
/// nothing appeared on the floor grid as busy, nothing survived a restart, and
/// `cancel_order` — whose whole finding is *"the order sits in the Processing
/// list forever and the table stays busy"* — could never find an order to
/// cancel. It was a command that could not work.
///
/// **The moment the kitchen is told is the moment the order is real.** Before
/// that it is somebody typing; after it, food is being cooked and a shop that
/// loses the record has lost money. So `print_kitchen_ticket` parks it, before
/// the paper, for the same reason `complete_bill` saves before it prints (D4).
///
/// Numbers are claimed exactly once: the first park claims them (D6, atomic),
/// and `complete_bill` reuses the parked order rather than claiming again.
pub(crate) fn park_open_order(app: &App) -> UiResult<String> {
    let at = now();
    let existing = app.with_cart(|state| Ok(state.order_id.clone()))?;
    let staff = app
        .sessions()
        .current()
        .map_or_else(|| mb_core::StaffId::new(crate::state::DEFAULT_STAFF), |s| s.actor.staff_id);

    let draft = app.with_cart(|state| {
        let opened_by = state.opened_by.clone().unwrap_or_else(|| staff.clone());
        state.to_draft(at, opened_by, app.terminal_id())
    })?;
    let id = draft.core.id.clone();

    let number = app.with_shop(|shop| {
        let till = mb_db::Till::new(OUTLET, app.terminal_id());
        let found = match &existing {
            Some(_) => shop
                .db
                .transaction(|tx| mb_db::Repos::new(tx).orders().find(&id))
                .map_err(|e| words::from_db(&e))?,
            None => None,
        };

        match found {
            // Already parked: keep its numbers, take the cart as it is now.
            Some(mb_core::AnyOrder::Open(mut open)) => {
                open.core = draft.core.clone();
                let number = open.bill_number.formatted.clone();
                shop.db
                    .transaction(|tx| {
                        mb_db::Repos::new(tx).orders().save(
                            OUTLET,
                            app.terminal_id(),
                            &mb_core::AnyOrder::Open(open.clone()),
                        )
                    })
                    .map_err(|e| words::from_db(&e))?;
                Ok(number)
            }
            // Settled, voided or cancelled: it is not ours to touch any more.
            Some(_) => Err(UiError::new(
                "order.finished",
                "This order has already been finished. Start a new one.",
            )),
            None => {
                let open = mb_db::open_draft(&shop.db, till, draft.clone())
                    .map_err(|e| words::from_db(&e))?;
                Ok(open.bill_number.formatted)
            }
        }
    })?;

    // The cart now knows which order it is, so the next park updates rather
    // than claiming a second set of numbers.
    app.with_cart_mut(|state| {
        state.order_id = Some(id.as_str().to_owned());
        if state.opened_by.is_none() {
            state.opened_by = Some(staff.clone());
        }
        Ok(())
    })?;

    Ok(number)
}

// ---------------------------------------------------------------------------
// The command seats (D46). The bodies above take `&App` so the sequences they
// belong to can be driven in a test — see `signin_tests.rs`.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn print_kitchen_ticket(app: State<'_, App>) -> UiResult<String> {
    print_kitchen_ticket_on(&app)
}

#[tauri::command]
pub fn complete_bill(app: State<'_, App>) -> UiResult<String> {
    complete_bill_on(&app)
}
