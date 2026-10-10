//! Returns must reconcile against original tenders and commit with their void.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests against disposable shop fixtures"
)]

use mb_auth::RolePreset;
use mb_core::{AnyOrder, Money, OrderId, Payment, PaymentMode};
use mb_db::Repos;

use crate::signin_tests::{Scratch, a_trading_shop, hire, order_teas};
use crate::state::{App, OUTLET};

pub(crate) fn paid(app: &App, payments: Vec<(PaymentMode, i64)>) -> String {
    order_teas(app, 4);
    app.with_cart_mut(|state| {
        assert_eq!(state.bill(&app.shop_config())?.grand_total.paise(), 10_500);
        for (mode, amount) in payments {
            state
                .settlement
                .add(Payment::new(mode, Money::from_paise(amount)).expect("payment"))
                .expect("receipt");
        }
        Ok(())
    })
    .expect("receipts");
    crate::flows::complete_bill_on(app, None).expect("issued bill");
    crate::corrections::list_bills_on(app).expect("bills")[0]
        .order_id
        .clone()
}

pub(crate) fn amounts(values: &[(&str, i64)]) -> Vec<(String, Money)> {
    values
        .iter()
        .map(|(mode, value)| ((*mode).to_owned(), Money::from_paise(*value)))
        .collect()
}

fn refund(app: &App, id: &str, values: &[(&str, i64)]) -> crate::words::UiResult<()> {
    crate::refunds::refund_batch_on(
        app,
        id.to_owned(),
        amounts(values),
        "Customer return".to_owned(),
    )
}

fn void(app: &App, id: &str) {
    crate::corrections::void_bill_on(app, id.to_owned(), "Customer return".to_owned(), None, None)
        .expect("void");
}

pub(crate) fn drawer(app: &App) -> i64 {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Ok(Repos::new(tx)
                    .money()
                    .cash_position(OUTLET, crate::flows::today(crate::flows::now()))?
                    .expected
                    .paise())
            })
            .map_err(|error| crate::words::from_db(&error))
    })
    .expect("drawer")
}

pub(crate) fn refund_rows(app: &App, id: &str) -> Vec<(String, i64)> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let mut query = tx.prepare(
                    "SELECT mode, amount FROM refunds WHERE order_id = ?1 ORDER BY rowid",
                )?;
                let rows = query.query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .map_err(|error| crate::words::from_db(&error))
    })
    .expect("persisted refunds")
}

#[test]
fn partial_cash_returns_use_only_the_remaining_balance_and_never_deduct_sales_twice() {
    let scratch = Scratch::new("return_partial_cash");
    let app = a_trading_shop(&scratch);
    let id = paid(&app, vec![(PaymentMode::Cash, 10_500)]);
    assert_eq!(drawer(&app), 10_500);
    void(&app, &id);
    assert_eq!(
        drawer(&app),
        10_500,
        "void cancels sales, not physical cash"
    );
    refund(&app, &id, &[("cash", 3_000)]).expect("first return");
    assert_eq!(drawer(&app), 7_500);
    let offer = crate::refunds::offer_on(&app, id.clone()).expect("remaining");
    assert_eq!(offer.remaining.paise, 7_500);
    assert_eq!(offer.tenders[0].input, "75.00");
    refund(&app, &id, &[("cash", 7_501)]).expect_err("one paisa too much");
    assert_eq!(refund_rows(&app, &id), vec![("cash".to_owned(), 3_000)]);
    refund(&app, &id, &[("cash", 7_500)]).expect("remaining return");
    refund(&app, &id, &[("cash", 7_500)]).expect_err("completed return cannot repeat");
    assert_eq!(drawer(&app), 0);
    let offer = crate::refunds::offer_on(&app, id).expect("nothing left");
    assert_eq!(offer.remaining.paise, 0);
    assert!(offer.tenders.is_empty());
    let totals = crate::corrections::day_totals_on(&app).expect("financial totals");
    assert_eq!(totals.net.paise, 0);
    assert_eq!(totals.voids.paise, 10_500);
    assert_eq!(totals.refunded.paise, 10_500);
}

