//! Expenses and the drawer, driven end to end.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_db::{Db, DbConfig, Repos};

use crate::expenses::{
    ExpenseEdit, confirm_due_on, delete_expense_on, expenses_on, save_expense_on, save_movement_on,
    save_recurring_on,
};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

fn a_shop(scratch: &Scratch) -> App {
    let path = scratch.dir().join("expenses.db");
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    db.transaction(|tx| {
        Repos::new(tx).menu().save_item(
            OUTLET,
            &mb_db::repo::menu::MenuItem {
                id: mb_core::ItemId::new("itm_dosa"),
                category_id: None,
                name: "Masala Dosa".to_owned(),
                unit_price: mb_core::Money::from_paise(12_000),
                tax_class_id: mb_core::seeded_placement(mb_core::TaxSpec::gst(mb_core::TaxRate::from_percent(5).expect("5%"))).expect("a seeded slab").0,
                price_basis: mb_core::seeded_placement(mb_core::TaxSpec::gst(mb_core::TaxRate::from_percent(5).expect("5%"))).expect("a seeded slab").1,
                hsn: None,
                cost_price: None,
                short_code: None,
                prep_minutes: None,
                course: None,
                is_open_price: false,
                is_available: true,
                sort_order: 0,
            },
            crate::flows::now(),
        )
    })
    .expect("a menu");

    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    app
}

fn spend(app: &App, id: &str, what: &str, amount: &str, mode: &str) -> ExpenseEdit {
    let edit = ExpenseEdit {
        id: id.to_owned(),
        category_id: Some("exc_vegetables".to_owned()),
        description: what.to_owned(),
        amount: amount.to_owned(),
        mode: mode.to_owned(),
        paid_to: "Mandi".to_owned(),
        reference: String::new(),
        gst_percent: String::new(),
        note: String::new(),
    };
    save_expense_on(app, edit.clone()).expect("recorded");
    edit
}

