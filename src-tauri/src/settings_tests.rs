//! The settings spine, driven against a real database.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_db::{Db, DbConfig, Repos};

use crate::settings::catalog::{CATALOG, Entry, Group};
use crate::settings::value::{Kind, Value};
use crate::settings::{ShopConfig, Storage};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

fn a_shop(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    app
}

// Every setting changes the paper.

/// A value for this entry that is different from the one it holds.
fn something_else(entry: &Entry, config: &ShopConfig) -> Value {
    match ((entry.read)(config), entry.kind) {
        (Value::Bool(b), _) => Value::Bool(!b),
        (Value::Int(n), Kind::Int { min, max, .. }) => Value::Int(if n == min { max } else { min }),
        (Value::Int(n), _) => Value::Int(n + 1),
        (Value::Money(m), Kind::Money { max_paise, .. }) => {
            if m.paise() == 0 {
                Value::Money(mb_core::Money::from_paise(max_paise.min(2_500)))
            } else {
                Value::Money(mb_core::Money::ZERO)
            }
        }
        (Value::Money(_), _) => Value::Money(mb_core::Money::from_paise(1_234)),
        (Value::Text(t), Kind::Choice(options)) => options
            .iter()
            .find(|o| o.value != t)
            .map_or(Value::Text(t.clone()), |o| Value::Text(o.value.to_owned())),
        (Value::Text(t), Kind::Text { max_len, .. }) => {
            let mut other = format!("{t}x");
            other.truncate(max_len);
            Value::Text(other)
        }
        (Value::Text(t), _) => Value::Text(t),
    }
}

/// The fixture is deliberately maximal.
fn a_shop_with_everything() -> ShopConfig {
    let mut config = ShopConfig::default();
    config.store.name = "Anna Kuteera".to_owned();
    config.store.address = "12 MG Road, Jayanagar, Bengaluru 560011".to_owned();
    config.store.phone = "9880012345".to_owned();
    config.store.gstin = "29ABCDE1234F1ZW".to_owned();
    config.store.fssai = "11223344556677".to_owned();
    config.store.state_code = "29".to_owned();
    config.store.upi_id = "anna@upi".to_owned();
    config.store.upi_merchant_name = "Anna Kuteera".to_owned();
    config.store.upi_reference = "MB1".to_owned();
    config.store.registration = "composition".to_owned();
    config.receipt.show.hsn = true;
    config.receipt.qr = mb_print::settings::QrMode::Dynamic;
    config.receipt.logo = mb_print::settings::LogoPosition::Top;
    config.kitchen.show_column_names = true;
    config
}

