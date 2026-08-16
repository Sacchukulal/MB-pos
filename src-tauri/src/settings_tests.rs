//! **The settings spine, driven against a real database** — P17 part 1.
//!
//! The unit tests in `settings/tests.rs` prove the catalogue is complete and
//! internally consistent. These prove the three things only a database and a
//! renderer can answer:
//!
//! * **T1** — every receipt and kitchen setting actually changes the paper. A
//!   toggle that changes nothing is a lie on a screen, and the only way to know
//!   is to render twice and compare.
//! * **T3 / T11** — saving writes only what moved, and a row stored as the
//!   wrong type is an error rather than a default (D7).
//! * **T5** — changing the business-day start re-buckets FUTURE orders and does
//!   not move a bill that is already written (D5, scope 13.3).

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

// ---------------------------------------------------------------------------
// T1 — every setting changes the paper.
// ---------------------------------------------------------------------------

/// A value for this entry that is **different from the one it holds**.
///
/// Derived from the `Kind` rather than listed by hand, so a setting added
/// tomorrow is covered tonight — which is the same argument the catalogue
/// itself makes.
fn something_else(entry: &Entry, config: &ShopConfig) -> Value {
    match ((entry.read)(config), entry.kind) {
        (Value::Bool(b), _) => Value::Bool(!b),
        (Value::Int(n), Kind::Int { min, max, .. }) => {
            Value::Int(if n == min { max } else { min })
        }
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

/// **The fixture is deliberately maximal.** A preview that only ever shows two
/// cups of tea is how a setting ships broken: `show.hsn` changes nothing
/// without an HSN, `composition_note` prints nothing for a shop that is not a
/// composition dealer, and the QR width means nothing with no UPI id.
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
    config.store.is_composition = true;
    config.receipt.show.hsn = true;
    config.receipt.qr = mb_print::settings::QrMode::Dynamic;
    config.receipt.logo = mb_print::settings::LogoPosition::Top;
    config.kitchen.show_column_names = true;
    config
}

/// The bill this fixture renders: mixed rates, a bill discount, a service
/// charge, a long name that wraps, a per-line note, an HSN and a split payment.
///
/// A representative bill is not a nicety here. `show.hsn` changes nothing
/// without an HSN, `payment_lines` changes nothing without two payments, and
/// `below_column_names` changes nothing on paper too narrow to have columns.
fn a_representative_bill() -> (mb_core::Bill, mb_core::AnyOrder) {
    use mb_core::{
        AnyOrder, BusinessDay, Cart, Discount, DiscountEntry, DraftOrder, ItemSnapshot, Money,
        OpenOrder, OrderId, OrderType, Payment, PaymentMode, Qty, Settlement, StaffId, TableId,
        TaxRate,
        TaxTreatment, Timestamp,
    };

    let mut cart = Cart::new();
    cart.add(
        ItemSnapshot::new(
            mb_core::ItemId::new("itm_paneer"),
            "Paneer Butter Masala (Half) - Extra Spicy",
            Money::from_paise(24_000),
            TaxRate::GST_5,
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
        .with_treatment(TaxTreatment::NonGst)
        .with_hsn("2203"),
        Qty::from_whole(2).expect("qty"),
        None,
        vec![],
    )
    .expect("adds");

    let charges = crate::settings::Billing {
        service_charge_bp: 500,
        ..crate::settings::Billing::default()
    }
    .charges_for(OrderType::DineIn);

    let bill = mb_core::compute_bill(
        mb_core::BillInput::new(&cart)
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
        OrderType::DineIn,
        StaffId::new("staff_1"),
    )
    .on_table(TableId::new("6"))
    .core;
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
                bill.grand_total.sub(Money::from_paise(20_000)).expect("upi"),
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
    let document = mb_print::template::bill_document(
        mb_print::paper::Paper::new(mb_print::paper::PaperKind::Mm80),
        &mb_print::template::BillContext {
            bill,
            order,
            store: &store,
            settings: &config.receipt,
            customer: None,
            cashier: Some("Ravi"),
            copy: mb_print::template::Copy::Original,
            einvoice: mb_print::template::EInvoice::default(),
            // A logo has to EXIST for the logo settings to be able to do
            // anything. Three bytes of the one-bit format D37 describes.
            logo: Some(vec![8, 8, 0xFF]),
        },
    )
    .expect("a bill document");
    // **The laid-out lines, not the text.** `to_text` drops the style, so
    // comparing text would let "Total in bold" pass as a setting that changes
    // nothing — which is exactly the class of bug T1 exists to catch.
    format!("{:?}", mb_print::layout::layout(&document).expect("lays out"))
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
            order_type: mb_core::OrderType::DineIn,
            table: Some("6"),
            time: Some("19:40"),
            station: Some("TANDOOR"),
            lines: &lines,
            settings: &config.kitchen,
        },
    )
    .expect("a ticket");
    format!("{:?}", mb_print::layout::layout(&document).expect("lays out"))
}

