//! The two flows that put an order on paper.

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

/// The shop's day rule, as one number, for the whole process.
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

/// A dine-in order needs a table before anything leaves the counter.
pub(crate) fn require_a_table(state: &crate::billing::CartState) -> UiResult<()> {
    if state.order_type == mb_core::OrderType::DineIn && state.table.is_none() {
        return Err(UiError::new(
            "bill.no_table",
            "This is a dine-in order with no table. Type the table number and \
             press Enter, or change the order type.",
        ));
    }
    Ok(())
}

/// The one place kitchen paper is made.
#[expect(
    clippy::too_many_arguments,
    reason = "the one door to kitchen paper — see the note above; every argument is \n              a thing only the caller knows, and splitting them into a struct would \n              let a caller build half of one"
)]
pub(crate) fn queue_kitchen_lines(
    app: &App,
    kind: mb_print::template::TicketKind,
    order_type: mb_core::OrderType,
    table: Option<&str>,
    order: Option<&mb_core::AnyOrder>,
    lines: Vec<(mb_core::ItemId, mb_print::template::TicketLine)>,
    reprint: bool,
    reason: String,
) -> UiResult<String> {
    if lines.is_empty() {
        return Ok(String::new());
    }
    let at = now();
    // The table's own name, never its id.
    let table = label_for(app, table);
    let token = order.and_then(|o| o.token().map(|t| t.formatted.clone()));
    let bill_number = order.and_then(|o| o.bill_number().map(|b| b.formatted.clone()));
    let waiter = order.and_then(|o| staff_name(app, &o.core().created_by));
    let time = clock_time(at);

    // Grouped by printer id and NOT by category: a shop with six categories and two printers
    // wants two tickets, not six.
    let fallback = default_kitchen_printer(app)?;
    let categories = categories_of(
        app,
        &lines.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
    );

    let mut stations: Vec<(PrinterConfig, Vec<mb_print::template::TicketLine>)> = Vec::new();
    for (item, line) in lines {
        let printer = routed_printer(app, categories.get(item.as_str()).map(String::as_str))
            .unwrap_or_else(|| fallback.clone());
        match stations.iter_mut().find(|(p, _)| p.id == printer.id) {
            Some((_, theirs)) => theirs.push(line),
            None => stations.push((printer, vec![line])),
        }
    }

    // The shop's own settings, not this crate's idea of sensible ones.
    let settings = app.shop_config().kitchen;

    // The id of the FIRST ticket is what comes back, because the caller uses it for one thing —
    // telling the cashier something is printing.
    let mut id = String::new();
    for (printer, station_lines) in &stations {
        // One number per roll of paper.
        let kot = claim_kot_number(app, today(at));
        let ctx = mb_print::template::KitchenContext {
            kind,
            token: token.as_deref(),
            bill_number: bill_number.as_deref(),
            kot_number: kot.as_deref(),
            order_type,
            table: table.as_deref(),
            time: Some(time.as_str()),
            waiter: waiter.as_deref(),
            reprint,
            // Which station this roll belongs to, on the paper.
            station: if stations.len() > 1 {
                Some(printer.name.as_str())
            } else {
                None
            },
            lines: station_lines,
            settings: &settings,
        };
        let document = mb_print::template::kitchen_document(printer.paper, &ctx)
            .map_err(|e| words::from_print(&e))?;
        let queued = app.print(
            Job::new(JobKind::Kitchen, &printer.id, document, today(now())).because(reason.clone()),
        )?;
        if id.is_empty() {
            id = queued;
        }
    }
    Ok(id)
}

/// The words a cook reads, for a set of ledger deltas.
pub(crate) fn ticket_lines(
    cart: &mb_core::Cart,
    delta: &[(mb_core::LineIdentity, mb_core::Qty)],
) -> Vec<(mb_core::ItemId, mb_print::template::TicketLine)> {
    delta
        .iter()
        .map(|(identity, qty)| {
            (
                identity.item_id.clone(),
                mb_print::template::TicketLine {
                    name: cart
                        .lines()
                        .iter()
                        .find(|line| &line.identity() == identity)
                        .map_or_else(
                            || identity.item_id.as_str().to_owned(),
                            |line| line.snapshot.name.clone(),
                        ),
                    qty: *qty,
                    note: identity.note.clone(),
                    modifiers: Vec::new(),
                },
            )
        })
        .collect()
}

