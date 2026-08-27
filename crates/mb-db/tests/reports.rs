//! The reports, against a generated year.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    reason = "tests: expect is the assertion; the fixture splits round numbers"
)]

mod common;

use common::Scratch;
use common::shop::{self, OUTLET, TERMINAL};
use mb_core::{
    BillInput, BusinessDay, Cart, DayRule, ItemSnapshot, Money, OrderId, OrderType, Payment,
    PaymentMode, PlaceOfSupply, Qty, Registration, RoundingMode, Settlement, StaffId, TaxRate,
    TaxSpec, Timestamp, UtcOffset, compute_bill,
};
use mb_db::repo::reports::{Period, SalesBy};
use mb_db::{Db, Repos};

fn day(n: i32) -> BusinessDay {
    BusinessDay::from_days_since_epoch(20_700 + n)
}

fn dosa() -> ItemSnapshot {
    ItemSnapshot {
        item_id: mb_core::ItemId::new("itm_dosa"),
        name: "Masala Dosa".to_owned(),
        unit_price: Money::from_paise(10_000),
        tax: TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
        hsn: Some("2106".to_owned()),
        category_id: None,
        station: None,
        course: None,
        prep_minutes: None,
    }
}

fn water() -> ItemSnapshot {
    ItemSnapshot {
        item_id: mb_core::ItemId::new("itm_water"),
        name: "Water".to_owned(),
        unit_price: Money::from_paise(2_000),
        tax: TaxSpec::liquor(TaxRate::ZERO),
        hsn: Some("2201".to_owned()),
        category_id: None,
        station: None,
        course: None,
        prep_minutes: None,
    }
}

/// One settled bill, on a given business day, at a given instant.
fn settle_on(
    db: &Db,
    id: &str,
    on: BusinessDay,
    at: Timestamp,
    dosas: i64,
    beers: i64,
) -> mb_core::SettledOrder {
    let mut cart = Cart::new();
    if dosas > 0 {
        cart.add(dosa(), Qty::from_whole(dosas).expect("qty"), None, vec![])
            .expect("adds");
    }
    if beers > 0 {
        cart.add(water(), Qty::from_whole(beers).expect("qty"), None, vec![])
            .expect("adds");
    }

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
        OrderId::new(id),
        on,
        at,
        OrderType::Parcel,
        StaffId::new("staff_1"),
    );
    draft.core.cart = cart;

    let till = mb_db::Till::new(OUTLET, TERMINAL);
    let open = mb_db::open_draft(db, till, draft).expect("opened");
    mb_db::settle(
        db,
        till,
        open,
        bill,
        settlement,
        at,
        StaffId::new("staff_1"),
    )
    .expect("settled")
}

#[test]
fn a_bill_after_midnight_appears_on_exactly_one_day_in_every_report() {
    let scratch = Scratch::new("reports-boundary");
    let db = scratch.open();
    shop::build(&db);

    let after_midnight = Timestamp::from_millis(1_785_696_900_000);
    let belongs_to = BusinessDay::of(after_midnight, DayRule::DEFAULT, UtcOffset::INDIA);
    let calendar_date = BusinessDay::of(
        after_midnight,
        DayRule::new(0).expect("midnight"),
        UtcOffset::INDIA,
    );
    assert_ne!(
        belongs_to, calendar_date,
        "the fixture must straddle the day rule or this test proves nothing"
    );

    settle_on(&db, "ord_late", belongs_to, after_midnight, 1, 0);

    db.transaction(|tx| {
        let reports = Repos::new(tx).reports();

        // On the day it belongs to.
        let mine = reports.sales_by(OUTLET, Period::one_day(belongs_to), SalesBy::Day)?;
        assert_eq!(mine.len(), 1, "not on its own business day");
        assert_eq!(mine[0].bills, 1);

        // And on NO other day, including the calendar date its clock shows.
        let other = reports.sales_by(OUTLET, Period::one_day(calendar_date), SalesBy::Day)?;
        assert!(
            other.is_empty(),
            "the same bill appeared on the calendar date too — that is B1"
        );

        // Every other grouping agrees, because they all read the same column.
        for by in [
            SalesBy::Hour,
            SalesBy::OrderType,
            SalesBy::PaymentMode,
            SalesBy::Cashier,
            SalesBy::Item,
            SalesBy::Category,
        ] {
            let here = reports.sales_by(OUTLET, Period::one_day(belongs_to), by)?;
            let there = reports.sales_by(OUTLET, Period::one_day(calendar_date), by)?;
            assert!(!here.is_empty(), "{by:?} lost the bill on its own day");
            assert!(
                there.is_empty(),
                "{by:?} also showed it on the calendar date"
            );
        }
        Ok(())
    })
    .expect("the reports read");
}

