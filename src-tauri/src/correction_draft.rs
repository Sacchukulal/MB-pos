//! Leaving an issued-bill correction without replacing the issued sale.
//! Payments and kitchen work already performed are never silently thrown away.

use mb_auth::{Permission, audit::{AuditEntry, action}};
use mb_core::{AnyOrder, OrderId};
use crate::{guard, state::{App, OUTLET}, words::{self, UiError, UiResult}};

use crate::corrections::APPROVAL_KEY;

/// An issued bill's working copy still belongs to Reports. New orders remain free.
/// Keep this conditional check separate from parking/discarding so expiry cannot trap
/// a cashier on an old draft instead of allowing a new bill.
pub(crate) fn require_edit_access(app: &App) -> UiResult<()> {
    let editing = app.with_cart(|state| Ok(state.account.revision > 0 && state.account.billed_into.is_none()))?;
    if editing { crate::licensing::gate(app, mb_license::Feature::Reports)?; }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CorrectionSaveView {
    pub cart: crate::billing::CartView,
    pub proposal_token: String,
    pub original_total: crate::ipc::MoneyView,
    pub new_total: crate::ipc::MoneyView,
    pub needs_approval: bool,
    pub approvers: Vec<crate::lan::PersonPick>,
}

/// The exact issued bill and approval policy authorized before any new receipt is taken.
#[derive(Debug)]
pub struct Authorization {
    issued: AnyOrder,
    threshold: Option<mb_core::Money>,
    by: mb_core::StaffId,
    manager: bool,
}

fn proposal(app: &App) -> UiResult<(AnyOrder, mb_core::Money, Option<mb_core::Money>)> {
    let (id, amount) = app.with_cart(|state| Ok((state.order_id().map(str::to_owned), state.bill(&app.shop_config())?.grand_total)))?;
    let id = id.ok_or_else(|| UiError::new("revert.not_editing", "Open an issued bill's correction first."))?;
    app.with_shop(|shop| shop.db.read_transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        let issued = repos.orders().find(&OrderId::new(id))?
            .filter(|order| matches!(order, AnyOrder::Settled(_)))
            .ok_or_else(|| mb_db::DbError::invariant("The issued bill has changed. Open it again."))?;
        Ok((issued, amount, repos.settings().get(OUTLET, APPROVAL_KEY)?))
    }).map_err(|error| words::from_db(&error)))
}

fn needs_manager(issued: &AnyOrder, amount: mb_core::Money, threshold: Option<mb_core::Money>) -> bool {
    let original = match issued { AnyOrder::Settled(bill) => bill.bill.grand_total, _ => amount };
    threshold.is_some_and(|limit| original.max(amount) >= limit)
}

pub fn preview_on(app: &App) -> UiResult<CorrectionSaveView> {
    let _one_at_a_time = app.begin_action();
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    let actor = guard::require(app, Permission::BillRevert)?;
    app.refresh_open_cart()?;
    let (issued, amount, threshold) = proposal(app)?;
    let original = match &issued { AnyOrder::Settled(bill) => bill.bill.grand_total, _ => amount };
    let approvers = app.with_shop(|shop| shop.db.read_transaction(|tx| {
        Ok(mb_db::Repos::new(tx).people().list_staff(OUTLET)?.into_iter()
            .filter(|person| person.status == mb_db::repo::people::StaffStatus::Active
                && person.permissions.has(Permission::BillVoid) && person.id != actor.staff_id)
            .map(|person| crate::lan::PersonPick { id: person.id.as_str().to_owned(), name: person.name }).collect())
    }).map_err(|error| words::from_db(&error)))?;
    let cart = app.with_cart(|state| crate::billing::cart_view(state, &app.shop_config()))?;
    Ok(CorrectionSaveView { cart, proposal_token: proposal_token(app)?, original_total: original.into(), new_total: amount.into(), needs_approval: needs_manager(&issued, amount, threshold), approvers })
}

pub fn proposal_token(app: &App) -> UiResult<String> {
    app.with_cart(|state| serde_json::to_string(&(state.to_core(crate::flows::now(), &mb_core::StaffId::new("preview"), app.terminal_id())?,
        &state.customer, state.bill(&app.shop_config())?))
        .map_err(|error| UiError::new("revert.preview", "The correction could not be reviewed.").with_detail(error.to_string())))
}

