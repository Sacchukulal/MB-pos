//! Voids, cancels, reprints, refunds — and the sum that has to tie.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Scratch;
use common::shop::{self, OUTLET, TERMINAL};
use mb_core::{
    AnyOrder, BillInput, BusinessDay, Cart, ItemSnapshot, Money, OrderId, OrderType, Payment,
    PaymentMode, PlaceOfSupply, Qty, Registration, RoundingMode, Settlement, StaffId, TaxRate,
    TaxSpec, Timestamp, compute_bill,
};
use mb_db::repo::corrections::{Reason, Refund};
use mb_db::{Db, Repos};

fn at(n: i64) -> Timestamp {
    Timestamp::from_millis(1_770_000_000_000 + n * 1_000)
}

fn day() -> BusinessDay {
    BusinessDay::from_days_since_epoch(20_700)
}

fn tea() -> ItemSnapshot {
    ItemSnapshot {
        item_id: mb_core::ItemId::new("itm_dosa"),
        name: "Masala Dosa".to_owned(),
        unit_price: Money::from_paise(10_000),
        tax: TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
        hsn: None,
        category_id: None,
        station: None,
        course: None,
        prep_minutes: None,
    }
}

/// Settle one bill of `qty` dosa, and return the order.
fn settle_one(db: &Db, id: &str, qty: i64) -> mb_core::SettledOrder {
    let mut cart = Cart::new();
    cart.add(tea(), Qty::from_whole(qty).expect("in range"), None, vec![])
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
        OrderId::new(id),
        day(),
        at(1),
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
        at(2),
        StaffId::new("staff_1"),
    )
    .expect("settled")
}

/// Gross − voids = net.
#[test]
fn gross_minus_voids_equals_net_across_a_day() {
    let scratch = Scratch::new("recon");
    let db = scratch.open();
    shop::build(&db);

    // Five bills, of which two are voided.
    let mut settled = Vec::new();
    for n in 1..=5 {
        settled.push(settle_one(&db, &format!("ord_recon_{n}"), n));
    }

    let mut voided_total = Money::ZERO;
    for order in settled.iter().take(2) {
        voided_total = voided_total.add(order.bill.grand_total).expect("in range");
        let voided = order
            .clone()
            .void("Billed twice", StaffId::new("staff_1"), at(9))
            .expect("voided");
        db.transaction(|tx| {
            Repos::new(tx)
                .orders()
                .save(OUTLET, TERMINAL, &AnyOrder::Voided(voided.clone()))
        })
        .expect("saved");
    }

    let totals = db
        .transaction(|tx| Repos::new(tx).corrections().day_totals(OUTLET, day()))
        .expect("totals");

    println!(
        "\n  gross {}   voids {}   net {}   ({} bills, {} voided)",
        totals.gross.to_plain_string(),
        totals.voids.to_plain_string(),
        totals.net.to_plain_string(),
        totals.bills,
        totals.voided_bills
    );

    assert_eq!(totals.bills, 5, "a voided bill vanished from the count");
    assert_eq!(totals.voided_bills, 2);
    assert_eq!(totals.voids, voided_total);
    assert_eq!(
        totals.net,
        totals.gross.sub(totals.voids).expect("in range"),
        "gross - voids != net"
    );
    assert!(totals.net.is_positive());
}

/// A voided bill keeps its number, and keeps its money on the row.
#[test]
fn a_voided_bill_keeps_its_number_and_its_amounts() {
    let scratch = Scratch::new("void_keeps");
    let db = scratch.open();
    shop::build(&db);

    let settled = settle_one(&db, "ord_keeps", 2);
    let number = settled.bill_number.formatted.clone();
    let total = settled.bill.grand_total;

    let voided = settled
        .void("Wrong items billed", StaffId::new("staff_1"), at(9))
        .expect("voided");
    db.transaction(|tx| {
        Repos::new(tx)
            .orders()
            .save(OUTLET, TERMINAL, &AnyOrder::Voided(voided))
    })
    .expect("saved");

    let read = db
        .transaction(|tx| Repos::new(tx).orders().find(&OrderId::new("ord_keeps")))
        .expect("read")
        .expect("the order");

    match read {
        AnyOrder::Voided(o) => {
            assert_eq!(o.bill_number.formatted, number, "the number changed");
            assert_eq!(o.bill.grand_total, total, "a void edited history");
            assert_eq!(o.reason, "Wrong items billed");
            assert_eq!(o.voided_by, StaffId::new("staff_1"));
            // The original settlement is still there — who took it and when.
            assert_eq!(o.settled_by, StaffId::new("staff_1"));
        }
        other => panic!("expected a voided order, got {other:?}"),
    }
}

