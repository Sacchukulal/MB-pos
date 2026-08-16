//! **The four ways a shop takes something back** — audit B5, B6 and D7.
//!
//! > **B5.** *"There is no way to cancel a finalised bill. Once 'Complete Bill'
//! > is pressed, that bill is in your sales and your GST forever. Every shop
//! > makes a mistake sometimes."*
//! > **B6.** *"There is no way to cancel an open (KOT'd) order from the
//! > counter… the order sits in the Processing list forever and the table stays
//! > busy."*
//! > **D7.** *"A reprinted bill is indistinguishable from the original, which
//! > is an obvious fraud opening."*
//!
//! P11 gave the counter the ability to say no. This gives it the ability to say
//! sorry, and the two are the same feature from either side: **a correction
//! nobody can trace is indistinguishable from theft.** That is why every flow
//! here carries a reason, a person and a row, and why none of them deletes
//! anything.
//!
//! # Bodies over `&App`, seats in `ipc.rs`
//!
//! D46. These are sequences — void, then refund, then reprint the voided copy —
//! and a sequence that can only be checked by clicking is a sequence that gets
//! checked once.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::{AnyOrder, BusinessDay, Money, OrderId, StaffId, Timestamp};
use mb_db::repo::corrections::{Refund, Reason};
use mb_print::queue::{Job, JobKind};
use serde::Serialize;
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

/// Above this, a void needs a second person (item 4). Zero means always;
/// absent means never. P17 gives it a screen.
const APPROVAL_KEY: &str = "bill.void.approval_above_paise";

// ---------------------------------------------------------------------------
// What the screens see.
// ---------------------------------------------------------------------------

/// One of today's bills, as the Bills list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BillRowView {
    pub order_id: String,
    pub number: String,
    /// Already formatted. R8 — TypeScript does no arithmetic, on money or time.
    pub at: String,
    pub table: Option<String>,
    pub order_type: String,
    pub total: MoneyView,
    pub cashier: Option<String>,
    /// "settled", "voided", "cancelled".
    pub state: String,
    /// Present on a voided bill, and shown. A void without its reason on screen
    /// is the same silence audit B5 complains about.
    pub void_reason: Option<String>,
    pub refunded: Option<MoneyView>,
    /// How many pieces of paper this bill has produced beyond the first.
    pub reprints: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ReasonView {
    pub id: String,
    pub text: String,
}

/// The three figures that must tie, for the screen's footer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DayTotalsView {
    pub gross: MoneyView,
    pub voids: MoneyView,
    pub net: MoneyView,
    pub refunded: MoneyView,
    pub bills: i64,
    pub voided_bills: i64,
    pub cancelled_orders: i64,
}

// ---------------------------------------------------------------------------
// The list, and the reasons.
// ---------------------------------------------------------------------------

/// Today's bills, newest first — **the way in** to every flow below.
///
/// P09's grid shows OPEN orders only, so before this a settled bill was
/// unreachable and void, reprint and refund had no door.
///
/// `reports.view` rather than `bill.create`: this is the day's takings on a
/// screen, and audit C1's first example is *"anybody can open Reports and see
/// the whole day's cash"*.
pub fn list_bills_on(app: &App) -> UiResult<Vec<BillRowView>> {
    guard::require(app, Permission::ReportsView)?;
    let day = today(now());

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let staff = repos.people().list_staff(OUTLET)?;
                let name_of = |id: &StaffId| {
                    staff
                        .iter()
                        .find(|s| s.id == *id)
                        .map(|s| s.name.clone())
                };

                let mut out = Vec::new();
                for order in repos.orders().list_for_day(OUTLET, day)? {
                    let Some(row) = bill_row(&order, &name_of) else {
                        // A draft or an open order: it is on the floor grid,
                        // not in the day's bills.
                        continue;
                    };
                    let id = OrderId::new(row.order_id.clone());
                    let reprints = repos.corrections().reprint_count(&id)?;
                    let refunded = repos.corrections().refunded_so_far(&id)?;
                    out.push(BillRowView {
                        reprints,
                        refunded: refunded.is_positive().then(|| MoneyView::from(refunded)),
                        ..row
                    });
                }
                // Newest first: the bill somebody wants is nearly always the
                // one that just printed.
                out.reverse();
                Ok(out)
            })
            .map_err(|e| words::from_db(&e))
    })
}

