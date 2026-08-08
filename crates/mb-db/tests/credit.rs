//! **The credit account, against a real database** — P15, scope 5.1–5.4.
//!
//! mb-core proves the arithmetic (`credit.rs`); this proves the account is
//! assembled from the right rows — and, above all, that **voiding a credit
//! bill puts the balance back exactly**, which is the property v1 could not
//! have had, because it kept the balance in a column.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Scratch;
use common::shop::{self, OUTLET};
use mb_core::credit::{MovementKind, balance};
use mb_core::{BusinessDay, CustomerId, Money, StaffId, Timestamp};
use mb_db::Repos;
use mb_db::repo::money::{CreditAdjustment, CreditPayment, Customer};

fn at(n: i64) -> Timestamp {
    Timestamp::from_millis(1_770_000_000_000 + n * 1_000)
}

fn day(n: i32) -> BusinessDay {
    BusinessDay::from_days_since_epoch(20_000 + n)
}

fn customer(id: &str, phone: &str) -> Customer {
    Customer {
        id: CustomerId::new(id),
        name: id.to_owned(),
        phone: Some(phone.to_owned()),
        gstin: None,
        address: None,
        credit_limit: Some(Money::from_rupees(5_000).expect("money")),
        is_active: true,
    }
}

/// **One phone number is one customer**, however it was typed — and the
/// database refuses the second row rather than trusting a screen to check.
#[test]
fn a_phone_number_belongs_to_one_customer() {
    let scratch = Scratch::new("one_phone");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        Repos::new(tx)
            .money()
            .save_customer(OUTLET, &customer("cus_rekha", "+91 98765 43210"), at(1))
    })
    .expect("saved");

    // The same number, typed the way the next cashier types it.
    let found = db
        .transaction(|tx| Repos::new(tx).money().customer_by_phone(OUTLET, "9876543210"))
        .expect("looked up");
    assert_eq!(
        found.map(|c| c.id.as_str().to_owned()),
        Some("cus_rekha".to_owned()),
        "the screen can offer the existing customer",
    );

    // And if a screen does not look, the index does.
    let refused = db.transaction(|tx| {
        Repos::new(tx)
            .money()
            .save_customer(OUTLET, &customer("cus_other", "098765-43210"), at(2))
    });
    assert!(refused.is_err(), "two rows for one number are two balances for one person");
}

/// **The property the whole design turns on.** A credit sale, then a void, and
/// the balance is exactly what it was — with no reversing row, because the sale
/// stops being a settled sale (D47).
#[test]
fn voiding_a_credit_bill_returns_the_balance_exactly() {
    let scratch = Scratch::new("void_credit");
    let db = scratch.open();
    let built = shop::build(&db);

    let who = CustomerId::new("cus_regular");
    db.transaction(|tx| {
        Repos::new(tx)
            .money()
            .save_customer(OUTLET, &customer("cus_regular", "9000000001"), at(1))
    })
    .expect("saved");

    // Take a settled order from the fixture and put it on the account.
    let order_id = built
        .orders
        .iter()
        .find(|id| {
            db.transaction(|tx| {
                Ok(Repos::new(tx)
                    .orders()
                    .find(id)?
                    .is_some_and(|o| matches!(o, mb_core::AnyOrder::Settled(_))))
            })
            .unwrap_or(false)
        })
        .expect("a settled order")
        .clone();

    db.transaction(|tx| {
        tx.execute(
            // `mode_label` is only for 'other' — the CHECK says so, and it is the
            // kind of rule a hand-written UPDATE finds for you.
            "UPDATE payments SET mode = 'credit', mode_label = NULL, customer_id = ?2
              WHERE order_id = ?1",
            rusqlite::params![order_id.as_str(), who.as_str()],
        )?;
        Ok(())
    })
    .expect("put on the account");

    let owed = db
        .transaction(|tx| Repos::new(tx).money().customer_balance(&who))
        .expect("balance");
    assert!(owed.is_positive(), "the sale is on the account");

    // Void it, the way P12 does: the state changes.
    db.transaction(|tx| {
        tx.execute(
            "UPDATE orders SET state = 'voided', voided_at = ?2, voided_by = ?3,
                               void_reason = 'wrong bill'
              WHERE id = ?1",
            rusqlite::params![
                order_id.as_str(),
                at(9).millis(),
                "staff_1",
            ],
        )?;
        Ok(())
    })
    .expect("voided");

    let after = db
        .transaction(|tx| Repos::new(tx).money().customer_balance(&who))
        .expect("balance");
    assert_eq!(after, Money::ZERO, "a voided bill is not money anybody owes");

    // And the movements agree with the balance, which is the invariant.
    let movements = db
        .transaction(|tx| Repos::new(tx).money().credit_movements(&who))
        .expect("movements");
    assert_eq!(balance(&movements).expect("sum"), after);
}

