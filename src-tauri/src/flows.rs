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

/// **A dine-in order needs a table before anything leaves the counter.**
///
/// mb-db refuses this at `open_draft` (audit 2.3), which is the right place for
/// the rule and the wrong moment for the person: by then they have taken the
/// money. So it is asked here, in words that say what to do — and, since the
/// owner's round of 22 Aug 2026, at every door that leads to paper rather than
/// only at the last one.
///
/// It used to be written out twice, in settling and in parking, and the kitchen
/// flow reached parking only *after* its tickets were queued: the cook had the
/// paper and the counter then said the order could not be saved. One copy, at
/// the top of each flow, is the whole fix.
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

/// **The one place kitchen paper is made** — the owner's round of 22 Aug 2026.
///
/// Three paths used to build their own: the button, the twenty-second fallback
/// and the cancellation slip. Only the button asked for the KITCHEN printer and
/// split the lines by station. The other two asked [`default_printer`], which
/// is the *bill* printer — and `mb_print::queue` refuses a kitchen job on a
/// bill printer, so in a shop with two machines those tickets were thrown away
/// while the delivery was still marked "printed". In a shop whose single
/// printer does both jobs they came out instead, whole and duplicated.
///
/// Every caller now hands its lines to this. The only thing that differs
/// between them is *which* lines, which is the only thing that ever should
/// have differed.
///
/// An empty list is not an error: it means the kitchen already has everything,
/// and there is no paper to make.
///
/// # Everything the paper needs is resolved HERE — P32
///
/// `token`, `time` and `bill_number` used to be arguments, and every caller
/// passed `None` for at least one of them: `time` and `bill_number` were
/// hard-coded `None` inside this function, and two of the three callers passed
/// `token: None`. So `show_time`, `show_bill_number` and `show_token` were three
/// settings that defaulted to *on* and could not do anything — while the
/// settings screen's sample passed all three, so the preview showed lines the
/// paper would never have.
///
/// A caller cannot forget what it is not asked for. The order goes in; the
/// ticket comes out with a token, a time, a KOT number and the table's real
/// name on it.
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
    // **The table's own name, never its id.** This printed
    // `tbl_outlet_default_sec_mt48cjqf_h1_` on real paper until P32.
    let table = label_for(app, table);
    let token = order.and_then(|o| o.token().map(|t| t.formatted.clone()));
    let bill_number = order.and_then(|o| o.bill_number().map(|b| b.formatted.clone()));
    let waiter = order.and_then(|o| staff_name(app, &o.core().created_by));
    let time = clock_time(at);

    // **Grouped by printer id and NOT by category**: a shop with six categories
    // and two printers wants two tickets, not six. Getting that wrong would be
    // a worse bug than the one being fixed, because a cook would read it as the
    // counter sending duplicates.
    let fallback = default_kitchen_printer(app)?;
    let categories = categories_of(app, &lines.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>());

    let mut stations: Vec<(PrinterConfig, Vec<mb_print::template::TicketLine>)> = Vec::new();
    for (item, line) in lines {
        let printer = routed_printer(app, categories.get(item.as_str()).map(String::as_str))
            .unwrap_or_else(|| fallback.clone());
        match stations.iter_mut().find(|(p, _)| p.id == printer.id) {
            Some((_, theirs)) => theirs.push(line),
            None => stations.push((printer, vec![line])),
        }
    }

    // P17: the shop's own settings, not this crate's idea of sensible ones.
    let settings = app.shop_config().kitchen;

    // The id of the FIRST ticket is what comes back, because the caller uses it
    // for one thing — telling the cashier something is printing.
    let mut id = String::new();
    for (printer, station_lines) in &stations {
        // **One number per roll of paper.** Two stations cooking two halves of
        // one order get KOT 14 and KOT 15, which is what a kitchen needs to
        // talk about them.
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
            // **Which station this roll belongs to**, on the paper. Two
            // stations printing two different halves of one order, with nothing
            // on either saying which is which, is how a cook ends up making the
            // other station's food.
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
            Job::new(JobKind::Kitchen, &printer.id, document, today(now()))
                .because(reason.clone()),
        )?;
        if id.is_empty() {
            id = queued;
        }
    }
    Ok(id)
}

/// The words a cook reads, for a set of ledger deltas.
///
/// The name comes from the cart line's frozen snapshot (crown jewel 4), and
/// falls back to the item id rather than the word "Item" — a cook can act on
/// an id, and cannot act on "Item".
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

