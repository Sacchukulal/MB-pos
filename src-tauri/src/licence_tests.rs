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
pub(crate) fn a_trading_shop(scratch: &Scratch, name: &str) -> App {
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
                        tax_class_id: mb_core::seeded_placement(mb_core::TaxSpec::gst(
                            mb_core::TaxRate::from_percent(5).expect("5%"),
                        ))
                        .expect("a seeded slab")
                        .0,
                        price_basis: mb_core::seeded_placement(mb_core::TaxSpec::gst(
                            mb_core::TaxRate::from_percent(5).expect("5%"),
                        ))
                        .expect("a seeded slab")
                        .1,
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
        .activate("MB-STUB-0001", at, std::time::Duration::from_secs(2))
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
pub(crate) fn a_bill_is_taken(app: &App) -> String {
    let item = app.find_menu_item("itm_tea").expect("on the menu");
    app.with_cart_mut(|state| {
        *state = crate::billing::CartState::new_order(mb_core::OrderType::Parcel);
        state
            .cart
            .add(
                crate::billing::snapshot_for(&item, &app.shop_config().tax).expect("a slab"),
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

#[test]
fn expired_license_locks_all_reports_but_keeps_new_billing_and_operational_drawer() {
    let scratch = Scratch::new("expired_bill_work");
    let app = a_trading_shop(&scratch, "expired_bill_work");
    a_bill_is_taken(&app);
    let id = crate::corrections::list_bills_on(&app).expect("paid lookup")[0].order_id.clone();
    app.use_licensing(licence_in(&scratch, "expired", Status::Active, -100));
    let denied = [
        crate::reports::list_on(&app).expect_err("Reports list"),
        crate::corrections::bills_on(&app, Default::default()).expect_err("Bills list"),
        crate::corrections::bill_detail_on(&app, id.clone()).expect_err("history"),
        crate::refunds::offer_on(&app, id.clone()).expect_err("refund offer"),
        crate::corrections::revert_bill_on(&app, id.clone(), "Edit".into(), None, None).expect_err("edit"),
        crate::corrections::void_bill_on(&app, id.clone(), "Void".into(), None, None).expect_err("void"),
        crate::refunds::refund_batch_on(&app, id.clone(), vec![("cash".into(), mb_core::Money::from_paise(100))], "Return".into()).expect_err("refund"),
        crate::refunds::return_closed_bill_on(&app, id.clone(), vec![], "Return".into(), "expired-return".into(), None, None).expect_err("historical return"),
        crate::corrections::reprint_bill_on(&app, id.clone(), "Copy".into()).expect_err("reprint"),
        crate::flows::bill_pdf_on(&app, id.clone()).expect_err("historical invoice"),
        crate::flows::preview_order_on(&app, Some(id)).expect_err("historical preview"),
        crate::dayclose::days_on(&app).expect_err("Days history"),
        crate::reports::dashboard_on(&app, None).expect_err("dashboard"),
    ];
    for refusal in denied { assert_eq!(refusal.code, "licence.not_operating"); }
    a_bill_is_taken(&app);
    let drawer = crate::dayclose::drawer_on(&app, None).expect("drawer counting remains available");
    assert!(drawer.takings.is_empty(), "drawer must not bundle sales summaries");
    assert!(drawer.expected.paise > 0, "operational expected cash is still available");
}

#[test]
fn configured_grace_keeps_reports_and_history_open_until_it_ends() {
    let scratch = Scratch::new("reports_grace");
    let app = a_trading_shop(&scratch, "reports_grace");
    a_bill_is_taken(&app);
    app.use_licensing(licence_in(&scratch, "grace", Status::Active, -1));
    assert!(app.entitlement().operating());
    crate::reports::list_on(&app).expect("Reports in grace");
    let bill = crate::corrections::list_bills_on(&app).expect("Bills in grace").remove(0);
    crate::corrections::bill_detail_on(&app, bill.order_id.clone()).expect("history in grace");
    crate::refunds::offer_on(&app, bill.order_id.clone()).expect("offer in grace");
    crate::corrections::revert_bill_on(&app, bill.order_id.clone(), "Fix".into(), None, None).expect("edit in grace");
    crate::correction_draft::discard_on(&app, bill.order_id).expect("discard");
    app.use_licensing(licence_in(&scratch, "past_grace", Status::Active, -100));
    assert_eq!(crate::reports::list_on(&app).expect_err("grace ended").code, "licence.not_operating");
}

#[test]
fn expired_shop_prints_and_previews_an_unpaid_order_even_with_a_preassigned_number() {
    let scratch = Scratch::new("expired_unpaid_print");
    let app = a_trading_shop(&scratch, "expired_unpaid_print");
    app.use_licensing(licence_in(&scratch, "expired", Status::Active, -100));
    crate::ipc::cart_add_on(&app, "itm_tea".into(), None, None).expect("new order");
    let mut open = crate::flows::park_open_order(&app).expect("park ordinary order");
    let id = open.core.id.as_str().to_owned();
    // The current print flow does not allocate an invoice number early, but the core
    // supports old/preassigned unpaid orders. A number alone is not an issued sale.
    open.bill_number = Some(mb_core::Claimed { value: 42, formatted: "PRE/0042".into(), business_day: open.core.business_day });
    crate::flows::save_order(&app, &mb_core::AnyOrder::Open(open)).expect("preassigned unpaid order");
    let (printed, jobs) = crate::signin_tests::queue_took(&app, || crate::flows::print_open_bill_on(&app, id.clone()));
    printed.expect("free unpaid printing");
    assert!(jobs.contains(&mb_print::queue::JobKind::Bill));
    crate::flows::preview_order_on(&app, Some(id)).expect("free unpaid preview");
    let (_, jobs) = crate::signin_tests::queue_took(&app, || a_bill_is_taken(&app));
    assert!(jobs.contains(&mb_print::queue::JobKind::Bill), "ordinary final receipt still prints after expiry");
}

#[test]
fn expiry_rechecks_an_open_correction_before_writes_but_allows_discard_and_new_billing() {
    let scratch = Scratch::new("stale_correction_licence");
    let app = a_trading_shop(&scratch, "stale_correction_licence");
    a_bill_is_taken(&app);
    let id = crate::corrections::list_bills_on(&app).expect("bill")[0].order_id.clone();
    crate::corrections::revert_bill_on(&app, id.clone(), "Fix".into(), None, None).expect("edit while paid");
    crate::ipc::cart_add_on(&app, "itm_tea".into(), None, None).expect("edit before expiry");
    let before = app.with_cart(|cart| Ok(cart.clone())).expect("snapshot");
    app.use_licensing(licence_in(&scratch, "expired", Status::Active, -100));
    let denied = [
        crate::correction_draft::preview_on(&app).expect_err("review"),
        crate::correction_draft::save_on(&app).expect_err("explicit save"),
        crate::flows::complete_bill_on(&app, Some("Cash".into())).expect_err("stale completion"),
        crate::ipc::cart_add_on(&app, "itm_tea".into(), None, None).expect_err("stale item edit"),
        crate::ipc::take_payment(&app, "Cash".into(), 100, None).expect_err("stale payment"),
    ];
    for refusal in denied { assert_eq!(refusal.code, "licence.not_operating"); }
    assert_eq!(app.with_cart(|cart| Ok(cart.clone())).expect("unchanged"), before);
    crate::correction_draft::discard_on(&app, id).expect("safe discard stays free");
    crate::ipc::cart_add_on(&app, "itm_tea".into(), Some("2".into()), None).expect("ordinary item addition stays free");
    crate::corrections::change_line_on(&app, 0, None, String::new()).expect("ordinary item cancellation stays free");
    a_bill_is_taken(&app);
}

#[test]
fn expired_correction_with_money_can_be_parked_without_trapping_new_bills() {
    let scratch = Scratch::new("expired_correction_exit");
    let app = a_trading_shop(&scratch, "expired_correction_exit");
    a_bill_is_taken(&app);
    let id = crate::corrections::list_bills_on(&app).expect("bill")[0].order_id.clone();
    crate::corrections::revert_bill_on(&app, id.clone(), "Add tea".into(), None, None).expect("edit");
    crate::ipc::cart_add_on(&app, "itm_tea".into(), None, None).expect("item");
    crate::ipc::take_payment(&app, "Cash".into(), 100, None).expect("partial new payment");
    app.use_licensing(licence_in(&scratch, "expired", Status::Active, -100));
    assert_eq!(crate::correction_draft::discard_on(&app, id.clone()).expect_err("money must not disappear").code, "revert.money_changed");
    crate::ipc::cart_clear_on(&app, false).expect("park and leave safely");
    assert_eq!(crate::ipc::open_order_on(&app, id).expect_err("correction is Reports work").code, "licence.not_operating");
    a_bill_is_taken(&app);
}

#[test]
fn expired_cashier_gets_license_refusal_before_report_permission_fallback() {
    let scratch = Scratch::new("expired_report_cashier");
    let app = a_trading_shop(&scratch, "expired_report_cashier");
    crate::signin_tests::hire(&app, "staff_cashier", "Cashier", mb_auth::RolePreset::Cashier, "1357");
    crate::ipc::lock_now_on(&app).expect("lock");
    crate::ipc::login_on(&app, "staff_cashier".into(), "1357".into()).expect("cashier");
    assert_eq!(crate::reports::list_on(&app).expect_err("paid permission fallback").code, "auth.denied");
    app.use_licensing(licence_in(&scratch, "expired", Status::Active, -100));
    assert_eq!(crate::reports::list_on(&app).expect_err("expired Reports").code, "licence.not_operating");
    assert_eq!(crate::corrections::bills_on(&app, Default::default()).expect_err("expired cashier Bills").code, "licence.not_operating");
}

#[test]
fn default_cashier_can_find_and_read_receipts_without_reports_permission() {
    let scratch = Scratch::new("cashier_bill_work");
    let app = a_trading_shop(&scratch, "cashier_bill_work");
    a_bill_is_taken(&app);
    crate::signin_tests::hire(&app, "staff_cashier", "Cashier", mb_auth::RolePreset::Cashier, "1357");
    crate::ipc::lock_now_on(&app).expect("lock");
    crate::ipc::login_on(&app, "staff_cashier".to_owned(), "1357".to_owned()).expect("cashier signs in");
    let bills = crate::corrections::bills_on(&app, Default::default()).expect("cashier lookup");
    assert_eq!(bills.rows.len(), 1);
    assert!(bills.can_revert && bills.can_reprint);
    assert!(bills.totals.is_none());
    assert!(!bills.can_export);
    crate::corrections::bill_detail_on(&app, bills.rows[0].order_id.clone()).expect("cashier receipt");
    assert_eq!(crate::reports::list_on(&app).expect_err("reports stay denied").code, "auth.denied");
}

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
            crate::reports::dashboard_on(&app, None).expect_err("the dashboard was allowed"),
        ),
        (
            "open_pairing",
            crate::lan::open_pairing_on(&app).expect_err("pairing was allowed"),
        ),
        (
            "allow_device",
            crate::lan::allow_on(&app, "req_anything".to_owned(), None)
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
    assert!(listed.contains(&"report_print"));
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
    match crate::dayclose::day_state_on(&app, None) {
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
    assert!(crate::licensing::gate_phones(&app).is_ok());
    assert!(crate::reports::list_on(&app).is_ok());
}

/// A licence whose plan carries exactly these feature codes and this many phones.
fn licence_with_plan(scratch: &Scratch, label: &str, features: &[&str], phones: u32) -> Licensing {
    let dir = scratch.dir().join(label);
    let _ = std::fs::create_dir_all(&dir);
    let at = crate::flows::now();
    let today = crate::flows::today(at);
    let mut licence = Stub::active(
        &machine(),
        BusinessDay::from_days_since_epoch(today.days_since_epoch() + 30),
        at,
    )
    .licence();
    licence.plan.features = mb_license::FeatureSet::from_codes(features.iter().copied());
    licence.plan.limits.devices = phones;
    let stub = Arc::new(Stub::with(licence, at));
    let mut licensing = Licensing::new(dir, machine(), stub as Arc<dyn Cloud>, "test");
    licensing
        .activate("MB-STUB-0001", at, std::time::Duration::from_secs(2))
        .expect("the stub activates");
    licensing
}

/// Phone ordering is a switch the admin panel holds, on for every plan unless somebody turns it
/// off. A plan that has it, with phones to spare, lets a phone in; a plan without it refuses by
/// name, whatever its phone count says.
#[test]
fn phone_ordering_is_the_plans_switch() {
    let scratch = Scratch::new("phones_switch");
    let app = a_trading_shop(&scratch, "phones_switch");
    app.use_licensing(licence_with_plan(&scratch, "on", &["mobile-ordering"], 10));
    assert_eq!(app.entitlement().standing, Standing::Fine);
    assert!(crate::licensing::gate_phones(&app).is_ok());
    // The command itself: whatever else stops it in a test (no network is up), it is not the
    // plan.
    if let Err(e) = crate::lan::open_pairing_on(&app) {
        assert!(
            !e.code.starts_with("licence."),
            "refused by the plan: {e:?}"
        );
    }

    app.use_licensing(licence_with_plan(&scratch, "off", &["reports"], 10));
    let refusal = crate::licensing::gate_phones(&app).expect_err("phones were allowed");
    assert_eq!(refusal.code, "licence.not_in_plan");
    assert!(
        refusal.message.contains("phone ordering"),
        "{}",
        refusal.message
    );
    assert_eq!(
        crate::lan::open_pairing_on(&app)
            .expect_err("pairing was allowed")
            .code,
        "licence.not_in_plan"
    );
}

/// Past the switch, the count is the control: a plan with no phones refuses them, in its own
/// words, with billing untouched.
#[test]
fn a_plan_with_no_phones_refuses_them_by_the_count() {
    let scratch = Scratch::new("phones_none");
    let app = a_trading_shop(&scratch, "phones_none");
    app.use_licensing(licence_with_plan(
        &scratch,
        "none",
        &["reports", "mobile-ordering"],
        0,
    ));

    let refusal = crate::licensing::gate_phones(&app).expect_err("phones were allowed");
    assert_eq!(refusal.code, "licence.no_phones");
    assert!(refusal.message.contains("bill"), "{}", refusal.message);
    let refusal = crate::lan::open_pairing_on(&app).expect_err("pairing was allowed");
    assert_eq!(refusal.code, "licence.no_phones");
    assert!(crate::lan::allow_on(&app, "req_anything".to_owned(), None).is_err());
    // The plan is fine; only the phones are refused.
    assert!(crate::licensing::gate(&app, Feature::Reports).is_ok());
    let _ = a_bill_is_taken(&app);
}

// Primitive new-order paths never gate. Flows also hosts historical documents and
// corrections, so its free new-bill path is covered by the expiry tests above.

/// PERFORMANCE §2.2: "Nothing in this table may ever be blocked by a > report, a sync, a print
/// job, a licence check or a backup.
#[test]
fn the_billing_path_does_not_ask_about_the_licence() {
    for (name, source) in [
        ("billing.rs", include_str!("billing.rs")),
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
        .activate("MB-STUB-0001", at, std::time::Duration::from_secs(2))
        .expect("activates");
    stub.behave(Behaviour::Unreachable);
    app.use_licensing(licensing);

    let view = crate::licensing::sign_out_on(&app).expect("deactivates locally");
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
    app.use_licensing(crate::licensing::for_tests_blank());
    let view = crate::licensing::view_on(&app);
    assert_eq!(view.standing, "never-activated");
    assert_eq!(view.chip, "Not activated");
    assert!(!view.has_licence);
    assert!(view.key.is_empty(), "a key on a counter with no licence");
    assert!(
        view.date_label.is_empty(),
        "a date row with no date to show"
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

/// The lock screen never asks about the plan: a PIN gets in whatever the licence says.
#[test]
fn a_lapsed_plan_does_not_keep_anybody_out() {
    let scratch = Scratch::new("no_door");
    let app = a_trading_shop(&scratch, "no_door");
    crate::signin_tests::hire(
        &app,
        "staff_boss",
        "Meena",
        mb_auth::RolePreset::Owner,
        "2468",
    );
    app.use_licensing(licence_in(&scratch, "no_door", Status::Active, -100));
    assert!(!app.entitlement().operating());
    crate::ipc::login_on(&app, "staff_boss".to_owned(), "2468".to_owned()).expect("signed in");
    // The plan running out while somebody is signed in changes the banner, not the session.
    crate::licensing::after_licence_change(&app);
    assert!(app.sessions().current().is_some());
    // And a cancelled plan inside its paid day is simply running.
    app.use_licensing(licence_in(&scratch, "no_door_ending", Status::Cancelled, 5));
    assert_eq!(
        app.entitlement().standing,
        Standing::Ending { days_left: 5 }
    );
    assert!(app.entitlement().operating());
}

/// Putting a licence on a counter from the Account page: the owner's row takes the licence
/// holder's name, gets the PIN that was typed, and that person is signed in.
#[test]
fn change_licence_names_the_owner_sets_the_pin_and_signs_them_in() {
    let scratch = Scratch::new("change_licence");
    let app = a_trading_shop(&scratch, "change_licence");
    app.use_licensing(crate::licensing::for_tests_blank());
    // The row the first run left behind, under a name that is not the account's.
    crate::firstrun::owner_row_named(&app, "Owner", false).expect("an owner row");

    let view = crate::licensing::change_licence_on(
        &app,
        crate::licensing::LicenceDoor::Key {
            key: "MB-STUB-0001".to_owned(),
        },
        false,
        "4839".to_owned(),
    )
    .expect("the stub licence goes on");

    assert!(view.has_licence);
    assert_eq!(
        view.owner_name, "Anna Kuteera",
        "the owner's name is the licence holder's"
    );
    assert_eq!(view.owner_phone, "9840011223");
    assert_eq!(view.key, "MB-STUB-0001", "the owner sees the key");

    let owners: Vec<(String, bool)> = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    Ok(Repos::new(tx)
                        .people()
                        .list_staff(OUTLET)?
                        .into_iter()
                        .filter(|p| p.role_id.as_deref() == Some("role_owner"))
                        .map(|p| (p.name, p.pin_hash.is_some()))
                        .collect())
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("the staff list");
    assert_eq!(
        owners,
        vec![("Anna Kuteera".to_owned(), true)],
        "one owner row, renamed, holding the new PIN"
    );
    let current = app.sessions().current().expect("somebody is signed in");
    assert_eq!(current.actor.name, "Anna Kuteera");
    assert!(
        crate::ipc::login_on(
            &app,
            current.actor.staff_id.as_str().to_owned(),
            "4839".to_owned()
        )
        .is_ok(),
        "the typed PIN signs the owner in"
    );

    // The same key again is refused: nothing to change.
    let again = crate::licensing::change_licence_on(
        &app,
        crate::licensing::LicenceDoor::Key {
            key: "MB-STUB-0001".to_owned(),
        },
        false,
        "4839".to_owned(),
    );
    assert_eq!(again.expect_err("refused").code, "licence.same");
}
