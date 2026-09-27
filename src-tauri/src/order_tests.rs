#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests: expect is the assertion"
)]

use std::sync::Arc;

use mb_auth::{Permission, PermissionSet};
use mb_core::{StaffId, Timestamp};
use mb_db::{Db, DbConfig, Repos};
use mb_lan::intent::{Intent, Outcome, What};

use crate::orders;
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

fn a_shop(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for (id, name, paise) in [
            ("itm_dosa", "Masala Dosa", 12_000_i64),
            ("itm_coffee", "Filter Coffee", 3_000),
        ] {
            repos.menu().save_item(
                OUTLET,
                &mb_db::repo::menu::MenuItem {
                    id: mb_core::ItemId::new(id),
                    category_id: None,
                    name: name.to_owned(),
                    unit_price: mb_core::Money::from_paise(paise),
                    tax_class_id: mb_core::seeded_placement(mb_core::TaxSpec::gst(mb_core::TaxRate::from_percent(5).expect("5%"))).expect("a seeded slab").0,
                    price_basis: mb_core::seeded_placement(mb_core::TaxSpec::gst(mb_core::TaxRate::from_percent(5).expect("5%"))).expect("a seeded slab").1,
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
            )?;
        }
        // A floor, because a phone opens a TABLE and a table is a foreign key.
        repos.floor().save_section(
            OUTLET,
            &mb_db::repo::floor::Section {
                id: "sec_main".to_owned(),
                name: "Main hall".to_owned(),
                sort_order: 0,
                is_active: true,
            },
            crate::flows::now(),
        )?;
        repos.floor().save_table(
            OUTLET,
            &mb_db::repo::floor::DiningTable {
                id: mb_core::TableId::new("tbl_7"),
                section_id: Some("sec_main".to_owned()),
                label: "7".to_owned(),
                seats: 4,
                pos: None,
                sort_order: 0,
                is_active: true,
            },
            crate::flows::now(),
        )?;
        Ok(())
    })
    .expect("a menu");

    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    with_gstin(&app);
    app
}

/// A registered shop — a blank GST number bills without GST, by design.
fn with_gstin(app: &App) {
    let mut config = app.shop_config();
    config.store.gstin = "29ABCDE1234F1Z5".to_owned();
    config.store.state_code = "29".to_owned();
    app.publish_shop_config(config);
}

fn waiter() -> (StaffId, PermissionSet) {
    let mut may = PermissionSet::new();
    may.insert(Permission::BillCreate);
    may.insert(Permission::OrderItemVoid);
    may.insert(Permission::OrderCancel);
    (StaffId::new("staff_default"), may)
}

fn intent(id: &str, order: Option<&str>, what: What) -> Intent {
    Intent {
        id: id.to_owned(),
        order_id: order.map(ToOwned::to_owned),
        open_intent_id: None,
        at: crate::flows::now().millis(),
        sent_at: None,
        what,
    }
}

fn go(app: &App, i: &Intent) -> Outcome {
    let (staff, may) = waiter();
    orders::apply(app, "dev_test", &staff, &may, i)
        .expect("the counter answered")
        .outcome
}

fn open_one(app: &App, table: Option<&str>) -> String {
    let out = go(
        app,
        &intent(
            &format!("open_{}", mb_auth::random_token(8)),
            None,
            What::OpenOrder {
                order_type: if table.is_some() { "dine_in" } else { "parcel" }.to_owned(),
                table_id: table.map(ToOwned::to_owned),
                covers: None,
            },
        ),
    );
    match out {
        Outcome::Ok { order_id, .. } => order_id,
        other => panic!("the order did not open: {other:?}"),
    }
}

/// Idempotency, hard.
#[test]
fn the_same_intent_fifty_times_makes_one_order() {
    let scratch = Scratch::new("p20_idem");
    let app = Arc::new(a_shop(&scratch, "idem"));

    let one = intent(
        "the-one-and-only",
        None,
        What::OpenOrder {
            order_type: "parcel".to_owned(),
            table_id: None,
            covers: None,
        },
    );

    let mut threads = Vec::new();
    for _ in 0..50 {
        let app = Arc::clone(&app);
        let one = one.clone();
        threads.push(std::thread::spawn(move || go(&app, &one)));
    }
    let replies: Vec<Outcome> = threads
        .into_iter()
        .map(|t| t.join().expect("no thread panicked"))
        .collect();

    assert_eq!(replies.len(), 50);
    let first = &replies[0];
    for (n, reply) in replies.iter().enumerate() {
        assert_eq!(
            reply, first,
            "reply {n} differs from the first — a waiter would see two answers \
             for one tap"
        );
    }

    // And exactly one order exists.
    let orders = app
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| Repos::new(tx).orders().list_open(OUTLET))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("read");
    assert_eq!(
        orders.len(),
        1,
        "fifty retries made {} orders",
        orders.len()
    );
}

/// Money is never computed on the phone.
#[test]
fn the_counters_figure_wins_over_anything_a_phone_sends() {
    let scratch = Scratch::new("p20_money");
    let app = a_shop(&scratch, "money");
    let order = open_one(&app, None);

    // A phone one version ahead sends a total.
    let lying: Intent = serde_json::from_str(&format!(
        r#"{{"id":"lie","order_id":"{order}","at":{},
             "what":{{"do":"add_item","item_id":"itm_dosa","qty":"2",
                      "note":null,"modifiers":[],"total":"1.00","price":"0.50"}}}}"#,
        crate::flows::now().millis()
    ))
    .expect("a lying phone still parses");

    match go(&app, &lying) {
        Outcome::Ok { total, lines, .. } => {
            // Two dosas at 120.00 is 240.00 plus the menu's 5% tax, from the counter's own menu.
            assert_eq!(total, "252.00", "the phone's number reached the bill");
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].amount, "240.00");
        }
        other => panic!("it refused a perfectly good intent: {other:?}"),
    }
}