pub fn authorize_on(app: &App, approver_id: Option<String>, pin: Option<String>) -> UiResult<Option<Authorization>> {
    let editing = app.with_cart(|state| Ok(state.account.revision > 0 && state.account.billed_into.is_none()))?;
    if !editing { return Ok(None); }
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    let actor = guard::require(app, Permission::BillRevert)?;
    let (issued, amount, threshold) = proposal(app)?;
    let manager = needs_manager(&issued, amount, threshold);
    let by = if manager {
        let (Some(id), Some(pin)) = (approver_id, pin) else {
            return Err(UiError::new("void.needs_approval", "This corrected bill needs a manager's approval before saving."));
        };
        if id == actor.staff_id.as_str() {
            return Err(UiError::new("void.second_person", "Another authorized manager must approve this correction."));
        }
        crate::corrections::verify_approver_on(app, &id, &pin)?;
        mb_core::StaffId::new(id)
    } else { actor.staff_id };
    Ok(Some(Authorization { issued, threshold, by, manager }))
}

impl Authorization {
    pub fn record(&self, repos: &mb_db::Repos<'_>, at: mb_core::Timestamp) -> Result<(), mb_db::DbError> {
        let id = &self.issued.core().id;
        let threshold: Option<mb_core::Money> = repos.settings().get(OUTLET, APPROVAL_KEY)?;
        if threshold != self.threshold || repos.orders().find(id)?.as_ref() != Some(&self.issued) {
            return Err(mb_db::DbError::invariant("The bill or approval policy changed. Review the correction again."));
        }
        if self.manager {
            let approver = repos.people().find_staff(OUTLET, self.by.as_str())?;
            if !approver.is_some_and(|person| person.status == mb_db::repo::people::StaffStatus::Active && person.permissions.has(Permission::BillVoid)) {
                return Err(mb_db::DbError::invariant("The manager can no longer approve this correction."));
            }
        }
        let revert = repos.corrections().reverts_of(id)?.into_iter().last()
            .ok_or_else(|| mb_db::DbError::invariant("This correction has no history entry."))?;
        if revert.approved_at.is_none() {
            repos.corrections().approve_revert(&revert.id, &self.by, at)?;
        }
        Ok(())
    }
}

#[tauri::command]
pub fn correction_save_preview(app: tauri::State<'_, App>) -> UiResult<CorrectionSaveView> {
    preview_on(&app)
}

pub fn save_on(app: &App) -> UiResult<()> {
    let _one_at_a_time = app.begin_action();
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    guard::require(app, Permission::BillRevert)?;
    let correction = app.with_cart(|state| Ok(state.bill_number.is_some() && state.account.revision > 0 && state.account.billed_into.is_none()))?;
    if !correction {
        return Err(UiError::new("revert.not_editing", "Open an issued bill's correction first."));
    }
    crate::flows::park_current(app)
}

#[tauri::command]
pub fn save_bill_correction_draft(app: tauri::State<'_, App>) -> UiResult<()> {
    save_on(&app)
}

