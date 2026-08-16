//! **P29's delivery half, driven end to end** — T10 and T11.
//!
//! `mb-db` proves the rows and `mb-core` proves the tax. What is proved here is
//! the EVENING, which is the part that costs a shop money:
//!
//! * a delivery charge taxed at its own rate, not the food's (T10);
//! * cash that leaves on a bike is **out of the drawer's expectation until it
//!   comes back**, and the handback is what brings it back (T10);
//! * a delivery that did not arrive is a state with a reason, and the bill is
//!   still owed by somebody (T11).
//!
//! The middle one is the reason the whole feature exists. A drawer that counts
//! a rider's cash as if it were in the till is short all evening for a reason
//! nobody can name, and the shortfall walks back in at nine o'clock — which
//! looks like a theft that resolved itself.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_auth::{Actor, Permission, PermissionSet, RolePreset};
use mb_core::{Money, StaffId};
use mb_db::{Db, DbConfig};

use crate::delivery::{
    DeliveryEdit, board_on, record_handback_on, save_delivery_on, set_rider_on,
};
use crate::ipc::{StaffEdit, save_staff_member_on};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

// ---------------------------------------------------------------------------
// The fixture
// ---------------------------------------------------------------------------

fn a_shop(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    // One item, at 5%, so the delivery charge's own rate is visibly a
    // different figure from the food's.
    db.transaction(|tx| {
        mb_db::Repos::new(tx).menu().save_item(
            OUTLET,
            &mb_db::repo::menu::MenuItem {
                id: mb_core::ItemId::new("itm_dosa"),
                category_id: None,
                name: "Masala Dosa".to_owned(),
                unit_price: Money::from_paise(12_000),
                tax_rate: mb_core::TaxRate::GST_5,
                tax_treatment: mb_core::TaxTreatment::Exclusive,
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

fn hire(app: &App, id: &str, name: &str) {
    save_staff_member_on(
        app,
        StaffEdit {
            id: id.to_owned(),
            name: name.to_owned(),
            code: None,
            role_id: Some(RolePreset::Cashier.id().to_owned()),
            status: "active".to_owned(),
        },
    )
    .expect("hired");
}

fn signed_in_with(app: &App, id: &str, name: &str, permissions: PermissionSet) {
    app.sessions().begin(
        Actor {
            staff_id: StaffId::new(id),
            name: name.to_owned(),
            role_id: None,
            role_name: None,
            permissions,
            max_discount_bp: None,
            max_discount: None,
        },
        crate::flows::now(),
        false,
    );
}

fn as_owner(app: &App, id: &str, name: &str) {
    hire(app, id, name);
    signed_in_with(app, id, name, PermissionSet::everything());
}

/// One delivery order, billed and settled in cash through the real path —
/// which is what a shop that takes the money at the counter and sends the food
/// out does, and the case the drawer gets wrong.
fn a_delivery_paid_in_cash(app: &App) -> (String, Money, mb_core::Bill) {
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Delivery;
        state.table = None;
        state.table_label = None;
        Ok(())
    })
    .expect("delivery");
    crate::ipc::cart_add_on(app, "itm_dosa".to_owned(), Some("2".to_owned()), None)
        .expect("added");
    let bill = app
        .with_cart(|state| state.bill(&app.shop_config()))
        .expect("bill");
    let total = bill.grand_total;
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

    let board = board_on(app, None).expect("the board");
    let row = board
        .deliveries
        .first()
        .expect("the delivery is on the board");
    (row.order_id.clone(), total, bill)
}

fn cash_expected(app: &App) -> Money {
    let day = crate::flows::today(crate::flows::now());
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).money().cash_position(OUTLET, day))
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a cash position")
    .expected
}

fn carrying(app: &App) -> Money {
    let day = crate::flows::today(crate::flows::now());
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).money().cash_position(OUTLET, day))
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a cash position")
    .with_riders
}

fn edit(order_id: &str, rider: &str, state: &str) -> DeliveryEdit {
    DeliveryEdit {
        order_id: order_id.to_owned(),
        address: String::new(),
        customer_id: String::new(),
        rider_id: rider.to_owned(),
        state: state.to_owned(),
        failure: String::new(),
    }
}

// ---------------------------------------------------------------------------
// T10 — the charge, the rider and the reconciliation
// ---------------------------------------------------------------------------

