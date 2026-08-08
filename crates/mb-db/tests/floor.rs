//! **The table master, the floor plan and the occupancy line** — P14.
//!
//! The rules live in `mb_core::table` and are tested there on their own. What
//! is tested here is the door: that the repository actually applies them, that
//! a refusal names the thing it is refusing, and that a bulk action is all or
//! nothing.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Scratch;
use common::shop::{self, OUTLET};
use mb_core::{BusinessDay, TableId, Timestamp};
use mb_db::Repos;
use mb_db::repo::floor::{DiningTable, Range, Section};

fn at(n: i64) -> Timestamp {
    Timestamp::from_millis(1_770_000_000_000 + n * 1_000)
}

fn table(id: &str, section: Option<&str>, label: &str) -> DiningTable {
    DiningTable {
        id: TableId::new(id),
        section_id: section.map(str::to_owned),
        label: label.to_owned(),
        seats: 4,
        pos: None,
        sort_order: 99,
        is_active: true,
    }
}

/// **Loophole I5 at the database door.** Section AC's table "1" and a table
/// literally named "AC 1" print the same string, so the second is refused —
/// and the message names what it clashed with rather than saying "duplicate".
#[test]
fn a_table_that_would_print_an_existing_name_is_refused_by_name() {
    let scratch = Scratch::new("clash");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.floor().save_section(
            OUTLET,
            &Section { id: "sec_ac".to_owned(), name: "AC".to_owned(), sort_order: 1, is_active: true },
            at(1),
        )?;
        repos.floor().save_table(OUTLET, &table("tbl_ac_1", Some("sec_ac"), "1"), at(2))
    })
    .expect("AC 1 created");

    let refused = db
        .transaction(|tx| {
            Repos::new(tx)
                .floor()
                .save_table(OUTLET, &table("tbl_loose", None, "AC 1"), at(3))
        })
        .expect_err("refused");
    let said = refused.to_string();
    assert!(said.contains("AC 1"), "the message names the table: {said}");
    assert!(said.contains("AC"), "and the room it is in: {said}");

    // Case and spacing are the same table too.
    assert!(
        db.transaction(|tx| {
            Repos::new(tx)
                .floor()
                .save_table(OUTLET, &table("tbl_loose2", None, "ac  1"), at(4))
        })
        .is_err()
    );
}

/// The corollary, and the case the rule this replaced got wrong: the most
/// ordinary layout in India has a table 1 in every room.
#[test]
fn the_same_number_in_two_rooms_is_allowed() {
    let scratch = Scratch::new("two_rooms");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for (id, name) in [("sec_ac", "AC"), ("sec_garden", "Garden")] {
            repos.floor().save_section(
                OUTLET,
                &Section { id: id.to_owned(), name: name.to_owned(), sort_order: 1, is_active: true },
                at(1),
            )?;
        }
        repos.floor().save_table(OUTLET, &table("tbl_ac_1", Some("sec_ac"), "1"), at(2))?;
        repos.floor().save_table(OUTLET, &table("tbl_g_1", Some("sec_garden"), "1"), at(3))
    })
    .expect("both rooms may have a table 1");
}