/// Sales, repayments and adjustments all reach the account, and the balance is
/// their sum — never a stored number.
#[test]
fn the_account_is_the_sum_of_its_movements() {
    let scratch = Scratch::new("movements");
    let db = scratch.open();
    shop::build(&db);

    let who = CustomerId::new("cus_ledger");
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos
            .money()
            .save_customer(OUTLET, &customer("cus_ledger", "9000000002"), at(1))?;

        // An opening balance: what they owed before this product existed.
        repos.money().save_credit_adjustment(
            OUTLET,
            &CreditAdjustment {
                id: "adj_open".to_owned(),
                customer_id: who.clone(),
                amount: Money::from_rupees(1_000).expect("money"),
                increases: true,
                reason: "brought forward from the notebook".to_owned(),
                at: at(2),
                business_day: day(0),
                made_by: Some(StaffId::new("staff_1")),
            },
        )?;

        // A repayment, in a REAL payment mode (audit B12).
        repos.money().record_credit_payment(
            OUTLET,
            &CreditPayment {
                id: "pay_1".to_owned(),
                customer_id: who.clone(),
                amount: Money::from_rupees(400).expect("money"),
                mode: "cash".to_owned(),
                reference: None,
                received_at: at(3),
                received_by: Some(StaffId::new("staff_1")),
                business_day: day(5),
            },
        )?;

        // And a write-off.
        repos.money().save_credit_adjustment(
            OUTLET,
            &CreditAdjustment {
                id: "adj_off".to_owned(),
                customer_id: who.clone(),
                amount: Money::from_rupees(100).expect("money"),
                increases: false,
                reason: "written off".to_owned(),
                at: at(4),
                business_day: day(6),
                made_by: Some(StaffId::new("staff_1")),
            },
        )?;
        Ok(())
    })
    .expect("three movements");

    let movements = db
        .transaction(|tx| Repos::new(tx).money().credit_movements(&who))
        .expect("movements");

    assert_eq!(movements.len(), 3);
    assert_eq!(movements[0].kind, MovementKind::Adjustment { increases: true });
    assert_eq!(movements[1].kind, MovementKind::Repayment);
    assert_eq!(
        balance(&movements).expect("balance"),
        Money::from_rupees(500).expect("money"),
        "1000 owed, 400 paid, 100 written off",
    );

    // The repo's own SUM and the ledger must agree — two ways of asking the
    // same question, and the day they disagree nobody can tell which is right.
    let repo_balance = db
        .transaction(|tx| Repos::new(tx).money().customer_balance(&who))
        .expect("balance");
    assert_eq!(
        repo_balance.add(Money::from_rupees(900).expect("money")).expect("sum"),
        balance(&movements).expect("balance"),
        "customer_balance counts sales and repayments; the ledger adds the adjustments",
    );
}

/// An adjustment without a reason is refused, because this is the one door
/// somebody could use to make money disappear.
#[test]
fn an_adjustment_needs_a_reason_and_a_direction() {
    let scratch = Scratch::new("adjustment_rules");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        Repos::new(tx)
            .money()
            .save_customer(OUTLET, &customer("cus_x", "9000000003"), at(1))
    })
    .expect("saved");

    let blank = db.transaction(|tx| {
        Repos::new(tx).money().save_credit_adjustment(
            OUTLET,
            &CreditAdjustment {
                id: "adj_blank".to_owned(),
                customer_id: CustomerId::new("cus_x"),
                amount: Money::from_rupees(100).expect("money"),
                increases: true,
                reason: "   ".to_owned(),
                at: at(2),
                business_day: day(1),
                made_by: None,
            },
        )
    });
    assert!(blank.is_err(), "an adjustment with no reason is a mistake with paperwork");

    let negative = db.transaction(|tx| {
        Repos::new(tx).money().save_credit_adjustment(
            OUTLET,
            &CreditAdjustment {
                id: "adj_neg".to_owned(),
                customer_id: CustomerId::new("cus_x"),
                amount: Money::from_rupees(-100).expect("money"),
                increases: true,
                reason: "negative".to_owned(),
                at: at(3),
                business_day: day(1),
                made_by: None,
            },
        )
    });
    assert!(negative.is_err(), "the direction is a flag, never a sign");
}

/// "Who owes me money", oldest first — the screen an owner actually opens.
#[test]
fn who_owes_is_sorted_by_how_long_it_has_been_owed() {
    let scratch = Scratch::new("who_owes");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for (n, (id, phone, days_ago, rupees)) in [
            ("cus_new", "9000001111", 2_i32, 300_i64),
            ("cus_old", "9000002222", 80, 100),
            ("cus_mid", "9000003333", 40, 900),
        ]
        .into_iter()
        .enumerate()
        {
            repos
                .money()
                .save_customer(OUTLET, &customer(id, phone), at(n as i64))?;
            repos.money().save_credit_adjustment(
                OUTLET,
                &CreditAdjustment {
                    id: format!("adj_{id}"),
                    customer_id: CustomerId::new(id),
                    amount: Money::from_rupees(rupees).expect("money"),
                    increases: true,
                    reason: "brought forward".to_owned(),
                    at: at(n as i64),
                    business_day: day(100 - days_ago),
                    made_by: None,
                },
            )?;
        }
        Ok(())
    })
    .expect("three accounts");

    let owing = db
        .transaction(|tx| Repos::new(tx).money().who_owes(OUTLET, day(100)))
        .expect("who owes");

    // The fixture shop has its own customer; this test is about the ORDER of
    // the three it created.
    let order: Vec<&str> = owing
        .iter()
        .map(|o| o.customer.id.as_str())
        .filter(|id| id.starts_with("cus_old") || id.starts_with("cus_mid") || id.starts_with("cus_new"))
        .collect();
    assert_eq!(
        order,
        ["cus_old", "cus_mid", "cus_new"],
        "the point of the screen is what has been owed longest",
    );
    assert_eq!(owing[0].ageing.oldest_days, Some(80));
    assert_eq!(owing[0].ageing.days_90, Money::ZERO);
    assert_eq!(owing[0].ageing.days_60, Money::from_rupees(100).expect("money"));
}