#[test]
fn split_returns_roll_back_the_first_tender_when_the_second_is_invalid() {
    let scratch = Scratch::new("return_split_atomic");
    let app = a_trading_shop(&scratch);
    let id = paid(
        &app,
        vec![(PaymentMode::Cash, 5_000), (PaymentMode::Card, 5_500)],
    );
    void(&app, &id);
    refund(&app, &id, &[("cash", 2_000), ("card", 5_501)]).expect_err("invalid second allocation");
    assert!(refund_rows(&app, &id).is_empty());
    assert_eq!(drawer(&app), 5_000);
    let offer = crate::refunds::offer_on(&app, id.clone()).expect("unchanged offer");
    assert_eq!(offer.remaining.paise, 10_500);
    refund(&app, &id, &[("cash", 2_000), ("card", 5_500)]).expect("correct split");
    assert_eq!(drawer(&app), 3_000);
    assert_eq!(
        refund_rows(&app, &id),
        vec![("cash".to_owned(), 2_000), ("card".to_owned(), 5_500)]
    );
    let remaining = crate::refunds::offer_on(&app, id).expect("cash remains");
    assert_eq!(remaining.tenders.len(), 1);
    assert_eq!(remaining.tenders[0].mode, "cash");
    assert_eq!(remaining.tenders[0].remaining.paise, 3_000);
}

#[test]
fn upi_money_returns_never_remove_cash_and_cash_substitution_is_refused() {
    let scratch = Scratch::new("return_upi_drawer");
    let app = a_trading_shop(&scratch);
    let id = paid(&app, vec![(PaymentMode::Upi, 10_500)]);
    void(&app, &id);
    refund(&app, &id, &[("cash", 10_500)]).expect_err("unapproved cross-method return");
    assert!(refund_rows(&app, &id).is_empty());
    refund(&app, &id, &[("UPI", 10_500)]).expect("original tender returned");
    assert_eq!(drawer(&app), 0);
    assert_eq!(refund_rows(&app, &id), vec![("upi".to_owned(), 10_500)]);
    let totals = crate::corrections::day_totals_on(&app).expect("totals");
    assert_eq!(totals.net.paise, 0);
    assert_eq!(totals.refunded.paise, 10_500);
}

#[test]
fn combined_void_and_return_is_atomic_when_the_return_is_invalid() {
    let scratch = Scratch::new("return_void_atomic");
    let app = a_trading_shop(&scratch);
    let id = paid(
        &app,
        vec![(PaymentMode::Cash, 5_000), (PaymentMode::Card, 5_500)],
    );
    crate::corrections::void_bill_with_returns_on(
        &app,
        id.clone(),
        "Customer return".to_owned(),
        None,
        None,
        Some(amounts(&[("cash", 2_000), ("card", 5_501)])),
    )
    .expect_err("invalid second return");
    assert!(matches!(
        crate::flows::find_order(&app, &OrderId::new(&id)).expect("order"),
        Some(AnyOrder::Settled(_))
    ));
    assert!(refund_rows(&app, &id).is_empty());
    assert_eq!(drawer(&app), 5_000);
    let totals = crate::corrections::day_totals_on(&app).expect("totals unchanged");
    assert_eq!(totals.net.paise, 10_500);
    assert_eq!(totals.voids.paise, 0);
    crate::corrections::void_bill_with_returns_on(
        &app,
        id.clone(),
        "Customer return".to_owned(),
        None,
        None,
        Some(amounts(&[("cash", 5_000), ("card", 5_500)])),
    )
    .expect("valid combined return");
    assert!(matches!(
        crate::flows::find_order(&app, &OrderId::new(&id)).expect("order"),
        Some(AnyOrder::Voided(_))
    ));
    assert_eq!(drawer(&app), 0);
    assert_eq!(refund_rows(&app, &id).len(), 2);
}

#[test]
fn cashier_without_void_rights_can_read_the_offer_but_cannot_return_money() {
    let scratch = Scratch::new("return_staff_permission");
    let app = a_trading_shop(&scratch);
    let id = paid(&app, vec![(PaymentMode::Cash, 10_500)]);
    void(&app, &id);
    hire(
        &app,
        "staff_return_cashier",
        "Cashier",
        RolePreset::Cashier,
        "1357",
    );
    crate::ipc::login_on(&app, "staff_return_cashier".to_owned(), "1357".to_owned())
        .expect("cashier login");
    assert_eq!(
        crate::refunds::offer_on(&app, id.clone())
            .expect("operational bill access")
            .remaining
            .paise,
        10_500
    );
    refund(&app, &id, &[("cash", 1_000)]).expect_err("no void permission");
    assert!(refund_rows(&app, &id).is_empty());
    assert_eq!(drawer(&app), 10_500);
}

