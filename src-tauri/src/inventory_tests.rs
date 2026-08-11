//! **The stock book, driven end to end** — P25.
//!
//! `mb-db`'s `tests/stock.rs` proves the arithmetic. This proves the SEQUENCE a
//! person actually performs: add a material with a pack, write a recipe in the
//! pack's units, sell something, and read the screen back — because every unit
//! conversion and every sentence happens at this layer, and a screen that shows
//! "50000" where a shop said "2 bags" is a screen nobody uses.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_db::{Db, DbConfig, Repos};

use crate::inventory::{
    MaterialEdit, MovementEdit, PackEdit, RecipeEdit, RecipeLineEdit, buy_list_text_on, inventory_on,
    rebuild_balances_on, record_movement_on, recipe_on, save_material_on, save_recipe_on,
};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

fn a_shop(scratch: &Scratch) -> App {
    let path = scratch.dir().join("stock.db");
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    db.transaction(|tx| {
        Repos::new(tx).menu().save_item(
            OUTLET,
            &mb_db::repo::menu::MenuItem {
                id: mb_core::ItemId::new("itm_dosa"),
                category_id: None,
                name: "Masala Dosa".to_owned(),
                unit_price: mb_core::Money::from_paise(12_000),
                tax_rate: mb_core::TaxRate::GST_5,
                tax_treatment: mb_core::TaxTreatment::Exclusive,
                tax_class_id: None,
                hsn: None,
                // **D119** — P13's typed cost, deliberately wrong, so the
                // recipe screen has a gap to show.
                cost_price: Some(mb_core::Money::from_paise(2_000)),
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

    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    // **The stock book is behind `Feature::Inventory`** (D86's gate, applied to
    // the SCREENS and never to the settle path). A test that did not license
    // the shop would be testing the refusal, which `licence_tests` already
    // does.
    app.use_licensing(crate::licence_tests::licence_in(
        scratch,
        "stock-licence",
        mb_license::Status::Active,
        90,
    ));
    app
}

fn rice(app: &App) -> MaterialEdit {
    let edit = MaterialEdit {
        id: "mat_rice".to_owned(),
        name: "Rice".to_owned(),
        dimension: "weight".to_owned(),
        category: "Dry goods".to_owned(),
        buy_from: "Metro".to_owned(),
        // **Typed in bags, stored in grams.** This is the conversion under
        // test: a shopkeeper says "order more when we are down to one bag".
        reorder_level: "1".to_owned(),
        reorder_qty: "2".to_owned(),
        reorder_unit: "bag".to_owned(),
        is_perishable: false,
        shelf_life_days: None,
        is_active: true,
        packs: vec![PackEdit {
            name: "bag".to_owned(),
            size: "25".to_owned(),
            unit: "kg".to_owned(),
        }],
        purchase_unit: "bag".to_owned(),
        recipe_unit: "g".to_owned(),
    };
    save_material_on(app, edit.clone()).expect("saved");
    edit
}

/// **A shop says "a bag is 25 kg" once, and every screen after it agrees.**
#[test]
fn a_pack_is_typed_once_and_read_back_everywhere() {
    let scratch = Scratch::new("stock_pack");
    let app = a_shop(&scratch);
    rice(&app);

    let view = inventory_on(&app, None).expect("the screen");
    let material = view.materials.iter().find(|m| m.id == "mat_rice").expect("there");

    // The reorder level was typed as "1 bag" and comes back as one bag, not as
    // 25,000 g and not as 25 kg.
    assert_eq!(material.reorder_level, "1 bag", "{material:?}");
    assert_eq!(material.reorder_qty, "2 bag");
    assert_eq!(material.base_unit, "g");
    // kg came free with the dimension; bag is the shop's own (D108).
    let names: Vec<&str> = material.units.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, vec!["g", "kg", "bag"]);
    assert!(material.units.iter().find(|u| u.name == "kg").expect("kg").is_standard);
    assert!(!material.units.iter().find(|u| u.name == "bag").expect("bag").is_standard);
    // Nothing has ever been bought, so there is no cost and the screen says so
    // rather than showing ₹0.00.
    assert_eq!(material.cost, "");
    assert_eq!(material.cost_when, "never priced");
    // **D115.**
    assert_eq!(material.last_counted, "never counted");
}

/// Buy two bags, sell forty plates, and read the shelf back in the words a
/// person would use.
#[test]
fn the_whole_sequence_a_shopkeeper_performs() {
    let scratch = Scratch::new("stock_seq");
    let app = a_shop(&scratch);
    rice(&app);

    // A recipe, typed in grams because that is how a dish is cooked.
    let view = save_recipe_on(
        &app,
        RecipeEdit {
            owner_kind: "item".to_owned(),
            owner_id: "itm_dosa".to_owned(),
            batch_qty: "1".to_owned(),
            batch_unit: String::new(),
            lines: vec![RecipeLineEdit {
                material_id: "mat_rice".to_owned(),
                qty: "180".to_owned(),
                unit: "g".to_owned(),
                yield_percent: 100,
            }],
        },
    )
    .expect("a recipe");
    assert!(view.exists);
    assert_eq!(view.lines[0].qty, "180");
    assert_eq!(view.lines[0].unit, "g");
    // Nothing has been priced, so the cost is zero AND the screen names the
    // material rather than quietly presenting ₹0.00 as a food cost.
    assert_eq!(view.cost.paise, 0);
    assert_eq!(view.unpriced.len(), 1);
    assert!(view.unpriced[0].contains("Rice"), "{:?}", view.unpriced);

    // Two bags in, at ₹1,500 a bag — a price typed per PACK, which is what a
    // shop knows.
    record_movement_on(
        &app,
        MovementEdit {
            material_id: "mat_rice".to_owned(),
            kind: "purchase".to_owned(),
            qty: "2".to_owned(),
            unit: "bag".to_owned(),
            reason_id: None,
            note: None,
            cost: Some("1500".to_owned()),
        },
    )
    .expect("bought");

    let view = inventory_on(&app, None).expect("the screen");
    let material = view.materials.iter().find(|m| m.id == "mat_rice").expect("there");
    assert_eq!(material.on_hand, "2 bag");
    assert_eq!(material.cost, "₹60.00 per bag".replace("60.00", "1,500.00"));
    // 50 kg at ₹60 a kilo is ₹3,000 of rice on the shelf.
    assert_eq!(material.value.paise, 300_000);
    assert!(!material.is_low, "two bags is not low when the level is one");

    // Forty plates.
    for n in 0..40 {
        sell_one(&app, n);
    }

    let view = inventory_on(&app, None).expect("the screen");
    let material = view.materials.iter().find(|m| m.id == "mat_rice").expect("there");
    assert_eq!(material.on_hand, "1.712 bag", "42,800 g read the way a person would");
    assert!(!material.is_negative);
    // The movements list shows what was typed, not the base units.
    let bought = view.movements.iter().find(|m| m.kind_tag == "purchase").expect("there");
    assert_eq!(bought.qty, "+2 bag");
    assert_eq!(bought.kind, "Bought");
    let sold = view.movements.iter().find(|m| m.kind_tag == "sale").expect("there");
    assert_eq!(sold.qty, "−180 g", "a minus sign, not a hyphen");
    assert!(sold.takes_out);

    // Now the food cost is real, and D119's gap is on the screen.
    let recipe = recipe_on(&app, "item".to_owned(), "itm_dosa".to_owned()).expect("recipe");
    assert_eq!(recipe.cost.paise, 1_080, "180 g at ₹60/kg");
    assert_eq!(recipe.typed_cost.expect("P13's figure").paise, 2_000);
    assert_eq!(recipe.sells_for.expect("the price").paise, 12_000);
    assert!(recipe.margin.contains("91"), "{}", recipe.margin);
    assert!(recipe.unpriced.is_empty(), "the rice has a price now");
}

/// The buy list counts in the pack you buy in, and reads as a sentence you can
/// send to somebody (scope 4.6, D116).
#[test]
fn the_buy_list_is_a_message_a_person_can_send() {
    let scratch = Scratch::new("stock_buy");
    let app = a_shop(&scratch);
    rice(&app);

    record_movement_on(
        &app,
        MovementEdit {
            material_id: "mat_rice".to_owned(),
            kind: "opening".to_owned(),
            qty: "12".to_owned(),
            unit: "kg".to_owned(),
            reason_id: None,
            note: None,
            cost: None,
        },
    )
    .expect("opening");

    let view = inventory_on(&app, None).expect("the screen");
    let material = view.materials.iter().find(|m| m.id == "mat_rice").expect("there");
    assert!(material.is_low, "12 kg is under the one-bag level");
    assert_eq!(material.buy, "2 bag");

    assert_eq!(view.buy_list.len(), 1);
    assert_eq!(view.buy_list[0].buy_from, "Metro");
    assert_eq!(view.buy_list[0].lines[0].line, "Rice — have 12 kg, buy 2 bag");

    let text = buy_list_text_on(&app).expect("text");
    println!("\n{text}");
    assert!(text.contains("Metro"));
    assert!(text.contains("Rice — have 12 kg, buy 2 bag"));
}

/// **A recipe that goes round in circles is refused in words a person reads**,
/// and D75 is what makes it reach the screen as itself.
#[test]
fn a_loop_reaches_the_screen_as_a_sentence_and_not_as_a_storage_error() {
    let scratch = Scratch::new("stock_loop");
    let app = a_shop(&scratch);

    for (id, name) in [("mat_a", "Paste"), ("mat_b", "Masala")] {
        save_material_on(
            &app,
            MaterialEdit {
                id: id.to_owned(),
                name: name.to_owned(),
                dimension: "weight".to_owned(),
                category: String::new(),
                buy_from: String::new(),
                reorder_level: String::new(),
                reorder_qty: String::new(),
                reorder_unit: String::new(),
                is_perishable: false,
                shelf_life_days: None,
                is_active: true,
                packs: vec![],
                purchase_unit: String::new(),
                recipe_unit: String::new(),
            },
        )
        .expect("saved");
    }

    let batch = |owner: &str, uses: &str| RecipeEdit {
        owner_kind: "material".to_owned(),
        owner_id: owner.to_owned(),
        batch_qty: "1".to_owned(),
        batch_unit: "kg".to_owned(),
        lines: vec![RecipeLineEdit {
            material_id: uses.to_owned(),
            qty: "100".to_owned(),
            unit: "g".to_owned(),
            yield_percent: 100,
        }],
    };

    save_recipe_on(&app, batch("mat_a", "mat_b")).expect("a → b");
    let refused = save_recipe_on(&app, batch("mat_b", "mat_a")).expect_err("b → a is a loop");
    println!("\n  {}", refused.message);
    assert!(refused.message.contains("Masala"), "{}", refused.message);
    assert!(refused.message.contains("Paste"), "{}", refused.message);
    assert!(
        !refused.message.contains("could not be read"),
        "a rule a person must act on arrived as a storage error — D75"
    );
}

/// **D114** — the screen checks the cache against the ledger every time it
/// opens, and Rebuild says how many it corrected.
#[test]
fn the_screen_notices_when_a_balance_stops_matching_its_movements() {
    let scratch = Scratch::new("stock_drift");
    let app = a_shop(&scratch);
    rice(&app);
    record_movement_on(
        &app,
        MovementEdit {
            material_id: "mat_rice".to_owned(),
            kind: "opening".to_owned(),
            qty: "10".to_owned(),
            unit: "kg".to_owned(),
            reason_id: None,
            note: None,
            cost: None,
        },
    )
    .expect("opening");

    let view = inventory_on(&app, None).expect("the screen");
    assert_eq!(view.cache_warning, "", "a fresh shop has nothing to warn about");

    // Corrupt the cache the way a bug or a hand-edited database would.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                tx.execute("UPDATE material_balances SET base_qty = 1", [])
                    .map_err(Into::into)
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("corrupted");

    let view = inventory_on(&app, None).expect("the screen");
    assert!(view.cache_warning.contains("1 material"), "{}", view.cache_warning);
    assert!(view.cache_warning.contains("Rebuild"), "{}", view.cache_warning);

    let view = rebuild_balances_on(&app).expect("rebuilt");
    assert_eq!(view.cache_warning, "");
    let material = view.materials.iter().find(|m| m.id == "mat_rice").expect("there");
    assert_eq!(material.on_hand, "10 kg");
}

/// A wastage entry is valued and reasoned, and is the report that catches
/// theft (scope 4.7).
#[test]
fn wastage_is_recorded_with_a_reason_and_a_value() {
    let scratch = Scratch::new("stock_waste");
    let app = a_shop(&scratch);
    rice(&app);
    record_movement_on(
        &app,
        MovementEdit {
            material_id: "mat_rice".to_owned(),
            kind: "purchase".to_owned(),
            qty: "1".to_owned(),
            unit: "bag".to_owned(),
            reason_id: None,
            note: None,
            cost: Some("1500".to_owned()),
        },
    )
    .expect("bought");

    let view = inventory_on(&app, None).expect("the screen");
    let reason = view.wastage_reasons.first().expect("a starting list").clone();
    assert!(!view.wastage_reasons.is_empty(), "a shop starts with wastage reasons");

    let view = record_movement_on(
        &app,
        MovementEdit {
            material_id: "mat_rice".to_owned(),
            kind: "wastage".to_owned(),
            qty: "500".to_owned(),
            unit: "g".to_owned(),
            reason_id: Some(reason.id.clone()),
            note: Some("left out overnight".to_owned()),
            cost: None,
        },
    )
    .expect("wasted");

    let wasted = view.movements.iter().find(|m| m.kind_tag == "wastage").expect("there");
    assert_eq!(wasted.qty, "−500 g");
    assert_eq!(wasted.kind, "Wasted");
    assert_eq!(wasted.reason, reason.text);
    // Valued without the caller passing a price — 500 g at ₹60/kg.
    assert_eq!(wasted.value.paise, 3_000);
}

/// **D55 — a session can see this screen, and must.**
///
/// Builds a shop with every state on the stock screen at once, in a folder that
/// becomes the app's whole `APPDATA`, so a demo can never touch a real shop's
/// data. Not part of the suite; run by hand and then look at the window.
///
/// ```text
/// $env:MB_DEMO="C:\some\scratch\demo"
/// cargo test -p magic-bill --bin magic-bill demo_stock -- --ignored --nocapture
/// $env:APPDATA="C:\some\scratch\demo"
/// cargo run -p magic-bill
/// ```
#[test]
#[ignore = "D55: run by hand to look at the screen, not part of the suite"]
fn demo_stock() {
    let Some(root) = std::env::var_os("MB_DEMO").map(std::path::PathBuf::from) else {
        panic!("set MB_DEMO to the folder that should become the demo's APPDATA");
    };
    let home = root.join("MagicBill");
    std::fs::create_dir_all(&home).expect("the demo folder");
    let db_path = home.join("magicbill.db");
    // Loudly, not `let _ =`: a demo seeded on top of the last one doubles every
    // figure, which cost ten minutes of reading numbers that were exactly twice
    // what they should have been. The usual cause is the app still running.
    for suffix in ["", "-wal", "-shm"] {
        let path = std::path::PathBuf::from(format!("{}{suffix}", db_path.display()));
        if path.exists() && std::fs::remove_file(&path).is_err() {
            panic!("{} is still open — close the app before seeding", path.display());
        }
    }

    let db = Db::open(&DbConfig::new(db_path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, db_path.clone());

    // **The stock screen is behind `Feature::Inventory`**, so the demo starts a
    // trial — rooted at the demo's OWN folder and against the machine id that
    // folder resolves to, so the `licence.json` this writes is the one the
    // running app reads when `APPDATA` points here.
    let machine = mb_license::MachineId::of(&home);
    let mut licensing = mb_license::Licensing::new(
        home.clone(),
        machine.clone(),
        std::sync::Arc::new(mb_license::cloud::Stub::active(
            &machine,
            crate::flows::today(crate::flows::now()),
            crate::flows::now(),
        )) as std::sync::Arc<dyn mb_license::Cloud>,
        env!("CARGO_PKG_VERSION"),
    );
    licensing
        .start_trial("+91 90000 00000", crate::flows::now(), std::time::Duration::from_secs(5))
        .expect("the stub starts a trial");
    app.use_licensing(licensing);

    // A menu, so the food-cost tab has dishes on it.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                for (id, name, price, cost) in [
                    ("itm_dosa", "Masala Dosa", 12_000_i64, Some(2_000_i64)),
                    ("itm_curry", "Paneer Butter Masala", 22_000, Some(9_000)),
                    ("itm_tea", "Tea", 2_000, None),
                ] {
                    repos.menu().save_item(
                        OUTLET,
                        &mb_db::repo::menu::MenuItem {
                            id: mb_core::ItemId::new(id),
                            category_id: None,
                            name: name.to_owned(),
                            unit_price: mb_core::Money::from_paise(price),
                            tax_rate: mb_core::TaxRate::GST_5,
                            tax_treatment: mb_core::TaxTreatment::Exclusive,
                            tax_class_id: None,
                            hsn: None,
                            cost_price: cost.map(mb_core::Money::from_paise),
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

    let material = |id: &str, name: &str, dimension: &str, pack: Option<(&str, &str, &str)>, buy: &str| MaterialEdit {
        id: id.to_owned(),
        name: name.to_owned(),
        dimension: dimension.to_owned(),
        category: "Kitchen".to_owned(),
        buy_from: buy.to_owned(),
        reorder_level: String::new(),
        reorder_qty: String::new(),
        reorder_unit: String::new(),
        is_perishable: false,
        shelf_life_days: None,
        is_active: true,
        packs: pack
            .map(|(n, s, u)| {
                vec![PackEdit { name: n.to_owned(), size: s.to_owned(), unit: u.to_owned() }]
            })
            .unwrap_or_default(),
        purchase_unit: pack.map_or(String::new(), |(n, _, _)| n.to_owned()),
        recipe_unit: String::new(),
    };

    // Rice, in bags, low enough to be on the buy list.
    let mut rice = material("mat_rice", "Rice", "weight", Some(("bag", "25", "kg")), "Metro");
    rice.reorder_level = "1".to_owned();
    rice.reorder_qty = "2".to_owned();
    rice.reorder_unit = "bag".to_owned();
    save_material_on(&app, rice).expect("rice");

    // Paneer, perishable, so D117's warning shows.
    let mut paneer = material("mat_paneer", "Paneer", "weight", None, "The milk van");
    paneer.is_perishable = true;
    paneer.shelf_life_days = Some(3);
    paneer.reorder_level = "2".to_owned();
    paneer.reorder_qty = "5".to_owned();
    paneer.reorder_unit = "kg".to_owned();
    save_material_on(&app, paneer).expect("paneer");

    for (id, name, dimension, buy) in [
        ("mat_tomato", "Tomato", "weight", "Vegetable market"),
        ("mat_onion", "Onion", "weight", "Vegetable market"),
        ("mat_oil", "Oil", "volume", "Metro"),
        ("mat_gravy", "Gravy base", "weight", ""),
    ] {
        save_material_on(&app, material(id, name, dimension, None, buy)).expect("material");
    }

    // Stock in, at real prices.
    for (id, qty, unit, cost) in [
        ("mat_rice", "12", "kg", "60"),
        ("mat_paneer", "3", "kg", "320"),
        ("mat_tomato", "20", "kg", "40"),
        ("mat_onion", "10", "kg", "30"),
        ("mat_oil", "15", "l", "150"),
    ] {
        record_movement_on(
            &app,
            MovementEdit {
                material_id: id.to_owned(),
                kind: "purchase".to_owned(),
                qty: qty.to_owned(),
                unit: unit.to_owned(),
                reason_id: None,
                note: None,
                cost: Some(cost.to_owned()),
            },
        )
        .expect("bought");
    }

    // A made material with a batch yield (D111), and two dishes that use it.
    save_recipe_on(
        &app,
        RecipeEdit {
            owner_kind: "material".to_owned(),
            owner_id: "mat_gravy".to_owned(),
            batch_qty: "4".to_owned(),
            batch_unit: "kg".to_owned(),
            lines: vec![
                RecipeLineEdit {
                    material_id: "mat_tomato".to_owned(),
                    qty: "3".to_owned(),
                    unit: "kg".to_owned(),
                    yield_percent: 100,
                },
                RecipeLineEdit {
                    material_id: "mat_onion".to_owned(),
                    qty: "1".to_owned(),
                    unit: "kg".to_owned(),
                    // Peeled, so 90% of what is issued reaches the pot (D110).
                    yield_percent: 90,
                },
            ],
        },
    )
    .expect("gravy");

    save_recipe_on(
        &app,
        RecipeEdit {
            owner_kind: "item".to_owned(),
            owner_id: "itm_dosa".to_owned(),
            batch_qty: "1".to_owned(),
            batch_unit: String::new(),
            lines: vec![
                RecipeLineEdit {
                    material_id: "mat_rice".to_owned(),
                    qty: "180".to_owned(),
                    unit: "g".to_owned(),
                    yield_percent: 100,
                },
                RecipeLineEdit {
                    material_id: "mat_oil".to_owned(),
                    qty: "20".to_owned(),
                    unit: "ml".to_owned(),
                    yield_percent: 100,
                },
            ],
        },
    )
    .expect("dosa");

    save_recipe_on(
        &app,
        RecipeEdit {
            owner_kind: "item".to_owned(),
            owner_id: "itm_curry".to_owned(),
            batch_qty: "1".to_owned(),
            batch_unit: String::new(),
            lines: vec![
                RecipeLineEdit {
                    material_id: "mat_gravy".to_owned(),
                    qty: "150".to_owned(),
                    unit: "g".to_owned(),
                    yield_percent: 100,
                },
                RecipeLineEdit {
                    material_id: "mat_paneer".to_owned(),
                    qty: "120".to_owned(),
                    unit: "g".to_owned(),
                    yield_percent: 100,
                },
            ],
        },
    )
    .expect("curry");

    // Sell some of each, so the ledger has sales, an automatic production and a
    // material that went below zero.
    for n in 0..12 {
        sell(&app, "itm_dosa", n);
    }
    for n in 0..30 {
        sell(&app, "itm_curry", 100 + n);
    }
    // And tea, which has no recipe — the coverage problem an owner has to see.
    for n in 0..8 {
        sell(&app, "itm_tea", 200 + n);
    }

    // Wastage, with a reason, so the theft report has something in it.
    let reasons = inventory_on(&app, None).expect("the screen").wastage_reasons;
    record_movement_on(
        &app,
        MovementEdit {
            material_id: "mat_paneer".to_owned(),
            kind: "wastage".to_owned(),
            qty: "400".to_owned(),
            unit: "g".to_owned(),
            reason_id: reasons.first().map(|r| r.id.clone()),
            note: Some("left out overnight".to_owned()),
            cost: None,
        },
    )
    .expect("wasted");

    std::fs::write(mb_db::locate::config_path(&home), db_path.display().to_string())
        .expect("the location file");

    let view = inventory_on(&app, None).expect("the screen");
    println!("demo ready: {}", db_path.display());
    println!("  {}", view.summary);
    for m in &view.materials {
        println!("    {:<14} {:>12}   {}", m.name, m.on_hand, m.cost);
    }
    for p in &view.problems {
        println!("    ! {}", p.sentence);
    }
    println!("now: $env:APPDATA=\"{}\"; cargo run -p magic-bill", root.display());
}

/// One bill of one item, for the demo.
fn sell(app: &App, item: &str, n: u32) {
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(app, item.to_owned(), Some("1".to_owned()), None).expect("added");
    let bill = app.with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total)).expect("bill");
    app.with_cart_mut(|state| {
        let payment = mb_core::Payment::new(mb_core::PaymentMode::Cash, bill).expect("cash");
        state.settlement.add(payment).map_err(|e| {
            crate::words::UiError::new("bill.pay", "That payment could not be taken.")
                .with_detail(e.to_string())
        })
    })
    .expect("paid");
    crate::flows::complete_bill_on(app).unwrap_or_else(|e| panic!("bill {n} did not settle: {e:?}"));
}

/// One bill, through the real billing path.
fn sell_one(app: &App, n: u32) {
    app.with_cart_mut(|state| {
        state.order_type = mb_core::OrderType::Parcel;
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(app, "itm_dosa".to_owned(), Some("1".to_owned()), None)
        .expect("added");
    let bill = app.with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total)).expect("bill");
    app.with_cart_mut(|state| {
        let payment = mb_core::Payment::new(mb_core::PaymentMode::Cash, bill)
            .expect("a cash payment");
        state.settlement.add(payment).map_err(|e| {
            crate::words::UiError::new("bill.pay", "That payment could not be taken.")
                .with_detail(e.to_string())
        })
    })
    .expect("paid");
    crate::flows::complete_bill_on(app).unwrap_or_else(|e| panic!("bill {n} did not settle: {e:?}"));
}
