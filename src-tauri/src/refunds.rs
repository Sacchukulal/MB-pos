//! Refund offers and writes share the same original-tender limits in the repository.
use mb_auth::{AuditEntry, Permission};
use mb_core::{Money, OrderId};
use serde::Serialize;
use ts_rs::TS;
use crate::{guard, state::{App, OUTLET}, words::{self, UiError, UiResult}};

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RefundTenderView {
    pub mode: String,
    pub remaining: crate::ipc::MoneyView,
    /// Exact decimal input, without currency symbols or grouping separators.
    pub input: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BillCorrectionOffer {
    pub tenders: Vec<RefundTenderView>,
    pub remaining: crate::ipc::MoneyView,
    pub needs_approval: bool,
    pub bill_total: crate::ipc::MoneyView,
    pub closed_day: bool,
    pub returned: bool,
}

pub fn offer_on(app: &App, order_id: String) -> UiResult<BillCorrectionOffer> {
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    guard::require_any(app, guard::BILL_LOOKUP_PERMISSIONS)?;
    let (tenders, total, closed_day, returned) = app.with_shop(|shop| shop.db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        let id = OrderId::new(order_id);
        let order = repos.orders().find(&id)?.ok_or_else(|| mb_db::DbError::invariant("This bill no longer exists."))?;
        let closed_day = repos.days().is_locked(OUTLET, order.core().business_day)?;
        let total = match order {
            mb_core::AnyOrder::Settled(o) => o.bill.grand_total,
            mb_core::AnyOrder::Voided(o) => o.bill.grand_total,
            _ => return Err(mb_db::DbError::invariant("Only an issued bill can be corrected.")),
        };
        Ok((repos.corrections().refundable_payments(&id)?, total, closed_day, repos.returns().contains(&id)?))
    }).map_err(|e| words::from_db(&e)))?;
    let remaining = Money::try_sum(tenders.iter().map(|(_, value)| *value))
        .map_err(|e| UiError::new("refund.amount", e.to_string()))?;
    Ok(BillCorrectionOffer {
        tenders: tenders.into_iter().filter(|(_, value)| value.is_positive()).map(|(mode, value)| RefundTenderView {
            mode, remaining: value.into(), input: value.to_plain_string(),
        }).collect(),
        remaining: remaining.into(),
        needs_approval: crate::corrections::approval_needed_on(app, total)?,
        bill_total: total.into(),
        closed_day, returned,
    })
}

pub fn parse_amounts(amounts: Vec<(String, String)>) -> UiResult<Vec<(String, Money)>> {
    let mut parsed = Vec::new();
    for (mode, typed) in amounts {
        if typed.trim().is_empty() { continue; }
        let amount = Money::parse(typed.trim()).map_err(|e| UiError::new("refund.amount", "Enter a valid return amount.").with_detail(e.to_string()))?;
        if !amount.is_positive() {
            return Err(UiError::new("refund.amount", "Each return amount must be greater than zero."));
        }
        let mode = mode.trim().to_ascii_lowercase();
        if parsed.iter().any(|(prior, _)| prior == &mode) {
            return Err(UiError::new("refund.mode", "Enter each payment method only once."));
        }
        parsed.push((mode, amount));
    }
    if parsed.is_empty() { return Err(UiError::new("refund.amount", "Enter the money being returned.")); }
    Ok(parsed)
}

/// Caller owns the transaction, so a failed tender rolls back every refund and the void.
pub fn record(
    repos: &mb_db::Repos<'_>, id: &OrderId, amounts: &[(String, Money)],
    reason: &str, who: &mb_auth::Actor, at: mb_core::Timestamp, request_id: &str,
) -> Result<(), mb_db::DbError> {
    let day = crate::flows::today(at);
    let reason = reason.trim();
    if reason.is_empty() { return Err(mb_db::DbError::invariant("A return needs a reason.")); }
    let mut normalized: Vec<(String, Money)> = Vec::new();
    for (mode, amount) in amounts {
        let mode = mode.trim().to_ascii_lowercase();
        if !amount.is_positive() || normalized.iter().any(|(prior, _)| prior == &mode) {
            return Err(mb_db::DbError::invariant("Return amounts must be positive, with each payment method entered once."));
        }
        normalized.push((mode, *amount));
    }
    let amounts = normalized.as_slice();
    if request_id.is_empty() || request_id.len() > 100 || !request_id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return Err(mb_db::DbError::invariant("The refund request ID is invalid."));
    }
    let prefix = format!("ref_{request_id}_");
    let prior = repos.corrections().refunds_for(id)?.into_iter().filter(|r| r.id.starts_with(&prefix)).collect::<Vec<_>>();
    if !prior.is_empty() {
        if prior.len() == amounts.len() && amounts.iter().all(|(mode, amount)| prior.iter().any(|r|
            r.mode == *mode && r.amount == *amount && r.reason == reason && r.refunded_by.as_ref() == Some(&who.staff_id))) { return Ok(()); }
        return Err(mb_db::DbError::invariant("This refund request was already used with different details. Reopen the bill."));
    }
    for (mode, amount) in amounts {
        let refund = mb_db::repo::corrections::Refund {
            id: format!("{prefix}{mode}"), order_id: id.clone(), amount: *amount,
            mode: mode.clone(), reason: reason.to_owned(), refunded_at: at,
            refunded_by: Some(who.staff_id.clone()),
        };
        repos.corrections().record_refund(OUTLET, &refund, day)?;
        repos.audit().append(OUTLET, &AuditEntry::new(at, day, Some(who.staff_id.clone()),
            mb_auth::audit::action::BILL_VOIDED, "refund").about(id.as_str()).with_after(
                serde_json::json!({"amount_paise": amount.paise(), "mode": mode, "reason": reason})))?;
    }
    Ok(())
}

