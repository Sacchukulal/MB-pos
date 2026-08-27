#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests: expect is the assertion"
)]

use std::sync::Arc;

use mb_core::BusinessDay;
use mb_db::{Db, DbConfig, Repos};
use mb_license::cloud::{Behaviour, Stub};
use mb_license::{Cloud, Feature, LicenceFile, Licensing, MachineId, Standing, Status};

use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

// A shop, and a licence in whatever state the test needs.

pub(crate) fn machine() -> MachineId {
    MachineId::for_tests("4c4c4544-0043-4a10-8033-b8c04f4d3132")
}

/// A shop that can trade: one item, an owner at the counter.
fn a_trading_shop(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Repos::new(tx).menu().save_item(
                    OUTLET,
                    &mb_db::repo::menu::MenuItem {
                        id: mb_core::ItemId::new("itm_tea"),
                        category_id: None,
                        name: "Masala Tea".to_owned(),
                        unit_price: mb_core::Money::from_paise(2_500),
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
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a menu item");
    app
}

/// Install a licence in a given state, through the real activate path.
pub(crate) fn licence_in(
    scratch: &Scratch,
    label: &str,
    status: Status,
    renews_in_days: i32,
) -> Licensing {
    let dir = scratch.dir().join(label);
    let _ = std::fs::create_dir_all(&dir);
    let at = crate::flows::now();
    let today = crate::flows::today(at);
    let stub = Arc::new(Stub::active(
        &machine(),
        BusinessDay::from_days_since_epoch(today.days_since_epoch() + renews_in_days),
        at,
    ));
    let mut licensing = Licensing::new(dir, machine(), Arc::clone(&stub) as Arc<dyn Cloud>, "test");
    licensing
        .activate(
            "MB-STUB-0001",
            "123456",
            at,
            std::time::Duration::from_secs(2),
        )
        .expect("the stub activates");
    if status != Status::Active {
        stub.set_status(status);
        licensing
            .refresh(at, std::time::Duration::from_secs(2))
            .expect("the stub refreshes");
    }
    licensing
}

/// Put a tea in the cart and settle it, the way the billing screen does.
fn a_bill_is_taken(app: &App) -> String {
    let item = app.find_menu_item("itm_tea").expect("on the menu");
    app.with_cart_mut(|state| {
        *state = crate::billing::CartState::new_order(mb_core::OrderType::Parcel);
        state
            .cart
            .add(
                crate::billing::snapshot_for(&item),
                mb_core::Qty::from_whole(2).expect("in range"),
                None,
                vec![],
            )
            .expect("added");
        Ok(())
    })
    .expect("a cart");

    app.with_cart_mut(|state| {
        let total = state.bill(&app.shop_config())?.grand_total;
        state
            .settlement
            .add(mb_core::Payment::new(mb_core::PaymentMode::Cash, total).expect("a payment"))
            .expect("paid");
        Ok(())
    })
    .expect("paid");

    let number = crate::flows::complete_bill_on(app, None).expect("the shop billed");

    // And the row really is there.
    let count = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let mut statement = tx.prepare("SELECT COUNT(*) FROM bills")?;
                    let n: i64 = statement.query_row([], |row| row.get(0))?;
                    Ok(n)
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("counted");
    assert!(count >= 1, "the bill did not reach the disk");
    number
}

// BILLING NEVER STOPS.

/// 1 of 5 — no internet.
#[test]
fn a_shop_bills_with_no_internet() {
    let scratch = Scratch::new("bills_offline");
    let app = a_trading_shop(&scratch, "offline");

    let dir = scratch.dir().join("offline-licence");
    let _ = std::fs::create_dir_all(&dir);
    let stub = Arc::new(Stub::active(
        &machine(),
        crate::flows::today(crate::flows::now()),
        crate::flows::now(),
    ));
    stub.behave(Behaviour::Unreachable);
    app.use_licensing(Licensing::new(
        dir,
        machine(),
        stub as Arc<dyn Cloud>,
        "test",
    ));

    assert_eq!(app.entitlement().standing, Standing::NeverActivated);
    a_bill_is_taken(&app);
}

/// 2 of 5 — an expired plan.
#[test]
fn a_shop_bills_with_an_expired_plan() {
    let scratch = Scratch::new("bills_expired");
    let app = a_trading_shop(&scratch, "expired");
    // Renewed a hundred days ago, so well past any grace period.
    app.use_licensing(licence_in(&scratch, "expired", Status::Active, -100));

    assert_eq!(app.entitlement().standing, Standing::Expired);
    assert!(!app.entitlement().operating());
    a_bill_is_taken(&app);
}

/// 3 of 5 — a suspended licence.
#[test]
fn a_shop_bills_while_suspended() {
    let scratch = Scratch::new("bills_suspended");
    let app = a_trading_shop(&scratch, "suspended");
    // A billing date a year away, and suspended anyway.
    app.use_licensing(licence_in(&scratch, "suspended", Status::Suspended, 365));

    assert_eq!(app.entitlement().standing, Standing::Suspended);
    a_bill_is_taken(&app);
}

/// 4 of 5 — a revoked licence.
#[test]
fn a_shop_bills_while_revoked() {
    let scratch = Scratch::new("bills_revoked");
    let app = a_trading_shop(&scratch, "revoked");
    app.use_licensing(licence_in(&scratch, "revoked", Status::Revoked, 365));

    assert_eq!(app.entitlement().standing, Standing::Revoked);
    a_bill_is_taken(&app);
}

/// 5 of 5 — a corrupt cache.
#[test]
fn a_shop_bills_with_a_corrupt_licence_file() {
    let scratch = Scratch::new("bills_corrupt");
    let app = a_trading_shop(&scratch, "corrupt");

    let dir = scratch.dir().join("corrupt-licence");
    std::fs::create_dir_all(&dir).expect("a folder");
    std::fs::write(LicenceFile::path(&dir), "{ half a file, no closing").expect("writes");

    app.use_licensing(Licensing::new(
        dir.clone(),
        machine(),
        Arc::new(Stub::active(
            &machine(),
            crate::flows::today(crate::flows::now()),
            crate::flows::now(),
        )) as Arc<dyn Cloud>,
        "test",
    ));

    assert_eq!(app.entitlement().standing, Standing::NeverActivated);
    a_bill_is_taken(&app);
    // And the broken file was kept, because it is evidence.
    assert!(
        LicenceFile::path(&dir)
            .with_extension("broken.json")
            .exists()
    );
}

// Refused in the core, not merely hidden.

/// Every gated command, called directly, with a licence that does not entitle the shop.
#[test]
fn every_gated_command_is_refused_when_the_shop_is_not_entitled() {
    let scratch = Scratch::new("gated");
    let app = a_trading_shop(&scratch, "gated");
    app.use_licensing(licence_in(&scratch, "gated", Status::Suspended, 365));
    assert!(!app.entitlement().operating());

    let day = crate::flows::today(crate::flows::now());
    let (year, month, date) = day.to_ymd();
    let period = crate::reports::PeriodArg {
        from: format!("{year:04}-{month:02}-{date:02}"),
        to: format!("{year:04}-{month:02}-{date:02}"),
    };

    let refusals: Vec<(&str, crate::words::UiError)> = vec![
        (
            "report_list",
            crate::reports::list_on(&app).expect_err("the list was allowed"),
        ),
        (
            "report",
            crate::reports::report_on(&app, "sales_day".to_owned(), period.clone())
                .expect_err("a report was allowed"),
        ),
        (
            "dashboard",
            crate::reports::dashboard_on(&app).expect_err("the dashboard was allowed"),
        ),
        (
            "open_pairing",
            crate::lan::open_pairing_on(&app).expect_err("pairing was allowed"),
        ),
        (
            "allow_device",
            crate::lan::allow_on(&app, "req_anything".to_owned())
                .expect_err("a device was allowed"),
        ),
    ];

    for (command, refusal) in &refusals {
        assert_eq!(
            refusal.code, "licence.not_operating",
            "{command} refused for the wrong reason: {refusal:?}"
        );
        // And the sentence says what still works.
        assert!(
            refusal.message.contains("bill"),
            "{command}'s refusal does not say billing is unaffected: {}",
            refusal.message
        );
    }

    // The table and the reality agree.
    let listed: Vec<&str> = crate::licensing::GATED
        .iter()
        .map(|(name, _)| *name)
        .collect();
    for (command, _) in &refusals {
        assert!(
            listed.contains(command),
            "{command} is gated and not listed"
        );
    }
    // `report_csv` and `report_pdf` go through `report_on`, so they are listed and covered by
    // that one refusal rather than by their own.
    assert!(listed.contains(&"report_csv"));
    assert!(listed.contains(&"report_pdf"));
}

/// The day close is NOT gated, and this is the test that keeps it that way.
#[test]
fn closing_the_day_is_not_behind_the_licence() {
    let scratch = Scratch::new("dayclose_gate");
    let app = a_trading_shop(&scratch, "dayclose");
    app.use_licensing(licence_in(&scratch, "dayclose", Status::Suspended, 365));
    assert!(!app.entitlement().operating());

    // It answers. What it says about the day is `dayclose`'s own business; the claim here is
    // only that the LICENCE did not stop it.
    match crate::dayclose::view_on(&app, None) {
        Ok(_) => {}
        Err(e) => assert_ne!(
            e.code, "licence.not_operating",
            "the day close was refused by the licence gate"
        ),
    }

    for command in mb_license::Feature::REPORTS_DOES_NOT_MEAN_THE_DAY_CLOSE {
        assert!(
            !crate::licensing::GATED
                .iter()
                .any(|(name, _)| name == command),
            "{command} is behind the licence gate and must not be"
        );
    }
}

/// A shop that IS entitled is not refused — otherwise the test above would pass on a gate that
/// refused everybody.
#[test]
fn an_entitled_shop_is_not_refused() {
    let scratch = Scratch::new("entitled");
    let app = a_trading_shop(&scratch, "entitled");
    app.use_licensing(licence_in(&scratch, "entitled", Status::Active, 30));

    assert_eq!(app.entitlement().standing, Standing::Fine);
    assert!(crate::licensing::gate(&app, Feature::Reports).is_ok());
    assert!(crate::licensing::gate(&app, Feature::MobileOrdering).is_ok());
    assert!(crate::reports::list_on(&app).is_ok());
}

// The licence is not allowed anywhere near the billing path.

/// PERFORMANCE §2.2: "Nothing in this table may ever be blocked by a > report, a sync, a print
/// job, a licence check or a backup.
#[test]
fn the_billing_path_does_not_ask_about_the_licence() {
    for (name, source) in [
        ("billing.rs", include_str!("billing.rs")),
        ("flows.rs", include_str!("flows.rs")),
        ("orders.rs", include_str!("orders.rs")),
        ("search.rs", include_str!("search.rs")),
    ] {
        for (number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for forbidden in ["licensing::gate", "entitlement()", "mb_license::"] {
                assert!(
                    !code.contains(forbidden),
                    "{name} line {} touches the licence, and it is on the billing \
                     path — PERFORMANCE §2.2: {}",
                    number + 1,
                    code.trim()
                );
            }
        }
    }
}

#[test]
fn an_offline_deactivate_tells_the_owner_the_licence_is_still_held() {
    let scratch = Scratch::new("still_held");
    let app = a_trading_shop(&scratch, "still_held");

    let dir = scratch.dir().join("still-held-licence");
    let _ = std::fs::create_dir_all(&dir);
    let at = crate::flows::now();
    let stub = Arc::new(Stub::active(&machine(), crate::flows::today(at), at));
    let mut licensing = Licensing::new(dir, machine(), Arc::clone(&stub) as Arc<dyn Cloud>, "test");
    licensing
        .activate(
            "MB-STUB-0001",
            "123456",
            at,
            std::time::Duration::from_secs(2),
        )
        .expect("activates");
    stub.behave(Behaviour::Unreachable);
    app.use_licensing(licensing);

    let view = crate::licensing::deactivate_on(&app).expect("deactivates locally");
    assert!(
        view.still_held.contains("still held"),
        "the screen did not say the licence is still held: {}",
        view.still_held
    );
    assert!(stub.released().is_empty());

    // And it is in the shop's history, with the reason.
    let rows = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let mut statement = tx.prepare("SELECT action, entity_id FROM audit_log")?;
                    let found: Vec<(String, Option<String>)> = statement
                        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                        .filter_map(Result::ok)
                        .collect();
                    Ok(found)
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("read the history");
    let note = rows
        .iter()
        .find(|(what, _)| what == "licence.deactivated")
        .expect("nothing was written to the history");
    assert!(
        note.1.as_deref().is_some_and(|d| d.contains("queued")),
        "the history does not record that the server was not told: {note:?}"
    );
}

/// The account screen draws on a counter with no licence and no shop.
#[test]
fn the_account_screen_draws_on_a_first_run() {
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    let view = crate::licensing::view_on(&app);
    assert_eq!(view.standing, "never-activated");
    assert_eq!(view.chip, "Not activated");
    assert!(!view.is_activated);
    assert!(
        !view.machine.is_empty(),
        "no machine id to read out to support"
    );
    assert!(!view.headline.is_empty());
    assert!(view.phones_allowed > 0);
}

#[test]
fn the_plans_phone_limit_reaches_the_network_layer() {
    let scratch = Scratch::new("device_limit");
    let app = a_trading_shop(&scratch, "device_limit");

    app.use_licensing(licence_in(&scratch, "limit", Status::Active, 30));
    let on_a_live_plan = app.entitlement().limits.devices;
    assert!(on_a_live_plan > 0);

    app.use_licensing(licence_in(&scratch, "limit2", Status::Suspended, 365));
    let suspended = app.entitlement();
    assert!(!suspended.operating());

    let source = include_str!("lan.rs");
    let body: String = source
        .lines()
        .skip_while(|line| !line.contains("fn device_limit"))
        .take(30)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        body.contains("entitlement()"),
        "device_limit does not ask the entitlement, so lowering a plan's phone \
         limit would not cut anybody off — WEBSITE-C5"
    );
}

#[test]
fn l1_the_gate_is_cheap_enough_to_put_anywhere() {
    let scratch = Scratch::new("l1");
    let app = a_trading_shop(&scratch, "l1");
    app.use_licensing(licence_in(&scratch, "l1", Status::Active, 30));

    let mut best = u128::MAX;
    for _ in 0..3 {
        let started = std::time::Instant::now();
        for _ in 0..1_000 {
            let _ = crate::licensing::gate(&app, Feature::Reports);
        }
        best = best.min(started.elapsed().as_nanos());
    }
    // A benchmark's average is the one place a remainder is not a loss.
    #[allow(
        clippy::integer_division,
        reason = "an average of a thousand timings, not a rupee"
    )]
    let each_ns = best / 1_000;
    // 50 µs budget, 200 µs ceiling.
    assert!(
        each_ns < 200_000,
        "the gate costs {each_ns} ns, past its 200 µs ceiling"
    );
    println!("L1: {each_ns} ns per gate check");
}