/// The bill this fixture renders: mixed rates, a bill discount, a service charge, a long name
/// that wraps, a per-line note, an HSN and a split payment.
fn a_representative_bill() -> (mb_core::Bill, mb_core::AnyOrder) {
    use mb_core::{
        AnyOrder, BusinessDay, Cart, Discount, DiscountEntry, DraftOrder, ItemSnapshot, Money,
        OpenOrder, OrderId, OrderType, Payment, PaymentMode, Qty, Settlement, StaffId, TableId,
        TaxRate, Timestamp,
    };

    let mut cart = Cart::new();
    cart.add(
        ItemSnapshot::new(
            mb_core::ItemId::new("itm_paneer"),
            "Paneer Butter Masala (Half) - Extra Spicy",
            Money::from_paise(24_000),
            TaxRate::from_percent(5).expect("5%"),
        )
        .with_hsn("2106"),
        Qty::from_whole(2).expect("qty"),
        Some("no onion".to_owned()),
        vec![],
    )
    .expect("adds");
    cart.add(
        ItemSnapshot::new(
            mb_core::ItemId::new("itm_beer"),
            "Beer 650ml",
            Money::from_paise(22_000),
            TaxRate::ZERO,
        )
        .with_tax(mb_core::TaxSpec::liquor(TaxRate::ZERO))
        .with_hsn("2203"),
        Qty::from_whole(2).expect("qty"),
        None,
        vec![],
    )
    .expect("adds");

    let book = mb_core::TaxBook::new(mb_core::starting_classes(), mb_core::PriceBasis::Exclusive);
    let charges = crate::settings::Billing {
        service_charge_bp: 500,
        ..crate::settings::Billing::default()
    }
    .charges_for(OrderType::DineIn, &book)
    .expect("charges");

    let bill = mb_core::compute_bill(
        mb_core::BillInput::new(&cart, mb_core::Registration::Regular)
            .with_charges(&charges)
            .with_bill_discount(DiscountEntry::new(
                Discount::percent_bp(500).expect("a discount"),
            )),
    )
    .expect("a bill");

    let day = BusinessDay::from_ymd(2026, 8, 3);
    let core = DraftOrder::new(
        OrderId::new("ord_0001"),
        day,
        Timestamp::from_millis(1_770_000_000_000),
        mb_core::Placement::on_table(TableId::new("6")),
        StaffId::new("staff_1"),
    )
    .core;
    // A cover count, so `receipt.show.covers` has something to show.
    let mut core = core;
    core.covers = Some(4);
    let open = OpenOrder {
        core,
        token: mb_core::Claimed {
            value: 42,
            formatted: "42".to_owned(),
            business_day: day,
        },
        bill_number: mb_core::Claimed {
            value: 1_207,
            formatted: "BIR/1207".to_owned(),
            business_day: day,
        },
    };

    // Two payments, so `show.payment_lines` has something to show.
    let mut settlement = Settlement::new();
    settlement
        .add(Payment::new(PaymentMode::Cash, Money::from_paise(20_000)).expect("cash"))
        .expect("cash");
    settlement
        .add(
            Payment::new(
                PaymentMode::Upi,
                bill.grand_total
                    .sub(Money::from_paise(20_000))
                    .expect("upi"),
            )
            .expect("upi"),
        )
        .expect("upi");

    let order = AnyOrder::Settled(
        open.settle(
            bill.clone(),
            settlement,
            Timestamp::from_millis(1_770_000_900_000),
            StaffId::new("staff_1"),
        )
        .expect("settles"),
    );
    (bill, order)
}

fn render_bill(config: &ShopConfig, bill: &mb_core::Bill, order: &mb_core::AnyOrder) -> String {
    let store = config.store.to_print_store();
    // The built-in face on 80 mm — `bill_document` measures the room before it decides whether
    // the item table is one line or two.
    let metrics = mb_print::metrics::Metrics::face(
        mb_print::paper::Paper::new(mb_print::paper::PaperKind::Mm80),
        std::sync::Arc::new(mb_print::font::Font::builtin().expect("the shipped face loads")),
    );
    let document = mb_print::template::bill_document(
        &metrics,
        &mb_print::template::BillContext {
            bill,
            order,
            store: &store,
            settings: &config.receipt,
            customer: None,
            cashier: Some("Ravi"),
            table: Some("6"),
            time: Some("19:42"),
            waiter: Some("Suresh"),
            copy: mb_print::template::Copy::Original,
            einvoice: mb_print::template::EInvoice::default(),
            // A logo has to EXIST for the logo settings to be able to do anything — and it has
            // to DECODE, or `logo` and `logo_width_pct` are two settings pointing at a picture
            // the sink skips.
            logo: Some(mb_print::image::Monochrome::blank(32, 16).encode()),
        },
    )
    .expect("a bill document");
    // The laid-out lines, not the text.
    format!(
        "{:?}",
        mb_print::layout::layout(&document).expect("lays out")
    )
}

fn render_ticket(config: &ShopConfig) -> String {
    let lines = vec![
        mb_print::template::TicketLine {
            name: "Masala Dosa".to_owned(),
            qty: mb_core::Qty::from_whole(2).expect("qty"),
            note: Some("no onion".to_owned()),
            modifiers: vec!["Extra chutney".to_owned()],
        },
        mb_print::template::TicketLine {
            name: "Filter Coffee".to_owned(),
            qty: mb_core::Qty::from_whole(3).expect("qty"),
            note: None,
            modifiers: vec![],
        },
    ];
    let document = mb_print::template::kitchen_document(
        mb_print::paper::Paper::new(mb_print::paper::PaperKind::Mm80),
        &mb_print::template::KitchenContext {
            kind: mb_print::template::TicketKind::New,
            token: Some("42"),
            bill_number: Some("BIR/1207"),
            kot_number: Some("14"),
            order_type: mb_core::OrderType::DineIn,
            table: Some("6"),
            time: Some("19:40"),
            waiter: Some("Suresh"),
            station: Some("TANDOOR"),
            reprint: false,
            lines: &lines,
            settings: &config.kitchen,
        },
    )
    .expect("a ticket");
    format!(
        "{:?}",
        mb_print::layout::layout(&document).expect("lays out")
    )
}

