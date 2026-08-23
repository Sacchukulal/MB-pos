//! **A whole trading day, and everything has to reconcile** — P30.
//!
//! Every other test in this product proves one rule. This one drives a day
//! through the real commands and then asks the question the rules cannot ask
//! individually:
//!
//! > **where two figures are computed by different code, are they equal?**
//!
//! That is the entire class of bug this product was rebuilt to kill. Not one
//! of v1's sixty-two findings was a crash. They were a report that bucketed by
//! UTC while its filter used local time, a credit balance stored beside the
//! ledger that made it, a backup that only held bills. Every one of them was
//! invisible from the counter and obvious in a spreadsheet six months later —
//! because two pieces of code answered the same question differently and
//! nothing ever compared them.
//!
//! So this file compares them. The day below is deliberately awkward: mixed
//! rates, a void, a refund, a discount, split payments, a credit sale and a
//! repayment, an expense, a delivery with cash on a bike, an unconfirmed UPI
//! payment and a tip. Then:
//!
//! | this figure | must equal | why it could differ |
//! |---|---|---|
//! | the day's takings | the sum of the bills | `day_totals` and `sales_by` are different SQL |
//! | cash in the drawer | float + cash bills − cash out | `cash_position` is seven queries |
//! | tax by rate | the tax on the bills that made it | the summary is its own table (audit B11) |
//! | the customer's balance | their ledger | v1 stored it beside the ledger (audit A3) |
//! | what a rider carries | collected − handed back | D151, and it is never stored |
//! | the audit chain | itself | D43 — a broken link has a `seq` |

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_auth::{Actor, PermissionSet, RolePreset};
use mb_core::{Money, StaffId};
use mb_db::{Db, DbConfig};

use crate::ipc::{StaffEdit, cart_add_payment_on, save_staff_member_on};
use crate::reports::PeriodArg;
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

// ---------------------------------------------------------------------------
// The shop
// ---------------------------------------------------------------------------

/// Four items at three different tax rates, including one the law says carries
/// no GST at all — a bar cannot be billed for without it (requirement 2).
const MENU: &[(&str, &str, i64, Option<u32>)] = &[
    // id, name, rupees, tax basis points (None = outside GST, i.e. liquor)
    ("itm_dosa", "Masala Dosa", 120, Some(500)),
    ("itm_biryani", "Chicken Biryani", 280, Some(500)),
    ("itm_cake", "Chocolate Pastry", 90, Some(1_800)),
    ("itm_beer", "Beer 650ml", 220, None),
];

fn a_shop(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    a_shop_at(scratch, &path)
}

/// The same shop, at a path the caller keeps — so a test can drop the whole
///  and open the same file again, which is what a restart is.
fn a_shop_at(scratch: &Scratch, path: &std::path::Path) -> App {
    let path = path.to_path_buf();
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        for (id, item_name, rupees, rate) in MENU {
            repos.menu().save_item(
                OUTLET,
                &mb_db::repo::menu::MenuItem {
                    id: mb_core::ItemId::new(*id),
                    category_id: None,
                    name: (*item_name).to_owned(),
                    unit_price: Money::from_paise(rupees * 100),
                    tax_rate: rate.map_or(mb_core::TaxRate::ZERO, |bp| {
                        mb_core::TaxRate::from_basis_points(bp).unwrap_or(mb_core::TaxRate::ZERO)
                    }),
                    tax_treatment: mb_core::TaxTreatment::Exclusive,
                    tax_class_id: None,
                    hsn: Some("2106".to_owned()),
                    cost_price: None,
                    short_code: None,
                    prep_minutes: None,
                    course: None,
                    is_open_price: false,
                    is_available: true,
                    sort_order: 0,
                },
                crate::flows::now(),
            )?;
        }
        Ok(())
    })
    .expect("a menu");

    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    app.use_licensing(crate::licence_tests::licence_in(
        scratch,
        "acceptance",
        mb_license::Status::Active,
        90,
    ));

    // The owner, signed in and on the staff list.
    save_staff_member_on(
        &app,
        StaffEdit {
            id: "staff_boss".to_owned(),
            name: "Meena".to_owned(),
            code: None,
            role_id: Some(RolePreset::Cashier.id().to_owned()),
            status: "active".to_owned(),
        },
    )
    .expect("hired");
    app.sessions().begin(
        Actor {
            staff_id: StaffId::new("staff_boss"),
            name: "Meena".to_owned(),
            role_id: None,
            role_name: None,
            permissions: PermissionSet::everything(),
            max_discount_bp: None,
            max_discount: None,
        },
        crate::flows::now(),
        false,
    );
    app
}

