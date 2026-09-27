//! Corrections must preserve the issued sale and the kitchen's actual quantities.
#![allow(clippy::expect_used, clippy::panic, reason = "tests: assertions against a scratch shop")]

use mb_core::{AnyOrder, Qty};
use crate::signin_tests::{Scratch, a_trading_shop, order_teas, slips_taken};
use crate::state::App;

fn shop(scratch: &Scratch) -> App {
    let app = a_trading_shop(scratch);
    order_teas(&app, 2);
    app
}

#[test]
fn reducing_only_unsent_quantity_needs_no_kitchen_cancellation() {
    let scratch = Scratch::new("reduce_unsent");
    let app = shop(&scratch);
    crate::flows::print_kitchen_ticket_on(&app).expect("two sent");
    crate::corrections::change_line_on(&app, 0, Some(Qty::from_whole(4).expect("qty")), String::new()).expect("increase");
    let (answer, slips) = slips_taken(&app, || crate::corrections::change_line_on(
        &app, 0, Some(Qty::from_whole(3).expect("qty")), String::new()));
    answer.expect("only unsent food reduced");
    assert!(slips.is_empty());
    app.with_cart(|state| {
        assert_eq!(state.cart.lines()[0].qty, Qty::from_whole(3).expect("qty"));
        assert_eq!(state.kitchen.quantity_told(&state.cart.lines()[0].identity()), Qty::from_whole(2).expect("qty"));
        Ok(())
    }).expect("state");
}

#[test]
fn a_sent_reduction_requires_a_reason_and_survives_reopening() {
    let scratch = Scratch::new("reduce_sent");
    let app = shop(&scratch);
    crate::flows::print_kitchen_ticket_on(&app).expect("sent");
    let id = app.with_cart(|s| Ok(s.order_id().expect("id").to_owned())).expect("cart");
    let error = crate::corrections::change_line_on(&app, 0, Some(Qty::ONE), String::new()).expect_err("reason");
    assert_eq!(error.code, "cart.reason_required");
    let (answer, slips) = slips_taken(&app, || crate::corrections::change_line_on(&app, 0, Some(Qty::ONE), "Guest changed order".to_owned()));
    answer.expect("reduced");
    assert_eq!(slips.len(), 1);
    crate::ipc::cart_clear_on(&app, false).expect("clear");
    crate::ipc::open_order_on(&app, id).expect("reopen");
    app.with_cart(|state| {
        assert_eq!(state.cart.lines()[0].qty, Qty::ONE);
        assert!(state.kitchen.pending(&state.cart).expect("pending").is_empty());
        assert!(state.kitchen.over_told(&state.cart).expect("cancel").is_empty());
        Ok(())
    }).expect("state");
}

#[test]
fn an_issued_bill_cannot_be_voided_under_a_pending_correction() {
    let scratch = Scratch::new("void_during_edit");
    let app = shop(&scratch);
    crate::flows::complete_bill_on(&app, Some("Cash".to_owned())).expect("settled");
    let row = crate::corrections::list_bills_on(&app).expect("bills").remove(0);
    crate::corrections::revert_bill_on(&app, row.order_id.clone(), "Correct items".to_owned(), None, None).expect("edit");
    crate::corrections::void_bill_on(&app, row.order_id.clone(), "Cancel".to_owned(), None, None).expect_err("draft protected");
    let working = crate::flows::find_order(&app, &mb_core::OrderId::new(&row.order_id)).expect("working");
    assert!(matches!(working, Some(AnyOrder::Open(_))));
    assert_eq!(crate::corrections::list_bills_on(&app).expect("issued")[0].total, row.total);
}

#[test]
fn joining_another_party_keeps_the_current_orders_unsent_edits() {
    let scratch = Scratch::new("join_keeps_edits");
    let app = shop(&scratch);
    crate::floor::save_table_on(&app, crate::floor::TableEdit {
        id: "table_one".to_owned(), label: "1".to_owned(), section_id: None,
        seats: 4, is_active: true,
    }).expect("table");
    crate::ipc::open_table_on(&app, "table_one".to_owned()).expect("first party");
    crate::flows::print_kitchen_ticket_on(&app).expect("sent");
    let id = app.with_cart(|s| Ok(s.order_id().expect("id").to_owned())).expect("cart");
    crate::ipc::cart_add_on(&app, "itm_tea".to_owned(), Some("1".to_owned()), None).expect("another tea");
    crate::ipc::join_table_on(&app, "table_one".to_owned(), None).expect("second party");
    crate::ipc::open_order_on(&app, id).expect("first party again");
    app.with_cart(|state| {
        assert_eq!(state.cart.lines()[0].qty, Qty::from_whole(3).expect("qty"));
        assert_eq!(state.kitchen.pending(&state.cart).expect("pending")[0].1, Qty::ONE);
        Ok(())
    }).expect("unsent item retained");
}