fn bill_row(
    order: &AnyOrder,
    name_of: &impl Fn(&StaffId) -> Option<String>,
) -> Option<BillRowView> {
    let core = order.core();
    let (state, total, cashier, void_reason) = match order {
        AnyOrder::Settled(o) => (
            "settled",
            o.bill.grand_total,
            name_of(&o.settled_by),
            None,
        ),
        AnyOrder::Voided(o) => (
            "voided",
            o.bill.grand_total,
            name_of(&o.settled_by),
            Some(o.reason.clone()),
        ),
        AnyOrder::Cancelled(o) => (
            "cancelled",
            Money::ZERO,
            name_of(&o.cancelled_by),
            Some(o.reason.clone()),
        ),
        AnyOrder::Draft(_) | AnyOrder::Open(_) => return None,
    };

    Some(BillRowView {
        order_id: core.id.as_str().to_owned(),
        number: order
            .bill_number()
            .map(|n| n.formatted.clone())
            .unwrap_or_default(),
        at: words::when(core.created_at),
        table: core.table.as_ref().map(|t| t.as_str().to_owned()),
        order_type: crate::billing::order_type_label(core.order_type).to_owned(),
        total: MoneyView::from(total),
        cashier,
        state: state.to_owned(),
        void_reason,
        refunded: None,
        reprints: 0,
    })
}

/// The shop's own reasons for one flow.
pub fn reasons_on(app: &App, kind: String) -> UiResult<Vec<ReasonView>> {
    guard::require(app, Permission::BillCreate)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).corrections().reasons(OUTLET, &kind))
            .map_err(|e| words::from_db(&e))
            .map(|list| {
                list.into_iter()
                    .map(|r: Reason| ReasonView {
                        id: r.id,
                        text: r.text,
                    })
                    .collect()
            })
    })
}

pub fn day_totals_on(app: &App) -> UiResult<DayTotalsView> {
    guard::require(app, Permission::ReportsView)?;
    let day = today(now());
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).corrections().day_totals(OUTLET, day))
            .map_err(|e| words::from_db(&e))
            .map(|t| DayTotalsView {
                gross: MoneyView::from(t.gross),
                voids: MoneyView::from(t.voids),
                net: MoneyView::from(t.net),
                refunded: MoneyView::from(t.refunded),
                bills: t.bills,
                voided_bills: t.voided_bills,
                cancelled_orders: t.cancelled_orders,
            })
    })
}

// ---------------------------------------------------------------------------
// 1. Void a bill — B5.
// ---------------------------------------------------------------------------

/// Reverse a settled bill.
///
/// **The bill keeps its number and its amounts.** `SettledOrder::void` (P03)
/// moves both across untouched, so this cannot edit history even by accident —
/// it adds a state.
///
/// **It does not touch the table.** That bill's table was vacated hours ago and
/// may have had three parties on it since. Only a cancel frees a table.
pub fn void_bill_on(
    app: &App,
    order_id: String,
    reason: String,
    approver_staff_id: Option<String>,
    approver_pin: Option<String>,
) -> UiResult<Vec<BillRowView>> {
    let who = guard::require(app, Permission::BillVoid)?;
    let at = now();
    let day = today(at);
    let id = OrderId::new(order_id.clone());

    let found = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&id))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(AnyOrder::Settled(settled)) = found else {
        return Err(UiError::new(
            "void.not_settled",
            "Only a bill that has been paid can be voided. Check the bill and try again.",
        ));
    };

    // **The day lock.** A closed day is P18's (scope 10.8) and the RULE belongs
    // with the void, so it is decided here and read from there: a void against
    // a closed day is refused, and the shop makes today's correction instead.
    if day_is_closed(app, settled.core.business_day)? {
        return Err(UiError::new(
            "void.day_closed",
            "That day has been closed. Record this as today's correction instead.",
        ));
    }

    // **The second person** — item 4, and T10.
    let total = settled.bill.grand_total;
    approve_if_needed(app, total, approver_staff_id, approver_pin)?;

    let voided = settled
        .clone()
        .void(&reason, who.staff_id.clone(), at)
        .map_err(|e| {
            UiError::new("void.refused", "This bill could not be voided.")
                .with_detail(e.to_string())
        })?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.orders().save(
                    OUTLET,
                    app.terminal_id(),
                    &AnyOrder::Voided(voided.clone()),
                )?;
                // **P25, D113 — put back exactly what was taken.**
                //
                // By NEGATING the rows this bill wrote, never by re-running the
                // recipe: voiding Tuesday's bill on Friday must return
                // Tuesday's quantities at Tuesday's costs, and if the chef
                // changed the gravy on Wednesday, re-exploding would leave the
                // rice balance permanently richer by the difference.
                repos.stock().reverse_for_bill(
                    OUTLET,
                    &voided.core.id,
                    at,
                    day,
                    Some(&who.staff_id),
                )?;
                // **R11 — the same transaction as the thing it describes.**
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(at, day, Some(who.staff_id.clone()), action::BILL_VOIDED, "bill")
                        .about(voided.bill_number.formatted.clone())
                        .changed(
                            serde_json::json!({
                                "state": "settled",
                                "total_paise": total.paise(),
                            }),
                            serde_json::json!({
                                "state": "voided",
                                "total_paise": total.paise(),
                                "reason": reason,
                            }),
                        ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    crate::log_bill!(
        voided.core.id,
        "bill {} voided by {} — {reason}",
        voided.bill_number.formatted,
        who.name
    );
    list_bills_on(app)
}

