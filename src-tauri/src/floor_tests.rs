//! **The floor, driven end to end against a real database** — P14.
//!
//! `signin_tests` gives the argument in full and it applies here twice over:
//! moving, merging and splitting an order are *sequences*, they are on the
//! money path, and they move the kitchen ledger between orders. A wrong answer
//! is not a wrong number on a screen — it is three more dosas on the pass.
//!
//! mb-core proves the arithmetic (`transfer.rs`) and mb-db proves the master
//! data (`tests/floor.rs`). What is proved here is the wiring: that the real
//! command bodies, against a real SQLite file, leave the disk saying what the
//! screen says.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_core::{
    AnyOrder, BusinessDay, Cart, DraftOrder, ItemSnapshot, Money, OrderId, OrderType, Qty, StaffId,
    TableId, TaxRate,
};
use mb_db::repo::floor::{DiningTable, Section};
use mb_db::{Db, DbConfig, Repos};

use crate::floor::{
    SplitRequest, even_split_on, floor_on, merge_orders_on, move_order_on, save_thresholds_on,
    split_order_on,
};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

fn at(n: i64) -> mb_core::Timestamp {
    mb_core::Timestamp::from_millis(1_770_000_000_000 + n * 60_000)
}

fn day() -> BusinessDay {
    BusinessDay::from_ymd(2026, 8, 8)
}

fn snapshot(id: &str, paise: i64) -> ItemSnapshot {
    ItemSnapshot::new(mb_core::ItemId::new(id), id, Money::from_paise(paise), TaxRate::GST_5)
}

/// A shop with a room and a menu, and nothing on the floor yet.
fn a_shop_with_a_room(scratch: &Scratch) -> App {
    let path = scratch.dir().join("floor.db");
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.floor().save_section(
            OUTLET,
            &Section {
                id: "sec_hall".to_owned(),
                name: "Hall".to_owned(),
                sort_order: 0,
                is_active: true,
            },
            at(0),
        )?;
        for n in 1..=4 {
            repos.floor().save_table(
                OUTLET,
                &DiningTable {
                    id: TableId::new(format!("tbl_{n}")),
                    section_id: Some("sec_hall".to_owned()),
                    label: n.to_string(),
                    seats: 4,
                    pos: None,
                    sort_order: n,
                    is_active: true,
                },
                at(0),
            )?;
        }
        for (id, name, paise) in [("itm_dosa", "Dosa", 12_000_i64), ("itm_tea", "Tea", 2_000)] {
            repos.menu().save_item(
                OUTLET,
                &mb_db::repo::menu::MenuItem {
                    id: mb_core::ItemId::new(id),
                    category_id: None,
                    name: name.to_owned(),
                    unit_price: Money::from_paise(paise),
                    tax_rate: TaxRate::GST_5,
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
                at(0),
            )?;
        }
        Ok(())
    })
    .expect("a room and a menu");

    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    app
}

