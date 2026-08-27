//! The floor, driven end to end against a real database.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_auth::RolePreset;
use mb_core::{
    AnyOrder, BusinessDay, Cart, DraftOrder, ItemSnapshot, Money, OrderId, Qty, StaffId, TableId,
    TaxRate,
};
use mb_db::repo::floor::{DiningTable, Section};
use mb_db::{Db, DbConfig, Repos};

use crate::floor::{
    SplitRequest, floor_on, merge_orders_on, move_order_on, save_thresholds_on,
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
    ItemSnapshot::new(
        mb_core::ItemId::new(id),
        id,
        Money::from_paise(paise),
        TaxRate::from_percent(5).expect("5%"),
    )
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
                    tax: mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
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

/// Put an open order on a table, with `told` of the first line already sent to the kitchen.
fn seat(
    app: &App,
    id: &str,
    table: &str,
    items: &[(&str, i64, i64)],
    told: Option<i64>,
) -> OrderId {
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
        mb_core::Placement::on_table(TableId::new(table)),
        StaffId::new(crate::state::DEFAULT_STAFF),
    );
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
                    crate::terminals::TERMINAL,
                    mb_db::numbering::CounterKind::Token,
                    day(),
                )?;
                let bill_number = mb_db::numbering::claim(
                    tx,
                    OUTLET,
                    crate::terminals::TERMINAL,
                    mb_db::numbering::CounterKind::Bill,
                    day(),
                )?;
                repos.orders().save(
                    OUTLET,
                    crate::terminals::TERMINAL,
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

#[test]
fn moving_an_order_changes_the_table_and_nothing_else() {
    let scratch = Scratch::new("move");
    let app = a_shop_with_a_room(&scratch);
    let order = seat(
        &app,
        "ord_move",
        "tbl_1",
        &[("itm_dosa", 12_000, 2)],
        Some(2),
    );

    let before = read(&app, &order);
    let number = before.bill_number().map(|c| c.formatted.clone());

    move_order_on(&app, order.as_str().to_owned(), "tbl_3".to_owned()).expect("moved");

    let after = read(&app, &order);
    assert_eq!(
        after.core().table().map(TableId::as_str),
        Some("tbl_3"),
        "the order is at the new table",
    );
    assert_eq!(
        after.bill_number().map(|c| c.formatted.clone()),
        number,
        "the number is untouched"
    );
    assert_eq!(after.core().cart, before.core().cart, "and so is the food");
    assert_eq!(
        after.core().kitchen,
        before.core().kitchen,
        "and the kitchen ledger"
    );

    // Table 1 is free again — the floor says so, not just the row.
    let floor = floor_on(&app).expect("the floor");
    let one = floor
        .tiles
        .iter()
        .find(|t| t.label == "1")
        .expect("table 1");
    assert!(one.order_id.is_none(), "the old table is free");

    // And a move onto an occupied table is refused in words.
    seat(&app, "ord_other", "tbl_2", &[("itm_tea", 2_000, 1)], None);
    let refused =
        move_order_on(&app, order.as_str().to_owned(), "tbl_2".to_owned()).expect_err("occupied");
    assert!(
        refused.message.contains("already an order"),
        "{}",
        refused.message
    );
    assert_eq!(refused.code, "floor.table_busy");
}

/// MERGE. Two tables told the kitchen about dosas separately; the merged order was told about
/// all of them, and one bill number survives while the other order is recorded rather than
/// deleted.
#[test]
fn merging_two_tables_combines_the_food_and_never_re_tells_the_kitchen() {
    let scratch = Scratch::new("merge");
    let app = a_shop_with_a_room(&scratch);
    let four = seat(
        &app,
        "ord_four",
        "tbl_1",
        &[("itm_dosa", 12_000, 2)],
        Some(2),
    );
    let five = seat(
        &app,
        "ord_five",
        "tbl_2",
        &[("itm_dosa", 12_000, 1)],
        Some(1),
    );

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

    // The absorbed order is CANCELLED with a link, not deleted.
    let absorbed = read(&app, &five);
    assert!(
        matches!(absorbed, AnyOrder::Cancelled(_)),
        "recorded, not deleted"
    );
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
    assert_eq!(
        link.as_deref(),
        Some(four.as_str()),
        "where the food went is a row"
    );

    // Table 2 is free, table 1 is busy.
    let floor = floor_on(&app).expect("the floor");
    let two = floor
        .tiles
        .iter()
        .find(|t| t.label == "2")
        .expect("table 2");
    assert!(two.order_id.is_none());
}

#[test]
fn splitting_gives_the_new_bill_its_own_number_and_the_right_ledger() {
    let scratch = Scratch::new("split");
    let app = a_shop_with_a_room(&scratch);
    // Three dosas, two of them already told to the kitchen, plus a tea so the origin is not
    // emptied.
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
    assert_eq!(
        kept.core().cart.lines()[0].qty,
        Qty::from_whole(1).expect("one")
    );
    let dosa = kept.core().cart.lines()[0].identity();
    assert!(
        !kept
            .core()
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

    assert_eq!(fresh.core().table().map(TableId::as_str), Some("tbl_4"));
    assert_eq!(
        fresh.core().cart.lines()[0].qty,
        Qty::from_whole(2).expect("two")
    );
    assert_ne!(
        fresh.bill_number().map(|c| c.formatted.clone()),
        kept.bill_number().map(|c| c.formatted.clone()),
        "two bills, two numbers",
    );
    let pending = fresh
        .core()
        .kitchen
        .pending(&fresh.core().cart)
        .expect("pending");
    assert_eq!(pending.len(), 1, "exactly one dosa still to cook");
    assert_eq!(pending[0].1, Qty::from_whole(1).expect("one"));
}

/// Splitting everything off is a MOVE, and is refused as one — the message says what to do
/// instead rather than minting a bill number and abandoning one.
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
    assert!(
        refused.message.contains("move the whole order"),
        "{}",
        refused.message
    );
}

/// Sub-tables. 6A and 6B are two orders at one table, both visible, and the second is created
/// by splitting the first without leaving the table.
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
        open.iter()
            .all(|o| o.core().table().map(TableId::as_str) == Some("tbl_1")),
        "both at the same table",
    );
    let seats: Vec<String> = open
        .iter()
        .filter_map(|o| o.core().seat().map(|s| s.as_str().to_owned()))
        .collect();
    assert_eq!(seats, ["B"], "the new party is seat B, upper-cased");
}

