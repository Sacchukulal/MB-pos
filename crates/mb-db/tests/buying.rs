#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Scratch;
use common::{OUTLET, STAFF_SQL};
use mb_core::purchase::{Entry, Invoice};
use mb_core::{
    BusinessDay, Dimension, MaterialId, Money, Pack, Qty, Registration, RoundingMode, StaffId,
    Timestamp, UnitCost,
};
use mb_db::repo::buying::{self, PurchaseKind, Supplier, SupplierPayment};
use mb_db::repo::stock::Material;
use mb_db::{Db, Repos};

fn at(n: i64) -> Timestamp {
    Timestamp::from_millis(1_770_000_000_000 + n * 1_000)
}

fn day() -> BusinessDay {
    BusinessDay::from_days_since_epoch(20_700)
}

/// What somebody wrote on the sheet: the truth and the label.
fn written(base: Qty, typed: Qty, unit: &str) -> mb_db::repo::counts::Written {
    mb_db::repo::counts::Written {
        base,
        typed,
        unit: unit.to_owned(),
    }
}

fn grams(n: i64) -> Qty {
    Qty::from_whole(n).expect("in range")
}

fn rupees(n: i64) -> Money {
    Money::from_paise(n * 100)
}

/// A 25 kg bag of rice.
fn bag() -> Pack {
    Pack::new("bag", grams(25_000)).expect("a pack")
}

fn base() -> Pack {
    Pack::new("g", Qty::ONE).expect("a pack")
}

fn rice() -> Material {
    let mut m = Material::new(MaterialId::new("mat_rice"), "Rice", Dimension::Weight);
    m.packs = vec![("bag".to_owned(), grams(25_000))];
    m.purchase_unit = Some("bag".to_owned());
    m
}

fn paneer() -> Material {
    Material::new(MaterialId::new("mat_paneer"), "Paneer", Dimension::Weight)
}

fn metro() -> Supplier {
    let mut s = Supplier::new("sup_metro", "Metro");
    s.terms_days = 15;
    s
}

/// A shop with two materials, one supplier and one member of staff.
fn shop_with_materials() -> (Scratch, Db) {
    let scratch = Scratch::new("buying");
    let db = scratch.open();
    db.transaction(|tx| {
        tx.execute_batch(STAFF_SQL)?;
        let repos = Repos::new(tx);
        repos.stock().save_material(OUTLET, &rice(), at(0))?;
        repos.stock().save_material(OUTLET, &paneer(), at(0))?;
        repos.buying().save_supplier(OUTLET, &metro(), at(0))
    })
    .expect("seed the shop");
    (scratch, db)
}

/// One line, in bags, at a rate, with a free quantity.
fn line(qty: i64, free: i64, rate: Money, pack: Pack) -> Entry {
    Entry {
        typed_qty: Qty::from_whole(qty).expect("in range"),
        free_typed_qty: Qty::from_whole(free).expect("in range"),
        pack,
        rate,
        discount: Money::ZERO,
        tax_rate_bp: 0,
    }
}

fn invoice(lines: Vec<Entry>) -> Invoice {
    Invoice {
        lines,
        invoice_discount: Money::ZERO,
        charges: Money::ZERO,
        tax_is_creditable: false,
        rounding: RoundingMode::None,
    }
}

/// Write a delivery and return it as it was stored.
fn deliver(
    db: &Db,
    id: &str,
    materials: &[(MaterialId, String)],
    invoice: &Invoice,
    on: BusinessDay,
    at: Timestamp,
) -> buying::Purchase {
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let supplier = repos
            .buying()
            .supplier(OUTLET, "sup_metro")?
            .expect("the supplier is on file");
        let draft = buying::draft(id, &supplier, on, at, materials, invoice)?;
        repos.buying().record_purchase(OUTLET, &draft)
    })
    .expect("record the delivery")
}

#[test]
fn t1_two_bags_land_on_the_shelf_at_forty_rupees_a_kilo() {
    let (_scratch, db) = shop_with_materials();
    deliver(
        &db,
        "pur_1",
        &[(MaterialId::new("mat_rice"), "bag".to_owned())],
        &invoice(vec![line(2, 0, rupees(1_000), bag())]),
        day(),
        at(1),
    );

    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        // 2 bags × 25 kg = 50,000 g.
        assert_eq!(
            repos
                .stock()
                .balance(OUTLET, &MaterialId::new("mat_rice"))?,
            grams(50_000)
        );
        // ₹2,000 for 50 kg is ₹40 a kilo, which is 4000 paise per 1,000 g.
        let material = repos
            .stock()
            .material(OUTLET, &MaterialId::new("mat_rice"))?;
        assert_eq!(
            material.expect("rice").avg_cost,
            UnitCost::from_paise_per_thousand(4_000)
        );
        Ok(())
    })
    .expect("read it back");
}