/// The reprint count is the number on the paper, and the original is copy 1.
#[test]
fn the_reprint_number_is_the_number_on_the_paper() {
    let scratch = Scratch::new("reprints");
    let db = scratch.open();
    shop::build(&db);
    settle_one(&db, "ord_reprint", 1);
    let order = OrderId::new("ord_reprint");

    let copies: Vec<u32> = db
        .transaction(|tx| {
            let repos = Repos::new(tx);
            let mut out = Vec::new();
            for _ in 0..3 {
                out.push(repos.corrections().record_reprint(
                    OUTLET,
                    &order,
                    Some(&StaffId::new("staff_1")),
                    Some("Customer asked for a copy"),
                    at(20),
                    day(),
                )?);
            }
            Ok(out)
        })
        .expect("reprints");

    // The original was copy 1, so the first reprint is copy 2.
    assert_eq!(copies, vec![2, 3, 4]);

    let rows = db
        .transaction(|tx| Repos::new(tx).corrections().reprints_for(&order))
        .expect("rows");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].copy, 2);
    assert_eq!(rows[2].copy, 4);
    assert_eq!(rows[0].reason.as_deref(), Some("Customer asked for a copy"));
    assert_eq!(rows[0].printed_by, Some(StaffId::new("staff_1")));
}

/// A refund needs a voided bill, and cannot exceed what was taken.
#[test]
fn money_only_goes_back_against_a_voided_bill_and_never_more_than_came_in() {
    let scratch = Scratch::new("refunds");
    let db = scratch.open();
    shop::build(&db);

    let settled = settle_one(&db, "ord_refund", 3);
    let taken = settled.bill.grand_total;
    let order = OrderId::new("ord_refund");

    let refund = |amount: Money, id: &str| Refund {
        id: id.to_owned(),
        order_id: order.clone(),
        amount,
        mode: "cash".to_owned(),
        reason: "Billed twice".to_owned(),
        refunded_at: at(30),
        refunded_by: Some(StaffId::new("staff_1")),
    };

    // Not while it is still a settled bill.
    let refused = db
        .transaction(|tx| {
            Repos::new(tx)
                .corrections()
                .record_refund(OUTLET, &refund(taken, "ref_1"), day())
        })
        .expect_err("money went back on a live bill");
    assert!(refused.to_string().contains("voided"), "{refused}");

    // Void it.
    let voided = settled
        .void("Billed twice", StaffId::new("staff_1"), at(29))
        .expect("voided");
    db.transaction(|tx| {
        Repos::new(tx)
            .orders()
            .save(OUTLET, TERMINAL, &AnyOrder::Voided(voided))
    })
    .expect("saved");

    // Not more than came in.
    let too_much = taken.add(Money::from_paise(100)).expect("in range");
    let refused = db
        .transaction(|tx| {
            Repos::new(tx)
                .corrections()
                .record_refund(OUTLET, &refund(too_much, "ref_2"), day())
        })
        .expect_err("more went back than came in");
    assert!(
        refused.to_string().contains("left to give back"),
        "{refused}"
    );

    // Part of it now.
    let half = Money::from_paise(1_000);
    db.transaction(|tx| {
        Repos::new(tx)
            .corrections()
            .record_refund(OUTLET, &refund(half, "ref_3"), day())
    })
    .expect("half went back");

    // And the rest is what is left — not the whole amount again.
    let refused = db
        .transaction(|tx| {
            Repos::new(tx)
                .corrections()
                .record_refund(OUTLET, &refund(taken, "ref_4"), day())
        })
        .expect_err("the second refund ignored the first");
    assert!(
        refused.to_string().contains("left to give back"),
        "{refused}"
    );

    let so_far = db
        .transaction(|tx| Repos::new(tx).corrections().refunded_so_far(&order))
        .expect("so far");
    assert_eq!(so_far, half);

    // And the day's figures see it, beside the void rather than inside it.
    let totals = db
        .transaction(|tx| Repos::new(tx).corrections().day_totals(OUTLET, day()))
        .expect("totals");
    assert_eq!(totals.refunded, half);
    assert_eq!(
        totals.voids, taken,
        "the void is the whole bill, refund or not"
    );
}

