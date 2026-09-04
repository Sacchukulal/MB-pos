//! The business day, driven end to end: the gate, closing, holidays, the lock, and the drawer
//! count beside it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_core::BusinessDay;
use mb_db::{Db, DbConfig, Repos};

use crate::dayclose::{
    CountArg, close_day_on, close_pending_on, count_drawer_on, day_state_on, days_on, drawer_on,
    reopen_day_on, set_holiday_on,
};
use crate::expenses::save_movement_on;
use crate::signin_tests::{Scratch, queue_took};
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
                tax_class_id: mb_core::seeded_placement(mb_core::TaxSpec::gst(
                    mb_core::TaxRate::from_percent(5).expect("5%"),
                ))
                .expect("a seeded slab")
                .0,
                price_basis: mb_core::seeded_placement(mb_core::TaxSpec::gst(
                    mb_core::TaxRate::from_percent(5).expect("5%"),
                ))
                .expect("a seeded slab")
                .1,
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

fn today() -> BusinessDay {
    crate::flows::today(crate::flows::now())
}

/// One cash sale through the real billing path, so the figures are figures the product
/// produced rather than ones the test typed.
fn a_cash_sale(app: &App) -> mb_core::Money {
    app.with_cart_mut(|state| {
        state.set_order_type(mb_core::OrderType::Parcel);
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(app, "itm_dosa".to_owned(), Some("1".to_owned()), None).expect("added");
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
    crate::flows::complete_bill_on(app, None).expect("settled");
    total
}

/// The clock cannot be turned back, so a day in the past is made by moving today's rows to it:
/// the STORED business day is the one thing every report and the gate read.
fn move_today_to(app: &App, day: BusinessDay) {
    let from = today().days_since_epoch();
    let to = day.days_since_epoch();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                for table in ["orders", "payments", "expenses"] {
                    tx.execute(
                        &format!("UPDATE {table} SET business_day = ?1 WHERE business_day = ?2"),
                        [to, from],
                    )?;
                }
                Ok(())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("moved");
}

/// Notes and coins adding up to exactly `paise`, using the smallest coin for the tail.
#[allow(
    clippy::integer_division,
    reason = "counting change: the quotient is how many notes and the remainder is what is left"
)]
fn count_of(paise: i64) -> Vec<CountArg> {
    let mut left = paise;
    let mut out = Vec::new();
    for value in [
        50_000_i64, 20_000, 10_000, 5_000, 2_000, 1_000, 500, 200, 100,
    ] {
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

fn row_for(view: &crate::dayclose::DaysView, day: BusinessDay) -> &crate::dayclose::DayRowView {
    view.days
        .iter()
        .find(|row| row.day == day.to_string())
        .unwrap_or_else(|| panic!("{day} is not in the last fourteen days"))
}

// The gate.

/// Signing in the morning after a day that was never closed: the gate names that day, and one
/// press closes it.
#[test]
fn a_day_left_open_is_pending_until_it_is_closed() {
    let scratch = Scratch::new("day_pending");
    let app = a_shop(&scratch, "pending");
    let yesterday = today().previous();

    // Nothing has ever happened: nothing to ask.
    let quiet = day_state_on(&app, None).expect("the gate");
    assert!(quiet.pending.is_empty(), "{:?}", quiet.pending);
    assert_eq!(quiet.pending_says, "");
    assert_eq!(quiet.action_label, "");
    assert_eq!(quiet.today_state, "open");
    assert!(
        quiet.today_says.starts_with("Today, "),
        "{}",
        quiet.today_says
    );

    a_cash_sale(&app);
    move_today_to(&app, yesterday);

    let state = day_state_on(&app, None).expect("the gate");
    assert_eq!(state.pending.len(), 1, "{:?}", state.pending);
    let row = &state.pending[0];
    assert_eq!(row.day, yesterday.to_string());
    assert_eq!(row.bills, 1);
    assert!(row.net.paise > 0);
    assert_eq!(row.cash.paise, row.net.paise, "a cash sale is cash");
    assert_eq!(row.upi_and_card.paise, 0);
    assert!(!row.looks_like_holiday);
    assert_eq!(row.suggested, "close");
    let weekday = crate::words::weekday(yesterday);
    assert_eq!(state.pending_says, format!("{weekday} was never closed."));
    assert_eq!(state.action_label, format!("Close {weekday}"));
    assert!(state.may_act);
    assert_eq!(state.blocked_says, "");

    close_day_on(&app, yesterday.to_string(), None, String::new(), false).expect("closed");

    let after = day_state_on(&app, None).expect("the gate");
    assert!(after.pending.is_empty(), "{:?}", after.pending);
    let days = days_on(&app).expect("the days");
    let closed = row_for(&days, yesterday);
    assert!(closed.is_locked);
    assert_eq!(closed.state, "closed");
    assert_eq!(closed.kind, "trading");
    assert_eq!(closed.bills, 1, "the figure was frozen with the day");
    assert!(
        closed.closed_says.starts_with("Closed "),
        "{}",
        closed.closed_says
    );

    // Closing it again is refused, and so is a day that has not come.
    let again = close_day_on(&app, yesterday.to_string(), None, String::new(), false)
        .expect_err("closed twice");
    assert_eq!(again.code, "day.already_closed");
    let tomorrow = close_day_on(&app, today().next().to_string(), None, String::new(), false)
        .expect_err("closed tomorrow");
    assert_eq!(tomorrow.code, "day.future");
}

/// Two days with nothing on them look like holidays, and the gate's one press marks both.
#[test]
fn skipped_days_look_like_holidays_and_one_press_marks_them() {
    let scratch = Scratch::new("day_holidays");
    let app = a_shop(&scratch, "holidays");
    let three_ago = today().previous().previous().previous();
    let (two_ago, yesterday) = (three_ago.next(), three_ago.next().next());

    a_cash_sale(&app);
    move_today_to(&app, three_ago);
    close_day_on(&app, three_ago.to_string(), None, String::new(), false).expect("closed");

    let state = day_state_on(&app, None).expect("the gate");
    let days: Vec<&str> = state.pending.iter().map(|p| p.day.as_str()).collect();
    assert_eq!(days, vec![two_ago.to_string(), yesterday.to_string()]);
    assert!(
        state
            .pending
            .iter()
            .all(|p| p.looks_like_holiday && p.suggested == "holiday")
    );
    assert_eq!(state.pending_says, "2 days were never closed.");
    assert_eq!(state.action_label, "Mark 2 holidays");

    // The person switches one back to Close: the words follow.
    let mixed = day_state_on(&app, Some(vec![yesterday.to_string()])).expect("the gate");
    assert_eq!(mixed.pending[0].suggested, "close");
    assert_eq!(mixed.pending[1].suggested, "holiday");
    assert_eq!(mixed.action_label, "Close 1 day and mark 1 holiday");

    let done = close_pending_on(&app, vec![two_ago.to_string(), yesterday.to_string()])
        .expect("one press");
    assert!(done.pending.is_empty(), "{:?}", done.pending);

    let view = days_on(&app).expect("the days");
    for day in [two_ago, yesterday] {
        let row = row_for(&view, day);
        assert!(row.is_locked, "{day} is not locked");
        assert_eq!(row.kind, "holiday");
        assert_eq!(row.state, "holiday");
        assert!(
            row.closed_says.starts_with("Holiday, marked "),
            "{}",
            row.closed_says
        );
    }

    // And the history says so, once per day.
    let history = crate::ipc::audit_trail_on(&app, None, None, None).expect("history");
    let marked = history
        .entries
        .iter()
        .filter(|e| e.what.contains("holiday"))
        .count();
    assert_eq!(marked, 2, "{:?}", history.entries);
}

/// A day with a bill on it was not a holiday, whatever anybody presses.
#[test]
fn a_day_with_bills_cannot_be_a_holiday() {
    let scratch = Scratch::new("day_not_holiday");
    let app = a_shop(&scratch, "not_holiday");
    let yesterday = today().previous();
    a_cash_sale(&app);
    move_today_to(&app, yesterday);

    let refused =
        set_holiday_on(&app, vec![yesterday.to_string()], true).expect_err("a holiday with a bill");
    assert_eq!(refused.code, "day.not_empty");
    assert!(refused.message.contains("1 bill"), "{}", refused.message);

    // The gate ignores the choice too: the day is closed, not marked.
    let state = day_state_on(&app, Some(vec![yesterday.to_string()])).expect("the gate");
    assert_eq!(state.pending[0].suggested, "close");
    let done = close_pending_on(&app, vec![yesterday.to_string()]).expect("closed");
    assert!(done.pending.is_empty());
    let view = days_on(&app).expect("days");
    assert_eq!(row_for(&view, yesterday).kind, "trading");

    // An expense is activity too.
    let scratch = Scratch::new("day_expense");
    let app = a_shop(&scratch, "expense");
    crate::expenses::save_expense_on(
        &app,
        crate::expenses::ExpenseEdit {
            id: "exp_1".to_owned(),
            category_id: None,
            description: "Gas".to_owned(),
            amount: "500".to_owned(),
            mode: "cash".to_owned(),
            paid_to: String::new(),
            reference: String::new(),
            gst_percent: String::new(),
            note: String::new(),
        },
    )
    .expect("an expense");
    move_today_to(&app, yesterday);
    let state = day_state_on(&app, None).expect("the gate");
    assert_eq!(state.pending.len(), 1);
    assert!(!state.pending[0].looks_like_holiday);
    assert_eq!(state.pending[0].expenses.paise, 50_000);
    let refused =
        set_holiday_on(&app, vec![yesterday.to_string()], true).expect_err("with an expense");
    assert_eq!(refused.code, "day.not_empty");
}

/// A holiday for a day that has not come is listed, and it is not something to close.
#[test]
fn a_future_holiday_is_listed_and_not_pending() {
    let scratch = Scratch::new("day_future");
    let app = a_shop(&scratch, "future");
    let sunday = today().next().next();

    set_holiday_on(&app, vec![sunday.to_string()], true).expect("planned");
    let view = days_on(&app).expect("days");
    assert_eq!(view.upcoming.len(), 1, "{:?}", view.upcoming);
    assert_eq!(view.upcoming[0].day, sunday.to_string());
    assert_eq!(view.upcoming[0].state, "holiday");
    assert!(view.may_plan_holiday);

    a_cash_sale(&app);
    let state = day_state_on(&app, None).expect("the gate");
    assert!(state.pending.is_empty(), "{:?}", state.pending);

    // Taking the mark off puts the day back to an ordinary one.
    set_holiday_on(&app, vec![sunday.to_string()], false).expect("unmarked");
    assert!(days_on(&app).expect("days").upcoming.is_empty());
    let twice = set_holiday_on(&app, vec![sunday.to_string()], false).expect_err("unmarked twice");
    assert_eq!(twice.code, "day.not_holiday");
}

/// An order nobody finished keeps its day open, and the gate says so instead of closing over
/// it.
#[test]
fn a_day_with_an_open_order_is_left_for_later() {
    let scratch = Scratch::new("day_open_order");
    let app = a_shop(&scratch, "open_order");
    let yesterday = today().previous();
    app.with_cart_mut(|state| {
        state.set_order_type(mb_core::OrderType::Parcel);
        Ok(())
    })
    .expect("a parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), None, None).expect("a dosa");
    crate::flows::park_open_order(&app).expect("parked");
    move_today_to(&app, yesterday);

    let state = day_state_on(&app, None).expect("the gate");
    assert_eq!(state.pending.len(), 1);
    assert_eq!(state.pending[0].open_orders.len(), 1);
    assert!(
        state.pending[0].open_says.contains("Parcel"),
        "{}",
        state.pending[0].open_says
    );
    assert!(!state.pending[0].looks_like_holiday);
    assert_eq!(state.action_label, "", "nothing can be closed yet");
    assert_eq!(state.escape_label, "Finish the open orders first");

    let refused = close_day_on(&app, yesterday.to_string(), None, String::new(), false)
        .expect_err("closed over an open order");
    assert_eq!(refused.code, "day.open_orders");
    // And the one press leaves it alone rather than failing.
    let after = close_pending_on(&app, Vec::new()).expect("nothing to do is not an error");
    assert_eq!(after.pending.len(), 1);
}

// The lock.

/// A closed day takes no more money: not a bill, not an expense, not a drawer movement.
#[test]
fn a_locked_day_refuses_a_settle_an_expense_and_a_cash_movement() {
    let scratch = Scratch::new("day_settle_lock");
    let app = a_shop(&scratch, "settle_lock");
    a_cash_sale(&app);
    close_day_on(&app, today().to_string(), None, String::new(), false).expect("closed");

    app.with_cart_mut(|state| {
        state.set_order_type(mb_core::OrderType::Parcel);
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    let total = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    crate::ipc::cart_add_payment_on(&app, "Cash".to_owned(), total.paise(), None).expect("paid");
    let refused =
        crate::flows::complete_bill_on(&app, None).expect_err("settled into a closed day");
    assert_eq!(refused.code, "bill.day_closed");
    assert!(
        refused
            .message
            .ends_with("Open it again under Day close to keep billing."),
        "{}",
        refused.message
    );
    assert!(
        refused.message.starts_with("That day was closed at "),
        "{}",
        refused.message
    );

    let expense = crate::expenses::save_expense_on(
        &app,
        crate::expenses::ExpenseEdit {
            id: "exp_late".to_owned(),
            category_id: None,
            description: "Milk".to_owned(),
            amount: "80".to_owned(),
            mode: "cash".to_owned(),
            paid_to: String::new(),
            reference: String::new(),
            gst_percent: String::new(),
            note: String::new(),
        },
    )
    .expect_err("an expense into a closed day");
    assert_eq!(expense.code, "expense.day_closed");

    let moved = save_movement_on(
        &app,
        "payout".to_owned(),
        "100".to_owned(),
        "tea".to_owned(),
    )
    .expect_err("a payout from a closed day");
    assert_eq!(moved.code, "cash.day_closed");

    // Opened again, the bill goes through.
    reopen_day_on(
        &app,
        today().to_string(),
        "the last customer came back".to_owned(),
    )
    .expect("reopened");
    crate::flows::complete_bill_on(&app, None).expect("settled after reopening");
}

#[test]
fn a_closed_day_refuses_a_void_until_somebody_opens_it_again() {
    let scratch = Scratch::new("day_lock");
    let app = a_shop(&scratch, "lock");
    a_cash_sale(&app);

    let bills = crate::corrections::list_bills_on(&app).expect("bills");
    let order_id = bills.first().expect("one bill").order_id.clone();
    close_day_on(&app, today().to_string(), None, String::new(), false).expect("closed");

    let refused = crate::corrections::void_bill_on(
        &app,
        order_id.clone(),
        "wrong table".to_owned(),
        None,
        None,
    )
    .expect_err("it voided into a closed day");
    assert_eq!(refused.code, "void.day_closed");
    assert!(refused.message.contains("Day close"), "{}", refused.message);

    // Reopening needs a reason, for the same reason the close leaves a mark.
    let no_reason =
        reopen_day_on(&app, today().to_string(), String::new()).expect_err("it opened silently");
    assert_eq!(no_reason.code, "day.reopen_reason");

    let opened = reopen_day_on(
        &app,
        today().to_string(),
        "the 8 o'clock bill was on the wrong table".to_owned(),
    )
    .expect("reopened");
    assert_eq!(opened.today_state, "open");

    crate::corrections::void_bill_on(&app, order_id, "wrong table".to_owned(), None, None)
        .expect("voided after reopening");

    // Three rows, and an owner can read the whole story.
    let history = crate::ipc::audit_trail_on(&app, None, None, None).expect("history");
    let actions: Vec<&str> = history.entries.iter().map(|e| e.what.as_str()).collect();
    assert!(
        actions.iter().any(|a| a.contains("Closed the day")),
        "{actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|a| a.contains("Opened a closed day again")),
        "the reopen is not in the history: {actions:?}"
    );
    assert!(actions.iter().any(|a| a.contains("Void")), "{actions:?}");

    // And a day that is not closed cannot be reopened.
    let nothing = reopen_day_on(&app, today().to_string(), "again".to_owned())
        .expect_err("it reopened an open day");
    assert_eq!(nothing.code, "day.not_closed");
}

/// Opening and closing again freezes the figures once and carries the float once.
#[test]
fn reopen_then_close_counts_nothing_twice() {
    let scratch = Scratch::new("day_twice");
    let app = a_shop(&scratch, "twice");
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
    let sale = a_cash_sale(&app);
    let day = today();

    let view = days_on(&app).expect("days");
    assert!(view.carry_says.contains("2000.00"), "{}", view.carry_says);

    close_day_on(&app, day.to_string(), None, String::new(), false).expect("closed");
    reopen_day_on(&app, day.to_string(), "forgot a bill".to_owned()).expect("reopened");
    a_cash_sale(&app);
    close_day_on(&app, day.to_string(), None, String::new(), false).expect("closed again");

    let (row, tomorrow) = app
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| {
                    let repos = Repos::new(tx);
                    Ok((
                        repos.days().find(OUTLET, day)?,
                        repos.money().cash_position(OUTLET, day.next())?,
                    ))
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("read back");
    let row = row.expect("the day has a row");
    assert!(row.is_locked);
    assert_eq!(row.bills, 2, "two bills, counted once each");
    assert_eq!(row.net.paise(), sale.paise() * 2);
    assert_eq!(row.cash_taken, row.net);
    assert!(row.reopened_at.is_some(), "the reopen left its mark");
    // Tomorrow starts with ONE float, through the ordinary cash position.
    assert_eq!(tomorrow.opening_float.paise(), 200_000);
}

/// What was on the shelf when the day ended is written with the day (B11).
#[test]
fn closing_the_day_freezes_the_stock() {
    let scratch = Scratch::new("day_stock");
    let app = a_shop(&scratch, "stock");
    app.use_licensing(crate::licence_tests::licence_in(
        &scratch,
        "day-stock-licence",
        mb_license::Status::Active,
        90,
    ));
    crate::inventory::save_material_on(
        &app,
        crate::inventory::MaterialEdit {
            id: "mat_rice".to_owned(),
            name: "Rice".to_owned(),
            dimension: "weight".to_owned(),
            category: "Dry goods".to_owned(),
            buy_from: String::new(),
            reorder_level: "1".to_owned(),
            reorder_qty: "2".to_owned(),
            reorder_unit: "bag".to_owned(),
            is_perishable: false,
            shelf_life_days: None,
            is_active: true,
            packs: vec![crate::inventory::PackEdit {
                name: "bag".to_owned(),
                size: "25".to_owned(),
                unit: "kg".to_owned(),
            }],
            purchase_unit: "bag".to_owned(),
            recipe_unit: "g".to_owned(),
        },
    )
    .expect("a material");
    crate::inventory::record_movement_on(
        &app,
        crate::inventory::MovementEdit {
            material_id: "mat_rice".to_owned(),
            kind: "purchase".to_owned(),
            qty: "2".to_owned(),
            unit: "bag".to_owned(),
            reason_id: None,
            note: None,
            cost: Some("1500".to_owned()),
        },
    )
    .expect("bought");

    close_day_on(&app, today().to_string(), None, String::new(), false).expect("closed");

    let closing: i64 = app
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| {
                    Ok(tx.query_row(
                        "SELECT closing_qty FROM stock_day_closes
                          WHERE outlet_id = ?1 AND business_day = ?2 AND material_id = 'mat_rice'",
                        (OUTLET, today().days_since_epoch()),
                        |r| r.get(0),
                    )?)
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("the closing figure is there");
    assert_eq!(
        closing, 50_000_000,
        "two 25 kg bags, in thousandths of a gram"
    );
}

// The drawer.

#[test]
fn a_drawer_that_matches_is_counted_without_a_reason_and_locks_nothing() {
    let scratch = Scratch::new("drawer_matches");
    let app = a_shop(&scratch, "matches");

    save_movement_on(
        &app,
        "float".to_owned(),
        "2000".to_owned(),
        "opening".to_owned(),
    )
    .expect("float");
    let sale = a_cash_sale(&app);

    let before = drawer_on(&app, None).expect("the drawer");
    // One answer to "how much should be in the drawer?" — the same figure the Spends screen
    // shows, not a second sum.
    let position = crate::expenses::expenses_on(&app).expect("spends");
    assert_eq!(before.expected.paise, position.cash.expected.paise);
    assert_eq!(before.expected.paise, 200_000 + sale.paise());
    assert_eq!(before.counted_says, "");
    // Nothing counted yet, so the whole drawer reads as missing — in words.
    assert_eq!(before.variance_kind, "short");
    assert!(
        before.variance_says.starts_with("Short by"),
        "{}",
        before.variance_says
    );

    let counts = count_of(before.expected.paise);
    let counted = drawer_on(&app, Some(counts.clone())).expect("counted");
    assert_eq!(counted.counted.paise, before.expected.paise);
    assert_eq!(counted.variance.paise, 0);
    assert_eq!(counted.variance_says, "The drawer matches exactly.");
    assert!(
        !counted.needs_reason,
        "an exact drawer needs no explanation"
    );

    let written = count_drawer_on(&app, counts, String::new(), false).expect("counted");
    assert!(
        written.counted_says.starts_with("Counted "),
        "{}",
        written.counted_says
    );

    // The count is not the close: the day is still open for billing.
    assert_eq!(day_state_on(&app, None).expect("gate").today_state, "open");
    a_cash_sale(&app);
}

/// Over the shop's threshold, the reason is required — and it survives onto the record.
#[test]
fn a_short_drawer_cannot_be_counted_without_saying_why() {
    let scratch = Scratch::new("drawer_short");
    let app = a_shop(&scratch, "short");
    save_movement_on(
        &app,
        "float".to_owned(),
        "2000".to_owned(),
        "opening".to_owned(),
    )
    .expect("float");

    // ₹200 short, against the default ₹20 threshold.
    let expected = drawer_on(&app, None).expect("open").expected.paise;
    let counts = count_of(expected - 20_000);

    let counted = drawer_on(&app, Some(counts.clone())).expect("counted");
    assert_eq!(counted.variance_says, "Short by 200.00.");
    assert!(counted.needs_reason);
    assert!(
        counted.reason_says.contains("20.00"),
        "{}",
        counted.reason_says
    );

    let refused = count_drawer_on(&app, counts.clone(), "   ".to_owned(), false)
        .expect_err("it wrote a short drawer with no explanation");
    assert_eq!(refused.code, "drawer.needs_reason");
    assert!(refused.message.contains("20.00"), "{}", refused.message);

    let written = count_drawer_on(
        &app,
        counts,
        "paid the vegetable man from the drawer".to_owned(),
        false,
    )
    .expect("counted with a reason");
    assert_eq!(written.reason, "paid the vegetable man from the drawer");

    let history = crate::ipc::audit_trail_on(&app, None, None, None).expect("history");
    let row = history
        .entries
        .iter()
        .find(|e| e.what.contains("Counted the drawer"))
        .expect("the count is in the history");
    assert!(
        row.after
            .as_deref()
            .unwrap_or_default()
            .contains("vegetable man"),
        "the reason did not reach the history: {row:?}"
    );
}

/// The counting slip is a JOB, not a log line.
#[test]
fn counting_the_drawer_with_the_slip_actually_queues_it() {
    let scratch = Scratch::new("drawer_slip");
    let app = a_shop(&scratch, "slip");
    let expected = a_cash_sale(&app).paise();

    let (counted, took) = queue_took(&app, || {
        count_drawer_on(&app, count_of(expected), String::new(), true)
    });
    counted.expect("counted");
    assert!(
        took.contains(&mb_print::queue::JobKind::DayClose),
        "the drawer was counted and no slip reached the queue: {took:?}"
    );
}

/// The shift handover report.
#[test]
fn the_handover_report_names_the_shift_the_person_and_the_difference() {
    let scratch = Scratch::new("drawer_handover");
    let app = a_shop(&scratch, "handover");
    app.use_licensing(crate::licence_tests::licence_in(
        &scratch,
        "handover-licence",
        mb_license::Status::Active,
        90,
    ));
    let expected = a_cash_sale(&app).paise();

    count_drawer_on(
        &app,
        count_of(expected - 10_000),
        "Paid the milkman from the drawer".to_owned(),
        false,
    )
    .expect("counted");

    let day = today().to_string();
    let report = crate::reports::report_on(
        &app,
        "handover".to_owned(),
        crate::reports::PeriodArg {
            from: day.clone(),
            to: day,
        },
    )
    .expect("the handover report");

    let row = report.rows.first().expect("one drawer was counted");
    assert!(row.iter().any(|c| c.contains("Short by")), "{row:?}");
    assert!(row.iter().any(|c| c.contains("milkman")), "{row:?}");
    assert!(
        report.notes.iter().any(|n| n.contains("came up short")),
        "{:?}",
        report.notes
    );
}

// The switch.

/// Turn this shop's day closing on or off, the way the settings screen does.
fn closing(app: &App, on: bool) {
    let mut config = app.shop_config();
    config.day.must_close = on;
    app.publish_shop_config(config);
}

/// Switched off: no gate, no lock, nothing refused — and what each day came to is still there.
#[test]
fn a_shop_that_does_not_close_its_days_is_never_asked_and_never_refused() {
    let scratch = Scratch::new("day_switch_off");
    let app = a_shop(&scratch, "switch_off");
    a_cash_sale(&app);
    move_today_to(&app, today().previous());

    // On, and yesterday is at the gate.
    let asked = day_state_on(&app, None).expect("the gate");
    assert_eq!(asked.pending.len(), 1, "yesterday should be pending");

    closing(&app, false);
    let quiet = day_state_on(&app, None).expect("the gate");
    assert!(quiet.pending.is_empty(), "the gate came up anyway");
    assert_eq!(quiet.action_label, "", "there is nothing to press");

    let view = days_on(&app).expect("the days");
    assert!(!view.may_act, "nothing on the screen may be pressed");
    assert!(!view.may_plan_holiday);
    assert!(
        view.closing_says.contains("does not close its days"),
        "{}",
        view.closing_says
    );
    // The figures are still figures: switching it off hides no money.
    assert!(
        view.days.iter().any(|row| row.bills > 0),
        "yesterday's bills went missing"
    );

    // And the writes are refused as a switch that is off, not silently ignored.
    for refused in [
        close_day_on(&app, today().to_string(), None, String::new(), false).expect_err("closed"),
        set_holiday_on(&app, vec![today().next().to_string()], true).expect_err("holiday"),
        close_pending_on(&app, Vec::new()).expect_err("pending"),
    ] {
        assert_eq!(refused.code, "day.closing_off");
        assert!(
            refused.message.contains("Close the day every day"),
            "the refusal does not name the setting: {}",
            refused.message
        );
    }
}

/// A day that was locked while the shop still closed its days stops binding when the switch
/// goes off — and binds again when it comes back on. Nothing is rewritten either way.
#[test]
fn the_switch_decides_whether_a_closed_day_still_refuses_money() {
    let scratch = Scratch::new("day_switch_lock");
    let app = a_shop(&scratch, "switch_lock");
    a_cash_sale(&app);
    close_day_on(&app, today().to_string(), None, String::new(), false).expect("closed");

    assert_eq!(
        another_cash_sale(&app)
            .expect_err("billed into a closed day")
            .code,
        "bill.day_closed"
    );

    closing(&app, false);
    another_cash_sale(&app).expect("the switch is off, so nothing is locked");

    closing(&app, true);
    assert_eq!(
        another_cash_sale(&app).expect_err("locked again").code,
        "bill.day_closed",
        "the day is still closed, so the switch coming back on must bind it again"
    );
}

/// One more cash sale down the real billing path, returning whatever the counter answered.
fn another_cash_sale(app: &App) -> crate::words::UiResult<String> {
    app.with_cart_mut(|state| {
        state.set_order_type(mb_core::OrderType::Parcel);
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(app, "itm_dosa".to_owned(), Some("1".to_owned()), None).expect("added");
    let total = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    crate::ipc::cart_add_payment_on(app, "Cash".to_owned(), total.paise(), None).expect("paid");
    crate::flows::complete_bill_on(app, None).map(|_| String::new())
}

// The lock, everywhere.

/// The one gate, on every path that moves money in a day. Before this, a closed day refused a
/// bill and an expense and let a credit collection, a supplier payment, a staff advance and a
/// rider's handback straight through — six ways round a lock the schema calls the one every
/// money path checks.
#[test]
fn a_closed_day_refuses_every_path_that_moves_money_in_it() {
    let scratch = Scratch::new("day_every_path");
    let app = a_shop(&scratch, "every_path");
    app.use_licensing(crate::licence_tests::licence_in(
        &scratch,
        "every-path-licence",
        mb_license::Status::Active,
        90,
    ));
    a_cash_sale(&app);

    // Somebody to owe money, and somebody to be owed it.
    crate::credit::save_customer_on(
        &app,
        crate::credit::CustomerEdit {
            id: "cus_ravi".to_owned(),
            name: "Ravi".to_owned(),
            phone: "9876500001".to_owned(),
            gstin: String::new(),
            address: String::new(),
            credit_limit: "5000".to_owned(),
            is_active: true,
        },
    )
    .expect("a customer");
    crate::buying::save_supplier_on(
        &app,
        crate::buying::SupplierEdit {
            id: "sup_veg".to_owned(),
            name: "Green Grocer".to_owned(),
            phone: "9876500002".to_owned(),
            gstin: String::new(),
            address: String::new(),
            terms_days: "0".to_owned(),
            note: String::new(),
            is_active: true,
        },
    )
    .expect("a supplier");

    close_day_on(&app, today().to_string(), None, String::new(), false).expect("closed");

    // Credit collected in cash is a note going into a drawer that has been settled.
    assert_eq!(
        crate::credit::record_repayment_on(
            &app,
            "cus_ravi".to_owned(),
            "200".to_owned(),
            "cash".to_owned(),
            String::new(),
        )
        .expect_err("a repayment into a closed day")
        .code,
        "credit.day_closed"
    );

    // A supplier paid in cash is a note coming out of it.
    assert_eq!(
        crate::buying::record_supplier_payment_on(
            &app,
            "sup_veg".to_owned(),
            "500".to_owned(),
            "cash".to_owned(),
            String::new(),
        )
        .expect_err("a supplier paid from a closed day")
        .code,
        "payment.day_closed"
    );

    // And the order nobody should be cooking for: refused BEFORE the kitchen, not after it.
    app.with_cart_mut(|state| {
        state.set_order_type(mb_core::OrderType::Parcel);
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    assert_eq!(
        crate::flows::park_open_order(&app)
            .expect_err("parked into a closed day")
            .code,
        "order.day_closed",
        "the kitchen would have cooked for a day the till will not settle"
    );
}

// Closing with the drawer, in one press.

/// The count is optional and it travels with the day: one press writes the drawer and locks the
/// day, through the same path the count button uses.
#[test]
fn closing_the_day_takes_the_drawer_count_with_it_and_does_not_need_one() {
    let scratch = Scratch::new("day_close_with_count");
    let app = a_shop(&scratch, "close_with_count");
    let expected = a_cash_sale(&app).paise();

    let view = close_day_on(
        &app,
        today().to_string(),
        Some(count_of(expected)),
        String::new(),
        false,
    )
    .expect("closed with the drawer counted");
    assert_eq!(view.today_state, "closed");

    // The drawer was written, once, and it says what was counted.
    let drawer = drawer_on(&app, None).expect("the drawer");
    assert_eq!(drawer.counted.paise, expected);
    assert_eq!(drawer.variance_says, "The drawer matches exactly.");
    assert!(
        drawer.counted_says.starts_with("Counted "),
        "{}",
        drawer.counted_says
    );

    // A second shop closes without ever touching the grid, and closes just the same.
    let plain = a_shop(&scratch, "close_without_count");
    a_cash_sale(&plain);
    let view = close_day_on(&plain, today().to_string(), None, String::new(), false)
        .expect("closed with no count at all");
    assert_eq!(view.today_state, "closed");
    assert_eq!(
        drawer_on(&plain, None).expect("the drawer").counted_says,
        "",
        "a day closed without a count must not claim one"
    );
}

/// A drawer out by more than the shop's threshold still has to say why — and that refusal
/// arrives with the day still open, not with it locked behind.
#[test]
fn closing_with_a_short_drawer_and_no_reason_leaves_the_day_open() {
    let scratch = Scratch::new("day_close_short");
    let app = a_shop(&scratch, "close_short");
    let expected = a_cash_sale(&app).paise();

    let refused = close_day_on(
        &app,
        today().to_string(),
        Some(count_of(expected - 10_000)),
        String::new(),
        false,
    )
    .expect_err("closed a short drawer with no reason");
    assert_eq!(refused.code, "drawer.needs_reason");
    assert_eq!(
        days_on(&app).expect("the days").today_state,
        "open",
        "the day was closed behind a refusal the person still has to answer"
    );

    // With the reason, both happen.
    close_day_on(
        &app,
        today().to_string(),
        Some(count_of(expected - 10_000)),
        "Paid the milkman from the drawer".to_owned(),
        false,
    )
    .expect("closed");
    assert_eq!(days_on(&app).expect("the days").today_state, "closed");
}

/// A day that has gone takes no drawer count: the box under this till is today's.
#[test]
fn a_day_that_has_gone_cannot_be_closed_with_todays_drawer() {
    let scratch = Scratch::new("day_close_old_count");
    let app = a_shop(&scratch, "close_old_count");
    a_cash_sale(&app);
    let yesterday = today().previous();
    move_today_to(&app, yesterday);

    let refused = close_day_on(
        &app,
        yesterday.to_string(),
        Some(count_of(50_000)),
        String::new(),
        false,
    )
    .expect_err("counted a drawer for a day that has gone");
    assert_eq!(refused.code, "day.count_not_today");
    // And without one it closes normally.
    close_day_on(&app, yesterday.to_string(), None, String::new(), false).expect("closed");
}

/// The day rule reaches the screen that closes days, in the words a person reads.
#[test]
fn the_day_screen_says_when_the_shops_day_starts() {
    let scratch = Scratch::new("day_runs_says");
    let app = a_shop(&scratch, "runs_says");

    let view = days_on(&app).expect("the days");
    assert!(
        view.day_runs_says.contains("5:00 am"),
        "the standard rule is not said: {}",
        view.day_runs_says
    );

    let mut config = app.shop_config();
    config.day.starts_at_minutes = 390; // 06:30
    app.publish_shop_config(config);
    assert!(
        days_on(&app)
            .expect("the days")
            .day_runs_says
            .contains("6:30 am"),
        "the shop's rule is not the one said"
    );

    let mut config = app.shop_config();
    config.day.starts_at_minutes = 0;
    app.publish_shop_config(config);
    assert!(
        days_on(&app)
            .expect("the days")
            .day_runs_says
            .contains("midnight"),
        "a midnight rule should read as midnight, not as a clock time"
    );
}

/// A close that is going to be refused writes nothing: no drawer count is left behind by a
/// press that did not close the day.
#[test]
fn a_close_refused_for_an_open_order_leaves_no_drawer_count_behind() {
    let scratch = Scratch::new("day_close_refused");
    let app = a_shop(&scratch, "close_refused");
    let expected = a_cash_sale(&app).paise();

    // A table nobody finished.
    app.with_cart_mut(|state| {
        state.set_order_type(mb_core::OrderType::Parcel);
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    crate::flows::park_open_order(&app).expect("parked");

    let refused = close_day_on(
        &app,
        today().to_string(),
        Some(count_of(expected)),
        String::new(),
        false,
    )
    .expect_err("closed over an open order");
    assert_eq!(refused.code, "day.open_orders");
    assert_eq!(
        drawer_on(&app, None).expect("the drawer").counted_says,
        "",
        "the press was refused and still wrote a count"
    );
}

// The drawer figure.

/// Cash handed over against a credit account is a note in the box. Leaving it out read as
/// "over" by exactly that amount, every day a shop collected on an account.
#[test]
fn credit_collected_in_cash_is_in_the_drawer_it_landed_in() {
    let scratch = Scratch::new("day_credit_drawer");
    let app = a_shop(&scratch, "credit_drawer");
    let takings = a_cash_sale(&app).paise();

    crate::credit::save_customer_on(
        &app,
        crate::credit::CustomerEdit {
            id: "cus_ravi".to_owned(),
            name: "Ravi".to_owned(),
            phone: "9876500001".to_owned(),
            gstin: String::new(),
            address: String::new(),
            credit_limit: "5000".to_owned(),
            is_active: true,
        },
    )
    .expect("a customer");

    let before = drawer_on(&app, None).expect("the drawer");
    assert_eq!(before.expected.paise, takings);

    crate::credit::record_repayment_on(
        &app,
        "cus_ravi".to_owned(),
        "200".to_owned(),
        "cash".to_owned(),
        String::new(),
    )
    .expect("collected");

    let after = drawer_on(&app, None).expect("the drawer");
    assert_eq!(
        after.expected.paise,
        takings + 20_000,
        "the drawer does not expect the money that was put in it"
    );
    // And it is a line a person can point at, not a number that moved on its own.
    let line = after
        .drawer
        .iter()
        .find(|row| row.label == "Credit collected in cash")
        .expect("no line says where it came from");
    assert_eq!(line.amount.paise, 20_000);

    // Counting exactly that much is exact, which is the whole point.
    let counted = drawer_on(&app, Some(count_of(takings + 20_000))).expect("counted");
    assert_eq!(counted.variance_says, "The drawer matches exactly.");

    // Taken by UPI it is real money, but it is not in the box.
    crate::credit::record_repayment_on(
        &app,
        "cus_ravi".to_owned(),
        "100".to_owned(),
        "upi".to_owned(),
        String::new(),
    )
    .expect("collected");
    assert_eq!(
        drawer_on(&app, None).expect("the drawer").expected.paise,
        takings + 20_000,
        "a UPI repayment put itself in the cash drawer"
    );
}

// The day's own record.

/// The reason somebody gave to open a day again is still there after it closes again — that is
/// the only reason to have asked for one.
#[test]
fn the_reason_a_day_was_opened_again_survives_the_next_close() {
    let scratch = Scratch::new("day_reopen_note");
    let app = a_shop(&scratch, "reopen_note");
    a_cash_sale(&app);
    close_day_on(&app, today().to_string(), None, String::new(), false).expect("closed");
    reopen_day_on(
        &app,
        today().to_string(),
        "the last customer came back".to_owned(),
    )
    .expect("reopened");
    close_day_on(&app, today().to_string(), None, String::new(), false).expect("closed again");

    let row = app
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| Repos::new(tx).days().find(OUTLET, today()))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("the day")
        .expect("a row");
    assert_eq!(row.note.as_deref(), Some("the last customer came back"));
    assert!(row.reopened_at.is_some(), "the mark went with the reason");
}

/// A day that is already closed is a record. Calling it a holiday is refused until somebody
/// opens it again, which leaves a mark.
#[test]
fn a_closed_day_cannot_be_relabelled_a_holiday() {
    let scratch = Scratch::new("day_relabel");
    let app = a_shop(&scratch, "relabel");
    // A quiet day: closed with nothing on it at all.
    close_day_on(&app, today().to_string(), None, String::new(), false).expect("closed");

    let refused =
        set_holiday_on(&app, vec![today().to_string()], true).expect_err("relabelled a close");
    assert_eq!(refused.code, "day.already_closed");
    assert!(
        refused.message.contains("Open it again first"),
        "{}",
        refused.message
    );
    // And the screen does not offer the button either.
    let view = days_on(&app).expect("the days");
    assert!(!row_for(&view, today()).may_be_holiday);
    assert_eq!(row_for(&view, today()).state, "closed");
}

/// A day the shop banked its takings and collected on an account was a day somebody stood in
/// it, whatever the bill count says.
#[test]
fn a_day_with_money_moving_and_no_bills_is_not_offered_as_a_holiday() {
    let scratch = Scratch::new("day_moved");
    let app = a_shop(&scratch, "moved");
    save_movement_on(
        &app,
        "bank_drop".to_owned(),
        "5000".to_owned(),
        "took it to the bank".to_owned(),
    )
    .expect("banked");

    let view = days_on(&app).expect("the days");
    assert!(
        !row_for(&view, today()).may_be_holiday,
        "a day the shop banked 5000 was offered as a holiday"
    );
    let refused =
        set_holiday_on(&app, vec![today().to_string()], true).expect_err("called it a holiday");
    assert_eq!(refused.code, "day.not_empty");
}

/// Opening a day again takes back the float it left for the next one, so tomorrow does not
/// expect money nobody left in the drawer.
#[test]
fn opening_a_day_again_takes_back_the_float_it_left() {
    let scratch = Scratch::new("day_float_back");
    let app = a_shop(&scratch, "float_back");
    let mut config = app.shop_config();
    config.day.carry_float = true;
    config.day.float_amount = mb_core::Money::from_paise(200_000);
    app.publish_shop_config(config);

    a_cash_sale(&app);
    let float_of = |app: &App, day: BusinessDay| -> i64 {
        app.with_shop(|shop| {
            shop.db
                .read_transaction(|tx| {
                    Ok(Repos::new(tx)
                        .money()
                        .cash_position_of(OUTLET, day, None)?
                        .opening_float
                        .paise())
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("the float")
    };

    close_day_on(&app, today().to_string(), None, String::new(), false).expect("closed");
    assert_eq!(float_of(&app, today().next()), 200_000, "no float was left");

    reopen_day_on(&app, today().to_string(), "a bill was missed".to_owned()).expect("reopened");
    assert_eq!(
        float_of(&app, today().next()),
        0,
        "tomorrow still expects a float from a day that was never finished"
    );

    // Closing it again leaves it once, not twice.
    close_day_on(&app, today().to_string(), None, String::new(), false).expect("closed again");
    assert_eq!(float_of(&app, today().next()), 200_000);
}