/// The thresholds are settings, not a constant, and both states are reachable.
#[test]
fn the_two_timers_come_from_settings() {
    let scratch = Scratch::new("timers");
    let app = a_shop_with_a_room(&scratch);
    seat(&app, "ord_old", "tbl_1", &[("itm_dosa", 12_000, 1)], None);

    // The order was created an hour ago in fixture time, which is the past relative to a real
    // clock, so it is comfortably "late" at any threshold.
    let before = floor_on(&app).expect("the floor");
    assert_eq!(
        i64::from(before.warn_minutes),
        crate::floor::DEFAULT_WARN_MINUTES
    );
    assert_eq!(
        i64::from(before.late_minutes),
        crate::floor::DEFAULT_LATE_MINUTES
    );

    let after = save_thresholds_on(&app, 5, 10).expect("saved");
    assert_eq!(after.warn_minutes, 5);
    assert_eq!(after.late_minutes, 10);

    // And a pair that would make the amber state unreachable is refused rather than stored and
    // quietly repaired every read.
    assert!(save_thresholds_on(&app, 30, 30).is_err());
    assert!(save_thresholds_on(&app, 0, 10).is_err());
}

/// The fallback is not a degraded mode.
#[test]
fn a_shop_with_no_floor_plan_still_has_a_floor() {
    let scratch = Scratch::new("no_plan");
    let app = a_shop_with_a_room(&scratch);
    seat(&app, "ord_plain", "tbl_2", &[("itm_tea", 2_000, 1)], None);

    let floor = floor_on(&app).expect("the floor");
    assert!(!floor.has_layout, "nothing has been placed");
    assert_eq!(floor.tiles.len(), 4, "and every table is still a tile");
    assert!(
        floor.tiles.iter().any(|t| t.order_id.is_some()),
        "with its order on it"
    );
    assert!(
        floor.occupancy.busy.contains("1 of 4"),
        "{}",
        floor.occupancy.busy
    );
}

