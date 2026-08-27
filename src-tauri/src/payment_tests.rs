#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use std::sync::Arc;

use mb_auth::{Actor, PermissionSet, RolePreset};
use mb_core::provider::{Answer, Scripted};
use mb_core::{Money, StaffId};
use mb_db::{Db, DbConfig};

use crate::ipc::{StaffEdit, cart_add_payment_on, save_staff_member_on};
use crate::payments::{confirm_payment_on, payments_on};
use crate::reports::{PeriodArg, report_on};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

// The fixture.

fn a_shop(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    db.transaction(|tx| {
        mb_db::Repos::new(tx).menu().save_item(
            OUTLET,
            &mb_db::repo::menu::MenuItem {
                id: mb_core::ItemId::new("itm_dosa"),
                category_id: None,
                name: "Masala Dosa".to_owned(),
                unit_price: Money::from_paise(12_000),
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
    // Reports are behind the licence gate.
    app.use_licensing(crate::licence_tests::licence_in(
        scratch,
        "payments-licence",
        mb_license::Status::Active,
        90,
    ));
    app
}

fn as_owner(app: &App, id: &str, name: &str) {
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
    app.sessions().begin(
        Actor {
            staff_id: StaffId::new(id),
            name: name.to_owned(),
            role_id: None,
            role_name: None,
            permissions: PermissionSet::everything(),
            max_discount_bp: None,
            max_discount: None,
        },
        crate::flows::now(),
        false,
    );
}

/// A cart with one dosa on it, ready to be paid for.
fn one_dosa(app: &App) -> Money {
    app.with_cart_mut(|state| {
        state.set_order_type(mb_core::OrderType::Parcel);
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(app, "itm_dosa".to_owned(), Some("1".to_owned()), None).expect("added");
    app.with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill")
}

fn today() -> String {
    crate::flows::today(crate::flows::now()).to_string()
}

fn period() -> PeriodArg {
    PeriodArg {
        from: today(),
        to: today(),
    }
}

// The unconfirmed list.

/// A UPI payment taken by hand keeps its reference, and is visibly unconfirmed until somebody
/// says the money arrived.
#[test]
fn a_upi_payment_is_visibly_unconfirmed_until_somebody_says_otherwise() {
    let scratch = Scratch::new("pay_t7");
    let app = a_shop(&scratch, "t7");
    as_owner(&app, "staff_boss", "Meena");

    let total = one_dosa(&app);
    cart_add_payment_on(
        &app,
        "UPI".to_owned(),
        total.paise(),
        Some("UPI/4477112233".to_owned()),
    )
    .expect("the payment was taken");
    crate::flows::complete_bill_on(&app, None).expect("settled");

    // It is on the list, with its reference and the provider that answered.
    let view = payments_on(&app).expect("the payments screen");
    assert_eq!(view.provider, "Typed in by hand");
    let row = view
        .unconfirmed
        .first()
        .expect("an unconfirmed payment is on the list");
    assert_eq!(row.mode, "UPI");
    assert_eq!(row.reference, "UPI/4477112233");
    assert_eq!(row.amount.paise, total.paise());
    assert!(view.says.contains("not been confirmed"), "{}", view.says);

    // The ask itself was written down, with the provider's own words.
    let attempt = view.attempts.first().expect("the ask was recorded");
    assert_eq!(attempt.answer, "waiting");
    assert_eq!(attempt.because, "nobody has checked the bank yet");

    // Somebody looks at the bank app and says so.
    let after = confirm_payment_on(
        &app,
        row.order_id.clone(),
        row.seq,
        "UPI/4477112233".to_owned(),
    )
    .expect("confirmed");
    assert!(
        after.unconfirmed.is_empty(),
        "a confirmed payment leaves the list"
    );
    assert_eq!(after.says, "Everything taken today is confirmed.");

    // And it cannot be confirmed twice — a second confirmation would be a second name on the
    // same money.
    let again = confirm_payment_on(&app, row.order_id.clone(), row.seq, String::new());
    assert!(again.is_err());
}

/// Cash is never on the unconfirmed list.
#[test]
fn cash_never_reaches_the_unconfirmed_list() {
    let scratch = Scratch::new("pay_cash");
    let app = a_shop(&scratch, "cash");
    as_owner(&app, "staff_boss", "Meena");

    let total = one_dosa(&app);
    cart_add_payment_on(&app, "Cash".to_owned(), total.paise(), None).expect("taken");
    crate::flows::complete_bill_on(&app, None).expect("settled");

    let view = payments_on(&app).expect("the payments screen");
    assert!(view.unconfirmed.is_empty());
    assert!(
        view.attempts.is_empty(),
        "and no attempt row either — one line per cash sale is a ledger nobody reads"
    );
}

// A decline.

/// A declined card leaves the bill unsettled, and the reason is recorded in the machine's own
/// words.
#[test]
fn a_declined_card_leaves_the_bill_unsettled_and_says_why() {
    let scratch = Scratch::new("pay_t8");
    let app = a_shop(&scratch, "t8");
    as_owner(&app, "staff_boss", "Meena");

    app.use_provider(Arc::new(Scripted::new(vec![
        Answer::Declined {
            because: "the bank refused the card".to_owned(),
        },
        Answer::Approved {
            reference: "APPROVAL 8891".to_owned(),
        },
    ])));

    let total = one_dosa(&app);
    let refused = cart_add_payment_on(&app, "Card".to_owned(), total.paise(), None);
    let error = refused.expect_err("a declined card must not become a payment");
    assert!(
        error.message.contains("the bank refused the card"),
        "{}",
        error.message
    );

    // Nothing was taken, so the bill cannot be completed.
    let paid = app
        .with_cart(|state| Ok(state.settlement.total_paid().unwrap_or(Money::ZERO)))
        .expect("the cart");
    assert_eq!(paid, Money::ZERO);
    assert!(
        crate::flows::complete_bill_on(&app, None).is_err(),
        "an unpaid bill does not settle"
    );

    // The refusal is written down, in the machine's words.
    let view = payments_on(&app).expect("the payments screen");
    let attempt = view.attempts.first().expect("the refusal was recorded");
    assert_eq!(attempt.answer, "declined");
    assert_eq!(attempt.because, "the bank refused the card");

    // The customer taps again and it goes through — same command, same code.
    cart_add_payment_on(&app, "Card".to_owned(), total.paise(), None).expect("approved");
    crate::flows::complete_bill_on(&app, None).expect("settled");

    let view = payments_on(&app).expect("the payments screen");
    assert!(
        view.unconfirmed.is_empty(),
        "a provider that APPROVED is a confirmation — this is the whole point \
         of the seam"
    );
    assert!(view.says.contains("refused"), "{}", view.says);
}

// A tip is not takings.

/// A tip appears in none of the three places it could wrongly appear: the sales figure, the tax
/// summary, or the staff-cost denominator — and it reconciles per person on the tips report.
#[test]
fn a_tip_is_in_no_sales_figure_no_tax_figure_and_no_cost_percentage() {
    let scratch = Scratch::new("pay_t9");
    let app = a_shop(&scratch, "t9");
    as_owner(&app, "staff_boss", "Meena");

    let total = one_dosa(&app);
    let tip = Money::from_paise(5_000);
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
        total.add(tip).expect("in range").paise(),
        None,
    )
    .expect("paid, tip included");
    crate::flows::complete_bill_on(&app, None).expect("settled");

    // Not in sales. The day's takings are the bill, not the bill plus the tip.
    let sales = report_on(&app, "sales_day".to_owned(), period()).expect("the sales report");
    let figures = sales.rows.iter().flat_map(|r| r.iter()).collect::<Vec<_>>();
    assert!(
        figures.iter().any(|c| c.contains(&total.to_plain_string())),
        "the day's sales should be the bill total: {figures:?}"
    );
    let with_tip = total.add(tip).expect("in range").to_plain_string();
    assert!(
        !figures.iter().any(|c| c.contains(&with_tip)),
        "a tip has reached the sales figure: {figures:?}"
    );

    // Not in tax. A tip is not a supply by the restaurant, so it is not a taxable value
    // anywhere in the GST summary.
    let gst = report_on(&app, "tax_rate".to_owned(), period()).expect("the tax report");
    let taxable: Vec<&String> = gst.rows.iter().flat_map(|r| r.iter()).collect();
    assert!(
        !taxable.iter().any(|c| c.contains(&with_tip)),
        "a tip has reached the tax summary: {taxable:?}"
    );

    // Not in the staff-cost denominator.
    let cost = crate::employment::staff_cost_on(&app, today(), today()).expect("the staff cost");
    assert_eq!(
        cost.revenue.paise,
        total.paise(),
        "the staff-cost denominator is sales, and a tip is not a sale"
    );

    // And it reconciles per person: one bill, one tipper, the whole tip.
    let tips = report_on(&app, "tips".to_owned(), period()).expect("the tips report");
    let row = tips.rows.first().expect("somebody took a tip");
    assert!(row.iter().any(|c| c.contains("Meena")), "{row:?}");
    assert!(
        row.iter().any(|c| c.contains(&tip.to_plain_string())),
        "the tip reconciles against the payment that carried it: {row:?}"
    );
    assert!(
        tips.notes
            .iter()
            .any(|n| n.contains("not the shop's money")),
        "the report says so in words, once, in Rust: {:?}",
        tips.notes
    );
}

// Money off a bill.

/// A percentage comes off, and the tax comes off with it.
#[test]
fn a_percentage_comes_off_the_bill_before_the_tax() {
    let scratch = Scratch::new("discount_percent");
    let app = a_shop(&scratch, "discount");
    as_owner(&app, "staff_meena", "Meena");
    let full = one_dosa(&app);

    let view = crate::ipc::cart_set_discount_on(&app, "percent".to_owned(), "10".to_owned(), None)
        .expect("the discount is given");

    // 120.00 of dosa, 10% off is 12.00, and the 5% GST is charged on 108.00.
    assert_eq!(
        view.bill.bill_discount.text, "12.00",
        "{:?}",
        view.bill.bill_discount
    );
    assert_eq!(
        view.bill.subtotal.text, "120.00",
        "the subtotal is before the discount"
    );
    assert!(
        view.bill.grand_total.paise < full.paise(),
        "the bill did not get smaller: {} then {}",
        full.to_plain_string(),
        view.bill.grand_total.text
    );
    // The tax was charged on the DISCOUNTED value, which is the claim.
    let taxed: i64 = view.bill.tax_rows.iter().map(|r| r.taxable.paise).sum();
    assert_eq!(taxed, 10_800, "tax was charged on the undiscounted value");

    // And taking it away puts the bill back exactly where it was — a discount is not a payment
    // and must not leave a trace in the money.
    let back = crate::ipc::cart_clear_discount_on(&app).expect("cleared");
    assert_eq!(back.bill.grand_total.paise, full.paise());
    assert_eq!(back.bill.bill_discount.paise, 0);
}

/// A flat amount, and half a per cent, both without a float.
#[test]
fn a_discount_is_parsed_without_a_float() {
    let scratch = Scratch::new("discount_parse");
    let app = a_shop(&scratch, "discount");
    as_owner(&app, "staff_meena", "Meena");
    one_dosa(&app);

    // Half a per cent of 120.00 is 0.60.
    let view = crate::ipc::cart_set_discount_on(&app, "percent".to_owned(), "0.5".to_owned(), None)
        .expect("half a per cent");
    assert_eq!(
        view.bill.bill_discount.text, "0.60",
        "{:?}",
        view.bill.bill_discount
    );

    // 12.5% of 120.00 is 15.00 — and "12.5" must be 1250bp, not 125bp.
    let view =
        crate::ipc::cart_set_discount_on(&app, "percent".to_owned(), "12.5".to_owned(), None)
            .expect("twelve and a half");
    assert_eq!(
        view.bill.bill_discount.text, "15.00",
        "{:?}",
        view.bill.bill_discount
    );

    // And rupees off.
    let view = crate::ipc::cart_set_discount_on(&app, "amount".to_owned(), "50".to_owned(), None)
        .expect("fifty rupees");
    assert_eq!(
        view.bill.bill_discount.text, "50.00",
        "{:?}",
        view.bill.bill_discount
    );
}

/// The refusals a cashier can act on, in the words Rust already wrote.
#[test]
fn a_discount_that_makes_no_sense_is_refused_in_words() {
    let scratch = Scratch::new("discount_refused");
    let app = a_shop(&scratch, "discount");
    as_owner(&app, "staff_meena", "Meena");
    one_dosa(&app);

    for (kind, value) in [
        ("percent", "101"),    // more than the whole bill
        ("percent", "abc"),    // not a number
        ("percent", ""),       // nothing typed
        ("percent", "10.555"), // finer than basis points go
        ("amount", "-5"),      // a "negative discount" is a charge
        ("nonsense", "10"),    // neither kind
    ] {
        assert!(
            crate::ipc::cart_set_discount_on(&app, kind.to_owned(), value.to_owned(), None)
                .is_err(),
            "{kind} {value:?} was accepted"
        );
    }

    // And none of that left a discount behind on the bill.
    let has = app
        .with_cart(|state| Ok(state.bill_discount.is_some()))
        .expect("the cart");
    assert!(!has, "a refused discount was applied anyway");
}

// The press that could only ever fail.

/// A zero-rupee payment is refused before anything outside this process is touched.
#[test]
fn a_zero_payment_is_refused_without_asking_the_provider() {
    let scratch = Scratch::new("pay_zero");
    let app = a_shop(&scratch, "zero");
    as_owner(&app, "staff_boss", "Meena");

    let total = one_dosa(&app);
    cart_add_payment_on(&app, "Cash".to_owned(), total.paise(), None).expect("taken");

    // The bill is covered.
    let refused = cart_add_payment_on(&app, "Card".to_owned(), 0, None)
        .expect_err("a zero payment was accepted");
    assert_eq!(refused.code, "payment.invalid");
    assert_eq!(
        refused.detail.as_deref(),
        Some("a payment has to be more than zero"),
        "the reason a shopkeeper reads must still be the real one",
    );

    // Card is the mode on purpose: cash and credit skip the ledger anyway, so only a mode that
    // WOULD be written down can prove nothing was.
    let view = payments_on(&app).expect("the payments screen");
    assert!(
        view.attempts.is_empty(),
        "a doomed press reached the machine and left a row behind: {:?}",
        view.attempts,
    );

    // And a negative amount is the same refusal, not a credit note by accident.
    assert_eq!(
        cart_add_payment_on(&app, "Card".to_owned(), -100, None)
            .expect_err("a negative payment was accepted")
            .code,
        "payment.invalid",
    );
}