/// The kitchen delta, and the retry that must not re-send.
#[test]
fn the_counter_decides_the_kitchen_delta_and_a_retry_sends_nothing() {
    let scratch = Scratch::new("p20_kitchen");
    let app = a_shop(&scratch, "kitchen");
    let order = open_one(&app, None);

    go(
        &app,
        &intent(
            "add1",
            Some(&order),
            What::AddItem {
                item_id: "itm_dosa".to_owned(),
                qty: "2".to_owned(),
                note: None,
                modifiers: vec![],
            },
        ),
    );

    let first = go(&app, &intent("fire1", Some(&order), What::SendToKitchen));
    assert!(
        first.message().contains("2 items") || first.message().contains("1 item"),
        "it did not say what it sent: {}",
        first.message()
    );

    // The same intent again — the retry a flaky connection produces.
    let retry = go(&app, &intent("fire1", Some(&order), What::SendToKitchen));
    assert_eq!(retry, first, "a retry printed a second ticket");

    // A DIFFERENT send, with nothing new, tells the truth rather than reprinting.
    let again = go(&app, &intent("fire2", Some(&order), What::SendToKitchen));
    assert!(
        again.message().contains("already has"),
        "it re-sent food the kitchen already had: {}",
        again.message()
    );

    // Add more, and only the NEW part goes.
    go(
        &app,
        &intent(
            "add2",
            Some(&order),
            What::AddItem {
                item_id: "itm_coffee".to_owned(),
                qty: "1".to_owned(),
                note: None,
                modifiers: vec![],
            },
        ),
    );
    let third = go(&app, &intent("fire3", Some(&order), What::SendToKitchen));
    assert!(
        third.message().contains("1 item"),
        "the delta was wrong: {}",
        third.message()
    );
}

/// A line raised after it went out tells the phone how much the kitchen has.
#[test]
fn a_raised_line_says_how_much_the_kitchen_has() {
    let scratch = Scratch::new("p20_raised");
    let app = a_shop(&scratch, "raised");
    let order = open_one(&app, None);
    go(
        &app,
        &intent(
            "add",
            Some(&order),
            What::AddItem {
                item_id: "itm_dosa".to_owned(),
                qty: "2".to_owned(),
                note: None,
                modifiers: vec![],
            },
        ),
    );
    go(&app, &intent("fire1", Some(&order), What::SendToKitchen));

    let raised = go(
        &app,
        &intent(
            "raise",
            Some(&order),
            What::SetQty {
                line: 0,
                qty: "3".to_owned(),
            },
        ),
    );
    let Outcome::Ok { lines, .. } = &raised else {
        panic!("{raised:?}");
    };
    assert_eq!(lines[0].qty, "3");
    assert_eq!(lines[0].in_kitchen, "2", "the kitchen was told about two");
    assert!(
        !lines[0].sent_to_kitchen,
        "the phone would hide Send to kitchen with one still to go"
    );

    let fired = go(&app, &intent("fire2", Some(&order), What::SendToKitchen));
    assert!(
        fired.message().contains("1 item"),
        "only the extra one goes: {}",
        fired.message()
    );
    let Outcome::Ok { lines, .. } = &fired else {
        panic!("{fired:?}");
    };
    assert_eq!(lines[0].in_kitchen, "3");
    assert!(lines[0].sent_to_kitchen, "the kitchen has all of it now");
}