/// Print a kitchen ticket for an order that is already on disk.
pub fn print_kitchen_ticket_for(app: &App, order_id: &str) -> UiResult<String> {
    let id = mb_core::OrderId::new(order_id.to_owned());
    let order = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&id))
            .map_err(|e| words::from_db(&e))
    })?;
    let Some(order) = order else {
        return Err(UiError::new(
            "kitchen.no_order",
            "That order is no longer on this counter.",
        ));
    };

    // The delta, exactly like the button.
    let core = order.core();
    let delta = core.kitchen.pending(&core.cart).map_err(|e| {
        UiError::new(
            "kitchen.pending",
            "What the kitchen still needs could not be worked out. Nothing has been sent.",
        )
        .with_detail(e.to_string())
    })?;
    if delta.is_empty() {
        return Ok(String::new());
    }

    let table = core.table.as_ref().map(|t| t.as_str().to_owned());
    let queued = queue_kitchen_lines(
        app,
        mb_print::template::TicketKind::New,
        core.order_type,
        table.as_deref(),
        Some(&order),
        ticket_lines(&core.cart, &delta),
        false,
        "no kitchen screen drew this in time".to_owned(),
    )?;

    // And the ledger remembers, on disk.
    let mut order = order;
    order.core_mut().kitchen.mark_printed(&delta).map_err(|e| {
        UiError::new(
            "kitchen.record",
            "The ticket was sent but could not be recorded. Check with the \
             kitchen before sending it again.",
        )
        .with_detail(e.to_string())
    })?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .orders()
                    .save(OUTLET, app.terminal_id(), &order)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    Ok(queued)
}

/// Print what the kitchen has not seen — the delta, never the order.
pub fn print_kitchen_ticket_on(app: &App) -> UiResult<String> {
    // One counter action at a time — see `App::begin_action`.
    let _one_at_a_time = app.begin_action();
    crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    let at = now();

    // What is new, from the ledger that travels with the order — so this is right after a
    // merge, after a restart, and on a second terminal.
    let (delta, lines, order_type, table) = app.with_cart(|state| {
        // Before the paper, not after it.
        require_a_table(state)?;
        let delta = state.cart_pending()?;
        if delta.is_empty() {
            return Err(UiError::new(
                "kitchen.nothing",
                "The kitchen already has everything on this bill.",
            )
            .quietly());
        }
        // Each line keeps its ITEM as well as its words, because which station cooks it is
        // decided by the item's category further down.
        let lines: Vec<(mb_core::ItemId, mb_print::template::TicketLine)> = delta
            .iter()
            .map(|(identity, qty)| {
                (
                    identity.item_id.clone(),
                    mb_print::template::TicketLine {
                        // The ledger stores ids; the name comes from the cart line's frozen
                        // snapshot.
                        name: state
                            .cart
                            .lines()
                            .iter()
                            .find(|line| &line.identity() == identity)
                            .map_or_else(|| "Item".to_owned(), |line| line.snapshot.name.clone()),
                        qty: *qty,
                        note: identity.note.clone(),
                        modifiers: Vec::new(),
                    },
                )
            })
            .collect();
        Ok((delta, lines, state.order_type, state.table.clone()))
    })?;

    // Split the ticket by station.
    let reason = label_for(app, table.as_deref())
        .map_or_else(|| "kitchen ticket".to_owned(), |t| format!("table {t}"));

    // 2a. The order goes on disk BEFORE the paper.
    park_open_order(app)?;
    let order = open_order_now(app);

    let id = queue_kitchen_lines(
        app,
        mb_print::template::TicketKind::New,
        order_type,
        table.as_deref(),
        order.as_ref(),
        lines,
        false,
        reason,
    )?;

    // Only once it is durably queued do we remember it was told.
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

    // And the ledger goes to disk too.
    if let Err(e) = park_open_order(app) {
        log_warn!("the kitchen ledger could not be saved: {e}");
    }

    // 4b. And the kitchen SCREEN is told too.
    if let Some(order_id) = app.with_cart(|state| Ok(state.order_id.clone()))?
        && let Err(e) = crate::kitchen::send(app, &order_id, None)
    {
        log_warn!("order={order_id} the kitchen screen could not be told: {e}");
    }

    // And WHEN it was told.
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
            // The paper is already out.
            log_warn!("the kitchen ticket time could not be recorded: {e}");
        }
    }

    log_info!("the kitchen was told about {} line(s)", delta.len());
    Ok(id)
}