pub fn discard_on(app: &App, order_id: String) -> UiResult<crate::billing::CartView> {
    let _one_at_a_time = app.begin_action();
    let who = guard::require(app, Permission::BillRevert)?;
    let id = OrderId::new(order_id);
    let local = app.with_cart(|state| Ok(state.clone()))?;
    if local.order_id() != Some(id.as_str()) {
        return Err(UiError::new("revert.not_current", "Open this correction before discarding its changes."));
    }
    let at = crate::flows::now();
    app.with_shop(|shop| shop.db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        let Some(AnyOrder::Settled(issued)) = repos.orders().find(&id)? else {
            return Ok(Err(UiError::new("revert.not_issued", "This bill is no longer available to restore. Open it again.")));
        };
        let Some(AnyOrder::Open(working)) = repos.orders().find_working(&id)? else {
            return Ok(Err(UiError::new("revert.not_editing", "This bill has no unfinished correction.")));
        };
        if local.origin.as_ref().and_then(|origin| origin.baseline.as_ref()) != Some(&working.core) {
            return Ok(Err(crate::billing::order_conflict()));
        }
        let original_receipts = issued.clone().reopen().core.billing.settlement;
        if local.settlement != original_receipts || working.core.billing.settlement != original_receipts {
            return Ok(Err(UiError::new("revert.money_changed", "Money has been recorded during this correction. Finish the correction so the payment or return stays accounted for.")));
        }
        if local.kitchen != issued.core.kitchen || working.core.kitchen != issued.core.kitchen {
            return Ok(Err(UiError::new("revert.kitchen_changed", "The kitchen has already been told about this correction. Finish it, or restore the items and send the kitchen update before saving.")));
        }
        repos.orders().finish_edit(&id)?;
        // Nothing was issued by this draft, so it does not need a later financial review.
        if let Some(revert) = repos.corrections().reverts_of(&id)?.into_iter().last()
            && revert.approved_at.is_none()
        {
            repos.corrections().approve_revert(&revert.id, &who.staff_id, at)?;
        }
        repos.audit().append(OUTLET, &AuditEntry::new(at, crate::flows::today(at), Some(who.staff_id.clone()), action::BILL_REVERTED, "bill")
            .about(issued.bill_number.formatted)
            .changed(serde_json::json!({ "state": "editing" }), serde_json::json!({ "state": "settled", "correction": "discarded", "issued_bill_unchanged": true })))?;
        Ok(Ok(()))
    }).map_err(|error| words::from_db(&error)))??;
    let kind = crate::billing::starting_order_type(&app.shop_config(), local.order_type());
    app.with_cart_mut(|state| { *state = crate::billing::CartState::new_order(kind); Ok(()) })?;
    app.push(crate::state::Pushed::Floor);
    app.with_cart(|state| crate::billing::cart_view(state, &app.shop_config()))
}