/// The conflicts, each as documented.
#[test]
fn every_conflict_resolves_the_way_the_protocol_says() {
    let scratch = Scratch::new("p20_conflict");
    let app = a_shop(&scratch, "conflict");

    // (a) two waiters open the same table at once: the second JOINS the first, because two
    // waiters at one table are serving one party.
    let first = open_one(&app, Some("tbl_7"));
    let second = go(
        &app,
        &intent(
            "join",
            None,
            What::OpenOrder {
                order_type: "dine_in".to_owned(),
                table_id: Some("tbl_7".to_owned()),
                covers: None,
            },
        ),
    );
    match second {
        Outcome::Ok { order_id, note, .. } => {
            assert_eq!(order_id, first, "the table grew a second order");
            assert!(
                note.unwrap_or_default().contains("same order"),
                "the waiter was not told they had joined somebody"
            );
        }
        other => panic!("{other:?}"),
    }

    // (c) voiding a line the kitchen has already made: the waiter holds the permission the
    // counter's own screen asks for, so it goes through, the kitchen is told to stop, and the
    // audit trail says who.
    go(
        &app,
        &intent(
            "c-add",
            Some(&first),
            What::AddItem {
                item_id: "itm_dosa".to_owned(),
                qty: "1".to_owned(),
                note: None,
                modifiers: vec![],
            },
        ),
    );
    go(&app, &intent("c-fire", Some(&first), What::SendToKitchen));
    let voided = go(
        &app,
        &intent(
            "c-void",
            Some(&first),
            What::VoidItem {
                line: 0,
                reason: "customer changed their mind".to_owned(),
            },
        ),
    );
    let said = voided.message();
    assert!(
        said.contains("off the order"),
        "a cooked dish could not be taken off from the floor: {said}"
    );
    assert!(
        said.contains("stop"),
        "it did not say the kitchen is being told: {said}"
    );
    let Outcome::Ok { lines, .. } = &voided else {
        panic!("{voided:?}");
    };
    assert!(lines.is_empty(), "the line is still on the order");
    let Some(mb_core::AnyOrder::Open(open)) =
        crate::flows::find_order(&app, &mb_core::OrderId::new(first.clone())).expect("read")
    else {
        panic!("the order is not open any more");
    };
    assert!(
        open.core.kitchen.is_empty(),
        "the ledger still says the kitchen is cooking it"
    );
    let voids = app
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| {
                    Repos::new(tx).audit().list(
                        OUTLET,
                        &mb_db::repo::AuditFilter {
                            action: Some(mb_auth::audit::action::ITEM_VOIDED.to_owned()),
                            limit: 10,
                            ..Default::default()
                        },
                    )
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("the audit trail");
    assert_eq!(voids.len(), 1, "the void was not written down: {voids:?}");

    // And reducing the quantity below what was cooked is refused, in words with the number in
    // them: it is a void, and a void wants a reason.
    go(
        &app,
        &intent(
            "c-add2",
            Some(&first),
            What::AddItem {
                item_id: "itm_dosa".to_owned(),
                qty: "2".to_owned(),
                note: None,
                modifiers: vec![],
            },
        ),
    );
    go(&app, &intent("c-fire2", Some(&first), What::SendToKitchen));
    let shrunk = go(
        &app,
        &intent(
            "c-qty",
            Some(&first),
            What::SetQty {
                line: 0,
                qty: "1".to_owned(),
            },
        ),
    );
    assert!(
        shrunk.message().contains("already been told"),
        "{}",
        shrunk.message()
    );

    // (e) a phone editing an order the counter has finished with.
    let done = open_one(&app, None);
    go(
        &app,
        &intent(
            "e-cancel",
            Some(&done),
            What::CancelOrder {
                reason: "walked out".to_owned(),
            },
        ),
    );
    let after = go(
        &app,
        &intent(
            "e-add",
            Some(&done),
            What::AddItem {
                item_id: "itm_dosa".to_owned(),
                qty: "1".to_owned(),
                note: None,
                modifiers: vec![],
            },
        ),
    );
    let said = after.message();
    assert!(
        said.contains("cancelled at the counter"),
        "a cancelled order took another item: {said}"
    );
    assert!(
        said.contains("Start a new one"),
        "the waiter was not told what to do instead: {said}"
    );

    // A table this shop does not have.
    let ghost = go(
        &app,
        &intent(
            "ghost-table",
            None,
            What::OpenOrder {
                order_type: "dine_in".to_owned(),
                table_id: Some("tbl_does_not_exist".to_owned()),
                covers: None,
            },
        ),
    );
    let said = ghost.message();
    assert!(
        said.contains("not on this shop's floor"),
        "a deleted table gave a database error: {said}"
    );
    assert!(
        said.contains("refresh"),
        "it did not say what to do: {said}"
    );

    // A void with no reason is refused.
    let no_reason = go(
        &app,
        &intent(
            "no-reason",
            Some(&first),
            What::VoidItem {
                line: 0,
                reason: "   ".to_owned(),
            },
        ),
    );
    assert!(no_reason.message().contains("needs a reason"));
}

/// The cashier is never clobbered.
#[test]
fn a_phone_cannot_wipe_what_the_cashier_is_typing() {
    let scratch = Scratch::new("p20_cashier");
    let app = a_shop(&scratch, "cashier");
    let order = open_one(&app, None);

    // The cashier has that order open and has typed a payment into it.
    app.with_cart_mut(|state| {
        state.origin = Some(crate::billing::Origin {
            baseline: None,
            id: mb_core::OrderId::new(order.clone()),
            created_at: crate::flows::now(),
            business_day: crate::flows::today(crate::flows::now()),
            opened_by: mb_core::StaffId::new(crate::state::DEFAULT_STAFF),
        });
        state
            .settlement
            .add(
                mb_core::Payment::new(
                    mb_core::PaymentMode::Cash,
                    mb_core::Money::from_paise(50_000),
                )
                .expect("a payment"),
            )
            .map_err(|e| crate::words::UiError::new("bill.pay", "no").with_detail(e.to_string()))?;
        Ok(())
    })
    .expect("the cashier is settling");

    let (staff, may) = waiter();
    let applied = orders::apply(
        &app,
        "dev_test",
        &staff,
        &may,
        &intent(
            "floor-add",
            Some(&order),
            What::AddItem {
                item_id: "itm_dosa".to_owned(),
                qty: "2".to_owned(),
                note: None,
                modifiers: vec![],
            },
        ),
    )
    .expect("applied");

    // The counter took the change — it is the authority — and the cashier is TOLD rather than
    // overwritten.
    assert!(matches!(applied.outcome, Outcome::Ok { .. }));
    let change = applied
        .tell_the_cashier
        .expect("the cashier was not told the floor had touched their order");
    assert!(change.says.contains("Masala Dosa"), "{}", change.says);
    assert!(change.says.contains('2'), "{}", change.says);

    // The cashier's payment is untouched.
    app.with_cart(|state| {
        assert_eq!(state.settlement.payments().len(), 1);
        assert_eq!(
            state.settlement.total_paid().expect("a total").paise(),
            50_000
        );
        Ok(())
    })
    .expect("the cart survived");
}

/// A batch of offline intents, in order, with a per-intent report.
#[test]
fn a_batch_applies_in_order_and_reports_each_one() {
    let scratch = Scratch::new("p20_batch");
    let app = a_shop(&scratch, "batch");
    let order = open_one(&app, None);

    let mut intents = Vec::new();
    for n in 0..100 {
        intents.push(intent(
            &format!("b{n}"),
            Some(&order),
            What::AddItem {
                item_id: "itm_coffee".to_owned(),
                qty: "1".to_owned(),
                note: None,
                modifiers: vec![],
            },
        ));
    }

    let (staff, may) = waiter();
    let batch = mb_lan::Batch { intents };
    let result = orders::apply_batch(&app, "dev_test", &staff, &may, &batch).expect("applied");

    assert_eq!(result.outcomes.len(), 100, "the report lost intents");
    assert!(
        result
            .outcomes
            .iter()
            .all(|(_, o)| matches!(o, Outcome::Ok { .. })),
        "some of the batch silently failed"
    );
    // A hundred additions and nothing for the kitchen: the counter has nothing to say but
    // "done" — never a count of "order changes".
    assert_eq!(result.says, "Done.");

    // Sending the WHOLE batch again changes nothing — idempotency across the batch, which is
    // what a phone retrying a half-answered batch does.
    let again = orders::apply_batch(&app, "dev_test", &staff, &may, &batch).expect("applied");
    assert_eq!(again.outcomes.len(), 100);

    let orders_now = app
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| Repos::new(tx).orders().find(&mb_core::OrderId::new(&order)))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("read")
        .expect("it is there");
    // One line of 100 coffees, not 200: the retry added nothing.
    let lines = orders_now.core().cart.lines();
    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0].qty.to_string(),
        "100",
        "the retry doubled the order"
    );
}

