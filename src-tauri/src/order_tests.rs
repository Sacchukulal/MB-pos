//! **P20's T1–T10, against the real applier and a real database.**
//!
//! Not against a mock: the whole point of this session is that the COUNTER is
//! the authority, so a test that stubs the counter is testing nothing. These
//! drive `orders::apply` with a real SQLite file, the real order lifecycle and
//! the real kitchen ledger.

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
                    tax_rate: mb_core::TaxRate::GST_5,
                    tax_treatment: mb_core::TaxTreatment::Exclusive,
                    tax_class_id: None,
                    hsn: None,
                    cost_price: None,
                    short_code: None,
                    prep_minutes: None,
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
    app
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
        at: crate::flows::now().millis(),
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
                order_type: "parcel".to_owned(),
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

/// **T1 — idempotency, hard.** The same intent fifty times, concurrently.
/// Exactly one order exists and all fifty replies are identical.
///
/// This is the single most important property in the protocol: a waiter on a
/// flaky connection retries, and a phone that lost a reply cannot know whether
/// its intent landed.
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

/// **T5 — money is never computed on the phone.**
#[test]
fn the_counters_figure_wins_over_anything_a_phone_sends() {
    let scratch = Scratch::new("p20_money");
    let app = a_shop(&scratch, "money");
    let order = open_one(&app, None);

    // A phone one version ahead sends a total. The type has nowhere to put it,
    // so it arrives as an unknown field and is IGNORED rather than rejected —
    // refusing would break a floor full of phones on the older app.
    let lying: Intent = serde_json::from_str(&format!(
        r#"{{"id":"lie","order_id":"{order}","at":{},
             "what":{{"do":"add_item","item_id":"itm_dosa","qty":"2",
                      "note":null,"modifiers":[],"total":"1.00","price":"0.50"}}}}"#,
        crate::flows::now().millis()
    ))
    .expect("a lying phone still parses");

    match go(&app, &lying) {
        Outcome::Ok { total, lines, .. } => {
            // Two dosas at 120.00 is 240.00, from the counter's own menu.
            assert_eq!(total, "240.00", "the phone's number reached the bill");
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].amount, "240.00");
        }
        other => panic!("it refused a perfectly good intent: {other:?}"),
    }
}

/// **T4 — the kitchen delta, and the retry that must not re-send.**
///
/// Crown jewel 2: only the counter decides what is new. A phone that computed
/// its own delta would double-print on a retry.
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

    // **The same intent again** — the retry a flaky connection produces. It
    // must return the ORIGINAL answer and print nothing.
    let retry = go(&app, &intent("fire1", Some(&order), What::SendToKitchen));
    assert_eq!(retry, first, "a retry printed a second ticket");

    // A DIFFERENT send, with nothing new, tells the truth rather than
    // reprinting.
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