// Carrying the bill to the table.

/// The bill goes to the table and the table stays open.
#[test]
fn the_bill_can_go_to_the_table_without_settling_it() {
    let scratch = Scratch::new("bill_to_table");
    let app = a_shop_with_a_room(&scratch);
    let order = seat(
        &app,
        "ord_bill",
        "tbl_3",
        &[("itm_dosa", 12_000, 2)],
        Some(2),
    );

    let before = read(&app, &order);
    let said = crate::flows::print_open_bill_on(&app, order.as_str().to_owned())
        .expect("the bill printed");

    // What the SHOP calls the table.
    assert_eq!(said, "The bill for table 3 is printing.", "{said}");
    assert!(
        !said.contains("tbl_"),
        "the toast shows a database id: {said}"
    );

    // Paper. And marked, so it can never be mistaken for a paid bill.
    let jobs = app.print_queue_snapshot();
    assert!(
        jobs.iter()
            .any(|j| j.reason.as_deref() == Some("bill to the table")),
        "nothing reached the printer: {jobs:?}"
    );

    // And nothing else. Same state, same table, same bill number — a settled order here would
    // mean the button closed a table that had not paid, which is the failure worth having a
    // test for.
    let after = read(&app, &order);
    assert!(
        matches!(after, AnyOrder::Open(_)),
        "printing the bill settled the order"
    );
    let (AnyOrder::Open(before), AnyOrder::Open(after)) = (&before, &after) else {
        panic!("the order stopped being open");
    };
    assert_eq!(
        before.bill_number.formatted, after.bill_number.formatted,
        "printing the bill burned a bill number"
    );
    assert_eq!(
        before.core.table(),
        after.core.table(),
        "the party was moved off its table"
    );
    assert_eq!(
        before.core.cart.lines().len(),
        after.core.cart.lines().len()
    );

    // Twice is two pieces of paper and nothing else — a waiter who lost the first one presses
    // it again, and that has to be free.
    crate::flows::print_open_bill_on(&app, order.as_str().to_owned()).expect("and again");
    assert!(matches!(read(&app, &order), AnyOrder::Open(_)));
}

/// A table with nothing on it, and a bill that has already been paid, are the two ways this can
/// be pressed on something it cannot print — and both say so in words a shopkeeper can act on
/// rather than failing silently.
#[test]
fn there_is_no_bill_to_carry_for_an_order_that_is_not_open() {
    let scratch = Scratch::new("bill_to_nobody");
    let app = a_shop_with_a_room(&scratch);

    let missing = crate::flows::print_open_bill_on(&app, "ord_nothing".to_owned())
        .expect_err("printed a bill for an order that does not exist");
    assert_eq!(missing.code, "bill.not_open", "{missing:?}");

    // An order that exists and has nothing on it.
    seat(&app, "ord_empty", "tbl_4", &[], None);
    let empty = crate::flows::print_open_bill_on(&app, "ord_empty".to_owned())
        .expect_err("printed an empty bill");
    assert_eq!(empty.code, "bill.empty", "{empty:?}");
}

/// The table the cashier is on is marked, even when nothing is on it yet.
#[test]
fn an_empty_table_is_marked_the_moment_it_is_opened() {
    let scratch = Scratch::new("selected_empty");
    let app = a_shop_with_a_room(&scratch);

    // Nothing is open, so nothing is selected.
    let before = crate::ipc::open_orders_on(&app).expect("the floor");
    assert!(
        before.iter().all(|tile| !tile.selected),
        "a floor nobody has touched has a table selected"
    );

    // Tap table 2 and type nothing at all — the exact case in the screenshot.
    crate::ipc::open_table_on(&app, "tbl_2".to_owned()).expect("opened");
    assert!(
        app.with_cart(|state| Ok(state.order_id().map(str::to_owned)))
            .expect("cart")
            .is_none(),
        "an empty table must not have an order yet, or this test is not the bug",
    );

    let after = crate::ipc::open_orders_on(&app).expect("the floor");
    let selected: Vec<&str> = after
        .iter()
        .filter(|tile| tile.selected)
        .map(|tile| tile.label.as_str())
        .collect();
    assert_eq!(
        selected,
        ["2"],
        "the tapped table is the one that is marked"
    );

    // And it is still FREE.
    let two = after.iter().find(|t| t.label == "2").expect("table 2");
    assert_eq!(two.state, crate::billing::TableState::Free);
    assert!(two.order_id.is_none());
}

