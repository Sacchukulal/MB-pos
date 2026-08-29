//! The two flows that put an order on paper.

use mb_core::{AnyOrder, BusinessDay, DayRule, OrderId, StaffId, TableId, Timestamp, UtcOffset};
use mb_print::printer::{PrinterConfig, Role};
use mb_print::queue::{Job, JobKind};
use mb_print::template::Copy;
use tauri::State;

use crate::billing::bill_for;
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

/// The only way a business day is decided.
pub fn today(at: Timestamp) -> BusinessDay {
    BusinessDay::of(at, day_rule(), UtcOffset::INDIA)
}

/// Who is doing this: the signed-in person, or the stand-in on a shop with no PIN.
fn staff_now(app: &App) -> StaffId {
    app.sessions().current().map_or_else(
        || StaffId::new(crate::state::DEFAULT_STAFF),
        |s| s.actor.staff_id,
    )
}

// The kitchen.

/// The one place kitchen paper is made.
#[expect(
    clippy::too_many_arguments,
    reason = "every argument is a thing only the caller knows, and a struct would let a caller build half of one"
)]
pub(crate) fn queue_kitchen_lines(
    app: &App,
    kind: mb_print::template::TicketKind,
    order_type: mb_core::OrderType,
    table: Option<&TableId>,
    order: Option<&AnyOrder>,
    lines: Vec<(mb_core::ItemId, mb_print::template::TicketLine)>,
    reprint: bool,
    reason: String,
) -> UiResult<String> {
    if lines.is_empty() {
        return Ok(String::new());
    }
    let at = now();
    let table = table.and_then(|id| table_name(app, id));
    let token = order.and_then(|o| o.token().map(|t| t.formatted.clone()));
    let bill_number = order.and_then(|o| o.bill_number().map(|b| b.formatted.clone()));
    let waiter = order.and_then(|o| staff_name(app, &o.core().created_by));
    let time = clock_time(at);

    // Grouped by printer and NOT by category: a shop with six categories and two printers
    // wants two tickets, not six.
    let fallback = printer_for(app, Role::Kitchen)?;
    let routes = routes(
        app,
        &lines.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
    );

    let mut stations: Vec<(PrinterConfig, Vec<mb_print::template::TicketLine>)> = Vec::new();
    for (item, line) in lines {
        let printer = routes
            .get(item.as_str())
            .cloned()
            .unwrap_or_else(|| fallback.clone());
        match stations.iter_mut().find(|(p, _)| p.id == printer.id) {
            Some((_, theirs)) => theirs.push(line),
            None => stations.push((printer, vec![line])),
        }
    }

    let settings = app.shop_config().kitchen;

    // The id of the FIRST ticket is what comes back: the caller only tells the cashier
    // something is printing.
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
            Job::new(JobKind::Kitchen, &printer.id, document, today(at)).because(reason.clone()),
        )?;
        if id.is_empty() {
            id = queued;
        }
    }
    Ok(id)
}

/// Which printer each of these items' categories is routed to — one read, not one per line.
fn routes(
    app: &App,
    items: &[mb_core::ItemId],
) -> std::collections::HashMap<String, PrinterConfig> {
    let mut out = std::collections::HashMap::new();
    let read = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let category_printers = repos.settings().category_printers(OUTLET)?;
                let printers = repos.settings().list_printers(OUTLET)?;
                let mut routed = Vec::new();
                for item in items {
                    let Some(item) = repos.menu().find_item(item)? else {
                        continue;
                    };
                    let Some(category) = item.category_id else {
                        continue;
                    };
                    let Some((_, printer_id)) = category_printers
                        .iter()
                        .find(|(c, _)| *c == category.as_str())
                    else {
                        continue;
                    };
                    if let Some(row) = printers.iter().find(|p| &p.id == printer_id) {
                        routed.push((
                            item.id.as_str().to_owned(),
                            crate::state::printer_config_for(row),
                        ));
                    }
                }
                Ok(routed)
            })
            .map_err(|e| words::from_db(&e))
    });
    // A menu that will not read is not a reason to lose the ticket: every line goes to the
    // default kitchen printer.
    if let Ok(routed) = read {
        out.extend(routed);
    }
    out
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

