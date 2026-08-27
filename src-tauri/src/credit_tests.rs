//! Customers and credit, driven end to end.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_db::{Db, DbConfig, Repos};

use crate::credit::{
    CustomerEdit, account_on, customers_on, headroom_on, put_on_account_on, record_repayment_on,
    save_adjustment_on, save_customer_on, who_owes_on,
};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

fn a_shop(scratch: &Scratch) -> App {
    let path = scratch.dir().join("credit.db");
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    db.transaction(|tx| {
        Repos::new(tx).menu().save_item(
            OUTLET,
            &mb_db::repo::menu::MenuItem {
                id: mb_core::ItemId::new("itm_dosa"),
                category_id: None,
                name: "Masala Dosa".to_owned(),
                unit_price: mb_core::Money::from_paise(12_000),
                tax: mb_core::TaxSpec::gst(mb_core::TaxRate::from_percent(5).expect("5%")),
                tax_class_id: None,
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

fn edit(id: &str, name: &str, phone: &str, limit: &str) -> CustomerEdit {
    CustomerEdit {
        id: id.to_owned(),
        name: name.to_owned(),
        phone: phone.to_owned(),
        gstin: String::new(),
        address: String::new(),
        credit_limit: limit.to_owned(),
        is_active: true,
    }
}

/// A duplicate phone offers the existing customer, and however the number was typed.
#[test]
fn one_phone_number_is_one_customer_and_the_second_try_says_whose() {
    let scratch = Scratch::new("one_phone");
    let app = a_shop(&scratch);

    save_customer_on(&app, edit("cus_rekha", "Rekha", "+91 98765 43210", "5000")).expect("saved");

    let refused =
        save_customer_on(&app, edit("cus_new", "Somebody", "9876543210", "")).expect_err("refused");
    assert_eq!(refused.code, "credit.duplicate_phone");
    assert!(refused.message.contains("Rekha"), "{}", refused.message);
    // The id rides in the detail so the screen can OPEN them.
    assert_eq!(refused.detail.as_deref(), Some("cus_rekha"));

    // Editing the SAME customer is not a duplicate.
    save_customer_on(
        &app,
        edit("cus_rekha", "Rekha M", "+91 98765 43210", "6000"),
    )
    .expect("her own number is not somebody else's");
}

/// Blank is no limit, which is not a limit of zero — the difference between "may owe anything"
/// and "may owe nothing".
#[test]
fn no_limit_is_not_a_limit_of_zero() {
    let scratch = Scratch::new("no_limit");
    let app = a_shop(&scratch);
    let people =
        save_customer_on(&app, edit("cus_free", "Anand", "9000000009", "")).expect("saved");

    let anand = people.iter().find(|c| c.id == "cus_free").expect("there");
    assert!(anand.credit_limit.is_none(), "blank means no limit");
}

/// The sequence a shop performs: a bill on the account, a repayment later, and the statement
/// adds up at every step.
#[test]
fn a_bill_goes_on_the_account_and_money_comes_back_later() {
    let scratch = Scratch::new("account_flow");
    let app = a_shop(&scratch);
    save_customer_on(&app, edit("cus_regular", "Suresh", "9000000010", "5000")).expect("saved");

    // A bill in the cart.
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("2".to_owned()), None)
        .expect("added");

    // What would this do to the account?
    let room = headroom_on(&app, "cus_regular".to_owned()).expect("headroom");
    assert_eq!(room.verdict, "fine");
    assert!(room.says.contains("Suresh"), "{}", room.says);
    assert!(
        room.says.contains("5,000.00") || room.says.contains("5000.00"),
        "{}",
        room.says
    );

    let cart = put_on_account_on(&app, "cus_regular".to_owned(), false).expect("on the account");
    assert_eq!(cart.balance.paise, 0, "the bill is covered by the account");

    // Settle it, so it becomes a real sale.
    crate::flows::complete_bill_on(&app).expect("settled");

    let account = account_on(&app, "cus_regular".to_owned()).expect("account");
    assert_eq!(account.movements.len(), 1);
    assert_eq!(account.movements[0].kind, "Bill");
    assert!(account.customer.balance.paise > 0, "he owes for the dosas");
    let owed = account.customer.balance.paise;

    // Money comes back, in a REAL payment mode.
    let after = record_repayment_on(
        &app,
        "cus_regular".to_owned(),
        "100".to_owned(),
        "cash".to_owned(),
        String::new(),
    )
    .expect("repaid");

    assert_eq!(after.movements.len(), 2);
    assert_eq!(after.movements[1].kind, "Repayment");
    assert_eq!(after.customer.balance.paise, owed - 10_000);

    // The statement's running column ends at the balance — the property that makes it a
    // statement.
    let last = after.movements.last().expect("a row");
    assert_eq!(last.running.paise, after.customer.balance.paise);
    assert!(after.statement.contains("Suresh"));
    assert!(after.statement.contains("Outstanding"));

    // And he is on the who-owes list.
    let owing = who_owes_on(&app).expect("who owes");
    assert!(owing.iter().any(|c| c.id == "cus_regular"));
}

/// A repayment is real money arriving, so it carries a real mode.
#[test]
fn a_repayment_will_not_take_a_mode_that_is_not_a_payment_mode() {
    let scratch = Scratch::new("real_mode");
    let app = a_shop(&scratch);
    save_customer_on(&app, edit("cus_a", "A", "9000000011", "")).expect("saved");

    let refused = record_repayment_on(
        &app,
        "cus_a".to_owned(),
        "100".to_owned(),
        "Full Settlement".to_owned(),
        String::new(),
    )
    .expect_err("refused");
    assert_eq!(refused.code, "credit.mode");

    for mode in ["cash", "card", "upi"] {
        record_repayment_on(
            &app,
            "cus_a".to_owned(),
            "10".to_owned(),
            mode.to_owned(),
            String::new(),
        )
        .expect("a real mode is taken");
    }
}

/// The limit blocks, and says the numbers.
#[test]
fn a_bill_past_the_limit_is_refused_until_somebody_approves_it() {
    let scratch = Scratch::new("limit");
    let app = a_shop(&scratch);
    save_customer_on(&app, edit("cus_tight", "Tight", "9000000012", "100")).expect("saved");

    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("2".to_owned()), None)
        .expect("added");

    let room = headroom_on(&app, "cus_tight".to_owned()).expect("headroom");
    assert_eq!(room.verdict, "over");
    assert!(
        room.says.contains("100.00"),
        "the message says the limit: {}",
        room.says
    );

    let refused = put_on_account_on(&app, "cus_tight".to_owned(), false).expect_err("refused");
    assert_eq!(refused.code, "credit.over_limit");
    assert!(
        refused.message.contains("Ask somebody"),
        "{}",
        refused.message
    );

    // Approved, it goes on — and the approval leaves a row.
    put_on_account_on(&app, "cus_tight".to_owned(), true).expect("approved");

    let trail = crate::ipc::audit_trail_on(&app, None, None, None).expect("audit");
    assert!(
        trail
            .entries
            .iter()
            .any(|e| e.what.contains("past the credit limit")),
        "an override has a name on it",
    );
}

/// An adjustment needs a reason, because it is the one door that could make money disappear —
/// and both directions are real.
#[test]
fn an_adjustment_needs_a_reason_and_moves_the_balance_both_ways() {
    let scratch = Scratch::new("adjust");
    let app = a_shop(&scratch);
    save_customer_on(&app, edit("cus_adj", "Adj", "9000000013", "")).expect("saved");

    let blank = save_adjustment_on(
        &app,
        "cus_adj".to_owned(),
        "500".to_owned(),
        true,
        "   ".to_owned(),
    )
    .expect_err("refused");
    assert_eq!(blank.code, "credit.reason");

    let opened = save_adjustment_on(
        &app,
        "cus_adj".to_owned(),
        "500".to_owned(),
        true,
        "brought forward from the notebook".to_owned(),
    )
    .expect("an opening balance");
    assert_eq!(opened.customer.balance.paise, 50_000);

    let written_off = save_adjustment_on(
        &app,
        "cus_adj".to_owned(),
        "200".to_owned(),
        false,
        "written off".to_owned(),
    )
    .expect("a write-off");
    assert_eq!(written_off.customer.balance.paise, 30_000);
    assert_eq!(written_off.movements.len(), 2);
}

/// Everything a screen shows about an account comes from the movements, so the list and the
/// account cannot disagree.
#[test]
fn the_list_and_the_account_never_disagree() {
    let scratch = Scratch::new("agree");
    let app = a_shop(&scratch);
    save_customer_on(&app, edit("cus_x", "X", "9000000014", "")).expect("saved");
    save_adjustment_on(
        &app,
        "cus_x".to_owned(),
        "750".to_owned(),
        true,
        "opening".to_owned(),
    )
    .expect("opening");

    let listed = customers_on(&app).expect("list");
    let one = listed.iter().find(|c| c.id == "cus_x").expect("there");
    let account = account_on(&app, "cus_x".to_owned()).expect("account");

    assert_eq!(one.balance.paise, account.customer.balance.paise);
    assert_eq!(one.oldest, account.ageing.oldest);
}

/// A phone number is ten digits, and the phone box is not a notes field.
#[test]
fn a_customer_phone_is_ten_digits_or_it_is_refused() {
    let scratch = Scratch::new("cus_phone");
    let app = a_shop(&scratch);

    for (typed, why) in [
        ("Ravi Kumar", "a name in the phone box"),
        ("98765abcde", "letters among the digits"),
        ("98765", "too few"),
        ("98765432100123", "too many"),
    ] {
        let refused =
            save_customer_on(&app, edit("cus_bad", "Somebody", typed, "")).expect_err(why);
        assert_eq!(refused.code, "credit.phone", "{why}: {typed:?}");
    }

    // And what IS stored is the ten digits, however they were typed — so the row and
    // `phone_key` agree by construction rather than by luck.
    for typed in ["+91 98765 43210", "098765-43210", "9876543210"] {
        let people =
            save_customer_on(&app, edit("cus_ok", "Rekha", typed, "")).expect("a real number");
        let stored = people
            .iter()
            .find(|c| c.id == "cus_ok")
            .and_then(|c| c.phone.clone());
        assert_eq!(stored.as_deref(), Some("9876543210"), "{typed:?}");
    }
}

/// Two records saved in the same instant are two records.
#[test]
fn a_hundred_adjustments_in_a_row_are_a_hundred_adjustments() {
    let scratch = Scratch::new("adj_race");
    let app = a_shop(&scratch);
    save_customer_on(&app, edit("cus_race", "Rekha", "9876543210", "")).expect("a customer");

    for n in 0..100 {
        save_adjustment_on(
            &app,
            "cus_race".to_owned(),
            "10".to_owned(),
            true,
            format!("opening balance {n}"),
        )
        .unwrap_or_else(|e| panic!("adjustment {n} collided: {e:?}"));
    }

    let account = who_owes_on(&app).expect("the screen");
    let rekha = account
        .iter()
        .find(|c| c.id == "cus_race")
        .expect("the customer");
    // A hundred adjustments of ten rupees each, and not one of them lost.
    assert_eq!(
        rekha.balance.paise, 100_000,
        "some adjustments went missing"
    );
}