/// The cash position, from a day the commands actually built.
#[test]
fn the_drawer_says_what_should_be_in_it() {
    let scratch = Scratch::new("drawer");
    let app = a_shop(&scratch);

    save_movement_on(
        &app,
        "float".to_owned(),
        "2000".to_owned(),
        "opening".to_owned(),
    )
    .expect("float");

    // A cash sale, through the real billing path.
    app.with_cart_mut(|state| {
        state.set_order_type(mb_core::OrderType::Parcel);
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    let bill = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    app.with_cart_mut(|state| {
        let payment =
            mb_core::Payment::new(mb_core::PaymentMode::Cash, bill).expect("a cash payment");
        state.settlement.add(payment).map_err(|e| {
            crate::words::UiError::new("bill.pay", "That payment could not be taken.")
                .with_detail(e.to_string())
        })
    })
    .expect("paid");
    crate::flows::complete_bill_on(&app, None).expect("settled");

    spend(&app, "exp_veg", "Vegetables", "400", "cash");
    spend(&app, "exp_rent", "Rent", "9000", "bank");
    save_movement_on(
        &app,
        "payout".to_owned(),
        "300".to_owned(),
        "to the boy".to_owned(),
    )
    .expect("payout");
    let view = save_movement_on(
        &app,
        "bank_drop".to_owned(),
        "1000".to_owned(),
        "night drop".to_owned(),
    )
    .expect("drop");

    // Float 2000 + sales 126 + top-ups 0 − expenses 400 − payouts 300 − drops 1000.
    assert_eq!(view.cash.opening_float.paise, 200_000);
    assert_eq!(view.cash.cash_sales.paise, bill.paise());
    assert_eq!(
        view.cash.cash_expenses.paise, 40_000,
        "the bank-paid rent is not out of the drawer",
    );
    assert_eq!(view.cash.payouts.paise, 30_000);
    assert_eq!(view.cash.bank_drops.paise, 100_000);
    assert_eq!(
        view.cash.expected.paise,
        200_000 + bill.paise() - 40_000 - 30_000 - 100_000,
    );
    // And it says the sum out loud, so no screen assembles it.
    assert!(view.cash.says.contains("float"), "{}", view.cash.says);

    // Both expenses are on the day's list; the total is both of them.
    assert_eq!(view.rows.len(), 2);
    assert_eq!(view.total.paise, 940_000);
    // Category totals reconcile with the list by construction.
    let summed: i64 = view.categories.iter().map(|c| c.total.paise).sum();
    assert_eq!(summed, view.total.paise);
}

/// The input credit is extracted from what was paid, and shown as one string.
#[test]
fn a_gst_split_says_the_rate_and_the_money() {
    let scratch = Scratch::new("input_credit");
    let app = a_shop(&scratch);

    let view = save_expense_on(
        &app,
        ExpenseEdit {
            id: "exp_gas".to_owned(),
            category_id: Some("exc_gas".to_owned()),
            description: "Gas cylinder".to_owned(),
            amount: "1180".to_owned(),
            mode: "bank".to_owned(),
            paid_to: "HP".to_owned(),
            reference: "INV-9".to_owned(),
            gst_percent: "18".to_owned(),
            note: String::new(),
        },
    )
    .expect("recorded");

    let gas = view.rows.iter().find(|r| r.id == "exp_gas").expect("there");
    assert_eq!(
        gas.input_credit.as_deref(),
        Some("18% · 180.00"),
        "1,180 at 18% CONTAINS 180 — it is not 1,180 plus tax",
    );
    assert_eq!(gas.mode, "Bank");
    assert_eq!(gas.reference.as_deref(), Some("INV-9"));
}

/// An edit and a delete are both accountable.
#[test]
fn an_edit_and_a_delete_both_leave_a_trail_with_before_and_after() {
    let scratch = Scratch::new("trail");
    let app = a_shop(&scratch);

    spend(&app, "exp_milk", "Milk", "40", "cash");
    let mut edit = spend(&app, "exp_milk", "Milk", "60", "cash");
    edit.mode = "upi".to_owned();
    let view = save_expense_on(&app, edit).expect("edited again");
    assert_eq!(view.rows[0].amount.paise, 6_000);
    assert_eq!(view.rows[0].mode, "UPI");

    let after_delete = delete_expense_on(&app, "exp_milk".to_owned()).expect("deleted");
    assert!(after_delete.rows.is_empty(), "an expense really is deleted");

    let trail = crate::ipc::audit_trail_on(&app, None, None, None).expect("audit");
    let saves = trail
        .entries
        .iter()
        .filter(|e| e.what.contains("Recorded an expense"))
        .count();
    assert!(saves >= 3, "every save is a row: {saves}");
    assert!(
        trail
            .entries
            .iter()
            .any(|e| e.what.contains("Deleted an expense")),
        "and so is the delete",
    );
    // The delete carries what it deleted, which is the point of `before`.
    let deleted = trail
        .entries
        .iter()
        .find(|e| e.what.contains("Deleted an expense"))
        .expect("the delete");
    assert!(
        deleted.before.is_some(),
        "a delete with no before is not evidence"
    );
}

/// A reminder posts NOTHING until somebody confirms it, and confirming twice on one day cannot
/// post it twice.
#[test]
fn a_reminder_is_a_reminder_until_somebody_says_yes() {
    let scratch = Scratch::new("reminder");
    let app = a_shop(&scratch);

    let view = save_recurring_on(
        &app,
        "rec_rent".to_owned(),
        "Shop rent".to_owned(),
        "25000".to_owned(),
        "bank".to_owned(),
        "month".to_owned(),
        Some("exc_rent".to_owned()),
    )
    .expect("a template");

    assert_eq!(view.due.len(), 1, "it is due today");
    assert_eq!(view.due[0].when, "due today");
    assert!(view.rows.is_empty(), "and it has posted nothing");

    let posted = confirm_due_on(&app, "rec_rent".to_owned()).expect("confirmed");
    assert_eq!(posted.rows.len(), 1);
    assert_eq!(posted.rows[0].amount.paise, 2_500_000);
    assert!(posted.due.is_empty(), "and it is no longer due");

    // Confirming again cannot post it twice.
    let again = confirm_due_on(&app, "rec_rent".to_owned()).expect_err("not due");
    assert_eq!(again.code, "expense.not_due");
    assert_eq!(
        expenses_on(&app).expect("view").rows.len(),
        1,
        "still one rent, not two",
    );
}

/// Money leaving a drawer without a reason is how a shortfall becomes an argument, so it is
/// refused.
#[test]
fn a_payout_needs_a_reason_and_an_expense_needs_a_description() {
    let scratch = Scratch::new("refusals");
    let app = a_shop(&scratch);

    let no_reason = save_movement_on(&app, "payout".to_owned(), "100".to_owned(), "  ".to_owned())
        .expect_err("refused");
    assert_eq!(no_reason.code, "cash.reason");

    let nonsense = save_movement_on(
        &app,
        "borrowed".to_owned(),
        "100".to_owned(),
        "a reason".to_owned(),
    )
    .expect_err("refused");
    assert_eq!(nonsense.code, "cash.kind");

    let blank = save_expense_on(
        &app,
        ExpenseEdit {
            id: "exp_blank".to_owned(),
            category_id: None,
            description: "   ".to_owned(),
            amount: "40".to_owned(),
            mode: "cash".to_owned(),
            paid_to: String::new(),
            reference: String::new(),
            gst_percent: String::new(),
            note: String::new(),
        },
    )
    .expect_err("refused");
    assert_eq!(blank.code, "expense.what");
}

/// Two spends made in the very same instant are two spends.
#[test]
fn two_spends_made_in_the_same_instant_are_two_spends() {
    let scratch = Scratch::new("exp_same_instant");
    let app = a_shop(&scratch);

    // The same instant, stated rather than raced for.
    let at = crate::flows::now();
    let first = crate::newid::fresh_at("exp", at);
    let second = crate::newid::fresh_at("exp", at);
    assert_ne!(first, second, "the id is still coming out of the clock");

    spend(&app, &first, "Rent", "1000", "cash");
    spend(&app, &second, "Electricity", "1000", "cash");
    let view = crate::expenses::expenses_on(&app).expect("the screen");

    let recorded: Vec<&str> = view
        .rows
        .iter()
        .map(|r| r.description.as_str())
        .filter(|d| *d == "Rent" || *d == "Electricity")
        .collect();
    assert_eq!(
        recorded.len(),
        2,
        "one spend replaced the other: {recorded:?}"
    );
}

/// And the flow it protects: two reminders due on the same day can both be confirmed.
#[test]
fn two_reminders_due_together_can_both_be_confirmed() {
    let scratch = Scratch::new("recurring_race");
    let app = a_shop(&scratch);

    for (id, what) in [("rec_rent", "Rent"), ("rec_power", "Electricity")] {
        crate::expenses::save_recurring_on(
            &app,
            id.to_owned(),
            what.to_owned(),
            "1000".to_owned(),
            "cash".to_owned(),
            "monthly".to_owned(),
            None,
        )
        .expect("a reminder");
    }

    // Back to back, as fast as the machine goes — which is what "the same instant" means in
    // practice.
    crate::expenses::confirm_due_on(&app, "rec_rent".to_owned()).expect("rent");
    let view = crate::expenses::confirm_due_on(&app, "rec_power".to_owned()).expect("power");

    let recorded: Vec<&str> = view
        .rows
        .iter()
        .map(|r| r.description.as_str())
        .filter(|d| *d == "Rent" || *d == "Electricity")
        .collect();
    assert_eq!(
        recorded.len(),
        2,
        "one spend replaced the other: {recorded:?}",
    );
}
