//! **Expenses and the drawer** — P16, scope 10.6.
//!
//! The figure that matters here is the **cash position**: what should be in
//! the drawer. P18's day close reads exactly the function these tests exercise,
//! rather than writing a second one that could disagree with it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Scratch;
use common::shop::{self, OUTLET};
use mb_core::expense::Every;
use mb_core::{BusinessDay, Money, StaffId, Timestamp};
use mb_db::Repos;
use mb_db::repo::money::{CashMovement, Expense, ExpenseCategory, Recurring};

fn at(n: i64) -> Timestamp {
    Timestamp::from_millis(1_770_000_000_000 + n * 1_000)
}

fn day() -> BusinessDay {
    BusinessDay::from_days_since_epoch(20_000)
}

fn expense(id: &str, rupees: i64, mode: &str) -> Expense {
    Expense {
        id: id.to_owned(),
        category_id: Some("exc_vegetables".to_owned()),
        description: id.to_owned(),
        amount: Money::from_rupees(rupees).expect("money"),
        mode: mode.to_owned(),
        paid_to: Some("Mandi".to_owned()),
        reference: None,
        gst_rate_bp: None,
        gst_amount: None,
        paid_at: at(1),
        paid_by: Some(StaffId::new("staff_1")),
        business_day: day(),
        note: None,
    }
}

fn movement(id: &str, kind: &str, rupees: i64) -> CashMovement {
    CashMovement {
        id: id.to_owned(),
        kind: kind.to_owned(),
        amount: Money::from_rupees(rupees).expect("money"),
        reason: format!("{kind} for the day"),
        at: at(2),
        business_day: day(),
        moved_by: Some(StaffId::new("staff_1")),
    }
}

/// **The cash position, to the paisa** — and it is one function, because P18
/// reads this one rather than writing a second.
#[test]
fn the_drawer_adds_up() {
    let scratch = Scratch::new("cash_position");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.money().save_cash_movement(OUTLET, &movement("cm_float", "float", 2_000))?;
        repos.money().save_cash_movement(OUTLET, &movement("cm_top", "top_up", 500))?;
        repos.money().save_cash_movement(OUTLET, &movement("cm_pay", "payout", 300))?;
        repos.money().save_cash_movement(OUTLET, &movement("cm_drop", "bank_drop", 1_000))?;
        repos.money().save_expense(OUTLET, &expense("exp_veg", 400, "cash"))?;
        // A bank payment is not the drawer's business.
        repos.money().save_expense(OUTLET, &expense("exp_rent", 9_000, "bank"))?;
        Ok(())
    })
    .expect("a day of movements");

    let position = db
        .transaction(|tx| Repos::new(tx).money().cash_position(OUTLET, day()))
        .expect("position");

    assert_eq!(position.opening_float, Money::from_rupees(2_000).expect("m"));
    assert_eq!(position.top_ups, Money::from_rupees(500).expect("m"));
    assert_eq!(position.payouts, Money::from_rupees(300).expect("m"));
    assert_eq!(position.bank_drops, Money::from_rupees(1_000).expect("m"));
    assert_eq!(
        position.cash_expenses,
        Money::from_rupees(400).expect("m"),
        "the bank-paid rent is not out of the drawer",
    );

    // float + sales + top-ups − expenses − payouts − drops.
    let by_hand = position
        .opening_float
        .add(position.cash_sales)
        .expect("sum")
        .add(position.top_ups)
        .expect("sum")
        .sub(position.cash_expenses)
        .expect("sum")
        .sub(position.payouts)
        .expect("sum")
        .sub(position.bank_drops)
        .expect("sum");
    assert_eq!(position.expected, by_hand);
}

/// **A cash expense is one row, not two.** The movements table holds no row
/// for it, so the two cannot disagree — before or after an edit.
#[test]
fn a_cash_expense_has_no_second_row_to_disagree_with() {
    let scratch = Scratch::new("no_double");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        Repos::new(tx).money().save_expense(OUTLET, &expense("exp_milk", 40, "cash"))
    })
    .expect("saved");

    let before = db
        .transaction(|tx| Repos::new(tx).money().cash_position(OUTLET, day()))
        .expect("position");
    assert_eq!(before.cash_expenses, Money::from_rupees(40).expect("m"));

    let movements = db
        .transaction(|tx| Repos::new(tx).money().list_cash_movements(OUTLET, day()))
        .expect("movements");
    assert!(movements.is_empty(), "no shadow row exists to fall out of step");

    // Edit the amount. One row changes, so one figure changes.
    db.transaction(|tx| {
        Repos::new(tx).money().save_expense(OUTLET, &expense("exp_milk", 60, "cash"))
    })
    .expect("edited");
    let after = db
        .transaction(|tx| Repos::new(tx).money().cash_position(OUTLET, day()))
        .expect("position");
    assert_eq!(after.cash_expenses, Money::from_rupees(60).expect("m"));

    // Change it to a bank payment and it leaves the drawer figure entirely.
    db.transaction(|tx| {
        Repos::new(tx).money().save_expense(OUTLET, &expense("exp_milk", 60, "bank"))
    })
    .expect("moved to the bank");
    let moved = db
        .transaction(|tx| Repos::new(tx).money().cash_position(OUTLET, day()))
        .expect("position");
    assert_eq!(moved.cash_expenses, Money::ZERO);

    // And deleting it removes it from both, because there is only one.
    db.transaction(|tx| Repos::new(tx).money().delete_expense(OUTLET, "exp_milk", at(9)))
        .expect("deleted");
    assert!(
        db.transaction(|tx| Repos::new(tx).money().list_expenses(OUTLET, day()))
            .expect("list")
            .is_empty()
    );
}