/// **T2 — the conflicts, each as documented.**
#[test]
fn every_conflict_resolves_the_way_the_protocol_says() {
    let scratch = Scratch::new("p20_conflict");
    let app = a_shop(&scratch, "conflict");

    // (a) two waiters open the same table at once: the second JOINS the first,
    //     because two waiters at one table are serving one party.
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

    // (c) voiding a line the kitchen has already made goes to the counter.
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
        said.contains("kitchen has already made this"),
        "a cooked dish was thrown away from the floor: {said}"
    );
    assert!(said.contains("counter"), "it did not say what to do: {said}");

    // And reducing the quantity below what was cooked is refused for the same
    // reason, in words with the number in them.
    let shrunk = go(
        &app,
        &intent(
            "c-qty",
            Some(&first),
            What::SetQty {
                line: 0,
                qty: "0".to_owned(),
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

    // **A table this shop does not have.** Found by running the tests: it
    // reached the waiter as "The shop's data could not be read" — a foreign
    // key violation in a sentence meant for a support engineer, on a phone,
    // with a customer waiting (audit F8).
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
    assert!(said.contains("refresh"), "it did not say what to do: {said}");

    // A void with no reason is refused — the same rule the counter's own
    // screen has had since P03.
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

/// **T3 — the cashier is never clobbered.**
#[test]
fn a_phone_cannot_wipe_what_the_cashier_is_typing() {
    let scratch = Scratch::new("p20_cashier");
    let app = a_shop(&scratch, "cashier");
    let order = open_one(&app, None);

    // The cashier has that order open and has typed a payment into it.
    app.with_cart_mut(|state| {
        state.order_id = Some(order.clone());
        state.settlement.add(
            mb_core::Payment::new(mb_core::PaymentMode::Cash, mb_core::Money::from_paise(50_000))
                .expect("a payment"),
        )
        .map_err(|e| {
            crate::words::UiError::new("bill.pay", "no").with_detail(e.to_string())
        })?;
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

    // The counter took the change — it is the authority — and the cashier is
    // TOLD rather than overwritten.
    assert!(matches!(applied.outcome, Outcome::Ok { .. }));
    let change = applied
        .tell_the_cashier
        .expect("the cashier was not told the floor had touched their order");
    assert!(change.says.contains("Masala Dosa"), "{}", change.says);
    assert!(change.says.contains('2'), "{}", change.says);

    // **The cashier's payment is untouched.** That is the rule.
    app.with_cart(|state| {
        assert_eq!(state.settlement.payments().len(), 1);
        assert_eq!(state.settlement.total_paid().expect("a total").paise(), 50_000);
        Ok(())
    })
    .expect("the cart survived");
}

/// **T6 — a batch of offline intents, in order, with a per-intent report.**
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
        result.outcomes.iter().all(|(_, o)| matches!(o, Outcome::Ok { .. })),
        "some of the batch silently failed"
    );
    assert!(result.says.contains("100"), "{}", result.says);

    // Sending the WHOLE batch again changes nothing — idempotency across the
    // batch, which is what a phone retrying a half-answered batch does.
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
    assert_eq!(lines[0].qty.to_string(), "100", "the retry doubled the order");
}

/// **T7 — stale intents are held, not applied.**
///
/// v1 printed yesterday's tickets at 7 a.m. into a kitchen making breakfast.
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
    old.at = crate::flows::now().millis() - (orders::HOLD_AFTER_HOURS + 2) * 60 * 60 * 1_000;

    let out = go(&app, &old);
    match &out {
        Outcome::Held { message, .. } => {
            assert!(message.contains("waiting for somebody"), "{message}");
            assert!(message.contains(&orders::HOLD_AFTER_HOURS.to_string()), "{message}");
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

    // Released — the phone re-sends it with a fresh timestamp and a NEW id,
    // because the held one is a different decision.
    let mut released = old.clone();
    released.id = "released".to_owned();
    released.at = crate::flows::now().millis();
    assert!(matches!(go(&app, &released), Outcome::Ok { .. }));
}

/// A phone whose staff member may not do something is refused SERVER-SIDE,
/// even though the phone would have hidden the button. D45, for the floor.
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

/// **T10 — nothing here can block the billing screen.**
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

    // Generous, because this runs on a laptop that is also compiling. What it
    // guards against is an order-of-magnitude regression — an intent holding a
    // lock the billing path needs — not scheduler noise.
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

/// The catalogue's version changes when a phone would SEE something different,
/// and not otherwise.
#[test]
fn the_catalogue_version_tracks_what_a_phone_can_see() {
    let scratch = Scratch::new("p20_cat");
    let app = a_shop(&scratch, "cat");

    let first = orders::catalogue(&app).expect("a catalogue");
    assert_eq!(first.items.len(), 2);
    assert!(first.items.iter().all(|i| i.is_available));
    assert!(!first.version.is_empty());

    // Asking again with nothing changed gives the SAME version, which is what
    // lets a phone skip the download.
    let again = orders::catalogue(&app).expect("a catalogue");
    assert_eq!(first.version, again.version);

    // Take a dish off the menu and the version moves — scope 3.9, because a
    // waiter selling a dish that ran out twenty minutes ago is the complaint
    // this exists to prevent.
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
        after.items.iter().any(|i| i.id == "itm_dosa" && !i.is_available),
        "the item is not marked sold out"
    );
}

/// The clock's edge, kept honest: `Timestamp` is milliseconds and a phone's
/// clock can be ahead of the counter's.
#[test]
fn a_phone_clock_running_fast_is_not_stale() {
    let now = Timestamp::from_millis(10_000_000_000);
    assert!(!orders::is_stale(now.millis() + 60_000, now));
}