/// Stale intents are held, not applied.
#[test]
fn an_intent_from_last_night_waits_for_a_person() {
    let scratch = Scratch::new("p20_stale");
    let app = a_shop(&scratch, "stale");
    let order = open_one(&app, None);

    let mut old = intent(
        "yesterday",
        Some(&order),
        What::AddItem {
            item_id: "itm_dosa".to_owned(),
            qty: "1".to_owned(),
            note: None,
            modifiers: vec![],
        },
    );
    // Typed fourteen hours before it was sent — by the phone's own clock, which is the only
    // clock the rule reads.
    old.sent_at = Some(crate::flows::now().millis());
    old.at = crate::flows::now().millis() - (orders::HOLD_AFTER_HOURS + 2) * 60 * 60 * 1_000;

    let out = go(&app, &old);
    match &out {
        Outcome::Held { message, .. } => {
            assert!(message.contains("waiting for somebody"), "{message}");
            assert!(
                message.contains(&orders::HOLD_AFTER_HOURS.to_string()),
                "{message}"
            );
        }
        other => panic!("yesterday's order was applied silently: {other:?}"),
    }

    // Nothing was written — a held intent changes nothing at all.
    let found = app
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| Repos::new(tx).orders().find(&mb_core::OrderId::new(&order)))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("read")
        .expect("it is there");
    assert!(
        found.core().cart.is_empty(),
        "a held intent still changed the order"
    );

    let mut released = old.clone();
    released.id = "released".to_owned();
    released.at = crate::flows::now().millis();
    assert!(matches!(go(&app, &released), Outcome::Ok { .. }));
}

/// A phone whose staff member may not do something is refused SERVER-SIDE, even though the
/// phone would have hidden the button.
#[test]
fn a_permission_is_checked_on_the_counter() {
    let scratch = Scratch::new("p20_perm");
    let app = a_shop(&scratch, "perm");
    let order = open_one(&app, None);

    let mut only_billing = PermissionSet::new();
    only_billing.insert(Permission::BillCreate);
    let staff = StaffId::new("staff_default");

    let out = orders::apply(
        &app,
        "dev_test",
        &staff,
        &only_billing,
        &intent(
            "cancel-it",
            Some(&order),
            What::CancelOrder {
                reason: "walked out".to_owned(),
            },
        ),
    )
    .expect("answered")
    .outcome;

    let said = out.message();
    assert!(said.contains("do not have permission"), "{said}");
    assert!(said.contains("cancel the order"), "{said}");
    assert!(said.contains("Ask somebody"), "{said}");
}

/// Nothing here can block the billing screen.
#[test]
fn a_load_of_intents_does_not_slow_the_till() {
    let scratch = Scratch::new("p20_load");
    let app = Arc::new(a_shop(&scratch, "load"));
    let order = open_one(&app, None);

    let quiet = time_a_cart(&app);

    let mut threads = Vec::new();
    for t in 0..8 {
        let app = Arc::clone(&app);
        let order = order.clone();
        threads.push(std::thread::spawn(move || {
            for n in 0..25 {
                let _ = go(
                    &app,
                    &intent(
                        &format!("load-{t}-{n}"),
                        Some(&order),
                        What::AddItem {
                            item_id: "itm_coffee".to_owned(),
                            qty: "1".to_owned(),
                            note: None,
                            modifiers: vec![],
                        },
                    ),
                );
            }
        }));
    }
    let busy = time_a_cart(&app);
    for t in threads {
        let _ = t.join();
    }

    println!("\n--- T10: the till under a load of phone intents (P20) ---");
    println!("  reading the cashier's cart, network quiet: {quiet:?}");
    println!("  the same, with 200 intents in flight:      {busy:?}");

    // Generous, because this runs on a laptop that is also compiling.
    assert!(
        busy < quiet * 50 + std::time::Duration::from_millis(200),
        "the till took {busy:?} against {quiet:?} under load"
    );
}

/// What the cashier's screen does on every keystroke: read the cart.
fn time_a_cart(app: &App) -> std::time::Duration {
    let start = std::time::Instant::now();
    for _ in 0..200 {
        let _ = app.with_cart(|state| Ok(state.cart.len()));
    }
    start.elapsed()
}

/// The catalogue's version changes when a phone would SEE something different, and not
/// otherwise.
#[test]
fn the_catalogue_version_tracks_what_a_phone_can_see() {
    let scratch = Scratch::new("p20_cat");
    let app = a_shop(&scratch, "cat");

    let first = orders::catalogue(&app).expect("a catalogue");
    assert_eq!(first.items.len(), 2);
    assert!(first.items.iter().all(|i| i.is_available));
    assert!(!first.version.is_empty());

    // Asking again with nothing changed gives the SAME version, which is what lets a phone skip
    // the download.
    let again = orders::catalogue(&app).expect("a catalogue");
    assert_eq!(first.version, again.version);

    // Take a dish off the menu and the version moves.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Repos::new(tx).menu().set_available(
                    OUTLET,
                    &mb_core::ItemId::new("itm_dosa"),
                    false,
                    crate::flows::now(),
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("sold out");

    let after = orders::catalogue(&app).expect("a catalogue");
    assert_ne!(
        first.version, after.version,
        "a sold-out dish did not reach the phones"
    );
    assert!(
        after
            .items
            .iter()
            .any(|i| i.id == "itm_dosa" && !i.is_available),
        "the item is not marked sold out"
    );
}

/// The clock's edge, kept honest: the age of an intent is measured on the PHONE's clock alone
/// — typed-at against sent-at — so a phone that is hours wrong is still exactly right about
/// how long it held the order. A tablet with auto-time off once held every order it sent.
#[test]
fn a_wrong_phone_clock_is_not_staleness() {
    let counter_now = Timestamp::from_millis(10_000_000_000);
    // Sixteen hours BEHIND the counter, typed and sent five seconds apart: fresh.
    let behind = counter_now.millis() - 16 * 60 * 60 * 1_000;
    assert!(!orders::is_stale(behind, Some(behind + 5_000)));
    // Ahead of the counter: fresh.
    assert!(!orders::is_stale(counter_now.millis() + 60_000, Some(counter_now.millis() + 61_000)));
    // Really held overnight — fourteen hours between typing and sending, on the same clock.
    assert!(orders::is_stale(behind, Some(behind + 14 * 60 * 60 * 1_000)));
    // A phone that did not say when it sent is taken as sending now: never held by a clock
    // that is not its own.
    assert!(!orders::is_stale(behind, None));
}