/// Does this void need a second person, and did it get one?
///
/// There is **no token and no "authorised" flag** — a one-shot token would be a
/// thing to leak, replay and forget to expire, for no gain. The approval is
/// needed at the moment of the void and nowhere else.
///
/// What it protects against: a cashier quietly voiding their own mistakes, or
/// their own takings, one bill at a time. Not an attacker — P11 handles those,
/// and somebody holding the manager's PIN has already won that argument. This
/// is a control against the ordinary, which is what almost all shrinkage is.
fn approve_if_needed(
    app: &App,
    total: Money,
    approver_staff_id: Option<String>,
    approver_pin: Option<String>,
) -> UiResult<()> {
    let threshold: Option<Money> = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).settings().get(OUTLET, APPROVAL_KEY))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(threshold) = threshold else {
        return Ok(()); // absent means never
    };
    if total.paise() < threshold.paise() {
        return Ok(());
    }

    let (Some(staff_id), Some(pin)) = (approver_staff_id, approver_pin) else {
        return Err(UiError::new(
            "void.needs_approval",
            format!(
                "A void of {} needs a manager. Ask somebody who can void bills to \
                 enter their PIN.",
                total.to_plain_string()
            ),
        ));
    };

    let pin = mb_auth::Pin::parse(&pin)
        .map_err(|e| UiError::new("auth.pin_shape", format!("{e}.")).with_detail(e.to_string()))?;

    let member = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).people().find_staff(OUTLET, &staff_id))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(member) = member else {
        return Err(UiError::new(
            "void.no_approver",
            "That person is not on this shop's staff list.",
        ));
    };
    // Approving is voiding. Somebody who may not void may not wave one through.
    if !member.permissions.has(Permission::BillVoid) {
        return Err(UiError::new(
            "void.approver_denied",
            format!("{} cannot void bills either.", member.name),
        ));
    }
    let stored = member
        .pin()
        .map_err(|e| words::from_db(&e))?
        .ok_or_else(|| {
            UiError::new(
                "void.approver_no_pin",
                format!("{} has no PIN, so they cannot approve this.", member.name),
            )
        })?;

    if !mb_auth::verify_pin(&pin, &stored) {
        return Err(UiError::new("void.approver_wrong_pin", "That PIN is not right."));
    }
    Ok(())
}

/// Scope 10.8's seam. P18 builds the day lock; this reads it.
///
/// Until then there is nothing to read and nothing is closed — which is honest
/// rather than convenient: the rule exists and its input does not yet.
fn day_is_closed(app: &App, day: BusinessDay) -> UiResult<bool> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).corrections().day_is_locked(OUTLET, day))
            .map_err(|e| words::from_db(&e))
    })
}

// ---------------------------------------------------------------------------
// 2. Cancel an open order — B6.
// ---------------------------------------------------------------------------