/// Settle the bill.
pub fn complete_bill_on(app: &App) -> UiResult<String> {
    // One counter action at a time — see `App::begin_action`.
    let _one_at_a_time = app.begin_action();
    let who = crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    let at = now();
    let settled_by = who.staff_id.clone();

    let (draft, bill, settlement) = app.with_cart(|state| {
        if state.cart.is_empty() {
            return Err(UiError::new("bill.empty", "There is nothing on this bill yet.").quietly());
        }
        // Mb-db refuses this at `open_draft`, which is the right place for the rule but the
        // wrong moment for the person: they have already taken the money.
        require_a_table(state)?;
        let bill = state.bill(&app.shop_config())?;
        // `is_settled`, not `amount_due`. `amount_due` is what the bill asks for and is
        // positive on every bill that has anything on it — so checking it refused paid bills
        // too, which is what running this found.
        let settled = state.settlement.is_settled(bill.grand_total).map_err(|e| {
            UiError::new("bill.money", "This bill's balance could not be worked out.")
                .with_detail(e.to_string())
        })?;
        if !settled {
            let left = state.settlement.balance(bill.grand_total).map_err(|e| {
                UiError::new("bill.money", "This bill's balance could not be worked out.")
                    .with_detail(e.to_string())
            })?;
            return Err(UiError::new(
                "bill.unpaid",
                format!("{} is still to pay on this bill.", left.to_plain_string()),
            ));
        }
        // Whoever opened it, and only this person if nobody else did.
        let opened_by = state
            .opened_by
            .clone()
            .unwrap_or_else(|| settled_by.clone());
        Ok((
            state.to_draft(at, opened_by, app.terminal_id())?,
            bill,
            state.settlement.clone(),
        ))
    })?;

    let (number, settled) = app.with_shop(|shop| {
        let till = mb_db::Till::new(OUTLET, app.terminal_id());
        // An order the kitchen already knows about is already on disk, with its numbers claimed
        // (see `park_open_order`).
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
            // Settled, voided or cancelled: it is not ours to bill again.
            Some(_) => {
                return Err(UiError::new(
                    "order.finished",
                    "This order has already been finished. Start a new one.",
                ));
            }
            // Opening claims the token and the bill number atomically.
            None => mb_db::open_draft(&shop.db, till, draft).map_err(|e| words::from_db(&e))?,
        };
        let formatted = open.bill_number.formatted.clone();
        // And settling writes the order, its lines, its payments and its ledger in ONE commit.
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

    // A line about a bill carries the order id, so a support call that begins "the bill I
    // printed at about half past eight" can reach a line in a file.
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

    // THE ORDER IS ON DISK.
    if let Err(e) = queue_bill_print(app, &number, &settled, &bill, &who.name) {
        // Deliberately not fatal: the money is recorded.
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

/// The real bill, with the real cashier's name on it.
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
    let table = order
        .core()
        .table
        .as_ref()
        .and_then(|t| table_label(app, t));
    let time = clock_time(now());
    let waiter = staff_name(app, &order.core().created_by);

    let (metrics, _) = app.metrics_for(JobKind::Bill, &printer);
    let document = mb_print::template::bill_document(
        &metrics,
        &mb_print::template::BillContext {
            bill,
            order: &order,
            store: &store,
            settings,
            customer: None,
            cashier: Some(cashier),
            // The real values, resolved once, here.
            table: table.as_deref(),
            time: Some(time.as_str()),
            waiter: waiter.as_deref(),
            copy: mb_print::template::Copy::Original,
            einvoice: mb_print::template::EInvoice::default(),
            // This was `None` here and at every other call site, so `receipt.logo` and
            // `receipt.logo_width_pct` were two settings pointing at a picture nothing could
            // supply.
            logo: crate::logo::stored(app),
        },
    )
    .map_err(|e| words::from_print(&e))?;

    app.print(
        Job::new(JobKind::Bill, &printer.id, document, today(now()))
            .because(format!("bill {number}")),
    )?;
    Ok(())
}

/// Where a BILL goes.
pub(crate) fn default_printer(app: &App) -> UiResult<PrinterConfig> {
    let rows = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .map_err(|e| words::from_db(&e))
    })?;

    let prints_bills = |p: &&mb_db::repo::settings::Printer| p.role == "bill" || p.role == "both";

    rows.iter()
        .find(|p| p.is_default && prints_bills(p))
        .or_else(|| rows.iter().find(prints_bills))
        .or_else(|| rows.iter().find(|p| p.is_default))
        .or_else(|| rows.first())
        .map(crate::state::printer_config_for)
        // Start-up saves a stand-in when the shop has none (see `state::fallback_row`), so this
        // is genuinely unreachable — and it returns an error rather than inventing a printer,
        // because a printer only this function believes in is exactly the bug that produced
        // "there is no printer prn_none" and then "FOREIGN KEY constraint failed".
        .ok_or_else(|| {
            UiError::new(
                "print.no_printer",
                "This shop has no printer set up, and the stand-in is missing. \
                 Add a printer in Settings.",
            )
        })
}