/// **Print a kitchen ticket for an order that is already on disk** — P24's
/// paper fallback.
///
/// `print_kitchen_ticket_on` prints the delta for the cart the cashier is
/// looking at. This prints the delta for a SAVED order, which is what is needed
/// when no kitchen screen drew a ticket in time: there is no cart involved and
/// the cashier may be three orders further on.
///
/// **The kitchen must never go blind.** That is the whole reason this exists,
/// and it is why it takes an order id rather than looking at the screen.
///
/// It used to print the saved order's whole cart, which is what made a single
/// press produce two rolls of paper — see [`queue_kitchen_lines`].
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

    // **The delta, exactly like the button — crown jewel 2.**
    //
    // This used to print `core.cart`, the whole order, every time. So a table
    // that had already been cooked got a ticket repeating all of it, and the
    // press that had just printed correctly was followed twenty seconds later
    // by a second, wrong roll. Reading the ledger instead is what makes the
    // duplicate impossible rather than merely unlikely: the button marks the
    // ledger as its paper is queued, so by the time this runs there is nothing
    // left to send and nothing is printed.
    //
    // A course fired from the kitchen screen never printed paper and never
    // marked the ledger, which is exactly the case this fallback exists for —
    // and it still finds those lines.
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

    // **And the ledger remembers, on disk.** Without this the next round of the
    // timer finds the same lines pending and prints them again, every five
    // seconds, for as long as the order is open.
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