#[tauri::command]
pub fn discard_bill_correction(app: tauri::State<'_, App>, order_id: String) -> UiResult<crate::billing::CartView> {
    discard_on(&app, order_id)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "tests: expect is the assertion")]
    use super::*;
    use crate::signin_tests::Scratch;

    fn editing(scratch: &Scratch) -> (App, String, AnyOrder) {
        let app = crate::licence_tests::a_trading_shop(scratch, "discard");
        crate::licence_tests::a_bill_is_taken(&app);
        let id = crate::corrections::list_bills_on(&app).expect("receipt")[0].order_id.clone();
        let before = crate::flows::find_order(&app, &OrderId::new(&id)).expect("read").expect("issued");
        crate::corrections::revert_bill_on(&app, id.clone(), "Fix quantity".to_owned(), None, None).expect("edit");
        (app, id, before)
    }

    #[test]
    fn discarding_an_unsent_edit_preserves_the_issued_bill_and_receipts() {
        let scratch = Scratch::new("discard_safe_edit");
        let (app, id, before) = editing(&scratch);
        app.with_cart_mut(|state| {
            state.cart.set_qty(0, mb_core::Qty::from_whole(3).expect("quantity")).expect("change");
            Ok(())
        }).expect("cart");
        save_on(&app).expect("save draft");
        let cart = discard_on(&app, id.clone()).expect("discard");
        assert!(cart.is_empty && !cart.is_correction);
        assert_eq!(crate::flows::find_order(&app, &OrderId::new(id)).expect("read"), Some(before));
    }

    #[test]
    fn discarding_cannot_lose_a_new_payment() {
        let scratch = Scratch::new("discard_payment");
        let (app, id, _) = editing(&scratch);
        app.with_cart_mut(|state| {
            state.settlement.add(mb_core::Payment::new(mb_core::PaymentMode::Cash, mb_core::Money::from_paise(100)).expect("payment")).expect("receipt");
            Ok(())
        }).expect("cart");
        assert_eq!(discard_on(&app, id).expect_err("keep recorded money").code, "revert.money_changed");
        assert!(app.with_cart(|state| Ok(state.account.revision > 0)).expect("draft still here"));
    }

    #[test]
    fn discarding_cannot_erase_an_update_already_sent_to_the_kitchen() {
        let scratch = Scratch::new("discard_kitchen");
        let (app, id, _) = editing(&scratch);
        crate::flows::print_kitchen_ticket_on(&app).expect("send ticket");
        assert_eq!(discard_on(&app, id).expect_err("keep kitchen update").code, "revert.kitchen_changed");
    }

    #[test]
    fn final_approval_covers_the_new_amount_and_refusal_takes_no_payment() {
        let scratch = Scratch::new("final_correction_approval");
        let (app, id, before) = editing(&scratch);
        crate::signin_tests::hire(&app, "manager_final", "Manager", mb_auth::RolePreset::Manager, "2468");
        app.with_shop(|shop| shop.db.transaction(|tx| {
            mb_db::Repos::new(tx).settings().set(OUTLET, APPROVAL_KEY,
                &mb_core::Money::from_paise(6_000), crate::flows::now(), None)
        }).map_err(|error| words::from_db(&error))).expect("threshold");
        app.with_cart_mut(|state| {
            state.cart.set_qty(0, mb_core::Qty::from_whole(4).expect("quantity")).expect("change");
            Ok(())
        }).expect("cart");
        let preview = preview_on(&app).expect("review");
        assert!(preview.needs_approval, "new total exceeds threshold even though original did not");
        let receipts = app.with_cart(|state| Ok(state.settlement.clone())).expect("receipts");
        let refused = crate::flows::complete_bill_on(&app, Some("Cash".to_owned())).expect_err("approval required");
        assert_eq!(refused.code, "void.needs_approval");
        assert_eq!(app.with_cart(|state| Ok(state.settlement.clone())).expect("receipts"), receipts);
        let issued = app.with_shop(|shop| shop.db.read_transaction(|tx| mb_db::Repos::new(tx).orders().find(&OrderId::new(&id)))
            .map_err(|error| words::from_db(&error))).expect("issued bill");
        assert_eq!(issued, Some(before));
        crate::flows::complete_bill_authorized_on(&app, Some("Cash".to_owned()), None, None,
            Some("manager_final".to_owned()), Some("2468".to_owned()), Some(preview.proposal_token)).expect("single approved save");
        let detail = crate::corrections::bill_detail_on(&app, id).expect("history");
        assert_eq!(detail.edits.last().and_then(|edit| edit.approved_by.as_deref()), Some("Manager"));
        assert!(!detail.can_approve);
    }

    #[test]
    fn lowering_the_corrected_total_does_not_avoid_approval_on_the_original_amount() {
        let scratch = Scratch::new("final_original_threshold");
        let (app, _, _) = editing(&scratch);
        app.with_shop(|shop| shop.db.transaction(|tx| mb_db::Repos::new(tx).settings().set(
            OUTLET, APPROVAL_KEY, &mb_core::Money::from_paise(4_000), crate::flows::now(), None))
            .map_err(|error| words::from_db(&error))).expect("threshold");
        app.with_cart_mut(|state| {
            state.cart.set_qty(0, mb_core::Qty::ONE).expect("change"); Ok(())
        }).expect("cart");
        assert!(preview_on(&app).expect("review").needs_approval);
    }

    #[test]
    fn a_changed_proposal_requires_a_fresh_review_before_money_is_taken() {
        let scratch = Scratch::new("stale_correction_review");
        let (app, _, _) = editing(&scratch);
        let preview = preview_on(&app).expect("review");
        let receipts = app.with_cart(|state| Ok(state.settlement.clone())).expect("receipts");
        app.with_cart_mut(|state| {
            state.cart.set_qty(0, mb_core::Qty::from_whole(4).expect("qty")).expect("change"); Ok(())
        }).expect("cart");
        let refusal = crate::flows::complete_bill_authorized_on(&app, Some("Cash".to_owned()), None, None,
            None, None, Some(preview.proposal_token)).expect_err("review changed");
        assert_eq!(refusal.code, "revert.review_changed");
        assert_eq!(app.with_cart(|state| Ok(state.settlement.clone())).expect("receipts"), receipts);
    }
}