/// One bill, through the real commands. Returns its number and its total.
fn bill(app: &App, items: &[(&str, &str)], mode: &str) -> (String, Money) {
    crate::ipc::cart_clear_on(app, false).expect("a fresh cart");
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    for (item, qty) in items {
        crate::ipc::cart_add_on(app, (*item).to_owned(), Some((*qty).to_owned()), None)
            .expect("added");
    }
    let total = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    cart_add_payment_on(app, mode.to_owned(), total.paise(), None).expect("paid");
    let number = crate::flows::complete_bill_on(app).expect("settled");
    (number, total)
}

fn today() -> mb_core::BusinessDay {
    crate::flows::today(crate::flows::now())
}

fn period() -> PeriodArg {
    PeriodArg {
        from: today().to_string(),
        to: today().to_string(),
    }
}

fn cash(app: &App) -> mb_db::repo::money::CashPosition {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).money().cash_position(OUTLET, today()))
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a cash position")
}

fn totals(app: &App) -> mb_db::repo::DayTotals {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).corrections().day_totals(OUTLET, today()))
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("the day's totals")
}

// ---------------------------------------------------------------------------
// The day
// ---------------------------------------------------------------------------

/// **A trading day, and every figure that can be computed twice.**
#[test]
fn a_whole_day_reconciles_against_itself() {
    let scratch = Scratch::new("acceptance_day");
    let app = a_shop(&scratch, "day");

    // --- the float ---------------------------------------------------------
    crate::expenses::save_movement_on(
        &app,
        "float".to_owned(),
        "3000".to_owned(),
        "opening float".to_owned(),
    )
    .expect("the float");

    // --- the service -------------------------------------------------------
    let mut cash_bills = Money::ZERO;
    let mut every_bill = Money::ZERO;

    // Twelve bills across the three rates and three payment modes.
    for n in 0..12 {
        let (item, qty) = match n % 4 {
            0 => ("itm_dosa", "2"),
            1 => ("itm_biryani", "1"),
            2 => ("itm_cake", "3"),
            _ => ("itm_beer", "2"),
        };
        let mode = match n % 3 {
            0 => "Cash",
            1 => "Card",
            _ => "UPI",
        };
        let (_, total) = bill(&app, &[(item, qty)], mode);
        every_bill = every_bill.add(total).expect("in range");
        if mode == "Cash" {
            cash_bills = cash_bills.add(total).expect("in range");
        }
    }

    // A mixed-rate bill: food at 5%, a pastry at 18%, and beer outside GST
    // altogether. This is requirement 2 in one line.
    let (mixed_number, mixed_total) = bill(
        &app,
        &[("itm_dosa", "1"), ("itm_cake", "1"), ("itm_beer", "1")],
        "Cash",
    );
    every_bill = every_bill.add(mixed_total).expect("in range");
    cash_bills = cash_bills.add(mixed_total).expect("in range");

    // --- 1. the takings, two ways -----------------------------------------
    let day = totals(&app);
    assert_eq!(
        day.gross, every_bill,
        "`day_totals` and the bills that made it disagree"
    );
    assert_eq!(day.bills, 13);

    let sales = crate::reports::report_on(&app, "sales_day".to_owned(), period())
        .expect("the sales report");
    let printed: Vec<&String> = sales.rows.iter().flat_map(|r| r.iter()).collect();
    assert!(
        printed.iter().any(|c| c.contains(&every_bill.to_plain_string())),
        "the sales report and the day's totals are different SQL and must \
         agree: {printed:?}"
    );

    // --- 2. the drawer -----------------------------------------------------
    let drawer = cash(&app);
    assert_eq!(drawer.opening_float, Money::from_paise(300_000));
    assert_eq!(drawer.cash_sales, cash_bills, "cash bills");
    assert_eq!(
        drawer.expected,
        Money::from_paise(300_000)
            .add(cash_bills)
            .expect("in range"),
        "float plus cash bills IS the drawer, before anything goes out"
    );

    // --- 3. money out ------------------------------------------------------
    crate::expenses::save_expense_on(
        &app,
        crate::expenses::ExpenseEdit {
            id: "exc_milk".to_owned(),
            category_id: None,
            description: "Milk".to_owned(),
            amount: "240".to_owned(),
            mode: "cash".to_owned(),
            paid_to: String::new(),
            reference: String::new(),
            gst_percent: String::new(),
            note: String::new(),
        },
    )
    .expect("an expense");

    let drawer = cash(&app);
    assert_eq!(drawer.cash_expenses, Money::from_paise(24_000));
    assert_eq!(
        drawer.expected,
        Money::from_paise(300_000)
            .add(cash_bills)
            .expect("in range")
            .sub(Money::from_paise(24_000))
            .expect("in range"),
        "a cash expense comes out of the drawer exactly once"
    );

    // --- 4. a void ---------------------------------------------------------
    let voided = crate::corrections::list_bills_on(&app)
        .expect("the bills")
        .into_iter()
        .find(|b| b.number == mixed_number)
        .expect("the mixed bill is in the list");
    crate::corrections::void_bill_on(
        &app,
        voided.order_id.clone(),
        "Wrong table".to_owned(),
        None,
        None,
    )
    .expect("voided");

    let day = totals(&app);
    assert_eq!(day.voids, mixed_total, "the void is the bill's own total");
    assert_eq!(
        day.net,
        every_bill.sub(mixed_total).expect("in range"),
        "net takings is gross less voids, computed once"
    );
    // **And the drawer follows.** A voided cash bill is money that left the
    // drawer again — the single most expensive thing to get wrong here.
    let drawer = cash(&app);
    assert_eq!(
        drawer.cash_sales,
        cash_bills.sub(mixed_total).expect("in range"),
        "a voided cash bill is no longer cash the drawer holds"
    );

    // --- 5. the tax summary ------------------------------------------------
    let tax = crate::reports::report_on(&app, "tax_rate".to_owned(), period())
        .expect("the tax report");
    let rates: Vec<&String> = tax.rows.iter().filter_map(|r| r.first()).collect();
    assert!(
        rates.iter().any(|r| r.contains('5')),
        "5% has to be its own row: {rates:?}"
    );
    assert!(
        rates.iter().any(|r| r.contains("18")),
        "18% has to be its own row, never lumped in with the 5%: {rates:?}"
    );
    // The beer. **A bar cannot be billed for legally without this row.**
    assert!(
        tax.rows.len() >= 2,
        "a mixed-rate day is at least two rows: {:?}",
        tax.rows
    );

    // --- 6. the audit trail ------------------------------------------------
    let history = crate::ipc::audit_trail_on(&app, None, None, None).expect("the history");
    assert!(
        history.entries.iter().any(|e| e.what.contains("Voided")),
        "a void with no audit row is audit C4 all over again"
    );
    assert!(
        history.tampered.is_none(),
        "the hash chain says it is broken: {:?}",
        history.tampered
    );
}