/// The waiter's phone and the counter show one total: charges, tax and rounding included.
#[test]
fn the_phone_and_the_counter_agree_on_the_total() {
    let scratch = Scratch::new("p20_total");
    let app = a_shop(&scratch, "total");
    let mut config = app.shop_config();
    config.billing.packing_charge = mb_core::Money::from_paise(1_000);
    config.billing.packing_charge_tax = "tax_food_5".to_owned();
    app.publish_shop_config(config.clone());

    let order = open_one(&app, None);
    let out = go(
        &app,
        &intent(
            "add_dosa",
            Some(&order),
            What::AddItem {
                item_id: "itm_dosa".to_owned(),
                qty: "2".to_owned(),
                note: None,
                modifiers: vec![],
            },
        ),
    );
    let Outcome::Ok { total, .. } = out else {
        panic!("the dosa did not go on: {out:?}");
    };

    let stored = crate::flows::find_order(&app, &mb_core::OrderId::new(&order))
        .expect("read")
        .expect("on disk");
    let counter = crate::billing::bill_for(
        &stored.core().cart,
        stored.core().order_type(),
        None,
        &config,
    )
    .expect("the counter's bill");
    assert_eq!(total, counter.grand_total.to_plain_string());
    // 240 + 5% tax + 10 packing + 5% on it, rounded to the rupee.
    assert_eq!(total, "263.00");
}

/// Parking an order again keeps its time and its day: the floor's timer never restarts.
#[test]
fn parking_twice_keeps_the_time_and_the_day() {
    let scratch = Scratch::new("p20_park");
    let app = a_shop(&scratch, "park");
    app.with_cart_mut(|state| {
        state.place_on(mb_core::TableId::new("tbl_7"), "7".to_owned(), None);
        Ok(())
    })
    .expect("a table");
    crate::ipc::cart_add_on(&app, "itm_dosa".to_owned(), None, None).expect("a dosa");
    let first = crate::flows::park_open_order(&app).expect("parked");

    std::thread::sleep(std::time::Duration::from_millis(5));
    crate::ipc::cart_add_on(&app, "itm_coffee".to_owned(), None, None).expect("a coffee");
    let second = crate::flows::park_open_order(&app).expect("parked again");

    assert_eq!(
        second.core.id, first.core.id,
        "a second park is the same order"
    );
    assert_eq!(
        second.core.created_at, first.core.created_at,
        "the time moved"
    );
    assert_eq!(
        second.core.business_day, first.core.business_day,
        "the day moved"
    );
    assert_eq!(second.bill_number, first.bill_number, "the number moved");
    assert_eq!(second.core.cart.lines().len(), 2, "the cart did not follow");
    assert!(
        crate::flows::now().millis() > first.core.created_at.millis(),
        "the clock stood still, so the test proved nothing"
    );
}

/// A parcel cannot be on a table, and a dine-in cart with no table cannot become an order.
#[test]
fn a_table_and_a_parcel_cannot_meet() {
    let mut state = crate::billing::CartState::default();
    let by = StaffId::new("staff_default");
    let refused = state
        .to_core(crate::flows::now(), &by, "t1")
        .expect_err("no table");
    assert_eq!(refused.code, "bill.no_table");

    state.place_on(mb_core::TableId::new("tbl_7"), "7".to_owned(), None);
    state.set_order_type(mb_core::OrderType::Parcel);
    assert!(state.table().is_none(), "a parcel kept its table");
    let core = state
        .to_core(crate::flows::now(), &by, "t1")
        .expect("a parcel");
    assert_eq!(core.placement, mb_core::Placement::Parcel);

    state.place_on(mb_core::TableId::new("tbl_7"), "7".to_owned(), None);
    assert_eq!(
        state.order_type(),
        mb_core::OrderType::DineIn,
        "a table makes it dine-in"
    );
}

/// The floor as the phones are told it: a taken table names its order, an open order carries
/// the same lines and total an outcome would, and a settled one is simply not there.
#[test]
fn the_floor_body_says_what_every_phone_needs() {
    let scratch = Scratch::new("floor_body");
    let app = a_shop(&scratch, "floor_body");

    let empty = orders::floor_body(&app).expect("a floor");
    assert_eq!(empty["tables"][0]["id"], "tbl_7");
    assert_eq!(empty["tables"][0]["state"], "free");
    assert!(empty["orders"].as_array().expect("orders").is_empty());

    let order = open_one(&app, Some("tbl_7"));
    go(
        &app,
        &intent(
            "add_1",
            Some(&order),
            What::AddItem {
                item_id: "itm_dosa".to_owned(),
                qty: "2".to_owned(),
                note: None,
                modifiers: vec![],
            },
        ),
    );

    let busy = orders::floor_body(&app).expect("a floor");
    assert_eq!(busy["tables"][0]["state"], "taken");
    assert_eq!(busy["tables"][0]["order_id"], order);
    let open = &busy["orders"][0];
    assert_eq!(open["order_id"], order);
    assert_eq!(open["table_id"], "tbl_7");
    assert_eq!(open["table_label"], "Main hall 7");
    assert_eq!(open["order_type"], "dine_in");
    assert_eq!(open["lines"][0]["name"], "Masala Dosa");
    assert_eq!(open["lines"][0]["qty"], "2");
    assert!(
        open["total"].as_str().is_some_and(|t| t != "0.00"),
        "the total is the counter's figure, not a placeholder"
    );
    assert!(open["token"].is_string(), "an open order has its token");
}

