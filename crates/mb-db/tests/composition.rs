//! **Variants, modifiers and combos** — scope 6.1, 6.2, 6.3.
//!
//! The one that matters is the combo: a meal deal with a 5% dish and an 18%
//! drink in it has to produce two correct rate rows, and the money has to add
//! back to the paisa.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Scratch;
use common::shop::{self, OUTLET};
use mb_core::{ItemId, ModifierId, Money, Qty, Timestamp};
use mb_db::repo::composition::{Combo, ComboPart, Modifier, ModifierGroup, Variant};
use mb_db::Repos;

fn at(n: i64) -> Timestamp {
    Timestamp::from_millis(1_770_000_000_000 + n * 1_000)
}

/// **Scope 6.1.** Half and full are two prices on one item, and each is its own
/// thing to cook.
#[test]
fn an_item_can_have_sizes() {
    let scratch = Scratch::new("variants");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for (id, name, paise, order) in [
            ("itm_dosa_half", "Half", 8_000_i64, 0_i64),
            ("itm_dosa_full", "Full", 12_000, 1),
        ] {
            repos.composition().save_variant(
                OUTLET,
                &Variant {
                    id: ItemId::new(id),
                    item_id: ItemId::new("itm_dosa"),
                    name: name.to_owned(),
                    unit_price: Money::from_paise(paise),
                    sort_order: order,
                    is_active: true,
                },
                at(1),
            )?;
        }
        Ok(())
    })
    .expect("saved");

    let sizes = db
        .transaction(|tx| Repos::new(tx).composition().variants_of(&ItemId::new("itm_dosa")))
        .expect("variants");

    assert_eq!(sizes.len(), 2);
    assert_eq!(sizes[0].name, "Half");
    assert_eq!(sizes[0].unit_price, Money::from_paise(8_000));
    assert_eq!(sizes[1].unit_price, Money::from_paise(12_000));
    // A half dosa is not a discounted dosa: it carries its own price, so the
    // rate summary and the margin are both right.
    assert_ne!(sizes[0].unit_price, sizes[1].unit_price);
}

/// **T6 — the group rules are enforced, and in Rust.**
#[test]
fn a_group_that_says_choose_one_means_it() {
    let spice = ModifierGroup {
        id: "grp_spice".to_owned(),
        name: "Spice level".to_owned(),
        min_select: 1,
        max_select: Some(1),
        sort_order: 0,
        modifiers: vec![],
    };
    assert!(spice.check(1).is_ok());

    let none = spice.check(0).expect_err("nothing chosen was allowed");
    assert!(none.to_string().contains("choose a spice level"), "{none}");

    let two = spice.check(2).expect_err("two were allowed");
    assert!(two.to_string().contains("only one"), "{two}");

    // "Any number" really means any number.
    let addons = ModifierGroup {
        id: "grp_addons".to_owned(),
        name: "Add-ons".to_owned(),
        min_select: 0,
        max_select: None,
        sort_order: 1,
        modifiers: vec![],
    };
    assert!(addons.check(0).is_ok());
    assert!(addons.check(9).is_ok());
}

/// A group nobody could satisfy is refused when it is saved, not when a cashier
/// meets it at the counter.
#[test]
fn an_impossible_group_is_refused_at_the_moment_it_is_written() {
    let scratch = Scratch::new("group_impossible");
    let db = scratch.open();
    shop::build(&db);

    let refused = db
        .transaction(|tx| {
            Repos::new(tx).composition().save_group(
                OUTLET,
                &ModifierGroup {
                    id: "grp_bad".to_owned(),
                    name: "Sauces".to_owned(),
                    min_select: 3,
                    max_select: Some(1),
                    sort_order: 0,
                    modifiers: vec![],
                },
                at(1),
            )
        })
        .expect_err("an impossible group was saved");
    assert!(refused.to_string().contains("nobody can satisfy"), "{refused}");
}

/// **Scope 6.2**, round trip — including a negative delta, which is a real
/// line on a real menu.
#[test]
fn modifiers_round_trip_including_a_discount_one() {
    let scratch = Scratch::new("modifiers");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.composition().save_group(
            OUTLET,
            &ModifierGroup {
                id: "grp_addons".to_owned(),
                name: "Add-ons".to_owned(),
                min_select: 0,
                max_select: None,
                sort_order: 0,
                modifiers: vec![
                    Modifier {
                        id: ModifierId::new("mod_cheese"),
                        name: "Extra cheese".to_owned(),
                        price_delta: Money::from_paise(2_000),
                        sort_order: 0,
                        is_active: true,
                    },
                    Modifier {
                        id: ModifierId::new("mod_no_onion"),
                        name: "No onion".to_owned(),
                        price_delta: Money::from_paise(-1_000),
                        sort_order: 1,
                        is_active: true,
                    },
                ],
            },
            at(1),
        )?;
        repos
            .composition()
            .attach_group(OUTLET, &ItemId::new("itm_dosa"), "grp_addons", 0, at(1))
    })
    .expect("saved");

    let groups = db
        .transaction(|tx| {
            Repos::new(tx)
                .composition()
                .groups_for_item(OUTLET, &ItemId::new("itm_dosa"))
        })
        .expect("groups");

    assert_eq!(groups.len(), 1);
    let addons = &groups[0];
    assert_eq!(addons.modifiers.len(), 2);
    assert_eq!(addons.modifiers[0].price_delta, Money::from_paise(2_000));
    assert_eq!(
        addons.modifiers[1].price_delta,
        Money::from_paise(-1_000),
        "a negative delta did not survive — \"no cheese, -10\" is a real menu line"
    );

    // Saving again REPLACES rather than accumulating.
    db.transaction(|tx| {
        Repos::new(tx).composition().save_group(
            OUTLET,
            &ModifierGroup {
                modifiers: vec![Modifier {
                    id: ModifierId::new("mod_cheese"),
                    name: "Extra cheese".to_owned(),
                    price_delta: Money::from_paise(2_500),
                    sort_order: 0,
                    is_active: true,
                }],
                ..addons.clone()
            },
            at(2),
        )
    })
    .expect("saved again");

    let groups = db
        .transaction(|tx| Repos::new(tx).composition().groups(OUTLET))
        .expect("groups");
    assert_eq!(groups[0].modifiers.len(), 1, "the old choices accumulated");
    assert_eq!(groups[0].modifiers[0].price_delta, Money::from_paise(2_500));
}

