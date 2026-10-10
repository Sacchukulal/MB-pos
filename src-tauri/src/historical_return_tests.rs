//! Historical-return commands, using the same cashier fixtures as ordinary refunds.
#![allow(clippy::expect_used, clippy::panic, reason = "isolated app test fixtures")]

use mb_core::{Money, OrderId, PaymentMode, StaffId};
use mb_db::{Repos, repo::{DayKind, DayRow}};
use crate::{refund_tests::{amounts, drawer, paid, refund_rows}, signin_tests::{Scratch, a_trading_shop}, state::{App, OUTLET}};

fn make_historical(app: &App, id: &str) {
    let at = crate::flows::now();
    let yesterday = crate::flows::today(at).previous();
    app.with_shop(|shop| shop.db.transaction(|tx| {
        // Fixture only: move the whole paid receipt to yesterday, then close that day.
        tx.execute("UPDATE orders SET business_day = ?1 WHERE id = ?2", (yesterday.days_since_epoch(), id))?;
        tx.execute("UPDATE payments SET business_day = ?1 WHERE order_id = ?2", (yesterday.days_since_epoch(), id))?;
        let repos = Repos::new(tx);
        let figures = repos.days().figures(OUTLET, yesterday)?;
        repos.days().lock(OUTLET, &DayRow { day: yesterday, kind: DayKind::Trading, is_locked: true,
            closed_at: Some(at), closed_by: Some(StaffId::new("staff_owner")), reopened_at: None, reopened_by: None,
            note: None, bills: figures.bills, net: figures.net, cash_taken: figures.cash })
    }).map_err(|e| crate::words::from_db(&e))).expect("closed historical fixture");
}

fn send(app: &App, id: &str, payments: &[(&str, i64)], key: &str) -> crate::words::UiResult<()> {
    crate::refunds::return_closed_bill_on(app, id.to_owned(), amounts(payments), "Returned meal".to_owned(), key.to_owned(), None, None)
}

#[test]
fn a_closed_mixed_bill_returns_today_once_and_keeps_the_original_and_close_intact() {
    let scratch = Scratch::new("historical_return_mixed"); let app = a_trading_shop(&scratch);
    let id = paid(&app, vec![(PaymentMode::Cash, 6000), (PaymentMode::Card, 4500)]);
    make_historical(&app, &id);
    let original = crate::flows::find_order(&app, &OrderId::new(&id)).expect("original");
    assert!(crate::refunds::offer_on(&app, id.clone()).expect("offer").closed_day);
    send(&app, &id, &[("cash", 6000), ("card", 4500)], "historical_mixed").expect("full return");
    send(&app, &id, &[("cash", 6000), ("card", 4500)], "historical_mixed").expect("same request retry");
    assert_eq!(drawer(&app), -6000, "only actual current-day cash leaves the drawer");
    assert_eq!(refund_rows(&app, &id).len(), 2);
    assert_eq!(crate::flows::find_order(&app, &OrderId::new(&id)).expect("original after"), original);
    assert!(crate::refunds::offer_on(&app, id.clone()).expect("returned offer").returned);
    send(&app, &id, &[("cash", 5900), ("card", 4500)], "historical_mixed").expect_err("same request changed");
    assert_eq!(drawer(&app), -6000);
    crate::corrections::void_bill_on(&app, id.clone(), "Cannot void again".to_owned(), None, None).expect_err("no second sale reversal");
    app.with_shop(|shop| shop.db.read_transaction(|tx| {
        let repos = Repos::new(tx); let today = crate::flows::today(crate::flows::now());
        assert!(repos.days().is_locked(OUTLET, today.previous())?);
        assert_eq!(repos.corrections().day_totals(OUTLET, today.previous())?.net.paise(), 10500);
        assert_eq!(repos.corrections().day_totals(OUTLET, today)?.net.paise(), -10500);
        Ok(())
    }).map_err(|e| crate::words::from_db(&e))).expect("dated totals");
}

#[test]
fn an_invalid_historical_tender_rolls_back_return_header_refunds_and_account_changes() {
    let scratch = Scratch::new("historical_return_atomic"); let app = a_trading_shop(&scratch);
    let id = paid(&app, vec![(PaymentMode::Cash, 10500)]); make_historical(&app, &id);
    send(&app, &id, &[("cash", 6000), ("card", 4500)], "bad_tender").expect_err("no original card receipt");
    assert!(refund_rows(&app, &id).is_empty());
    assert!(!crate::refunds::offer_on(&app, id.clone()).expect("not returned").returned);
    send(&app, &id, &[("cash", 10500)], "valid_cash").expect("cash return");
    assert_eq!(drawer(&app), -10500);
}

#[test]
fn an_unpaid_return_request_cannot_be_retried_with_a_different_payout() {
    let scratch = Scratch::new("historical_return_no_payout_retry"); let app = a_trading_shop(&scratch);
    let id = paid(&app, vec![(PaymentMode::Cash, 10500)]); make_historical(&app, &id);
    send(&app, &id, &[], "no_payout").expect("record full return, cash still owed");
    send(&app, &id, &[], "no_payout").expect("identical retry");
    send(&app, &id, &[("cash", 10500)], "no_payout").expect_err("changed retry must not pay");
    assert!(refund_rows(&app, &id).is_empty());
    crate::refunds::refund_batch_with_id_on(&app, id.clone(), amounts(&[("cash", 10500)]),
        "Cash returned afterward".to_owned(), "later_payout".to_owned()).expect("separate actual refund");
    assert_eq!(drawer(&app), -10500);
}