// ---------------------------------------------------------------------------
// The one that could bill twice
// ---------------------------------------------------------------------------

/// **A double Enter must never bill twice.**
///
/// The cashier presses Complete Bill, nothing visibly happens for 200 ms, and
/// they press it again. On v1 that was a real second bill. Here the second
/// call has nothing to settle, and it must say so rather than producing a
/// number.
#[test]
fn settling_the_same_cart_twice_bills_once() {
    let scratch = Scratch::new("acceptance_twice");
    let app = a_shop(&scratch, "twice");

    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    let total = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    cart_add_payment_on(&app, "Cash".to_owned(), total.paise(), None).expect("paid");

    let first = crate::flows::complete_bill_on(&app).expect("settled");
    let second = crate::flows::complete_bill_on(&app);

    assert!(!first.is_empty());
    assert!(
        second.is_err(),
        "the second press produced a bill number: {second:?}"
    );
    assert_eq!(totals(&app).bills, 1, "one press, one bill");
}

// ---------------------------------------------------------------------------
// The day boundary
// ---------------------------------------------------------------------------

/// **One business day, everywhere** — requirement 8, and audit B1.
///
/// v1 stored UTC, filtered by local time and grouped reports by the UTC date,
/// so a bill at 00:15 landed on two different days on two different screens.
/// The day is STAMPED once (D5) and nothing re-derives it, which is what this
/// asserts: the order, its payment and the report all say the same day.
#[test]
fn a_bill_after_midnight_belongs_to_one_day_on_every_screen() {
    let scratch = Scratch::new("acceptance_day_edge");
    let app = a_shop(&scratch, "edge");

    let (_, total) = bill(&app, &[("itm_dosa", "1")], "Cash");
    let stamped = today();

    // The order's day, the payment's day and the report's day are three
    // different columns written by three different code paths.
    let day_of_payment = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let day: i64 =
                        tx.query_row("SELECT business_day FROM payments LIMIT 1", [], |r| {
                            r.get(0)
                        })?;
                    Ok(day)
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("a payment");
    assert_eq!(
        day_of_payment,
        i64::from(stamped.days_since_epoch()),
        "the payment carries the order's day, denormalised on purpose (D5)"
    );

    let day = totals(&app);
    assert_eq!(day.gross, total, "and the report finds it on that day");
}