/// Where a KITCHEN TICKET goes, and it is the category that decides.
fn routed_printer(app: &App, category: Option<&str>) -> Option<PrinterConfig> {
    let category = category?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).settings().category_printers(OUTLET))
            .map_err(|e| words::from_db(&e))
    })
    .ok()?
    .into_iter()
    .find(|(c, _)| c == category)
    .and_then(|(_, printer_id)| printer_by_id(app, &printer_id).ok())
}

/// Where a kitchen ticket goes when its category has no route of its own.
fn default_kitchen_printer(app: &App) -> UiResult<PrinterConfig> {
    let rows = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .map_err(|e| words::from_db(&e))
    })?;

    let prints_tickets =
        |p: &&mb_db::repo::settings::Printer| p.role == "kitchen" || p.role == "both";

    rows.iter()
        .find(|p| p.is_default && prints_tickets(p))
        .or_else(|| rows.iter().find(prints_tickets))
        .or_else(|| rows.iter().find(|p| p.is_default))
        .or_else(|| rows.first())
        .map(crate::state::printer_config_for)
        .ok_or_else(|| {
            UiError::new(
                "print.no_printer",
                "This shop has no printer set up, and the stand-in is missing. \
                 Add a printer in Settings.",
            )
        })
}

/// Which category each item on this ticket belongs to.
fn categories_of(
    app: &App,
    items: &[mb_core::ItemId],
) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Ok(menu) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).menu().list_items(OUTLET, false))
            .map_err(|e| words::from_db(&e))
    }) else {
        // A menu that will not read is not a reason to lose the ticket: every line falls
        // through to the default kitchen printer, which is where they all went before routing
        // existed at all.
        return out;
    };
    for item in menu {
        if items.iter().any(|wanted| wanted == &item.id)
            && let Some(category) = item.category_id
        {
            out.insert(item.id.as_str().to_owned(), category.as_str().to_owned());
        }
    }
    out
}

/// One named printer.
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

/// Carry the bill to the table.
pub fn print_open_bill_on(app: &App, order_id: String) -> UiResult<String> {
    // One counter action at a time — see `App::begin_action`.
    let _one_at_a_time = app.begin_action();
    let who = crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    let id = mb_core::OrderId::new(order_id.clone());

    let found = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&id))
            .map_err(|e| words::from_db(&e))
    })?;

    // Only an OPEN order.
    let open = match found {
        Some(mb_core::AnyOrder::Open(open)) => open,
        Some(mb_core::AnyOrder::Settled(_)) => {
            return Err(UiError::new(
                "bill.already_settled",
                "This bill is already paid. Print another copy from Bills.",
            )
            .quietly());
        }
        _ => {
            return Err(UiError::new(
                "bill.not_open",
                "There is no open order on that table any more.",
            ));
        }
    };

    if open.core.cart.is_empty() {
        return Err(
            UiError::new("bill.empty", "There is nothing on this table's bill yet.").quietly(),
        );
    }

    let config = app.shop_config();
    let charges = config.billing.charges_for(open.core.order_type);
    let bill = mb_core::compute_bill(
        mb_core::BillInput::new(&open.core.cart, crate::billing::registration_of(&config))
            .with_order_type(open.core.order_type)
            .with_rounding(config.billing.rounding)
            .with_charges(&charges),
    )
    .map_err(|e| {
        UiError::new(
            "bill.money",
            "This table's bill could not be worked out. Nothing has been changed.",
        )
        .with_detail(e.to_string())
    })?;

    let table = open.core.table.clone();
    let order = mb_core::AnyOrder::Open(open);
    queue_bill_copy(
        app,
        &order,
        &bill,
        &who.name,
        mb_print::template::Copy::NotPaid,
    )?;

    // What the shop calls this table, not what the database calls it.
    let label = table.as_ref().and_then(|id| table_label(app, id));

    log_info!(
        "the bill for {} was carried out by {}",
        label.as_deref().unwrap_or("an order with no table"),
        who.name
    );
    Ok(match label {
        Some(label) => format!("The bill for table {label} is printing."),
        None => "The bill is printing.".to_owned(),
    })
}