#[test]
fn t1_the_row_still_reads_two_bags_after_the_bag_size_changes() {
    // Store only "2 bags" and correcting the bag size rewrites last month; store only base
    // units and the buy list says "bring 50,000 g".
    let (_scratch, db) = shop_with_materials();
    deliver(
        &db,
        "pur_1",
        &[(MaterialId::new("mat_rice"), "bag".to_owned())],
        &invoice(vec![line(2, 0, rupees(1_000), bag())]),
        day(),
        at(1),
    );

    db.transaction(|tx| {
        let mut changed = rice();
        changed.packs = vec![("bag".to_owned(), grams(26_000))];
        Repos::new(tx)
            .stock()
            .save_material(OUTLET, &changed, at(2))
    })
    .expect("change the bag size");

    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        let purchase = repos.buying().purchase(OUTLET, "pur_1")?.expect("on file");
        assert_eq!(purchase.lines[0].typed_qty, Qty::from_whole(2).expect("2"));
        assert_eq!(purchase.lines[0].typed_unit, "bag");
        assert_eq!(purchase.lines[0].base_qty, grams(50_000));
        // And the shelf did not move because a pack was renamed.
        assert_eq!(
            repos
                .stock()
                .balance(OUTLET, &MaterialId::new("mat_rice"))?,
            grams(50_000)
        );
        Ok(())
    })
    .expect("read it back");
}

#[test]
fn t2_the_weighted_average_is_exact_across_a_sequence() {
    // Hand-computed, step by step, and asserted against the arithmetic rather than against what
    // the code happened to produce:.
    let (_scratch, db) = shop_with_materials();
    let mat = [(MaterialId::new("mat_rice"), "bag".to_owned())];

    deliver(
        &db,
        "pur_1",
        &mat,
        &invoice(vec![line(2, 0, rupees(1_000), bag())]),
        day(),
        at(1),
    );
    assert_eq!(
        avg_cost(&db, "mat_rice"),
        UnitCost::from_paise_per_thousand(4_000)
    );

    deliver(
        &db,
        "pur_2",
        &mat,
        &invoice(vec![line(1, 0, rupees(1_300), bag())]),
        day(),
        at(2),
    );
    assert_eq!(
        avg_cost(&db, "mat_rice"),
        UnitCost::from_paise_per_thousand(4_400)
    );

    // Twenty-five kilos leave as wastage.
    db.transaction(|tx| {
        let movement = mb_db::repo::stock::Movement::new(
            "mov_waste",
            MaterialId::new("mat_rice"),
            mb_db::repo::stock::MovementKind::Wastage,
            grams(-25_000),
            at(3),
            day(),
        );
        Repos::new(tx).stock().record(OUTLET, &movement)
    })
    .expect("waste some rice");
    assert_eq!(
        avg_cost(&db, "mat_rice"),
        UnitCost::from_paise_per_thousand(4_400)
    );

    deliver(
        &db,
        "pur_3",
        &mat,
        &invoice(vec![line(1, 0, rupees(1_000), bag())]),
        day(),
        at(4),
    );
    // (50,000 × 4400 + 25,000 × 4000) ÷ 75,000 = 4266.66…, rounded once.
    assert_eq!(
        avg_cost(&db, "mat_rice"),
        UnitCost::from_paise_per_thousand(4_267)
    );

    let before = balance(&db, "mat_rice");
    db.transaction(|tx| Repos::new(tx).stock().rebuild_balances(OUTLET, at(5)))
        .expect("rebuild");
    assert_eq!(balance(&db, "mat_rice"), before);
}

fn avg_cost(db: &Db, material: &str) -> UnitCost {
    db.read_transaction(|tx| {
        Ok(Repos::new(tx)
            .stock()
            .material(OUTLET, &MaterialId::new(material))?
            .expect("the material")
            .avg_cost)
    })
    .expect("read the cost")
}

fn balance(db: &Db, material: &str) -> Qty {
    db.read_transaction(|tx| {
        Repos::new(tx)
            .stock()
            .balance(OUTLET, &MaterialId::new(material))
    })
    .expect("read the balance")
}

#[test]
fn t3_a_free_bag_reaches_the_shelf_and_lowers_the_cost() {
    // The free bag is a DENOMINATOR.
    let (_scratch, db) = shop_with_materials();
    deliver(
        &db,
        "pur_1",
        &[(MaterialId::new("mat_rice"), "bag".to_owned())],
        &invoice(vec![line(10, 1, rupees(1_000), bag())]),
        day(),
        at(1),
    );
    assert_eq!(balance(&db, "mat_rice"), grams(275_000));
    assert_eq!(
        avg_cost(&db, "mat_rice"),
        UnitCost::from_paise_per_thousand(3_636)
    );
}

// The four ledgers reconcile.