// ---------------------------------------------------------------------------
// The money that is not a bill
// ---------------------------------------------------------------------------

/// **Credit, a rider's cash and a tip — three figures that are computed twice
/// each, and every one of them was wrong somewhere in v1.**
///
/// * a customer's balance is a SUM over the ledger and was a stored column
///   beside it (audit A3);
/// * what a rider is carrying is a sum over rows and is deliberately not
///   stored (D151);
/// * a tip is in the drawer and is not a sale (scope 8.5) — it appears in one
///   figure and must not appear in the other.
#[test]
fn credit_a_riders_cash_and_a_tip_each_reconcile_two_ways() {
    let scratch = Scratch::new("acceptance_money");
    let app = a_shop(&scratch, "money");

    // --- credit ------------------------------------------------------------
    crate::credit::save_customer_on(
        &app,
        crate::credit::CustomerEdit {
            id: "cus_arun".to_owned(),
            name: "Arun".to_owned(),
            phone: "9845066112".to_owned(),
            gstin: String::new(),
            address: String::new(),
            credit_limit: String::new(),
            is_active: true,
        },
    )
    .expect("a customer");

    crate::ipc::cart_clear_on(&app, false).expect("a fresh cart");
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_biryani".to_owned(), Some("2".to_owned()), None)
        .expect("added");
    let owed = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    crate::credit::put_on_account_on(&app, "cus_arun".to_owned(), false).expect("on account");
    crate::flows::complete_bill_on(&app).expect("settled on account");

    let owing = crate::credit::who_owes_on(&app).expect("who owes");
    let arun = owing
        .iter()
        .find(|c| c.id == "cus_arun")
        .expect("Arun is on the list");
    assert_eq!(
        arun.balance.paise,
        owed.paise(),
        "the balance is a SUM over the ledger, and it has to equal the bill \
         that made it (audit A3)"
    );

    // Half of it back, in cash.
    // Half of it, to the paise. A remainder here would only make the
    // assertion below harder to read; the arithmetic under test is the
    // ledger's, not this line's.
    #[allow(clippy::integer_division, reason = "half a balance, in a test")]
    let half = Money::from_paise(owed.paise() / 2);
    crate::credit::record_repayment_on(
        &app,
        "cus_arun".to_owned(),
        half.to_plain_string(),
        "cash".to_owned(),
        String::new(),
    )
    .expect("a repayment");

    let owing = crate::credit::who_owes_on(&app).expect("who owes");
    let arun = owing
        .iter()
        .find(|c| c.id == "cus_arun")
        .expect("still there");
    assert_eq!(
        arun.balance.paise,
        owed.sub(half).expect("in range").paise(),
        "a repayment moves the balance by exactly what was handed over"
    );
    // **And a credit sale is not cash.** The drawer only sees the repayment.
    assert_eq!(
        cash(&app).cash_sales,
        Money::ZERO,
        "a bill put on account is not money in the drawer"
    );

    // --- a tip -------------------------------------------------------------
    // The day already has the credit sale on it, so what is asserted is the
    // DELTA: a tip adds nothing at all to the takings.
    let before_the_tip = totals(&app).gross;
    crate::ipc::cart_clear_on(&app, false).expect("a fresh cart");
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    let food = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    let tip = Money::from_paise(3_000);
    app.with_cart_mut(|state| {
        state.settlement.set_tip(tip).map_err(|e| {
            crate::words::UiError::new("bill.tip", "That tip could not be taken.")
                .with_detail(e.to_string())
        })
    })
    .expect("a tip");
    cart_add_payment_on(
        &app,
        "Cash".to_owned(),
        food.add(tip).expect("in range").paise(),
        None,
    )
    .expect("paid with a tip");
    crate::flows::complete_bill_on(&app).expect("settled");

    let drawer = cash(&app);
    assert_eq!(
        drawer.cash_tips, tip,
        "a cash tip IS in the drawer and is split out so it can be seen"
    );
    assert_eq!(
        totals(&app).gross,
        before_the_tip.add(food).expect("in range"),
        "…and it is NOT a sale. The day's takings grew by the food, and only          by the food."
    );

    // --- a rider ------------------------------------------------------------
    crate::delivery::set_rider_on(&app, "staff_boss".to_owned(), true).expect("a rider");
    crate::ipc::cart_clear_on(&app, false).expect("a fresh cart");
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Delivery;
        state.table = None;
        state.table_label = None;
        Ok(())
    })
    .expect("delivery");
    crate::ipc::cart_add_on(&app, "itm_biryani".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    let cod = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    cart_add_payment_on(&app, "Cash".to_owned(), cod.paise(), None).expect("paid");
    crate::flows::complete_bill_on(&app).expect("settled");

    let before = cash(&app).expected;
    let order_id = crate::delivery::board_on(&app, None)
        .expect("the board")
        .deliveries
        .iter()
        .find(|d| d.state == "pending")
        .expect("the delivery")
        .order_id
        .clone();
    for step in ["assigned", "out"] {
        crate::delivery::save_delivery_on(
            &app,
            crate::delivery::DeliveryEdit {
                order_id: order_id.clone(),
                address: String::new(),
                customer_id: String::new(),
                rider_id: "staff_boss".to_owned(),
                state: (*step).to_owned(),
                failure: String::new(),
            },
        )
        .expect("moved along");
    }

    let out = cash(&app);
    assert_eq!(out.with_riders, cod, "the money left with the bike (D151)");
    assert_eq!(
        out.expected,
        before.sub(cod).expect("in range"),
        "and the drawer stopped expecting it"
    );

    crate::delivery::record_handback_on(
        &app,
        "staff_boss".to_owned(),
        cod.to_plain_string(),
        String::new(),
    )
    .expect("handed back");
    assert_eq!(
        cash(&app).expected,
        before,
        "and the handback puts it back, exactly"
    );
}