/// What the shop calls a table.
pub(crate) fn table_label(app: &App, id: &mb_core::TableId) -> Option<String> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).floor().list_tables(OUTLET))
            .map_err(|e| words::from_db(&e))
    })
    .ok()?
    .into_iter()
    .find(|t| t.id.as_str() == id.as_str())
    .map(|t| t.label)
}

/// The same, for a table held as a bare string on the cart.
pub(crate) fn label_for(app: &App, id: Option<&str>) -> Option<String> {
    table_label(app, &mb_core::TableId::new(id?))
}

/// The shop's first table, by the name the shop gave it.
#[must_use]
pub fn first_table_label(app: &App) -> Option<String> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).floor().list_tables(OUTLET))
            .map_err(|e| words::from_db(&e))
    })
    .ok()?
    .into_iter()
    .next()
    .map(|t| t.label)
}

/// Who is signed in, by name.
#[must_use]
pub fn current_staff_name(app: &App) -> Option<String> {
    app.sessions().current().map(|s| s.actor.name)
}

// See the bill before it prints.

/// The bill for an order, as it would print right now.
pub fn preview_order_on(
    app: &App,
    order_id: Option<String>,
) -> UiResult<crate::preview::PreviewDoc> {
    let printer = default_printer(app)?;
    let config = app.shop_config();
    let store = config.store.to_print_store();
    let (metrics, engine) = app.metrics_for(JobKind::Bill, &printer);

    let (bill, order) = match order_id {
        Some(id) => {
            let found = app.with_shop(|shop| {
                shop.db
                    .transaction(|tx| {
                        mb_db::Repos::new(tx)
                            .orders()
                            .find(&mb_core::OrderId::new(&id))
                    })
                    .map_err(|e| words::from_db(&e))
            })?;
            let order = found.ok_or_else(|| {
                UiError::new("preview.no_order", "That order is not on this counter.")
            })?;
            let bill = bill_of(app, &order, &config)?;
            (bill, order)
        }
        // The cart's own order, when it has one.
        None if app
            .with_cart(|s| Ok(s.order_id.clone()))
            .ok()
            .flatten()
            .is_some() =>
        {
            let bill = app.with_cart(|state| state.bill(&config))?;
            let order = open_order_now(app).ok_or_else(|| {
                UiError::new("preview.no_order", "That order is not on this counter.")
            })?;
            (bill, order)
        }
        None => {
            let bill = app.with_cart(|state| state.bill(&config))?;
            // A cart is not an order yet, so it is shown as the bill a waiter would carry to
            // the table — which is what it is.
            let at = now();
            let day = today(at);
            let mut core = app.with_cart(|state| Ok(state.to_core_for_printing()))?;
            core.business_day = day;
            core.created_at = at;
            let open = mb_core::OpenOrder {
                core,
                token: mb_core::Claimed {
                    value: 0,
                    // Empty, so no token line prints at all.
                    formatted: String::new(),
                    business_day: day,
                },
                bill_number: mb_core::Claimed {
                    value: 0,
                    // Not a plausible number.
                    formatted: "NOT YET".to_owned(),
                    business_day: day,
                },
            };
            (bill, mb_core::AnyOrder::Open(open))
        }
    };

    let table = order
        .core()
        .table
        .as_ref()
        .and_then(|t| table_label(app, t));
    let time = clock_time(now());
    let waiter = staff_name(app, &order.core().created_by);
    let copy = match &order {
        mb_core::AnyOrder::Settled(_) => mb_print::template::Copy::Original,
        _ => mb_print::template::Copy::NotPaid,
    };

    // The same `metrics` the layout below uses, so the document is built for the shape it will
    // be drawn in.
    let document = mb_print::template::bill_document(
        &metrics,
        &mb_print::template::BillContext {
            bill: &bill,
            order: &order,
            store: &store,
            settings: &config.receipt,
            customer: None,
            cashier: current_staff_name(app).as_deref(),
            table: table.as_deref(),
            time: Some(time.as_str()),
            waiter: waiter.as_deref(),
            copy,
            einvoice: mb_print::template::EInvoice::default(),
            logo: crate::logo::stored(app),
        },
    )
    .map_err(|e| words::from_print(&e))?;

    let laid =
        mb_print::layout::layout_for(&document, &metrics).map_err(|e| words::from_print(&e))?;
    Ok(crate::preview::to_preview(&laid, &metrics, engine))
}

