//! ```text
//! cargo test -p magic-bill --release perf_ -- --nocapture
//! ```

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a stopwatch, not the money path"
)]

use std::time::{Duration, Instant};

use mb_auth::RolePreset;
use mb_db::Repos;

use crate::state::{App, OUTLET};

/// Three readings, middle one.
fn three(mut f: impl FnMut()) -> Duration {
    let mut readings: Vec<Duration> = (0..3)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    readings.sort_unstable();
    readings[1]
}

/// A shop with a real menu of `count` items.
fn a_shop_with_a_menu(app: &App, count: usize) {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                for n in 0..count {
                    repos.menu().save_item(
                        OUTLET,
                        &mb_db::repo::menu::MenuItem {
                            id: mb_core::ItemId::new(format!("itm_{n}")),
                            category_id: None,
                            // Names that actually rank against each other: some start with the
                            // word, some contain it.
                            name: if n % 3 == 0 {
                                format!("Masala Dosa {n}")
                            } else if n % 3 == 1 {
                                format!("Plain Masala {n}")
                            } else {
                                format!("Item {n} with masala")
                            },
                            unit_price: mb_core::Money::from_paise(10_000),
                            tax_class_id: mb_core::seeded_placement(mb_core::TaxSpec::gst(
                                mb_core::TaxRate::from_percent(5).expect("5%"),
                            )).expect("a seeded slab").0,
                            price_basis: mb_core::seeded_placement(mb_core::TaxSpec::gst(
                                mb_core::TaxRate::from_percent(5).expect("5%"),
                            )).expect("a seeded slab").1,
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
                Ok(())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a menu");
}

/// 2,000 items, ranked, in 20 ms (ceiling 50).
#[test]
fn perf_b2_two_thousand_items_ranked() {
    let scratch = crate::signin_tests::scratch("b2");
    let app = crate::signin_tests::shop_for_perf(&scratch);
    a_shop_with_a_menu(&app, 2_000);

    let elapsed = three(|| {
        let hits = crate::ipc::search_items_on(&app, "masala".to_owned(), None).expect("a search");
        assert!(!hits.is_empty(), "the search found nothing to rank");
    });

    println!("B2 search 2,000 items : {elapsed:?} (budget 20 ms, ceiling 50 ms)");

    #[cfg(not(debug_assertions))]
    assert!(
        elapsed < Duration::from_millis(50),
        "B2: {elapsed:?} is over the 50 ms ceiling"
    );
}

/// Open an existing table's order into the cart, in 80 ms (ceiling 200).
#[test]
fn perf_b7_open_an_existing_table() {
    let scratch = crate::signin_tests::scratch("b7");
    let app = crate::signin_tests::shop_for_perf(&scratch);
    a_shop_with_a_menu(&app, 200);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                repos.people().save_role(
                    OUTLET,
                    &RolePreset::Owner.shape(),
                    crate::flows::now(),
                )?;
                repos.floor().save_section(
                    OUTLET,
                    &mb_db::repo::floor::Section {
                        id: "sec_hall".to_owned(),
                        name: "Hall".to_owned(),
                        sort_order: 0,
                        is_active: true,
                    },
                    crate::flows::now(),
                )?;
                repos.floor().save_table(
                    OUTLET,
                    &mb_db::repo::floor::DiningTable {
                        id: mb_core::TableId::new("tbl_7"),
                        section_id: Some("sec_hall".to_owned()),
                        label: "7".to_owned(),
                        seats: 4,
                        pos: None,
                        sort_order: 7,
                        is_active: true,
                    },
                    crate::flows::now(),
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a floor");

    // A real open order on that table: twelve lines, told to the kitchen.
    crate::ipc::open_table_on(&app, "tbl_7".to_owned()).expect("opened");
    for n in 0..12 {
        crate::ipc::cart_add_on(&app, format!("itm_{n}"), Some("2".to_owned()), None)
            .expect("added");
    }
    crate::flows::print_kitchen_ticket_on(&app).expect("parked");

    let elapsed = three(|| {
        crate::ipc::cart_clear_on(&app, false).expect("a fresh cart");
        let view = crate::ipc::open_table_on(&app, "tbl_7".to_owned()).expect("opened");
        assert_eq!(view.lines.len(), 12, "the order did not come back whole");
    });

    println!("B7 open a busy table : {elapsed:?} (budget 80 ms, ceiling 200 ms)");

    #[cfg(not(debug_assertions))]
    assert!(
        elapsed < Duration::from_millis(200),
        "B7: {elapsed:?} is over the 200 ms ceiling"
    );
}
