//! Live carts reconcile mobile changes without losing counter edits or receipts.
#![allow(clippy::expect_used, clippy::panic, reason = "focused scratch-shop regressions")]

use mb_core::{AnyOrder, Money, Qty};
use mb_lan::{Intent, Outcome, What};
use crate::signin_tests::{Scratch, a_trading_shop, order_teas};
use crate::state::App;

fn selected(scratch: &Scratch) -> (App, mb_core::OpenOrder) {
    let app = a_trading_shop(scratch);
    order_teas(&app, 1);
    let open = crate::flows::park_open_order(&app).expect("saved selected order");
    (app, open)
}

fn phone(app: &App, open: &mb_core::OpenOrder, id: &str, what: What) -> Outcome {
    crate::orders::apply(app, "phone_test", &mb_core::StaffId::new("staff_owner"),
        &mb_auth::PermissionSet::everything(), &Intent {
            id: id.to_owned(), order_id: Some(open.core.id.as_str().to_owned()),
            open_intent_id: None, at: crate::flows::now().millis(), sent_at: None, what,
        }).expect("phone request").outcome
}

fn add(note: Option<&str>) -> What {
    What::AddItem { item_id: "itm_tea".to_owned(), qty: "1".to_owned(),
        note: note.map(str::to_owned), modifiers: vec![] }
}

#[test]
fn mobile_add_updates_selected_cart_once_and_parks_without_a_reopen_loop() {
    let scratch = Scratch::new("cart_sync_clean");
    let (app, open) = selected(&scratch);
    assert!(matches!(phone(&app, &open, "floor_add", add(None)), Outcome::Ok { .. }));
    // A retried delivery and two acknowledgements must not add food again.
    phone(&app, &open, "floor_add", add(None));
    crate::orders::take_the_floors_items_on(&app).expect("acknowledged");
    crate::orders::take_the_floors_items_on(&app).expect("acknowledged again");
    let shown = crate::ipc::current_cart_on(&app).expect("fresh cart");
    assert_eq!(shown.lines[0].qty, "2");
    assert!(shown.from_the_floor.is_empty());
    assert!(shown.lines[0].edit_token.is_some());
    let saved = crate::flows::park_open_order(&app).expect("save rebased cart");
    assert_eq!(saved.core.cart.lines()[0].qty, Qty::from_whole(2).expect("qty"));
    let total = crate::flows::bill_of(&app, &AnyOrder::Open(saved)).expect("saved bill").grand_total;
    assert_eq!(shown.bill.grand_total.paise, total.paise());
    crate::ipc::open_order_on(&app, open.core.id.as_str().to_owned()).expect("same order reopens");
    crate::flows::complete_bill_on(&app, Some("Cash".to_owned())).expect("settled current total");
}

#[test]
fn independent_local_edits_and_receipts_survive_mobile_items_and_kitchen_updates() {
    let scratch = Scratch::new("cart_sync_local");
    let (app, open) = selected(&scratch);
    crate::ipc::cart_add_on(&app, "itm_tea".to_owned(), None, Some("counter".to_owned())).expect("local line");
    crate::ipc::cart_add_payment_on(&app, "Cash".to_owned(), 50_000, None).expect("typed receipt");
    phone(&app, &open, "independent_add", add(Some("floor")));
    phone(&app, &open, "floor_kitchen", What::SendToKitchen);
    app.with_cart(|state| {
        assert_eq!(state.cart.lines().len(), 3);
        assert_eq!(state.cart.lines()[0].snapshot, open.core.cart.lines()[0].snapshot);
        assert_eq!(state.settlement.total_paid().expect("paid"), Money::from_paise(50_000));
        assert!(state.account.settlement.payments().is_empty(), "refresh cannot save typed cash");
        assert_eq!(state.kitchen.quantity_told(&state.cart.lines()[0].identity()), Qty::ONE);
        assert_eq!(state.kitchen.pending(&state.cart).expect("pending").len(), 1, "only the counter's extra line remains unsent");
        Ok(())
    }).expect("independent edits merged");
    app.with_cart_mut(|state| {
        // The actual clear-cash operation restores this persisted receipt baseline.
        state.settlement = state.account.settlement.clone();
        Ok(())
    }).expect("correct typed cash");
    crate::ipc::cart_add_payment_on(&app, "Cash".to_owned(), 60_000, None).expect("corrected receipt");
    let saved = crate::flows::park_open_order(&app).expect("save all independent changes");
    assert_eq!(saved.core.billing.settlement.total_paid().expect("paid"), Money::from_paise(60_000));
    assert_eq!(saved.core.cart.lines().len(), 3);
}

#[test]
fn conflicting_quantity_changes_preserve_local_work_until_explicit_reload() {
    let scratch = Scratch::new("cart_sync_conflict");
    let (app, open) = selected(&scratch);
    app.with_cart_mut(|state| {
        state.cart.set_qty(0, Qty::from_whole(3).expect("qty")).expect("local quantity");
        Ok(())
    }).expect("typed quantity");
    phone(&app, &open, "remote_qty", What::SetQty { line: 0, qty: "4".to_owned() });
    assert_eq!(crate::ipc::current_cart_on(&app).expect_err("conflict").code, "order.changed");
    assert_eq!(crate::flows::park_open_order(&app).expect_err("cannot overwrite").code, "order.changed");
    app.with_cart(|state| {
        assert_eq!(state.cart.lines()[0].qty, Qty::from_whole(3).expect("qty"));
        assert_eq!(state.origin.as_ref().expect("origin").baseline.as_ref().expect("baseline"), &open.core);
        Ok(())
    }).expect("local draft intact");
    let reloaded = crate::ipc::reload_current_order_on(&app).expect("explicit discard");
    assert_eq!(reloaded.lines[0].qty, "4");
    crate::flows::park_open_order(&app).expect("reload recovers save");
}