#[test]
fn cash_change_is_not_money_available_for_a_second_return() {
    let scratch = Scratch::new("return_cash_change");
    let app = a_trading_shop(&scratch);
    let id = paid(&app, vec![(PaymentMode::Cash, 20_000)]);
    let reloaded = crate::flows::find_order(&app, &OrderId::new(&id))
        .expect("reload")
        .expect("bill");
    let AnyOrder::Settled(receipt) = reloaded else {
        panic!("settled receipt");
    };
    assert_eq!(
        receipt.settlement.total_paid().expect("tendered").paise(),
        20_000
    );
    assert_eq!(
        receipt
            .settlement
            .change_due(receipt.bill.grand_total)
            .expect("change")
            .paise(),
        9_500
    );
    assert_eq!(
        drawer(&app),
        10_500,
        "change already left the drawer at settlement"
    );
    assert_eq!(
        crate::refunds::offer_on(&app, id.clone())
            .expect("net cash available")
            .remaining
            .paise,
        10_500
    );
    void(&app, &id);
    refund(&app, &id, &[("cash", 20_000)])
        .expect_err("gross tender includes change already returned");
    refund(&app, &id, &[("cash", 10_500)]).expect("net original receipt");
    assert_eq!(drawer(&app), 0);
}

#[test]
fn different_custom_tenders_do_not_share_a_refund_limit_or_touch_cash() {
    let scratch = Scratch::new("return_custom_tenders");
    let app = a_trading_shop(&scratch);
    let id = paid(
        &app,
        vec![
            (PaymentMode::Other("Meal card".to_owned()), 5_000),
            (PaymentMode::Other("Cash".to_owned()), 5_500),
        ],
    );
    let offer = crate::refunds::offer_on(&app, id.clone()).expect("custom tenders");
    assert!(
        offer
            .tenders
            .iter()
            .any(|t| t.mode == "other:meal card" && t.remaining.paise == 5_000)
    );
    assert!(
        offer
            .tenders
            .iter()
            .any(|t| t.mode == "other:cash" && t.remaining.paise == 5_500)
    );
    void(&app, &id);
    refund(&app, &id, &[("cash", 100)]).expect_err("custom label cannot authorize physical cash");
    refund(&app, &id, &[("other:meal card", 5_001)])
        .expect_err("other custom tender cannot cover this one");
    refund(
        &app,
        &id,
        &[("other:meal card", 5_000), ("other:cash", 5_500)],
    )
    .expect("original custom tenders");
    assert_eq!(drawer(&app), 0);
}

#[test]
fn a_partial_return_retry_does_not_pay_twice_and_changed_retries_are_refused() {
    let scratch = Scratch::new("return_partial_retry");
    let app = a_trading_shop(&scratch);
    let id = paid(&app, vec![(PaymentMode::Cash, 10_500)]);
    void(&app, &id);
    let send = |value| {
        crate::refunds::refund_batch_with_id_on(
            &app,
            id.clone(),
            amounts(&[("cash", value)]),
            "Customer return".to_owned(),
            "return-request-one".to_owned(),
        )
    };
    send(1_000).expect("first partial refund");
    send(1_000).expect("retry after response lost");
    assert_eq!(drawer(&app), 9_500, "same partial return happened once");
    assert_eq!(refund_rows(&app, &id), vec![("cash".to_owned(), 1_000)]);
    send(2_000).expect_err("same request cannot change its amount");
    assert_eq!(drawer(&app), 9_500);
    crate::refunds::refund_batch_with_id_on(
        &app,
        id.clone(),
        amounts(&[("cash", 1_000)]),
        "Second return".to_owned(),
        "return-request-two".to_owned(),
    )
    .expect("separate real return");
    assert_eq!(drawer(&app), 8_500);
    assert_eq!(refund_rows(&app, &id).len(), 2);
}

#[test]
fn invalid_return_amounts_do_not_create_rows_or_change_cash() {
    let scratch = Scratch::new("return_amount_validation");
    let app = a_trading_shop(&scratch);
    let id = paid(&app, vec![(PaymentMode::Cash, 10_500)]);
    void(&app, &id);
    for amount in [0, -1] {
        refund(&app, &id, &[("cash", amount)]).expect_err("positive amounts only");
    }
    refund(&app, &id, &[("cash", 100), ("cash", 100)]).expect_err("duplicate tender rejected atomically even for a direct caller");
    crate::refunds::refund_batch_on(&app, id.clone(), amounts(&[("cash", 100)]), " ".to_owned())
        .expect_err("reason required");
    assert!(refund_rows(&app, &id).is_empty());
    assert_eq!(drawer(&app), 10_500);
    assert!(
        crate::refunds::parse_amounts(vec![
            ("Cash".to_owned(), "1.00".to_owned()),
            ("cash".to_owned(), "2.00".to_owned())
        ])
        .is_err()
    );
}