/// Drive every receipt and kitchen setting to a different value and assert the paper differs.
#[test]
fn every_setting_on_the_paper_changes_the_paper() {
    let base_config = a_shop_with_everything();
    let (bill, order) = a_representative_bill();
    let base_bill = render_bill(&base_config, &bill, &order);
    let base_ticket = render_ticket(&base_config);

    for entry in CATALOG {
        if !matches!(entry.group, Group::Receipt | Group::Kitchen) {
            continue;
        }
        // The two typeface settings, and they are exempt for a real reason rather than a
        // convenient one.
        if entry.key == "receipt.font" || entry.key == "kitchen.font" {
            continue;
        }
        let mut config = base_config.clone();
        let other = something_else(entry, &config);
        assert_ne!(
            other,
            (entry.read)(&config),
            "{} has no second value to try, so this test is vacuous for it",
            entry.key
        );
        (entry.write)(&mut config, &other)
            .unwrap_or_else(|e| panic!("{} refused {other:?}: {}", entry.key, e.message));

        let changed = if entry.group == Group::Receipt {
            render_bill(&config, &bill, &order)
        } else {
            render_ticket(&config)
        };
        let base = if entry.group == Group::Receipt {
            &base_bill
        } else {
            &base_ticket
        };
        assert_ne!(
            *base, changed,
            "changing \"{}\" ({}) did not change the paper — the setting is \
             dead, and a screen that offers it is lying",
            entry.label, entry.key
        );
    }
}

/// Every `settings` row in the shop, for a before/after comparison.
fn every_row(app: &App) -> Vec<(String, String)> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let mut stmt = tx.prepare("SELECT key, value FROM settings ORDER BY key")?;
                let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row?);
                }
                Ok(out)
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("the settings table reads")
}

/// Changing one setting writes one row.
#[test]
fn saving_one_setting_writes_exactly_one_row() {
    let scratch = Scratch::new("settings-one-row");
    let app = a_shop(&scratch, "one-row");

    let before = every_row(&app);
    assert!(before.is_empty(), "a fresh shop has stored no settings");

    let old = app.shop_config();
    let mut new = old.clone();
    new.receipt.footer = "Come back soon".to_owned();

    let changed = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    crate::settings::save_changes(
                        &Repos::new(tx),
                        OUTLET,
                        &old,
                        &new,
                        crate::flows::now(),
                        None,
                    )
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("saves");

    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].key, "receipt.footer");
    assert_eq!(changed[0].before, "Thank you, visit again");
    assert_eq!(changed[0].after, "Come back soon");

    let after = every_row(&app);
    assert_eq!(
        after.len(),
        1,
        "posting the whole form back wrote {} rows: {after:?}",
        after.len()
    );
    assert_eq!(after[0].0, "receipt.footer");
}

/// The store profile is one row, and it is only rewritten when something in it moved — a shop
/// editing its footer must not restamp its own GSTIN.
#[test]
fn a_footer_edit_does_not_touch_the_store_profile() {
    let scratch = Scratch::new("settings-profile");
    let app = a_shop(&scratch, "profile");

    let old = app.shop_config();
    let mut new = old.clone();
    new.store.name = "Anna Kuteera".to_owned();
    save(&app, &old, &new);
    let stamped = updated_at(&app);

    let old = new.clone();
    let mut newer = old.clone();
    newer.receipt.footer = "Come back soon".to_owned();
    save(&app, &old, &newer);

    assert_eq!(
        updated_at(&app),
        stamped,
        "a receipt edit rewrote the shop's identity row"
    );
}