/// Put an open order on a table, with `told` of the first line already sent to
/// the kitchen.
fn seat(app: &App, id: &str, table: &str, items: &[(&str, i64, i64)], told: Option<i64>) -> OrderId {
    let mut cart = Cart::new();
    for (item, paise, qty) in items {
        cart.add(
            snapshot(item, *paise),
            Qty::from_whole(*qty).expect("qty"),
            None,
            Vec::new(),
        )
        .expect("added");
    }

    let mut draft = DraftOrder::new(
        OrderId::new(id),
        day(),
        at(1),
        OrderType::DineIn,
        StaffId::new(crate::state::DEFAULT_STAFF),
    )
    .on_table(TableId::new(table));
    draft.core.cart = cart;

    if let Some(told) = told {
        let identity = draft.core.cart.lines()[0].identity();
        draft
            .core
            .kitchen
            .mark_printed(&[(identity, Qty::from_whole(told).expect("qty"))])
            .expect("told");
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                let token = mb_db::numbering::claim(
                    tx,
                    OUTLET,
                    crate::billing::TERMINAL,
                    mb_db::numbering::CounterKind::Token,
                    day(),
                )?;
                let bill_number = mb_db::numbering::claim(
                    tx,
                    OUTLET,
                    crate::billing::TERMINAL,
                    mb_db::numbering::CounterKind::Bill,
                    day(),
                )?;
                repos.orders().save(
                    OUTLET,
                    crate::billing::TERMINAL,
                    &AnyOrder::Open(mb_core::OpenOrder {
                        core: draft.core.clone(),
                        token,
                        bill_number,
                    }),
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("seated");

    OrderId::new(id)
}

fn read(app: &App, id: &OrderId) -> AnyOrder {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Repos::new(tx)
                    .orders()
                    .find(id)?
                    .ok_or_else(|| mb_db::DbError::invariant("gone"))
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("the order")
}

/// **T5 and T6 — MOVE.** The party changed seats, and that is all that
/// changed: same order, same bill number, same food, same ledger.
#[test]
fn moving_an_order_changes_the_table_and_nothing_else() {
    let scratch = Scratch::new("move");
    let app = a_shop_with_a_room(&scratch);
    let order = seat(&app, "ord_move", "tbl_1", &[("itm_dosa", 12_000, 2)], Some(2));

    let before = read(&app, &order);
    let number = before.bill_number().map(|c| c.formatted.clone());

    move_order_on(&app, order.as_str().to_owned(), "tbl_3".to_owned()).expect("moved");

    let after = read(&app, &order);
    assert_eq!(
        after.core().table.as_ref().map(TableId::as_str),
        Some("tbl_3"),
        "the order is at the new table",
    );
    assert_eq!(after.bill_number().map(|c| c.formatted.clone()), number, "the number is untouched");
    assert_eq!(after.core().cart, before.core().cart, "and so is the food");
    assert_eq!(after.core().kitchen, before.core().kitchen, "and the kitchen ledger");

    // Table 1 is free again — the floor says so, not just the row.
    let floor = floor_on(&app).expect("the floor");
    let one = floor.tiles.iter().find(|t| t.label == "1").expect("table 1");
    assert!(one.order_id.is_none(), "the old table is free");

    // And a move onto an occupied table is refused in words.
    seat(&app, "ord_other", "tbl_2", &[("itm_tea", 2_000, 1)], None);
    let refused = move_order_on(&app, order.as_str().to_owned(), "tbl_2".to_owned())
        .expect_err("occupied");
    assert!(refused.message.contains("already an order"), "{}", refused.message);
    assert_eq!(refused.code, "floor.table_busy");
}

/// **T7 — MERGE.** Two tables told the kitchen about dosas separately; the
/// merged order was told about all of them, and one bill number survives while
/// the other order is recorded rather than deleted.
#[test]
fn merging_two_tables_combines_the_food_and_never_re_tells_the_kitchen() {
    let scratch = Scratch::new("merge");
    let app = a_shop_with_a_room(&scratch);
    let four = seat(&app, "ord_four", "tbl_1", &[("itm_dosa", 12_000, 2)], Some(2));
    let five = seat(&app, "ord_five", "tbl_2", &[("itm_dosa", 12_000, 1)], Some(1));

    merge_orders_on(&app, five.as_str().to_owned(), four.as_str().to_owned()).expect("merged");

    let survivor = read(&app, &four);
    assert_eq!(survivor.core().cart.len(), 1, "one dish, one line");
    assert_eq!(
        survivor.core().cart.lines()[0].qty,
        Qty::from_whole(3).expect("three"),
    );
    assert!(
        survivor
            .core()
            .kitchen
            .pending(&survivor.core().cart)
            .expect("pending")
            .is_empty(),
        "the kitchen was told about all three already — this is the three-dosa bug",
    );

    // The absorbed order is CANCELLED with a link, not deleted (D47).
    let absorbed = read(&app, &five);
    assert!(matches!(absorbed, AnyOrder::Cancelled(_)), "recorded, not deleted");
    assert!(absorbed.bill_number().is_some(), "and it keeps its number");

    let link: Option<String> = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    Ok(tx.query_row(
                        "SELECT merged_into FROM orders WHERE id = ?1",
                        [five.as_str()],
                        |r| r.get(0),
                    )?)
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("read back");
    assert_eq!(link.as_deref(), Some(four.as_str()), "where the food went is a row");

    // Table 2 is free, table 1 is busy.
    let floor = floor_on(&app).expect("the floor");
    let two = floor.tiles.iter().find(|t| t.label == "2").expect("table 2");
    assert!(two.order_id.is_none());
}

/// **T8 and T10 — SPLIT.** Part of a table leaves for a bill of its own, and
/// the exact told-quantity case from `transfer.rs` survives the round trip to
/// disk and back.
#[test]
fn splitting_gives_the_new_bill_its_own_number_and_the_right_ledger() {
    let scratch = Scratch::new("split");
    let app = a_shop_with_a_room(&scratch);
    // Three dosas, two of them already told to the kitchen, plus a tea so the
    // origin is not emptied.
    let order = seat(
        &app,
        "ord_split",
        "tbl_1",
        &[("itm_dosa", 12_000, 3), ("itm_tea", 2_000, 1)],
        Some(2),
    );

    split_order_on(
        &app,
        SplitRequest {
            order_id: order.as_str().to_owned(),
            lines: vec![(0, "2".to_owned())],
            to_table: Some("tbl_4".to_owned()),
            seat: None,
        },
    )
    .expect("split");

    let kept = read(&app, &order);
    assert_eq!(kept.core().cart.lines()[0].qty, Qty::from_whole(1).expect("one"));
    let dosa = kept.core().cart.lines()[0].identity();
    assert!(
        !kept.core()
            .kitchen
            .pending(&kept.core().cart)
            .expect("pending")
            .iter()
            .any(|(id, _)| id == &dosa),
        "the origin must not ask the kitchen for a dosa it has already served",
    );

    // The new order: its own bill number, and one dosa still to tell.
    let fresh = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| Repos::new(tx).orders().list_open(OUTLET))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("open orders")
        .into_iter()
        .find(|o| o.core().id != order)
        .expect("a second order");

    assert_eq!(fresh.core().table.as_ref().map(TableId::as_str), Some("tbl_4"));
    assert_eq!(fresh.core().cart.lines()[0].qty, Qty::from_whole(2).expect("two"));
    assert_ne!(
        fresh.bill_number().map(|c| c.formatted.clone()),
        kept.bill_number().map(|c| c.formatted.clone()),
        "two bills, two numbers",
    );
    let pending = fresh.core().kitchen.pending(&fresh.core().cart).expect("pending");
    assert_eq!(pending.len(), 1, "exactly one dosa still to cook");
    assert_eq!(pending[0].1, Qty::from_whole(1).expect("one"));
}

/// Splitting everything off is a MOVE, and is refused as one — the message
/// says what to do instead rather than minting a bill number and abandoning
/// one.
#[test]
fn splitting_the_whole_order_is_refused() {
    let scratch = Scratch::new("split_all");
    let app = a_shop_with_a_room(&scratch);
    let order = seat(&app, "ord_all", "tbl_1", &[("itm_dosa", 12_000, 2)], None);

    let refused = split_order_on(
        &app,
        SplitRequest {
            order_id: order.as_str().to_owned(),
            lines: vec![(0, "2".to_owned())],
            to_table: None,
            seat: None,
        },
    )
    .expect_err("refused");
    assert!(refused.message.contains("move the whole order"), "{}", refused.message);
}

/// **T17 — sub-tables.** 6A and 6B are two orders at one table, both visible,
/// and the second is created by splitting the first without leaving the table.
#[test]
fn two_parties_can_share_one_table() {
    let scratch = Scratch::new("sub_table");
    let app = a_shop_with_a_room(&scratch);
    let order = seat(
        &app,
        "ord_shared",
        "tbl_1",
        &[("itm_dosa", 12_000, 2), ("itm_tea", 2_000, 2)],
        None,
    );

    split_order_on(
        &app,
        SplitRequest {
            order_id: order.as_str().to_owned(),
            lines: vec![(1, "2".to_owned())],
            to_table: None,
            seat: Some("b".to_owned()),
        },
    )
    .expect("split into a second seat");

    let open = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| Repos::new(tx).orders().list_open(OUTLET))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("open orders");

    assert_eq!(open.len(), 2, "two parties");
    assert!(
        open.iter().all(|o| o.core().table.as_ref().map(TableId::as_str) == Some("tbl_1")),
        "both at the same table",
    );
    let seats: Vec<String> = open
        .iter()
        .filter_map(|o| o.core().sub_table.as_ref().map(|s| s.as_str().to_owned()))
        .collect();
    assert_eq!(seats, ["B"], "the new party is seat B, upper-cased");
}

