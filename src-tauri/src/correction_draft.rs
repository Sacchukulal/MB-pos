//! Leaving an issued-bill correction without replacing the issued sale.
//! Payments and kitchen work already performed are never silently thrown away.

use mb_auth::{Permission, audit::{AuditEntry, action}};
use mb_core::{AnyOrder, OrderId};
use crate::{guard, state::{App, OUTLET}, words::{self, UiError, UiResult}};

/// An issued bill's working copy still belongs to Reports. New orders remain free.
/// Keep this conditional check separate from parking/discarding so expiry cannot trap
/// a cashier on an old draft instead of allowing a new bill.
pub(crate) fn require_edit_access(app: &App) -> UiResult<()> {
    let editing = app.with_cart(|state| Ok(state.account.revision > 0 && state.account.billed_into.is_none()))?;
    if editing {
        crate::licensing::gate(app, mb_license::Feature::Reports)?;
        guard::require(app, Permission::BillRevert)?;
    }
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

/// The issued bill checked before taking receipts. Staff review happens separately.
#[derive(Debug)]
pub struct Authorization {
    issued: AnyOrder,
    owner: Option<mb_core::StaffId>,
}

fn proposal(app: &App) -> UiResult<(AnyOrder, mb_core::Money)> {
    let (id, amount) = app.with_cart(|state| Ok((state.order_id().map(str::to_owned), state.bill(&app.shop_config())?.grand_total)))?;
    let id = id.ok_or_else(|| UiError::new("revert.not_editing", "Open an issued bill's correction first."))?;
    app.with_shop(|shop| shop.db.read_transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        let issued = repos.orders().find(&OrderId::new(id))?
            .filter(|order| matches!(order, AnyOrder::Settled(_)))
            .ok_or_else(|| mb_db::DbError::invariant("The issued bill has changed. Open it again."))?;
        Ok((issued, amount))
    }).map_err(|error| words::from_db(&error)))
}

pub fn preview_on(app: &App) -> UiResult<CorrectionSaveView> {
    let _one_at_a_time = app.begin_action();
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    guard::require(app, Permission::BillRevert)?;
    app.refresh_open_cart()?;
    let (issued, amount) = proposal(app)?;
    let original = match &issued { AnyOrder::Settled(bill) => bill.bill.grand_total, _ => amount };
    let cart = app.with_cart(|state| crate::billing::cart_view(state, &app.shop_config()))?;
    // A cashier completes and prints now; a reviewer signs off in Reports later.
    // Keep these wire fields for older screens, but never request a PIN to save.
    Ok(CorrectionSaveView { cart, proposal_token: proposal_token(app)?, original_total: original.into(), new_total: amount.into(), needs_approval: false, approvers: Vec::new() })
}

pub fn proposal_token(app: &App) -> UiResult<String> {
    app.with_cart(|state| serde_json::to_string(&(state.to_core(crate::flows::now(), &mb_core::StaffId::new("preview"), app.terminal_id())?,
        &state.customer, state.bill(&app.shop_config())?))
        .map_err(|error| UiError::new("revert.preview", "The correction could not be reviewed.").with_detail(error.to_string())))
}

pub fn authorize_on(app: &App, _approver_id: Option<String>, _pin: Option<String>) -> UiResult<Option<Authorization>> {
    let editing = app.with_cart(|state| Ok(state.account.revision > 0 && state.account.billed_into.is_none()))?;
    if !editing { return Ok(None); }
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    let actor = guard::require(app, Permission::BillRevert)?;
    let (issued, _) = proposal(app)?;
    let owner = app.sessions().current().is_some_and(|session| session.is_owner()).then_some(actor.staff_id);
    Ok(Some(Authorization { issued, owner }))
}