/// A whole order in ONE batch: open the table, add the dishes, send to the kitchen — the phone
/// cannot know the order id the batch itself creates, so the counter carries it forward. This
/// is what makes tapping "Send to kitchen" one round trip, and what stops the 0.00 ghost
/// orders a tap-to-open design left on the floor.
#[test]
fn one_batch_opens_fills_and_fires_a_whole_order() {
    let scratch = Scratch::new("batch_whole");
    let app = a_shop(&scratch, "batch_whole");
    let (staff, may) = waiter();

    let batch = mb_lan::Batch {
        intents: vec![
            intent(
                "b_open",
                None,
                What::OpenOrder {
                    order_type: "dine_in".to_owned(),
                    table_id: Some("tbl_7".to_owned()),
                    covers: None,
                },
            ),
            intent(
                "b_add1",
                None,
                What::AddItem {
                    item_id: "itm_dosa".to_owned(),
                    qty: "2".to_owned(),
                    note: None,
                    modifiers: vec![],
                },
            ),
            intent(
                "b_add2",
                None,
                What::AddItem {
                    item_id: "itm_coffee".to_owned(),
                    qty: "1".to_owned(),
                    note: None,
                    modifiers: vec![],
                },
            ),
            intent("b_fire", None, What::SendToKitchen),
        ],
    };
    let result = orders::apply_batch(&app, "dev_test", &staff, &may, &batch).expect("applied");
    for (id, outcome) in &result.outcomes {
        assert!(matches!(outcome, Outcome::Ok { .. }), "{id} was not ok: {outcome:?}");
    }
    let Outcome::Ok { order_id, total, lines, .. } = &result.outcomes[3].1 else {
        panic!("no final view");
    };
    assert_eq!(lines.len(), 2, "both dishes are on the order");
    assert!(lines.iter().all(|l| l.sent_to_kitchen), "the kitchen has everything");
    assert_ne!(total, "0.00", "the total is real, not a ghost");

    // The replay of the WHOLE batch (a phone that lost the reply) makes nothing new.
    let again = orders::apply_batch(&app, "dev_test", &staff, &may, &batch).expect("replayed");
    let Outcome::Ok { order_id: same, .. } = &again.outcomes[0].1 else {
        panic!("no replayed open");
    };
    assert_eq!(order_id, same, "the replayed batch reuses the same order");
    let Outcome::Ok { lines: after, .. } = &again.outcomes[3].1 else {
        panic!("no replayed view");
    };
    assert_eq!(after.len(), 2, "the replay added nothing twice");
}

#[test]
fn a_phone_sending_to_the_kitchen_queues_the_same_ticket() {
    let scratch = Scratch::new("phone_kot");
    let app = a_shop(&scratch, "phone_kot");
    let (staff, may) = waiter();


    let jobs = |app: &App| -> i64 {
        app.with_shop(|shop| {
            shop.db
                .transaction(|tx| Repos::new(tx).print_jobs().count(OUTLET))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("counted")
    };
    assert_eq!(jobs(&app), 0, "nothing queued before the order");

    let batch = mb_lan::Batch {
        intents: vec![
            intent(
                "p_open",
                None,
                What::OpenOrder {
                    order_type: "dine_in".to_owned(),
                    table_id: Some("tbl_7".to_owned()),
                    covers: None,
                },
            ),
            intent(
                "p_add",
                None,
                What::AddItem {
                    item_id: "itm_dosa".to_owned(),
                    qty: "2".to_owned(),
                    note: None,
                    modifiers: vec![],
                },
            ),
            intent("p_fire", None, What::SendToKitchen),
        ],
    };
    let result = orders::apply_batch(&app, "dev_test", &staff, &may, &batch).expect("applied");
    for (id, outcome) in &result.outcomes {
        assert!(matches!(outcome, Outcome::Ok { .. }), "{id} was not ok: {outcome:?}");
    }
    let after = jobs(&app);
    assert!(after >= 1, "the kitchen ticket is on the print queue, got {after}");

    // The replay (a phone that lost the reply) prints nothing twice.
    orders::apply_batch(&app, "dev_test", &staff, &may, &batch).expect("replayed");
    assert_eq!(jobs(&app), after, "a replayed batch queues no second ticket");
}

/// A phone order is audited once, at the moments that matter: the taps that build it are not
/// history (their exactly-once record is `applied_events`), the void and the settle are.
#[test]
fn a_phone_order_is_audited_once_not_per_tap() {
    let scratch = Scratch::new("audit_once");
    let app = a_shop(&scratch, "audit_once");
    let before = audit_rows(&app);
    let order = open_one(&app, None);
    for n in 0..10 {
        let _ = go(
            &app,
            &intent(
                &format!("tap-{n}"),
                Some(&order),
                What::AddItem {
                    item_id: "itm_coffee".to_owned(),
                    qty: "1".to_owned(),
                    note: None,
                    modifiers: vec![],
                },
            ),
        );
    }
    let _ = go(&app, &intent("qty", Some(&order), What::SetQty { line: 0, qty: "3".to_owned() }));
    assert_eq!(audit_rows(&app) - before, 0, "building the order wrote history rows");
    let _ = go(
        &app,
        &intent("void", Some(&order), What::VoidItem { line: 0, reason: "wrong table".to_owned() }),
    );
    assert_eq!(audit_rows(&app) - before, 1, "a void is one history row");
    let _ = go(&app, &intent("bill", Some(&order), What::RequestBill));
    assert_eq!(audit_rows(&app) - before, 2, "asking for the bill is one history row");
    assert!(orders::is_audited(&What::RequestSettle { payment: None }));
    assert!(orders::is_audited(&What::CancelOrder { reason: String::new() }));
    assert!(!orders::is_audited(&What::SendToKitchen));
    assert!(!orders::is_audited(&What::SetCovers { covers: Some(2) }));
}

fn two_sent_dosas(app: &App) -> (String, mb_lan::intent::LineView) {
    let order = open_one(app, Some("tbl_7"));
    go(app, &intent("reduction-add", Some(&order), What::AddItem {
        item_id: "itm_dosa".into(), qty: "2".into(), note: Some("No onion".into()), modifiers: vec![],
    }));
    let Outcome::Ok { lines, .. } = go(app, &intent("reduction-send", Some(&order), What::SendToKitchen)) else {
        panic!("the kitchen was not told");
    };
    (order, lines[0].clone())
}

#[test]
fn phone_partial_cancellation_keeps_one_and_tells_the_kitchen_to_stop_only_one_once() {
    let scratch = Scratch::new("phone_reduce");
    let app = a_shop(&scratch, "reduce");
    let (order, expected) = two_sent_dosas(&app);
    let (staff, may) = waiter();
    let request = intent("reduce-one", Some(&order), What::ReduceQty {
        expected, qty: "1".into(), reason: "Customer changed mind".into(),
    });
    let before_audit = audit_rows(&app);
    let applied = orders::apply(&app, "dev_test", &staff, &may, &request).expect("reduced");
    let Outcome::Ok { lines, total, .. } = &applied.outcome else { panic!("{:?}", applied.outcome) };
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].qty, "1");
    assert_eq!(lines[0].in_kitchen, "1");
    assert!(lines[0].sent_to_kitchen);
    assert_eq!(total, "126.00");
    let paper = applied.kitchen_paper.expect("a cancellation KOT");
    assert_eq!(paper.kind, mb_print::template::TicketKind::Cancellation);
    assert_eq!(paper.lines.len(), 1);
    assert_eq!(paper.lines[0].1.qty.to_string(), "1");
    assert_eq!(paper.lines[0].1.name, "Masala Dosa");
    assert_eq!(paper.lines[0].1.note.as_deref(), Some("No onion"));
    let replay = orders::apply(&app, "dev_test", &staff, &may, &request).expect("replay");
    assert_eq!(replay.outcome, applied.outcome);
    assert!(replay.kitchen_paper.is_none());
    assert_eq!(audit_rows(&app), before_audit + 1);
    let resent = go(&app, &intent("reduction-resend", Some(&order), What::SendToKitchen));
    assert!(resent.message().contains("already has everything"));
    assert_eq!(orders::floor_body(&app).expect("floor")["partial_item_cancellation"], true);
    let audit = app.with_shop(|shop| shop.db.read_transaction(|tx| {
        Repos::new(tx).audit().list(OUTLET, &mb_db::repo::AuditFilter {
            action: Some(mb_auth::audit::action::ITEM_VOIDED.into()), limit: 10, ..Default::default()
        })
    }).map_err(|e| crate::words::from_db(&e))).expect("audit");
    assert_eq!(audit.len(), 1);
    let saved = serde_json::to_string(&audit[0]).expect("audit JSON");
    assert!(saved.contains("Customer changed mind"));
}