#[test]
fn t5_a_cash_delivery_moves_the_shelf_the_ledger_and_the_drawer_and_nothing_else() {
    let (_scratch, db) = shop_with_materials();
    let purchase = deliver(
        &db,
        "pur_1",
        &[(MaterialId::new("mat_rice"), "bag".to_owned())],
        &invoice(vec![line(2, 0, rupees(1_000), bag())]),
        day(),
        at(1),
    );

    db.transaction(|tx| {
        Repos::new(tx).buying().record_payment(
            OUTLET,
            &SupplierPayment {
                id: "spay_1".to_owned(),
                supplier_id: "sup_metro".to_owned(),
                amount: purchase.total,
                mode: "cash".to_owned(),
                reference: None,
                purchase_id: Some(purchase.id.clone()),
                paid_at: at(2),
                business_day: day(),
                paid_by: None,
                note: None,
            },
        )
    })
    .expect("pay at the door");

    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        // The shelf moved.
        assert_eq!(
            repos
                .stock()
                .balance(OUTLET, &MaterialId::new("mat_rice"))?,
            grams(50_000)
        );
        // Nothing is owed.
        assert_eq!(repos.buying().supplier_balance("sup_metro")?, Money::ZERO);
        // The drawer paid it out.
        assert_eq!(repos.buying().cash_paid_out(OUTLET, day())?, rupees(2_000));

        // And there is no second row for the same fact.
        let expenses: i64 = tx.query_row("SELECT count(*) FROM expenses", [], |r| r.get(0))?;
        let cash: i64 = tx.query_row("SELECT count(*) FROM cash_movements", [], |r| r.get(0))?;
        assert_eq!(
            (expenses, cash),
            (0, 0),
            "a purchase writes neither of these"
        );
        Ok(())
    })
    .expect("read it back");
}

#[test]
fn t5_a_credit_delivery_leaves_the_drawer_alone_and_the_balance_owing() {
    let (_scratch, db) = shop_with_materials();
    deliver(
        &db,
        "pur_1",
        &[(MaterialId::new("mat_rice"), "bag".to_owned())],
        &invoice(vec![line(2, 0, rupees(1_000), bag())]),
        day(),
        at(1),
    );

    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        assert_eq!(repos.buying().cash_paid_out(OUTLET, day())?, Money::ZERO);
        assert_eq!(repos.buying().supplier_balance("sup_metro")?, rupees(2_000));
        Ok(())
    })
    .expect("read it back");
}

#[test]
fn t6_the_supplier_balance_is_always_the_sum_of_its_rows() {
    let (_scratch, db) = shop_with_materials();
    let mat = [(MaterialId::new("mat_rice"), "bag".to_owned())];

    // A month: four deliveries, two part-payments and a write-off.
    let mut expected = Money::ZERO;
    for n in 0..4 {
        let purchase = deliver(
            &db,
            &format!("pur_{n}"),
            &mat,
            &invoice(vec![line(2, 0, rupees(1_000), bag())]),
            BusinessDay::from_days_since_epoch(day().days_since_epoch() + n),
            at(i64::from(n) + 1),
        );
        expected = expected.add(purchase.total).expect("adds");
    }

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for (n, amount) in [(1, rupees(1_500)), (2, rupees(2_500))] {
            repos.buying().record_payment(
                OUTLET,
                &SupplierPayment {
                    id: format!("spay_{n}"),
                    supplier_id: "sup_metro".to_owned(),
                    amount,
                    mode: "upi".to_owned(),
                    reference: None,
                    purchase_id: None,
                    paid_at: at(10 + n),
                    business_day: day(),
                    paid_by: None,
                    note: None,
                },
            )?;
        }
        repos.buying().save_adjustment(
            OUTLET,
            &buying::SupplierAdjustment {
                id: "sadj_1".to_owned(),
                supplier_id: "sup_metro".to_owned(),
                amount: rupees(200),
                increases: false,
                reason: "Damaged bag, agreed on the phone".to_owned(),
                at: at(20),
                business_day: day(),
                made_by: None,
            },
        )
    })
    .expect("pay and adjust");

    let expected = expected
        .sub(rupees(1_500))
        .and_then(|m| m.sub(rupees(2_500)))
        .and_then(|m| m.sub(rupees(200)))
        .expect("adds");
    db.read_transaction(|tx| {
        assert_eq!(
            Repos::new(tx).buying().supplier_balance("sup_metro")?,
            expected
        );
        Ok(())
    })
    .expect("read it back");
}

#[test]
fn t7_a_purchase_ages_from_its_due_day_and_not_its_invoice_day() {
    // A payment term is a shift of the date, not a second algorithm.
    let (_scratch, db) = shop_with_materials();
    deliver(
        &db,
        "pur_1",
        &[(MaterialId::new("mat_rice"), "bag".to_owned())],
        &invoice(vec![line(2, 0, rupees(1_000), bag())]),
        day(),
        at(1),
    );

    let ageing_on = |offset: i32| {
        db.read_transaction(|tx| {
            let today = BusinessDay::from_days_since_epoch(day().days_since_epoch() + offset);
            Ok(Repos::new(tx)
                .buying()
                .outstanding(OUTLET, today)?
                .first()
                .expect("one supplier owes money")
                .ageing)
        })
        .expect("age the account")
    };

    // Fourteen days after the invoice is one day BEFORE it is due, and the ageing says so with
    // a negative number — "due tomorrow".
    assert_eq!(
        ageing_on(14).oldest_days,
        Some(-1),
        "due tomorrow, not overdue"
    );
    // Twenty days after the invoice is five days past due.
    assert_eq!(ageing_on(20).oldest_days, Some(5));

    // The same rows with no terms age from the invoice day instead.
    db.transaction(|tx| {
        let mut cash_and_carry = metro();
        cash_and_carry.terms_days = 0;
        Repos::new(tx)
            .buying()
            .save_supplier(OUTLET, &cash_and_carry, at(30))
    })
    .expect("change the terms");
    // And last month does not move, because `due_day` was frozen at entry.
    assert_eq!(ageing_on(20).oldest_days, Some(5));
}