/// Print what the kitchen has not seen — **the delta, never the order.**
///
/// > Crown jewel 2: *"only what the kitchen has not seen gets printed, and what
/// > was printed is remembered **in the database**, not in the screen's
/// > memory."*
pub fn print_kitchen_ticket_on(app: &App) -> UiResult<String> {
    // One counter action at a time — see `App::begin_action`. Held for the
    // whole flow, so a second press cannot land between the read and the write.
    let _one_at_a_time = app.begin_action();
    crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    let at = now();

    // 1. What is new, from the ledger that travels with the order — so this is
    //    right after a merge, after a restart, and on a second terminal.
    let (delta, lines, order_type, table) = app.with_cart(|state| {
        // **Before the paper, not after it.** Step 2b parks the order, and
        // parking is where this rule used to be caught — by which time the
        // tickets were already queued and the cook was already cooking.
        require_a_table(state)?;
        let delta = state.cart_pending()?;
        if delta.is_empty() {
            return Err(UiError::new(
                "kitchen.nothing",
                "The kitchen already has everything on this bill.",
            )
            .quietly());
        }
        // Each line keeps its ITEM as well as its words, because which station
        // cooks it is decided by the item's category further down.
        let lines: Vec<(mb_core::ItemId, mb_print::template::TicketLine)> = delta
            .iter()
            .map(|(identity, qty)| {
                (
                    identity.item_id.clone(),
                    mb_print::template::TicketLine {
                        // The ledger stores ids; the name comes from the cart
                        // line's frozen snapshot (crown jewel 4).
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

    // 2. **Split the ticket by station** — scope 3.1, and see `routed_printer`
    //    for why this is new. Each line goes to its category's printer; every
    //    line whose category has no route of its own shares the default
    //    kitchen printer, which is the single ticket this used to always be.
    //
    //    Grouped by printer id and NOT by category: a shop with six categories
    //    and two printers wants two tickets, not six. Getting that wrong would
    //    have been a worse bug than the one being fixed, because a cook would
    //    see it as the counter sending duplicates.
    // **What the shop calls the table, in the print queue too** — P32. This
    // said `table tbl_outlet_default_none_t1_`, which is the same fault as the
    // one on the paper: the queue is a list a cashier reads.
    let reason = label_for(app, table.as_deref())
        .map_or_else(|| "kitchen ticket".to_owned(), |t| format!("table {t}"));

    // 2a. **The order goes on disk BEFORE the paper.** Same rule as
    //     `complete_bill`, and the same reason (D4): if the order is not saved
    //     first, a kitchen ticket exists for something the shop has no record
    //     of — and, until P12, there was no Open order on disk at all, so audit
    //     B6's "cancel the order" had nothing to cancel and the floor grid had
    //     nothing to show (scope 1.4).
    //
    // **It used to happen AFTER the queue, which is what left the ticket with
    // no token on it** (P32). A token is claimed when a draft becomes an open
    // order; queueing first meant there was no token to print, so this call
    // site passed `None` and `show_token` — on by default — could never do
    // anything on the ordinary path. The comment above already said the order
    // goes on disk first; now it does.
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

    log_info!("the kitchen was told about {} line(s)", delta.len());
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
    // One counter action at a time — see `App::begin_action`. Held for the
    // whole flow, so a second press cannot land between the read and the write.
    let _one_at_a_time = app.begin_action();
    let who = crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    let at = now();
    let settled_by = who.staff_id.clone();

    let (draft, bill, settlement) = app.with_cart(|state| {
        if state.cart.is_empty() {
            return Err(
                UiError::new("bill.empty", "There is nothing on this bill yet.").quietly(),
            );
        }
        // mb-db refuses this at `open_draft` (audit 2.3), which is the right
        // place for the rule but the wrong moment for the person: they have
        // already taken the money. Say it here, in words that say what to do.
        require_a_table(state)?;
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
            // **Settled, voided or cancelled: it is not ours to bill again.**
            //
            // This arm was folded into the `_` below, so an order that had
            // already been paid fell through to "open a new one" — which claims
            // a fresh bill number and upserts the settled row away. One sale
            // became two bill numbers, which for a GST book is a hole in the
            // series. `park_open_order` had answered this properly since P12;
            // settling, which is the one that takes the money, had not.
            Some(_) => {
                return Err(UiError::new(
                    "order.finished",
                    "This order has already been finished. Start a new one.",
                ));
            }
            // Opening claims the token and the bill number atomically (D6).
            None => mb_db::open_draft(&shop.db, till, draft).map_err(|e| words::from_db(&e))?,
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
    let table = order.core().table.as_ref().and_then(|t| table_label(app, t));
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
            // **P32 — the real values, resolved once, here.** The table used to
            // print as its database id and the bill had no time at all.
            table: table.as_deref(),
            time: Some(time.as_str()),
            waiter: waiter.as_deref(),
            copy: mb_print::template::Copy::Original,
            einvoice: mb_print::template::EInvoice::default(),
            // **P31.** This was `None` here and at every other call site, so
            // `receipt.logo` and `receipt.logo_width_pct` were two settings
            // pointing at a picture nothing could supply. `logo::stored`
            // returns `None` for a shop that has not chosen one and for a file
            // that will not read, so a bad logo is still a bill (D37).
            logo: crate::logo::stored(app),
        },
    )
    .map_err(|e| words::from_print(&e))?;

    app.print(Job::new(JobKind::Bill, &printer.id, document, today(now()))
                    .because(format!("bill {number}")),)?;
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

/// **Where a BILL goes.**
///
/// # It reads the role now, and that is a bug fix
///
/// This was `find(is_default).or(first)`, which ignored `role` completely — so
/// a shop that set its kitchen printer as the default, or that had only a
/// printer marked *"Kitchen tickets only"*, had every customer's bill printed
/// on the pass. The three role choices on the printer screen were collected,
/// stored, shown back, and never consulted by anything (2026-08-17).
///
/// The order is: the default, if it may print bills; else any printer that may
/// print bills; else the default whatever it is; else the first row. The last
/// two steps matter — a shop whose only printer is marked kitchen-only should
/// still get its bill on paper rather than silently nothing, because a wrong
/// setting is a thing somebody can see and fix and a missing bill is not.
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

/// **Where a KITCHEN TICKET goes, and it is the category that decides.**
///
/// # This is scope 3.1, and until 2026-08-17 it was decoration
///
/// `route_category` has written a row per category since P17, the settings
/// screen has drawn a dropdown per category, and the owner could set the
/// tandoor's food to the tandoor printer and watch it save. **Nothing ever
/// read it.** `print_kitchen_ticket_on` asked `default_printer` and sent every
/// ticket there, so a two-printer kitchen got both stations' food on one roll
/// and the setting that was supposed to fix that did nothing at all. The owner
/// found it the way it deserved to be found — *"more importently just not show
/// off, make it functional also"*.
///
/// Returns `None` when this category has no route of its own, which is the
/// ordinary case and means "the kitchen printer everything else uses".
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
///
/// The same shape and the same fix as [`default_printer`]: prefer a printer
/// that may print kitchen tickets, and only fall back to "whatever there is"
/// so that a shop with one mis-marked printer still gets paper.
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

/// **Which category each item on this ticket belongs to.**
///
/// One read of the menu rather than one per line: a sixty-line wedding order
/// would otherwise open sixty transactions on the billing path (budget B1).
fn categories_of(app: &App, items: &[mb_core::ItemId]) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Ok(menu) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).menu().list_items(OUTLET, false))
            .map_err(|e| words::from_db(&e))
    }) else {
        // A menu that will not read is not a reason to lose the ticket: every
        // line falls through to the default kitchen printer, which is where
        // they all went before routing existed at all.
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

/// **Carry the bill to the table** — the owner's fourth ask of 2026-08-17:
/// *"one small print logo inside; if any order inside, it should print the
/// bill."*
///
/// # It does not take the money, and that is the feature
///
/// An Indian restaurant settles in two steps: the party asks for the bill, the
/// waiter brings it, they read it, and *then* somebody pays at the counter.
/// Until now this counter could only do the second half — `complete_bill`
/// settles and prints in one press, so the only way to show a table what it
/// owed was to close the bill before they had paid it.
///
/// So this **prints and changes nothing**. The order stays open, the table
/// stays busy, no bill number is claimed that was not already claimed, no
/// payment is recorded, and the cash drawer does not open (`should_kick`
/// refuses anything that is not [`Copy::Original`](mb_print::template::Copy),
/// which is the right answer here for free). Press it twice and you get two
/// pieces of paper and no other difference — which is what a waiter who lost
/// the first one needs.
///
/// # Why the bill is recomputed rather than read
///
/// An open order has no `bills` row: there is no bill yet, only what one would
/// say right now. `billing::running_total` already recomputes it for the floor
/// tile, for exactly the same reason (R2 — a second money path is two answers).
/// This wants the whole [`Bill`](mb_core::Bill) rather than its total, so it
/// calls `compute_bill` the same way with the same inputs. The paper and the
/// tile therefore cannot disagree about what the table owes.
pub fn print_open_bill_on(app: &App, order_id: String) -> UiResult<String> {
    // One counter action at a time — see `App::begin_action`. Held for the
    // whole flow, so a second press cannot land between the read and the write.
    let _one_at_a_time = app.begin_action();
    let who = crate::guard::require(app, mb_auth::Permission::BillCreate)?;
    let id = mb_core::OrderId::new(order_id.clone());

    let found = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&id))
            .map_err(|e| words::from_db(&e))
    })?;

    // **Only an OPEN order.** A settled one has a real bill and its own button
    // (Bills > Reprint, which counts the copy — D7); a cancelled or voided one
    // is not something to hand anybody. Saying which is why this is not one
    // `let else`.
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
        return Err(UiError::new(
            "bill.empty",
            "There is nothing on this table's bill yet.",
        )
        .quietly());
    }

    let config = app.shop_config();
    let charges = config.billing.charges_for(open.core.order_type);
    let bill = mb_core::compute_bill(
        mb_core::BillInput::new(&open.core.cart)
            .with_order_type(open.core.order_type)
            .with_place_of_supply(config.store.place_of_supply())
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
    queue_bill_copy(app, &order, &bill, &who.name, mb_print::template::Copy::NotPaid)?;

    // **What the shop calls this table, not what the database calls it.**
    //
    // This said `table.as_str()`, which is a `TableId` — so pressing the print
    // mark raised a toast reading *"The bill for table
    // tbl_outlet_default_sec_ac_2_ is printing."* Found by pressing it in the
    // running window, which is the only place that string has ever existed:
    // every test asserted on the code, and the id is a perfectly good value
    // for `format!` to accept. Audit F8, and UI_GUIDELINES §6 — never a system
    // message, and an internal id is the purest form of one.
    //
    // A label that cannot be read is not a failure worth refusing a printed
    // bill over: the paper is already queued, so this falls back to the
    // sentence without a table in it.
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

/// What the shop calls a table. `None` rather than an error: a caller that has
/// already done the important thing should not fail over a name.
///
/// **Every piece of paper goes through here now** — P32. It used to reach a
/// toast and the kitchen screen only, so a real bill printed
/// `Table  tbl_outlet_default_sec_mt48cjqf_h1_` — the database key, 36 of the
/// 48 columns on 80 mm paper. There is no fallback to the id: a bill with no
/// table line is honest, and a bill with a primary key on it is not.
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

/// **The shop's first table, by the name the shop gave it.**
///
/// Only the settings preview wants this, and it wants it for one reason: a
/// sample that invents `"6"` cannot show a bill printing a database id, which
/// is exactly what reached the owner's printer. Going through the real list is
/// what makes the preview capable of showing the fault.
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

/// Who is signed in, by name. `None` on a first run, before anybody has a PIN.
#[must_use]
pub fn current_staff_name(app: &App) -> Option<String> {
    app.sessions().current().map(|s| s.actor.name)
}

// ---------------------------------------------------------------------------
// **Audit D6 — see the bill before it prints.** P32.
//
// > *"No bill preview before printing. You cannot see the actual bill for the
// > actual order before it comes out of the printer."*
//
// Written into `ipc.rs` as a promise at P08 and never kept. These two functions
// build the SAME document `queue_bill_print` and `queue_kitchen_lines` build —
// the same template, the same table label, the same time, the same face, the
// same engine — and hand it to the screen instead of the printer.
//
// **The same document, not a similar one.** A preview that took its own route
// is exactly the sample-versus-real gap that let a database key reach paper.
// ---------------------------------------------------------------------------

/// The bill for an order, as it would print right now.
///
/// `None` means the order on this counter's cart — the common case, and the one
/// the billing screen's Preview button uses. A settled order's id previews the
/// bill that was printed.
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
                    .transaction(|tx| mb_db::Repos::new(tx).orders().find(&mb_core::OrderId::new(&id)))
                    .map_err(|e| words::from_db(&e))
            })?;
            let order = found.ok_or_else(|| {
                UiError::new("preview.no_order", "That order is not on this counter.")
            })?;
            let bill = bill_of(app, &order, &config)?;
            (bill, order)
        }
        // **The cart's own order, when it has one** — P32, found by running it.
        //
        // A cart that has already sent a kitchen ticket is a parked order with
        // a token and a bill number on disk, and the preview was building a
        // fresh stand-in beside it: the paper would carry `TOKEN 3` and the
        // screen showed none. The cart is only a stand-in before it is parked.
        None if app.with_cart(|s| Ok(s.order_id.clone())).ok().flatten().is_some() => {
            let bill = app.with_cart(|state| state.bill(&config))?;
            let order = open_order_now(app).ok_or_else(|| {
                UiError::new("preview.no_order", "That order is not on this counter.")
            })?;
            (bill, order)
        }
        None => {
            let bill = app.with_cart(|state| state.bill(&config))?;
            // A cart is not an order yet, so it is shown as the bill a waiter
            // would carry to the table — which is what it is.
            let at = now();
            let day = today(at);
            let mut core = app.with_cart(|state| Ok(state.to_core_for_printing()))?;
            // **Today's date, not 1970.** `to_core_for_printing` stamps day
            // zero because P12 only ever wanted it for looking line names up,
            // and the first real preview printed `1970-01-01` at the top.
            core.business_day = day;
            core.created_at = at;
            let open = mb_core::OpenOrder {
                core,
                token: mb_core::Claimed {
                    value: 0,
                    // **Empty, so no token line prints at all.** A token is
                    // claimed when the order is parked; showing a placeholder
                    // would be showing a number the paper will not have.
                    formatted: String::new(),
                    business_day: day,
                },
                bill_number: mb_core::Claimed {
                    value: 0,
                    // **Not a plausible number.** This bill has not been
                    // settled and must not look as though it has.
                    formatted: "NOT YET".to_owned(),
                    business_day: day,
                },
            };
            (bill, mb_core::AnyOrder::Open(open))
        }
    };

    let table = order.core().table.as_ref().and_then(|t| table_label(app, t));
    let time = clock_time(now());
    let waiter = staff_name(app, &order.core().created_by);
    let copy = match &order {
        mb_core::AnyOrder::Settled(_) => mb_print::template::Copy::Original,
        _ => mb_print::template::Copy::NotPaid,
    };

    // **The same `metrics` the layout below uses**, so the document is built
    // for the shape it will be drawn in.
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

    let laid = mb_print::layout::layout_for(&document, &metrics)
        .map_err(|e| words::from_print(&e))?;
    Ok(crate::preview::to_preview(&laid, &metrics, engine))
}