/// **T10.** A delivery charge is taxed on its OWN rate, and the cash that goes
/// out with a rider reconciles against what they hand back.
#[test]
fn a_delivery_is_taxed_on_its_own_rate_and_the_cod_cash_reconciles_against_the_handback() {
    let scratch = Scratch::new("delivery_t10");
    let app = a_shop(&scratch, "t10");
    as_owner(&app, "staff_boss", "Meena");
    hire(&app, "staff_kumar", "Kumar");

    // A ₹40 delivery charge at 5%, while the food is at whatever the menu says
    // — the point is that the two rates are separate figures on one bill.
    let mut config = app.shop_config();
    config.billing.delivery_charge = Money::from_paise(4_000);
    config.billing.delivery_charge_tax_bp = 500;
    app.publish_shop_config(config);

    let (order_id, total, bill) = a_delivery_paid_in_cash(&app);

    // The charge is on the bill, at its own rate.
    let charge = bill
        .charges
        .iter()
        .find(|c| c.kind == mb_core::ChargeKind::Delivery)
        .expect("a delivery charge on a delivery bill");
    assert_eq!(charge.amount, Money::from_paise(4_000));
    assert_eq!(
        charge.rate.basis_points(),
        500,
        "the delivery charge carries its OWN tax rate, not the food's"
    );

    // Nobody is carrying anything until somebody takes it.
    assert_eq!(carrying(&app), Money::ZERO);
    let before = cash_expected(&app);

    // Kumar is a rider, and the order goes out with him.
    set_rider_on(&app, "staff_kumar".to_owned(), true).expect("a rider");
    save_delivery_on(&app, edit(&order_id, "staff_kumar", "assigned")).expect("assigned");
    let board = save_delivery_on(&app, edit(&order_id, "staff_kumar", "out")).expect("out");

    // **The drawer stops expecting the money the moment the bike leaves.**
    assert_eq!(carrying(&app), total);
    assert_eq!(
        cash_expected(&app),
        before.sub(total).expect("a smaller figure"),
        "cash on a bike is not cash in the drawer"
    );
    let kumar = board
        .riders
        .iter()
        .find(|r| r.id == "staff_kumar")
        .expect("Kumar's evening");
    assert_eq!(kumar.carrying.paise, total.paise());
    assert!(kumar.says.starts_with("Carrying"), "{}", kumar.says);

    // He delivers it, and hands the money over.
    save_delivery_on(&app, edit(&order_id, "staff_kumar", "delivered")).expect("delivered");
    assert_eq!(carrying(&app), total, "still his until he hands it back");

    let board = record_handback_on(
        &app,
        "staff_kumar".to_owned(),
        total.to_plain_string(),
        "end of the round".to_owned(),
    )
    .expect("handed back");

    // **And it reconciles.** Nothing outstanding, and the drawer expects the
    // whole day's cash again.
    assert_eq!(carrying(&app), Money::ZERO);
    assert_eq!(cash_expected(&app), before);
    let kumar = board
        .riders
        .iter()
        .find(|r| r.id == "staff_kumar")
        .expect("Kumar's evening");
    assert_eq!(kumar.carrying.paise, 0);
    assert_eq!(kumar.handed_back.paise, total.paise());
    assert_eq!(kumar.says, "Nothing outstanding");
    assert_eq!(board.says, "Everything is back.");
}

/// A handback that is SHORT says by how much, rather than quietly balancing.
#[test]
fn a_short_handback_leaves_the_difference_visible() {
    let scratch = Scratch::new("delivery_short");
    let app = a_shop(&scratch, "short");
    as_owner(&app, "staff_boss", "Meena");
    hire(&app, "staff_kumar", "Kumar");

    let (order_id, total, _) = a_delivery_paid_in_cash(&app);
    set_rider_on(&app, "staff_kumar".to_owned(), true).expect("a rider");
    save_delivery_on(&app, edit(&order_id, "staff_kumar", "assigned")).expect("assigned");
    save_delivery_on(&app, edit(&order_id, "staff_kumar", "out")).expect("out");
    save_delivery_on(&app, edit(&order_id, "staff_kumar", "delivered")).expect("delivered");

    // A hundred rupees short.
    let short = Money::from_paise(10_000);
    let handed = total.sub(short).expect("less than the total");
    let board =
        record_handback_on(&app, "staff_kumar".to_owned(), handed.to_plain_string(), String::new())
            .expect("handed back");

    let kumar = board
        .riders
        .iter()
        .find(|r| r.id == "staff_kumar")
        .expect("Kumar's evening");
    assert_eq!(
        kumar.carrying.paise,
        short.paise(),
        "the difference is a figure on the screen, not a rounding somewhere"
    );
    assert_eq!(carrying(&app), short);
}