// Cancel, and a return at the cost it arrived at.

#[test]
fn t12_cancelling_a_delivery_negates_it_at_the_original_cost() {
    let (_scratch, db) = shop_with_materials();
    deliver(
        &db,
        "pur_1",
        &[(MaterialId::new("mat_rice"), "bag".to_owned())],
        &invoice(vec![line(2, 0, rupees(1_000), bag())]),
        day(),
        at(1),
    );

    db.transaction(|tx| {
        Repos::new(tx).buying().cancel_purchase(
            OUTLET,
            "pur_1",
            "Entered twice",
            Some(&StaffId::new("staff_1")),
            at(5),
        )
    })
    .expect("cancel it");

    assert_eq!(balance(&db, "mat_rice"), Qty::ZERO);
    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        let purchase = repos
            .buying()
            .purchase(OUTLET, "pur_1")?
            .expect("still on file");
        // The paper is still there, marked, and the reason with it.
        assert!(purchase.cancelled.is_some());
        assert_eq!(
            purchase.cancelled.expect("cancelled").reason,
            "Entered twice"
        );
        // And it is out of the supplier's ledger.
        assert_eq!(repos.buying().supplier_balance("sup_metro")?, Money::ZERO);
        Ok(())
    })
    .expect("read it back");
}

#[test]
fn t12_a_return_leaves_at_what_those_goods_cost_when_they_came() {
    let (_scratch, db) = shop_with_materials();
    let mat = [(MaterialId::new("mat_rice"), "bag".to_owned())];

    let first = deliver(
        &db,
        "pur_1",
        &mat,
        &invoice(vec![line(10, 0, rupees(1_000), bag())]),
        day(),
        at(1),
    );
    deliver(
        &db,
        "pur_2",
        &mat,
        &invoice(vec![line(10, 0, rupees(1_500), bag())]),
        day(),
        at(2),
    );
    // (250,000 × 4000 + 250,000 × 6000) ÷ 500,000 = 5000 paise per kilo.
    assert_eq!(
        avg_cost(&db, "mat_rice"),
        UnitCost::from_paise_per_thousand(5_000)
    );

    let original_cost = first.lines[0].landed_unit_cost;
    assert_eq!(original_cost, UnitCost::from_paise_per_thousand(4_000));

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let supplier = repos
            .buying()
            .supplier(OUTLET, "sup_metro")?
            .expect("on file");
        let mut back = buying::draft(
            "pur_ret_1",
            &supplier,
            day(),
            at(3),
            &mat,
            &invoice(vec![line(2, 0, rupees(1_000), bag())]),
        )?;
        back.kind = PurchaseKind::Return;
        back.parent_id = Some("pur_1".to_owned());
        back.lines[0].returns_seq = Some(1);
        // The cost travels from the parent line, never from today's average.
        back.lines[0].landed_unit_cost = original_cost;
        repos.buying().record_purchase(OUTLET, &back)?;
        Ok(())
    })
    .expect("send two bags back");

    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        // Twenty bags in, two back out.
        assert_eq!(
            repos
                .stock()
                .balance(OUTLET, &MaterialId::new("mat_rice"))?,
            grams(450_000)
        );
        // The movement is valued at ₹40 a kilo — 2 bags × 25 kg × ₹40 = ₹2,000.
        let value: i64 = tx.query_row(
            "SELECT total_cost FROM stock_movements WHERE id = 'mov_pur_ret_1_1'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(
            value, -200_000,
            "the goods leave at what they cost coming in"
        );
        // Eight of the original ten are still returnable.
        assert_eq!(repos.buying().returnable("pur_1", 1)?, grams(200_000));
        Ok(())
    })
    .expect("read it back");
}