#[test]
fn phone_partial_cancellation_validates_permission_reason_target_and_original_line() {
    let scratch = Scratch::new("phone_reduce_guards");
    let app = a_shop(&scratch, "guards");
    let (order, expected) = two_sent_dosas(&app);
    let (staff, _) = waiter();
    let mut may = PermissionSet::new();
    may.insert(Permission::BillCreate);
    let what = What::ReduceQty { expected: expected.clone(), qty: "1".into(), reason: "Changed mind".into() };
    let denied = orders::apply(&app, "dev_test", &staff, &may, &intent("denied", Some(&order), what)).expect("answer");
    assert!(matches!(denied.outcome, Outcome::Refused { .. }));
    assert!(denied.kitchen_paper.is_none());
    for (id, qty, reason) in [("blank", "1", " "), ("zero", "0", "Reason"), ("half", "0.5", "Reason"), ("fraction", "1.5", "Reason"), ("raise", "3", "Reason"), ("same", "2", "Reason"), ("invalid", "bad", "Reason")] {
        let result = go(&app, &intent(id, Some(&order), What::ReduceQty {
            expected: expected.clone(), qty: qty.into(), reason: reason.into(),
        }));
        assert!(matches!(result, Outcome::Refused { .. }), "{id}: {result:?}");
    }
    // Another waiter changes this line while the phone holds its old snapshot.
    go(&app, &intent("other-waiter", Some(&order), What::SetQty { line: 0, qty: "3".into() }));
    let stale = go(&app, &intent("stale", Some(&order), What::ReduceQty {
        expected, qty: "1".into(), reason: "Changed mind".into(),
    }));
    assert!(stale.message().contains("changed at the counter"));
    let floor = orders::floor_body(&app).expect("floor");
    assert_eq!(floor["orders"][0]["lines"][0]["qty"], "3");
    assert_eq!(floor["orders"][0]["lines"][0]["in_kitchen"], "2");
}

#[test]
fn phone_partial_cancellation_of_a_partly_sent_line_cancels_only_the_sent_excess() {
    let scratch = Scratch::new("phone_reduce_partial");
    let app = a_shop(&scratch, "partial");
    let (order, _) = two_sent_dosas(&app);
    let Outcome::Ok { lines, .. } = go(&app, &intent("raise", Some(&order), What::SetQty { line: 0, qty: "3".into() })) else { panic!("raised") };
    let (staff, may) = waiter();
    let applied = orders::apply(&app, "dev_test", &staff, &may, &intent("reduce", Some(&order), What::ReduceQty {
        expected: lines[0].clone(), qty: "1".into(), reason: "Wrong quantity".into(),
    })).expect("reduced");
    assert_eq!(applied.kitchen_paper.expect("stop slip").lines[0].1.qty.to_string(), "1");
    let Outcome::Ok { lines, .. } = applied.outcome else { panic!("reduced") };
    assert_eq!(lines[0].qty, "1");
    assert_eq!(lines[0].in_kitchen, "1");
}

#[test]
fn phone_parties_have_distinct_orders_and_labels_and_retry_is_idempotent() {
    let scratch = Scratch::new("phone_parties");
    let app = a_shop(&scratch, "parties");
    let first = open_one(&app, Some("tbl_7"));
    let request = intent("party_b", None, What::OpenParty { table_id: "tbl_7".into(), covers: None });
    let party = go(&app, &request);
    assert_eq!(party, go(&app, &request));
    let floor = orders::floor_body(&app).expect("floor");
    let rows = floor["orders"].as_array().expect("orders");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|r| r["order_id"] == first && r["table_label"] == "Main hall 7"));
    assert!(rows.iter().any(|r| r["seat"] == "B" && r["table_label"] == "Main hall 7B"));
    assert_eq!(floor["tables_complete"], true);
}

#[test]
fn phone_table_snapshot_and_version_follow_details_hiding_and_deletion() {
    let scratch = Scratch::new("phone_tables");
    let app = a_shop(&scratch, "tables");
    let mut version = orders::catalogue(&app).expect("catalogue").version;
    for sql in [
        "UPDATE dining_tables SET seats = 8",
        "UPDATE sections SET name = 'Garden'",
        "UPDATE dining_tables SET section_id = NULL",
        "UPDATE dining_tables SET is_active = 0",
    ] {
        app.with_shop(|shop| shop.db.transaction(|tx| { tx.execute(sql, [])?; Ok(()) }).map_err(|e| crate::words::from_db(&e))).expect("edit");
        let next = orders::catalogue(&app).expect("catalogue");
        assert_ne!(version, next.version, "{sql}");
        version = next.version;
    }
    assert!(orders::catalogue(&app).expect("catalogue").tables.is_empty());
    assert!(orders::floor_body(&app).expect("floor")["tables"].as_array().expect("tables").is_empty());
    assert!(matches!(go(&app, &intent("hidden", None, What::OpenParty { table_id: "tbl_7".into(), covers: None })), Outcome::Refused { .. }));
    app.with_shop(|shop| shop.db.transaction(|tx| {
        tx.execute("UPDATE dining_tables SET is_active = 1", [])?;
        Ok(())
    }).map_err(|e| crate::words::from_db(&e))).expect("restore");
    let before_delete = orders::catalogue(&app).expect("catalogue");
    app.with_shop(|shop| shop.db.transaction(|tx| Repos::new(tx).floor().delete_table(OUTLET, &mb_core::TableId::new("tbl_7"), crate::flows::now())).map_err(|e| crate::words::from_db(&e))).expect("delete");
    let deleted = orders::catalogue(&app).expect("catalogue");
    assert_ne!(before_delete.version, deleted.version);
    assert!(deleted.tables.is_empty());
}