/// Print a kitchen ticket for an order that is already on disk — the paper fallback.
pub fn print_kitchen_ticket_for(app: &App, order_id: &str) -> UiResult<String> {
    let id = OrderId::new(order_id.to_owned());
    let Some(mut order) = find_order(app, &id)? else {
        return Err(UiError::new(
            "kitchen.no_order",
            "That order is no longer on this counter.",
        ));
    };

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

    let queued = queue_kitchen_lines(
        app,
        mb_print::template::TicketKind::New,
        core.order_type(),
        core.table(),
        Some(&order),
        ticket_lines(&core.cart, &delta),
        false,
        "no kitchen screen drew this in time".to_owned(),
    )?;

    // And the ledger remembers, on disk.
    order.core_mut().kitchen.mark_printed(&delta).map_err(|e| {
        UiError::new(
            "kitchen.record",
            "The ticket was sent but could not be recorded. Check with the \
             kitchen before sending it again.",
        )
        .with_detail(e.to_string())
    })?;
    save_order(app, &order)?;
    Ok(queued)
}

/// A shop that said it has no kitchen ticket does not print one.
fn kitchen_ticket_allowed(app: &App) -> UiResult<()> {
    if app.shop_config().billing.kitchen_ticket_off {
        return Err(UiError::new(
            "kitchen.off",
            "This shop has no kitchen ticket. Turn it on under Settings › Billing.",
        )
        .quietly());
    }
    Ok(())
}

/// Print what the kitchen has not seen — the delta, never the order.
pub fn print_kitchen_ticket_on(app: &App) -> UiResult<String> {
    let _one_at_a_time = app.begin_action();
    crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    kitchen_ticket_allowed(app)?;
    let at = now();

    // The order goes on disk BEFORE the paper.
    let open = park_open_order(app)?;
    let core = open.core.clone();
    let delta = core.kitchen.pending(&core.cart).map_err(|e| {
        UiError::new(
            "kitchen.delta",
            "What the kitchen still needs could not be worked out. Nothing has been sent.",
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

    let reason = core
        .table()
        .and_then(|t| table_name(app, t))
        .map_or_else(|| "kitchen ticket".to_owned(), |t| format!("table {t}"));
    let order = AnyOrder::Open(open);
    let id = queue_kitchen_lines(
        app,
        mb_print::template::TicketKind::New,
        core.order_type(),
        core.table(),
        Some(&order),
        ticket_lines(&core.cart, &delta),
        false,
        reason,
    )?;

    // Only once the paper is durably queued does anything remember it was told — the cart,
    // the order, the event and the kitchen screen, in ONE commit.
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
    let AnyOrder::Open(mut open) = order else {
        unreachable!("built as Open above")
    };
    open.core.kitchen.mark_printed(&delta).map_err(|e| {
        UiError::new("kitchen.record", "The ticket could not be recorded.")
            .with_detail(e.to_string())
    })?;
    let order_id = open.core.id.as_str().to_owned();
    let day = open.core.business_day;
    let saved = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos
                    .orders()
                    .save(OUTLET, app.terminal_id(), &AnyOrder::Open(open.clone()))?;
                repos.events().record(
                    &order_id,
                    at,
                    day,
                    mb_db::repo::events::KITCHEN_TICKET,
                    None,
                    None,
                )?;
                crate::kitchen::send_in(&repos, &app.shop_config(), &order_id, None, at)?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    });
    if let Err(e) = saved {
        // The paper is already out.
        log_warn!("order={order_id} the kitchen ticket was printed but not recorded: {e}");
        return Err(e);
    }

    log_info!("the kitchen was told about {} line(s)", delta.len());
    Ok(id)
}

/// Send the kitchen the whole order again.
pub fn reprint_kitchen_ticket_on(app: &App) -> UiResult<String> {
    let _one_at_a_time = app.begin_action();
    crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    kitchen_ticket_allowed(app)?;

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
        Ok((
            lines,
            state.order_type(),
            state.table().map(|t| t.id.clone()),
        ))
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
        table.as_ref(),
        order.as_ref(),
        lines,
        true,
        "reprint".to_owned(),
    )
}

// The bill.