fn paid_bill(app: &App) -> String {
    order_teas(app, 2);
    crate::flows::complete_bill_on(app, Some("Cash".to_owned())).expect("paid bill");
    crate::corrections::list_bills_on(app).expect("bills")[0].order_id.clone()
}

#[test]
fn revert_parks_current_items_and_receipts_without_settling_them() {
    let scratch = Scratch::new("revert_preserves_current");
    let app = a_trading_shop(&scratch);
    let target = paid_bill(&app);
    order_teas(&app, 1);
    let current = crate::flows::park_open_order(&app).expect("saved order").core.id;
    crate::ipc::cart_add_on(&app, "itm_tea".to_owned(), Some("1".to_owned()), None).expect("local item");
    app.with_cart_mut(|state| {
        state.settlement.add(mb_core::Payment::new(mb_core::PaymentMode::Cash, mb_core::Money::from_paise(500)).expect("payment")).expect("receipt");
        Ok(())
    }).expect("local receipt");
    crate::corrections::revert_bill_on(&app, target.clone(), "Fix other bill".to_owned(), None, None).expect("switch to correction");
    let saved = crate::flows::find_order(&app, &current).expect("current order").expect("retained");
    assert!(matches!(saved, AnyOrder::Open(_)));
    assert_eq!(saved.core().cart.lines()[0].qty, Qty::from_whole(2).expect("qty"));
    assert_eq!(saved.core().billing.settlement.payments()[0].amount, mb_core::Money::from_paise(500));
    app.with_cart(|state| { assert_eq!(state.order_id(), Some(target.as_str())); Ok(()) }).expect("target loaded");
}

#[test]
fn revert_can_leave_a_clean_order_changed_on_the_phone() {
    let scratch = Scratch::new("revert_clean_mobile_order");
    let app = a_trading_shop(&scratch);
    let target = paid_bill(&app);
    order_teas(&app, 1);
    let mut current = crate::flows::park_open_order(&app).expect("saved order");
    current.core.cart.set_qty(0, Qty::from_whole(3).expect("qty")).expect("phone addition");
    crate::flows::save_order(&app, &AnyOrder::Open(current.clone())).expect("phone saved");
    crate::corrections::revert_bill_on(&app, target, "Fix other bill".to_owned(), None, None).expect("clean old selection can be left");
    let saved = crate::flows::find_order(&app, &current.core.id).expect("current order").expect("retained");
    assert_eq!(saved.core().cart.lines()[0].qty, Qty::from_whole(3).expect("qty"));
}

#[test]
fn revert_can_leave_a_clean_selection_settled_elsewhere() {
    let scratch = Scratch::new("revert_clean_finished_selection");
    let app = a_trading_shop(&scratch);
    let target = paid_bill(&app);
    order_teas(&app, 1);
    let current = crate::flows::park_open_order(&app).expect("saved order").core.id;
    let selected = app.with_cart(|state| Ok(state.clone())).expect("old selection");
    crate::flows::complete_bill_on(&app, Some("Cash".to_owned())).expect("settled elsewhere");
    app.with_cart_mut(|state| { *state = selected; Ok(()) }).expect("counter still shows old selection");
    crate::corrections::revert_bill_on(&app, target, "Fix other bill".to_owned(), None, None).expect("clean finished selection can be left");
    let saved = crate::flows::find_order(&app, &current).expect("current order").expect("retained");
    assert!(matches!(saved, AnyOrder::Settled(_)));
    assert!(!saved.core().billing.settlement.payments().is_empty());
}