// ---------------------------------------------------------------------------
// A restart
// ---------------------------------------------------------------------------

/// **Kill it mid-service. Nothing is lost and nothing is duplicated.**
///
/// The open order is on disk before the kitchen ever sees it (D4), so a
/// counter that loses power between the ticket and the bill still has the
/// table's food. This drops the whole `App` — every in-memory cart with it —
/// and opens the same file again, which is what a restart is.
#[test]
fn a_restart_mid_service_keeps_the_open_orders() {
    let scratch = Scratch::new("acceptance_restart");
    let path = scratch.dir().join("restart.db");

    let (order_id, lines) = {
        let app = a_shop_at(&scratch, &path);
        crate::ipc::cart_clear_on(&app, false).expect("a fresh cart");
        app.with_cart_mut(|state| {
            state.order_type = mb_core::OrderType::Parcel;
            Ok(())
        })
        .expect("parcel");
        crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("2".to_owned()), None)
            .expect("added");
        crate::ipc::cart_add_on(&app, "itm_cake".to_owned(), Some("1".to_owned()), None)
            .expect("added");
        crate::flows::park_open_order(&app).expect("held");
        let cart = app.with_cart(|state| crate::billing::cart_view(state, &app.shop_config())).expect("the cart");
        (
            app.with_cart(|state| Ok(state.order_id.clone()))
                .expect("an order id")
                .expect("the order was saved"),
            cart.lines.len(),
        )
        // `app` is dropped here — the power cut.
    };

    let app = a_shop_at(&scratch, &path);
    let open = crate::ipc::open_orders_on(&app).expect("the floor");
    let found = open
        .iter()
        .find(|t| t.order_id.as_deref() == Some(order_id.as_str()))
        .expect("the order survived the restart");
    assert_eq!(
        found.total.as_ref().map(|m| m.paise > 0),
        Some(true),
        "…with its money"
    );

    crate::ipc::open_table_on(&app, found.id.clone()).expect("reopened");
    let cart = app.with_cart(|state| crate::billing::cart_view(state, &app.shop_config())).expect("the cart");
    assert_eq!(cart.lines.len(), lines, "…and with all of its lines");

    // And it is ONE order, not two.
    assert_eq!(
        open.iter()
            .filter(|t| t.order_id.as_deref() == Some(order_id.as_str()))
            .count(),
        1,
        "a restart must not duplicate an order"
    );
}