#[test]
fn t8_the_count_posts_a_delta_so_mondays_delivery_survives() {
    let (_scratch, db) = shop_with_materials();
    db.transaction(|tx| {
        let movement = mb_db::repo::stock::Movement::new(
            "mov_open",
            MaterialId::new("mat_paneer"),
            mb_db::repo::stock::MovementKind::Opening,
            grams(12_000),
            at(1),
            day(),
        )
        .costing(UnitCost::from_paise_per_thousand(40_000));
        Repos::new(tx).stock().record(OUTLET, &movement)
    })
    .expect("opening stock");

    // Sunday night.
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos
            .counts()
            .open(OUTLET, "cnt_1", "Store", day(), at(2), None)?;
        repos.counts().record_line(
            OUTLET,
            "cnt_1",
            &MaterialId::new("mat_paneer"),
            &written(grams(10_000), Qty::from_whole(10).expect("10"), "kg"),
            at(3),
        )?;
        Ok(())
    })
    .expect("count the store");

    // Monday morning: a delivery, before anybody approves anything.
    deliver(
        &db,
        "pur_1",
        &[(MaterialId::new("mat_paneer"), "g".to_owned())],
        &invoice(vec![line(25_000, 0, Money::from_paise(40), base())]),
        day(),
        at(4),
    );
    assert_eq!(balance(&db, "mat_paneer"), grams(37_000));

    // Monday nine o'clock.
    let moved = db
        .transaction(|tx| {
            Repos::new(tx).counts().approve(
                OUTLET,
                "cnt_1",
                at(5),
                day(),
                Some(&StaffId::new("staff_1")),
            )
        })
        .expect("approve the count");
    assert_eq!(moved, 1);

    // 37 − 2 = 35 kg.
    assert_eq!(balance(&db, "mat_paneer"), grams(35_000));

    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        let count = repos.counts().count(OUTLET, "cnt_1")?.expect("on file");
        // The line still shows the book as it was at 11 pm on Sunday.
        assert_eq!(count.lines[0].book_qty, grams(12_000));
        assert_eq!(count.lines[0].variance_qty, grams(-2_000));
        // Valued at cost: 2 kg of paneer at ₹400 a kilo is ₹800 short.
        assert_eq!(count.lines[0].variance_value, rupees(-800));
        // And the adjustment posted is the variance, not the counted figure.
        let adjustment: i64 = tx.query_row(
            "SELECT base_qty FROM stock_movements WHERE id = 'mov_cnt_cnt_1_1'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(adjustment, -2_000_000, "the delta, never the count");
        assert!(
            repos
                .stock()
                .material(OUTLET, &MaterialId::new("mat_paneer"))?
                .expect("paneer")
                .last_counted_at
                .is_some()
        );
        Ok(())
    })
    .expect("read it back");
}

#[test]
fn t9_an_approved_count_is_sealed_and_a_second_open_one_is_refused() {
    let (_scratch, db) = shop_with_materials();
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos
            .counts()
            .open(OUTLET, "cnt_1", "Store", day(), at(1), None)?;
        repos.counts().record_line(
            OUTLET,
            "cnt_1",
            &MaterialId::new("mat_rice"),
            &written(grams(5_000), grams(5_000), "g"),
            at(2),
        )?;
        Ok(())
    })
    .expect("count something");

    // One open count per location, and the refusal names the first.
    let second = db.transaction(|tx| {
        Repos::new(tx)
            .counts()
            .open(OUTLET, "cnt_2", "Store", day(), at(3), None)
    });
    let message = second.expect_err("a second count is refused").to_string();
    assert!(message.contains("already open"), "{message}");

    db.transaction(|tx| {
        Repos::new(tx)
            .counts()
            .approve(OUTLET, "cnt_1", at(4), day(), None)
    })
    .expect("approve");

    // Sealed: no more lines, no second approval, no giving up on it.
    let edited = db.transaction(|tx| {
        Repos::new(tx).counts().record_line(
            OUTLET,
            "cnt_1",
            &MaterialId::new("mat_rice"),
            &written(grams(1), grams(1), "g"),
            at(5),
        )
    });
    assert!(edited.is_err(), "an approved count cannot be added to");
    assert!(
        db.transaction(|tx| Repos::new(tx)
            .counts()
            .approve(OUTLET, "cnt_1", at(6), day(), None))
            .is_err(),
        "an approved count cannot be approved twice"
    );
    assert!(
        db.transaction(|tx| Repos::new(tx)
            .counts()
            .abandon(OUTLET, "cnt_1", "no", at(7)))
            .is_err(),
        "an approved count cannot be given up on"
    );

    // And once the first is finished, a second may start.
    db.transaction(|tx| {
        Repos::new(tx)
            .counts()
            .open(OUTLET, "cnt_2", "Store", day(), at(8), None)
    })
    .expect("a second count is allowed now");
}

#[test]
fn t9_a_count_is_never_deleted_and_giving_up_needs_a_reason() {
    let (_scratch, db) = shop_with_materials();
    db.transaction(|tx| {
        Repos::new(tx)
            .counts()
            .open(OUTLET, "cnt_1", "Store", day(), at(1), None)
    })
    .expect("open");

    assert!(
        db.transaction(|tx| Repos::new(tx)
            .counts()
            .abandon(OUTLET, "cnt_1", "   ", at(2)))
            .is_err(),
        "a blank reason is refused"
    );

    db.transaction(|tx| {
        Repos::new(tx)
            .counts()
            .abandon(OUTLET, "cnt_1", "Ran out of time", at(3))
    })
    .expect("give up");

    db.read_transaction(|tx| {
        let count = Repos::new(tx)
            .counts()
            .count(OUTLET, "cnt_1")?
            .expect("still on file");
        assert_eq!(count.state, mb_db::repo::counts::CountState::Abandoned);
        assert_eq!(count.ended_reason.as_deref(), Some("Ran out of time"));
        Ok(())
    })
    .expect("read it back");
}