impl Authorization {
    pub fn record(&self, repos: &mb_db::Repos<'_>, at: mb_core::Timestamp) -> Result<(), mb_db::DbError> {
        let id = &self.issued.core().id;
        if repos.orders().find(id)?.as_ref() != Some(&self.issued) {
            return Err(mb_db::DbError::invariant("The bill changed. Review the correction again."));
        }
        let revert = repos.corrections().reverts_of(id)?.into_iter().last()
            .ok_or_else(|| mb_db::DbError::invariant("This correction has no history entry."))?;
        if let Some(by) = &self.owner {
            repos.corrections().approve_revert(&revert.id, by, at)?;
            repos.audit().append(OUTLET, &AuditEntry::new(at, crate::flows::today(at), Some(by.clone()), action::REVERT_APPROVED, "bill")
                .about(self.issued.bill_number().map(|number| number.formatted.as_str()).unwrap_or(id.as_str()))
                .with_after(serde_json::json!({ "revert": revert.id, "owner_completed_correction": true })))?;
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
            repos.corrections().discard_revert(&revert.id, &who.staff_id, at)?;
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
    fn cashier_corrections_print_now_and_require_separate_review_regardless_of_void_threshold() {
        for threshold in [None, Some(1), Some(999_999)] {
            let scratch = Scratch::new("cashier_pending_review");
            let app = crate::signin_tests::a_trading_shop(&scratch);
            crate::signin_tests::hire(&app, "cashier", "Cashier", mb_auth::RolePreset::Cashier, "1357");
            if let Some(limit) = threshold {
                app.with_shop(|shop| shop.db.transaction(|tx| mb_db::Repos::new(tx).settings().set(
                    OUTLET, crate::corrections::APPROVAL_KEY, &mb_core::Money::from_paise(limit), crate::flows::now(), None))
                    .map_err(|error| words::from_db(&error))).expect("void threshold");
            }
            crate::signin_tests::order_teas(&app, 2);
            crate::flows::complete_bill_on(&app, Some("Cash".into())).expect("original bill");
            let id = crate::corrections::list_bills_on(&app).expect("bills")[0].order_id.clone();
            crate::ipc::login_on(&app, "cashier".into(), "1357".into()).expect("cashier");
            crate::corrections::revert_bill_on(&app, id.clone(), "Wrong quantity".into(), None, None).expect("edit");
            app.with_cart_mut(|state| { state.cart.set_qty(0, mb_core::Qty::ONE).expect("quantity"); Ok(()) }).expect("change");
            let preview = preview_on(&app).expect("review final amounts");
            assert!(!preview.needs_approval);
            let (completed, prints) = crate::signin_tests::slips_taken(&app, ||
                crate::flows::complete_bill_with_return_on(&app, None, Some("Cash".into())));
            completed.expect("finish and return difference without PIN");
            assert!(!prints.is_empty(), "corrected receipt is queued");
            let detail = crate::corrections::bill_detail_on(&app, id.clone()).expect("history");
            assert_eq!(detail.row.approval.as_deref(), Some("waiting"));
            assert!(detail.edits[0].approved_by.is_none());
            assert!(!detail.can_approve);
            assert_eq!(crate::corrections::approve_revert_on(&app, detail.edits[0].id.clone()).expect_err("cashier cannot approve").code, "auth.denied");
            crate::corrections::reprint_bill_on(&app, id.clone(), "Customer copy".into()).expect("pending bill can be reprinted");
            crate::ipc::login_on(&app, "staff_owner".into(), "2468".into()).expect("owner");
            crate::corrections::approve_revert_on(&app, detail.edits[0].id.clone()).expect("later review");
            let approved = crate::corrections::bill_detail_on(&app, id).expect("approved");
            assert_eq!(approved.row.approval.as_deref(), Some("approved"));
            assert_eq!(approved.edits[0].approved_by.as_deref(), Some("Sachin"));
        }
    }

    #[test]
    fn reopening_and_discarding_cannot_hide_or_approve_an_earlier_pending_edit() {
        let scratch = Scratch::new("repeat_pending_review");
        let app = crate::signin_tests::a_trading_shop(&scratch);
        crate::signin_tests::hire(&app, "cashier", "Cashier", mb_auth::RolePreset::Cashier, "1357");
        crate::signin_tests::order_teas(&app, 2);
        crate::flows::complete_bill_on(&app, Some("Cash".into())).expect("issued");
        let id = crate::corrections::list_bills_on(&app).expect("bills")[0].order_id.clone();
        crate::ipc::login_on(&app, "cashier".into(), "1357".into()).expect("cashier");
        crate::corrections::revert_bill_on(&app, id.clone(), "First edit".into(), None, None).expect("edit");
        crate::flows::complete_bill_on(&app, Some("Cash".into())).expect("complete");
        crate::corrections::revert_bill_on(&app, id.clone(), "Second attempt".into(), None, None).expect("reopen");
        let bills = crate::corrections::bills_on(&app, Default::default()).expect("pending remains visible");
        assert_eq!(bills.waiting, 1);
        assert_eq!(bills.rows[0].approval.as_deref(), Some("waiting"));
        crate::ipc::login_on(&app, "staff_owner".into(), "2468".into()).expect("reviewer");
        let detail = crate::corrections::bill_detail_on(&app, id.clone()).expect("details");
        assert!(detail.can_approve, "earlier completed edit is reviewable");
        assert!(crate::corrections::approve_revert_on(&app, detail.edits[1].id.clone()).is_err(), "unfinished draft is not reviewable");
        crate::corrections::approve_revert_on(&app, detail.edits[0].id.clone()).expect("review earlier edit during later draft");
        crate::ipc::login_on(&app, "cashier".into(), "1357".into()).expect("cashier");
        discard_on(&app, id.clone()).expect("discard only the draft");
        let detail = crate::corrections::bill_detail_on(&app, id).expect("history");
        assert!(detail.edits[1].approved_at.is_none());
        assert!(detail.edits[1].discarded_at.is_some());
        assert_eq!(detail.edits[1].discarded_by.as_deref(), Some("Cashier"));
        assert_eq!(crate::corrections::bills_on(&app, Default::default()).expect("waiting").waiting, 0);
    }

    #[test]
    fn staff_without_edit_permission_cannot_resume_or_mutate_an_existing_correction() {
        let scratch = Scratch::new("correction_resume_permission");
        let (app, id, _) = editing(&scratch);
        crate::signin_tests::hire(&app, "waiter", "Waiter", mb_auth::RolePreset::Waiter, "1357");
        save_on(&app).expect("park draft");
        crate::ipc::login_on(&app, "waiter".into(), "1357".into()).expect("waiter");
        assert_eq!(crate::ipc::open_order_on(&app, id).expect_err("cannot resume").code, "auth.denied");
        assert_eq!(require_edit_access(&app).expect_err("cannot mutate inherited draft").code, "auth.denied");
    }

    #[test]
    fn void_permission_or_a_pin_cannot_approve_but_an_approval_only_reviewer_can() {
        let scratch = Scratch::new("separate_review_permissions");
        let app = crate::signin_tests::a_trading_shop(&scratch);
        crate::signin_tests::hire(&app, "cashier", "Cashier", mb_auth::RolePreset::Cashier, "1357");
        crate::signin_tests::hire(&app, "voider", "Voider", mb_auth::RolePreset::Manager, "3579");
        crate::signin_tests::hire(&app, "reviewer", "Reviewer", mb_auth::RolePreset::Waiter, "4567");
        app.with_shop(|shop| shop.db.transaction(|tx| {
            tx.execute("DELETE FROM role_permissions WHERE role_id = 'role_manager' AND permission_code = 'bill.revert.approve'", [])?;
            tx.execute("DELETE FROM role_permissions WHERE role_id = 'role_waiter'", [])?;
            tx.execute("INSERT INTO role_permissions (role_id, permission_code) VALUES ('role_waiter', 'bill.revert.approve')", [])?;
            Ok(())
        }).map_err(|error| words::from_db(&error))).expect("separate role grants");
        crate::signin_tests::order_teas(&app, 2);
        crate::flows::complete_bill_on(&app, Some("Cash".into())).expect("issued");
        let id = crate::corrections::list_bills_on(&app).expect("bills")[0].order_id.clone();
        crate::ipc::login_on(&app, "cashier".into(), "1357".into()).expect("cashier");
        crate::corrections::revert_bill_on(&app, id.clone(), "Fix items".into(), None, None).expect("edit");
        crate::flows::complete_bill_authorized_on(&app, Some("Cash".into()), None, None,
            Some("voider".into()), Some("3579".into()), None).expect("save, not approve");
        let pending = crate::corrections::bill_detail_on(&app, id.clone()).expect("pending");
        assert_eq!(pending.row.approval.as_deref(), Some("waiting"));
        crate::ipc::login_on(&app, "voider".into(), "3579".into()).expect("voider");
        assert_eq!(crate::corrections::approve_revert_on(&app, pending.edits[0].id.clone()).expect_err("voiding is not reviewing").code, "auth.denied");
        crate::ipc::login_on(&app, "reviewer".into(), "4567".into()).expect("reviewer");
        crate::corrections::approve_revert_on(&app, pending.edits[0].id.clone()).expect("approval permission alone is sufficient");
        let approved = crate::corrections::bill_detail_on(&app, id).expect("approved");
        assert_eq!(approved.edits[0].approved_by.as_deref(), Some("Reviewer"));
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