/// The customer walked out.
///
/// **The table frees immediately** — *"the order sits in the Processing list
/// forever and the table stays busy"* is the whole finding — and if the kitchen
/// has been told, it gets a slip telling it to stop cooking **everything**, not
/// a delta, because the whole order is off.
pub fn cancel_order_on(app: &App, order_id: String, reason: String) -> UiResult<()> {
    let who = guard::require(app, Permission::OrderCancel)?;
    let at = now();
    let day = today(at);
    let id = OrderId::new(order_id.clone());

    let found = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&id))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(AnyOrder::Open(open)) = found else {
        return Err(UiError::new(
            "cancel.not_open",
            "Only an order that is still open can be cancelled.",
        ));
    };

    // The kitchen first — while the order still says what it was told.
    let told: Vec<(mb_core::LineIdentity, mb_core::Qty)> =
        open.core.kitchen.told().to_vec();
    let table = open.core.table.as_ref().map(|t| t.as_str().to_owned());

    let cancelled = open
        .clone()
        .cancel(&reason, who.staff_id.clone(), at)
        .map_err(|e| {
            UiError::new("cancel.refused", "This order could not be cancelled.")
                .with_detail(e.to_string())
        })?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.orders().save(
                    OUTLET,
                    app.terminal_id(),
                    &AnyOrder::Cancelled(cancelled.clone()),
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::ORDER_CANCELLED,
                        "order",
                    )
                    .about(order_id.clone())
                    .changed(
                        serde_json::json!({ "state": "open", "table": table }),
                        serde_json::json!({ "state": "cancelled", "reason": reason }),
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // The order is on disk and the table is free. NOW the paper — the same
    // order of operations as `complete_bill`, and for the same reason (D4).
    if !told.is_empty()
        && let Err(e) = print_cancellation(app, &open.core, &told, table.as_deref())
    {
        log_warn!("order {order_id} was cancelled but the kitchen slip failed: {e}");
    }

    // **P24 — and this is the one thing on the kitchen screen that is allowed
    // to interrupt.** Food already cooking gets thrown away, and food not
    // started gets cooked for nobody, so the ticket does not vanish: it turns
    // red and stays until a cook presses "Got it" (D107).
    //
    // Not fatal — the order is already cancelled, and a screen that could not
    // be told is a screen somebody has to be told about in person. It is
    // logged so that conversation can start.
    if let Err(e) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .kitchen()
                    .cancel_order(&order_id, at)
            })
            .map_err(|e| words::from_db(&e))
    }) {
        log_warn!("order {order_id} was cancelled but the kitchen screen was not told: {e}");
    }

    log_info!("order {order_id} cancelled by {} — {reason}", who.name);
    Ok(())
}