/// The kitchen ticket that would print right now — the delta, exactly as the button would send
/// it.
pub fn preview_kitchen_on(
    app: &App,
    order_id: Option<String>,
) -> UiResult<crate::preview::PreviewDoc> {
    let printer = default_kitchen_printer(app)?;
    let config = app.shop_config();
    let (metrics, engine) = app.metrics_for(JobKind::Kitchen, &printer);

    let (core, order) = match order_id {
        Some(id) => {
            let found = app.with_shop(|shop| {
                shop.db
                    .transaction(|tx| {
                        mb_db::Repos::new(tx)
                            .orders()
                            .find(&mb_core::OrderId::new(&id))
                    })
                    .map_err(|e| words::from_db(&e))
            })?;
            let order = found.ok_or_else(|| {
                UiError::new("preview.no_order", "That order is not on this counter.")
            })?;
            (order.core().clone(), Some(order))
        }
        // The CART's lines, with the ORDER's numbers.
        None => (
            app.with_cart(|state| Ok(state.to_core_for_printing()))?,
            open_order_now(app),
        ),
    };

    let delta = core.kitchen.pending(&core.cart).map_err(|e| {
        UiError::new(
            "kitchen.pending",
            "What the kitchen still needs could not be worked out.",
        )
        .with_detail(e.to_string())
    })?;
    if delta.is_empty() {
        return Err(UiError::new(
            "kitchen.nothing",
            "The kitchen already has everything on this bill.",
        )
        .quietly());
    }

    let lines: Vec<mb_print::template::TicketLine> = ticket_lines(&core.cart, &delta)
        .into_iter()
        .map(|(_, line)| line)
        .collect();
    let table = core.table.as_ref().and_then(|t| table_label(app, t));
    let time = clock_time(now());
    let waiter = staff_name(app, &core.created_by);
    let token = order
        .as_ref()
        .and_then(|o| o.token().map(|t| t.formatted.clone()));
    let number = order
        .as_ref()
        .and_then(|o| o.bill_number().map(|b| b.formatted.clone()));

    let document = mb_print::template::kitchen_document(
        printer.paper,
        &mb_print::template::KitchenContext {
            kind: mb_print::template::TicketKind::New,
            token: token.as_deref(),
            bill_number: number.as_deref(),
            // Not claimed. A preview that took a number would burn one out of the shop's own
            // series every time somebody looked at it.
            kot_number: None,
            order_type: core.order_type,
            table: table.as_deref(),
            time: Some(time.as_str()),
            waiter: waiter.as_deref(),
            station: None,
            reprint: false,
            lines: &lines,
            settings: &config.kitchen,
        },
    )
    .map_err(|e| words::from_print(&e))?;

    let laid =
        mb_print::layout::layout_for(&document, &metrics).map_err(|e| words::from_print(&e))?;
    Ok(crate::preview::to_preview(&laid, &metrics, engine))
}

