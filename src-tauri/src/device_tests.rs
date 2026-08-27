#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_auth::{Actor, PermissionSet, RolePreset};
use mb_core::{Money, StaffId};
use mb_db::{Db, DbConfig};

use crate::devices::{devices_on, read_scale_on};
use crate::ipc::{StaffEdit, save_staff_member_on};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

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
    app
}

/// One whole bill, through the real path.
fn a_bill(app: &App) -> String {
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
    crate::flows::complete_bill_on(app, None).expect("settled")
}

// Nothing plugged in.

/// A counter with no scanner, no scale, no display and no label printer bills normally, and
/// nothing on the device screen reads as a problem.
#[test]
fn a_shop_with_no_devices_at_all_bills_normally_and_is_not_nagged() {
    let scratch = Scratch::new("dev_t1");
    let app = a_shop(&scratch, "t1");

    // The defaults are "nothing plugged in", which is what nearly every shop that buys this
    // will have.
    let config = app.shop_config();
    assert!(config.devices.scale_port.is_empty());
    assert!(!config.devices.display_on);
    assert!(config.devices.label_printer.is_empty());

    // It bills.
    let number = a_bill(&app);
    assert!(!number.is_empty(), "a bill number came back");

    // And the device screen says so without calling it a fault.
    let view = devices_on(&app).expect("the device screen");
    let scale = view
        .devices
        .iter()
        .find(|d| d.kind == "scale")
        .expect("a scale row");
    assert!(!scale.set_up);
    assert!(
        scale.says.contains("Most shops have no scale"),
        "a shop with no scale is finished, not broken: {}",
        scale.says
    );
    assert!(!scale.testable, "there is nothing to test");

    // The scanner is always ready, because there is nothing to plug in.
    let scanner = view
        .devices
        .iter()
        .find(|d| d.kind == "scanner")
        .expect("a scanner row");
    assert!(scanner.set_up);

    // Reading a scale that does not exist is not an error, and does not interrupt anything.
    let read = read_scale_on(&app).expect("no error");
    assert!(!read.answered);
    assert_eq!(read.says, "No scale is set up. Type the quantity instead.");

    // And it still bills afterwards.
    assert!(!a_bill(&app).is_empty());
}

// Plugged in, not answering.

/// A scale on a port that does not exist gives up inside its deadline, says something a
/// shopkeeper can act on, and the sale still completes.
#[test]
fn a_scale_that_does_not_answer_says_so_quickly_and_the_bill_still_settles() {
    let scratch = Scratch::new("dev_t2");
    let app = a_shop(&scratch, "t2");

    // A port nothing is on.
    let mut config = app.shop_config();
    config.devices.scale_port = "COM99".to_owned();
    app.publish_shop_config(config);

    let started = std::time::Instant::now();
    let read = read_scale_on(&app).expect("a device problem is not an error");
    let took = started.elapsed();

    assert!(!read.answered);
    assert!(
        !read.says.is_empty() && read.says.ends_with('.'),
        "a sentence a shopkeeper can read: {}",
        read.says
    );
    assert!(
        read.says.contains("COM99"),
        "and it names the port, because that is what they can check: {}",
        read.says
    );
    assert!(
        took < std::time::Duration::from_secs(3),
        "a dead device must give up inside its deadline, took {took:?}"
    );

    // The sale still completes.
    let number = a_bill(&app);
    assert!(!number.is_empty());

    // And the device screen shows it as set up but says what happened when asked — a configured
    // device that is not answering is a different fact from one that was never set up.
    let view = devices_on(&app).expect("the device screen");
    let scale = view
        .devices
        .iter()
        .find(|d| d.kind == "scale")
        .expect("a scale row");
    assert!(scale.set_up);
    assert!(scale.testable);
}

/// A label printer that is not set up refuses in words and does not pretend.
#[test]
fn a_label_with_no_label_printer_says_where_to_set_one_up() {
    let scratch = Scratch::new("dev_label");
    let app = a_shop(&scratch, "label");

    let refused =
        crate::devices::print_label_on(&app, "2 x Masala Dosa".to_owned(), "T-14".to_owned());
    let error = refused.expect_err("there is no label printer");
    assert_eq!(error.code, "label.none");
    assert!(error.message.contains("Settings"), "{}", error.message);

    // And the bill is unaffected.
    assert!(!a_bill(&app).is_empty());
}

// The customer display never takes focus.

#[test]
fn the_customer_display_is_built_unfocused_and_never_asks_for_focus() {
    let source = include_str!("devices.rs");

    assert!(
        source.contains("const DISPLAY_TAKES_FOCUS: bool = false;"),
        "the rule is a named constant so that changing it means reading why"
    );
    assert!(
        source.contains(".focused(DISPLAY_TAKES_FOCUS)"),
        "the display window must be built from that constant"
    );
    // The two calls that would undo it.
    assert!(
        !source.contains("set_focus"),
        "nothing in the device module may ask for focus"
    );
    assert!(
        !source.contains("always_on_top"),
        "a display over the billing window is the same bug wearing a hat"
    );
}
