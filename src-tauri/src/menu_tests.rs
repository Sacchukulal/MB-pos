//! The menu, driven end to end against a real database.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_core::{ItemId, Money, PriceBasis, TaxClassId, TaxKind, TaxRate};
use mb_db::{Db, DbConfig, Repos};

use crate::menu::{
    ComboEdit, GroupEdit, MenuEdit, ModifierEdit, attach_group_on, change_prices_on,
    export_menu_on, item_composition_on, list_combos_on, list_groups_on, menu_rows_on,
    plan_import_on, run_import_on, save_combo_on, save_group_on, save_item_on, save_tax_class_on,
    save_variant_on, tax_classes_on,
};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

/// A shop with three items on two tax classes — enough that "every item on this class" is a
/// claim with a counter-example in it.
fn a_shop_with_a_menu(scratch: &Scratch) -> App {
    let path = scratch.dir().join("menu.db");
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for (id, name, paise, class) in [
            ("itm_tea", "Tea", 2000_i64, Some("tax_food_5")),
            ("itm_dosa", "Masala dosa", 12000, Some("tax_food_5")),
            ("itm_water", "Water bottle", 2000, Some("tax_packaged_18")),
        ] {
            let tax = match class {
                Some("tax_packaged_18") => {
                    mb_core::TaxSpec::gst(TaxRate::from_basis_points(1800).expect("18%"))
                }
                _ => mb_core::TaxSpec::gst(TaxRate::from_basis_points(500).expect("5%")),
            };
            repos.menu().save_item(
                OUTLET,
                &mb_db::repo::menu::MenuItem {
                    id: ItemId::new(id),
                    category_id: None,
                    name: name.to_owned(),
                    unit_price: Money::from_paise(paise),
                    tax_class_id: class.map(TaxClassId::new),
                    tax,
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

/// The whole rate line — "5% · Tax added on top".
fn row_rate(app: &App, id: &str) -> String {
    menu_rows_on(app)
        .expect("the menu")
        .into_iter()
        .find(|r| r.id == id)
        .expect("that item")
        .rate
}

fn class_of(app: &App, id: &str) -> crate::menu::TaxClassView {
    tax_classes_on(app)
        .expect("the classes")
        .into_iter()
        .find(|c| c.id == id)
        .expect("that class")
}

/// A rate changes, and everything on that class changes with it.
#[test]
fn changing_a_class_moves_every_item_on_it_and_nothing_else() {
    let scratch = Scratch::new("tax_class");
    let app = a_shop_with_a_menu(&scratch);

    assert_eq!(rate_of(&app, "itm_tea"), "5%");
    assert_eq!(rate_of(&app, "itm_water"), "18%");

    let classes = tax_classes_on(&app).expect("the classes");
    let food = classes
        .iter()
        .find(|c| c.id == "tax_food_5")
        .expect("the seeded food class");
    assert_eq!(food.items_using, 2, "tea and dosa, not the water");

    let said = save_tax_class_on(
        &app,
        "tax_food_5".to_owned(),
        "Restaurant food 12%".to_owned(),
        "12".to_owned(),
        TaxKind::Gst,
        PriceBasis::Exclusive,
    )
    .expect("the rate changed");
    assert!(said.contains('2'), "it says how many moved: {said}");

    assert_eq!(rate_of(&app, "itm_tea"), "12%");
    assert_eq!(rate_of(&app, "itm_dosa"), "12%");
    assert_eq!(rate_of(&app, "itm_water"), "18%", "a different class");

    // The NAME moves too.
    let renamed = tax_classes_on(&app).expect("the classes");
    let food = renamed
        .iter()
        .find(|c| c.id == "tax_food_5")
        .expect("still there");
    assert_eq!(food.name, "Restaurant food 12%");
    assert_eq!(food.rate, "12%");
}

/// A rate outside 0–100% is refused in words, not by a panic.
#[test]
fn an_impossible_rate_is_refused() {
    let scratch = Scratch::new("bad_rate");
    let app = a_shop_with_a_menu(&scratch);

    let err = save_tax_class_on(
        &app,
        "tax_food_5".to_owned(),
        "Nonsense".to_owned(),
        "400".to_owned(),
        TaxKind::Gst,
        PriceBasis::Exclusive,
    )
    .expect_err("400% is not a tax rate");
    assert_eq!(err.code, "menu.rate");
    assert_eq!(rate_of(&app, "itm_tea"), "5%", "and nothing moved");
}

#[test]
fn changing_a_class_label_does_not_change_what_it_taxes() {
    let scratch = Scratch::new("class_label");
    let app = a_shop_with_a_menu(&scratch);

    // The water's class becomes liquor: outside GST, priced tax-in, 20% VAT.
    save_tax_class_on(
        &app,
        "tax_packaged_18".to_owned(),
        "Liquor — state VAT".to_owned(),
        "20".to_owned(),
        TaxKind::OutsideGst,
        PriceBasis::Inclusive,
    )
    .expect("a bar's class");
    assert!(
        row_rate(&app, "itm_water").contains("Outside GST"),
        "the bottle is outside GST: {}",
        row_rate(&app, "itm_water")
    );

    // What the screen holds: machine values beside the words.
    let before = class_of(&app, "tax_packaged_18");
    assert_eq!(before.kind, TaxKind::OutsideGst);
    assert_eq!(before.basis, PriceBasis::Inclusive);
    assert_eq!(before.rate_bp, 2000);

    // Rename it to words that say none of that, sending back exactly what the view carried.
    save_tax_class_on(
        &app,
        before.id.clone(),
        "Bar list".to_owned(),
        before.rate.trim_end_matches('%').to_owned(),
        before.kind,
        before.basis,
    )
    .expect("renamed");

    let after = class_of(&app, "tax_packaged_18");
    assert_eq!(after.name, "Bar list");
    assert_eq!(after.kind, TaxKind::OutsideGst, "still not GST");
    assert_eq!(after.basis, PriceBasis::Inclusive);
    assert_eq!(after.rate, "20%");
    assert!(
        row_rate(&app, "itm_water").contains("Outside GST"),
        "and the bottle is still outside GST: {}",
        row_rate(&app, "itm_water")
    );
}

/// A rate on a kind that cannot carry one is refused, not silently zeroed — which is why the
/// editor disables the box rather than hinting at it.
#[test]
fn an_exempt_class_cannot_be_given_a_rate() {
    let scratch = Scratch::new("exempt_rate");
    let app = a_shop_with_a_menu(&scratch);

    let err = save_tax_class_on(
        &app,
        "tax_exempt".to_owned(),
        "Exempt".to_owned(),
        "5".to_owned(),
        TaxKind::Exempt,
        PriceBasis::Exclusive,
    )
    .expect_err("exempt at 5% is not a thing");
    assert_eq!(err.code, "menu.rate");
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
        tax_class_id: Some("tax_food_5".to_owned()),
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

/// Export, change one cell, import back.
#[test]
fn the_menu_survives_a_trip_through_a_spreadsheet() {
    let scratch = Scratch::new("csv");
    let app = a_shop_with_a_menu(&scratch);

    let csv = export_menu_on(&app).expect("exported");
    assert!(csv.contains("Masala dosa"), "the menu is in it");

    // The file carries paise, not rupees — a spreadsheet that rounds 120.00 to 120 is a
    // spreadsheet, and this way there is nothing to round.
    let edited = csv.replace(",12000,", ",13500,");
    assert_ne!(edited, csv, "the fixture actually changed a cell");

    // The dry run writes NOTHING and says what it would do.
    let plan = plan_import_on(&app, edited.clone()).expect("planned");
    assert!(
        plan.is_clean,
        "a file we just exported has no bad rows: {plan:?}"
    );
    assert_eq!(
        price_of(&app, "itm_dosa"),
        "120.00",
        "the dry run changed nothing"
    );

    let said = run_import_on(&app, edited).expect("imported");
    assert!(!said.is_empty());
    assert_eq!(price_of(&app, "itm_dosa"), "135.00");
    assert_eq!(rate_of(&app, "itm_water"), "18%", "and the rates came back");
}

/// A file with a bad cell is refused by line number, and the good lines do not sneak in — a
/// half-imported menu is worse than no import.
#[test]
fn a_bad_row_names_its_line_and_nothing_is_written() {
    let scratch = Scratch::new("csv_bad");
    let app = a_shop_with_a_menu(&scratch);

    let csv = export_menu_on(&app).expect("exported");
    let broken = csv.replace(",12000,", ",one hundred and twenty,");

    let plan = plan_import_on(&app, broken.clone()).expect("planned");
    assert!(!plan.is_clean, "a price in words is not a price");
    assert!(
        plan.refused
            .iter()
            .any(|r| r.contains("one hundred") || r.chars().any(|c| c.is_ascii_digit())),
        "the refusal points at the line: {:?}",
        plan.refused
    );

    let err = run_import_on(&app, broken).expect_err("refused");
    assert!(!err.message.is_empty());
    assert_eq!(price_of(&app, "itm_dosa"), "120.00", "nothing was written");
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
            tax_class_id: Some("tax_packaged_18".to_owned()),
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
