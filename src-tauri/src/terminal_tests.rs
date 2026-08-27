//! Two tills in one shop, driven end to end.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests: expect is the assertion"
)]

use std::collections::BTreeSet;

use mb_auth::{Permission, PermissionSet};
use mb_core::{AnyOrder, Money, OrderType, Payment, PaymentMode, StaffId};
use mb_db::repo::terminals::Terminal;
use mb_db::{Db, DbConfig, Repos};
use mb_lan::intent::{Intent, Outcome, What};

use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

// A shop, and a till in it.

/// One menu item, so every till in a test sells the same thing for the same money — which is
/// what makes "the two books agree" mean something.
fn seed_menu(db: &Db) {
    db.transaction(|tx| {
        Repos::new(tx).menu().save_item(
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
}

/// A till: its own database, its own identity and its own series.
fn a_till(scratch: &Scratch, file: &str, id: &str, name: &str, prefix: &str) -> App {
    let path = scratch.dir().join(format!("{file}.db"));
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    seed_menu(&db);
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let mut row = Terminal::new(id, name, crate::flows::now());
        row.series_prefix = prefix.to_owned();
        repos.terminals().save(OUTLET, &row, crate::flows::now())
    })
    .expect("the till is on file");

    let app = App::new(crate::config::AppConfig::default())
        .expect("the font loads")
        .becoming_till(id);
    app.open_shop(db, path);
    app
}

/// One cash sale through the real path, returning the bill number it printed.
fn a_cash_sale(app: &App) -> String {
    app.with_cart_mut(|state| {
        state.order_type = OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(app, "itm_dosa".to_owned(), Some("1".to_owned()), None).expect("added");
    let total = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    app.with_cart_mut(|state| {
        let payment = Payment::new(PaymentMode::Cash, total).expect("a cash payment");
        state.settlement.add(payment).map_err(|e| {
            crate::words::UiError::new("bill.pay", "That payment could not be taken.")
                .with_detail(e.to_string())
        })
    })
    .expect("paid");
    crate::flows::complete_bill_on(app).expect("settled")
}

/// Every bill number in a till's book, in the order they were written.
fn numbers_in(app: &App) -> Vec<String> {
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                Ok(Repos::new(tx)
                    .orders()
                    .list_all()?
                    .iter()
                    .filter_map(|o| match o {
                        AnyOrder::Settled(s) => Some(s.bill_number.formatted.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("the book reads")
}

/// A table to sit at.
fn a_table(id: &str, label: &str) -> mb_db::repo::floor::DiningTable {
    mb_db::repo::floor::DiningTable {
        id: mb_core::TableId::new(id),
        section_id: None,
        label: label.to_owned(),
        seats: 4,
        pos: None,
        sort_order: 0,
        is_active: true,
    }
}

/// Somebody at a counter who may bill.
fn a_cashier() -> (StaffId, PermissionSet) {
    let mut may = PermissionSet::new();
    may.insert(Permission::BillCreate);
    may.insert(Permission::OrderItemVoid);
    may.insert(Permission::OrderCancel);
    (StaffId::new(crate::state::DEFAULT_STAFF), may)
}

/// Take the money for an open order exactly where it stands.
fn settle_where_it_stands(app: &App, order_id: &str, staff: &StaffId) {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                let Some(AnyOrder::Open(open)) =
                    repos.orders().find(&mb_core::OrderId::new(order_id))?
                else {
                    panic!("that order is not open");
                };
                let bill = mb_core::compute_bill(mb_core::BillInput::new(
                    &open.core.cart,
                    mb_core::Registration::Regular,
                ))
                .map_err(|e: mb_core::BillError| mb_db::DbError::invariant(e.to_string()))?;
                let mut settlement = mb_core::Settlement::default();
                settlement
                    .add(Payment::new(PaymentMode::Cash, bill.grand_total).expect("cash"))
                    .expect("paid");
                let done = open
                    .settle(bill, settlement, crate::flows::now(), staff.clone())
                    .map_err(|e: mb_core::OrderError| mb_db::DbError::invariant(e.to_string()))?;
                repos.orders().save(OUTLET, "t_a", &AnyOrder::Settled(done))
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("settled at the other counter");
}

/// Carry whatever a till is holding across to the master, as the wire would — including the
/// sender clearing its queue only on a confirmed apply.
fn hand_over(from: &App, to: &App) -> usize {
    let waiting = crate::forwarding::waiting_on(from).expect("the queue reads");
    if waiting.is_empty() {
        return 0;
    }
    let batch = crate::forwarding::from_here(from, waiting).expect("it describes itself");
    let receipt = crate::forwarding::receive_on(to, &batch).expect("the master takes it");
    crate::forwarding::confirmed_on(from, &receipt).expect("the queue clears");
    receipt.stored.iter().filter(|(_, ok)| *ok).count()
}

// No number twice.

#[test]
fn no_number_is_ever_issued_twice_even_while_a_third_till_joins() {
    let scratch = Scratch::new("term_numbers");
    let path = scratch.dir().join("shop.db");
    let db = std::sync::Arc::new(Db::open(&DbConfig::new(path)).expect("open"));
    let day = crate::flows::today(crate::flows::now());

    for (id, name, prefix) in [("t_a", "Counter 1", "A/"), ("t_b", "Counter 2", "B/")] {
        db.transaction(|tx| {
            let mut row = Terminal::new(id, name, crate::flows::now());
            row.series_prefix = prefix.to_owned();
            Repos::new(tx)
                .terminals()
                .save(OUTLET, &row, crate::flows::now())
        })
        .expect("a till");
    }

    const EACH: usize = 250;
    let mut threads = Vec::new();
    for id in ["t_a", "t_b"] {
        let db = std::sync::Arc::clone(&db);
        threads.push(std::thread::spawn(move || {
            let mut mine = Vec::new();
            for _ in 0..EACH {
                let claimed = db
                    .transaction(|tx| {
                        let bill = mb_db::numbering::claim(
                            tx,
                            OUTLET,
                            id,
                            mb_db::numbering::CounterKind::Bill,
                            day,
                        )?;
                        let token = mb_db::numbering::claim(
                            tx,
                            OUTLET,
                            id,
                            mb_db::numbering::CounterKind::Token,
                            day,
                        )?;
                        Ok((bill, token))
                    })
                    .expect("a number");
                mine.push(claimed);
            }
            mine
        }));
    }

    // The third till joins while the other two are billing.
    std::thread::sleep(std::time::Duration::from_millis(5));
    db.transaction(|tx| {
        let mut row = Terminal::new("t_c", "Parcel window", crate::flows::now());
        row.series_prefix = "C/".to_owned();
        Repos::new(tx)
            .terminals()
            .save(OUTLET, &row, crate::flows::now())
    })
    .expect("the third till joins mid-service");

    let mut bills = Vec::new();
    let mut tokens = Vec::new();
    for thread in threads {
        for (bill, token) in thread.join().expect("a till finished") {
            bills.push(bill);
            tokens.push(token);
        }
    }

    // Unique across the shop, both kinds.
    let unique: BTreeSet<&str> = bills.iter().map(|b| b.formatted.as_str()).collect();
    assert_eq!(unique.len(), bills.len(), "two bills share a number");
    let unique_tokens: BTreeSet<&str> = tokens.iter().map(|t| t.formatted.as_str()).collect();
    assert_eq!(
        unique_tokens.len(),
        tokens.len(),
        "two customers share a token"
    );

    // And DENSE from one, per series.
    for prefix in ["A/", "B/"] {
        let mut values: Vec<u64> = bills
            .iter()
            .filter(|b| b.formatted.starts_with(prefix))
            .map(|b| b.value)
            .collect();
        values.sort_unstable();
        assert_eq!(values.len(), EACH, "{prefix} lost bills");
        for (i, value) in values.iter().enumerate() {
            assert_eq!(*value, i as u64 + 1, "{prefix} has a gap at {value}");
        }
    }

    // The third till starts at one, under its own letter, with nothing skipped because two
    // other tills had been running for 500 bills.
    let first = db
        .transaction(|tx| {
            mb_db::numbering::claim(tx, OUTLET, "t_c", mb_db::numbering::CounterKind::Bill, day)
        })
        .expect("the new till's first bill");
    assert_eq!(first.formatted, "C/0001");
}

/// The master dies mid-service, and the secondary keeps taking money.
#[test]
fn a_secondary_keeps_billing_while_the_master_is_off_and_hands_over_when_it_returns() {
    let scratch = Scratch::new("term_dead");
    let master = a_till(&scratch, "master", "t_a", "Counter 1", "A/");
    let second = a_till(&scratch, "second", "t_b", "Counter 2", "B/");

    // The master is off.
    let printed: Vec<String> = (0..3).map(|_| a_cash_sale(&second)).collect();
    assert_eq!(printed, vec!["B/0001", "B/0002", "B/0003"]);

    // And the till says so, in a sentence rather than a spinner.
    let waiting = crate::forwarding::waiting_on(&second).expect("the queue");
    assert_eq!(waiting.len(), 3);
    let says = crate::forwarding::waiting_says(waiting.len());
    assert!(says.contains("3 bills are waiting"), "{says}");
    assert!(says.contains("Nothing is lost"), "{says}");

    // Nothing reached the master while it was away.
    assert!(numbers_in(&master).is_empty());

    // It comes back.
    assert_eq!(hand_over(&second, &master), 3);
    let mut landed = numbers_in(&master);
    landed.sort();
    assert_eq!(landed, vec!["B/0001", "B/0002", "B/0003"]);
    // The queue cleared, and only because the master confirmed.
    assert!(
        crate::forwarding::waiting_on(&second)
            .expect("queue")
            .is_empty()
    );
    assert_eq!(crate::forwarding::waiting_says(0), "");
}

/// Forwarding is idempotent.
#[test]
fn sending_the_same_bills_five_times_changes_nothing_after_the_first() {
    let scratch = Scratch::new("term_idem");
    let master = a_till(&scratch, "master", "t_a", "Counter 1", "A/");
    let second = a_till(&scratch, "second", "t_b", "Counter 2", "B/");

    a_cash_sale(&second);
    a_cash_sale(&second);

    let waiting = crate::forwarding::waiting_on(&second).expect("queue");
    let batch = crate::forwarding::from_here(&second, waiting).expect("described");

    let first = crate::forwarding::receive_on(&master, &batch).expect("stored");
    for _ in 0..4 {
        let again = crate::forwarding::receive_on(&master, &batch).expect("stored again");
        // Byte for byte the same answer, because the sender may be showing it to a person.
        assert_eq!(again.stored, first.stored);
        assert_eq!(again.says, first.says);
        assert!(again.refused.is_empty());
    }

    let mut landed = numbers_in(&master);
    landed.sort();
    assert_eq!(landed, vec!["B/0001", "B/0002"], "a bill was stored twice");
}

/// A THIRTY-MINUTE PARTITION, BOTH TILLS BILLING, HEALS WITH NOTHING LOST AND NOTHING DOUBLED.
#[test]
fn a_thirty_minute_partition_loses_nothing_and_doubles_nothing() {
    let scratch = Scratch::new("term_split");
    let master = a_till(&scratch, "master", "t_a", "Counter 1", "A/");
    let second = a_till(&scratch, "second", "t_b", "Counter 2", "B/");

    // Before the switch is kicked, they are in step.
    a_cash_sale(&master);
    a_cash_sale(&second);
    assert_eq!(hand_over(&second, &master), 1);

    // The split. Half an hour, both counters serving.
    let mut on_master = Vec::new();
    let mut on_second = Vec::new();
    for _ in 0..12 {
        on_master.push(a_cash_sale(&master));
        on_second.push(a_cash_sale(&second));
    }

    // The second till is holding its half and saying so.
    let held = crate::forwarding::waiting_on(&second).expect("queue").len();
    assert_eq!(held, 12);

    // The switch goes back in.
    assert_eq!(hand_over(&second, &master), 12);
    assert_eq!(hand_over(&second, &master), 0, "it re-sent a settled queue");

    let book = numbers_in(&master);
    // Nothing doubled.
    let unique: BTreeSet<&str> = book.iter().map(String::as_str).collect();
    assert_eq!(unique.len(), book.len(), "a bill is in the book twice");
    // Nothing lost: 1 + 12 from each till, both series dense from one.
    assert_eq!(book.len(), 26, "the shop's book is not whole");
    for (series, count) in [("A/", 13), ("B/", 13)] {
        let mine: BTreeSet<&str> = book
            .iter()
            .map(String::as_str)
            .filter(|n| n.starts_with(series))
            .collect();
        assert_eq!(mine.len(), count, "{series} is short");
        for n in 1..=count {
            let want = format!("{series}{n:04}");
            assert!(
                mine.contains(want.as_str()),
                "{want} is missing from the book"
            );
        }
    }
    // No collision: the two series share no value, by construction.
    assert!(on_master.iter().all(|n| n.starts_with("A/")));
    assert!(on_second.iter().all(|n| n.starts_with("B/")));

    // And the money ties.
    let day = crate::flows::today(crate::flows::now());
    let takings = |app: &App, till: Option<&str>| -> i64 {
        app.with_shop(|shop| {
            shop.db
                .read_transaction(|tx| {
                    Ok(Repos::new(tx)
                        .orders()
                        .list_for_day(OUTLET, day)?
                        .iter()
                        .filter_map(|o| match o {
                            AnyOrder::Settled(s)
                                if till.is_none_or(|t| s.bill_number.formatted.starts_with(t)) =>
                            {
                                Some(s.bill.grand_total.paise())
                            }
                            _ => None,
                        })
                        .sum::<i64>())
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("takings")
    };
    assert_eq!(
        takings(&master, None),
        takings(&master, Some("A/")) + takings(&second, Some("B/")),
        "the shop's takings do not equal the two tills' takings"
    );
    // And the secondary's own book is untouched — it is still the record of what happened at
    // that counter.
    assert_eq!(numbers_in(&second).len(), 13);
}

// The conflicts.

/// Every conflict resolves as documented, and the loser gets the sentence — with the till's
/// NAME in it, because "at the counter" is not an answer when there are two counters.
#[test]
fn every_conflict_resolves_as_documented_and_names_the_till() {
    let scratch = Scratch::new("term_conflict");
    let master = a_till(&scratch, "master", "t_a", "Counter 1", "A/");
    // A second till on the master's own file, so the master can see both — it is the master
    // that answers every intent.
    master
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let repos = Repos::new(tx);
                    let mut row = Terminal::new("t_b", "Counter 2", crate::flows::now());
                    row.series_prefix = "B/".to_owned();
                    repos.terminals().save(OUTLET, &row, crate::flows::now())?;
                    repos
                        .floor()
                        .save_table(OUTLET, &a_table("tbl_5", "5"), crate::flows::now())
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("a floor and a second till");

    let (staff, may) = a_cashier();
    let open_table = |id: &str| Intent {
        id: id.to_owned(),
        order_id: None,
        at: crate::flows::now().millis(),
        what: What::OpenOrder {
            order_type: "dine_in".to_owned(),
            table_id: Some("tbl_5".to_owned()),
            covers: Some(2),
        },
    };

    // Conflict one. Counter 1 opens table 5; Counter 2 opens it a moment later and JOINS it
    // rather than being turned away — a party half-ordered on one till must be servable at the
    // other, and two orders on one table is the failure this avoids.
    let first =
        crate::orders::apply(&master, "t_a", &staff, &may, &open_table("i1")).expect("opened");
    let Outcome::Ok { order_id, .. } = first.outcome else {
        panic!("the first open was not applied");
    };

    let second =
        crate::orders::apply(&master, "t_b", &staff, &may, &open_table("i2")).expect("joined");
    let Outcome::Ok {
        order_id: joined,
        note,
        ..
    } = second.outcome
    else {
        panic!("the second open was not applied");
    };
    assert_eq!(joined, order_id, "two tills opened two orders on one table");
    let note = note.unwrap_or_default();
    assert!(
        note.contains("Counter 1"),
        "the sentence does not say which till has it: {note}"
    );
    assert!(note.contains("same order"), "{note}");

    // Conflict two: the settle wins.
    let add = |id: &str| Intent {
        id: id.to_owned(),
        order_id: Some(order_id.clone()),
        at: crate::flows::now().millis(),
        what: What::AddItem {
            item_id: "itm_dosa".to_owned(),
            qty: "1".to_owned(),
            note: None,
            modifiers: Vec::new(),
        },
    };
    crate::orders::apply(&master, "t_b", &staff, &may, &add("i2b")).expect("a dosa");
    settle_where_it_stands(&master, &order_id, &staff);

    let late = crate::orders::apply(&master, "t_b", &staff, &may, &add("i3")).expect("answered");
    let Outcome::Refused { message } = late.outcome else {
        panic!("adding to a settled bill was allowed");
    };
    assert!(
        message.contains("Counter 1"),
        "the refusal does not say where it was paid: {message}"
    );
    assert!(message.contains("Start a new order"), "{message}");

    // Conflict three: a till goes offline mid-order.
    let hanging =
        crate::orders::apply(&master, "t_b", &staff, &may, &open_table("i4")).expect("opened");
    let Outcome::Ok {
        order_id: hanging, ..
    } = hanging.outcome
    else {
        panic!("the abandoned order did not open");
    };
    let travelling = crate::forwarding::waiting_on(&master).expect("queue");
    let ids: Vec<String> = travelling
        .iter()
        .filter_map(|o| o.get("core")?.get("id")?.as_str().map(ToOwned::to_owned))
        .collect();
    assert!(
        !ids.contains(&hanging),
        "an unfinished order escaped onto the wire: {ids:?}"
    );
    // What DID travel is the settled bill, and only that.
    assert_eq!(ids, vec![order_id], "something other than a fact is queued");
}

// The live floor.

/// A table opened on one till is on the other's floor immediately.
#[test]
fn a_table_opened_on_one_till_is_on_the_other_tills_floor() {
    let scratch = Scratch::new("term_floor");
    let master = a_till(&scratch, "master", "t_a", "Counter 1", "A/");
    master
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    Repos::new(tx).floor().save_table(
                        OUTLET,
                        &a_table("tbl_9", "9"),
                        crate::flows::now(),
                    )
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("a floor");

    let before = crate::floor::floor_on(&master).expect("the grid");
    assert!(
        before.tables.iter().all(|t| !t.is_busy),
        "the table is busy before anybody sat down"
    );

    let (staff, may) = a_cashier();
    crate::orders::apply(
        &master,
        "t_b",
        &staff,
        &may,
        &Intent {
            id: "i9".to_owned(),
            order_id: None,
            at: crate::flows::now().millis(),
            what: What::OpenOrder {
                order_type: "dine_in".to_owned(),
                table_id: Some("tbl_9".to_owned()),
                covers: Some(4),
            },
        },
    )
    .expect("opened from the other till");

    let after = crate::floor::floor_on(&master).expect("the grid again");
    let table = after
        .tables
        .iter()
        .find(|t| t.id == "tbl_9")
        .expect("the table is still on the floor");
    assert!(
        table.is_busy,
        "the other till cannot see the table is taken"
    );
}

/// Two tills cannot take the same prefix, and the refusal names the one that has it.
#[test]
fn two_tills_cannot_share_a_prefix_and_the_refusal_names_the_one_that_has_it() {
    let scratch = Scratch::new("term_prefix");
    let app = a_till(&scratch, "shop", "t_a", "Counter 1", "A/");

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                let mut second = Terminal::new("t_b", "Counter 2", crate::flows::now());
                second.series_prefix = "B/".to_owned();
                repos.terminals().save(OUTLET, &second, crate::flows::now())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a second till");

    let clash = crate::terminals::save_till_on(
        &app,
        crate::terminals::TerminalEdit {
            id: "t_b".to_owned(),
            name: "Counter 2".to_owned(),
            prefix: "A/".to_owned(),
        },
    )
    .expect_err("it let two tills share a series");
    assert!(
        clash.message.contains("Counter 1"),
        "the refusal does not name the till that has it: {}",
        clash.message
    );

    // And an EMPTY prefix is refused too, which is the collision people do not see coming: two
    // tills both printing bare 0001.
    let bare = crate::terminals::save_till_on(
        &app,
        crate::terminals::TerminalEdit {
            id: "t_b".to_owned(),
            name: "Counter 2".to_owned(),
            prefix: String::new(),
        },
    )
    .expect_err("it let a second till print bare numbers");
    assert!(bare.message.contains("prefix"), "{}", bare.message);
}

/// Moving the master, and there is never a moment when two answer.
#[test]
fn moving_the_master_leaves_exactly_one_and_the_old_one_stands_down() {
    let scratch = Scratch::new("term_master");
    let app = a_till(&scratch, "shop", "t_a", "Counter 1", "A/");
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                let mut second = Terminal::new("t_b", "Counter 2", crate::flows::now());
                second.series_prefix = "B/".to_owned();
                repos
                    .terminals()
                    .save(OUTLET, &second, crate::flows::now())?;
                repos
                    .terminals()
                    .make_master(OUTLET, "t_a", crate::flows::now())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a shop with a master");

    let before = crate::terminals::tills_on(&app).expect("the roster");
    assert_eq!(before.tills.iter().filter(|t| t.is_master).count(), 1);
    assert!(before.is_master, "this machine is the master");

    crate::terminals::make_master_on(&app, "t_b".to_owned()).expect("moved");

    let after = crate::terminals::tills_on(&app).expect("the roster again");
    // Exactly one, at every moment: the clear and the set are one transaction, so there is no
    // instant with two or none.
    assert_eq!(
        after.tills.iter().filter(|t| t.is_master).count(),
        1,
        "the shop has two masters or none"
    );
    assert!(after.tills.iter().any(|t| t.id == "t_b" && t.is_master));
    assert!(
        !after.is_master,
        "the old master still thinks it is the master"
    );
}

// The arithmetic.

/// THE ARITHMETIC TIES.
#[test]
fn per_till_drawers_sum_exactly_to_the_shops_day() {
    let scratch = Scratch::new("term_close");
    let master = a_till(&scratch, "master", "t_a", "Counter 1", "A/");
    let second = a_till(&scratch, "second", "t_b", "Counter 2", "B/");

    let mut expected = 0_i64;
    for _ in 0..4 {
        a_cash_sale(&master);
    }
    for _ in 0..3 {
        a_cash_sale(&second);
    }
    hand_over(&second, &master);

    let day = crate::flows::today(crate::flows::now());
    // What each till took, read off the master's book by whose series the bill is in — which is
    // the only thing that identifies a till on a receipt.
    for series in ["A/", "B/"] {
        let took = master
            .with_shop(|shop| {
                shop.db
                    .read_transaction(|tx| {
                        Ok(Repos::new(tx)
                            .orders()
                            .list_for_day(OUTLET, day)?
                            .iter()
                            .filter_map(|o| match o {
                                AnyOrder::Settled(s)
                                    if s.bill_number.formatted.starts_with(series) =>
                                {
                                    Some(s.bill.grand_total.paise())
                                }
                                _ => None,
                            })
                            .sum::<i64>())
                    })
                    .map_err(|e| crate::words::from_db(&e))
            })
            .expect("takings");
        assert!(took > 0, "{series} took nothing");
        expected += took;
    }

    let whole = master
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| {
                    Ok(Repos::new(tx)
                        .orders()
                        .list_for_day(OUTLET, day)?
                        .iter()
                        .filter_map(|o| match o {
                            AnyOrder::Settled(s) => Some(s.bill.grand_total.paise()),
                            _ => None,
                        })
                        .sum::<i64>())
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("the shop's day");
    // Exactly, to the paisa.
    assert_eq!(whole, expected, "the tills do not sum to the shop");
}

// The budget that must NOT have moved.

#[test]
fn the_billing_path_cannot_reach_the_main_till() {
    for (name, source) in [
        ("billing.rs", include_str!("billing.rs")),
        ("flows.rs", include_str!("flows.rs")),
        ("orders.rs", include_str!("orders.rs")),
    ] {
        for (number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for forbidden in ["mb_lan::Master", "forwarding::send", "forward_blocking"] {
                assert!(
                    !code.contains(forbidden),
                    "{name} line {} can reach the main till, and it is on the \
                     billing path — D135 exists so that it cannot: {}",
                    number + 1,
                    code.trim()
                );
            }
        }
    }
}

#[test]
#[ignore = "a measurement, not an assertion — see the note above"]
fn r9_a_settle_on_a_secondary_costs_the_same_whether_the_master_is_there_or_not() {
    let scratch = Scratch::new("term_budget");
    let master = a_till(&scratch, "master", "t_a", "Counter 1", "A/");
    let second = a_till(&scratch, "second", "t_b", "Counter 2", "B/");

    // Warm: the first bill of a process pays for lazy statements everywhere.
    for _ in 0..5 {
        a_cash_sale(&second);
    }
    hand_over(&second, &master);

    let time_ten = || {
        let started = std::time::Instant::now();
        for _ in 0..10 {
            a_cash_sale(&second);
        }
        started.elapsed()
    };

    // With the master reachable — and note there is nothing to "connect" to, which IS the
    // finding: the settle path has no client in it.
    let with_master = time_ten();
    hand_over(&second, &master);
    // With it unplugged. Same code, because the sender is a different thread and the settle
    // never asks it anything.
    let alone = time_ten();

    // Printed, because a number a person reads is the point of running this.
    println!("R9: ten bills with the main till up  — {with_master:?}");
    println!("R9: ten bills with the main till off — {alone:?}");

    // The only assertion, and it is enormous on purpose: a single connect timeout on the settle
    // path adds TWO SECONDS to a run, so five seconds for ten bills cannot be reached by a slow
    // disk or a busy scheduler.
    for (what, took) in [
        ("with the main till up", with_master),
        ("with it off", alone),
    ] {
        assert!(
            took < std::time::Duration::from_secs(5),
            "ten bills {what} took {took:?} — that is long enough to be waiting \
             on a network, and nothing on this path may"
        );
    }
}