/// Selecting a table costs it none of its own signal.
#[test]
fn a_late_table_that_is_open_in_the_cart_still_looks_late() {
    let scratch = Scratch::new("selected_late");
    let app = a_shop_with_a_room(&scratch);
    // `seat` stamps the order an hour into fixture time, which is the past against a real clock
    // — comfortably late at any threshold.
    seat(&app, "ord_late", "tbl_3", &[("itm_dosa", 12_000, 1)], None);

    let before = crate::ipc::open_orders_on(&app).expect("the floor");
    let three = before.iter().find(|t| t.label == "3").expect("table 3");
    assert_eq!(three.state, crate::billing::TableState::Late);
    assert!(!three.selected);

    crate::ipc::open_table_on(&app, "tbl_3".to_owned()).expect("opened");

    let after = crate::ipc::open_orders_on(&app).expect("the floor");
    let three = after.iter().find(|t| t.label == "3").expect("table 3");
    assert!(three.selected, "the open table is not marked");
    assert_eq!(
        three.state,
        crate::billing::TableState::Late,
        "opening a late table hid that it was late",
    );
}

/// One table at a time, and moving on releases the last one.
#[test]
fn only_one_table_is_ever_marked() {
    let scratch = Scratch::new("selected_one");
    let app = a_shop_with_a_room(&scratch);
    seat(&app, "ord_busy", "tbl_4", &[("itm_tea", 2_000, 1)], None);

    for (tapped, label) in [("tbl_1", "1"), ("tbl_4", "4"), ("tbl_2", "2")] {
        crate::ipc::open_table_on(&app, tapped.to_owned()).expect("opened");
        let floor = crate::ipc::open_orders_on(&app).expect("the floor");
        let marked: Vec<&str> = floor
            .iter()
            .filter(|tile| tile.selected)
            .map(|tile| tile.label.as_str())
            .collect();
        assert_eq!(marked, [label], "after tapping {tapped}");
    }
}

/// The floor plan marks NOTHING, and that is not an oversight.
#[test]
fn the_floor_plan_marks_no_table_because_it_has_no_cart() {
    let scratch = Scratch::new("selected_both");
    let app = a_shop_with_a_room(&scratch);
    crate::ipc::open_table_on(&app, "tbl_3".to_owned()).expect("opened");

    let marked = |tiles: &[crate::billing::TableView]| -> Vec<String> {
        tiles
            .iter()
            .filter(|t| t.selected)
            .map(|t| t.label.clone())
            .collect()
    };

    // The billing grid marks it, because that is where the cart is.
    let grid = crate::ipc::open_orders_on(&app).expect("the grid");
    assert_eq!(marked(&grid), ["3"]);

    // The floor plan does not.
    let plan = floor_on(&app).expect("the plan");
    assert!(
        marked(&plan.tiles).is_empty(),
        "the floor plan is ringing the billing screen's table: {:?}",
        marked(&plan.tiles),
    );
}

/// Several tables at once, and all or nothing.
#[test]
fn a_bulk_delete_takes_all_the_tables_or_none_of_them() {
    let scratch = Scratch::new("bulk_delete");
    let app = a_shop_with_a_room(&scratch);

    // Four tables; table 2 has an order sitting on it.
    seat(&app, "ord_busy", "tbl_2", &[("itm_tea", 2_000, 1)], None);

    let refused = crate::floor::delete_tables_on(
        &app,
        vec!["tbl_1".to_owned(), "tbl_2".to_owned(), "tbl_3".to_owned()],
    )
    .expect_err("a busy table was deleted");
    assert_eq!(refused.code, "db.failed");
    assert!(
        refused.detail.unwrap_or_default().contains("open order"),
        "the refusal must name what stopped it",
    );

    let floor = floor_on(&app).expect("the floor");
    assert_eq!(floor.tables.len(), 4, "part of the set was deleted anyway");

    // Without the busy one, all three go in one command.
    crate::floor::delete_tables_on(
        &app,
        vec!["tbl_1".to_owned(), "tbl_3".to_owned(), "tbl_4".to_owned()],
    )
    .expect("three free tables");
    let floor = floor_on(&app).expect("the floor");
    assert_eq!(
        floor
            .tables
            .iter()
            .map(|t| t.label.as_str())
            .collect::<Vec<_>>(),
        ["2"],
    );
}