/// Send the kitchen the whole order again.
pub fn reprint_kitchen_ticket_on(app: &App) -> UiResult<String> {
    let _one_at_a_time = app.begin_action();
    crate::guard::require(app, mb_auth::Permission::BillCreate)?;

    let (lines, order_type, table) = app.with_cart(|state| {
        let lines: Vec<(mb_core::ItemId, mb_print::template::TicketLine)> = state
            .cart
            .lines()
            .iter()
            .map(|line| {
                (
                    line.snapshot.item_id.clone(),
                    mb_print::template::TicketLine {
                        name: line.snapshot.name.clone(),
                        qty: line.qty,
                        note: line.note.clone(),
                        modifiers: line.modifiers.iter().map(|m| m.name.clone()).collect(),
                    },
                )
            })
            .collect();
        Ok((lines, state.order_type, state.table.clone()))
    })?;

    if lines.is_empty() {
        return Err(
            UiError::new("kitchen.nothing", "There is nothing on this bill to send.").quietly(),
        );
    }

    let order = open_order_now(app);
    queue_kitchen_lines(
        app,
        mb_print::template::TicketKind::New,
        order_type,
        table.as_deref(),
        order.as_ref(),
        lines,
        true,
        "reprint".to_owned(),
    )
}

pub fn bill_pdf_on(app: &App, order_id: String) -> UiResult<crate::reports::SavedFileView> {
    crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    let config = app.shop_config();
    let store = config.store.to_print_store();

    let order = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    mb_db::Repos::new(tx)
                        .orders()
                        .find(&mb_core::OrderId::new(&order_id))
                })
                .map_err(|e| words::from_db(&e))
        })?
        .ok_or_else(|| UiError::new("preview.no_order", "That bill is not on this counter."))?;
    let bill = bill_of(app, &order, &config)?;

    let number = order
        .bill_number()
        .map(|n| n.formatted.clone())
        .ok_or_else(|| {
            UiError::new(
                "invoice.no_number",
                "That order has no bill number yet, so there is no invoice to make.",
            )
        })?;
    let table = order
        .core()
        .table
        .as_ref()
        .and_then(|t| table_label(app, t));
    let time = clock_time(order.core().created_at);
    let waiter = staff_name(app, &order.core().created_by);

    // The printer's own metrics, not a face's.
    let metrics = mb_print::metrics::Metrics::printer_font(mb_print::paper::Paper::new(
        mb_print::paper::PaperKind::A4,
    ));
    let document = mb_print::template::bill_document(
        &metrics,
        &mb_print::template::BillContext {
            bill: &bill,
            order: &order,
            store: &store,
            settings: &config.receipt,
            customer: None,
            cashier: None,
            table: table.as_deref(),
            time: Some(time.as_str()),
            waiter: waiter.as_deref(),
            copy: mb_print::template::Copy::Original,
            einvoice: mb_print::template::EInvoice::default(),
            logo: None,
        },
    )
    .map_err(|e| words::from_print(&e))?;

    let laid =
        mb_print::layout::layout_for(&document, &metrics).map_err(|e| words::from_print(&e))?;

    let clean: String = number
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    crate::reports::save(
        &format!("Invoice-{clean}.pdf"),
        &mb_print::pdf::to_pdf(&laid),
    )
}

/// The bill for a stored order — recomputed from its own cart, which is what every other reader
/// of a stored order does.
fn bill_of(
    app: &App,
    order: &mb_core::AnyOrder,
    config: &crate::settings::ShopConfig,
) -> UiResult<mb_core::Bill> {
    if let mb_core::AnyOrder::Settled(settled) = order {
        // A settled order carries the bill it was settled with, and that is the one that was
        // printed.
        return Ok(settled.bill.clone());
    }
    let _ = app;
    let core = order.core();
    let charges = config.billing.charges_for(core.order_type);
    let input = mb_core::BillInput::new(&core.cart, crate::billing::registration_of(config))
        .with_order_type(core.order_type)
        .with_rounding(config.billing.rounding)
        .with_charges(&charges);
    mb_core::compute_bill(input).map_err(|e| {
        UiError::new(
            "bill.compute",
            "This bill could not be worked out. Nothing has been changed.",
        )
        .with_detail(e.to_string())
    })
}

/// The clock, as a bill and a ticket want to read it.
#[must_use]
#[expect(
    clippy::integer_division,
    reason = "seconds into hours and minutes for a printed clock; no amount is involved"
)]
pub fn clock_time(at: Timestamp) -> String {
    // Minutes since midnight, local.
    let local = at.millis() / 1_000 + i64::from(UtcOffset::INDIA.minutes()) * 60;
    let of_day = local.rem_euclid(86_400);
    let hours = of_day / 3_600;
    let minutes = (of_day % 3_600) / 60;
    format!("{hours:02}:{minutes:02}")
}