/// **T4, through storage.** A mixed-rate combo apportions exactly, and the
/// stored shares are proportions rather than money.
#[test]
fn a_combo_apportions_its_price_across_what_is_in_it() {
    let scratch = Scratch::new("combo");
    let db = scratch.open();
    shop::build(&db);

    // The fixture's dosa is 5% at 120.00 and its water is 18% at 20.00 — the
    // exact case a combo gets wrong.
    let standalone = [
        (ItemId::new("itm_dosa"), Money::from_paise(12_000)),
        (ItemId::new("itm_water"), Money::from_paise(2_000)),
    ];

    db.transaction(|tx| {
        Repos::new(tx).composition().save_combo(
            OUTLET,
            &Combo {
                id: "cmb_lunch".to_owned(),
                name: "Lunch deal".to_owned(),
                // ₹129 across a ₹120 and a ₹20 item: nothing divides.
                unit_price: Money::from_paise(12_900),
                is_active: true,
                components: vec![
                    ComboPart {
                        item_id: ItemId::new("itm_dosa"),
                        qty: Qty::ONE,
                        share_bp: 0,
                    },
                    ComboPart {
                        item_id: ItemId::new("itm_water"),
                        qty: Qty::ONE,
                        share_bp: 0,
                    },
                ],
            },
            &standalone,
            at(1),
        )
    })
    .expect("saved");

    let combos = db
        .transaction(|tx| Repos::new(tx).composition().combos(OUTLET))
        .expect("combos");

    assert_eq!(combos.len(), 1);
    let lunch = &combos[0];
    assert_eq!(lunch.unit_price, Money::from_paise(12_900));
    assert_eq!(lunch.components.len(), 2);

    // The shares are proportions of the combo price, and the dosa — worth six
    // times the water — takes much the larger slice.
    let dosa = lunch
        .components
        .iter()
        .find(|c| c.item_id == ItemId::new("itm_dosa"))
        .expect("the dosa");
    let water = lunch
        .components
        .iter()
        .find(|c| c.item_id == ItemId::new("itm_water"))
        .expect("the water");
    assert!(dosa.share_bp > water.share_bp);
    // 120 of 140 is about 85.7%.
    assert!((8_500..=8_600).contains(&dosa.share_bp), "{}", dosa.share_bp);

    // And the money itself is exact — recomputed from live prices at the
    // moment of sale, which is the half `share_bp` deliberately does not do.
    let parts: Vec<mb_core::ComboComponent> = lunch
        .components
        .iter()
        .map(|part| mb_core::ComboComponent {
            item_id: part.item_id.clone(),
            qty: part.qty,
            standalone: standalone
                .iter()
                .find(|(id, _)| *id == part.item_id)
                .map_or(Money::ZERO, |(_, p)| *p),
        })
        .collect();
    let shares = mb_core::apportion(lunch.unit_price, &parts).expect("apportions");
    let total = Money::try_sum(shares.iter().map(|s| s.share)).expect("sums");
    assert_eq!(total, lunch.unit_price, "a paisa went missing in the combo");
}

/// A combo whose parts have no price of their own is refused rather than split
/// arbitrarily — an invented split is the wrong rate on somebody's return.
#[test]
fn a_combo_of_priceless_things_is_refused() {
    let scratch = Scratch::new("combo_free");
    let db = scratch.open();
    shop::build(&db);

    let refused = db
        .transaction(|tx| {
            Repos::new(tx).composition().save_combo(
                OUTLET,
                &Combo {
                    id: "cmb_nothing".to_owned(),
                    name: "Mystery".to_owned(),
                    unit_price: Money::from_paise(10_000),
                    is_active: true,
                    components: vec![ComboPart {
                        item_id: ItemId::new("itm_dosa"),
                        qty: Qty::ONE,
                        share_bp: 0,
                    }],
                },
                // Nothing standalone: the caller did not say what the dosa is
                // worth on its own.
                &[],
                at(1),
            )
        })
        .expect_err("an arbitrary split was allowed");
    assert!(refused.to_string().contains("price of its own"), "{refused}");
}