#[test]
fn revert_already_active_correction_keeps_local_items_and_receipts() {
    let scratch = Scratch::new("revert_same_correction");
    let app = a_trading_shop(&scratch);
    let target = paid_bill(&app);
    crate::corrections::revert_bill_on(&app, target.clone(), "Fix items".to_owned(), None, None).expect("first edit");
    crate::ipc::cart_add_on(&app, "itm_tea".to_owned(), Some("1".to_owned()), None).expect("local item");
    app.with_cart_mut(|state| {
        state.settlement.add(mb_core::Payment::new(mb_core::PaymentMode::Cash, mb_core::Money::from_paise(500)).expect("payment")).expect("receipt");
        Ok(())
    }).expect("local receipt");
    let before = app.with_cart(|state| Ok((state.cart.clone(), state.settlement.clone(), state.bill_number.clone()))).expect("before");
    crate::corrections::revert_bill_on(&app, target, "Resume edit".to_owned(), None, None).expect("same correction");
    let after = app.with_cart(|state| Ok((state.cart.clone(), state.settlement.clone(), state.bill_number.clone()))).expect("after");
    assert_eq!(after, before);
}

#[test]
fn revert_invalid_target_keeps_the_current_unsaved_changes() {
    let scratch = Scratch::new("revert_invalid_target");
    let app = shop(&scratch);
    let current = crate::flows::park_open_order(&app).expect("saved order").core.id;
    crate::ipc::cart_add_on(&app, "itm_tea".to_owned(), Some("1".to_owned()), None).expect("local item");
    let before = app.with_cart(|state| Ok((state.cart.clone(), state.origin.clone(), state.settlement.clone()))).expect("before");
    let error = crate::corrections::revert_bill_on(&app, "missing_bill".to_owned(), "Fix it".to_owned(), None, None).expect_err("missing target");
    assert_eq!(error.code, "revert.not_settled");
    let after = app.with_cart(|state| Ok((state.cart.clone(), state.origin.clone(), state.settlement.clone()))).expect("after");
    assert_eq!(after, before);
    assert_eq!(crate::flows::find_order(&app, &current).expect("saved").expect("order").core().cart.lines()[0].qty, Qty::from_whole(2).expect("qty"));
}

#[test]
fn revert_never_discards_an_unsaved_receipt_on_an_empty_cart() {
    let scratch = Scratch::new("revert_empty_with_receipt");
    let app = a_trading_shop(&scratch);
    let target = paid_bill(&app);
    app.with_cart_mut(|state| {
        state.settlement.add(mb_core::Payment::new(mb_core::PaymentMode::Cash, mb_core::Money::from_paise(500)).expect("payment")).expect("receipt");
        Ok(())
    }).expect("unsaved receipt");
    let error = crate::corrections::revert_bill_on(&app, target.clone(), "Fix it".to_owned(), None, None).expect_err("receipt protected");
    assert_eq!(error.code, "revert.counter_busy");
    app.with_cart(|state| { assert_eq!(state.settlement.payments()[0].amount, mb_core::Money::from_paise(500)); Ok(()) }).expect("receipt still present");
    assert!(matches!(crate::flows::find_order(&app, &mb_core::OrderId::new(target)).expect("bill"), Some(AnyOrder::Settled(_))));
}

#[test]
fn changing_a_line_preserves_independent_phone_additions() {
    let scratch = Scratch::new("line_change_keeps_phone_addition");
    let app = shop(&scratch);
    let mut remote = crate::flows::park_open_order(&app).expect("saved order");
    let item = remote.core.cart.lines()[0].snapshot.clone();
    remote.core.cart.add(item, Qty::ONE, Some("Phone addition".to_owned()), vec![]).expect("another line");
    crate::flows::save_order(&app, &AnyOrder::Open(remote.clone())).expect("phone saved");
    crate::corrections::change_line_on(&app, 0, Some(Qty::ONE), String::new()).expect("change original line");
    let saved = crate::flows::find_order(&app, &remote.core.id).expect("saved").expect("order");
    assert_eq!(saved.core().cart.len(), 2);
    assert_eq!(saved.core().cart.lines()[0].qty, Qty::ONE);
    assert_eq!(saved.core().cart.lines()[1].note.as_deref(), Some("Phone addition"));
}

#[test]
fn changing_a_line_follows_its_identity_when_another_line_was_removed() {
    let scratch = Scratch::new("line_change_after_reorder");
    let app = shop(&scratch);
    crate::ipc::cart_add_on(&app, "itm_tea".to_owned(), Some("2".to_owned()), Some("Chosen line".to_owned())).expect("second line");
    let mut remote = crate::flows::park_open_order(&app).expect("saved order");
    remote.core.cart.remove(0).expect("phone removed other line");
    crate::flows::save_order(&app, &AnyOrder::Open(remote.clone())).expect("phone saved");
    crate::corrections::change_line_on(&app, 1, Some(Qty::ONE), String::new()).expect("same line at new position");
    let saved = crate::flows::find_order(&app, &remote.core.id).expect("saved").expect("order");
    assert_eq!(saved.core().cart.len(), 1);
    assert_eq!(saved.core().cart.lines()[0].note.as_deref(), Some("Chosen line"));
    assert_eq!(saved.core().cart.lines()[0].qty, Qty::ONE);
}