fn save(app: &App, old: &ShopConfig, new: &ShopConfig) {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                crate::settings::save_changes(
                    &Repos::new(tx),
                    OUTLET,
                    old,
                    new,
                    crate::flows::now(),
                    None,
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("saves");
    app.reload_shop_config();
}

fn updated_at(app: &App) -> i64 {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Ok(tx.query_row(
                    "SELECT updated_at FROM store_profile WHERE outlet_id = ?1",
                    [OUTLET],
                    |row| row.get::<_, i64>(0),
                )?)
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("the profile exists")
}

/// Export, wipe, import, and every setting is back.
#[test]
fn a_configuration_survives_the_database_being_emptied() {
    let scratch = Scratch::new("settings-round-trip");
    let app = a_shop(&scratch, "round-trip");

    let old = app.shop_config();
    let mut new = old.clone();
    new.receipt.footer = "Visit again!".to_owned();
    new.receipt.pattern = mb_print::doc::Pattern::Double;
    new.receipt.qr_width_pct = 55;
    new.billing.packing_charge = mb_core::Money::from_paise(1_500);
    new.billing.idle_lock_minutes = 0;
    new.day.starts_at_minutes = 240;
    new.store.name = "Anna Kuteera".to_owned();
    new.store.state_code = "29".to_owned();
    new.store.registration = "composition".to_owned();
    save(&app, &old, &new);

    let exported = crate::settings::to_map(&app.shop_config());

    // Everything gone, exactly as a new machine would find it.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                tx.execute("DELETE FROM settings", [])?;
                tx.execute("DELETE FROM store_profile", [])?;
                Ok(())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("wipes");
    app.reload_shop_config();
    // The slabs are seeded rows, not settings, so they survive the wipe by design.
    let loaded = app.shop_config();
    let expected = ShopConfig {
        tax: loaded.tax.clone(),
        ..ShopConfig::default()
    };
    assert_eq!(loaded, expected, "the wipe did not take");

    let (wanted, plan) = crate::settings::plan_import(&app.shop_config(), &exported);
    assert!(plan.is_usable(), "{:?}", plan.problems);
    let blank = app.shop_config();
    save(&app, &blank, &wanted);

    assert_eq!(app.shop_config(), new);
}

/// A row stored as the wrong type is an error that names the key, not a silent default.
#[test]
fn a_setting_stored_as_the_wrong_type_is_an_error_that_names_it() {
    let scratch = Scratch::new("settings-wrong-type");
    let app = a_shop(&scratch, "wrong-type");

    // What a hand-edited database, or a downgrade, would leave behind.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO settings (outlet_id, key, value, value_type, updated_at)
                     VALUES (?1, 'receipt.logo_width_pct', 'quite big', 'text', 0)",
                    [OUTLET],
                )?;
                Ok(())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("writes");

    let error = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| crate::settings::load(&Repos::new(tx), OUTLET))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect_err("a text logo width was accepted");
    assert!(
        error
            .detail
            .unwrap_or_default()
            .contains("receipt.logo_width_pct")
            || error.message.contains("receipt.logo_width_pct"),
        "the failure must name the setting so somebody can fix it"
    );
}

/// A value outside its range, stored, is refused on the way IN as well as out.
#[test]
fn a_stored_value_outside_its_range_is_refused_on_load() {
    let scratch = Scratch::new("settings-out-of-range");
    let app = a_shop(&scratch, "out-of-range");

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO settings (outlet_id, key, value, value_type, updated_at)
                     VALUES (?1, 'receipt.logo_width_pct', '400', 'int', 0)",
                    [OUTLET],
                )?;
                Ok(())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("writes");

    assert!(
        app.with_shop(|shop| {
            shop.db
                .transaction(|tx| crate::settings::load(&Repos::new(tx), OUTLET))
                .map_err(|e| crate::words::from_db(&e))
        })
        .is_err(),
        "a logo four times the width of the paper was accepted"
    );
}

// The day start.

