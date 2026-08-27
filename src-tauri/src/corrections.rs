//! The four ways a shop takes something back.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::{AnyOrder, BusinessDay, Money, OrderId, StaffId};
use mb_db::repo::corrections::{Reason, Refund};
use serde::Serialize;
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

/// Above this, a void needs a second person (item 4).
const APPROVAL_KEY: &str = "bill.void.approval_above_paise";

// What the screens see.

/// One of today's bills, as the Bills list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BillRowView {
    pub order_id: String,
    pub number: String,
    pub at: String,
    pub table: Option<String>,
    pub order_type: String,
    pub total: MoneyView,
    pub cashier: Option<String>,
    /// "settled", "voided", "cancelled".
    pub state: String,
    /// Present on a voided bill, and shown.
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

// The list, and the reasons.

/// Today's bills, newest first — the way in to every flow below.
pub fn list_bills_on(app: &App) -> UiResult<Vec<BillRowView>> {
    guard::require(app, Permission::ReportsView)?;
    let day = today(now());

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let staff = repos.people().list_staff(OUTLET)?;
                let name_of =
                    |id: &StaffId| staff.iter().find(|s| s.id == *id).map(|s| s.name.clone());

                let mut out = Vec::new();
                for order in repos.orders().list_for_day(OUTLET, day)? {
                    let Some(row) = bill_row(&order, &name_of) else {
                        // A draft or an open order: it is on the floor grid, not in the day's
                        // bills.
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
                // Newest first: the bill somebody wants is nearly always the one that just
                // printed.
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
        AnyOrder::Settled(o) => ("settled", o.bill.grand_total, name_of(&o.settled_by), None),
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
        table: core.table().map(|t| t.as_str().to_owned()),
        order_type: crate::billing::order_type_label(core.order_type()).to_owned(),
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

// Void a bill.

/// Reverse a settled bill.
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

    if day_is_closed(app, settled.core.business_day)? {
        return Err(UiError::new(
            "void.day_closed",
            "That day has been closed. Record this as today's correction instead.",
        ));
    }

    // The second person.
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
                repos.stock().reverse_for_bill(
                    OUTLET,
                    &voided.core.id,
                    at,
                    day,
                    Some(&who.staff_id),
                )?;
                // The same transaction as the thing it describes.
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::BILL_VOIDED,
                        "bill",
                    )
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
                repos.kitchen().close_order(voided.core.id.as_str())?;
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
        return Err(UiError::new(
            "void.approver_wrong_pin",
            "That PIN is not right.",
        ));
    }
    Ok(())
}

fn day_is_closed(app: &App, day: BusinessDay) -> UiResult<bool> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .corrections()
                    .day_is_locked(OUTLET, day)
            })
            .map_err(|e| words::from_db(&e))
    })
}

// Cancel an open order.

/// The customer walked out.
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
    let told: Vec<(mb_core::LineIdentity, mb_core::Qty)> = open.core.kitchen.told().to_vec();
    let table = open.core.table().cloned();

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
                        serde_json::json!({ "state": "open", "table": table.as_ref().map(mb_core::TableId::as_str) }),
                        serde_json::json!({ "state": "cancelled", "reason": reason }),
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // The order is on disk and the table is free.
    if !told.is_empty()
        && let Err(e) = print_cancellation(
            app,
            &open.core,
            &told,
            table.as_ref(),
            Some(&mb_core::AnyOrder::Open(open.clone())),
        )
    {
        log_warn!("order {order_id} was cancelled but the kitchen slip failed: {e}");
    }

    // And this is the one thing on the kitchen screen that is allowed to interrupt.
    if let Err(e) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).kitchen().cancel_order(&order_id, at))
            .map_err(|e| words::from_db(&e))
    }) {
        log_warn!("order {order_id} was cancelled but the kitchen screen was not told: {e}");
    }

    log_info!("order {order_id} cancelled by {} — {reason}", who.name);
    Ok(())
}

/// Tell the kitchen to stop.
fn print_cancellation(
    app: &App,
    core: &mb_core::OrderCore,
    lines: &[(mb_core::LineIdentity, mb_core::Qty)],
    table: Option<&mb_core::TableId>,
    // The order, when the caller has one — so the slip carries the same token and bill number
    // the ticket that started the cooking did.
    order: Option<&mb_core::AnyOrder>,
) -> UiResult<()> {
    // A cancellation slip is a kitchen ticket, so it obeys the shop's kitchen-ticket settings.
    crate::flows::queue_kitchen_lines(
        app,
        mb_print::template::TicketKind::Cancellation,
        core.order_type(),
        table,
        order,
        crate::flows::ticket_lines(&core.cart, lines),
        false,
        "cancellation".to_owned(),
    )?;
    Ok(())
}

// Void one line.

/// Take one line off the order in the cart.
pub fn void_line_on(app: &App, index: usize, reason: String) -> UiResult<crate::billing::CartView> {
    // One counter action at a time — see `App::begin_action`.
    let _one_at_a_time = app.begin_action();
    let who = guard::require(app, Permission::OrderItemVoid)?;
    let at = now();
    let day = today(at);

    let (name, qty, order_id) = app.with_cart(|state| {
        let line = state.cart.lines().get(index).ok_or_else(|| {
            UiError::new("void_line.gone", "That line is not on this bill any more.")
        })?;
        Ok((
            line.snapshot.name.clone(),
            line.qty,
            state.order_id().map(str::to_owned),
        ))
    })?;

    // Off the order.
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
        Ok((cancel, state.to_core(at, &who.staff_id, app.terminal_id())))
    })?;
    // The kitchen's slip does not need a table to exist yet, so a dine-in cart with no table is
    // not refused here — there is no paper without a ledger, and no ledger without a park.
    let core = core.ok();

    // The paper, before the ledger.
    if !cancel.is_empty() {
        let Some(core) = core.as_ref() else {
            return Err(UiError::new(
                "void_line.no_table",
                "This is a dine-in order with no table, so no kitchen slip can be printed.",
            ));
        };
        if let Err(e) = print_cancellation(app, core, &cancel, core.table(), None) {
            // Deliberately not fatal, and deliberately loud: the line IS off the bill, so the
            // customer is not charged.
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
        // And only now does the ledger believe it.
        app.with_cart_mut(|state| {
            state.kitchen.mark_cancelled(&cancel).map_err(|e| {
                UiError::new(
                    "void_line.record",
                    "The cancellation could not be recorded.",
                )
                .with_detail(e.to_string())
            })
        })?;
    }

    app.record(
        &AuditEntry::new(
            at,
            day,
            Some(who.staff_id.clone()),
            action::ITEM_VOIDED,
            "order",
        )
        .about(order_id.unwrap_or_else(|| "unsaved".to_owned()))
        .changed(
            serde_json::json!({ "item": name, "qty": qty.to_string() }),
            serde_json::json!({ "removed": true, "reason": reason }),
        ),
    );

    log_info!("{name} voided off the bill by {} — {reason}", who.name);
    app.with_cart(|state| crate::billing::cart_view(state, &app.shop_config()))
}

/// Print another copy, and say so on the paper.
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

    crate::flows::queue_bill(app, &order, &bill, &who.name, marking)?;

    log_info!("bill {order_id} reprinted as copy {copy} by {}", who.name);
    Ok(format!("Copy {copy} is printing."))
}

// Refund — 8.7.

/// Record money going back to a customer.
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
                    id: format!("{}_{order_id}", crate::newid::fresh_at("ref", at)),
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
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::BILL_VOIDED,
                        "refund",
                    )
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

// The command seats.

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