#[test]
fn changing_a_line_refuses_a_quantity_changed_on_the_phone() {
    let scratch = Scratch::new("line_change_remote_conflict");
    let app = shop(&scratch);
    let mut remote = crate::flows::park_open_order(&app).expect("saved order");
    remote.core.cart.set_qty(0, Qty::from_whole(3).expect("qty")).expect("phone changed line");
    crate::flows::save_order(&app, &AnyOrder::Open(remote.clone())).expect("phone saved");
    let error = crate::corrections::change_line_on(&app, 0, Some(Qty::ONE), String::new()).expect_err("changed item must be selected again");
    assert_eq!(error.code, "order.changed");
    let saved = crate::flows::find_order(&app, &remote.core.id).expect("saved").expect("order");
    assert_eq!(saved.core().cart.lines()[0].qty, Qty::from_whole(3).expect("qty"));
}

#[test]
fn changing_a_line_rechecks_food_sent_to_the_kitchen_on_the_phone() {
    let scratch = Scratch::new("line_change_remote_kitchen");
    let app = shop(&scratch);
    let mut remote = crate::flows::park_open_order(&app).expect("saved order");
    remote.core.kitchen.mark_printed(&remote.core.kitchen.pending(&remote.core.cart).expect("pending")).expect("phone sent kitchen");
    crate::flows::save_order(&app, &AnyOrder::Open(remote.clone())).expect("phone saved");
    let error = crate::corrections::change_line_on(&app, 0, Some(Qty::ONE), String::new()).expect_err("sent reduction requires reason");
    assert_eq!(error.code, "cart.reason_required");
    let saved = crate::flows::find_order(&app, &remote.core.id).expect("saved").expect("order");
    assert_eq!(saved.core().cart.lines()[0].qty, Qty::from_whole(2).expect("qty"));
    assert_eq!(saved.core().kitchen, remote.core.kitchen);
}

#[test]
fn changing_a_line_rejects_a_stale_screen_after_the_counter_already_refreshed() {
    let scratch = Scratch::new("line_change_stale_screen_token");
    let app = shop(&scratch);
    let mut remote = crate::flows::park_open_order(&app).expect("saved order");
    let token = app.with_cart(|state| Ok(crate::billing::line_edit_token(state, &state.cart.lines()[0]))).expect("shown token");
    let item = remote.core.cart.lines()[0].snapshot.clone();
    remote.core.cart.remove(0).expect("phone removed selected line");
    remote.core.cart.add(item, Qty::ONE, Some("Different customer request".to_owned()), vec![]).expect("different line at same index");
    crate::flows::save_order(&app, &AnyOrder::Open(remote.clone())).expect("phone saved");
    app.with_cart_mut(|state| { *state = crate::billing::CartState::load(&AnyOrder::Open(remote.clone()), None); Ok(()) }).expect("backend refreshed before click");
    let error = crate::corrections::change_line_checked_on(&app, 0, None, String::new(), Some(token)).expect_err("stale screen must not delete the new line");
    assert_eq!(error.code, "order.changed");
    app.with_cart(|state| {
        assert_eq!(state.cart.lines(), remote.core.cart.lines());
        Ok(())
    }).expect("current line retained");
    let saved = crate::flows::find_order(&app, &remote.core.id).expect("saved").expect("order");
    assert_eq!(saved.core().cart, remote.core.cart);
}

#[test]
fn changing_a_line_accepts_the_current_screen_token() {
    let scratch = Scratch::new("line_change_current_screen_token");
    let app = shop(&scratch);
    let current = crate::flows::park_open_order(&app).expect("saved order").core.id;
    let token = app.with_cart(|state| Ok(crate::billing::line_edit_token(state, &state.cart.lines()[0]))).expect("shown token");
    crate::corrections::change_line_checked_on(&app, 0, Some(Qty::ONE), String::new(), Some(token)).expect("valid selection");
    let saved = crate::flows::find_order(&app, &current).expect("saved").expect("order");
    assert_eq!(saved.core().cart.lines()[0].qty, Qty::ONE);
}