/// Changing when the day starts re-buckets FUTURE orders and does not move one that is already
/// written.
#[test]
fn changing_the_day_start_does_not_move_a_bill_that_is_already_written() {
    use mb_core::{BusinessDay, DayRule, Timestamp, UtcOffset};

    let at = Timestamp::from_millis(1_785_704_400_000);
    let early = DayRule::new(60).expect("01:00 is a time");
    let late = DayRule::DEFAULT;

    let under_late = BusinessDay::of(at, late, UtcOffset::INDIA);
    let under_early = BusinessDay::of(at, early, UtcOffset::INDIA);
    assert_ne!(
        under_late, under_early,
        "the fixture must straddle both rules or this test proves nothing"
    );

    let scratch = Scratch::new("settings-day");
    let app = a_shop(&scratch, "day");

    // A bill written under the standard rule.
    let day_written = crate::flows::today(at);
    assert_eq!(day_written, under_late);

    let old = app.shop_config();
    let mut new = old.clone();
    new.day.starts_at_minutes = 60;
    save(&app, &old, &new);

    // The NEXT order buckets by the new rule..
    assert_eq!(crate::flows::today(at), under_early);
    // ...and the one already written is untouched: it is a value that was stored, and nothing
    // in `save_changes` goes near the orders table.
    assert_eq!(day_written, under_late);

    // Put it back, because the day rule is process-wide and the rest of this binary's tests
    // assume the standard one.
    let old = new.clone();
    let mut back = old.clone();
    back.day.starts_at_minutes = 300;
    save(&app, &old, &back);
    assert_eq!(crate::flows::day_rule(), DayRule::DEFAULT);
}

/// The whole point of part 1: the printed bill uses what the shop chose.
#[test]
fn the_printed_bill_uses_the_shops_own_settings() {
    let scratch = Scratch::new("settings-printed");
    let app = a_shop(&scratch, "printed");

    let old = app.shop_config();
    let mut new = old.clone();
    new.store.name = "Anna Kuteera".to_owned();
    new.receipt.footer = "Come back soon".to_owned();
    save(&app, &old, &new);

    // Read back through `App`, which is what `flows::queue_bill_print` uses.
    let config = app.shop_config();
    assert_eq!(config.store.name, "Anna Kuteera");
    assert_eq!(config.receipt.footer, "Come back soon");

    let (bill, order) = a_representative_bill();
    let paper = render_bill(&config, &bill, &order);
    assert!(
        paper.contains("Come back soon"),
        "the footer never reached the paper"
    );
    assert!(paper.contains("Anna Kuteera"));
}

/// Every entry is stored where it says it is stored.
#[test]
fn only_the_store_group_lives_in_the_store_profile() {
    for entry in CATALOG {
        if entry.storage == Storage::Store {
            assert!(
                entry.key.starts_with("store."),
                "{} is kept in store_profile but is not one of its columns",
                entry.key
            );
        }
    }
}

// The typeface, from the settings screen to the job.

/// The shop's chosen face is the one on the job.
#[test]
fn the_chosen_typeface_is_the_one_the_bill_is_printed_in() {
    let scratch = Scratch::new("settings-typeface");
    let app = a_shop(&scratch, "typeface");

    crate::settings::ipc::save_on(
        &app,
        vec![
            crate::settings::ipc::SettingEdit {
                key: "receipt.font".to_owned(),
                value: "consolas".to_owned(),
            },
            crate::settings::ipc::SettingEdit {
                key: "kitchen.font".to_owned(),
                value: "courier".to_owned(),
            },
        ],
    )
    .expect("both faces save");

    let config = app.shop_config();
    assert_eq!(config.receipt.font, "consolas");
    assert_eq!(config.kitchen.font, "courier");

    // What `App::print` would stamp on each kind of paper.
    let day = crate::flows::today(crate::flows::now());
    let doc = || {
        mb_print::doc::Document::new(mb_print::paper::Paper::new(
            mb_print::paper::PaperKind::Mm80,
        ))
    };
    let bill = mb_print::queue::Job::new(mb_print::queue::JobKind::Bill, "p", doc(), day);
    let ticket = mb_print::queue::Job::new(mb_print::queue::JobKind::Kitchen, "p", doc(), day);

    assert_eq!(
        app.face_for_test(bill.kind),
        Some("consolas".to_owned()),
        "the bill did not take the shop's bill face"
    );
    assert_eq!(
        app.face_for_test(ticket.kind),
        Some("courier".to_owned()),
        "the kitchen ticket did not take the shop's kitchen face"
    );

    // And a shop that has chosen nothing asks for nothing, which is the built-in face — not an
    // empty string the loader would have to special-case.
    crate::settings::ipc::save_on(
        &app,
        vec![crate::settings::ipc::SettingEdit {
            key: "receipt.font".to_owned(),
            value: "builtin".to_owned(),
        }],
    )
    .expect("saves");
    assert_eq!(app.face_for_test(bill.kind), Some("builtin".to_owned()));
}