/// The reason list is data, and a shop's edits are its own.
#[test]
fn the_reason_list_is_the_shops_own() {
    let scratch = Scratch::new("reasons");
    let db = scratch.open();
    shop::build(&db);

    let seeded = db
        .transaction(|tx| Repos::new(tx).corrections().reasons(OUTLET, "void"))
        .expect("reasons");
    assert!(seeded.len() >= 4, "a new shop has no reasons to offer");
    assert!(seeded.iter().any(|r| r.text == "Billed twice"));

    // Every kind has something, or the dialog opens empty for that flow.
    for kind in ["void", "cancel", "item_void", "reprint"] {
        let list = db
            .transaction(|tx| Repos::new(tx).corrections().reasons(OUTLET, kind))
            .expect("reasons");
        assert!(!list.is_empty(), "{kind} has no reasons");
    }

    // A shop adds its own and retires one of ours.
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.corrections().save_reason(
            OUTLET,
            &Reason {
                id: "rsn_void_shop".to_owned(),
                kind: "void".to_owned(),
                text: "Owner's decision".to_owned(),
                sort_order: 0,
                is_active: true,
            },
            at(1),
        )?;
        repos.corrections().save_reason(
            OUTLET,
            &Reason {
                id: "rsn_void_test".to_owned(),
                kind: "void".to_owned(),
                text: "Test bill".to_owned(),
                sort_order: 9,
                is_active: false,
            },
            at(1),
        )
    })
    .expect("edited");

    let now = db
        .transaction(|tx| Repos::new(tx).corrections().reasons(OUTLET, "void"))
        .expect("reasons");
    assert!(now.iter().any(|r| r.text == "Owner's decision"));
    assert!(
        !now.iter().any(|r| r.text == "Test bill"),
        "a retired reason is still being offered"
    );
}

/// A cancelled order is a state, not a deletion — and it still has its number.
#[test]
fn a_cancelled_order_keeps_its_number_and_is_counted() {
    let scratch = Scratch::new("cancel");
    let db = scratch.open();
    shop::build(&db);

    let mut cart = Cart::new();
    cart.add(tea(), Qty::ONE, None, vec![]).expect("adds");
    let mut draft = mb_core::DraftOrder::new(
        OrderId::new("ord_walkout"),
        day(),
        at(1),
        OrderType::Parcel,
        StaffId::new("staff_1"),
    );
    draft.core.cart = cart;

    let till = mb_db::Till::new(OUTLET, TERMINAL);
    let open = mb_db::open_draft(&db, till, draft).expect("opened");
    let number = open.bill_number.formatted.clone();

    let cancelled = open
        .cancel("Customer left", StaffId::new("staff_1"), at(5))
        .expect("cancelled");
    db.transaction(|tx| {
        Repos::new(tx)
            .orders()
            .save(OUTLET, TERMINAL, &AnyOrder::Cancelled(cancelled))
    })
    .expect("saved");

    let read = db
        .transaction(|tx| Repos::new(tx).orders().find(&OrderId::new("ord_walkout")))
        .expect("read")
        .expect("the order");
    match read {
        AnyOrder::Cancelled(o) => {
            assert_eq!(o.bill_number.formatted, number);
            assert_eq!(o.reason, "Customer left");
        }
        other => panic!("expected a cancelled order, got {other:?}"),
    }

    // It is not in the open list any more — which is how the table frees.
    let open_now = db
        .transaction(|tx| Repos::new(tx).orders().list_open(OUTLET))
        .expect("open");
    assert!(
        !open_now
            .iter()
            .any(|o| o.core().id == OrderId::new("ord_walkout")),
        "the cancelled order is still holding its table"
    );

    let totals = db
        .transaction(|tx| Repos::new(tx).corrections().day_totals(OUTLET, day()))
        .expect("totals");
    assert_eq!(totals.cancelled_orders, 1);
    // A cancel is not a void: nothing was ever taken, so it is not in gross.
    assert_eq!(totals.gross, Money::ZERO);
    assert_eq!(totals.voids, Money::ZERO);
}