#[cfg(test)]
pub fn refund_batch_on(app: &App, order_id: String, amounts: Vec<(String, Money)>, reason: String) -> UiResult<()> {
    refund_batch_with_id_on(app, order_id, amounts, reason, crate::newid::fresh_at("return", crate::flows::now()))
}

pub fn refund_batch_with_id_on(app: &App, order_id: String, amounts: Vec<(String, Money)>, reason: String, request_id: String) -> UiResult<()> {
    let _action = app.begin_action();
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    let who = guard::require(app, Permission::BillVoid)?;
    let at = crate::flows::now();
    if amounts.is_empty() || reason.trim().is_empty() { return Err(UiError::new("refund.reason", "Choose a reason and a return amount.")); }
    if let Some(error) = crate::dayclose::day_refusal_on(app, crate::flows::today(at), "refund.day_closed", "record this return")? { return Err(error); }
    app.with_shop(|shop| shop.db.transaction(|tx|
        record(&mb_db::Repos::new(tx), &OrderId::new(order_id), &amounts, &reason, &who, at, &request_id)
    ).map_err(|e| words::from_db(&e)))
}

#[tauri::command]
pub fn bill_correction_offer(app: tauri::State<'_, App>, order_id: String) -> UiResult<BillCorrectionOffer> { offer_on(&app, order_id) }

#[tauri::command]
pub fn return_bill_money(app: tauri::State<'_, App>, order_id: String, amounts: Vec<(String, String)>, reason: String, request_id: String) -> UiResult<()> {
    refund_batch_with_id_on(&app, order_id, parse_amounts(amounts)?, reason, request_id)
}

#[tauri::command]
pub fn void_and_return_bill(app: tauri::State<'_, App>, order_id: String, amounts: Vec<(String, String)>, reason: String,
    approver_staff_id: Option<String>, approver_pin: Option<String>) -> UiResult<()> {
    let amounts = if amounts.is_empty() { Vec::new() } else { parse_amounts(amounts)? };
    crate::corrections::void_bill_with_returns_on(&app, order_id, reason, approver_staff_id, approver_pin, Some(amounts)).map(|_| ())
}

pub fn return_closed_bill_on(app: &App, order_id: String, amounts: Vec<(String, Money)>, reason: String,
    request_id: String, approver_staff_id: Option<String>, approver_pin: Option<String>) -> UiResult<()> {
    let _action = app.begin_action();
    let who = guard::require(app, Permission::BillVoid)?;
    let offer = offer_on(app, order_id.clone())?;
    crate::corrections::approve_if_needed(app, Money::from_paise(offer.bill_total.paise), approver_staff_id, approver_pin)?;
    let at = crate::flows::now();
    let id = OrderId::new(order_id);
    app.with_shop(|shop| shop.db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        let created = repos.returns().record_full(OUTLET, &mb_db::repo::returns::FullReturn {
            id: format!("return_{request_id}"), order_id: id.clone(), business_day: crate::flows::today(at),
            terminal_id: app.terminal_id().to_owned(), at, by: who.staff_id.clone(), reason: reason.clone(),
        })?;
        // A first request can record a full return with no cash handed back yet. Retrying
        // that same request must not turn it into a different payment later.
        if !created && !amounts.is_empty() {
            let prefix = format!("ref_{request_id}_");
            let previously_paid = repos.corrections().refunds_for(&id)?.iter().any(|refund| refund.id.starts_with(&prefix));
            if !previously_paid {
                return Err(mb_db::DbError::invariant("This return request was already used with different payment details. Reopen the bill."));
            }
        }
        record(&repos, &id, &amounts, &reason, &who, at, &request_id)?;
        if created {
            repos.audit().append(OUTLET, &AuditEntry::new(at, crate::flows::today(at), Some(who.staff_id.clone()),
                mb_auth::audit::action::BILL_VOIDED, "return").about(id.as_str()).with_after(
                    serde_json::json!({"reason":reason,"original_bill_unchanged":true,"stock":"not_restocked"})))?;
        }
        Ok(())
    }).map_err(|e| words::from_db(&e)))?;
    app.push(crate::state::Pushed::Floor);
    Ok(())
}

#[tauri::command]
pub fn return_closed_bill(app: tauri::State<'_, App>, order_id: String, amounts: Vec<(String, String)>, reason: String,
    request_id: String, approver_staff_id: Option<String>, approver_pin: Option<String>) -> UiResult<()> {
    let amounts = if amounts.is_empty() { Vec::new() } else { parse_amounts(amounts)? };
    return_closed_bill_on(&app, order_id, amounts, reason, request_id, approver_staff_id, approver_pin)
}