// The printers.

use crate::settings::printers::{
    PrinterEdit, printers_on, save_printer_on, set_default_printer_on,
};

fn a_printer(name: &str, windows_name: &str, role: &str) -> PrinterEdit {
    PrinterEdit {
        id: String::new(),
        name: name.to_owned(),
        kind: "spooler".to_owned(),
        address: windows_name.to_owned(),
        paper_mm: 80,
        is_default: false,
        role: role.to_owned(),
        engine: "raster".to_owned(),
        is_bold_dark: false,
        can_kick_drawer: false,
    }
}

/// Adding your printer makes it the one bills print on.
#[test]
fn adding_a_real_printer_takes_the_default_off_the_stand_in() {
    let scratch = Scratch::new("printer_default");
    let app = a_shop(&scratch, "printers");

    // The state every shop is in on its first day.
    let before = printers_on(&app).expect("the printers");
    assert_eq!(
        before.printers.len(),
        1,
        "the stand-in should be the only row"
    );
    assert!(before.printers[0].is_stand_in);
    assert!(before.printers[0].is_default, "and it holds the default");

    let after = save_printer_on(&app, a_printer("TVS", "TVSE RP3200 Lite", "both"))
        .expect("the printer saves");

    let real = after
        .printers
        .iter()
        .find(|p| p.name == "TVS")
        .expect("the printer is not in the list");
    assert!(
        real.is_default,
        "adding a real printer left the default on the stand-in — bills print nowhere"
    );
    assert_eq!(
        after.printers.iter().filter(|p| p.is_default).count(),
        1,
        "two printers claim the default"
    );

    // And the thing that actually decides where a bill goes agrees.
    let chosen = crate::flows::default_printer(&app).expect("a printer for bills");
    assert_eq!(chosen.name, "TVS", "bills still go to the stand-in");
}

/// A shop already stuck on the stand-in repairs itself when it opens.
#[test]
fn a_shop_already_printing_nothing_fixes_itself_on_the_way_up() {
    let scratch = Scratch::new("printer_repair");
    let path = scratch.dir().join("stuck.db");
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let at = crate::flows::now();
        repos.settings().save_printer(
            OUTLET,
            &mb_db::repo::settings::Printer {
                id: crate::state::NO_PRINTER.to_owned(),
                name: "No printer set up yet".to_owned(),
                kind: "none".to_owned(),
                address: None,
                paper_mm: 80,
                is_default: true,
                can_kick_drawer: false,
                offset_x_mm: 0,
                offset_y_mm: 0,
                role: "both".to_owned(),
                engine: "raster".to_owned(),
                is_bold_dark: false,
            },
            at,
        )?;
        repos.settings().save_printer(
            OUTLET,
            &mb_db::repo::settings::Printer {
                id: "prn_tvs".to_owned(),
                name: "TVS".to_owned(),
                kind: "spooler".to_owned(),
                address: Some("TVSE RP3200 Lite".to_owned()),
                paper_mm: 80,
                is_default: false,
                can_kick_drawer: false,
                offset_x_mm: 0,
                offset_y_mm: 0,
                role: "both".to_owned(),
                engine: "raster".to_owned(),
                is_bold_dark: false,
            },
            at,
        )
    })
    .expect("the broken shop");

    // Opening it is the whole test.
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);

    assert_eq!(
        crate::flows::default_printer(&app).expect("a printer").name,
        "TVS",
        "the shop opened still printing to the stand-in"
    );
    let view = printers_on(&app).expect("the printers");
    assert!(
        view.printers
            .iter()
            .find(|p| p.name == "TVS")
            .expect("tvs")
            .is_default
    );
    assert!(
        !view
            .printers
            .iter()
            .find(|p| p.is_stand_in)
            .expect("stand-in")
            .is_default,
        "two printers claim the default"
    );
}

/// And a shop on its FIRST day is left alone.
#[test]
fn a_brand_new_shop_keeps_its_stand_in() {
    let scratch = Scratch::new("printer_firstday");
    let app = a_shop(&scratch, "firstday");

    let view = printers_on(&app).expect("the printers");
    assert_eq!(view.printers.len(), 1);
    assert!(view.printers[0].is_stand_in);
    assert!(
        view.printers[0].is_default,
        "a new shop has no default at all"
    );
}