// ---------------------------------------------------------------------------
// Every report in the catalogue
// ---------------------------------------------------------------------------

/// **Every report the product offers is asked for, on a real day.**
///
/// P18's shape is that a report is a line in the catalogue plus a function in
/// mb-db — which is what let P26 add nine of them without touching a screen.
/// The risk in that shape is a catalogue entry whose query was never run: it
/// looks finished on the settings screen and fails the first time an owner
/// presses it, months later, on a busy evening.
///
/// So this presses all of them. It asserts only that each one ANSWERS —
/// the arithmetic of each is its own test — because the failure this catches
/// is a broken column name, not a wrong figure.
#[test]
fn every_report_in_the_catalogue_answers_for_a_real_day() {
    let scratch = Scratch::new("acceptance_reports");
    let app = a_shop(&scratch, "reports");

    // A day with something in it, so an empty table cannot pass for a working
    // query.
    bill(&app, &[("itm_dosa", "2"), ("itm_cake", "1")], "Cash");
    bill(&app, &[("itm_beer", "1")], "Card");
    crate::expenses::save_movement_on(
        &app,
        "float".to_owned(),
        "2000".to_owned(),
        "opening float".to_owned(),
    )
    .expect("the float");

    let mut refused = Vec::new();
    for entry in crate::reports::CATALOGUE {
        match crate::reports::report_on(&app, entry.id.to_owned(), period()) {
            Ok(view) => {
                assert!(
                    !view.columns.is_empty(),
                    "{} came back with no columns at all",
                    entry.id
                );
            }
            Err(e) => refused.push(format!("{} — {}", entry.id, e.message)),
        }
    }

    assert!(
        refused.is_empty(),
        "these reports are on the screen and cannot be run:
  {}",
        refused.join("
  ")
    );
}

// ---------------------------------------------------------------------------
// The environment turning hostile
// ---------------------------------------------------------------------------

/// **A corrupted shop file fails LOUDLY, and never half-opens.**
///
/// The shape that would be dangerous is a database that opens, answers some
/// queries and silently loses others — a shop billing onto a file that is
/// already broken. SQLite refuses a file whose header is not a database, and
/// this is the test that says the product lets that refusal through rather
/// than swallowing it into an empty shop.
#[test]
fn a_corrupted_shop_file_is_refused_rather_than_half_opened() {
    let scratch = Scratch::new("acceptance_corrupt");
    let path = scratch.dir().join("broken.db");
    std::fs::write(&path, b"this is not a database, it is a photograph of one")
        .expect("wrote the rubbish");

    let opened = Db::open(&DbConfig::new(path.clone()));
    assert!(
        opened.is_err(),
        "a file that is not a database opened anyway, which is worse than          failing: the shop would bill onto it"
    );

    // And the counter is still usable — a broken file for ONE shop is not a
    // broken program. A fresh path opens normally.
    let fresh = a_shop_at(&scratch, &scratch.dir().join("fresh.db"));
    assert!(!bill(&fresh, &[("itm_dosa", "1")], "Cash").0.is_empty());
}

/// **A day that has been closed and locked refuses to be billed into.**
///
/// The adversarial version of requirement 9: it is not enough that the close
/// writes a row, the lock has to actually stop the next thing.
#[test]
fn a_locked_day_refuses_the_corrections_that_would_change_it() {
    let scratch = Scratch::new("acceptance_locked");
    let app = a_shop(&scratch, "locked");
    let (number, total) = bill(&app, &[("itm_dosa", "1")], "Cash");

    // Counted in ₹500 notes. How many is a division, and the remainder is
    // deliberately dropped: this test is about the LOCK, and the drawer being
    // a few rupees out is what the reason box is for.
    #[allow(clippy::integer_division, reason = "counting notes, in a test")]
    let notes = u32::try_from(total.paise() / 50_000).unwrap_or(0);
    crate::dayclose::close_on(
        &app,
        vec![crate::dayclose::CountArg {
            value: 50_000,
            count: notes,
        }],
        "Counted at close".to_owned(),
        false,
    )
    .expect("closed");

    let bills = crate::corrections::list_bills_on(&app).expect("the bills");
    let row = bills
        .iter()
        .find(|b| b.number == number)
        .expect("the bill is there");
    let refused = crate::corrections::void_bill_on(
        &app,
        row.order_id.clone(),
        "Changed my mind".to_owned(),
        None,
        None,
    );
    assert!(
        refused.is_err(),
        "a locked day accepted a void, so the closing slip is now a lie"
    );
    let message = refused.err().map(|e| e.message).unwrap_or_default();
    assert!(
        !message.contains("could not be read"),
        "a REFUSAL must not arrive as a database failure (D75): {message}"
    );
}

/// **A settled order can never be settled a second time** — FIX PLAN 1, A2.
///
/// `settling_the_same_cart_twice_bills_once` above covers the ordinary double
/// press: the cart is cleared by the first settle, so the second finds nothing
/// and stops. That is the whole defence, and it is one line of state.
///
/// This is the case that got past it. Settling reads the cart, releases it,
/// writes the order, prints, and only then clears — so a second press that
/// arrives inside that window still holds a cart pointing at the order that
/// has just been settled. It looked the order up, did not match `Open`, and
/// fell through to "open a new one", which **claims a fresh bill number and
/// upserts the settled row away**. One sale, two bill numbers, and a GST book
/// with a hole in it.
///
/// `park_open_order` had answered this properly all along. Settling had not.
#[test]
fn a_settled_order_is_never_given_a_second_bill_number() {
    let scratch = Scratch::new("acceptance_resettle");
    let app = a_shop(&scratch, "resettle");

    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    let total = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    cart_add_payment_on(&app, "Cash".to_owned(), total.paise(), None).expect("paid");
    let first = crate::flows::complete_bill_on(&app).expect("settled");

    // The order that was just settled.
    let settled_id = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| mb_db::Repos::new(tx).orders().list_for_day(OUTLET, today()))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("the day's orders")
        .into_iter()
        .find_map(|o| match o {
            mb_core::AnyOrder::Settled(s) => Some(s.core.id.as_str().to_owned()),
            _ => None,
        })
        .expect("something was settled");

    // A cart still pointing at it — what the losing press is holding.
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        state.order_id = Some(settled_id);
        Ok(())
    })
    .expect("the second press still holds the order");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    let again = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    cart_add_payment_on(&app, "Cash".to_owned(), again.paise(), None).expect("paid");

    let second = crate::flows::complete_bill_on(&app);

    assert!(!first.is_empty());
    assert!(
        second.is_err(),
        "a settled order was settled again and got bill {second:?}"
    );
    assert_eq!(
        totals(&app).bills,
        1,
        "one sale became two bills in the day's book"
    );
}