/// Tell the kitchen to stop. `TicketKind::Cancellation` is P06's, already built.
fn print_cancellation(
    app: &App,
    core: &mb_core::OrderCore,
    lines: &[(mb_core::LineIdentity, mb_core::Qty)],
    table: Option<&str>,
) -> UiResult<()> {
    let printer = crate::flows::default_printer(app)?;
    // P17: a cancellation slip is a kitchen ticket, so it obeys the shop's
    // kitchen-ticket settings. A slip that looked different from every other
    // ticket would be the one the kitchen does not recognise.
    let settings = app.shop_config().kitchen;
    let ticket: Vec<mb_print::template::TicketLine> = lines
        .iter()
        .map(|(identity, qty)| mb_print::template::TicketLine {
            name: core
                .cart
                .lines()
                .iter()
                .find(|line| &line.identity() == identity)
                .map_or_else(|| identity.item_id.as_str().to_owned(), |line| {
                    line.snapshot.name.clone()
                }),
            qty: *qty,
            note: identity.note.clone(),
            modifiers: Vec::new(),
        })
        .collect();

    let ctx = mb_print::template::KitchenContext {
        kind: mb_print::template::TicketKind::Cancellation,
        token: None,
        bill_number: None,
        order_type: core.order_type,
        table,
        time: None,
        station: None,
        lines: &ticket,
        settings: &settings,
    };
    let document = mb_print::template::kitchen_document(printer.paper, &ctx)
        .map_err(|e| words::from_print(&e))?;

    app.print(Job::new(JobKind::Kitchen, &printer.id, document, today(now()))
                    .because("cancellation".to_owned()),)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. Void one line — 1.19, and P03's seam.
// ---------------------------------------------------------------------------

/// Take one line off the order in the cart.
///
/// **The order of operations is load-bearing** (and it is P03's own note):
/// remove the line, ask `over_told` what the kitchen is now cooking for
/// nobody, **print**, and only then `mark_cancelled`. Recording before the
/// paper is durable loses the cancellation silently and the kitchen carries on.
pub fn void_line_on(app: &App, index: usize, reason: String) -> UiResult<crate::billing::CartView> {
    let who = guard::require(app, Permission::OrderItemVoid)?;
    let at = now();
    let day = today(at);

    // What is being taken off, for the audit row and the slip.
    let (name, qty, order_id) = app.with_cart(|state| {
        let line = state.cart.lines().get(index).ok_or_else(|| {
            UiError::new("void_line.gone", "That line is not on this bill any more.")
        })?;
        Ok((
            line.snapshot.name.clone(),
            line.qty,
            state.order_id.clone(),
        ))
    })?;

    // 1. Off the order.
    let (cancel, core) = app.with_cart_mut(|state| {
        state.cart.remove(index).map_err(|e| {
            UiError::new("void_line.refused", "That line could not be removed.")
                .with_detail(e.to_string())
        })?;
        let cancel = state.kitchen.over_told(&state.cart).map_err(|e| {
            UiError::new(
                "void_line.kitchen",
                "The kitchen's list could not be worked out.",
            )
            .with_detail(e.to_string())
        })?;
        Ok((cancel, state.to_core_for_printing()))
    })?;

    // 2. The paper, before the ledger.
    if !cancel.is_empty() {
        let table = app.with_cart(|state| Ok(state.table_label.clone()))?;
        if let Err(e) = print_cancellation(app, &core, &cancel, table.as_deref()) {
            // Deliberately not fatal, and deliberately loud: the line IS off
            // the bill, so the customer is not charged. What failed is telling
            // the kitchen, and the cashier has to be the one who does that now.
            log_warn!("a line was voided but the kitchen slip failed: {e}");
            return Err(UiError::new(
                "void_line.no_slip",
                format!(
                    "{name} is off the bill, but the kitchen slip did not print. \
                     Tell the kitchen to stop it."
                ),
            )
            .with_detail(e.to_string()));
        }
        // 3. And only now does the ledger believe it.
        app.with_cart_mut(|state| {
            state.kitchen.mark_cancelled(&cancel).map_err(|e| {
                UiError::new("void_line.record", "The cancellation could not be recorded.")
                    .with_detail(e.to_string())
            })
        })?;
    }

    app.record(
        &AuditEntry::new(at, day, Some(who.staff_id.clone()), action::ITEM_VOIDED, "order")
            .about(order_id.unwrap_or_else(|| "unsaved".to_owned()))
            .changed(
                serde_json::json!({ "item": name, "qty": qty.to_string() }),
                serde_json::json!({ "removed": true, "reason": reason }),
            ),
    );

    log_info!("{name} voided off the bill by {} — {reason}", who.name);
    app.with_cart(|state| crate::billing::cart_view(state, &app.shop_config()))
}

// ---------------------------------------------------------------------------
// 4. Reprint — D7.
// ---------------------------------------------------------------------------

/// Print another copy, and say so on the paper.
///
/// **One template, two arguments.** `bill_document` takes a `Copy`, and there
/// is no second call site to diverge — *"v1's diverged because it had its own
/// copy of the layout"*.
///
/// **A reprint of a voided bill prints as VOIDED, not as a duplicate.** The
/// void is the more important fact about that piece of paper.
pub fn reprint_bill_on(app: &App, order_id: String, reason: String) -> UiResult<String> {
    let who = guard::require(app, Permission::BillReprint)?;
    let at = now();
    let day = today(at);
    let id = OrderId::new(order_id.clone());

    let found = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&id))
            .map_err(|e| words::from_db(&e))
    })?;

    let (bill, order, voided_reason) = match found {
        Some(AnyOrder::Settled(o)) => (o.bill.clone(), AnyOrder::Settled(o), None),
        Some(AnyOrder::Voided(o)) => {
            let reason = o.reason.clone();
            (o.bill.clone(), AnyOrder::Voided(o), Some(reason))
        }
        _ => {
            return Err(UiError::new(
                "reprint.not_a_bill",
                "There is no bill to reprint for that order.",
            ));
        }
    };

    let copy = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let copy = repos.corrections().record_reprint(
                    OUTLET,
                    &id,
                    Some(&who.staff_id),
                    Some(&reason),
                    at,
                    day,
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::BILL_REPRINTED,
                        "bill",
                    )
                    .about(order_id.clone())
                    .with_after(serde_json::json!({ "copy": copy, "reason": reason })),
                )?;
                Ok(copy)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    let marking = match voided_reason {
        Some(reason) => mb_print::template::Copy::Voided { reason },
        None => mb_print::template::Copy::Duplicate { number: copy },
    };

    crate::flows::queue_bill_copy(app, &order, &bill, &who.name, marking)?;

    log_info!("bill {order_id} reprinted as copy {copy} by {}", who.name);
    Ok(format!("Copy {copy} is printing."))
}