/// Settle the bill. Whatever is still owed is taken in `mode` first, so the cashier's one press
/// is one command.
pub fn complete_bill_on(app: &App, mode: Option<String>) -> UiResult<String> {
    let _one_at_a_time = app.begin_action();
    let who = crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    let at = now();
    let settled_by = who.staff_id.clone();
    let config = app.shop_config();

    if let Some(mode) = mode {
        let balance = app.with_cart(|state| {
            let bill = state.bill(&config)?;
            state.settlement.balance(bill.grand_total).map_err(|e| {
                UiError::new("bill.money", "This bill's balance could not be worked out.")
                    .with_detail(e.to_string())
            })
        })?;
        if balance.is_positive() {
            // Not `cart_add_payment_on`: this thread already holds the counter.
            crate::ipc::take_payment(app, mode, balance.paise(), None)?;
        }
    }

    let (core, bill, settlement) = app.with_cart(|state| {
        if state.cart.is_empty() {
            return Err(UiError::new("bill.empty", "There is nothing on this bill yet.").quietly());
        }
        let bill = state.bill(&config)?;
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
        Ok((
            state.to_core(at, &settled_by, app.terminal_id())?,
            bill,
            state.settlement.clone(),
        ))
    })?;

    let (number, settled) = app.with_shop(|shop| {
        let till = mb_db::Till::new(OUTLET, app.terminal_id());
        let existing = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&core.id))
            .map_err(|e| words::from_db(&e))?;

        let open = match existing {
            // Parked already: its numbers stay, the cart as it is now goes in. The time and the
            // day came from the order itself, through the cart's origin.
            Some(AnyOrder::Open(mut open)) => {
                open.core = core.clone();
                open
            }
            Some(_) => {
                return Err(UiError::new(
                    "order.finished",
                    "This order has already been finished. Start a new one.",
                ));
            }
            None => mb_db::open_draft(&shop.db, till, mb_core::DraftOrder { core: core.clone() })
                .map_err(|e| words::from_db(&e))?,
        };
        let formatted = open.bill_number.formatted.clone();
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

    // THE ORDER IS ON DISK. A printer that is off cannot lose the money.
    if let Err(e) = queue_bill(
        app,
        &AnyOrder::Settled(settled.clone()),
        &bill,
        &who.name,
        Copy::Original,
    ) {
        log_warn!(
            "order={} bill {number} settled but could not be queued to print: {e}",
            settled.core.id
        );
    }

    let config = app.shop_config();
    app.with_cart_mut(|state| {
        *state = crate::billing::CartState::new_order(crate::billing::starting_order_type(
            &config,
            state.order_type(),
        ));
        Ok(())
    })?;

    Ok(number)
}

/// The paper for a bill, built the same way for the printer, the preview and the PDF.
pub(crate) struct BillPaper<'a> {
    pub order: &'a AnyOrder,
    pub bill: &'a mb_core::Bill,
    pub copy: Copy,
    pub cashier: Option<&'a str>,
    /// The letterhead — off for a PDF invoice.
    pub logo: bool,
}

fn bill_document(
    app: &App,
    metrics: &mb_print::metrics::Metrics,
    paper: &BillPaper<'_>,
) -> UiResult<mb_print::doc::Document> {
    let config = app.shop_config();
    let store = config.store.to_print_store();
    let core = paper.order.core();
    let table = core.table().and_then(|t| table_name(app, t));
    let time = clock_time(match paper.order {
        AnyOrder::Settled(_) | AnyOrder::Voided(_) => core.created_at,
        _ => now(),
    });
    let waiter = staff_name(app, &core.created_by);
    mb_print::template::bill_document(
        metrics,
        &mb_print::template::BillContext {
            bill: paper.bill,
            order: paper.order,
            store: &store,
            settings: &config.receipt,
            customer: None,
            cashier: paper.cashier,
            table: table.as_deref(),
            time: Some(time.as_str()),
            waiter: waiter.as_deref(),
            copy: paper.copy.clone(),
            einvoice: mb_print::template::EInvoice::default(),
            logo: if paper.logo {
                crate::logo::stored(app)
            } else {
                None
            },
        },
    )
    .map_err(|e| words::from_print(&e))
}

/// Put a bill on the bill printer: the original, a copy, or the one carried to the table.
pub(crate) fn queue_bill(
    app: &App,
    order: &AnyOrder,
    bill: &mb_core::Bill,
    cashier: &str,
    copy: Copy,
) -> UiResult<()> {
    let printer = default_printer(app)?;
    let because = match &copy {
        Copy::Original => order
            .bill_number()
            .map_or_else(|| "bill".to_owned(), |n| format!("bill {}", n.formatted)),
        Copy::Duplicate { number } => format!("copy {number}"),
        Copy::Voided { .. } => "voided copy".to_owned(),
        Copy::NotPaid => "bill to the table".to_owned(),
    };
    let (metrics, _) = app.metrics_for(JobKind::Bill, &printer);
    let document = bill_document(
        app,
        &metrics,
        &BillPaper {
            order,
            bill,
            copy,
            cashier: Some(cashier),
            logo: true,
        },
    )?;
    app.print(Job::new(JobKind::Bill, &printer.id, document, today(now())).because(because))?;
    Ok(())
}