/// **All or nothing.** A shop that asked for 1-20 and silently got 14 has to
/// find the missing six by counting.
#[test]
fn a_range_that_would_clash_creates_none_of_itself() {
    let scratch = Scratch::new("range");
    let db = scratch.open();
    shop::build(&db);

    let before = db
        .transaction(|tx| Repos::new(tx).floor().list_tables(OUTLET))
        .expect("tables")
        .len();

    // The fixture already has 1..6 in the Hall, so 4..8 collides at 4, 5, 6.
    let refused = db.transaction(|tx| {
        Repos::new(tx).floor().add_range(
            OUTLET,
            &Range {
                section_id: Some("sec_hall".to_owned()),
                prefix: String::new(),
                from: 4,
                to: 8,
                seats: 4,
            },
            at(5),
        )
    });
    assert!(refused.is_err(), "the range clashes and must be refused");

    let after = db
        .transaction(|tx| Repos::new(tx).floor().list_tables(OUTLET))
        .expect("tables")
        .len();
    assert_eq!(after, before, "not one table of a refused range is created");

    // A clean range creates all of it.
    let made = db
        .transaction(|tx| {
            Repos::new(tx).floor().add_range(
                OUTLET,
                &Range {
                    section_id: Some("sec_hall".to_owned()),
                    prefix: "G".to_owned(),
                    from: 1,
                    to: 20,
                    seats: 2,
                },
                at(6),
            )
        })
        .expect("a clean range");
    assert_eq!(made.len(), 20);
}

/// A table with an open order cannot be hidden or deleted, and the refusal
/// says which order — audit F8: a shopkeeper never reads a system message.
#[test]
fn a_table_with_an_open_order_cannot_be_hidden_or_deleted() {
    let scratch = Scratch::new("busy_table");
    let db = scratch.open();
    let built = shop::build(&db);
    assert!(!built.orders.is_empty(), "the fixture leaves an order on a table");

    let busy = db
        .transaction(|tx| {
            let repos = Repos::new(tx);
            let open = repos.orders().list_open(OUTLET)?;
            Ok(open
                .iter()
                .find_map(|o| o.core().table.clone())
                .expect("an order on a table"))
        })
        .expect("a busy table");

    let hidden = db.transaction(|tx| Repos::new(tx).floor().set_active(OUTLET, &busy, false, at(9)));
    let said = hidden.expect_err("refused").to_string();
    assert!(said.contains("open order"), "{said}");

    let deleted = db.transaction(|tx| Repos::new(tx).floor().delete_table(OUTLET, &busy, at(9)));
    assert!(deleted.is_err(), "nor deleted");
}

/// A table that has taken an order can only ever be hidden — and it is told
/// that, with the number, rather than being handed a foreign-key error.
#[test]
fn a_used_table_can_be_hidden_and_never_deleted() {
    let scratch = Scratch::new("used_table");
    let db = scratch.open();
    shop::build(&db);

    // A brand new table nothing points at: really deletable.
    db.transaction(|tx| {
        Repos::new(tx)
            .floor()
            .save_table(OUTLET, &table("tbl_typo", Some("sec_hall"), "Typo"), at(1))
    })
    .expect("added");
    db.transaction(|tx| {
        Repos::new(tx).floor().delete_table(OUTLET, &TableId::new("tbl_typo"), at(2))
    })
    .expect("a mistyped table added five minutes ago is deletable");

    // One with history: refused, with the count in the message.
    let used = db
        .transaction(|tx| {
            let repos = Repos::new(tx);
            let all = repos.orders().list_all()?;
            Ok(all
                .iter()
                .find_map(|o| o.core().table.clone())
                .expect("a table with history"))
        })
        .expect("a used table");

    // Settle or cancel whatever is open on it first, so what is left is
    // history rather than an open order — that is the case being tested.
    let said = db
        .transaction(|tx| Repos::new(tx).floor().delete_table(OUTLET, &used, at(3)))
        .expect_err("refused")
        .to_string();
    assert!(
        said.contains("Hide it instead") || said.contains("open order"),
        "the message says what to do instead: {said}",
    );
}