#[test]
fn explicit_reload_refuses_to_discard_unsaved_receipts() {
    let scratch = Scratch::new("cart_sync_receipt");
    let (app, _) = selected(&scratch);
    crate::ipc::cart_add_payment_on(&app, "Cash".to_owned(), 50_000, None).expect("receipt");
    assert_eq!(crate::ipc::reload_current_order_on(&app).expect_err("receipt protected").code, "order.unsaved_payment");
    app.with_cart(|state| {
        assert_eq!(state.settlement.total_paid().expect("paid"), Money::from_paise(50_000));
        Ok(())
    }).expect("money retained");
}

#[test]
fn equal_independent_receipts_are_a_conflict_not_one_payment() {
    let scratch = Scratch::new("cart_sync_two_receipts");
    let (app, mut remote) = selected(&scratch);
    crate::ipc::cart_add_payment_on(&app, "Cash".to_owned(), 50_000, None).expect("local receipt");
    remote.core.billing.settlement.add(mb_core::Payment::new(mb_core::PaymentMode::Cash, Money::from_paise(50_000)).expect("remote receipt")).expect("paid");
    app.with_cart(|state| {
        assert_eq!(state.reconciled(&AnyOrder::Open(remote), None).expect_err("independent payments need review").code, "order.changed");
        Ok(())
    }).expect("no receipts collapsed");
}

#[test]
fn a_stale_discount_dialog_cannot_edit_the_line_that_moved_into_its_index() {
    let scratch = Scratch::new("cart_sync_discount_target");
    let (app, open) = selected(&scratch);
    phone(&app, &open, "second_line", add(Some("different guest")));
    let before = crate::ipc::current_cart_on(&app).expect("displayed cart");
    let token = before.lines[0].edit_token.clone();
    phone(&app, &open, "remove_first", What::VoidItem { line: 0, reason: "Guest changed order".to_owned() });
    let error = crate::ipc::cart_set_discount_checked_on(&app, "percent".to_owned(), "10".to_owned(), None, Some(0), token.clone()).expect_err("old line cannot discount replacement");
    assert_eq!(error.code, "order.changed");
    assert_eq!(crate::ipc::cart_clear_discount_checked_on(&app, Some(0), token).expect_err("clear also checks target").code, "order.changed");
    app.with_cart(|state| {
        assert_eq!(state.cart.lines().len(), 1);
        assert_eq!(state.cart.lines()[0].note.as_deref(), Some("different guest"));
        assert!(state.cart.lines()[0].line_discount.is_none());
        Ok(())
    }).expect("replacement line was untouched");
}

#[test]
fn quantity_reductions_at_the_counter_require_whole_targets_but_additions_may_be_fractional() {
    let scratch = Scratch::new("quantity_reduction_counter");
    let (app, _) = selected(&scratch);
    crate::ipc::cart_add_on(&app, "itm_tea".to_owned(), Some("0.5".to_owned()), None).expect("fractional addition remains supported");
    for quantity in ["0.5", "0"] {
        let error = crate::corrections::change_line_on(&app, 0, Some(Qty::parse(quantity).expect("quantity")), String::new()).expect_err("not a positive whole reduction target");
        assert_eq!(error.code, "cart.qty_whole");
    }
    assert_eq!(crate::ipc::current_cart_on(&app).expect("unchanged cart").lines[0].qty, "1.5");
    let reduced = crate::corrections::change_line_on(&app, 0, Some(Qty::ONE), String::new()).expect("whole target accepted");
    assert_eq!(reduced.lines[0].qty, "1");
}

#[test]
fn quantity_reductions_from_phones_require_whole_targets_in_both_intents() {
    let scratch = Scratch::new("quantity_reduction_phone");
    let (app, open) = selected(&scratch);
    let Outcome::Ok { lines, .. } = phone(&app, &open, "fractional_increase", What::SetQty { line: 0, qty: "2.5".to_owned() }) else { panic!("fractional increase should remain valid") };
    let legacy = phone(&app, &open, "fractional_set", What::SetQty { line: 0, qty: "1.5".to_owned() });
    assert!(matches!(legacy, Outcome::Refused { .. }));
    assert!(legacy.message().contains("whole quantity"));
    let partial = phone(&app, &open, "fractional_reduce", What::ReduceQty {
        expected: lines[0].clone(), qty: "1.5".to_owned(), reason: "Customer changed order".to_owned(),
    });
    assert!(matches!(partial, Outcome::Refused { .. }));
    assert!(partial.message().contains("whole-number quantity"));
    let Outcome::Ok { lines, .. } = phone(&app, &open, "whole_reduce", What::ReduceQty {
        expected: lines[0].clone(), qty: "2".to_owned(), reason: "Customer changed order".to_owned(),
    }) else { panic!("whole target should reduce") };
    assert_eq!(lines[0].qty, "2");
}