/// The kitchen ticket that would print right now — the **delta**, exactly as
/// the button would send it (crown jewel 2).
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
                    .transaction(|tx| mb_db::Repos::new(tx).orders().find(&mb_core::OrderId::new(&id)))
                    .map_err(|e| words::from_db(&e))
            })?;
            let order = found.ok_or_else(|| {
                UiError::new("preview.no_order", "That order is not on this counter.")
            })?;
            (order.core().clone(), Some(order))
        }
        // **The CART's lines, with the ORDER's numbers.**
        //
        // The delta has to come off the cart — that is what is about to be
        // sent, and the row on disk is a moment behind it. The token and the
        // bill number have to come off the parked order, because that is where
        // they are claimed. Taking both from the order showed an empty ticket
        // for an item that had just been typed in; taking both from the cart
        // showed a ticket with no token. Found by running it (P32).
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
    let token = order.as_ref().and_then(|o| o.token().map(|t| t.formatted.clone()));
    let number = order
        .as_ref()
        .and_then(|o| o.bill_number().map(|b| b.formatted.clone()));

    let document = mb_print::template::kitchen_document(
        printer.paper,
        &mb_print::template::KitchenContext {
            kind: mb_print::template::TicketKind::New,
            token: token.as_deref(),
            bill_number: number.as_deref(),
            // **Not claimed.** A preview that took a number would burn one out
            // of the shop's own series every time somebody looked at it.
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

    let laid = mb_print::layout::layout_for(&document, &metrics)
        .map_err(|e| words::from_print(&e))?;
    Ok(crate::preview::to_preview(&laid, &metrics, engine))
}

/// **Send the kitchen the whole order again** — P32, and the one caller of
/// `reprint`.
///
/// # Why it is not the same button
///
/// "Kitchen ticket" prints the **delta** and marks the ledger, which is crown
/// jewel 2 and must stay exactly as it is. This is the other question a shop
/// actually asks: *the cook has lost the paper.* It prints everything on the
/// order, marked `*** REPRINT ***`, and **touches nothing** — the ledger is not
/// written, so the next delta is still the same delta.
///
/// The mark is the whole reason it is a separate path. A second identical
/// ticket is a second lot of food; a ticket that says it is a reprint is a
/// cook checking before they cook.
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
        return Err(UiError::new(
            "kitchen.nothing",
            "There is nothing on this bill to send.",
        )
        .quietly());
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

/// **The A4 tax invoice, as a PDF** — scope 7.10, and it had no caller.
///
/// # It was built and never reached
///
/// `mb_print::pdf` has existed since P06 and `PaperKind::A4` with it. The whole
/// path was there: one document, one layout, a PDF sink with no crate behind
/// it. **The only thing that ever called it was the report exporter** — so a
/// B2B customer asking for an invoice on A4 got a 3-inch till roll, and a
/// feature the scope calls BUILD was a library function nothing could reach
/// (P32 found it by grepping `to_pdf`).
///
/// The same `bill_document` a thermal bill uses, on A4 paper. One template:
/// a shop that changes its footer changes both.
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
    let table = order.core().table.as_ref().and_then(|t| table_label(app, t));
    let time = clock_time(order.core().created_at);
    let waiter = staff_name(app, &order.core().created_by);

    // **The printer's own metrics, not a face's.** A4 has no thermal dots, so
    // there is nothing to measure a typeface against — the PDF sink draws in
    // Courier on a fixed grid and says so.
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
            // A4 is the PDF sink's paper and it cannot draw a picture — see
            // `pdf.rs`. Handing it one would be a promise this file cannot keep.
            logo: None,
        },
    )
    .map_err(|e| words::from_print(&e))?;

    let laid = mb_print::layout::layout_for(&document, &metrics)
        .map_err(|e| words::from_print(&e))?;

    let clean: String = number
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    crate::reports::save(&format!("Invoice-{clean}.pdf"), &mb_print::pdf::to_pdf(&laid))
}