#[test]
fn changing_a_tax_class_moves_the_menu_and_never_a_bill() {
    use mb_core::{TaxClassId, TaxRate};

    let scratch = Scratch::new("taxclass");
    let db = scratch.open();
    shop::build(&db);

    // The fixture's dosa is on "Restaurant food 5%", and there is a settled bill with a dosa on
    // it.
    let before_rate = db
        .transaction(|tx| {
            Ok(Repos::new(tx)
                .menu()
                .find_item(&mb_core::ItemId::new("itm_dosa"))?
                .expect("the dosa")
                .tax
                .rate)
        })
        .expect("read");
    assert_eq!(before_rate, TaxRate::from_percent(5).expect("5%"));

    // What the settled bill says today.
    let billed_rates: Vec<i64> = db
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT DISTINCT rate_bp FROM bill_tax_rows ORDER BY rate_bp")?;
            let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
            Ok(rows.filter_map(Result::ok).collect())
        })
        .expect("bill rates");
    assert!(billed_rates.contains(&500), "{billed_rates:?}");

    // The government moves restaurant food to 18%.
    let repriced = db
        .transaction(|tx| {
            let repos = Repos::new(tx);
            let mut class = repos
                .tax_classes()
                .find(OUTLET, &TaxClassId::new("tax_food_5"))?
                .expect("the class");
            class.tax.rate = TaxRate::from_percent(18).expect("18%");
            class.name = "Restaurant food 18%".to_owned();
            repos.tax_classes().save(OUTLET, &class, at(9))
        })
        .expect("saved");

    assert!(repriced >= 1, "no item followed the class: {repriced}");

    // The MENU moved.
    let after_rate = db
        .transaction(|tx| {
            Ok(Repos::new(tx)
                .menu()
                .find_item(&mb_core::ItemId::new("itm_dosa"))?
                .expect("the dosa")
                .tax
                .rate)
        })
        .expect("read");
    assert_eq!(
        after_rate,
        TaxRate::from_percent(18).expect("18%"),
        "the live menu did not follow"
    );

    // And the bill did not.
    let still: Vec<i64> = db
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT DISTINCT rate_bp FROM bill_tax_rows ORDER BY rate_bp")?;
            let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
            Ok(rows.filter_map(Result::ok).collect())
        })
        .expect("bill rates");
    assert_eq!(
        still, billed_rates,
        "changing a tax class rewrote a bill that had already been printed"
    );

    // An item that points at no class is untouched by anybody's class change.
    let orphan = db
        .transaction(|tx| {
            Ok(Repos::new(tx)
                .menu()
                .find_item(&mb_core::ItemId::new("itm_beer"))?
                .expect("the beer")
                .tax
                .kind)
        })
        .expect("read");
    assert_eq!(
        orphan,
        mb_core::TaxKind::OutsideGst,
        "the beer moved with the food"
    );
}

/// Every class `mb-core` ships is seeded and live, and the liquor one is what lets a bar bill
/// at all.
#[test]
fn the_seeded_classes_match_the_ones_mb_core_ships() {
    let scratch = Scratch::new("taxclass_seed");
    let db = scratch.open();

    let stored = db
        .transaction(|tx| Repos::new(tx).tax_classes().list(OUTLET))
        .expect("classes");

    for expected in &mb_core::starting_classes() {
        let found = stored
            .iter()
            .find(|c| c.id == expected.id)
            .unwrap_or_else(|| panic!("{} is not seeded", expected.name));
        assert_eq!(found.tax, expected.tax, "{}", expected.name);
        assert_eq!(found.name, expected.name);
        assert!(found.is_active, "{} is seeded switched off", expected.name);
    }

    // The retired slab: still there, so nothing points at a missing row, and off, so it is not
    // offered to anybody setting up a menu today.
    let retired = stored
        .iter()
        .find(|c| c.id.as_str() == "tax_packaged_12")
        .expect("the abolished 12% slab is retired, not deleted");
    assert!(
        !retired.is_active,
        "the abolished 12% slab is still on offer"
    );
    assert_eq!(stored.len(), mb_core::starting_classes().len() + 1);

    // The commercial one: outside GST, priced tax-in, with a rate the shop sets.
    let liquor = stored
        .iter()
        .find(|c| c.is_alcohol())
        .expect("a shop must be able to sell liquor");
    assert_eq!(liquor.name, "Liquor — state VAT");
    assert_eq!(liquor.tax.basis, mb_core::PriceBasis::Inclusive);
}

// The per-order-type rate override test is GONE, with the feature.