// ---------------------------------------------------------------------------
// T11 — a failure is a state
// ---------------------------------------------------------------------------

/// **T11.** A delivery that did not arrive is a STATE with a reason (D47), and
/// the money is still accounted for: nobody paid, so nobody is carrying it,
/// and the bill is still owed.
#[test]
fn a_failed_delivery_is_a_state_with_a_reason_and_the_money_is_still_accounted_for() {
    let scratch = Scratch::new("delivery_t11");
    let app = a_shop(&scratch, "t11");
    as_owner(&app, "staff_boss", "Meena");
    hire(&app, "staff_kumar", "Kumar");

    // Billed but NOT settled — the customer pays at the door, and this one
    // never gets there.
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Delivery;
        Ok(())
    })
    .expect("delivery");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    crate::flows::park_open_order(&app).expect("held on the counter");

    let order_id = board_on(&app, None)
        .expect("the board")
        .deliveries
        .first()
        .expect("a delivery")
        .order_id
        .clone();

    set_rider_on(&app, "staff_kumar".to_owned(), true).expect("a rider");
    save_delivery_on(&app, edit(&order_id, "staff_kumar", "assigned")).expect("assigned");
    save_delivery_on(&app, edit(&order_id, "staff_kumar", "out")).expect("out");

    // **A failure with no reason is refused.** Not a warning, not a default —
    // "did not arrive" with no words is the row nobody can settle an argument
    // with three weeks later.
    let mut fail = edit(&order_id, "staff_kumar", "failed");
    let refused = save_delivery_on(&app, fail.clone());
    assert!(refused.is_err(), "a failure has to say why");

    fail.failure = "Nobody was home, phone switched off".to_owned();
    let board = save_delivery_on(&app, fail).expect("failed, with a reason");

    let row = board.deliveries.first().expect("still on the board");
    assert_eq!(row.state, "failed");
    assert_eq!(row.state_says, "Did not arrive");
    assert_eq!(row.failure, "Nobody was home, phone switched off");
    assert!(!row.paid, "nobody paid for it");
    assert_eq!(
        row.collect.paise,
        row.total.paise,
        "the food is unpaid and the board still says so"
    );
    // **And the figure is a real one.** An unsettled order has no `bills` row,
    // so the board has to price it from the cart — the first version of this
    // screen showed a rider 0.00 and "nothing to collect" on an order nobody
    // had paid for. Found by looking at it (D55).
    assert!(
        row.total.paise > 0,
        "an unsettled delivery must still show what it is worth"
    );
    assert!(row.money_says.starts_with("Collect"), "{}", row.money_says);

    // Nobody collected anything, so nobody is carrying anything — and the
    // drawer is untouched by the whole episode.
    assert_eq!(carrying(&app), Money::ZERO);
    let kumar = board.riders.iter().find(|r| r.id == "staff_kumar");
    if let Some(kumar) = kumar {
        assert_eq!(kumar.failed, 1);
        assert_eq!(kumar.carrying.paise, 0);
    }

    // **And it cannot be quietly turned into a success.** A failed delivery is
    // corrected by a void with a reason, not by pretending the food arrived.
    let pretend = save_delivery_on(&app, edit(&order_id, "staff_kumar", "delivered"));
    assert!(
        pretend.is_err(),
        "a failed delivery must not walk back to delivered"
    );
}

// ---------------------------------------------------------------------------
// The permission
// ---------------------------------------------------------------------------

/// Anybody signed in may LOOK at the board. Only a dispatcher may move
/// anything on it, or take money off a rider.
#[test]
fn reading_the_board_is_open_and_moving_a_delivery_is_not() {
    let scratch = Scratch::new("delivery_perm");
    let app = a_shop(&scratch, "perm");
    as_owner(&app, "staff_boss", "Meena");
    hire(&app, "staff_kumar", "Kumar");
    let (order_id, _, _) = a_delivery_paid_in_cash(&app);
    set_rider_on(&app, "staff_kumar".to_owned(), true).expect("a rider");

    // A waiter with nothing but the till.
    signed_in_with(
        &app,
        "staff_kumar",
        "Kumar",
        [Permission::BillCreate].into_iter().collect(),
    );
    let board = board_on(&app, None).expect("a waiter may look");
    assert!(!board.may_dispatch, "and the screen knows they may not act");

    let refused = save_delivery_on(&app, edit(&order_id, "staff_kumar", "assigned"));
    assert!(refused.is_err());
    let refused = record_handback_on(&app, "staff_kumar".to_owned(), "1".to_owned(), String::new());
    assert!(refused.is_err());
}
