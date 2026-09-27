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
use crate::signin_tests::{Scratch, a_slip_said, slips_taken};
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
                    tax_class_id: mb_core::seeded_placement(mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%"))).expect("a seeded slab").0,
                    price_basis: mb_core::seeded_placement(mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%"))).expect("a seeded slab").1,
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
    seat_on_day(app, id, table, items, told, day())
}

fn seat_on_day(
    app: &App,
    id: &str,
    table: &str,
    items: &[(&str, i64, i64)],
    told: Option<i64>,
    business_day: BusinessDay,
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
        business_day,
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
                    business_day,
                )?;
                repos.orders().save(
                    OUTLET,
                    crate::terminals::TERMINAL,
                    &AnyOrder::Open(mb_core::OpenOrder {
                        core: draft.core.clone(),
                        token,
                        bill_number: None,
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

    // The absorbed order keeps serving its table, linked to the combined bill.
    let absorbed = read(&app, &five);
    assert!(
        matches!(absorbed, AnyOrder::Open(_)),
        "the source table remains occupied for service"
    );
    assert_eq!(
        absorbed.bill_number(),
        None,
        "no bill was ever made for it, so it spent no number"
    );

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

    // Table 2 stays occupied until the combined bill is paid and service is released.
    let floor = floor_on(&app).expect("the floor");
    let two = floor
        .tiles
        .iter()
        .find(|t| t.label == "2")
        .expect("table 2");
    assert_eq!(two.order_id.as_deref(), Some(five.as_str()));
    assert_eq!(two.billed_into.as_deref(), Some(four.as_str()));
}

#[test]
fn splitting_gives_the_new_order_its_own_token_and_the_right_ledger() {
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
    assert_ne!(fresh.token().map(|c| c.value), kept.token().map(|c| c.value), "two orders, two tokens");
    assert_eq!(
        (fresh.bill_number(), kept.bill_number()),
        (None, None),
        "neither has been billed, so neither has a bill number yet"
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
    let (said, slips) = slips_taken(&app, || {
        crate::flows::print_open_bill_on(&app, order.as_str().to_owned())
    });
    let said = said.expect("the bill printed");

    // What the SHOP calls the table.
    assert_eq!(said, "The bill for table 3 is printing.", "{said}");
    assert!(
        !said.contains("tbl_"),
        "the toast shows a database id: {said}"
    );

    // Paper. And marked, so it can never be mistaken for a paid bill.
    assert!(
        a_slip_said(&slips, "bill to the table"),
        "nothing reached the printer: {slips:?}"
    );

    // And nothing else. Same state, same table — a settled order here would mean the button
    // closed a table that had not paid, which is the failure worth having a test for. And no
    // bill number: that is born when the bill is paid, so the paper carried to the table
    // shows the token, and a print, or two, spends nothing from the bill book.
    let after = read(&app, &order);
    assert!(
        matches!(after, AnyOrder::Open(_)),
        "printing the bill settled the order"
    );
    let (AnyOrder::Open(before), AnyOrder::Open(after)) = (&before, &after) else {
        panic!("the order stopped being open");
    };
    assert_eq!(
        before.bill_number, None,
        "a number before any bill was made"
    );
    assert_eq!(
        after.bill_number, None,
        "printing the bill spent a bill number before it was paid"
    );
    crate::flows::print_open_bill_on(&app, order.as_str().to_owned()).expect("printed again");
    let AnyOrder::Open(again) = read(&app, &order) else {
        panic!("the order stopped being open");
    };
    assert_eq!(
        again.bill_number, None,
        "a second print of the bill spent a bill number"
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
    assert!(two.created_at.is_none());
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
    assert_eq!(three.created_at, Some(at(1).millis()));
    let json = serde_json::to_value(three).expect("tile JSON");
    assert_eq!(json["createdAt"].as_i64(), Some(at(1).millis()));

    crate::ipc::open_table_on(&app, "tbl_3".to_owned()).expect("opened");

    let after = crate::ipc::open_orders_on(&app).expect("the floor");
    let three = after.iter().find(|t| t.label == "3").expect("table 3");
    assert!(three.selected, "the open table is not marked");
    assert_eq!(three.created_at, Some(at(1).millis()));
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

/// Several tables at once: the ones that can go, go, and the one that cannot is NAMED rather
/// than taking the others down with it.
#[test]
fn a_bulk_delete_keeps_the_table_it_cannot_take_and_deletes_the_rest() {
    let scratch = Scratch::new("bulk_delete");
    let app = a_shop_with_a_room(&scratch);

    // Four tables; table 2 has an order sitting on it.
    seat(&app, "ord_busy", "tbl_2", &[("itm_tea", 2_000, 1)], None);

    let change = crate::floor::delete_tables_on(
        &app,
        vec!["tbl_1".to_owned(), "tbl_2".to_owned(), "tbl_3".to_owned()],
    )
    .expect("the two free tables");
    assert_eq!(change.kept.len(), 1, "{:?}", change.kept);
    let kept = &change.kept[0];
    assert!(
        kept.contains("Hall 2") && kept.contains("open order"),
        "the line must name the table and why: {kept}",
    );
    // The sentence is Rust's, and it counts both halves.
    assert!(
        change.said.contains("2 tables") && change.said.contains("1 table"),
        "{}",
        change.said,
    );

    // Table 2 is still standing; the other two are gone.
    assert_eq!(
        change
            .floor
            .tables
            .iter()
            .map(|t| t.label.as_str())
            .collect::<Vec<_>>(),
        ["2", "4"],
    );

    // And a set with nothing in the way keeps nothing back.
    let change =
        crate::floor::delete_tables_on(&app, vec!["tbl_4".to_owned()]).expect("one free table");
    assert!(change.kept.is_empty());
    assert_eq!(change.said, "1 table deleted.");
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

    // Back again, same command. Putting a table back is never refused.
    let change =
        crate::floor::set_tables_active_on(&app, vec!["tbl_1".to_owned()], true).expect("put back");
    assert!(change.kept.is_empty());
    assert_eq!(change.said, "1 table put back.");
    assert_eq!(
        change.floor.tables.iter().filter(|t| !t.is_active).count(),
        1,
    );
}

/// A table somebody is sitting at cannot be taken off the floor — and the other three still go.
#[test]
fn a_bulk_hide_keeps_the_busy_table_and_takes_the_rest_off() {
    let scratch = Scratch::new("bulk_hide_busy");
    let app = a_shop_with_a_room(&scratch);
    seat(&app, "ord_busy", "tbl_2", &[("itm_tea", 2_000, 1)], None);

    let change = crate::floor::set_tables_active_on(
        &app,
        vec!["tbl_1".to_owned(), "tbl_2".to_owned(), "tbl_3".to_owned()],
        false,
    )
    .expect("the two free tables");
    assert_eq!(change.kept.len(), 1, "{:?}", change.kept);
    assert!(change.kept[0].contains("Hall 2"), "{}", change.kept[0]);
    assert!(
        change.said.contains("taken off the floor"),
        "{}",
        change.said,
    );
    // Table 2 kept its tile; the order on it is untouched.
    assert_eq!(
        change
            .floor
            .tiles
            .iter()
            .map(|t| t.label.as_str())
            .collect::<Vec<_>>(),
        ["2", "4"],
    );
}

/// The billing grid and the floor screen group their tiles by the SAME room order — the shop's,
/// not the alphabet's. The tile carries the room's place so both can.
#[test]
fn a_tile_carries_the_place_its_room_was_put_in() {
    let scratch = Scratch::new("room_order");
    let app = a_shop_with_a_room(&scratch);

    // A second room, deliberately named so the alphabet and the shop's order disagree.
    crate::floor::save_section_on(&app, "sec_ac".to_owned(), "AC".to_owned(), 1, true)
        .expect("a second room");
    crate::floor::save_table_on(
        &app,
        crate::floor::TableEdit {
            id: "tbl_ac".to_owned(),
            label: "9".to_owned(),
            section_id: Some("sec_ac".to_owned()),
            seats: 4,
            is_active: true,
        },
    )
    .expect("a table in it");

    let grid = crate::ipc::open_orders_on(&app).expect("the grid");
    let place = |label: &str| {
        grid.iter()
            .find(|t| t.label == label)
            .and_then(|t| t.section_order)
    };
    // Hall was made first, so Hall comes first — even though "AC" sorts before it.
    assert_eq!(place("1"), Some(0), "Hall lost its place");
    assert_eq!(place("9"), Some(1), "AC lost its place");
}

/// `can_arrange` is the same question the commands ask, answered once for the screen — and it
/// is a courtesy, not the control.
#[test]
fn arranging_the_room_needs_the_permission_and_says_so_before_the_press() {
    let scratch = Scratch::new("can_arrange");
    let app = a_shop_with_a_room(&scratch);
    crate::signin_tests::hire(&app, "staff_boss", "Meena", RolePreset::Owner, "2468");
    crate::signin_tests::hire(&app, "staff_waiter", "Priya", RolePreset::Waiter, "1357");

    crate::ipc::login_on(&app, "staff_boss".to_owned(), "2468".to_owned()).expect("signed in");
    assert!(floor_on(&app).expect("the floor").can_arrange);

    crate::ipc::lock_now_on(&app).expect("locked");
    crate::ipc::login_on(&app, "staff_waiter".to_owned(), "1357".to_owned()).expect("Priya");

    let floor = floor_on(&app).expect("a waiter can still see the floor");
    assert!(
        !floor.can_arrange,
        "a waiter was offered the arranging panel"
    );

    // And the panel being hidden is not what stops them. The three answer with different
    // views, so each is asked for its error alone.
    for refused in [
        crate::floor::delete_tables_on(&app, vec!["tbl_1".to_owned()]).err(),
        crate::floor::set_tables_active_on(&app, vec!["tbl_1".to_owned()], false).err(),
        crate::floor::save_thresholds_on(&app, 5, 10).err(),
    ] {
        assert_eq!(
            refused.expect("a waiter arranged the room").code,
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

/// The room reads "2, 2B, 2C, 3": the table's own tile, then its parties in letter order —
/// never a letter before its number.
#[test]
fn a_table_comes_before_its_own_parties() {
    let scratch = Scratch::new("party_order");
    let app = a_shop_with_a_room(&scratch);
    seat(&app, "ord_a", "tbl_2", &[("itm_dosa", 12_000, 1)], None);
    a_second_party(&app, "tbl_2");
    // The next free letter is C.
    a_second_party(&app, "tbl_2");

    let labels: Vec<String> = crate::ipc::open_orders_on(&app)
        .expect("the floor")
        .iter()
        .map(|t| t.label.clone())
        .collect();
    let two = labels.iter().position(|l| l == "2").expect("table 2");
    assert_eq!(&labels[two..two + 4], ["2", "2B", "2C", "3"]);
}

/// Pressing + puts the cart on 2B before anything is typed or sent. The grid shows that party
/// at once, drawn from where the cart is, and drops it again when the cart moves on.
#[test]
fn the_party_just_opened_has_a_tile_before_it_is_saved() {
    let scratch = Scratch::new("party_unsaved");
    let app = a_shop_with_a_room(&scratch);
    seat(&app, "ord_a", "tbl_2", &[("itm_dosa", 12_000, 1)], None);

    crate::ipc::join_table_on(&app, "tbl_2".to_owned(), None).expect("2B in the cart");
    let tiles = crate::ipc::open_orders_on(&app).expect("the floor");
    let party = tiles.iter().find(|t| t.label == "2B").expect("a tile for 2B");
    assert_eq!(party.state, crate::billing::TableState::Free);
    assert!(party.selected, "the cart is on it");
    assert!(party.order_id.is_none(), "nothing has been saved");
    assert_eq!(party.seat.as_deref(), Some("B"));
    assert_eq!(party.id, "tbl_2", "pressing it re-joins the table");
    assert_eq!(marked_labels(&tiles), ["2B"], "the table's own tile stays unmarked");

    // The cart leaves: so does the tile.
    crate::ipc::cart_clear_on(&app, true).expect("new order");
    let tiles = crate::ipc::open_orders_on(&app).expect("the floor");
    assert!(tiles.iter().all(|t| t.label != "2B"), "2B outlived the cart");
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

#[test]
fn splitting_without_a_letter_allocates_a_visible_party_and_refuses_collisions() {
    let scratch = Scratch::new("split_auto_seat");
    let app = a_shop_with_a_room(&scratch);
    let order = seat(&app, "ord_split_auto", "tbl_1", &[("itm_dosa", 12_000, 4)], None);
    for expected in ["1B", "1C"] {
        let floor = split_order_on(&app, SplitRequest {
            order_id: order.as_str().to_owned(), lines: vec![(0, "1".to_owned())],
            to_table: None, seat: None,
        }).expect("split");
        assert!(floor.tiles.iter().any(|tile| tile.label == expected && tile.order_id.is_some()));
    }
    let before = read(&app, &order);
    split_order_on(&app, SplitRequest {
        order_id: order.as_str().to_owned(), lines: vec![(0, "1".to_owned())],
        to_table: None, seat: Some("B".to_owned()),
    }).expect_err("occupied seat");
    assert_eq!(read(&app, &order), before, "a refused split changes nothing");
}

#[test]
fn a_combined_bill_can_join_another_without_stranding_its_serving_tables() {
    let scratch = Scratch::new("nested_merge");
    let app = a_shop_with_a_room(&scratch);
    let a = seat(&app, "ord_a", "tbl_1", &[("itm_dosa", 12_000, 1)], None);
    let b = seat(&app, "ord_b", "tbl_2", &[("itm_tea", 2_000, 1)], None);
    let c = seat(&app, "ord_c", "tbl_3", &[("itm_tea", 2_000, 1)], None);
    merge_orders_on(&app, a.as_str().to_owned(), b.as_str().to_owned()).expect("first combine");
    merge_orders_on(&app, b.as_str().to_owned(), c.as_str().to_owned()).expect("second combine");
    for source in [&a, &b] {
        assert_eq!(read(&app, source).core().billing.billed_into.as_ref(), Some(&c));
    }
    let combined = read(&app, &c);
    assert_eq!(combined.core().billing.sources.len(), 2);
    assert_eq!(combined.core().billing.serving.len(), 2);
    crate::floor::release_serving_table_on(&app, a.as_str().to_owned()).expect_err("not paid yet");
    crate::ipc::open_order_on(&app, c.as_str().to_owned()).expect("open combined bill");
    crate::flows::complete_bill_on(&app, Some("cash".to_owned())).expect("paid");
    let paid_floor = floor_on(&app).expect("paid floor");
    let root_service = paid_floor.tiles.iter().find(|tile| tile.label == "3").expect("root table");
    assert_eq!(root_service.billed_into.as_deref(), Some(c.as_str()));
    let root_service_id = root_service.order_id.clone().expect("root table remains occupied");
    for source in [&a, &b] {
        crate::floor::release_serving_table_on(&app, source.as_str().to_owned()).expect("released");
        assert!(matches!(read(&app, source), AnyOrder::Cancelled(_)));
    }
    crate::floor::release_serving_table_on(&app, root_service_id).expect("root table released");
    assert!(floor_on(&app).expect("released floor").tiles.iter().all(|tile| tile.order_id.is_none()));
}

#[test]
fn moving_a_loaded_order_refreshes_the_saved_baseline() {
    let scratch = Scratch::new("move_loaded");
    let app = a_shop_with_a_room(&scratch);
    let id = seat(&app, "ord_move_loaded", "tbl_1", &[("itm_dosa", 12_000, 1)], None);
    crate::ipc::open_order_on(&app, id.as_str().to_owned()).expect("loaded");
    app.with_cart_mut(|state| {
        state.cart.add(snapshot("itm_tea", 2_000), Qty::from_whole(1).expect("one"), None, vec![]).expect("tea");
        Ok(())
    }).expect("typed");
    move_order_on(&app, id.as_str().to_owned(), "tbl_2".to_owned()).expect("moved");
    let parked = crate::flows::park_open_order(&app).expect("save after move has no stale conflict");
    assert_eq!(parked.core.table().map(TableId::as_str), Some("tbl_2"));
    assert_eq!(parked.core.cart.len(), 2, "typed food survives the move");
}

#[test]
fn combining_an_already_paid_combination_retargets_its_service_only_order() {
    let scratch = Scratch::new("recombine_paid");
    let app = a_shop_with_a_room(&scratch);
    crate::signin_tests::hire(&app, "staff_owner", "Owner", RolePreset::Owner, "2468");
    let mut config = app.shop_config();
    config.billing.kitchen_screen = true;
    app.publish_shop_config(config);
    let a = seat(&app, "ord_a", "tbl_1", &[("itm_dosa", 12_000, 1)], None);
    let b = seat(&app, "ord_b", "tbl_2", &[("itm_tea", 2_000, 1)], None);
    crate::kitchen::send(&app, a.as_str(), None).expect("first table's kitchen ticket");
    crate::kitchen::send(&app, b.as_str(), None).expect("root table's kitchen ticket");
    let before = crate::kitchen::look(&app, crate::kitchen::DEFAULT_STATION);
    let root_ticket = before.tickets.iter().find(|ticket| ticket.order_id == b.as_str()).expect("root ticket");
    let root_ticket_id = root_ticket.id.clone();
    let root_token = root_ticket.token.clone();
    merge_orders_on(&app, a.as_str().to_owned(), b.as_str().to_owned()).expect("combine");
    crate::ipc::open_order_on(&app, b.as_str().to_owned()).expect("load");
    crate::flows::complete_bill_on(&app, Some("cash".to_owned())).expect("first payment");
    let first_floor = floor_on(&app).expect("serving floor");
    let service = first_floor.tiles.iter().find(|tile| tile.label == "2")
        .and_then(|tile| tile.order_id.clone()).expect("service order");
    assert_ne!(service, b.as_str());
    let after = crate::kitchen::look(&app, crate::kitchen::DEFAULT_STATION);
    let root_ticket = after.tickets.iter().find(|ticket| ticket.id == root_ticket_id).expect("same kitchen ticket");
    assert_eq!(root_ticket.order_id, service, "kitchen follows the serving order");
    assert_eq!(root_ticket.token, root_token, "payment does not rename the cook's ticket");
    app.with_shop(|shop| shop.db.transaction(|tx| {
        assert!(Repos::new(tx).kitchen().courses_fired(&service)?.everything, "settling must not make courses unfired");
        Ok(())
    }).map_err(|error| crate::words::from_db(&error))).expect("course retained");
    let c = seat(&app, "ord_c", "tbl_3", &[("itm_tea", 2_000, 1)], None);
    crate::kitchen::send(&app, c.as_str(), None).expect("new root's kitchen ticket");
    merge_orders_on(&app, b.as_str().to_owned(), c.as_str().to_owned()).expect("combine paid root again");
    assert_eq!(read(&app, &OrderId::new(&service)).core().billing.billed_into.as_ref(), Some(&c));
    crate::ipc::open_order_on(&app, c.as_str().to_owned()).expect("load survivor");
    crate::flows::complete_bill_on(&app, Some("cash".to_owned())).expect("pay remainder");
    let occupied = floor_on(&app).expect("all serving tables");
    assert_eq!(occupied.tiles.iter().filter(|tile| tile.order_id.is_some()).count(), 3);
    for tile in occupied.tiles.into_iter().filter(|tile| tile.order_id.is_some()) {
        assert_eq!(tile.billed_into.as_deref(), Some(c.as_str()));
        crate::floor::release_serving_table_on(&app, tile.order_id.expect("occupied")).expect("released");
    }
    assert!(floor_on(&app).expect("free floor").tiles.iter().all(|tile| tile.order_id.is_none()));
    assert!(crate::kitchen::look(&app, crate::kitchen::DEFAULT_STATION).tickets.is_empty(), "releasing all tables closes their original kitchen work");
}

#[test]
fn candidates_include_each_bill_from_the_same_table() {
    let scratch = Scratch::new("combine_candidates");
    let app = a_shop_with_a_room(&scratch);
    crate::signin_tests::hire(&app, "staff_owner", "Owner", RolePreset::Owner, "2468");
    for id in ["ord_first", "ord_second"] {
        // Both represent separate visits to one physical table.
        seat_on_day(&app, id, "tbl_1", &[("itm_dosa", 12_000, 1)], None,
            crate::flows::today(crate::flows::now()));
        crate::ipc::open_order_on(&app, id.to_owned()).expect("open visit");
        crate::flows::complete_bill_on(&app, Some("cash".to_owned())).expect("paid visit");
    }
    let candidates = crate::floor::combine_candidates_on(&app).expect("candidates");
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().any(|tile| tile.id == "ord_first"));
    assert!(candidates.iter().any(|tile| tile.id == "ord_second"));
    assert!(candidates.iter().all(|tile| tile.bill_number.is_some()));
    merge_orders_on(&app, "ord_first".to_owned(), "ord_second".to_owned()).expect("combine paid visits");
    let candidates = crate::floor::combine_candidates_on(&app).expect("working candidates");
    assert_eq!(candidates.len(), 1, "an absorbed paid source is not offered while the correction is pending");
    assert_eq!(candidates[0].order_id.as_deref(), Some("ord_second"));
}

#[test]
fn moving_a_correction_keeps_the_issued_bill_and_updates_only_its_draft() {
    let scratch = Scratch::new("move_correction");
    let app = a_shop_with_a_room(&scratch);
    crate::signin_tests::hire(&app, "staff_owner", "Owner", RolePreset::Owner, "2468");
    let id = seat(&app, "ord_move_correction", "tbl_1", &[("itm_dosa", 12_000, 1)], None);
    crate::ipc::open_order_on(&app, id.as_str().to_owned()).expect("loaded");
    crate::flows::complete_bill_on(&app, Some("cash".to_owned())).expect("paid");
    let issued = read(&app, &id);
    crate::corrections::revert_bill_on(&app, id.as_str().to_owned(), "Correct table".to_owned(), None, None).expect("correction");
    move_order_on(&app, id.as_str().to_owned(), "tbl_3".to_owned()).expect("moved correction");
    assert_eq!(read(&app, &id), issued, "reports retain the issued sale");
    let working = crate::flows::find_order(&app, &id).expect("working").expect("draft");
    assert_eq!(working.core().table().map(TableId::as_str), Some("tbl_3"));
    crate::flows::park_open_order(&app).expect("draft baseline is current");
    let other = seat(&app, "ord_other_move", "tbl_4", &[("itm_tea", 2_000, 1)], None);
    let refused = move_order_on(&app, other.as_str().to_owned(), "tbl_3".to_owned()).expect_err("correction draft occupies target");
    assert_eq!(refused.code, "floor.table_busy");
    assert_eq!(read(&app, &other).core().table().map(TableId::as_str), Some("tbl_4"));
}

#[test]
fn splitting_sent_food_preserves_kitchen_station_course_and_completion() {
    use mb_core::kitchen_delivery::{Delivery, State};
    let scratch = Scratch::new("split_kitchen_work");
    let app = a_shop_with_a_room(&scratch);
    let id = seat(&app, "ord_split_kds", "tbl_1", &[("itm_dosa", 12_000, 2), ("itm_tea", 2_000, 2)], None);
    let mut order = read(&app, &id);
    let mut cart = Cart::new();
    for (index, source) in order.core().cart.lines().iter().enumerate() {
        let mut line = source.clone();
        line.snapshot.category_id = Some(mb_core::CategoryId::new(if index == 0 { "cat_hot" } else { "cat_cold" }));
        line.snapshot.course = Some(if index == 0 { "Main" } else { "Drinks" }.to_owned());
        cart.push(line).expect("line");
    }
    order.core_mut().cart = cart;
    let pending = order.core().kitchen.pending(&order.core().cart).expect("pending");
    order.core_mut().kitchen.mark_printed(&pending).expect("sent");
    app.with_shop(|shop| shop.db.transaction(|tx| {
        let repos = Repos::new(tx);
        for (id, station) in [("cat_hot", "Hot"), ("cat_cold", "Cold")] {
            repos.menu().save_category(OUTLET, &mb_db::repo::menu::Category {
                id: mb_core::CategoryId::new(id), name: station.to_owned(), sort_order: 0,
                is_active: true, station: Some(station.to_owned()), default_tax_class_id: None,
            }, at(1))?;
        }
        repos.orders().save(OUTLET, app.terminal_id(), &order)?;
        repos.devices().pair(OUTLET, &mb_db::repo::devices::LanDevice {
            id: "screen_kitchen".to_owned(), name: "Kitchen screen".to_owned(),
            platform: "android".to_owned(), secret_hash: "fixture-only".to_owned(),
            staff_id: None, paired_at: at(1), paired_by: None, last_seen_at: None,
            last_ip: None, revoked_at: None, install_id: None,
        }, None)?;
        for (ticket_id, station, course, state, item) in [
            ("kds_hot", "Hot", "Main", State::Shown, "itm_dosa"),
            ("kds_cold", "Cold", "Drinks", State::Bumped, "itm_tea"),
        ] {
            let mut delivery = Delivery::new(ticket_id, id.as_str(), station, at(2));
            delivery.state = state;
            delivery.shown_at = Some(at(3));
            delivery.bumped_at = (state == State::Bumped).then_some(at(4));
            repos.kitchen().send(OUTLET, &delivery, Some(course), Some(7), day())?;
            let mut ticket = repos.kitchen().get(ticket_id)?.expect("ticket");
            ticket.bumped_lines.push(item.to_owned());
            ticket.bumped_by = Some(StaffId::new(crate::state::DEFAULT_STAFF));
            ticket.bumped_on = Some("screen_kitchen".to_owned());
            repos.kitchen().save(&ticket)?;
        }
        Ok(())
    }).map_err(|error| crate::words::from_db(&error))).expect("kitchen work");
    let (result, paper) = slips_taken(&app, || split_order_on(&app, SplitRequest {
        order_id: id.as_str().to_owned(), lines: vec![(0, "1".to_owned()), (1, "2".to_owned())],
        to_table: None, seat: None,
    }));
    let floor = result.expect("split");
    assert!(paper.is_empty(), "splitting must not send another cooking slip");
    let split_id = floor.tiles.iter().find(|tile| tile.label == "1B").and_then(|tile| tile.order_id.clone()).expect("split party");
    app.with_shop(|shop| shop.db.transaction(|tx| {
        let repos = Repos::new(tx);
        let copies = repos.kitchen().for_order(&split_id)?;
        assert_eq!(copies.len(), 2);
        for copy in copies {
            let original_id = if copy.delivery.station == "Hot" { "kds_hot" } else { "kds_cold" };
            let original = repos.kitchen().get(original_id)?.expect("original");
            assert_ne!(copy.delivery.id, original.delivery.id);
            assert_eq!(copy.delivery.state, original.delivery.state);
            assert_eq!(copy.delivery.sent_at, original.delivery.sent_at);
            assert_eq!(copy.delivery.shown_at, original.delivery.shown_at);
            assert_eq!(copy.delivery.bumped_at, original.delivery.bumped_at);
            assert_eq!(copy.course, original.course);
            assert_eq!(copy.bumped_by, original.bumped_by);
            assert_eq!(copy.bumped_on, original.bumped_on);
            assert_eq!(copy.bumped_lines, vec![if copy.delivery.station == "Hot" { "itm_dosa" } else { "itm_tea" }.to_owned()]);
        }
        assert!(repos.kitchen().get("kds_cold")?.expect("cold").bumped_lines.is_empty(), "moved identities leave the source ticket");
        Ok(())
    }).map_err(|error| crate::words::from_db(&error))).expect("preserved work");
    let hot = crate::kitchen::look(&app, "Hot");
    assert_eq!(hot.tickets.len(), 2, "each serving party retains its already-sent food");
    assert!(hot.tickets.iter().all(|ticket| ticket.lines.len() == 1 && ticket.lines[0].qty == "1" && ticket.lines[0].is_done));
    assert!(crate::kitchen::look(&app, "Cold").tickets.is_empty(), "finished food is not resurrected");
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