/// A size an older build wrote still opens.
#[test]
fn a_size_saved_by_an_older_build_still_opens() {
    let scratch = Scratch::new("settings_legacy_size");
    let app = a_shop(&scratch, "legacy");

    // Exactly what is on disk in a shop that chose "Large" before today.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let settings = Repos::new(tx).settings();
                settings.set(
                    OUTLET,
                    "receipt.sections.items.scale",
                    &"2".to_owned(),
                    crate::flows::now(),
                    None,
                )?;
                settings.set(
                    OUTLET,
                    "receipt.sections.store_name.scale",
                    &"3".to_owned(),
                    crate::flows::now(),
                    None,
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("the old rows");

    let config = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| crate::settings::load(&Repos::new(tx), OUTLET))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("the settings load, rather than falling back to standard");

    // Rung for rung.
    assert_eq!(
        config.receipt.sections.items.size,
        mb_print::Style::LADDER[7],
        "\"Large\" stopped being large"
    );
    assert_eq!(
        config.receipt.sections.store_name.size,
        mb_print::Style::LADDER[9]
    );
    // And the text engine still gets the multiplier its hardware understands.
    assert_eq!(config.receipt.sections.items.scale(), 2);
    assert_eq!(config.receipt.sections.store_name.scale(), 3);
}

/// A size that is no longer on the list still opens, at the nearest one that is.
#[test]
fn a_size_that_left_the_list_snaps_to_the_nearest_one_on_it() {
    let scratch = Scratch::new("settings_offlist_size");
    let app = a_shop(&scratch, "offlist");

    // 46 and 28 were on the twenty-two-value list and are not on the ten.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let settings = Repos::new(tx).settings();
                settings.set(
                    OUTLET,
                    "receipt.sections.items.scale",
                    &"46".to_owned(),
                    crate::flows::now(),
                    None,
                )?;
                settings.set(
                    OUTLET,
                    "receipt.sections.meta.scale",
                    &"28".to_owned(),
                    crate::flows::now(),
                    None,
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("the rows");

    let config = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| crate::settings::load(&Repos::new(tx), OUTLET))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("the settings load rather than falling back to standard");

    // 46 was never on any list and snaps to the nearest rung; 28 was the fourth entry of the
    // nominal list and reads back as the fourth rung.
    assert!(mb_print::Style::LADDER.contains(&config.receipt.sections.items.size));
    assert_eq!(
        config.receipt.sections.meta.size,
        mb_print::Style::LADDER[3]
    );
    // Whatever it snapped to must be something the screen can show back, or the dropdown would
    // open on nothing.
    for size in [
        config.receipt.sections.items.size,
        config.receipt.sections.meta.size,
    ] {
        assert!(
            crate::settings::is_a_size(size),
            "{size} is not on the list"
        );
    }
}

/// Choosing 2 inch changes the paper the bill is laid out on.
#[test]
fn choosing_the_paper_width_relays_out_the_bill() {
    let scratch = Scratch::new("printer_paper");
    let app = a_shop(&scratch, "printers");
    save_printer_on(&app, a_printer("TVS", "TVSE RP3200 Lite", "both")).expect("saves");

    // Three inch is where a shop starts.
    let wide = crate::settings::ipc::preview_on(&app, "receipt".to_owned(), Vec::new())
        .expect("the preview");
    assert!(wide.paper.contains("3 inch"), "{}", wide.paper);
    // Measured, not assumed: how many characters fit is a fact about the face and the size, so
    // the number is whatever the built-in face gives at the body size — and it is more on the
    // wider roll, which is the whole claim this test makes.
    let wide_columns = wide.doc.columns;
    assert!(
        wide_columns >= 40,
        "80 mm paper fits only {wide_columns} characters"
    );

    crate::settings::printers::set_paper_on(&app, 58).expect("two inch");

    let narrow = crate::settings::ipc::preview_on(&app, "receipt".to_owned(), Vec::new())
        .expect("the preview");
    assert!(narrow.paper.contains("2 inch"), "{}", narrow.paper);
    assert!(
        narrow.doc.columns < wide_columns,
        "58 mm fits {} characters and 80 mm fits {wide_columns}",
        narrow.doc.columns
    );

    // And it is on the printer bills actually go to, so the paper a customer's bill comes out
    // on moved with it — not just the picture on the screen.
    assert_eq!(
        crate::flows::default_printer(&app)
            .expect("a printer")
            .paper
            .kind,
        mb_print::paper::PaperKind::Mm58
    );

    // Four inch, and a width nobody sells is refused rather than stored.
    crate::settings::printers::set_paper_on(&app, 100).expect("four inch");
    assert_eq!(
        crate::settings::ipc::preview_on(&app, "receipt".to_owned(), Vec::new())
            .expect("the preview")
            .doc
            .columns,
        64
    );
    assert!(crate::settings::printers::set_paper_on(&app, 70).is_err());
}