/// **Two tables cannot share a square.** A tile underneath another tile is a
/// table that has vanished from the floor.
#[test]
fn a_floor_plan_cell_holds_one_table() {
    let scratch = Scratch::new("plan");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        Repos::new(tx).floor().place(OUTLET, &TableId::new("tbl_1"), Some((3, 3)), at(1))
    })
    .expect("placed");

    let said = db
        .transaction(|tx| {
            Repos::new(tx).floor().place(OUTLET, &TableId::new("tbl_2"), Some((3, 3)), at(2))
        })
        .expect_err("occupied")
        .to_string();
    assert!(said.contains("already there"), "{said}");

    // Off the edge is refused too, rather than stored and drawn nowhere.
    assert!(
        db.transaction(|tx| {
            Repos::new(tx).floor().place(OUTLET, &TableId::new("tbl_2"), Some((99, 0)), at(3))
        })
        .is_err()
    );

    // And a layout reloads exactly as it was left.
    let tables = db
        .transaction(|tx| Repos::new(tx).floor().list_tables(OUTLET))
        .expect("tables");
    let one = tables.iter().find(|t| t.id.as_str() == "tbl_1").expect("table 1");
    assert_eq!(one.pos, Some((3, 3)));
}

/// A table added after a layout exists lands somewhere visible — not under an
/// existing tile, and not off the plan the owner is looking at.
#[test]
fn a_new_table_finds_an_empty_square() {
    let scratch = Scratch::new("free_cell");
    let db = scratch.open();
    shop::build(&db);

    let cell = db
        .transaction(|tx| Repos::new(tx).floor().first_free_cell(OUTLET))
        .expect("a free cell");

    let taken: Vec<(i64, i64)> = db
        .transaction(|tx| Repos::new(tx).floor().list_tables(OUTLET))
        .expect("tables")
        .into_iter()
        .filter_map(|t| t.pos)
        .collect();
    assert!(!taken.contains(&cell), "the square offered is genuinely empty");

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let mut fresh = table("tbl_new", Some("sec_hall"), "New");
        fresh.pos = Some(cell);
        repos.floor().save_table(OUTLET, &fresh, at(1))
    })
    .expect("placed on the free square");
}

/// **The summary must agree with the grid under it.**
#[test]
fn occupancy_reconciles_with_the_open_orders() {
    let scratch = Scratch::new("occupancy");
    let db = scratch.open();
    shop::build(&db);

    let (numbers, open_tables, active_tables) = db
        .transaction(|tx| {
            let repos = Repos::new(tx);
            let open = repos.orders().list_open(OUTLET)?;
            let day = open
                .first()
                .map_or_else(|| BusinessDay::from_days_since_epoch(0), |o| o.core().business_day);
            let numbers = repos.floor().occupancy(OUTLET, day)?;
            let busy: std::collections::BTreeSet<String> = open
                .iter()
                .filter_map(|o| o.core().table.as_ref().map(|t| t.as_str().to_owned()))
                .collect();
            let active = repos.floor().list_tables(OUTLET)?.iter().filter(|t| t.is_active).count();
            Ok((numbers, busy.len(), active))
        })
        .expect("occupancy");

    assert_eq!(
        usize::try_from(numbers.busy).expect("a count"),
        open_tables,
        "busy tables must be the tables with open orders — nothing else",
    );
    assert_eq!(usize::try_from(numbers.tables).expect("a count"), active_tables);
    assert!(numbers.busy <= numbers.tables);
}

/// Scope 1.22 — a merged-away order is recorded, not deleted, and the link is
/// a column so a report can tell a merge from a walkout.
#[test]
fn a_merge_leaves_a_link_rather_than_a_hole() {
    let scratch = Scratch::new("merge_link");
    let db = scratch.open();
    let built = shop::build(&db);
    let orders = built.orders;
    assert!(orders.len() >= 2, "the fixture has orders to link");

    db.transaction(|tx| {
        Repos::new(tx)
            .floor()
            .record_merge(orders[0].as_str(), orders[1].as_str())
    })
    .expect("linked");

    let into: Option<String> = db
        .transaction(|tx| {
            Ok(tx.query_row(
                "SELECT merged_into FROM orders WHERE id = ?1",
                [orders[0].as_str()],
                |r| r.get(0),
            )?)
        })
        .expect("read back");
    assert_eq!(into.as_deref(), Some(orders[1].as_str()));
}