// Both tax worlds.

#[test]
fn t11_the_same_invoice_costs_more_for_a_shop_that_cannot_claim_the_tax() {
    let (_scratch, db) = shop_with_materials();
    let mat = [(MaterialId::new("mat_paneer"), "g".to_owned())];

    let taxed = |creditable: bool| Invoice {
        lines: vec![Entry {
            tax_rate_bp: 500,
            ..line(1_000, 0, Money::from_paise(40), base())
        }],
        invoice_discount: Money::ZERO,
        charges: Money::ZERO,
        tax_is_creditable: creditable,
        rounding: RoundingMode::None,
    };

    let claiming = deliver(&db, "pur_1", &mat, &taxed(true), day(), at(1));
    let scheme = deliver(&db, "pur_2", &mat, &taxed(false), day(), at(2));

    // Both hand over the same money.
    assert_eq!(claiming.total, scheme.total);
    // Only one of them gets any of it back.
    assert_eq!(claiming.tax_creditable, Money::from_paise(2_000));
    assert_eq!(scheme.tax_creditable, Money::ZERO);
    // And the food costs exactly the tax more for the shop that does not.
    assert_eq!(
        scheme.lines[0]
            .landed_value
            .sub(claiming.lines[0].landed_value)
            .expect("subtracts"),
        Money::from_paise(2_000)
    );
    assert!(scheme.lines[0].landed_unit_cost > claiming.lines[0].landed_unit_cost);
}

// A purchase order, and a purchase with no order at all.

#[test]
fn t13_an_order_can_be_received_short_and_a_purchase_needs_no_order() {
    let (_scratch, db) = shop_with_materials();
    db.transaction(|tx| {
        Repos::new(tx).buying().save_order(
            OUTLET,
            &buying::PurchaseOrder {
                id: "po_1".to_owned(),
                supplier_id: "sup_metro".to_owned(),
                supplier_name: String::new(),
                number: "PO-1".to_owned(),
                state: buying::OrderState::Sent,
                expected_day: Some(day()),
                note: None,
                created_at: at(1),
                created_by: None,
                sent_at: Some(at(1)),
                closed_at: None,
                lines: vec![buying::OrderLine {
                    seq: 1,
                    material_id: MaterialId::new("mat_rice"),
                    material_name: String::new(),
                    typed_qty: Qty::from_whole(10).expect("10"),
                    typed_unit: "bag".to_owned(),
                    base_qty: grams(250_000),
                    rate: rupees(1_000),
                }],
            },
        )
    })
    .expect("raise an order");

    // Eight arrive, at a higher rate.
    let mut received = db
        .transaction(|tx| {
            let repos = Repos::new(tx);
            let supplier = repos
                .buying()
                .supplier(OUTLET, "sup_metro")?
                .expect("on file");
            let mut draft = buying::draft(
                "pur_1",
                &supplier,
                day(),
                at(2),
                &[(MaterialId::new("mat_rice"), "bag".to_owned())],
                &invoice(vec![line(8, 0, rupees(1_150), bag())]),
            )?;
            draft.po_id = Some("po_1".to_owned());
            repos.buying().record_purchase(OUTLET, &draft)
        })
        .expect("receive it");
    assert_eq!(received.lines[0].base_qty, grams(200_000));
    assert_eq!(balance(&db, "mat_rice"), grams(200_000));
    received.po_id = None;

    // And a purchase with no order touches nothing PO-shaped.
    deliver(
        &db,
        "pur_2",
        &[(MaterialId::new("mat_paneer"), "g".to_owned())],
        &invoice(vec![line(1_000, 0, Money::from_paise(40), base())]),
        day(),
        at(3),
    );
    db.read_transaction(|tx| {
        let plain = Repos::new(tx)
            .buying()
            .purchase(OUTLET, "pur_2")?
            .expect("on file");
        assert_eq!(plain.po_id, None);
        Ok(())
    })
    .expect("read it back");
}

// The profit statement, and the double count named out loud.