/// A category with money against it cannot be deleted, and the refusal says
/// how many — the same rule and sentence shape as a table with bills (P14).
#[test]
fn a_category_in_use_is_hidden_rather_than_deleted() {
    let scratch = Scratch::new("category_in_use");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        Repos::new(tx).money().save_expense(OUTLET, &expense("exp_veg", 100, "cash"))
    })
    .expect("saved");

    let said = db
        .transaction(|tx| {
            Repos::new(tx)
                .money()
                .delete_expense_category(OUTLET, "exc_vegetables", at(3))
        })
        .expect_err("refused")
        .to_string();
    assert!(said.contains("Hide it instead"), "{said}");
    assert!(said.contains('1'), "the refusal says how many: {said}");

    // Hiding works, and keeps the history.
    db.transaction(|tx| {
        Repos::new(tx).money().save_expense_category(
            OUTLET,
            &ExpenseCategory {
                id: "exc_vegetables".to_owned(),
                name: "Vegetables".to_owned(),
                sort_order: 0,
                is_active: false,
            },
            at(4),
        )
    })
    .expect("hidden");

    let categories = db
        .transaction(|tx| Repos::new(tx).money().list_expense_categories(OUTLET))
        .expect("categories");
    let veg = categories.iter().find(|c| c.id == "exc_vegetables").expect("still there");
    assert!(!veg.is_active);
    assert!(categories.len() >= 12, "the seeded list is data, not a hardcoded six");
}

/// The input credit rides on the row, and the database refuses a split that
/// claims more tax than was spent.
#[test]
fn a_gst_split_is_stored_and_cannot_exceed_the_amount() {
    let scratch = Scratch::new("input_credit");
    let db = scratch.open();
    shop::build(&db);

    let mut with_gst = expense("exp_gas", 1_180, "bank");
    with_gst.gst_rate_bp = Some(1_800);
    with_gst.gst_amount = Some(Money::from_rupees(180).expect("m"));

    db.transaction(|tx| Repos::new(tx).money().save_expense(OUTLET, &with_gst))
        .expect("saved");

    let read = db
        .transaction(|tx| Repos::new(tx).money().list_expenses(OUTLET, day()))
        .expect("list");
    let gas = read.iter().find(|e| e.id == "exp_gas").expect("there");
    assert_eq!(gas.gst_rate_bp, Some(1_800));
    assert_eq!(gas.gst_amount, Some(Money::from_rupees(180).expect("m")));

    let mut absurd = expense("exp_absurd", 100, "cash");
    absurd.gst_rate_bp = Some(1_800);
    absurd.gst_amount = Some(Money::from_rupees(500).expect("m"));
    assert!(
        db.transaction(|tx| Repos::new(tx).money().save_expense(OUTLET, &absurd)).is_err(),
        "a shop cannot claim more tax than it spent",
    );
}

/// A template is a reminder, and **nothing is posted until it is confirmed.**
#[test]
fn a_recurring_template_posts_nothing_by_itself() {
    let scratch = Scratch::new("recurring");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        Repos::new(tx).money().save_recurring(
            OUTLET,
            &Recurring {
                id: "rec_rent".to_owned(),
                category_id: Some("exc_rent".to_owned()),
                description: "Shop rent".to_owned(),
                amount: Money::from_rupees(25_000).expect("m"),
                mode: "bank".to_owned(),
                paid_to: Some("Landlord".to_owned()),
                every: Every::Month,
                next_due: day(),
                is_active: true,
            },
            at(1),
        )
    })
    .expect("a template");

    let templates = db
        .transaction(|tx| Repos::new(tx).money().list_recurring(OUTLET))
        .expect("templates");
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].every, Every::Month);
    assert_eq!(templates[0].next_due, day());

    // The point: a due template has NOT written an expense.
    assert!(
        db.transaction(|tx| Repos::new(tx).money().list_expenses(OUTLET, day()))
            .expect("list")
            .is_empty(),
        "silently writing money into a shop's books is not acceptable",
    );
}