#[test]
fn credit_only_historical_return_credits_the_account_and_never_invents_a_cash_receipt() {
    let scratch = Scratch::new("historical_return_credit"); let app = a_trading_shop(&scratch);
    let customer = mb_core::CustomerId::new("cus_historical");
    app.with_shop(|shop| shop.db.transaction(|tx| Repos::new(tx).money().save_customer(OUTLET,
        &mb_db::repo::money::Customer { id: customer.clone(), name: "Credit customer".to_owned(), phone: None,
            gstin: None, address: None, credit_limit: None, is_active: true }, crate::flows::now()))
        .map_err(|e| crate::words::from_db(&e))).expect("customer");
    let id = paid(&app, vec![(PaymentMode::Credit(customer.clone()), 10500)]);
    make_historical(&app, &id);
    send(&app, &id, &[], "credit_return").expect("credit bill returned");
    send(&app, &id, &[], "credit_return").expect("credit retry");
    assert!(refund_rows(&app, &id).is_empty()); assert_eq!(drawer(&app), 0);
    app.with_shop(|shop| shop.db.read_transaction(|tx| {
        let repos = Repos::new(tx); assert_eq!(repos.money().customer_balance(&customer)?, Money::ZERO);
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM credit_adjustments WHERE customer_id = ?1", [customer.as_str()], |row| row.get(0))?;
        assert_eq!(count, 1, "the credited account also has one cloud-synced ledger adjustment");
        Ok(())
    }).map_err(|e| crate::words::from_db(&e))).expect("account credited once");
}

#[test]
fn historical_return_approval_uses_the_whole_bill_even_when_only_some_cash_is_paid_back() {
    let scratch = Scratch::new("historical_return_approval"); let app = a_trading_shop(&scratch);
    let id = paid(&app, vec![(PaymentMode::Cash, 10500)]); make_historical(&app, &id);
    app.with_shop(|shop| shop.db.transaction(|tx| Repos::new(tx).settings().set(OUTLET,
        crate::corrections::APPROVAL_KEY, &Money::from_paise(10000), crate::flows::now(), Some("staff_owner")))
        .map_err(|e| crate::words::from_db(&e))).expect("approval threshold");
    assert!(crate::refunds::offer_on(&app, id.clone()).expect("offer").needs_approval);
    assert_eq!(send(&app, &id, &[("cash", 1000)], "threshold").expect_err("approval first").code, "void.needs_approval");
    crate::refunds::return_closed_bill_on(&app, id.clone(), amounts(&[("cash", 1000)]), "Returned meal".to_owned(),
        "threshold".to_owned(), Some("staff_owner".to_owned()), Some("9999".to_owned())).expect_err("bad PIN");
    assert!(!crate::refunds::offer_on(&app, id.clone()).expect("unmodified").returned);
    crate::refunds::return_closed_bill_on(&app, id.clone(), amounts(&[("cash", 1000)]), "Returned meal".to_owned(),
        "threshold".to_owned(), Some("staff_owner".to_owned()), Some("2468".to_owned())).expect("approved return");
    assert_eq!(drawer(&app), -1000);
}

#[test]
fn returning_a_prepared_meal_keeps_its_stock_consumption_and_cost_while_a_void_reverses_it() {
    for returning in [true, false] {
        let scratch = Scratch::new("return_prepared_stock"); let app = a_trading_shop(&scratch);
        let id = paid(&app, vec![(PaymentMode::Cash, 10500)]);
        let today = crate::flows::today(crate::flows::now());
        app.with_shop(|shop| shop.db.transaction(|tx| {
            tx.execute("INSERT INTO materials (id, outlet_id, name, dimension, created_at) VALUES ('mat_meal', ?1, 'Prepared meal', 'count', 0)", [OUTLET])?;
            let unit_cost = mb_core::UnitCost::from_batch(Money::from_paise(200), mb_core::Qty::ONE).expect("one prepared meal costs two rupees");
            let mut movement = mb_db::repo::stock::Movement::new("stock_used", mb_core::MaterialId::new("mat_meal"),
                mb_db::repo::stock::MovementKind::Sale, mb_core::Qty::from_thousandths(-1000), crate::flows::now(), today)
                .typed(mb_core::Qty::from_thousandths(-1000), "piece").costing(unit_cost);
            movement.order_id = Some(mb_core::OrderId::new(id.clone()));
            Repos::new(tx).stock().record(OUTLET, &movement)?;
            Ok(())
        }).map_err(|e| crate::words::from_db(&e))).expect("food already consumed");
        if returning {
            crate::corrections::void_bill_with_returns_on(&app, id.clone(), "Meal returned".to_owned(), None, None,
                Some(amounts(&[("cash", 10500)]))).expect("return prepared meal");
        } else {
            crate::corrections::void_bill_on(&app, id.clone(), "Sale entered by mistake".to_owned(), None, None).expect("void error");
        }
        app.with_shop(|shop| shop.db.read_transaction(|tx| {
            let balance: i64 = tx.query_row("SELECT SUM(base_qty) FROM stock_movements WHERE order_id = ?1", [&id], |row| row.get(0))?;
            assert_eq!(balance, if returning { -1000 } else { 0 });
            let margin = Repos::new(tx).reports().profit(OUTLET, mb_db::repo::reports::Period::one_day(today))?;
            assert_eq!(margin.food_used.paise(), if returning { 200 } else { 0 });
            Ok(())
        }).map_err(|e| crate::words::from_db(&e))).expect("prepared stock and margin agree");
    }
}
