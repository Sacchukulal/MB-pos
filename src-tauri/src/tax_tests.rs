//! The tax ladder, driven end to end: the item's own rate, else its category's, else the shop's.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_core::{ItemId, Money, TaxClassId};
use mb_db::{Db, DbConfig, Repos};

use crate::menu::{
    MenuEdit, delete_category_on, delete_item_on, menu_rows_on, save_category_on, save_item_on,
};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};
use crate::tax::{page_on, set_category_on, set_items_on, set_shop_rate_on};

/// A shop with two categories and four items: the dosa (no category) and the idli and lassi
/// (each in a category) follow the shop; the water is on its own 18%.
fn a_shop_on_the_ladder(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for (id, name, class) in [
            ("itm_dosa", "Masala dosa", "tax_food_5"),
            ("itm_water", "Water bottle", "tax_packaged_18"),
        ] {
            repos.menu().save_item(
                OUTLET,
                &mb_db::repo::menu::MenuItem {
                    id: ItemId::new(id),
                    category_id: None,
                    name: name.to_owned(),
                    unit_price: Money::from_paise(2_000),
                    tax_class_id: TaxClassId::new(class),
                    price_basis: None,
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
    .expect("a menu");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);

    for (id, name) in [("cat_tiffin", "Tiffin"), ("cat_drinks", "Drinks")] {
        save_category_on(&app, id.to_owned(), name.to_owned(), true, None).expect("a category");
    }
    for (id, name, category) in [
        ("itm_idli", "Idli", "cat_tiffin"),
        ("itm_lassi", "Lassi", "cat_drinks"),
    ] {
        save_item_on(&app, edit(id, name, category)).expect("an item on the shop's rate");
    }
    app
}

fn edit(id: &str, name: &str, category: &str) -> MenuEdit {
    MenuEdit {
        id: id.to_owned(),
        name: name.to_owned(),
        category_id: Some(category.to_owned()),
        price: "50".to_owned(),
        tax_class_id: None,
        price_basis: None,
        hsn: None,
        short_code: None,
        cost: None,
        is_open_price: false,
        is_available: true,
        course: None,
        prep_minutes: None,
    }
}

/// The whole rate line — "5% · added on top".
fn rate_of(app: &App, id: &str) -> String {
    menu_rows_on(app)
        .expect("the menu")
        .into_iter()
        .find(|r| r.id == id)
        .expect("that item")
        .rate
}

/// Just the percentage.
fn percent_of(app: &App, id: &str) -> String {
    rate_of(app, id)
        .split(" · ")
        .next()
        .expect("a rate always has a percentage in front")
        .to_owned()
}

fn from_of(app: &App, id: &str) -> String {
    page_on(app)
        .expect("the tax page")
        .categories
        .iter()
        .flat_map(|c| c.items.iter())
        .find(|i| i.id == id)
        .expect("that item")
        .from
        .clone()
}

#[test]
fn a_new_item_starts_on_the_shop_rate() {
    let scratch = Scratch::new("ladder_new");
    let app = a_shop_on_the_ladder(&scratch, "new");

    let page = page_on(&app).expect("the tax page");
    assert_eq!(page.shop_rate, "5", "a fresh shop is on the seeded 5%");
    assert_eq!(percent_of(&app, "itm_idli"), "5%");
    assert_eq!(from_of(&app, "itm_idli"), "shop");
    assert_eq!(from_of(&app, "itm_water"), "item");
}

/// Changing the shop's rate moves every item that follows it, and nothing else.
#[test]
fn the_shop_rate_moves_its_followers_and_nothing_else() {
    let scratch = Scratch::new("ladder_shop");
    let app = a_shop_on_the_ladder(&scratch, "shop");

    set_shop_rate_on(&app, "18".to_owned()).expect("the shop moved to 18%");
    assert_eq!(percent_of(&app, "itm_idli"), "18%");
    assert_eq!(percent_of(&app, "itm_lassi"), "18%");
    assert_eq!(percent_of(&app, "itm_dosa"), "18%");
    assert_eq!(from_of(&app, "itm_idli"), "shop");
    // The water was on 18% of its own; now it is the shop's rate too.
    assert_eq!(from_of(&app, "itm_water"), "shop");

    // A rate the shop never had is made on the spot, named after itself.
    let page = set_shop_rate_on(&app, "12".to_owned()).expect("12% is made");
    assert_eq!(page.shop_rate, "12");
    assert!(
        page.slabs.iter().any(|s| s.name == "12%"),
        "{:?}",
        page.slabs
    );
    assert_eq!(percent_of(&app, "itm_idli"), "12%");
    assert_eq!(percent_of(&app, "itm_water"), "12%");

    // A rate that is not a rate is refused.
    assert!(set_shop_rate_on(&app, "abc".to_owned()).is_err());
    assert!(set_shop_rate_on(&app, "".to_owned()).is_err());
}

/// A category's own rate moves the items in it that follow, and a later shop change leaves
/// that category alone.
#[test]
fn a_category_rate_holds_its_items_against_the_shop() {
    let scratch = Scratch::new("ladder_category");
    let app = a_shop_on_the_ladder(&scratch, "category");

    set_category_on(
        &app,
        "cat_drinks".to_owned(),
        Some("tax_packaged_18".to_owned()),
    )
    .expect("drinks at 18%");
    assert_eq!(percent_of(&app, "itm_lassi"), "18%");
    assert_eq!(from_of(&app, "itm_lassi"), "category");
    assert_eq!(percent_of(&app, "itm_idli"), "5%", "tiffin did not move");

    set_shop_rate_on(&app, "0".to_owned()).expect("the shop went to 0%");
    assert_eq!(percent_of(&app, "itm_idli"), "0%");
    assert_eq!(
        percent_of(&app, "itm_lassi"),
        "18%",
        "drinks kept their own rate"
    );

    // Back to the shop's rate: the lassi follows the shop again.
    set_category_on(&app, "cat_drinks".to_owned(), None).expect("drinks follow the shop");
    assert_eq!(percent_of(&app, "itm_lassi"), "0%");
    assert_eq!(from_of(&app, "itm_lassi"), "shop");
}

/// An item put on its own rate stays there through shop changes, and "follow" takes it back up
/// the ladder.
#[test]
fn an_item_on_its_own_rate_stays_until_told_to_follow() {
    let scratch = Scratch::new("ladder_item");
    let app = a_shop_on_the_ladder(&scratch, "item");

    set_items_on(
        &app,
        vec!["itm_idli".to_owned()],
        Some("tax_packaged_18".to_owned()),
        None,
    )
    .expect("idli at 18%");
    assert_eq!(from_of(&app, "itm_idli"), "item");

    set_shop_rate_on(&app, "0".to_owned()).expect("the shop went to 0%");
    assert_eq!(percent_of(&app, "itm_idli"), "18%", "its own rate held");

    set_items_on(
        &app,
        vec!["itm_idli".to_owned()],
        Some("follow".to_owned()),
        None,
    )
    .expect("idli follows again");
    assert_eq!(percent_of(&app, "itm_idli"), "0%");
    assert_eq!(from_of(&app, "itm_idli"), "shop");

    // The price rule on its own leaves the rate alone.
    set_items_on(
        &app,
        vec!["itm_idli".to_owned()],
        None,
        Some("inclusive".to_owned()),
    )
    .expect("priced tax-in");
    assert_eq!(rate_of(&app, "itm_idli"), "0% · in the price");
    assert!(set_items_on(&app, vec!["itm_idli".to_owned()], None, None).is_err());
}

/// Moving an item into a category takes that category's rate, unless the item has its own.
#[test]
fn moving_an_item_between_categories_follows_the_new_rung() {
    let scratch = Scratch::new("ladder_move");
    let app = a_shop_on_the_ladder(&scratch, "move");
    set_category_on(
        &app,
        "cat_drinks".to_owned(),
        Some("tax_packaged_18".to_owned()),
    )
    .expect("drinks at 18%");

    save_item_on(&app, edit("itm_idli", "Idli", "cat_drinks")).expect("moved");
    assert_eq!(
        percent_of(&app, "itm_idli"),
        "18%",
        "it took the drinks rate"
    );
    save_item_on(&app, edit("itm_idli", "Idli", "cat_tiffin")).expect("moved back");
    assert_eq!(
        percent_of(&app, "itm_idli"),
        "5%",
        "and the tiffin rate on the way back"
    );

    // The water is on its own 18% while the shop is on 5%; a move does not touch it.
    save_item_on(&app, edit("itm_water", "Water bottle", "cat_tiffin")).expect("moved");
    assert_eq!(percent_of(&app, "itm_water"), "18%");
    assert_eq!(from_of(&app, "itm_water"), "item");
}

/// A shop with no GST shows no GST on its items, whatever slab they are on.
#[test]
fn a_shop_with_no_gst_says_so_on_every_item() {
    let scratch = Scratch::new("ladder_no_gst");
    let app = a_shop_on_the_ladder(&scratch, "no_gst");
    crate::settings::ipc::save_on(
        &app,
        vec![crate::settings::ipc::SettingEdit {
            key: "store.registration".to_owned(),
            value: "unregistered".to_owned(),
        }],
    )
    .expect("GST off");
    assert_eq!(rate_of(&app, "itm_idli"), "No GST");
    let page = page_on(&app).expect("the tax page");
    assert!(!page.charges_gst);
    assert_eq!(page.registration, "unregistered");
}

/// Deleting an item that was never sold removes it; a deleted category leaves its items with
/// no category.
#[test]
fn deleting_items_and_categories() {
    let scratch = Scratch::new("ladder_delete");
    let app = a_shop_on_the_ladder(&scratch, "delete");

    let rows = delete_item_on(&app, "itm_idli".to_owned()).expect("deleted");
    assert!(!rows.iter().any(|r| r.id == "itm_idli"));

    let categories = delete_category_on(&app, "cat_drinks".to_owned()).expect("deleted");
    assert!(
        !categories
            .iter()
            .any(|c| c.id == "cat_drinks" && c.is_active)
    );
    let lassi = menu_rows_on(&app)
        .expect("the menu")
        .into_iter()
        .find(|r| r.id == "itm_lassi")
        .expect("still on the menu");
    assert_eq!(lassi.category_id, None);
}