/// **T11 — the thresholds are settings, not a constant**, and both states are
/// reachable.
#[test]
fn the_two_timers_come_from_settings() {
    let scratch = Scratch::new("timers");
    let app = a_shop_with_a_room(&scratch);
    seat(&app, "ord_old", "tbl_1", &[("itm_dosa", 12_000, 1)], None);

    // The order was created an hour ago in fixture time, which is the past
    // relative to a real clock, so it is comfortably "late" at any threshold.
    let before = floor_on(&app).expect("the floor");
    assert_eq!(i64::from(before.warn_minutes), crate::floor::DEFAULT_WARN_MINUTES);
    assert_eq!(i64::from(before.late_minutes), crate::floor::DEFAULT_LATE_MINUTES);

    let after = save_thresholds_on(&app, 5, 10).expect("saved");
    assert_eq!(after.warn_minutes, 5);
    assert_eq!(after.late_minutes, 10);

    // And a pair that would make the amber state unreachable is refused rather
    // than stored and quietly repaired every read.
    assert!(save_thresholds_on(&app, 30, 30).is_err());
    assert!(save_thresholds_on(&app, 0, 10).is_err());
}

/// **T9 — an even split assigns every paisa**, and says so out loud.
#[test]
fn an_even_split_says_who_pays_the_extra_paisa() {
    let scratch = Scratch::new("even");
    let app = a_shop_with_a_room(&scratch);

    // A bill in the cart. 100.01 three ways is the case that has a remainder.
    app.with_cart_mut(|state| {
        state
            .cart
            .add(snapshot("itm_dosa", 10_001), Qty::from_whole(1).expect("one"), None, Vec::new())
            .expect("added");
        Ok(())
    })
    .expect("a cart");

    let split = even_split_on(&app, 3).expect("three ways");
    assert_eq!(split.shares.len(), 3);
    let sum: i64 = split.shares.iter().map(|s| s.paise).sum();
    assert_eq!(sum, split.total.paise, "the shares add back to the bill exactly");
    assert!(!split.note.is_empty(), "and the remainder is said out loud");

    assert!(even_split_on(&app, 1).is_err(), "one way is not a split");
}

/// **T15 — the fallback is not a degraded mode.** A shop with no floor plan
/// gets the section grid and everything works.
#[test]
fn a_shop_with_no_floor_plan_still_has_a_floor() {
    let scratch = Scratch::new("no_plan");
    let app = a_shop_with_a_room(&scratch);
    seat(&app, "ord_plain", "tbl_2", &[("itm_tea", 2_000, 1)], None);

    let floor = floor_on(&app).expect("the floor");
    assert!(!floor.has_layout, "nothing has been placed");
    assert_eq!(floor.tiles.len(), 4, "and every table is still a tile");
    assert!(floor.tiles.iter().any(|t| t.order_id.is_some()), "with its order on it");
    assert!(floor.occupancy.busy.contains("1 of 4"), "{}", floor.occupancy.busy);
}
