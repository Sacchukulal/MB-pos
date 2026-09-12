//! The menu, driven end to end against a real database.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_core::{ItemId, Money, TaxClassId, TaxKind};
use mb_db::repo::ImportMode;
use mb_db::{Db, DbConfig, Repos};

use crate::menu::{
    ComboEdit, GroupEdit, MenuEdit, ModifierEdit, attach_group_on, change_prices_on,
    export_menu_to, item_composition_on, list_combos_on, list_groups_on, menu_rows_on,
    plan_import_on, run_import_on, save_combo_on, save_group_on, save_item_on, save_variant_on,
};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};
use crate::tax::{save_slab_on, slabs_on};

/// A shop with three items on two tax classes — enough that "every item on this class" is a
/// claim with a counter-example in it.
fn a_shop_with_a_menu(scratch: &Scratch) -> App {
    let path = scratch.dir().join("menu.db");
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for (id, name, paise, class) in [
            ("itm_tea", "Tea", 2000_i64, "tax_food_5"),
            ("itm_dosa", "Masala dosa", 12000, "tax_food_5"),
            ("itm_water", "Water bottle", 2000, "tax_packaged_18"),
        ] {
            repos.menu().save_item(
                OUTLET,
                &mb_db::repo::menu::MenuItem {
                    id: ItemId::new(id),
                    category_id: None,
                    name: name.to_owned(),
                    unit_price: Money::from_paise(paise),
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
    app
}

fn price_of(app: &App, id: &str) -> String {
    menu_rows_on(app)
        .expect("the menu")
        .into_iter()
        .find(|r| r.id == id)
        .expect("that item")
        .price
        .text
}

/// Just the percentage. The row also carries the treatment in words — "5% · Tax added on top" —
/// and that half is asserted where it matters.
fn rate_of(app: &App, id: &str) -> String {
    menu_rows_on(app)
        .expect("the menu")
        .into_iter()
        .find(|r| r.id == id)
        .expect("that item")
        .rate
        .split(" · ")
        .next()
        .expect("a rate always has a percentage in front")
        .to_owned()
}

/// The whole rate line — "5% · added on top".
fn row_rate(app: &App, id: &str) -> String {
    menu_rows_on(app)
        .expect("the menu")
        .into_iter()
        .find(|r| r.id == id)
        .expect("that item")
        .rate
}

fn slab_of(app: &App, id: &str) -> crate::tax::TaxSlabView {
    slabs_on(app)
        .expect("the slabs")
        .into_iter()
        .find(|c| c.id == id)
        .expect("that slab")
}

/// A rate changes, and everything on that slab changes with it — nothing is rewritten, the
/// items simply read their slab.
#[test]
fn changing_a_slab_moves_every_item_on_it_and_nothing_else() {
    let scratch = Scratch::new("tax_class");
    let app = a_shop_with_a_menu(&scratch);

    assert_eq!(rate_of(&app, "itm_tea"), "5%");
    assert_eq!(rate_of(&app, "itm_water"), "18%");

    let food = slab_of(&app, "tax_food_5");
    assert_eq!(food.items_using, 2, "tea and dosa, not the water");

    save_slab_on(
        &app,
        "tax_food_5".to_owned(),
        "GST 12%".to_owned(),
        "12".to_owned(),
        TaxKind::Gst,
        "shop".to_owned(),
    )
    .expect("the rate changed");

    assert_eq!(rate_of(&app, "itm_tea"), "12%");
    assert_eq!(rate_of(&app, "itm_dosa"), "12%");
    assert_eq!(rate_of(&app, "itm_water"), "18%", "a different slab");

    // The NAME moves too, and the counter's own book followed without a restart.
    let food = slab_of(&app, "tax_food_5");
    assert_eq!(food.name, "12%", "named by its rate on the page");
    assert_eq!(food.rate, "12%");
    assert_eq!(
        app.shop_config()
            .tax
            .spec_for(&TaxClassId::new("tax_food_5"), None)
            .expect("in the book")
            .rate,
        mb_core::TaxRate::from_percent(12).expect("12%")
    );
}

/// A rate outside 0–100% is refused in words, not by a panic.
#[test]
fn an_impossible_rate_is_refused() {
    let scratch = Scratch::new("bad_rate");
    let app = a_shop_with_a_menu(&scratch);

    let err = save_slab_on(
        &app,
        "tax_food_5".to_owned(),
        "Nonsense".to_owned(),
        "400".to_owned(),
        TaxKind::Gst,
        "shop".to_owned(),
    )
    .expect_err("400% is not a tax rate");
    assert_eq!(err.code, "tax.rate");
    assert_eq!(rate_of(&app, "itm_tea"), "5%", "and nothing moved");
}

#[test]
fn changing_a_slab_label_does_not_change_what_it_taxes() {
    let scratch = Scratch::new("class_label");
    let app = a_shop_with_a_menu(&scratch);

    // The water's slab becomes liquor: outside GST, priced tax-in, 20% VAT.
    save_slab_on(
        &app,
        "tax_packaged_18".to_owned(),
        "Liquor — state VAT".to_owned(),
        "20".to_owned(),
        TaxKind::OutsideGst,
        "inclusive".to_owned(),
    )
    .expect("a bar's slab");
    assert!(
        row_rate(&app, "itm_water").contains("VAT 20% · in the price"),
        "the bottle is outside GST: {}",
        row_rate(&app, "itm_water")
    );

    // What the screen holds: machine values beside the words.
    let before = slab_of(&app, "tax_packaged_18");
    assert_eq!(before.kind, TaxKind::OutsideGst);
    assert_eq!(before.basis, "inclusive");
    assert_eq!(before.rate_bp, 2000);

    // Rename it to words that say none of that, sending back exactly what the view carried.
    save_slab_on(
        &app,
        before.id.clone(),
        "Bar list".to_owned(),
        before.rate.trim_end_matches('%').to_owned(),
        before.kind,
        before.basis.clone(),
    )
    .expect("renamed");

    let after = slab_of(&app, "tax_packaged_18");
    assert_eq!(after.name, "Bar list");
    assert_eq!(after.kind, TaxKind::OutsideGst, "still not GST");
    assert_eq!(after.basis, "inclusive");
    assert_eq!(after.rate, "20%");
    assert!(
        row_rate(&app, "itm_water").contains("VAT 20%"),
        "and the bottle is still outside GST: {}",
        row_rate(&app, "itm_water")
    );
}

/// A rate on a kind that cannot carry one is refused, not silently zeroed — which is why the
/// editor disables the box rather than hinting at it.
#[test]
fn an_exempt_slab_cannot_be_given_a_rate() {
    let scratch = Scratch::new("exempt_rate");
    let app = a_shop_with_a_menu(&scratch);

    let err = save_slab_on(
        &app,
        "tax_exempt".to_owned(),
        "Exempt".to_owned(),
        "5".to_owned(),
        TaxKind::Exempt,
        "shop".to_owned(),
    )
    .expect_err("exempt at 5% is not a thing");
    assert_eq!(err.code, "tax.rate");
}

/// An HSN code is 2, 4, 6 or 8 digits.
#[test]
fn an_hsn_of_three_digits_is_refused() {
    let scratch = Scratch::new("hsn");
    let app = a_shop_with_a_menu(&scratch);

    let edit = |hsn: &str| MenuEdit {
        id: "itm_tea".to_owned(),
        name: "Tea".to_owned(),
        category_id: None,
        price: "20".to_owned(),
        tax_class_id: None,
        price_basis: None,
        hsn: Some(hsn.to_owned()),
        short_code: None,
        cost: None,
        is_open_price: false,
        is_available: true,
        course: None,
        prep_minutes: None,
    };

    let err = save_item_on(&app, edit("996")).expect_err("three digits is not a code");
    assert_eq!(err.code, "menu.hsn");
    assert!(
        save_item_on(&app, edit("21069099")).is_ok(),
        "eight digits is"
    );
    assert!(save_item_on(&app, edit("  ")).is_ok(), "and blank is fine");
    let err = save_item_on(&app, edit("9963A1")).expect_err("letters are not digits");
    assert_eq!(err.code, "menu.hsn");
}

/// Ten percent on one category, and the rest of the menu untouched.
#[test]
fn a_bulk_price_change_rounds_once_and_stays_inside_its_category() {
    let scratch = Scratch::new("bulk");
    let app = a_shop_with_a_menu(&scratch);

    let said = change_prices_on(&app, None, "10".to_owned()).expect("prices up");
    assert!(said.contains('3'), "three items moved: {said}");

    assert_eq!(price_of(&app, "itm_tea"), "22.00");
    assert_eq!(price_of(&app, "itm_dosa"), "132.00");

    // And back down again is NOT a round trip.
    change_prices_on(&app, None, "-10".to_owned()).expect("prices down");
    assert_eq!(price_of(&app, "itm_dosa"), "118.80");
}

/// The menu, exported to a file in the scratch folder and read back as text.
fn exported(app: &App, scratch: &Scratch, name: &str) -> String {
    let path = scratch.dir().join(name);
    let written = export_menu_to(app, &path).expect("exported");
    assert_eq!(written, path.display().to_string());
    std::fs::read_to_string(&path).expect("the file")
}

/// The file a restaurant already has: `category,name,price`, two hundred-odd rows, the same
/// dish in two sections, a misspelt heading or two. It goes in as it is.
const RESTAURANT_MENU: &str = include_str!("../fixtures/restaurant-menu.csv");

/// Export, change one cell, import back — and the export reads the way a person would write it.
#[test]
fn the_menu_survives_a_trip_through_a_spreadsheet() {
    let scratch = Scratch::new("csv");
    let app = a_shop_with_a_menu(&scratch);

    let csv = exported(&app, &scratch, "menu.csv");
    assert!(
        csv.starts_with("category,name,price,tax,"),
        "the file leads with the three columns every restaurant has: {csv}"
    );
    assert!(
        csv.contains(",Masala dosa,120.00,GST 5%,"),
        "rupees and the slab's name: {csv}"
    );
    assert!(!csv.contains("12000"), "no paise anywhere: {csv}");

    // Read straight back, nothing changes — and the plan says so rather than counting every
    // row as a change.
    let same = plan_import_on(&app, &csv, ImportMode::Update).expect("planned");
    assert!(same.is_clean, "{same:?}");
    assert!(same.is_empty, "{same:?}");
    assert_eq!(same.unchanged, 3);
    assert!(
        same.summary.starts_with("Nothing would change"),
        "{}",
        same.summary
    );

    let edited = csv.replace(",120.00,", ",135.00,");
    assert_ne!(edited, csv, "the fixture actually changed a cell");

    // The dry run writes NOTHING and says what it would do.
    let plan = plan_import_on(&app, &edited, ImportMode::Update).expect("planned");
    assert!(
        plan.is_clean,
        "a file we just exported has no bad rows: {plan:?}"
    );
    assert_eq!(
        (plan.new_items, plan.updated_items, plan.unchanged),
        (0, 1, 2)
    );
    assert_eq!(
        price_of(&app, "itm_dosa"),
        "120.00",
        "the dry run changed nothing"
    );

    let said = run_import_on(&app, &edited, ImportMode::Update).expect("imported");
    assert!(!said.is_empty());
    assert_eq!(price_of(&app, "itm_dosa"), "135.00");
    assert_eq!(rate_of(&app, "itm_water"), "18%", "and the rates came back");
}

/// A file exported before 1.6.17 — ids first, paise, slab ids — still imports.
#[test]
fn an_old_export_still_reads() {
    let scratch = Scratch::new("csv_old");
    let app = a_shop_with_a_menu(&scratch);

    let old = "id,name,category,price_paise,tax_class,price_basis,hsn,short_code,cost_paise,available\r\n\
               itm_dosa,Masala dosa,,13500,tax_food_5,shop,,,,yes\r\n";
    let plan = plan_import_on(&app, old, ImportMode::Update).expect("planned");
    assert!(plan.is_clean, "{plan:?}");
    assert_eq!((plan.new_items, plan.updated_items), (0, 1));
    run_import_on(&app, old, ImportMode::Update).expect("imported");
    assert_eq!(price_of(&app, "itm_dosa"), "135.00");
}

/// A file with a bad cell is refused by line number, and the good lines do not sneak in — a
/// half-imported menu is worse than no import.
#[test]
fn a_bad_row_names_its_line_and_nothing_is_written() {
    let scratch = Scratch::new("csv_bad");
    let app = a_shop_with_a_menu(&scratch);

    let csv = exported(&app, &scratch, "menu.csv");
    let broken = csv.replace(",120.00,", ",one hundred and twenty,");
    assert_ne!(broken, csv);

    let plan = plan_import_on(&app, &broken, ImportMode::Update).expect("planned");
    assert!(!plan.is_clean, "a price in words is not a price");
    assert!(
        plan.refused
            .iter()
            .any(|r| r.contains("one hundred") || r.chars().any(|c| c.is_ascii_digit())),
        "the refusal points at the line: {:?}",
        plan.refused
    );

    let err = run_import_on(&app, &broken, ImportMode::Update).expect_err("refused");
    assert!(!err.message.is_empty());
    assert_eq!(price_of(&app, "itm_dosa"), "120.00", "nothing was written");
}

/// The file a restaurant already has goes in as it is: every category made, the same dish in
/// two sections kept as two items, and a second import of the same file changing nothing.
#[test]
fn a_restaurants_own_file_imports_as_it_is() {
    let scratch = Scratch::new("csv_restaurant");
    let app = a_shop_with_a_menu(&scratch);

    let plan = plan_import_on(&app, RESTAURANT_MENU, ImportMode::Update).expect("planned");
    assert!(plan.is_clean, "{:?}", plan.refused);
    assert_eq!(plan.new_categories.len(), 9, "{:?}", plan.new_categories);
    // 241 rows, all new: the file puts "Tea" in TEA, and the shop's Tea has no category, so
    // that is a second Tea — the owner finds and deletes duplicates, the import does not guess.
    assert_eq!(
        (plan.new_items, plan.updated_items, plan.unchanged),
        (241, 0, 0)
    );
    assert!(plan.already.is_empty(), "{:?}", plan.already);

    run_import_on(&app, RESTAURANT_MENU, ImportMode::Update).expect("imported");
    let rows = menu_rows_on(&app).expect("the menu");
    assert_eq!(rows.len(), 244);

    // Boiled rice is Chinese at 40 and South Indian at 60. Two items.
    let boiled: Vec<_> = rows
        .iter()
        .filter(|r| r.name.eq_ignore_ascii_case("boiled rice"))
        .collect();
    assert_eq!(boiled.len(), 2, "{boiled:?}");
    assert_ne!(boiled[0].category_id, boiled[1].category_id);
    assert!(boiled.iter().any(|r| r.price.text == "40.00"));
    assert!(boiled.iter().any(|r| r.price.text == "60.00"));
    assert_eq!(rows.iter().filter(|r| r.name == "Tea").count(), 2);
    assert_eq!(
        price_of(&app, "itm_tea"),
        "20.00",
        "the shop's own Tea was not touched"
    );

    // The same file again is a no-op, and says so — the second Boiled rice finds ITS item, and
    // every row is named as already on the menu.
    let again = plan_import_on(&app, RESTAURANT_MENU, ImportMode::Update).expect("planned again");
    assert!(again.is_clean, "{:?}", again.refused);
    assert!(again.is_empty, "{}", again.summary);
    assert_eq!(again.unchanged, 241);
    assert_eq!(again.already.len(), 241);
    assert!(
        again
            .already
            .contains(&"Boiled Rice (SOUTH INDIAN)".to_owned()),
        "{:?}",
        &again.already[..5]
    );
    assert!(again.new_categories.is_empty());

    // And what went in comes out in the same shape, and goes back in unchanged.
    let out = exported(&app, &scratch, "menu.csv");
    let back = plan_import_on(&app, &out, ImportMode::Update).expect("planned the export");
    assert!(back.is_clean && back.is_empty, "{back:?}");
    assert_eq!(back.unchanged, 244);
}

/// REPLACE: the file becomes the menu. What it does not name is deleted — unless a size, a
/// combo, a recipe or a bill still points at it, in which case it is taken off the menu and
/// kept. A category left with nothing in it is removed.
#[test]
fn replacing_the_menu_keeps_only_what_the_file_names() {
    let scratch = Scratch::new("csv_replace");
    let app = a_shop_with_a_menu(&scratch);
    // The dosa has a half size, so something points at it.
    save_variant_on(
        &app,
        "itm_dosa".to_owned(),
        "var_half".to_owned(),
        "Half".to_owned(),
        "70".to_owned(),
        true,
    )
    .expect("a size");
    // And the water sits in a category of its own that the file will not name.
    run_import_on(
        &app,
        "category,name,price,id\nBottles,Water bottle,20,itm_water\n",
        ImportMode::Update,
    )
    .expect("moved");

    let file = "category,name,price\nTiffin,Idli,40\nTiffin,Tea,20\n";
    let plan = plan_import_on(&app, file, ImportMode::Replace).expect("planned");
    assert!(plan.is_clean, "{:?}", plan.refused);
    assert_eq!(plan.mode, "replace");
    // Idli is new; Tea has no category on the menu and the file gives one, so it is a second
    // Tea; the old Tea, the dosa and the water are not in the file.
    assert_eq!((plan.new_items, plan.updated_items), (2, 0));
    assert_eq!(
        plan.removed,
        vec!["Tea".to_owned(), "Water bottle".to_owned()],
        "{plan:?}"
    );
    assert_eq!(
        plan.taken_off,
        vec!["Masala dosa".to_owned()],
        "the dosa has a size"
    );
    assert_eq!(plan.retired_categories, vec!["Bottles".to_owned()]);
    assert!(
        plan.summary.contains("3 item(s) not in the file would go"),
        "{}",
        plan.summary
    );
    assert!(!plan.is_empty);

    // Nothing moved yet.
    assert_eq!(menu_rows_on(&app).expect("rows").len(), 3);

    let said = run_import_on(&app, file, ImportMode::Replace).expect("replaced");
    assert_eq!(
        said,
        "2 items imported. One category was added. 2 not in the file removed and 1 taken off \
         the menu."
    );
    let rows = menu_rows_on(&app).expect("rows");
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert!(
        names.contains(&"Idli") && names.contains(&"Masala dosa"),
        "{names:?}"
    );
    assert!(!names.contains(&"Water bottle"), "{names:?}");
    assert_eq!(rows.iter().filter(|r| r.name == "Tea").count(), 1);
    let dosa = rows
        .iter()
        .find(|r| r.id == "itm_dosa")
        .expect("kept for its size");
    assert!(!dosa.is_available, "taken off the menu, not deleted");
    let categories = crate::menu::categories_on(&app).expect("categories");
    let bottles = categories
        .iter()
        .find(|c| c.name == "Bottles")
        .expect("still a row");
    assert!(!bottles.is_active, "left empty, so retired");

    // Replacing with the export of what is there now is a no-op.
    let out = exported(&app, &scratch, "menu.csv");
    let again = plan_import_on(&app, &out, ImportMode::Replace).expect("planned");
    assert!(again.is_clean && again.is_empty, "{again:?}");
}

/// A plain list without categories updates by name; a name the shop has twice is asked about
/// rather than guessed at; a category column settles it.
#[test]
fn a_name_finds_its_item_and_a_repeated_name_is_asked_about() {
    let scratch = Scratch::new("csv_names");
    let app = a_shop_with_a_menu(&scratch);

    // Two items called Omlet, in two categories.
    run_import_on(
        &app,
        "category,name,price\nChinese,Omlet,40\nNorth Indian,Omlet,50\n",
        ImportMode::Update,
    )
    .expect("two omlets");
    assert_eq!(
        menu_rows_on(&app)
            .expect("rows")
            .iter()
            .filter(|r| r.name == "Omlet")
            .count(),
        2
    );

    // No category: "Tea" is one item, so it updates; "Omlet" is two, so the row is refused
    // with the reason.
    let plain = plan_import_on(&app, "name,price\nTEA,22\nOmlet,45\n", ImportMode::Update)
        .expect("planned");
    assert_eq!(plain.updated_items, 1, "{plain:?}");
    assert_eq!(plain.refused.len(), 1, "{plain:?}");
    assert!(
        plain.refused[0].contains("more than one item called Omlet"),
        "{:?}",
        plain.refused
    );

    // With the category, each row finds its own.
    let placed = plan_import_on(
        &app,
        "category,name,price\nChinese,omlet,45\nNorth Indian,OMLET,55\n",
        ImportMode::Update,
    )
    .expect("planned");
    assert!(placed.is_clean, "{:?}", placed.refused);
    assert_eq!((placed.new_items, placed.updated_items), (0, 2));

    // The same item twice in one file is refused, naming the first line.
    let twice =
        plan_import_on(&app, "name,price\nTea,22\ntea,23\n", ImportMode::Update).expect("planned");
    assert_eq!(twice.refused.len(), 1, "{twice:?}");
    assert!(twice.refused[0].contains("line 2"), "{:?}", twice.refused);
}

/// Tabs from a paste out of Excel, a semicolon file, a rate for the tax, a rupee sign in the
/// price: all of it reads.
#[test]
fn a_file_typed_by_hand_reads() {
    let scratch = Scratch::new("csv_hand");
    let app = a_shop_with_a_menu(&scratch);

    let tabs = "Item\tRate\tGST\nPaneer Tikka\t₹ 240\t18%\nLassi\tRs. 60\t5\n";
    let plan = plan_import_on(&app, tabs, ImportMode::Update).expect("planned");
    assert!(plan.is_clean, "{:?}", plan.refused);
    assert_eq!(plan.new_items, 2);
    run_import_on(&app, tabs, ImportMode::Update).expect("imported");
    let rows = menu_rows_on(&app).expect("rows");
    let tikka = rows
        .iter()
        .find(|r| r.name == "Paneer Tikka")
        .expect("tikka");
    assert_eq!(tikka.price.text, "240.00");
    assert_eq!(tikka.tax_class_id, "tax_packaged_18");
    let lassi = rows.iter().find(|r| r.name == "Lassi").expect("lassi");
    assert_eq!(lassi.tax_class_id, "tax_food_5");

    // A price below zero is not a price, and a rate the shop does not have is named.
    let bad = plan_import_on(
        &app,
        "name;price;tax\nGhost;-5;5%\nFree;10;12%\n",
        ImportMode::Update,
    )
    .expect("planned");
    assert_eq!(bad.refused.len(), 2, "{bad:?}");
    assert!(bad.refused[0].contains("Line 2"), "{:?}", bad.refused);
    assert!(bad.refused[1].contains("12%"), "{:?}", bad.refused);
}

/// The cost column travels with the owner, who may see margins — in rupees, like the price.
#[test]
fn the_cost_column_is_only_for_those_who_may_see_it() {
    let scratch = Scratch::new("csv_cost");
    let app = a_shop_with_a_menu(&scratch);
    let mut dosa = menu_rows_on(&app)
        .expect("rows")
        .into_iter()
        .find(|r| r.id == "itm_dosa")
        .map(|r| crate::menu::MenuEdit {
            id: r.id,
            name: r.name,
            category_id: r.category_id,
            price: r.price.text,
            tax_class_id: None,
            price_basis: None,
            hsn: None,
            short_code: None,
            cost: Some("45".to_owned()),
            is_open_price: false,
            is_available: true,
            course: None,
            prep_minutes: None,
        })
        .expect("dosa");
    dosa.cost = Some("45".to_owned());
    save_item_on(&app, dosa).expect("cost set");

    let csv = exported(&app, &scratch, "owner.csv");
    assert!(csv.contains(",45.00,"), "the owner sees the cost: {csv}");
}

/// A half plate is its own price, not a discount.
#[test]
fn a_size_is_a_price_of_its_own() {
    let scratch = Scratch::new("variants");
    let app = a_shop_with_a_menu(&scratch);

    let made = save_variant_on(
        &app,
        "itm_dosa".to_owned(),
        "var_dosa_half".to_owned(),
        "Half".to_owned(),
        "70".to_owned(),
        true,
    )
    .expect("a half dosa");

    assert_eq!(made.item_name, "Masala dosa");
    assert_eq!(made.variants.len(), 1);
    assert_eq!(made.variants[0].name, "Half");
    assert_eq!(made.variants[0].price.text, "70.00");
    assert_eq!(
        price_of(&app, "itm_dosa"),
        "120.00",
        "and the full plate is untouched"
    );

    // Editing it keeps the one row rather than growing a second Half.
    let again = save_variant_on(
        &app,
        "itm_dosa".to_owned(),
        "var_dosa_half".to_owned(),
        "Half".to_owned(),
        "75".to_owned(),
        true,
    )
    .expect("edited");
    assert_eq!(again.variants.len(), 1);
    assert_eq!(again.variants[0].price.text, "75.00");

    let nameless = save_variant_on(
        &app,
        "itm_dosa".to_owned(),
        "var_x".to_owned(),
        "   ".to_owned(),
        "10".to_owned(),
        true,
    )
    .expect_err("a size needs a name");
    assert_eq!(nameless.code, "menu.variant_name");
}

/// A group is made once and offered on many items — a shop has one "Spice level", not one per
/// curry.
#[test]
fn a_group_of_choices_is_shared_and_says_its_rule_in_words() {
    let scratch = Scratch::new("groups");
    let app = a_shop_with_a_menu(&scratch);

    let groups = save_group_on(
        &app,
        GroupEdit {
            id: "grp_spice".to_owned(),
            name: "Spice level".to_owned(),
            min_select: 1,
            max_select: Some(1),
            modifiers: vec![
                ModifierEdit {
                    id: "mod_mild".to_owned(),
                    name: "Mild".to_owned(),
                    price_delta: String::new(),
                },
                ModifierEdit {
                    id: "mod_extra".to_owned(),
                    name: "Extra spicy".to_owned(),
                    price_delta: "10".to_owned(),
                },
                ModifierEdit {
                    id: "mod_none".to_owned(),
                    name: "No onion".to_owned(),
                    price_delta: "-5".to_owned(),
                },
            ],
        },
    )
    .expect("a group");

    let spice = groups.iter().find(|g| g.id == "grp_spice").expect("saved");
    assert_eq!(spice.rule, "Choose one", "the rule, in words");
    assert_eq!(spice.modifiers.len(), 3);
    let choice = |name: &str| {
        spice
            .modifiers
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("{name} is in the group"))
            .price_delta
            .clone()
    };
    assert_eq!(choice("Mild").paise, 0, "blank is free");
    assert_eq!(choice("Extra spicy").paise, 1000);
    // A minus survives. Stripping it would quietly charge for "no onion".
    assert_eq!(
        choice("No onion").paise,
        -500,
        "a negative delta stays negative"
    );

    // Offered on the dosa, and on nothing else until it is.
    let dosa = attach_group_on(&app, "itm_dosa".to_owned(), "grp_spice".to_owned(), true)
        .expect("offered");
    assert!(
        dosa.groups
            .iter()
            .any(|g| g.id == "grp_spice" && g.attached)
    );

    let tea = item_composition_on(&app, "itm_tea".to_owned()).expect("the tea");
    assert!(
        tea.groups
            .iter()
            .any(|g| g.id == "grp_spice" && !g.attached),
        "every group is offered to the screen; only the ticked ones are on the item"
    );

    let off = attach_group_on(&app, "itm_dosa".to_owned(), "grp_spice".to_owned(), false)
        .expect("withdrawn");
    assert!(
        off.groups
            .iter()
            .any(|g| g.id == "grp_spice" && !g.attached)
    );
    assert_eq!(
        list_groups_on(&app).expect("still listed").len(),
        1,
        "withdrawing it from an item does not destroy the group"
    );
}

/// An impossible group is refused when it is written, not when a cashier meets it mid-rush.
#[test]
fn a_group_that_cannot_be_satisfied_is_refused() {
    let scratch = Scratch::new("bad_group");
    let app = a_shop_with_a_menu(&scratch);

    let err = save_group_on(
        &app,
        GroupEdit {
            id: "grp_impossible".to_owned(),
            name: "Pick three".to_owned(),
            min_select: 3,
            max_select: Some(1),
            modifiers: vec![ModifierEdit {
                id: "mod_a".to_owned(),
                name: "A".to_owned(),
                price_delta: String::new(),
            }],
        },
    )
    .expect_err("at least 3 of at most 1 is nobody's rule");
    assert!(!err.message.is_empty());
    assert!(list_groups_on(&app).expect("listed").is_empty());
}

/// A combo's price is shared by what is in it, and the shares add back to the price exactly —
/// which is the whole reason a mixed-rate combo can be billed at all.
#[test]
fn a_combo_shares_its_price_across_two_different_tax_rates() {
    let scratch = Scratch::new("combos");
    let app = a_shop_with_a_menu(&scratch);

    // Dosa 120 at 5%, water 20 at 18% — separately 140, sold for 130.
    let combos = save_combo_on(
        &app,
        ComboEdit {
            id: "cmb_lunch".to_owned(),
            name: "Lunch deal".to_owned(),
            price: "130".to_owned(),
            is_active: true,
            parts: vec![
                ("itm_dosa".to_owned(), "1".to_owned()),
                ("itm_water".to_owned(), "1".to_owned()),
            ],
        },
    )
    .expect("a combo");

    let lunch = combos.first().expect("saved");
    assert_eq!(lunch.price.text, "130.00");
    assert_eq!(lunch.separately.text, "140.00", "what it gives away");
    assert_eq!(lunch.parts.len(), 2);

    let total: i64 = lunch.parts.iter().map(|p| p.share.paise).sum();
    assert_eq!(total, 13000, "the shares add back to the price EXACTLY");

    let dosa = lunch
        .parts
        .iter()
        .find(|p| p.item_id == "itm_dosa")
        .expect("the dosa");
    let water = lunch
        .parts
        .iter()
        .find(|p| p.item_id == "itm_water")
        .expect("the water");
    assert_eq!(dosa.rate, "5%");
    assert_eq!(water.rate, "18%");
    assert!(
        dosa.share.paise > water.share.paise,
        "the dosa carries more of it"
    );

    // The shares follow today's prices, not the day the combo was made.
    save_item_on(
        &app,
        MenuEdit {
            id: "itm_water".to_owned(),
            name: "Water bottle".to_owned(),
            category_id: None,
            price: "40".to_owned(),
            tax_class_id: None,
            price_basis: None,
            hsn: None,
            short_code: None,
            cost: None,
            is_open_price: false,
            is_available: true,
            course: None,
            prep_minutes: None,
        },
    )
    .expect("the water went up");

    let after = list_combos_on(&app).expect("relisted");
    let lunch = after.first().expect("still there");
    assert_eq!(lunch.separately.text, "160.00");
    let water_now = lunch
        .parts
        .iter()
        .find(|p| p.item_id == "itm_water")
        .expect("the water");
    assert!(
        water_now.share.paise > water.share.paise,
        "a dearer part carries a bigger slice: {} then {}",
        water.share.paise,
        water_now.share.paise
    );
    let total: i64 = lunch.parts.iter().map(|p| p.share.paise).sum();
    assert_eq!(total, 13000, "and it still adds back exactly");
}

/// An empty combo is refused.
#[test]
fn a_combo_with_nothing_in_it_is_refused() {
    let scratch = Scratch::new("empty_combo");
    let app = a_shop_with_a_menu(&scratch);

    let err = save_combo_on(
        &app,
        ComboEdit {
            id: "cmb_nothing".to_owned(),
            name: "Nothing".to_owned(),
            price: "100".to_owned(),
            is_active: true,
            parts: Vec::new(),
        },
    )
    .expect_err("refused");
    assert_eq!(err.code, "menu.combo_empty");
    assert!(list_combos_on(&app).expect("listed").is_empty());
}