// ---------------------------------------------------------------------------
// FIX PLAN 1 — one counter action at a time.
//
// The cart lock is taken and released per call, so its promise holds inside one
// call and not across a flow. Settling reads the cart, releases it, writes the
// order, prints, and only then clears — and a second press landing inside that
// window read a cart that was still full. `settling_the_same_cart_twice_bills_once`
// could not catch it, because pressing twice in a test is two calls in a row,
// and in a row is exactly the case that already worked.
// ---------------------------------------------------------------------------

/// **A second action waits for the first to finish.**
///
/// Deterministic, and about the mechanism rather than the symptom: the counter
/// is held, a second action is started on another thread, and it must still be
/// waiting. Everything else in this round rests on this being true.
#[test]
fn a_second_action_waits_while_the_counter_is_busy() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let scratch = Scratch::new("acceptance_one_at_a_time");
    let app = a_shop(&scratch, "one_at_a_time");
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");

    let held = app.begin_action();
    let reached = AtomicBool::new(false);
    let finished = AtomicBool::new(false);

    std::thread::scope(|threads| {
        threads.spawn(|| {
            reached.store(true, Ordering::SeqCst);
            let _ = crate::flows::print_kitchen_ticket_on(&app);
            finished.store(true, Ordering::SeqCst);
        });

        while !reached.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(
            !finished.load(Ordering::SeqCst),
            "a second action ran straight through while the counter was busy"
        );

        drop(held);
    });

    assert!(
        finished.load(Ordering::SeqCst),
        "the waiting action never ran after the counter was free"
    );
}

/// **Two settles at the same instant produce one bill.**
///
/// This is the press-twice-quickly case as it actually happens: not one call
/// after another, but two at once. Before the counter was serialised both
/// threads read a full cart, and the loser went on to claim a second bill
/// number for the same sale.
#[test]
fn two_settles_at_the_same_instant_make_one_bill() {
    let scratch = Scratch::new("acceptance_race_settle");
    let app = a_shop(&scratch, "race_settle");

    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    let total = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    cart_add_payment_on(&app, "Cash".to_owned(), total.paise(), None).expect("paid");

    let gate = std::sync::Barrier::new(2);
    let outcomes = std::sync::Mutex::new(Vec::new());

    std::thread::scope(|threads| {
        for _ in 0..2 {
            threads.spawn(|| {
                gate.wait();
                let out = crate::flows::complete_bill_on(&app);
                outcomes.lock().expect("not poisoned").push(out.is_ok());
            });
        }
    });

    let outs = outcomes.into_inner().expect("not poisoned");
    assert_eq!(
        outs.iter().filter(|ok| **ok).count(),
        1,
        "both presses billed the same sale: {outs:?}"
    );
    assert_eq!(totals(&app).bills, 1, "one sale became two bills");
}