/// The worked month, in ordinary numbers.
#[test]
fn t14_the_profit_statement_reconciles_and_names_the_double_count() {
    let scratch = Scratch::new("profit");
    let db = scratch.open();
    common::shop::build(&db);

    // A dish made of 200 g of rice, and rice bought at ₹40 a kilo.
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let mut rice = rice();
        // The category matches an expense category, which is how the double count is found
        // later.
        rice.category = "Groceries".to_owned();
        repos.stock().save_material(OUTLET, &rice, at(0))?;
        repos.buying().save_supplier(OUTLET, &metro(), at(0))?;
        repos.stock().save_recipe(
            OUTLET,
            &mb_core::recipe::Recipe::for_one(
                mb_core::recipe::RecipeOwner::Item(mb_core::ItemId::new("itm_dosa")),
                vec![mb_core::recipe::RecipeLine::new(
                    MaterialId::new("mat_rice"),
                    grams(200),
                    grams(200),
                    "g",
                )],
            ),
            at(0),
        )
    })
    .expect("seed the kitchen");

    // Four bags in: 100 kg at ₹40 a kilo = ₹4,000 of rice on the shelf.
    deliver(
        &db,
        "pur_1",
        &[(MaterialId::new("mat_rice"), "bag".to_owned())],
        &invoice(vec![line(4, 0, rupees(1_000), bag())]),
        day(),
        at(1),
    );

    // Ten dosas at ₹100 each, GST 5% exclusive: ₹1,000 of goods, ₹50 of tax.
    settle_one(&db, "ord_1", 10);

    // ₹500 of running costs, of which ₹300 is typed into a category the shop also buys through
    // Purchases.
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for (id, category, amount) in [
            ("exp_1", Some("exc_groceries"), rupees(300)),
            ("exp_2", Some("exc_rent"), rupees(200)),
        ] {
            repos.money().save_expense(
                OUTLET,
                &mb_db::repo::money::Expense {
                    id: id.to_owned(),
                    category_id: category.map(str::to_owned),
                    description: "A cost".to_owned(),
                    amount,
                    mode: "cash".to_owned(),
                    paid_to: None,
                    reference: None,
                    gst_rate_bp: None,
                    gst_amount: None,
                    paid_at: at(9),
                    paid_by: None,
                    business_day: day(),
                    note: None,
                },
            )?;
        }
        Ok(())
    })
    .expect("record the running costs");

    let profit = db
        .read_transaction(|tx| {
            Repos::new(tx)
                .reports()
                .profit(OUTLET, mb_db::repo::reports::Period::one_day(day()))
        })
        .expect("the profit statement");

    // Sales ₹1,050 billed, ₹50 of it tax, so ₹1,000 kept.
    assert_eq!(profit.gross_sales, rupees(1_050));
    assert_eq!(profit.tax, rupees(50));
    assert_eq!(profit.net_sales, rupees(1_000));
    // The rice bought for ₹4,000 is not a cost.
    assert_eq!(profit.food_used, rupees(80));
    assert_eq!(profit.cost_of_food, rupees(80));
    assert_eq!(profit.gross_margin, rupees(920));
    assert_eq!(profit.margin_bp(), Some(9_200), "92% gross margin");
    // Running costs are the expenses, and the purchase is not among them.
    assert_eq!(profit.running_costs, rupees(500));
    assert_eq!(profit.left, rupees(420));
    // And the double count is named, not assumed away.
    assert_eq!(profit.double_counted, rupees(300));
}

/// One bill for `qty` dosas at ₹100, GST 5% exclusive.
fn settle_one(db: &Db, id: &str, qty: i64) -> mb_core::SettledOrder {
    use mb_core::{
        BillInput, Cart, ItemId, ItemSnapshot, OrderType, Payment, PaymentMode, PlaceOfSupply,
        Settlement, TaxRate, TaxSpec, compute_bill,
    };

    let snapshot = ItemSnapshot {
        item_id: ItemId::new("itm_dosa"),
        name: "Masala Dosa".to_owned(),
        unit_price: Money::from_paise(10_000),
        tax: TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
        hsn: None,
        category_id: None,
        station: None,
        course: None,
        prep_minutes: None,
    };
    let mut cart = Cart::new();
    cart.add(
        snapshot,
        Qty::from_whole(qty).expect("in range"),
        None,
        vec![],
    )
    .expect("adds");
    let bill = compute_bill(
        BillInput::new(&cart, Registration::Regular)
            .with_order_type(OrderType::Parcel)
            .with_place_of_supply(PlaceOfSupply::Intra)
            .with_rounding(RoundingMode::NearestRupee),
    )
    .expect("a bill");
    let mut settlement = Settlement::new();
    settlement
        .add(Payment::new(PaymentMode::Cash, bill.grand_total).expect("a payment"))
        .expect("paid");

    let mut draft = mb_core::DraftOrder::new(
        mb_core::OrderId::new(id),
        day(),
        at(6),
        OrderType::Parcel,
        StaffId::new("staff_1"),
    );
    draft.core.cart = cart;
    let till = mb_db::Till::new(OUTLET, common::TERMINAL);
    let open = mb_db::open_draft(db, till, draft).expect("opened");
    mb_db::settle(
        db,
        till,
        open,
        bill,
        settlement,
        at(7),
        StaffId::new("staff_1"),
    )
    .expect("settled")
}

// Every new report runs — the test that catches a typo in a hand-written query.