/// The bill for a stored order — recomputed from its own cart, which is what
/// every other reader of a stored order does (D4).
fn bill_of(
    app: &App,
    order: &mb_core::AnyOrder,
    config: &crate::settings::ShopConfig,
) -> UiResult<mb_core::Bill> {
    if let mb_core::AnyOrder::Settled(settled) = order {
        // A settled order carries the bill it was settled with, and that is the
        // one that was printed. Recomputing could produce a different number if
        // a price or a charge has changed since — which is exactly what crown
        // jewel 4 exists to prevent.
        return Ok(settled.bill.clone());
    }
    let _ = app;
    let core = order.core();
    let charges = config.billing.charges_for(core.order_type);
    let input = mb_core::BillInput::new(&core.cart)
        .with_order_type(core.order_type)
        .with_place_of_supply(config.store.place_of_supply())
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

/// **The clock, as a bill and a ticket want to read it.** `19:42`.
///
/// mb-print owns no clock (D5, D19), so every caller that puts a time on paper
/// formats it here — one format, and one place to change it. India is UTC+5:30
/// and this product runs on one counter in one shop, which is the same
/// assumption `BusinessDay::of` already makes.
#[must_use]
#[expect(
    clippy::integer_division,
    reason = "seconds into hours and minutes for a printed clock; no amount is involved"
)]
pub fn clock_time(at: Timestamp) -> String {
    // Minutes since midnight, local. `UtcOffset::INDIA` is the same constant
    // the business day is stamped with, so a bill's time and its day can never
    // disagree about which side of midnight they are on.
    let local = at.millis() / 1_000 + i64::from(UtcOffset::INDIA.minutes()) * 60;
    let of_day = local.rem_euclid(86_400);
    let hours = of_day / 3_600;
    let minutes = (of_day % 3_600) / 60;
    format!("{hours:02}:{minutes:02}")
}