/// Carry the bill to the table.
pub fn print_open_bill_on(app: &App, order_id: String) -> UiResult<String> {
    let _one_at_a_time = app.begin_action();
    let who = crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    let id = OrderId::new(order_id);

    let open = match find_order(app, &id)? {
        Some(AnyOrder::Open(open)) => open,
        Some(AnyOrder::Settled(_)) => {
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

    let order = AnyOrder::Open(open);
    let bill = bill_of(app, &order)?;
    queue_bill(app, &order, &bill, &who.name, Copy::NotPaid)?;

    let label = order.core().table().and_then(|t| table_name(app, t));
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

/// A phone asked for the bill: the same paper "Print bill" makes at the counter, on the bill
/// printer, with the waiter's name where the cashier's would be. No session guard — the
/// permission was checked on the intent, which is the door a phone comes through.
pub(crate) fn print_bill_from_floor(app: &App, order_id: &str, waiter: &StaffId) -> UiResult<()> {
    let id = OrderId::new(order_id.to_owned());
    let Some(order @ AnyOrder::Open(_)) = find_order(app, &id)? else {
        return Err(UiError::new(
            "bill.not_open",
            "There is no open order on that table any more.",
        ));
    };
    let bill = bill_of(app, &order)?;
    let name = staff_name(app, waiter).unwrap_or_else(|| "the floor".to_owned());
    queue_bill(app, &order, &bill, &name, Copy::NotPaid)?;
    log_info!(
        "the bill for {} was asked for from the floor by {name}",
        order
            .core()
            .table()
            .and_then(|t| table_name(app, t))
            .unwrap_or_else(|| "an order with no table".to_owned())
    );
    Ok(())
}

/// The bill for an order, as it would print right now.
pub fn preview_order_on(
    app: &App,
    order_id: Option<String>,
) -> UiResult<crate::preview::PreviewDoc> {
    let printer = default_printer(app)?;
    let (metrics, engine) = app.metrics_for(JobKind::Bill, &printer);

    let order = match order_id {
        Some(id) => find_order(app, &OrderId::new(&id))?.ok_or_else(|| {
            UiError::new("preview.no_order", "That order is not on this counter.")
        })?,
        // The cart's own order when it has one; otherwise the cart as the bill a waiter would
        // carry to the table.
        None => match open_order_now(app) {
            Some(order) => order,
            None => {
                let staff = staff_now(app);
                let core =
                    app.with_cart(|state| state.to_core(now(), &staff, app.terminal_id()))?;
                AnyOrder::Draft(mb_core::DraftOrder { core })
            }
        },
    };
    // The cart's lines, when the cart is what is being looked at.
    let bill =
        match order_id_of(&order) == app.with_cart(|s| Ok(s.order_id().map(str::to_owned)))? {
            true => app.with_cart(|state| state.bill(&app.shop_config()))?,
            false => bill_of(app, &order)?,
        };
    let copy = match &order {
        AnyOrder::Settled(_) => Copy::Original,
        _ => Copy::NotPaid,
    };
    let cashier = current_staff_name(app);
    let document = bill_document(
        app,
        &metrics,
        &BillPaper {
            order: &order,
            bill: &bill,
            copy,
            cashier: cashier.as_deref(),
            logo: true,
        },
    )?;
    let laid =
        mb_print::layout::layout_for(&document, &metrics).map_err(|e| words::from_print(&e))?;
    Ok(crate::preview::to_preview(&laid, &metrics, engine))
}

fn order_id_of(order: &AnyOrder) -> Option<String> {
    Some(order.core().id.as_str().to_owned())
}

pub fn bill_pdf_on(app: &App, order_id: String) -> UiResult<crate::reports::SavedFileView> {
    crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    let order = find_order(app, &OrderId::new(&order_id))?
        .ok_or_else(|| UiError::new("preview.no_order", "That bill is not on this counter."))?;
    let bill = bill_of(app, &order)?;
    let number = order
        .bill_number()
        .map(|n| n.formatted.clone())
        .ok_or_else(|| {
            UiError::new(
                "invoice.no_number",
                "That order has no bill number yet, so there is no invoice to make.",
            )
        })?;

    // The printer's own metrics, not a face's.
    let metrics = mb_print::metrics::Metrics::printer_font(mb_print::paper::Paper::new(
        mb_print::paper::PaperKind::A4,
    ));
    let document = bill_document(
        app,
        &metrics,
        &BillPaper {
            order: &order,
            bill: &bill,
            copy: Copy::Original,
            cashier: None,
            logo: false,
        },
    )?;
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

/// The bill for a stored order. A settled order carries the bill it was settled with; anything
/// else is computed from its own cart, the same way the counter does it.
pub(crate) fn bill_of(app: &App, order: &AnyOrder) -> UiResult<mb_core::Bill> {
    if let AnyOrder::Settled(settled) = order {
        return Ok(settled.bill.clone());
    }
    if let AnyOrder::Voided(voided) = order {
        return Ok(voided.bill.clone());
    }
    let core = order.core();
    bill_for(&core.cart, core.order_type(), None, &app.shop_config())
}

// Printers, names, the clock.

/// Where a bill goes.
pub(crate) fn default_printer(app: &App) -> UiResult<PrinterConfig> {
    printer_for(app, Role::Bill)
}

/// The printer for this kind of paper: the default if it can, else any that can, else the
/// default, else the first. Start-up saves a stand-in when the shop has none, so "none at all"
/// is genuinely unreachable and is an error rather than an invented printer.
pub(crate) fn printer_for(app: &App, role: Role) -> UiResult<PrinterConfig> {
    let rows = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .map_err(|e| words::from_db(&e))
    })?;
    let wanted = match role {
        Role::Kitchen => "kitchen",
        _ => "bill",
    };
    let can = |p: &&mb_db::repo::settings::Printer| p.role == wanted || p.role == "both";
    rows.iter()
        .find(|p| p.is_default && can(p))
        .or_else(|| rows.iter().find(can))
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
                "That printer is not set up any more. Choose another one in Settings, under Devices.",
            )
        })
}