/// Hiding is the same bargain, and it is what a table with history gets instead of a delete.
#[test]
fn a_bulk_hide_takes_them_off_the_floor_and_keeps_their_history() {
    let scratch = Scratch::new("bulk_hide");
    let app = a_shop_with_a_room(&scratch);

    crate::floor::set_tables_active_on(&app, vec!["tbl_1".to_owned(), "tbl_2".to_owned()], false)
        .expect("two off the floor");

    let floor = floor_on(&app).expect("the floor");
    let off: Vec<&str> = floor
        .tables
        .iter()
        .filter(|t| !t.is_active)
        .map(|t| t.label.as_str())
        .collect();
    assert_eq!(off, ["1", "2"]);
    assert_eq!(floor.tables.len(), 4, "hiding is not deleting");

    // And a hidden table is not on the billing grid, which is the point of it.
    let grid = crate::ipc::open_orders_on(&app).expect("the grid");
    assert_eq!(
        grid.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(),
        ["3", "4"],
    );

    // Back again, same command.
    crate::floor::set_tables_active_on(&app, vec!["tbl_1".to_owned()], true).expect("put back");
    let floor = floor_on(&app).expect("the floor");
    assert_eq!(floor.tables.iter().filter(|t| !t.is_active).count(), 1);
}

/// `can_arrange` is the same question the commands ask, answered once for the screen — and it
/// is a courtesy, not the control.
#[test]
fn arranging_the_room_needs_the_permission_and_says_so_before_the_press() {
    let scratch = Scratch::new("can_arrange");
    let app = a_shop_with_a_room(&scratch);
    crate::signin_tests::hire(&app, "staff_boss", "Meena", RolePreset::Owner);
    crate::signin_tests::hire(&app, "staff_waiter", "Priya", RolePreset::Waiter);

    crate::ipc::set_staff_pin_on(&app, "staff_boss".to_owned(), Some("2468".to_owned()))
        .expect("pin");
    crate::ipc::login_on(&app, "staff_boss".to_owned(), "2468".to_owned()).expect("signed in");
    assert!(floor_on(&app).expect("the floor").can_arrange);

    crate::ipc::set_staff_pin_on(&app, "staff_waiter".to_owned(), Some("1357".to_owned()))
        .expect("pin");
    crate::ipc::lock_now_on(&app).expect("locked");
    crate::ipc::login_on(&app, "staff_waiter".to_owned(), "1357".to_owned()).expect("Priya");

    let floor = floor_on(&app).expect("a waiter can still see the floor");
    assert!(
        !floor.can_arrange,
        "a waiter was offered the arranging panel"
    );

    // And the panel being hidden is not what stops them.
    for refused in [
        crate::floor::delete_tables_on(&app, vec!["tbl_1".to_owned()]),
        crate::floor::set_tables_active_on(&app, vec!["tbl_1".to_owned()], false),
        crate::floor::save_thresholds_on(&app, 5, 10),
    ] {
        assert_eq!(
            refused.expect_err("a waiter arranged the room").code,
            "auth.denied",
        );
    }
}