/// A shop that has already chosen is not overruled.
#[test]
fn a_second_printer_does_not_steal_the_default() {
    let scratch = Scratch::new("printer_second");
    let app = a_shop(&scratch, "printers");

    save_printer_on(&app, a_printer("Counter", "EPSON TM-T82", "both")).expect("saves");
    let after =
        save_printer_on(&app, a_printer("Kitchen", "TVSE RP3200 Lite", "kitchen")).expect("saves");

    let counter = after
        .printers
        .iter()
        .find(|p| p.name == "Counter")
        .expect("counter");
    let kitchen = after
        .printers
        .iter()
        .find(|p| p.name == "Kitchen")
        .expect("kitchen");
    assert!(counter.is_default, "the second printer stole the bills");
    assert!(!kitchen.is_default);
}

/// The dropdown works.
#[test]
fn choosing_where_bills_print_moves_them_there() {
    let scratch = Scratch::new("printer_choose");
    let app = a_shop(&scratch, "printers");

    save_printer_on(&app, a_printer("Counter", "EPSON TM-T82", "both")).expect("saves");
    let view =
        save_printer_on(&app, a_printer("Back office", "HP LaserJet", "both")).expect("saves");
    let back = view
        .printers
        .iter()
        .find(|p| p.name == "Back office")
        .expect("it")
        .id
        .clone();

    let after = set_default_printer_on(&app, back.clone()).expect("chooses");
    assert_eq!(
        after.printers.iter().filter(|p| p.is_default).count(),
        1,
        "choosing one did not clear the other"
    );
    assert!(
        after
            .printers
            .iter()
            .find(|p| p.id == back)
            .expect("it")
            .is_default
    );
    assert_eq!(
        crate::flows::default_printer(&app).expect("a printer").name,
        "Back office"
    );

    // And an id that is not a printer is refused rather than leaving the shop with no default
    // at all.
    assert!(set_default_printer_on(&app, "prn_nothing".to_owned()).is_err());
}

/// "Bills only" and "Kitchen tickets only" mean something now.
#[test]
fn a_kitchen_only_printer_does_not_get_the_bills() {
    let scratch = Scratch::new("printer_roles");
    let app = a_shop(&scratch, "printers");

    // The kitchen printer arrives first, so it is the one that displaces the stand-in and holds
    // `is_default`.
    save_printer_on(&app, a_printer("Kitchen", "TVSE RP3200 Lite", "kitchen")).expect("saves");
    save_printer_on(&app, a_printer("Counter", "EPSON TM-T82", "bill")).expect("saves");

    assert_eq!(
        crate::flows::default_printer(&app).expect("a printer").name,
        "Counter",
        "a bill was sent to a kitchen-tickets-only printer"
    );
}

/// The schedule takes a backup when the newest one is old, and leaves a fresh one alone.
#[test]
fn the_schedule_backs_up_when_the_newest_one_is_old() {
    let scratch = Scratch::new("backup_schedule");
    let app = a_shop(&scratch, "schedule");
    let copies = scratch.dir().join("copies");
    let mut config = app.shop_config();
    config.backup.every_hours = 1;
    config.backup.folder = copies.display().to_string();
    app.publish_shop_config(config);

    assert!(
        crate::settings::backup::take_if_due(&app).expect("taken"),
        "no backup yet, so one was due"
    );
    assert!(
        !crate::settings::backup::take_if_due(&app).expect("checked"),
        "a fresh backup was taken again"
    );
    assert_eq!(mb_db::backup::list(&copies).expect("list").len(), 1);
}