/// The totals tie.
#[test]
fn every_grouping_of_a_period_sums_to_the_same_gross() {
    let scratch = Scratch::new("reports-tie");
    let db = scratch.open();
    shop::build(&db);

    let period = Period::new(day(0), day(6));
    let mut expected = Money::ZERO;
    for n in 0..7_i32 {
        for k in 1..=3_i64 {
            let order = settle_on(
                &db,
                &format!("ord_tie_{n}_{k}"),
                day(n),
                Timestamp::from_millis(
                    1_785_000_000_000 + i64::from(n) * 86_400_000 + k * 3_600_000,
                ),
                k,
                i64::from(n % 2),
            );
            expected = expected.add(order.bill.grand_total).expect("in range");
        }
    }

    db.transaction(|tx| {
        let reports = Repos::new(tx).reports();
        // Every grouping that reports the BILL's total.
        for by in [
            SalesBy::Day,
            SalesBy::Hour,
            SalesBy::OrderType,
            SalesBy::Cashier,
            SalesBy::Section,
        ] {
            let rows = reports.sales_by(OUTLET, period, by)?;
            let total = rows
                .iter()
                .try_fold(Money::ZERO, |sum, row| sum.add(row.gross))
                .expect("in range");
            assert_eq!(total, expected, "{by:?} does not tie");
            let bills: i64 = rows.iter().map(|r| r.bills).sum();
            assert_eq!(bills, 21, "{by:?} counted the wrong number of bills");
        }
        Ok(())
    })
    .expect("the reports read");
}

#[test]
fn the_rate_wise_tax_report_equals_what_the_bills_printed() {
    let scratch = Scratch::new("reports-tax");
    let db = scratch.open();
    shop::build(&db);

    let mut printed_cgst = Money::ZERO;
    let mut printed_sgst = Money::ZERO;
    let mut printed_taxable = Money::ZERO;
    for n in 1..=4_i64 {
        let order = settle_on(
            &db,
            &format!("ord_tax_{n}"),
            day(0),
            Timestamp::from_millis(1_785_000_000_000 + n * 60_000),
            n,
            1,
        );
        printed_cgst = printed_cgst
            .add(order.bill.total_gst.central)
            .expect("in range");
        printed_sgst = printed_sgst
            .add(order.bill.total_gst.state)
            .expect("in range");
        for row in order.bill.summary.rows() {
            printed_taxable = printed_taxable.add(row.taxable).expect("in range");
        }
    }

    db.transaction(|tx| {
        let rates = Repos::new(tx)
            .reports()
            .tax_by_rate(OUTLET, Period::one_day(day(0)))?;

        let cgst = rates
            .iter()
            .try_fold(Money::ZERO, |sum, r| sum.add(r.cgst))
            .expect("in range");
        let sgst = rates
            .iter()
            .try_fold(Money::ZERO, |sum, r| sum.add(r.sgst))
            .expect("in range");
        assert_eq!(cgst, printed_cgst, "CGST does not match the printed bills");
        assert_eq!(sgst, printed_sgst, "SGST does not match the printed bills");

        // The non-GST line is reported and is NOT inside a GST total.
        let non_gst = rates.iter().find(|r| r.tax_kind == "outside_gst");
        assert!(
            non_gst.is_some(),
            "the non-GST line is missing from the tax report"
        );
        // And VAT never lands in a GST bucket.
        assert!(
            rates
                .iter()
                .all(|r| r.tax_kind == "outside_gst" || r.vat.is_zero()),
            "VAT leaked into a GST bucket"
        );
        assert!(
            non_gst.is_some_and(|r| r.cgst.is_zero() && r.sgst.is_zero()),
            "a non-GST line was given GST"
        );
        Ok(())
    })
    .expect("the reports read");
}

/// The HSN summary's taxable value agrees with the rate-wise one — they are the same rows
/// grouped two ways, and a GSTR-1 has both on it.
#[test]
fn the_hsn_summary_agrees_with_the_rate_wise_one() {
    let scratch = Scratch::new("reports-hsn");
    let db = scratch.open();
    shop::build(&db);

    for n in 1..=3_i64 {
        settle_on(
            &db,
            &format!("ord_hsn_{n}"),
            day(0),
            Timestamp::from_millis(1_785_000_000_000 + n * 60_000),
            n,
            1,
        );
    }

    db.transaction(|tx| {
        let reports = Repos::new(tx).reports();
        let period = Period::one_day(day(0));
        let by_rate = reports
            .tax_by_rate(OUTLET, period)?
            .iter()
            .try_fold(Money::ZERO, |sum, r| sum.add(r.taxable))
            .expect("in range");
        let by_hsn = reports
            .tax_by_hsn(OUTLET, period)?
            .iter()
            .try_fold(Money::ZERO, |sum, r| sum.add(r.taxable))
            .expect("in range");
        assert_eq!(by_rate, by_hsn, "the two tax reports disagree");
        Ok(())
    })
    .expect("the reports read");
}