/// **The order this counter is working on, as it now stands on disk.**
///
/// Read back after [`park_open_order`] so the kitchen ticket can carry the
/// token and the bill number the parking just claimed. `None` when there is
/// nothing on the cart or the row cannot be read — a ticket without a token is
/// a ticket, and a kitchen that gets no paper is a lost order.
fn open_order_now(app: &App) -> Option<mb_core::AnyOrder> {
    let id = app.with_cart(|state| Ok(state.order_id.clone())).ok()??;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&mb_core::OrderId::new(&id)))
            .map_err(|e| words::from_db(&e))
    })
    .ok()
    .flatten()
}

/// **The ticket's own running number** — P32.
///
/// Claimed from the same per-till, per-day series machinery the token and the
/// bill number use (`mb_db::numbering`), so a shop with two counters cannot
/// print KOT 14 twice. It resets daily, like the token: a kitchen says "KOT 14"
/// within a shift and a number in the tens of thousands is a number nobody says
/// out loud.
///
/// `None` rather than an error. A ticket without a number is a ticket; a
/// kitchen that gets no paper because a counter row was missing is a lost
/// order, and requirement 3 says the food goes out.
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

/// Who took the order, by name, for the paper. `None` when the staff row has
/// gone — a name is worth having and never worth failing a print over.
pub(crate) fn staff_name(app: &App, id: &mb_core::StaffId) -> Option<String> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).people().find_staff(OUTLET, id.as_str()))
            .map_err(|e| words::from_db(&e))
    })
    .ok()?
    .map(|s| s.name)
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
        mb_print::template::Copy::NotPaid => "bill to the table".to_owned(),
    };
    let table = order.core().table.as_ref().and_then(|t| table_label(app, t));
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
            // P31 — a duplicate is the same paper as the original (D30), so it
            // carries the same letterhead.
            logo: crate::logo::stored(app),
        },
    )
    .map_err(|e| words::from_print(&e))?;

    app.print(Job::new(JobKind::Bill, &printer.id, document, today(now())).because(because))?;
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

    // **The same guard `complete_bill` has, at the OTHER door into `open_draft`.**
    //
    // mb-db refuses a dine-in order with no table, correctly, deep inside
    // `open_draft` — and `words::from_db` turns that into *"The shop's data
    // could not be read"*, because the variant it arrives in also carries
    // "SQLITE_BUSY" and audit F8 forbids showing a cashier either one.
    //
    // `complete_bill` already said this properly and the kitchen ticket did
    // not, so the most common mistake on the whole screen — pressing Kitchen
    // ticket before typing the table number — read as a data-file failure.
    // Found by pressing it on a real install.
    app.with_cart(|state| {
        require_a_table(state)?;
        Ok(())
    })?;

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
pub fn reprint_kitchen_ticket(app: State<'_, App>) -> UiResult<String> {
    reprint_kitchen_ticket_on(&app)
}

/// Scope 7.10 — the B2B invoice, on A4, as a file a shop can email.
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