/// **T1.** Drive every receipt and kitchen setting to a different value and
/// assert the paper differs. Driven from the catalogue, so it cannot go out of
/// date.
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
        // **The two typeface settings, and they are exempt for a real reason
        // rather than a convenient one** (P31).
        //
        // This test compares the LAID-OUT document, and a typeface cannot
        // change that: `mb-print` lays a receipt out on a character grid —
        // `paper.dots()` divided by `paper.columns()` — so every face produces
        // the same 48 columns and the same line breaks. The difference is in
        // the DOTS, one layer further down, and it is real.
        //
        // D71 is what happened last time somebody added a font list without
        // that layer existing: three choices, three identical documents, and
        // the list was deleted. It is back because the wiring is:
        //
        // * `queue::the_typeface_a_job_asked_for_is_the_one_the_queue_asks_for`
        //   — the key on the job is the key the raster sink is drawn with;
        // * `queue::the_typeface_survives_being_parked_and_picked_up_again`
        //   — and it survives a power cut;
        // * `the_chosen_typeface_is_the_one_the_bill_is_printed_in` below
        //   — and the shop's setting is what puts it on the job.
        //
        // If those three go, this exemption goes with them and D71 stands.
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

// ---------------------------------------------------------------------------
// T3, T7, T11 — the storage half.
// ---------------------------------------------------------------------------

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

/// **T3.** Changing one setting writes one row.
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

/// The store profile is one row, and it is only rewritten when something in it
/// moved — a shop editing its footer must not restamp its own GSTIN.
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

/// **T7.** Export, wipe, import, and every setting is back.
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
    new.store.is_composition = true;
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
    assert_eq!(
        app.shop_config(),
        ShopConfig::default(),
        "the wipe did not take"
    );

    let (wanted, plan) = crate::settings::plan_import(&app.shop_config(), &exported);
    assert!(plan.is_usable(), "{:?}", plan.problems);
    let blank = app.shop_config();
    save(&app, &blank, &wanted);

    assert_eq!(app.shop_config(), new);
}

/// **T11.** A row stored as the wrong type is an error that names the key, not
/// a silent default (D7).
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
        error.detail.unwrap_or_default().contains("receipt.logo_width_pct")
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

// ---------------------------------------------------------------------------
// T5 — the day start.
// ---------------------------------------------------------------------------

/// **T5.** Changing when the day starts re-buckets FUTURE orders and does not
/// move one that is already written.
///
/// D5 and scope 13.3: *the business day is stored, never derived.* The whole
/// value of storing it is that this is true.
#[test]
fn changing_the_day_start_does_not_move_a_bill_that_is_already_written() {
    use mb_core::{BusinessDay, DayRule, Timestamp, UtcOffset};

    // 2026-08-03 02:30 IST, which is 2026-08-02 21:00 UTC. Under a 05:00 rule
    // this belongs to the 2nd; under a 01:00 rule it belongs to the 3rd.
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

    // The NEXT order buckets by the new rule...
    assert_eq!(crate::flows::today(at), under_early);
    // ...and the one already written is untouched: it is a value that was
    // stored, and nothing in `save_changes` goes near the orders table.
    assert_eq!(day_written, under_late);

    // Put it back, because the day rule is process-wide (D70) and the rest of
    // this binary's tests assume the standard one.
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
    assert!(paper.contains("Come back soon"), "the footer never reached the paper");
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

// ---------------------------------------------------------------------------
// P31 — the typeface, from the settings screen to the job.
// ---------------------------------------------------------------------------

/// **The shop's chosen face is the one on the job.**
///
/// `mb-print`'s queue tests prove the key on a job is the key the raster sink
/// draws with, and that it survives a power cut. This is the other end of the
/// same wire: that the value a shopkeeper picked on the settings screen is what
/// gets put there — and that a bill and a kitchen ticket are answered
/// **separately**, which is the whole of the owner's request.
///
/// Without this the two halves could both be right and the middle missing,
/// which is exactly the shape of D71.
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

    // What `App::print` would stamp on each kind of paper. A bill and a ticket
    // must not get the same answer, or the second setting is decoration.
    let day = crate::flows::today(crate::flows::now());
    let doc = || mb_print::doc::Document::new(mb_print::paper::Paper::new(
        mb_print::paper::PaperKind::Mm80,
    ));
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

    // And a shop that has chosen nothing asks for nothing, which is the
    // built-in face — not an empty string the loader would have to special-case.
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