/// The comparison period, across a month end and a leap year.
#[test]
fn the_previous_period_is_the_same_length_ending_the_day_before() {
    // One day: yesterday.
    let single = Period::one_day(BusinessDay::from_ymd(2026, 3, 1));
    assert_eq!(single.days(), 1);
    assert_eq!(single.previous().from, BusinessDay::from_ymd(2026, 2, 28));
    assert_eq!(single.previous().to, BusinessDay::from_ymd(2026, 2, 28));

    // A leap year: 2028-03-01's previous day is the 29th of February.
    let leap = Period::one_day(BusinessDay::from_ymd(2028, 3, 1));
    assert_eq!(leap.previous().to, BusinessDay::from_ymd(2028, 2, 29));

    // A week: the seven days before it, not "last calendar week".
    let week = Period::new(
        BusinessDay::from_ymd(2026, 3, 2),
        BusinessDay::from_ymd(2026, 3, 8),
    );
    assert_eq!(week.days(), 7);
    assert_eq!(week.previous().days(), 7);
    assert_eq!(week.previous().to, BusinessDay::from_ymd(2026, 3, 1));
    assert_eq!(week.previous().from, BusinessDay::from_ymd(2026, 2, 23));

    // A whole month, ending on a month end.
    let month = Period::new(
        BusinessDay::from_ymd(2026, 3, 1),
        BusinessDay::from_ymd(2026, 3, 31),
    );
    assert_eq!(month.days(), 31);
    assert_eq!(month.previous().to, BusinessDay::from_ymd(2026, 2, 28));
    assert_eq!(month.previous().days(), 31);
}

/// Menu engineering says what it does not know.
#[test]
fn menu_engineering_says_which_items_have_no_cost_price() {
    let scratch = Scratch::new("reports-margin");
    let db = scratch.open();
    shop::build(&db);

    settle_on(
        &db,
        "ord_margin",
        day(0),
        Timestamp::from_millis(1_785_000_000_000),
        2,
        1,
    );

    db.transaction(|tx| {
        let rows = Repos::new(tx)
            .reports()
            .menu_engineering(OUTLET, Period::one_day(day(0)))?;
        assert!(!rows.is_empty(), "nothing was sold");
        // The fixture's water has no cost price.
        let water = rows.iter().find(|r| r.name.starts_with("Water"));
        assert!(water.is_some(), "the water is missing");
        assert!(
            water.is_some_and(|r| r.cost.is_none()),
            "an uncosted item was reported as pure margin"
        );
        // And the dosa, which IS costed, comes back with its cost.
        let dosa = rows.iter().find(|r| r.name.starts_with("Masala"));
        assert!(
            dosa.is_some_and(|r| r.cost.is_some()),
            "a costed item lost its cost"
        );
        Ok(())
    })
    .expect("the reports read");
}

/// A voided bill is a deduction, not a disappearance.
#[test]
fn a_void_reaches_the_control_report_with_its_reason_and_its_person() {
    let scratch = Scratch::new("reports-control");
    let db = scratch.open();
    shop::build(&db);

    let order = settle_on(
        &db,
        "ord_control",
        day(0),
        Timestamp::from_millis(1_785_000_000_000),
        2,
        0,
    );
    let voided = order
        .void(
            "Billed twice",
            StaffId::new("staff_1"),
            Timestamp::from_millis(1_785_000_060_000),
        )
        .expect("voided");
    db.transaction(|tx| {
        Repos::new(tx)
            .orders()
            .save(OUTLET, TERMINAL, &mb_core::AnyOrder::Voided(voided))
    })
    .expect("saved");

    db.transaction(|tx| {
        let rows = Repos::new(tx)
            .reports()
            .control_log(OUTLET, Period::one_day(day(0)))?;
        let void = rows.iter().find(|r| r.kind == "void");
        assert!(void.is_some(), "the void is not in the control report");
        assert_eq!(void.map(|r| r.reason.as_str()), Some("Billed twice"));
        assert!(
            void.is_some_and(|r| !r.who.is_empty()),
            "a correction nobody can attribute is indistinguishable from theft"
        );
        Ok(())
    })
    .expect("the reports read");
}