#[test]
fn every_buying_report_runs_on_a_real_shop() {
    // SQL is not checked by the compiler, so a column that does not exist compiles and fails on
    // a customer's machine.
    let (_scratch, db) = shop_with_materials();
    deliver(
        &db,
        "pur_1",
        &[(MaterialId::new("mat_rice"), "bag".to_owned())],
        &invoice(vec![Entry {
            tax_rate_bp: 500,
            ..line(2, 0, rupees(1_000), bag())
        }]),
        day(),
        at(1),
    );
    deliver(
        &db,
        "pur_2",
        &[(MaterialId::new("mat_rice"), "bag".to_owned())],
        &invoice(vec![Entry {
            tax_rate_bp: 500,
            ..line(2, 0, rupees(1_400), bag())
        }]),
        day(),
        at(2),
    );
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos
            .counts()
            .open(OUTLET, "cnt_1", "Store", day(), at(3), None)?;
        repos.counts().record_line(
            OUTLET,
            "cnt_1",
            &MaterialId::new("mat_rice"),
            &written(grams(99_000), grams(99_000), "g"),
            at(4),
        )?;
        repos
            .counts()
            .approve(OUTLET, "cnt_1", at(5), day(), None)?;
        Ok(())
    })
    .expect("count and approve");

    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        let (from, to) = (day(), day());
        assert_eq!(repos.buying().by_supplier(OUTLET, from, to)?.len(), 1);
        assert_eq!(repos.buying().by_material(OUTLET, from, to)?.len(), 1);

        let trend = repos.buying().price_trend(OUTLET, from, to)?;
        assert_eq!(trend.len(), 1);
        // ₹40 then ₹56 a kilo — plus the 5% this shop cannot claim back, so the trend reads ₹42
        // and ₹58.80. The average is ₹50.40 and the last delivery is 16.6% above it.
        assert_eq!(trend[0].average, UnitCost::from_paise_per_thousand(5_040));
        assert_eq!(trend[0].latest, UnitCost::from_paise_per_thousand(5_880));
        assert_eq!(trend[0].change_bp(), Some(1_666));

        assert_eq!(repos.buying().input_credit(OUTLET, from, to)?.len(), 1);
        // The shop is not claiming, so nothing is creditable.
        assert_eq!(
            repos.buying().creditable_total(OUTLET, from, to)?,
            Money::ZERO
        );
        assert_eq!(repos.buying().outstanding(OUTLET, to)?.len(), 1);
        assert_eq!(repos.counts().variance_history(OUTLET, from, to)?.len(), 1);
        repos
            .reports()
            .profit(OUTLET, mb_db::repo::reports::Period::new(from, to))?;
        Ok(())
    })
    .expect("every report runs");
}

// The photograph, and a backup that is a folder.

#[test]
fn t15_a_backup_carries_the_photographs_and_a_verify_catches_a_damaged_one() {
    let scratch = Scratch::new("attach");
    let db = scratch.open();
    let dir = mb_db::backup::attachments_dir(&scratch.db_path());
    std::fs::create_dir_all(&dir).expect("the attachments folder");
    std::fs::write(dir.join("abc123.jpg"), vec![7_u8; 4_000]).expect("a photograph");

    let backup_path = scratch.db_path().with_file_name("backup.db");
    let backup = mb_db::backup::take(&db, &backup_path, "test").expect("take a backup");
    assert_eq!(backup.manifest.attachments.len(), 1);
    assert!(
        mb_db::backup::backup_attachments_dir(&backup_path)
            .join("abc123.jpg")
            .exists(),
        "the photograph is in the backup"
    );
    assert!(mb_db::backup::verify(&backup_path).expect("verify").is_ok());

    // Damage it. A picture of a ₹40,000 invoice that silently rots is exactly what a verify
    // exists to find.
    std::fs::write(
        mb_db::backup::backup_attachments_dir(&backup_path).join("abc123.jpg"),
        vec![9_u8; 4_000],
    )
    .expect("damage it");
    let report = mb_db::backup::verify(&backup_path).expect("verify");
    assert!(!report.is_ok());
    assert_eq!(report.bad_attachments, vec!["abc123.jpg".to_owned()]);
    assert!(
        report.summary().contains("photograph"),
        "{}",
        report.summary()
    );
}

#[test]
fn t15_a_backup_from_before_this_session_still_restores() {
    let scratch = Scratch::new("oldbackup");
    let db = scratch.open();
    let backup_path = scratch.db_path().with_file_name("old.db");
    let backup = mb_db::backup::take(&db, &backup_path, "test").expect("take a backup");
    assert!(
        backup.manifest.attachments.is_empty(),
        "no photographs, no lines"
    );
    assert!(
        !mb_db::backup::backup_attachments_dir(&backup_path).exists(),
        "and no empty folder either"
    );
    drop(db);

    let restored_to = scratch.db_path().with_file_name("restored.db");
    let report = mb_db::backup::restore(&backup_path, &restored_to).expect("restore");
    assert!(!report.rolled_back);
}