#[test]
fn a_failed_open_never_routes_its_dishes_to_the_previous_order() {
    for explicit in [false, true] {
        let scratch = Scratch::new(if explicit { "phone_dependency" } else { "phone_legacy_batch" });
        let app = a_shop(&scratch, "batch");
        let (staff, may) = waiter();
        let open = |id: &str, table: &str| intent(id, None, What::OpenOrder { order_type: "dine_in".into(), table_id: Some(table.into()), covers: None });
        let mut dish = intent("dish_bad", None, What::AddItem { item_id: "itm_dosa".into(), qty: "1".into(), note: None, modifiers: vec![] });
        if explicit { dish.open_intent_id = Some("open_bad".into()); }
        let result = orders::apply_batch(&app, "dev_test", &staff, &may, &mb_lan::Batch {
            intents: vec![open("open_good", "tbl_7"), open("open_bad", "deleted"), dish],
        }).expect("batch");
        assert!(matches!(result.outcomes[1].1, Outcome::Refused { .. }));
        assert!(matches!(result.outcomes[2].1, Outcome::Refused { .. }));
        let floor = orders::floor_body(&app).expect("floor");
        assert!(floor["orders"][0]["lines"].as_array().expect("lines").is_empty());
    }
}

#[test]
fn a_dish_keeps_its_open_dependency_across_batches_and_replays() {
    let scratch = Scratch::new("phone_dependency_replay");
    let app = a_shop(&scratch, "retry");
    let opening = intent("opening", None, What::OpenOrder { order_type: "parcel".into(), table_id: None, covers: None });
    let opened = go(&app, &opening);
    let mut dish = intent("dish", None, What::AddItem { item_id: "itm_dosa".into(), qty: "1".into(), note: None, modifiers: vec![] });
    dish.open_intent_id = Some("opening".into());
    let added = go(&app, &dish);
    assert!(matches!((&opened, &added), (Outcome::Ok { order_id: a, .. }, Outcome::Ok { order_id: b, .. }) if a == b));
    assert_eq!(added, go(&app, &dish));
}

#[test]
fn phones_cannot_change_either_side_of_a_combined_bill() {
    let scratch = Scratch::new("lan_combined");
    let app = a_shop(&scratch, "combined");
    let source = open_one(&app, None);
    let root = open_one(&app, None);
    app.with_shop(|shop| shop.db.transaction(|tx| {
        let repos = Repos::new(tx);
        let mut order = repos.orders().find(&mb_core::OrderId::new(&source))?.expect("source");
        order.core_mut().billing.billed_into = Some(mb_core::OrderId::new(&root));
        repos.orders().save(OUTLET, app.terminal_id(), &order)?;
        let mut order = repos.orders().find(&mb_core::OrderId::new(&root))?.expect("root");
        order.core_mut().billing.sources.push(mb_core::OrderId::new(&source));
        repos.orders().save(OUTLET, app.terminal_id(), &order)
    }).map_err(|error| crate::words::from_db(&error))).expect("linked");
    for (id, order) in [("change_source", source), ("change_root", root)] {
        let outcome = go(&app, &intent(id, Some(&order), What::SetOrderNote { note: Some("phone overwrite".to_owned()) }));
        assert!(matches!(outcome, Outcome::Refused { .. }), "{outcome:?}");
        app.with_shop(|shop| shop.db.transaction(|tx| {
            let saved = Repos::new(tx).orders().find(&mb_core::OrderId::new(&order))?.expect("order");
            assert!(saved.core().note.is_none());
            Ok(())
        }).map_err(|error| crate::words::from_db(&error))).expect("unchanged");
    }
}

#[test]
fn phones_leave_an_issued_bill_and_its_correction_draft_unchanged() {
    let scratch = Scratch::new("lan_correction");
    let app = a_shop(&scratch, "correction");
    crate::signin_tests::hire(&app, "staff_owner", "Owner", mb_auth::RolePreset::Owner, "2468");
    let id = open_one(&app, None);
    let added = go(&app, &intent("draft_food", Some(&id), What::AddItem {
        item_id: "itm_dosa".to_owned(), qty: "1".to_owned(), note: None, modifiers: vec![],
    }));
    assert!(matches!(added, Outcome::Ok { .. }));
    crate::ipc::open_order_on(&app, id.clone()).expect("load");
    crate::flows::complete_bill_on(&app, Some("cash".to_owned())).expect("paid");
    crate::corrections::revert_bill_on(&app, id.clone(), "Correct items".to_owned(), None, None).expect("draft");
    let outcome = go(&app, &intent("phone_during_edit", Some(&id), What::SetOrderNote { note: Some("overwrite".to_owned()) }));
    assert!(matches!(outcome, Outcome::Refused { .. }));
    app.with_shop(|shop| shop.db.transaction(|tx| {
        let repos = Repos::new(tx);
        let id = mb_core::OrderId::new(&id);
        let issued = repos.orders().find(&id)?.expect("issued");
        let draft = repos.orders().find_working(&id)?.expect("draft");
        assert!(matches!(issued, mb_core::AnyOrder::Settled(_)));
        assert!(matches!(draft, mb_core::AnyOrder::Open(_)));
        assert!(issued.core().note.is_none());
        assert!(draft.core().note.is_none());
        Ok(())
    }).map_err(|error| crate::words::from_db(&error))).expect("unchanged");
}

fn audit_rows(app: &App) -> i64 {
    app.shop_db()
        .expect("a shop")
        .read(|c| Ok(c.query_row("SELECT count(*) FROM audit_log WHERE action = 'intent.applied'", [], |r| r.get(0))?))
        .expect("count")
}