/// What the shop calls a table — the one lookup, by id.
pub(crate) fn table_name(app: &App, id: &TableId) -> Option<String> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).floor().find_table(id))
            .map_err(|e| words::from_db(&e))
    })
    .ok()
    .flatten()
    .map(|t| t.label)
}

/// The shop's first table, by the name the shop gave it — for the sample bill.
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

/// Who took the order, by name, for the paper.
pub(crate) fn staff_name(app: &App, id: &StaffId) -> Option<String> {
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

/// The clock, as a bill and a ticket want to read it.
#[must_use]
#[expect(
    clippy::integer_division,
    reason = "seconds into hours and minutes for a printed clock; no amount is involved"
)]
pub fn clock_time(at: Timestamp) -> String {
    let local = at.millis() / 1_000 + i64::from(UtcOffset::INDIA.minutes()) * 60;
    let of_day = local.rem_euclid(86_400);
    let hours = of_day / 3_600;
    let minutes = (of_day % 3_600) / 60;
    format!("{hours:02}:{minutes:02}")
}

// Orders on disk.

pub(crate) fn find_order(app: &App, id: &OrderId) -> UiResult<Option<AnyOrder>> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(id))
            .map_err(|e| words::from_db(&e))
    })
}

pub(crate) fn save_order(app: &App, order: &AnyOrder) -> UiResult<()> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .orders()
                    .save(OUTLET, app.terminal_id(), order)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// The order this counter is working on, as it now stands on disk.
pub(crate) fn open_order_now(app: &App) -> Option<AnyOrder> {
    let id = app
        .with_cart(|state| Ok(state.order_id().map(str::to_owned)))
        .ok()??;
    find_order(app, &OrderId::new(&id)).ok().flatten()
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

/// Put the open order on disk, and give the cart its identity if it had none. The numbers,
/// the time and the day are claimed once and never move.
pub(crate) fn park_open_order(app: &App) -> UiResult<mb_core::OpenOrder> {
    let at = now();
    let staff = staff_now(app);
    let core = app.with_cart(|state| state.to_core(at, &staff, app.terminal_id()))?;

    let open = app.with_shop(|shop| {
        let till = mb_db::Till::new(OUTLET, app.terminal_id());
        let found = shop
            .db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&core.id))
            .map_err(|e| words::from_db(&e))?;
        match found {
            Some(AnyOrder::Open(mut open)) => {
                open.core = core.clone();
                shop.db
                    .transaction(|tx| {
                        mb_db::Repos::new(tx).orders().save(
                            OUTLET,
                            app.terminal_id(),
                            &AnyOrder::Open(open.clone()),
                        )
                    })
                    .map_err(|e| words::from_db(&e))?;
                Ok(open)
            }
            Some(_) => Err(UiError::new(
                "order.finished",
                "This order has already been finished. Start a new one.",
            )),
            None => mb_db::open_draft(&shop.db, till, mb_core::DraftOrder { core: core.clone() })
                .map_err(|e| words::from_db(&e)),
        }
    })?;

    app.with_cart_mut(|state| {
        state.adopt(&open.core);
        Ok(())
    })?;
    Ok(open)
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
pub fn complete_bill(app: State<'_, App>, mode: Option<String>) -> UiResult<String> {
    complete_bill_on(&app, mode)
}

#[tauri::command]
pub fn print_open_bill(app: State<'_, App>, order_id: String) -> UiResult<String> {
    print_open_bill_on(&app, order_id)
}