/// A second party sits beside the first, gets its own tile, and pressing the table still opens
/// the first.
#[test]
fn a_second_party_gets_its_own_tile_and_the_table_keeps_its_first() {
    let scratch = Scratch::new("join_seat");
    let app = a_shop_with_a_room(&scratch);
    let first = seat(&app, "ord_first", "tbl_1", &[("itm_dosa", 12_000, 2)], None);

    // Typed at the counter, then put beside the first party as 1B.
    app.with_cart_mut(|state| {
        state
            .cart
            .add(
                snapshot("itm_tea", 2_000),
                Qty::from_whole(1).expect("qty"),
                None,
                Vec::new(),
            )
            .expect("added");
        Ok(())
    })
    .expect("a tea");
    let view =
        crate::ipc::join_table_on(&app, "tbl_1".to_owned(), Some("b".to_owned())).expect("seated");
    assert_eq!(view.table.as_deref(), Some("1B"));
    assert_eq!(view.lines.len(), 1, "the typed line came along");
    let second = crate::flows::park_open_order(&app).expect("parked");
    assert_eq!(second.core.seat().map(|s| s.as_str()), Some("B"));

    let tiles = crate::ipc::open_orders_on(&app).expect("the floor");
    let seat_tile = tiles
        .iter()
        .find(|t| t.label == "1B")
        .expect("a tile for 1B");
    assert_eq!(
        seat_tile.id,
        second.core.id.as_str(),
        "a seat's tile is its order"
    );
    let table_tile = tiles
        .iter()
        .find(|t| t.label == "1")
        .expect("the table's tile");
    assert_eq!(table_tile.id, "tbl_1");
    assert_eq!(table_tile.order_id.as_deref(), Some(first.as_str()));

    // The same letter twice is refused; pressing the table opens the first party.
    let refused = crate::ipc::join_table_on(&app, "tbl_1".to_owned(), Some("B".to_owned()))
        .expect_err("two parties on one letter");
    assert_eq!(refused.code, "table.seat_taken");
    let opened = crate::ipc::open_table_on(&app, "tbl_1".to_owned()).expect("opened");
    assert_eq!(opened.order_id.as_deref(), Some(first.as_str()));
}

/// A second party in the cart is on the table, but not on the table's own tile.
#[test]
fn only_the_party_in_the_cart_is_marked_never_its_table_as_well() {
    let scratch = Scratch::new("selected_seat");
    let app = a_shop_with_a_room(&scratch);
    seat(&app, "ord_a", "tbl_2", &[("itm_dosa", 12_000, 1)], None);
    let second = a_second_party(&app, "tbl_2");

    crate::ipc::open_order_on(&app, second.as_str().to_owned()).expect("opened 2B");
    let marked = marked_labels(&crate::ipc::open_orders_on(&app).expect("the floor"));
    assert_eq!(marked, ["2B"], "the table's own tile lit up with its second party");

    // And the other way round: the first party marks the table, not the seat.
    crate::ipc::open_table_on(&app, "tbl_2".to_owned()).expect("opened 2");
    let marked = marked_labels(&crate::ipc::open_orders_on(&app).expect("the floor"));
    assert_eq!(marked, ["2"]);
}

/// When the first party has paid, the second keeps its letter and the table is free again.
#[test]
fn a_second_party_keeps_its_letter_after_the_first_has_paid() {
    let scratch = Scratch::new("seat_outlives");
    let app = a_shop_with_a_room(&scratch);
    let second = a_second_party(&app, "tbl_2");

    let tiles = crate::ipc::open_orders_on(&app).expect("the floor");
    let table = tiles.iter().find(|t| t.id == "tbl_2").expect("table 2's own tile");
    assert_eq!(table.label, "2");
    assert_eq!(
        table.state,
        crate::billing::TableState::Free,
        "the second party was drawn as the table's own order"
    );
    assert!(table.order_id.is_none());
    let party = tiles.iter().find(|t| t.id == second.as_str()).expect("2B's tile");
    assert_eq!(party.label, "2B", "the letter was lost");
    assert_eq!(party.order_id.as_deref(), Some(second.as_str()));

    // Pressing the table starts a new first party; the second is untouched.
    let opened = crate::ipc::open_table_on(&app, "tbl_2".to_owned()).expect("opened");
    assert!(opened.order_id.is_none());
    assert_eq!(opened.table.as_deref(), Some("2"));
}

/// A party with a letter, parked with one line — the + on the tile, then a ticket.
fn a_second_party(app: &App, table: &str) -> OrderId {
    crate::ipc::join_table_on(app, table.to_owned(), None).expect("seated");
    app.with_cart_mut(|state| {
        state
            .cart
            .add(
                snapshot("itm_tea", 2_000),
                Qty::from_whole(1).expect("qty"),
                None,
                Vec::new(),
            )
            .expect("added");
        Ok(())
    })
    .expect("a tea");
    let parked = crate::flows::park_open_order(app).expect("parked");
    app.with_cart_mut(|state| {
        *state = crate::billing::CartState::default();
        Ok(())
    })
    .expect("cleared");
    parked.core.id
}