/// The order this counter is working on, as it now stands on disk.
fn open_order_now(app: &App) -> Option<mb_core::AnyOrder> {
    let id = app.with_cart(|state| Ok(state.order_id.clone())).ok()??;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .orders()
                    .find(&mb_core::OrderId::new(&id))
            })
            .map_err(|e| words::from_db(&e))
    })
    .ok()
    .flatten()
}

/// The ticket's own running number.
fn claim_kot_number(app: &App, day: BusinessDay) -> Option<String> {
    let terminal: String = app.terminal_id().to_owned();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::numbering::claim(tx, OUTLET, &terminal, mb_db::CounterKind::Kot, day)
            })
            .map_err(|e| words::from_db(&e))
    })
    .inspect_err(|e| {
        log_warn!("a kitchen ticket printed without a number: {}", e.message);
    })
    .ok()
    .map(|claimed| claimed.formatted)
}

/// Who took the order, by name, for the paper.
pub(crate) fn staff_name(app: &App, id: &mb_core::StaffId) -> Option<String> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .people()
                    .find_staff(OUTLET, id.as_str())
            })
            .map_err(|e| words::from_db(&e))
    })
    .ok()?
    .map(|s| s.name)
}

/// One template, another argument.
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
        mb_print::template::Copy::NotPaid => "bill to the table".to_owned(),
    };
    let table = order
        .core()
        .table
        .as_ref()
        .and_then(|t| table_label(app, t));
    let time = clock_time(now());
    let waiter = staff_name(app, &order.core().created_by);

    let (metrics, _) = app.metrics_for(JobKind::Bill, &printer);
    let document = mb_print::template::bill_document(
        &metrics,
        &mb_print::template::BillContext {
            bill,
            order,
            store: &store,
            settings,
            customer: None,
            cashier: Some(cashier),
            table: table.as_deref(),
            time: Some(time.as_str()),
            waiter: waiter.as_deref(),
            copy,
            einvoice: mb_print::template::EInvoice::default(),
            // A duplicate is the same paper as the original, so it carries the same letterhead.
            logo: crate::logo::stored(app),
        },
    )
    .map_err(|e| words::from_print(&e))?;

    app.print(Job::new(JobKind::Bill, &printer.id, document, today(now())).because(because))?;
    Ok(())
}

/// Put the open order on disk.
pub(crate) fn park_open_order(app: &App) -> UiResult<String> {
    let at = now();

    // The same guard `complete_bill` has, at the OTHER door into `open_draft`.
    app.with_cart(|state| {
        require_a_table(state)?;
        Ok(())
    })?;

    let existing = app.with_cart(|state| Ok(state.order_id.clone()))?;
    let staff = app.sessions().current().map_or_else(
        || mb_core::StaffId::new(crate::state::DEFAULT_STAFF),
        |s| s.actor.staff_id,
    );

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

    // The cart now knows which order it is, so the next park updates rather than claiming a
    // second set of numbers.
    app.with_cart_mut(|state| {
        state.order_id = Some(id.as_str().to_owned());
        if state.opened_by.is_none() {
            state.opened_by = Some(staff.clone());
        }
        Ok(())
    })?;

    Ok(number)
}

// The command seats.

#[tauri::command]
pub fn print_kitchen_ticket(app: State<'_, App>) -> UiResult<String> {
    print_kitchen_ticket_on(&app)
}

#[tauri::command]
pub fn reprint_kitchen_ticket(app: State<'_, App>) -> UiResult<String> {
    reprint_kitchen_ticket_on(&app)
}

#[tauri::command]
pub fn bill_pdf(app: State<'_, App>, order_id: String) -> UiResult<crate::reports::SavedFileView> {
    bill_pdf_on(&app, order_id)
}

#[tauri::command]
pub fn complete_bill(app: State<'_, App>) -> UiResult<String> {
    complete_bill_on(&app)
}

#[tauri::command]
pub fn print_open_bill(app: State<'_, App>, order_id: String) -> UiResult<String> {
    print_open_bill_on(&app, order_id)
}