// ---------------------------------------------------------------------------
// 5. Refund — 8.7.
// ---------------------------------------------------------------------------

/// Record money going back to a customer.
///
/// **Recording it is this session's; making a card terminal do it is not** —
/// that is scope 8.4 and P29's, and it needs hardware nobody has yet.
///
/// The two rules — only against a voided bill, never more than came in — are
/// the repository's, because both need to read the order.
pub fn refund_on(
    app: &App,
    order_id: String,
    amount_paise: i64,
    mode: String,
    reason: String,
) -> UiResult<Vec<BillRowView>> {
    let who = guard::require(app, Permission::BillVoid)?;
    let at = now();
    let day = today(at);

    if amount_paise <= 0 {
        return Err(UiError::new(
            "refund.amount",
            "Type how much is going back to the customer.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let refund = Refund {
                    id: format!("ref_{order_id}_{}", at.millis()),
                    order_id: OrderId::new(order_id.clone()),
                    amount: Money::from_paise(amount_paise),
                    mode: mode.clone(),
                    reason: reason.clone(),
                    refunded_at: at,
                    refunded_by: Some(who.staff_id.clone()),
                };
                repos.corrections().record_refund(OUTLET, &refund, day)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(at, day, Some(who.staff_id.clone()), action::BILL_VOIDED, "refund")
                        .about(order_id.clone())
                        .with_after(serde_json::json!({
                            "amount_paise": amount_paise,
                            "mode": mode,
                            "reason": reason,
                        })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!(
        "{} refunded on {order_id} by {}",
        Money::from_paise(amount_paise).to_plain_string(),
        who.name
    );
    list_bills_on(app)
}

/// The timestamp helper the audit rows share.
#[allow(dead_code, reason = "kept beside the flows it serves")]
fn stamp() -> (Timestamp, BusinessDay) {
    let at = now();
    (at, today(at))
}

// ---------------------------------------------------------------------------
// The command seats (D46).
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_bills(app: tauri::State<'_, App>) -> UiResult<Vec<BillRowView>> {
    list_bills_on(&app)
}

#[tauri::command]
pub fn day_totals(app: tauri::State<'_, App>) -> UiResult<DayTotalsView> {
    day_totals_on(&app)
}

#[tauri::command]
pub fn reasons(app: tauri::State<'_, App>, kind: String) -> UiResult<Vec<ReasonView>> {
    reasons_on(&app, kind)
}

#[tauri::command]
pub fn void_bill(
    app: tauri::State<'_, App>,
    order_id: String,
    reason: String,
    approver_staff_id: Option<String>,
    approver_pin: Option<String>,
) -> UiResult<Vec<BillRowView>> {
    void_bill_on(&app, order_id, reason, approver_staff_id, approver_pin)
}

#[tauri::command]
pub fn cancel_order(app: tauri::State<'_, App>, order_id: String, reason: String) -> UiResult<()> {
    cancel_order_on(&app, order_id, reason)
}

#[tauri::command]
pub fn void_line(
    app: tauri::State<'_, App>,
    index: usize,
    reason: String,
) -> UiResult<crate::billing::CartView> {
    void_line_on(&app, index, reason)
}

#[tauri::command]
pub fn reprint_bill(
    app: tauri::State<'_, App>,
    order_id: String,
    reason: String,
) -> UiResult<String> {
    reprint_bill_on(&app, order_id, reason)
}

#[tauri::command]
pub fn refund_bill(
    app: tauri::State<'_, App>,
    order_id: String,
    amount_paise: i64,
    mode: String,
    reason: String,
) -> UiResult<Vec<BillRowView>> {
    refund_on(&app, order_id, amount_paise, mode, reason)
}