fn marked_labels(tiles: &[crate::billing::TableView]) -> Vec<String> {
    tiles
        .iter()
        .filter(|t| t.selected)
        .map(|t| t.label.clone())
        .collect()
}

/// Typed items join a busy table's bill without being written until the counter parks them.
#[test]
fn typed_items_join_the_bill_on_a_busy_table() {
    let scratch = Scratch::new("join_merge");
    let app = a_shop_with_a_room(&scratch);
    let first = seat(
        &app,
        "ord_busy",
        "tbl_2",
        &[("itm_dosa", 12_000, 2)],
        Some(2),
    );
    app.with_cart_mut(|state| {
        state
            .cart
            .add(
                snapshot("itm_tea", 2_000),
                Qty::from_whole(3).expect("qty"),
                None,
                Vec::new(),
            )
            .expect("added");
        Ok(())
    })
    .expect("a tea");

    // Typing the table's number is enough: there is no question to answer.
    let view = crate::ipc::open_table_on(&app, "tbl_2".to_owned()).expect("joined");
    assert_eq!(view.order_id.as_deref(), Some(first.as_str()));
    assert_eq!(view.lines.len(), 2, "the dosa and the tea");
    assert!(!view.kitchen_up_to_date, "the tea is new to the kitchen");
    assert_eq!(
        read(&app, &first).core().cart.lines().len(),
        1,
        "nothing was written yet"
    );
}

/// Typed items go WITH the cashier to a free table — they were never on the floor to lose.
#[test]
fn typed_items_go_with_the_cashier_to_a_free_table() {
    let scratch = Scratch::new("open_free_typed");
    let app = a_shop_with_a_room(&scratch);
    app.with_cart_mut(|state| {
        state
            .cart
            .add(
                snapshot("itm_tea", 2_000),
                Qty::from_whole(2).expect("qty"),
                None,
                Vec::new(),
            )
            .expect("added");
        Ok(())
    })
    .expect("a tea");

    let view = crate::ipc::open_table_on(&app, "tbl_3".to_owned()).expect("opened");
    assert_eq!(view.table.as_deref(), Some("3"));
    assert_eq!(view.lines.len(), 1, "the tea came along");
    assert!(view.order_id.is_none(), "and nothing was written yet");
}

/// The + on a busy table: each press is the next free letter, and a parked order never moves.
#[test]
fn the_next_free_letter_is_chosen_when_none_is_given() {
    let scratch = Scratch::new("join_next");
    let app = a_shop_with_a_room(&scratch);
    let first = seat(&app, "ord_first", "tbl_1", &[("itm_dosa", 12_000, 1)], None);

    // The first party is A, so the first press is B.
    let b = crate::ipc::join_table_on(&app, "tbl_1".to_owned(), None).expect("seated");
    assert_eq!(b.table.as_deref(), Some("1B"));
    assert!(b.is_empty, "a new party starts from nothing");
    app.with_cart_mut(|state| {
        state
            .cart
            .add(
                snapshot("itm_tea", 2_000),
                Qty::from_whole(1).expect("qty"),
                None,
                Vec::new(),
            )
            .expect("added");
        Ok(())
    })
    .expect("a tea");
    let parked_b = crate::flows::park_open_order(&app).expect("parked");
    assert_eq!(parked_b.core.seat().map(|s| s.as_str()), Some("B"));

    // With B taken and B's order still open in the cart, the next press is C — and B stays B.
    let c = crate::ipc::join_table_on(&app, "tbl_1".to_owned(), None).expect("seated");
    assert_eq!(c.table.as_deref(), Some("1C"));
    assert!(c.order_id.is_none(), "the parked order was not carried to the new seat");
    let still_b = read(&app, &parked_b.core.id);
    assert_eq!(still_b.core().seat().map(|s| s.as_str()), Some("B"));

    // Pressing the table itself still opens the first party.
    let opened = crate::ipc::open_table_on(&app, "tbl_1".to_owned()).expect("opened");
    assert_eq!(opened.order_id.as_deref(), Some(first.as_str()));
}
