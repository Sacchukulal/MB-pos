//! **Closing the day, driven end to end** — P18, T5 to T8.
//!
//! Audit **B15** is a whole feature that did not exist, so the thing worth
//! proving is not that a function returns a number: it is that the *sequence* a
//! shop performs every night works, and that the lock it produces is real.
//!
//! Four claims, and each is a night in a real shop:
//!
//! 1. **the expected figure is the drawer's** — the close reads P16's cash
//!    position rather than a second sum of its own (T5);
//! 2. **a difference over the shop's threshold cannot be closed silently** —
//!    the reason is required, ends up on the slip and ends up in the history
//!    (T6);
//! 3. **a closed day refuses a void, and reopening it is the only way past —
//!    which leaves its own audit row** (T7, and D77);
//! 4. **the float carries forward** — tomorrow's drawer starts where tonight's
//!    ended, without tomorrow needing to know a close happened (T8).

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_db::{Db, DbConfig, Repos};

use crate::dayclose::{CountArg, close_on, reopen_on, view_on};
use crate::expenses::save_movement_on;
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

fn a_shop(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    db.transaction(|tx| {
        Repos::new(tx).menu().save_item(
            OUTLET,
            &mb_db::repo::menu::MenuItem {
                id: mb_core::ItemId::new("itm_dosa"),
                category_id: None,
                name: "Masala Dosa".to_owned(),
                unit_price: mb_core::Money::from_paise(12_000),
                tax_rate: mb_core::TaxRate::GST_5,
                tax_treatment: mb_core::TaxTreatment::Exclusive,
                tax_class_id: None,
                hsn: None,
                cost_price: None,
                short_code: None,
                prep_minutes: None,
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

/// One cash sale through the real billing path, so the drawer figure is a
/// figure the product produced rather than one the test typed.
fn a_cash_sale(app: &App) -> mb_core::Money {
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    let total = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    app.with_cart_mut(|state| {
        let payment =
            mb_core::Payment::new(mb_core::PaymentMode::Cash, total).expect("a cash payment");
        state.settlement.add(payment).map_err(|e| {
            crate::words::UiError::new("bill.pay", "That payment could not be taken.")
                .with_detail(e.to_string())
        })
    })
    .expect("paid");
    crate::flows::complete_bill_on(app).expect("settled");
    total
}

/// Notes and coins adding up to exactly `paise`, using the smallest coin for
/// the tail.
#[allow(
    clippy::integer_division,
    reason = "counting change: the quotient is how many notes and the remainder \n              is what is left to count, which is the whole algorithm"
)]
/// Deliberately naive: the point is that the count is a count, not a number.
fn count_of(paise: i64) -> Vec<CountArg> {
    let mut left = paise;
    let mut out = Vec::new();
    for value in [50_000_i64, 20_000, 10_000, 5_000, 2_000, 1_000, 500, 200, 100] {
        let count = left / value;
        if count > 0 {
            out.push(CountArg {
                value: i32::try_from(value).expect("in range"),
                count: u32::try_from(count).expect("in range"),
            });
            left -= count * value;
        }
    }
    assert_eq!(left, 0, "the fixture cannot count {paise} paise");
    out
}

/// **T5.** The expected figure is P16's, and a matching count closes the day.
#[test]
fn a_drawer_that_matches_closes_without_a_reason() {
    let scratch = Scratch::new("day_matches");
    let app = a_shop(&scratch, "matches");

    save_movement_on(&app, "float".to_owned(), "2000".to_owned(), "opening".to_owned())
        .expect("float");
    let sale = a_cash_sale(&app);

    let before = view_on(&app, None).expect("the screen opens");
    // **One answer to "how much should be in the drawer?"** — the same figure
    // the Spends screen shows, not a second sum.
    let position = crate::expenses::expenses_on(&app).expect("spends");
    assert_eq!(before.expected.paise, position.cash.expected.paise);
    assert_eq!(before.expected.paise, 200_000 + sale.paise());
    assert!(!before.is_closed);
    // Nothing counted yet, so the whole drawer reads as missing — and it says
    // so in words rather than as a minus sign.
    assert_eq!(before.variance_kind, "short");
    assert!(before.variance_says.starts_with("Short by"), "{}", before.variance_says);

    let counts = count_of(before.expected.paise);
    let counted = view_on(&app, Some(counts.clone())).expect("counted");
    assert_eq!(counted.counted.paise, before.expected.paise);
    assert_eq!(counted.variance.paise, 0);
    assert_eq!(counted.variance_says, "The drawer matches exactly.");
    assert!(!counted.needs_reason, "an exact drawer needs no explanation");

    let closed = close_on(&app, counts, String::new(), false).expect("closed");
    assert!(closed.is_closed);
    assert!(closed.closed_says.contains("Closed"), "{}", closed.closed_says);
}

/// **T6.** Over the shop's threshold, the reason is required — and it survives
/// onto the record.
#[test]
fn a_short_drawer_cannot_be_closed_without_saying_why() {
    let scratch = Scratch::new("day_short");
    let app = a_shop(&scratch, "short");

    save_movement_on(&app, "float".to_owned(), "2000".to_owned(), "opening".to_owned())
        .expect("float");

    // ₹200 short, against the default ₹20 threshold.
    let expected = view_on(&app, None).expect("open").expected.paise;
    let counts = count_of(expected - 20_000);

    let counted = view_on(&app, Some(counts.clone())).expect("counted");
    assert_eq!(counted.variance_says, "Short by 200.00.");
    assert!(counted.needs_reason);
    // The sentence explains the threshold rather than saying "required".
    assert!(counted.reason_says.contains("20.00"), "{}", counted.reason_says);

    let refused = close_on(&app, counts.clone(), "   ".to_owned(), false)
        .expect_err("it closed a short drawer with no explanation");
    assert_eq!(refused.code, "day.needs_reason");
    // **D75** — the refusal IS the feature, so the refusal is the sentence.
    assert!(refused.message.contains("20.00"), "{}", refused.message);

    let closed = close_on(
        &app,
        counts,
        "paid the vegetable man from the drawer".to_owned(),
        false,
    )
    .expect("closed with a reason");
    assert!(closed.is_closed);
    assert_eq!(closed.reason, "paid the vegetable man from the drawer");

    // And it is in the history, against the person who did it.
    let history = crate::ipc::audit_trail_on(&app, None, None, None).expect("history");
    let row = history
        .entries
        .iter()
        .find(|e| e.what.contains("Closed the day"))
        .expect("the close is in the history");
    assert!(
        row.after.as_deref().unwrap_or_default().contains("vegetable man"),
        "the reason did not reach the history: {row:?}"
    );
}

/// **T7 and D77.** A closed day refuses a void; reopening it is the way past,
/// and reopening leaves its own row.
#[test]
fn a_closed_day_refuses_a_void_until_somebody_opens_it_again() {
    let scratch = Scratch::new("day_lock");
    let app = a_shop(&scratch, "lock");

    save_movement_on(&app, "float".to_owned(), "2000".to_owned(), "opening".to_owned())
        .expect("float");
    a_cash_sale(&app);

    let bills = crate::corrections::list_bills_on(&app).expect("bills");
    let order_id = bills.first().expect("one bill").order_id.clone();

    let expected = view_on(&app, None).expect("open").expected.paise;
    close_on(&app, count_of(expected), String::new(), false).expect("closed");

    let refused = crate::corrections::void_bill_on(
        &app,
        order_id.clone(),
        "wrong table".to_owned(),
        None,
        None,
    )
    .expect_err("it voided into a closed day");
    assert_eq!(refused.code, "void.day_closed");

    // Reopening needs a reason, for the same reason the close does.
    let no_reason = reopen_on(&app, String::new()).expect_err("it opened silently");
    assert_eq!(no_reason.code, "day.reopen_reason");

    let opened = reopen_on(&app, "the 8 o'clock bill was on the wrong table".to_owned())
        .expect("reopened");
    assert!(!opened.is_closed);

    // Now the void goes through.
    crate::corrections::void_bill_on(&app, order_id, "wrong table".to_owned(), None, None)
        .expect("voided after reopening");

    // **Three rows, and an owner can read the whole story.** A hidden override
    // on the void would have left one.
    let history = crate::ipc::audit_trail_on(&app, None, None, None).expect("history");
    let actions: Vec<&str> = history.entries.iter().map(|e| e.what.as_str()).collect();
    assert!(
        actions.iter().any(|a| a.contains("Closed the day")),
        "{actions:?}"
    );
    assert!(
        actions.iter().any(|a| a.contains("Opened a closed day again")),
        "the reopen is not in the history: {actions:?}"
    );
    assert!(actions.iter().any(|a| a.contains("Void")), "{actions:?}");

    // And a day that is not closed cannot be reopened.
    let nothing = reopen_on(&app, "again".to_owned()).expect_err("it reopened an open day");
    assert_eq!(nothing.code, "day.not_closed");
}

/// **T8.** The float carries forward, and tomorrow needs to know nothing about
/// it: the close writes a `float` movement dated tomorrow, which is exactly
/// what `cash_position` already reads.
#[test]
fn the_float_left_in_the_drawer_is_tomorrows_opening() {
    let scratch = Scratch::new("day_float");
    let app = a_shop(&scratch, "float");

    // The shop keeps ₹2,000 overnight.
    crate::settings::ipc::save_on(
        &app,
        vec![
            crate::settings::ipc::SettingEdit {
                key: "day.carry_float".to_owned(),
                value: "true".to_owned(),
            },
            crate::settings::ipc::SettingEdit {
                key: "day.float_amount".to_owned(),
                value: "2000".to_owned(),
            },
        ],
    )
    .expect("saved");

    save_movement_on(&app, "float".to_owned(), "2000".to_owned(), "opening".to_owned())
        .expect("float");
    a_cash_sale(&app);

    let today = crate::flows::today(crate::flows::now());
    let expected = view_on(&app, None).expect("open").expected.paise;
    let view = view_on(&app, None).expect("open");
    assert!(view.carry_says.contains("2000.00"), "{}", view.carry_says);

    close_on(&app, count_of(expected), String::new(), false).expect("closed");

    // Tomorrow's drawer opens with it, through the ordinary cash position.
    let tomorrow = app
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| {
                    Repos::new(tx).money().cash_position(OUTLET, today.next())
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("tomorrow");
    assert_eq!(
        tomorrow.opening_float.paise(),
        200_000,
        "tomorrow started with an empty drawer"
    );
    assert_eq!(tomorrow.expected.paise(), 200_000);
}
