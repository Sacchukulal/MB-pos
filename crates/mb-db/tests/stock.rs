//! **The stock book** — P25, scope 4.2, 4.3, 4.4, 4.6, 4.7 and 4.9.
//!
//! The tests that decide whether this shipped or only compiled. Two of them
//! matter more than the rest:
//!
//! * **T6 — the sale still completes.** Four ways the stock book can be wrong,
//!   and a bill at the end of every one of them. A counter that will not take
//!   money because of a stock record is a broken POS.
//! * **T7 — the derived balance equals the sum of the ledger**, over a
//!   generated month. D114 makes `material_balances` a cache, and a cache
//!   nobody verifies is a stored balance with extra words.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Scratch;
use common::shop::{self, OUTLET, TERMINAL};
use mb_core::recipe::{Recipe, RecipeLine, RecipeOwner};
use mb_core::{
    Registration,
    BillInput, BusinessDay, Cart, Dimension, ItemId, ItemSnapshot, MaterialId, Money, ModifierId,
    OrderId, OrderType, Payment, PaymentMode, PlaceOfSupply, Qty, RoundingMode, Settlement,
    StaffId, TaxRate, TaxSpec, Timestamp, UnitCost, compute_bill,
};
use mb_db::repo::stock::{Material, Movement, MovementKind};
use mb_db::{Db, Repos};

fn at(n: i64) -> Timestamp {
    Timestamp::from_millis(1_770_000_000_000 + n * 1_000)
}

fn day() -> BusinessDay {
    BusinessDay::from_days_since_epoch(20_700)
}

fn material(id: &str) -> MaterialId {
    MaterialId::new(id)
}

fn grams(n: i64) -> Qty {
    Qty::from_whole(n).expect("in range")
}

/// Rice: a weight, bought in 25 kg bags.
fn rice() -> Material {
    let mut m = Material::new(material("mat_rice"), "Rice", Dimension::Weight);
    m.packs = vec![("bag".to_owned(), grams(25_000))];
    m.purchase_unit = Some("bag".to_owned());
    m.buy_from = "Metro".to_owned();
    m
}

fn plain(id: &str, name: &str) -> Material {
    Material::new(material(id), name, Dimension::Weight)
}

fn save(db: &Db, materials: &[Material]) {
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for m in materials {
            repos.stock().save_material(OUTLET, m, at(0))?;
        }
        Ok(())
    })
    .expect("materials saved");
}

fn recipe_for(item: &str, lines: Vec<(&str, i64)>) -> Recipe {
    Recipe::for_one(
        RecipeOwner::Item(ItemId::new(item)),
        lines
            .into_iter()
            .map(|(id, g)| RecipeLine::new(material(id), grams(g), grams(g), "g"))
            .collect(),
    )
}

fn put_recipe(db: &Db, recipe: &Recipe) {
    db.transaction(|tx| Repos::new(tx).stock().save_recipe(OUTLET, recipe, at(1)))
        .expect("recipe saved");
}

fn dosa() -> ItemSnapshot {
    ItemSnapshot {
        item_id: ItemId::new("itm_dosa"),
        name: "Masala Dosa".to_owned(),
        unit_price: Money::from_paise(10_000),
        tax: TaxSpec::gst(TaxRate::from_percent(5).expect("5%")),
        hsn: None,
        category_id: None,
        station: None,
        course: None,
        prep_minutes: None,
    }
}

/// Settle one bill for `qty` of `snapshot`, with optional modifiers.
fn settle_one(
    db: &Db,
    id: &str,
    snapshot: ItemSnapshot,
    qty: i64,
    modifiers: Vec<mb_core::Modifier>,
) -> mb_core::SettledOrder {
    let mut cart = Cart::new();
    cart.add(snapshot, Qty::from_whole(qty).expect("in range"), None, modifiers)
        .expect("adds");

    let bill = compute_bill(
        BillInput::new(&cart, Registration::Regular)
            .with_order_type(OrderType::Parcel)
            .with_place_of_supply(PlaceOfSupply::Intra)
            .with_rounding(RoundingMode::NearestRupee),
    )
    .expect("a bill");

    let mut settlement = Settlement::new();
    settlement
        .add(Payment::new(PaymentMode::Cash, bill.grand_total).expect("a payment"))
        .expect("paid");

    let mut draft = mb_core::DraftOrder::new(
        OrderId::new(id),
        day(),
        at(1),
        OrderType::Parcel,
        StaffId::new("staff_1"),
    );
    draft.core.cart = cart;

    let till = mb_db::Till::new(OUTLET, TERMINAL);
    let open = mb_db::open_draft(db, till, draft).expect("opened");
    mb_db::settle(db, till, open, bill, settlement, at(2), StaffId::new("staff_1"))
        .expect("settled")
}

fn balance(db: &Db, id: &str) -> Qty {
    db.transaction(|tx| Repos::new(tx).stock().balance(OUTLET, &material(id)))
        .expect("balance")
}

fn receive(db: &Db, id: &str, base: Qty, typed: Qty, unit: &str, cost: UnitCost, n: i64) {
    db.transaction(|tx| {
        Repos::new(tx).stock().record(
            OUTLET,
            &Movement::new(
                format!("mv_{id}_{n}"),
                material(id),
                MovementKind::Purchase,
                base,
                at(n),
                day(),
            )
            .typed(typed, unit)
            .costing(cost),
        )
    })
    .expect("received");
}

// ===========================================================================

/// **T1 — the worked example.** Two 25 kg bags in, forty plates at 180 g out,
/// and the remainder read as grams, as kilos and as bags.
///
/// Then the bag is redefined as 26 kg and the purchase row still says what it
/// said (D109). That is the half of this decision nobody notices is missing
/// until a shop corrects a pack size and last month silently changes.
#[test]
fn t1_two_bags_in_forty_plates_out_and_the_row_does_not_move() {
    let scratch = Scratch::new("stock_t1");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[rice()]);
    put_recipe(&db, &recipe_for("itm_dosa", vec![("mat_rice", 180)]));

    // Two bags in, at ₹1,500 a bag.
    let two_bags = grams(50_000);
    let per_bag = UnitCost::from_pack_price(Money::from_paise(150_000), &mb_core::Pack {
        name: "bag".to_owned(),
        base_per_unit: grams(25_000),
        is_standard: false,
    })
    .expect("cost");
    receive(&db, "mat_rice", two_bags, Qty::from_whole(2).expect("ok"), "bag", per_bag, 5);
    assert_eq!(balance(&db, "mat_rice"), grams(50_000));

    // Forty plates out.
    settle_one(&db, "ord_t1", dosa(), 40, vec![]);

    let left = balance(&db, "mat_rice");
    let units = rice().units();
    println!(
        "\n  bought 2 bags (50,000 g) · sold 40 plates × 180 g = 7,200 g\n  \
         left {} g  =  {} kg  =  {} bags   (\"{}\")",
        left,
        units.from_base(left, "kg").expect("kg"),
        units.from_base(left, "bag").expect("bag"),
        units.say(left),
    );
    assert_eq!(left, grams(42_800), "42,800 g");
    assert_eq!(units.from_base(left, "kg").expect("kg"), Qty::from_thousandths(42_800));
    assert_eq!(units.from_base(left, "bag").expect("bag"), Qty::from_thousandths(1_712));

    // **D109.** The shop corrects the bag to 26 kg. The purchase row must not
    // move: it recorded 50,000 g and it still says "2 bag".
    let mut bigger = rice();
    bigger.packs = vec![("bag".to_owned(), grams(26_000))];
    save(&db, &[bigger]);

    let rows = db
        .transaction(|tx| {
            Repos::new(tx).stock().movements(OUTLET, Some(&material("mat_rice")), None, None, 50)
        })
        .expect("movements");
    let purchase = rows
        .iter()
        .find(|r| r.kind == MovementKind::Purchase)
        .expect("the purchase is still there");
    assert_eq!(purchase.base_qty, grams(50_000), "the truth did not move");
    assert_eq!(purchase.typed_qty, Qty::from_whole(2).expect("ok"));
    assert_eq!(purchase.typed_unit, "bag", "the label did not move either");
    assert_eq!(balance(&db, "mat_rice"), grams(42_800), "and neither did the balance");
}

/// **T2 — a dish made of a gravy made of materials**, deducting both levels
/// with the batch yield pro-rated. And T11's first half: it recurses.
#[test]
fn t2_multi_level_recipe_deducts_through_every_level() {
    let scratch = Scratch::new("stock_t2");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[plain("mat_gravy", "Gravy base"), plain("mat_tomato", "Tomato"), plain("mat_onion", "Onion")]);

    // One batch of gravy makes 4 kg from 3 kg tomato and 1 kg onion.
    put_recipe(
        &db,
        &Recipe::batch(
            material("mat_gravy"),
            grams(4_000),
            vec![
                RecipeLine::new(material("mat_tomato"), grams(3_000), grams(3), "kg"),
                RecipeLine::new(material("mat_onion"), grams(1_000), grams(1), "kg"),
            ],
        ),
    );
    put_recipe(&db, &recipe_for("itm_dosa", vec![("mat_gravy", 150)]));
    receive(&db, "mat_tomato", grams(20_000), grams(20), "kg", UnitCost::from_paise_per_thousand(4_000), 5);
    receive(&db, "mat_onion", grams(10_000), grams(10), "kg", UnitCost::from_paise_per_thousand(3_000), 6);

    settle_one(&db, "ord_t2", dosa(), 10, vec![]);

    // Ten dosas need 1,500 g of gravy = 0.375 of a batch.
    assert_eq!(balance(&db, "mat_tomato"), grams(20_000 - 1_125));
    assert_eq!(balance(&db, "mat_onion"), grams(10_000 - 375));
    // The gravy was made and immediately used, so it nets to nothing — and
    // BOTH rows are in the ledger, because a production that only showed its
    // net effect would be a production nobody can audit.
    assert_eq!(balance(&db, "mat_gravy"), Qty::ZERO);

    let rows = db
        .transaction(|tx| Repos::new(tx).stock().movements(OUTLET, None, None, None, 100))
        .expect("movements");
    assert!(
        rows.iter().any(|r| r.kind == MovementKind::ProductionIn && r.was_automatic),
        "the automatic production is not in the ledger"
    );
    assert!(
        rows.iter()
            .any(|r| r.kind == MovementKind::ProductionOut && r.material == material("mat_tomato")),
        "the tomato was drawn as a sale rather than as an input to a production"
    );
    // **The made material's cost came from its inputs.** 3 kg at ₹40 plus 1 kg
    // at ₹30 is ₹150 for 4 kg, which is 3.75 paise a gram.
    let made = rows.iter().find(|r| r.kind == MovementKind::ProductionIn).expect("made");
    assert_eq!(made.unit_cost, UnitCost::from_paise_per_thousand(3_750));

    // **An input row names what it fed.** Without it the ledger says a tomato
    // moved and leaves the reader to work out why — which is the whole reason
    // `produced_for` is a column.
    let input = rows
        .iter()
        .find(|r| r.kind == MovementKind::ProductionOut && r.material == material("mat_tomato"))
        .expect("the tomato");
    assert_eq!(input.produced_for.as_deref(), Some("Gravy base"));

    // **The FIRST sale of a made material is valued**, which it was not until
    // the rows were written in the order the kitchen did them. `record` values
    // anything leaving the shelf at the material's current average cost, so a
    // gravy drawn before its batch existed came out at ₹0.00 — on the first
    // curry of a shop's life, silently, with every test green. Found by
    // settling thirty bills and reading the ledger back.
    let sold_gravy = rows
        .iter()
        .filter(|r| r.kind == MovementKind::Sale && r.material == material("mat_gravy"))
        .collect::<Vec<_>>();
    assert!(!sold_gravy.is_empty());
    for row in sold_gravy {
        assert_eq!(
            row.unit_cost,
            UnitCost::from_paise_per_thousand(3_750),
            "a gravy sale was valued at nothing"
        );
    }

    // **And the whole thing is exact, not pro-rated through a rounded batch
    // fraction.** Ten curries need 1,500 g of a 4 kg batch, which is 0.375 of
    // one — three decimal places is enough here, but 150 g of the same batch is
    // 0.0375, which is not. Found by seeding a demo shop and reading the
    // numbers; see `RecipeLine::issued_scaled`.
    let one = Scratch::new("stock_t2_exact");
    let db2 = one.open();
    shop::build(&db2);
    save(&db2, &[plain("mat_gravy", "Gravy base"), plain("mat_tomato", "Tomato")]);
    db2.transaction(|tx| {
        let stock = Repos::new(tx).stock();
        stock.save_recipe(
            OUTLET,
            &Recipe::batch(
                material("mat_gravy"),
                grams(4_000),
                vec![RecipeLine::new(material("mat_tomato"), grams(3_000), grams(3), "kg")],
            ),
            at(1),
        )?;
        stock.save_recipe(OUTLET, &recipe_for("itm_dosa", vec![("mat_gravy", 150)]), at(1))
    })
    .expect("recipes");
    receive(&db2, "mat_tomato", grams(20_000), grams(20), "kg", UnitCost::ZERO, 5);
    settle_one(&db2, "ord_exact", dosa(), 1, vec![]);
    // 150 g of gravy is 0.0375 of a batch, so 112.5 g of tomato — not the
    // 114 g a batch fraction rounded to thousandths would have taken.
    assert_eq!(
        balance(&db2, "mat_tomato"),
        Qty::from_thousandths(20_000_000 - 112_500),
        "the batch fraction was rounded before it was used"
    );
}

/// **T3 — a recipe that eats itself is refused at save, by name.**
#[test]
fn t3_a_loop_is_refused_and_the_message_names_it() {
    let scratch = Scratch::new("stock_t3");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[plain("mat_a", "Paste"), plain("mat_b", "Masala"), plain("mat_c", "Powder")]);

    put_recipe(
        &db,
        &Recipe::batch(
            material("mat_a"),
            grams(1_000),
            vec![RecipeLine::new(material("mat_b"), grams(100), grams(100), "g")],
        ),
    );
    put_recipe(
        &db,
        &Recipe::batch(
            material("mat_b"),
            grams(1_000),
            vec![RecipeLine::new(material("mat_c"), grams(100), grams(100), "g")],
        ),
    );

    // c → a → b → c: a chain of three.
    let looped = Recipe::batch(
        material("mat_c"),
        grams(1_000),
        vec![RecipeLine::new(material("mat_a"), grams(100), grams(100), "g")],
    );
    let refused = db
        .transaction(|tx| Repos::new(tx).stock().save_recipe(OUTLET, &looped, at(3)))
        .expect_err("a loop must be refused");
    let said = refused.to_string();
    println!("\n  {said}");
    assert!(said.contains("Powder"), "the message does not name the loop: {said}");
    assert!(said.contains("Paste"), "the message does not name the loop: {said}");
    assert!(said.contains("Masala"), "the message does not name the loop: {said}");

    // And re-saving an existing recipe unchanged is NOT a loop with itself.
    put_recipe(
        &db,
        &Recipe::batch(
            material("mat_a"),
            grams(1_000),
            vec![RecipeLine::new(material("mat_b"), grams(200), grams(200), "g")],
        ),
    );
}

/// **T4 — settling twice deducts once.** D82, through `applied_events`.
#[test]
fn t4_deduction_is_idempotent() {
    let scratch = Scratch::new("stock_t4");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[rice()]);
    put_recipe(&db, &recipe_for("itm_dosa", vec![("mat_rice", 180)]));
    receive(&db, "mat_rice", grams(10_000), grams(10), "kg", UnitCost::ZERO, 5);

    let settled = settle_one(&db, "ord_t4", dosa(), 5, vec![]);
    assert_eq!(balance(&db, "mat_rice"), grams(10_000 - 900));

    // Replay the deduction, as a retried settle would.
    for _ in 0..3 {
        db.transaction(|tx| Repos::new(tx).stock().deduct_for_bill(OUTLET, &settled, at(9)))
            .expect("replayed");
    }
    assert_eq!(balance(&db, "mat_rice"), grams(10_000 - 900), "stock moved twice");
}

/// **T5 — a void reverses to the base unit, by negating the ROWS.**
///
/// Proved by changing the recipe between the settle and the void: re-exploding
/// would put back the NEW amount, and the shop's rice balance would gain the
/// difference for ever (D113).
#[test]
fn t5_a_void_puts_back_what_was_actually_taken() {
    let scratch = Scratch::new("stock_t5");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[rice()]);
    put_recipe(&db, &recipe_for("itm_dosa", vec![("mat_rice", 180)]));
    receive(&db, "mat_rice", grams(10_000), grams(10), "kg", UnitCost::ZERO, 5);

    let settled = settle_one(&db, "ord_t5", dosa(), 10, vec![]);
    assert_eq!(balance(&db, "mat_rice"), grams(10_000 - 1_800));

    // The chef changes the recipe on Wednesday.
    put_recipe(&db, &recipe_for("itm_dosa", vec![("mat_rice", 250)]));

    // The bill is voided on Friday.
    db.transaction(|tx| {
        Repos::new(tx).stock().reverse_for_bill(
            OUTLET,
            &settled.core.id,
            at(20),
            day(),
            Some(&StaffId::new("staff_1")),
        )
    })
    .expect("reversed");

    assert_eq!(
        balance(&db, "mat_rice"),
        grams(10_000),
        "the reversal used the recipe as it is NOW, not as it was — D113 is broken"
    );

    // Reversing twice must not put it back twice.
    db.transaction(|tx| {
        Repos::new(tx).stock().reverse_for_bill(OUTLET, &settled.core.id, at(21), day(), None)
    })
    .expect("reversed again");
    assert_eq!(balance(&db, "mat_rice"), grams(10_000), "a second void doubled the stock");
}

/// **T6 — THE SALE STILL COMPLETES.** Four ways, four bills.
#[test]
fn t6_nothing_in_the_stock_book_can_refuse_a_bill() {
    let scratch = Scratch::new("stock_t6");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[rice(), plain("mat_ghee", "Ghee")]);

    // 1. A recipe still reaching for a material the shop has RETIRED.
    //
    //    Not a fabricated id: `recipe_lines.material_id` is a real foreign key
    //    and a material is never deleted (D47), so the only way this happens in
    //    a real shop is by switching one off underneath a live recipe. The food
    //    still comes off the shelf, because it was used.
    put_recipe(&db, &recipe_for("itm_dosa", vec![("mat_ghee", 50)]));
    let mut retired = plain("mat_ghee", "Ghee");
    retired.is_active = false;
    save(&db, &[retired]);
    let one = settle_one(&db, "ord_t6_1", dosa(), 1, vec![]);
    assert!(one.bill.grand_total.is_positive(), "the bill did not settle");
    assert_eq!(balance(&db, "mat_ghee"), grams(-50), "a retired material still moved");

    // 2. A shelf that goes below zero. Allowed, recorded, and the draw still
    //    happens — the book has to show what was really used.
    put_recipe(&db, &recipe_for("itm_dosa", vec![("mat_rice", 1_000)]));
    let two = settle_one(&db, "ord_t6_2", dosa(), 3, vec![]);
    assert!(two.bill.grand_total.is_positive(), "the bill did not settle");
    assert_eq!(balance(&db, "mat_rice"), grams(-3_000), "a negative balance is information");

    // 3. An item with no recipe at all — the coverage case.
    let water = ItemSnapshot::new(
        ItemId::new("itm_water"),
        "Water",
        Money::from_paise(2_000),
        TaxRate::from_percent(18).expect("18%"),
    );
    let three = settle_one(&db, "ord_t6_3", water, 2, vec![]);
    assert!(three.bill.grand_total.is_positive(), "the bill did not settle");

    // 4. An ad-hoc line with no item id at all.
    let adhoc = ItemSnapshot::new(ItemId::new(""), "Special", Money::from_paise(5_000), TaxRate::ZERO);
    let four = settle_one(&db, "ord_t6_4", adhoc, 1, vec![]);
    assert!(four.bill.grand_total.is_positive(), "the bill did not settle");

    let problems = db
        .transaction(|tx| Repos::new(tx).stock().problems(OUTLET))
        .expect("problems");
    for p in &problems {
        println!("\n  [{}] ×{}  {}", p.kind, p.occurrences, p.sentence);
    }
    let kinds: Vec<&str> = problems.iter().map(|p| p.kind.as_str()).collect();
    assert!(kinds.contains(&"retired_material"), "{kinds:?}");
    assert!(kinds.contains(&"went_negative"), "{kinds:?}");
    assert!(kinds.contains(&"no_recipe"), "{kinds:?}");
    // The ad-hoc line raised NOTHING. It is not a missing recipe; it is a thing
    // that was never on the menu, and saying so every time would be noise an
    // owner learns to ignore.
    assert!(
        !problems.iter().any(|p| p.subject.contains("Special")),
        "an ad-hoc line was reported as a missing recipe"
    );
    // Every one carries a sentence, not a code.
    assert!(problems.iter().all(|p| p.sentence.len() > 30 && p.sentence.ends_with('.')));
}

/// **T7 — the derived balance equals the sum of the ledger**, over a generated
/// month of purchases, sales, wastage and adjustments (D114).
#[test]
fn t7_the_cache_always_agrees_with_the_ledger() {
    let scratch = Scratch::new("stock_t7");
    let db = scratch.open();
    shop::build(&db);
    let materials: Vec<Material> = (0..6)
        .map(|n| plain(&format!("mat_{n}"), &format!("Material {n}")))
        .collect();
    save(&db, &materials);

    // A month. The quantities are arbitrary but the mixture is not: every kind
    // that can move stock outside a bill is in here.
    let kinds = [
        MovementKind::Purchase,
        MovementKind::Wastage,
        MovementKind::Adjustment,
        MovementKind::Opening,
    ];
    let mut n = 0_i64;
    for d in 0..30 {
        for (m, material_row) in materials.iter().enumerate() {
            for (k, kind) in kinds.iter().enumerate() {
                n += 1;
                let sign = if matches!(kind, MovementKind::Wastage) { -1 } else { 1 };
                let qty = Qty::from_thousandths(sign * (1_000 + (n * 37) % 5_000));
                db.transaction(|tx| {
                    Repos::new(tx).stock().record(
                        OUTLET,
                        &Movement::new(
                            format!("mv_{d}_{m}_{k}"),
                            material_row.id.clone(),
                            *kind,
                            qty,
                            at(n),
                            BusinessDay::from_days_since_epoch(20_700 + d),
                        )
                        .costing(UnitCost::from_paise_per_thousand(1_000 + n)),
                    )
                })
                .expect("recorded");
            }
        }
    }

    let cached = db.transaction(|tx| Repos::new(tx).stock().balances(OUTLET)).expect("cached");
    let corrected = db
        .transaction(|tx| Repos::new(tx).stock().rebuild_balances(OUTLET, at(9_999)))
        .expect("rebuilt");
    let rebuilt = db.transaction(|tx| Repos::new(tx).stock().balances(OUTLET)).expect("rebuilt");

    println!("\n  {n} movements over 30 days · {} materials · {corrected} corrections", materials.len());
    assert_eq!(corrected, 0, "the cache had drifted from the ledger");
    for (id, (qty, _)) in &rebuilt {
        assert_eq!(cached.get(id).map(|(q, _)| *q), Some(*qty), "{id} disagrees");
    }
    assert_eq!(cached.len(), materials.len());
}

/// **T8 — a variant recipe and a modifier recipe each deduct their own.**
///
/// The variant needs no code at all: P13 made a variant its own item id, so
/// "Dosa (Half)" is an [`RecipeOwner::Item`] like any other.
#[test]
fn t8_variants_and_modifiers_deduct_their_own_amounts() {
    let scratch = Scratch::new("stock_t8");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[plain("mat_batter", "Batter"), plain("mat_cheese", "Cheese")]);

    // The half plate is its own item, with its own smaller recipe.
    db.transaction(|tx| {
        tx.execute_batch(
            "INSERT INTO item_variants (id, item_id, name, unit_price, sort_order, is_active)
             VALUES ('itm_dosa_half', 'itm_dosa', 'Half', 7000, 0, 1);
             INSERT INTO items (id, outlet_id, category_id, name, unit_price, tax_rate_bp,
                                tax_kind, tax_basis, created_at, updated_at)
             VALUES ('itm_dosa_half', 'outlet_default', 'cat_food', 'Masala Dosa (Half)',
                     7000, 500, 'gst', 'exclusive', 0, 0);
             INSERT INTO modifier_groups (id, outlet_id, name, min_select, sort_order)
             VALUES ('grp_extras', 'outlet_default', 'Extras', 0, 0);
             INSERT INTO modifiers (id, group_id, name, price_delta, sort_order, is_active)
             VALUES ('mod_cheese', 'grp_extras', 'Extra cheese', 2000, 0, 1);",
        )
        .map_err(Into::into)
    })
    .expect("seeded");

    put_recipe(&db, &recipe_for("itm_dosa", vec![("mat_batter", 200)]));
    put_recipe(&db, &recipe_for("itm_dosa_half", vec![("mat_batter", 120)]));
    put_recipe(
        &db,
        &Recipe::for_one(
            RecipeOwner::Modifier(ModifierId::new("mod_cheese")),
            vec![RecipeLine::new(material("mat_cheese"), grams(30), grams(30), "g")],
        ),
    );
    receive(&db, "mat_batter", grams(10_000), grams(10), "kg", UnitCost::ZERO, 5);
    receive(&db, "mat_cheese", grams(5_000), grams(5), "kg", UnitCost::ZERO, 6);

    let mut half = dosa();
    half.item_id = ItemId::new("itm_dosa_half");
    half.name = "Masala Dosa (Half)".to_owned();
    half.unit_price = Money::from_paise(7_000);

    settle_one(
        &db,
        "ord_t8",
        half,
        2,
        vec![mb_core::Modifier::new(
            ModifierId::new("mod_cheese"),
            "Extra cheese",
            Money::from_paise(2_000),
        )],
    );

    assert_eq!(balance(&db, "mat_batter"), grams(10_000 - 240), "the half plate used the full recipe");
    assert_eq!(balance(&db, "mat_cheese"), grams(5_000 - 60), "the modifier did not deduct");
}

/// **T9 — theoretical against actual**, and the "never counted" that stops it
/// being a confident lie (D115).
#[test]
fn t9_the_variance_report_finds_a_seeded_wastage_and_admits_what_it_does_not_know() {
    let scratch = Scratch::new("stock_t9");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[rice()]);
    put_recipe(&db, &recipe_for("itm_dosa", vec![("mat_rice", 100)]));
    receive(&db, "mat_rice", grams(100_000), grams(100), "kg", UnitCost::from_paise_per_thousand(6_000), 5);

    // 100 plates × 100 g = 10,000 g theoretical.
    settle_one(&db, "ord_t9", dosa(), 100, vec![]);
    // Plus a seeded 3% wastage — 300 g.
    db.transaction(|tx| {
        Repos::new(tx).stock().record(
            OUTLET,
            &Movement::new(
                "mv_waste",
                material("mat_rice"),
                MovementKind::Wastage,
                grams(-300),
                at(8),
                day(),
            )
            .typed(grams(-300), "g"),
        )
    })
    .expect("wasted");

    let rows = db
        .transaction(|tx| Repos::new(tx).stock().consumption(OUTLET, day(), day()))
        .expect("consumption");
    let rice_row = rows.iter().find(|r| r.material == material("mat_rice")).expect("rice");
    println!(
        "\n  theoretical {} g · actual {} g · variance {} g ({} bp) · worth {} · last counted {:?}",
        rice_row.theoretical,
        rice_row.actual,
        rice_row.variance,
        rice_row.variance_bp().unwrap_or(0),
        rice_row.variance_value.to_plain_string(),
        rice_row.last_counted_at,
    );
    assert_eq!(rice_row.theoretical, grams(10_000));
    assert_eq!(rice_row.actual, grams(10_300));
    assert_eq!(rice_row.variance, grams(300));
    assert_eq!(rice_row.variance_bp(), Some(300), "3.00%");
    assert_eq!(rice_row.variance_value, Money::from_paise(1_800), "300 g at ₹60/kg");

    // **D115.** Nobody has counted this, and the row says so rather than
    // implying the figure has been checked against a shelf.
    assert_eq!(rice_row.last_counted_at, None, "a variance nobody counted must say so");
}

/// **D118 — the average cost is what actually came in.**
#[test]
fn the_cost_of_a_material_is_a_weighted_average_and_not_a_typed_price() {
    let scratch = Scratch::new("stock_cost");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[rice()]);

    receive(&db, "mat_rice", grams(10_000), grams(10), "kg", UnitCost::from_paise_per_thousand(6_000), 5);
    receive(&db, "mat_rice", grams(10_000), grams(10), "kg", UnitCost::from_paise_per_thousand(8_000), 6);

    let found = db
        .transaction(|tx| Repos::new(tx).stock().material(OUTLET, &material("mat_rice")))
        .expect("read")
        .expect("there");
    assert_eq!(found.avg_cost, UnitCost::from_paise_per_thousand(7_000), "₹70 a kilo");
    assert!(found.cost_changed_at.is_some(), "the screen has to be able to say when");

    // And saving the material back does NOT let a screen type over it.
    let mut edited = found.clone();
    edited.avg_cost = UnitCost::from_paise_per_thousand(1);
    save(&db, &[edited]);
    let after = db
        .transaction(|tx| Repos::new(tx).stock().material(OUTLET, &material("mat_rice")))
        .expect("read")
        .expect("there");
    assert_eq!(
        after.avg_cost,
        UnitCost::from_paise_per_thousand(7_000),
        "a screen typed over the cost the ledger decided"
    );
}

/// Scope 4.6 — the buy list, in the pack the shop buys in.
#[test]
fn the_buy_list_counts_in_bags_and_groups_by_where_you_buy_it() {
    let scratch = Scratch::new("stock_low");
    let db = scratch.open();
    shop::build(&db);
    let mut r = rice();
    r.reorder_level = grams(30_000);
    r.reorder_qty = grams(50_000);
    let mut milk = plain("mat_milk", "Milk");
    milk.buy_from = "The milk van".to_owned();
    milk.reorder_level = grams(5_000);
    save(&db, &[r, milk]);

    receive(&db, "mat_rice", grams(12_000), grams(12), "kg", UnitCost::ZERO, 5);
    receive(&db, "mat_milk", grams(20_000), grams(20), "kg", UnitCost::ZERO, 6);

    let on_hand = db
        .transaction(|tx| Repos::new(tx).stock().on_hand(OUTLET, false))
        .expect("on hand");
    let low: Vec<_> = on_hand.iter().filter(|h| h.is_low()).collect();
    assert_eq!(low.len(), 1, "the milk is not low and should not be on the list");
    let rice_row = low[0];
    assert_eq!(rice_row.material.buy_from, "Metro");
    // The shortfall a person can carry: 0.48 of a bag on the shelf, buy 2 bags.
    let units = rice_row.material.units();
    let shortfall = units
        .from_base(rice_row.material.reorder_qty, &rice_row.material.default_purchase_unit())
        .expect("in bags");
    println!("\n  {} — have {} · buy {} bag(s) from {}",
        rice_row.material.name, units.say(rice_row.base_qty), shortfall, rice_row.material.buy_from);
    assert_eq!(shortfall, Qty::from_whole(2).expect("ok"));
}

/// Scope 4.7 — wastage is valued, because a wastage figure with no rupees on it
/// is one nobody reads.
#[test]
fn wastage_is_valued_at_what_the_material_costs() {
    let scratch = Scratch::new("stock_waste");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[rice()]);
    receive(&db, "mat_rice", grams(10_000), grams(10), "kg", UnitCost::from_paise_per_thousand(6_000), 5);

    db.transaction(|tx| {
        let mut m = Movement::new(
            "mv_burnt",
            material("mat_rice"),
            MovementKind::Wastage,
            grams(-500),
            at(8),
            day(),
        )
        .typed(Qty::from_thousandths(-500), "kg")
        .by(StaffId::new("staff_1"));
        m.reason_id = Some("rsn_wst_burnt".to_owned());
        Repos::new(tx).stock().record(OUTLET, &m)
    })
    .expect("wasted");

    let rows = db
        .transaction(|tx| Repos::new(tx).stock().movements(OUTLET, None, None, None, 10))
        .expect("movements");
    let waste = rows.iter().find(|r| r.kind == MovementKind::Wastage).expect("there");
    // The caller passed NO cost, and the row is still valued — resolved inside
    // `record`, so no call site can produce a theft report reading ₹0.
    assert_eq!(waste.total_cost, Money::from_paise(-3_000), "500 g at ₹60/kg");
    assert_eq!(waste.reason.as_deref(), Some("Burnt or overcooked"));
    assert_eq!(waste.staff, Some(StaffId::new("staff_1")));
}

/// A shop that has never opened this module deducts nothing and costs nothing.
#[test]
fn a_shop_with_no_recipes_leaves_no_trace() {
    let scratch = Scratch::new("stock_none");
    let db = scratch.open();
    shop::build(&db);

    settle_one(&db, "ord_none", dosa(), 3, vec![]);

    let rows = db
        .transaction(|tx| Repos::new(tx).stock().movements(OUTLET, None, None, None, 10))
        .expect("movements");
    assert!(rows.is_empty(), "a shop with no recipes wrote a stock row");
    let problems = db
        .transaction(|tx| Repos::new(tx).stock().problems(OUTLET))
        .expect("problems");
    assert!(problems.is_empty(), "a shop with no recipes was told it has a problem");
    // And no idempotency row either — the gate is before everything.
    let claimed: i64 = db
        .transaction(|tx| {
            tx.query_row(
                "SELECT COUNT(*) FROM applied_events WHERE source = 'stock'",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
        })
        .expect("counted");
    assert_eq!(claimed, 0);
}

/// The day close writes a closing figure per material (P18's hook).
#[test]
fn the_day_close_freezes_a_closing_figure_per_material() {
    let scratch = Scratch::new("stock_close");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[rice(), plain("mat_oil", "Oil")]);
    receive(&db, "mat_rice", grams(10_000), grams(10), "kg", UnitCost::from_paise_per_thousand(6_000), 5);
    receive(&db, "mat_oil", grams(5_000), grams(5), "kg", UnitCost::from_paise_per_thousand(12_000), 6);

    let written = db
        .transaction(|tx| Repos::new(tx).stock().close_day(OUTLET, day()))
        .expect("closed");
    assert_eq!(written, 2);

    // Closing twice overwrites rather than duplicating, which is what D77's
    // reopen-and-close-again door requires.
    receive(&db, "mat_rice", grams(1_000), grams(1), "kg", UnitCost::from_paise_per_thousand(6_000), 7);
    db.transaction(|tx| Repos::new(tx).stock().close_day(OUTLET, day())).expect("closed again");

    let (qty, cost): (i64, i64) = db
        .transaction(|tx| {
            tx.query_row(
                "SELECT closing_qty, unit_cost FROM stock_day_closes
                  WHERE material_id = 'mat_rice' AND business_day = ?1",
                [day().days_since_epoch()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(Into::into)
        })
        .expect("read back");
    assert_eq!(qty, 11_000_000, "11 kg");
    assert_eq!(cost, 6_000);
}

/// Where a material is used — asked before retiring one.
#[test]
fn a_material_knows_which_recipes_use_it() {
    let scratch = Scratch::new("stock_used");
    let db = scratch.open();
    shop::build(&db);
    save(&db, &[plain("mat_rice", "Rice")]);
    put_recipe(&db, &recipe_for("itm_dosa", vec![("mat_rice", 100)]));
    put_recipe(&db, &recipe_for("itm_water", vec![("mat_rice", 5)]));

    let used = db
        .transaction(|tx| Repos::new(tx).stock().where_used(OUTLET, &material("mat_rice")))
        .expect("used");
    assert_eq!(used.len(), 2, "{used:?}");
}

/// **T10 — budget I1.** What the deduction adds to a settle.
///
/// It lives here rather than in `tests/perf.rs` because it has to run the REAL
/// settle path — a cart, a computed bill, `mb_db::settle` — and those helpers
/// are in this file. `perf.rs` writes rows with SQL, which would measure a
/// different thing and flatter this one.
///
/// §3.1's rules, obeyed: every run prints the number; the assertion is against
/// the ceiling and only in release.
#[test]
#[allow(clippy::float_arithmetic, reason = "a stopwatch, not the money path")]
fn t10_budget_i1_what_deduction_costs_a_settle() {
    use std::time::Instant;
    const BILLS: usize = 250;

    // A shop with recipes: five materials on the dish, one of them a made
    // material with two of its own, which is deeper than most shops go.
    let with = Scratch::new("stock_i1_with");
    let db = with.open();
    shop::build(&db);
    let materials: Vec<Material> = ["mat_rice", "mat_oil", "mat_salt", "mat_masala", "mat_ghee"]
        .iter()
        .map(|id| plain(id, id))
        .chain(std::iter::once(plain("mat_gravy", "Gravy base")))
        .collect();
    save(&db, &materials);
    put_recipe(
        &db,
        &Recipe::batch(
            material("mat_gravy"),
            grams(4_000),
            vec![
                RecipeLine::new(material("mat_masala"), grams(300), grams(300), "g"),
                RecipeLine::new(material("mat_ghee"), grams(200), grams(200), "g"),
            ],
        ),
    );
    put_recipe(
        &db,
        &recipe_for(
            "itm_dosa",
            vec![("mat_rice", 180), ("mat_oil", 20), ("mat_salt", 5), ("mat_gravy", 60)],
        ),
    );
    for m in &materials {
        receive(&db, m.id.as_str(), grams(500_000), grams(500), "kg", UnitCost::from_paise_per_thousand(6_000), 1);
    }

    let mut samples = Vec::with_capacity(BILLS);
    for n in 0..BILLS {
        let started = Instant::now();
        settle_one(&db, &format!("ord_i1_{n}"), dosa(), 2, vec![]);
        samples.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    samples.sort_by(f64::total_cmp);

    // The same shop with NO recipes, so the difference is the deduction and not
    // the settle.
    let without = Scratch::new("stock_i1_without");
    let bare = without.open();
    shop::build(&bare);
    let mut bare_samples = Vec::with_capacity(BILLS);
    for n in 0..BILLS {
        let started = Instant::now();
        settle_one(&bare, &format!("ord_i1b_{n}"), dosa(), 2, vec![]);
        bare_samples.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    bare_samples.sort_by(f64::total_cmp);

    let p95 = samples[samples.len() * 95 / 100];
    let median = samples[samples.len() / 2];
    let bare_p95 = bare_samples[bare_samples.len() * 95 / 100];
    let bare_median = bare_samples[bare_samples.len() / 2];

    println!("\n--- I1: what the stock deduction adds to a settle ---");
    println!("  bills                   {BILLS}, 4 recipe lines + a made material");
    println!("  with recipes            median {median:.2} ms · p95 {p95:.2} ms");
    println!("  no recipes at all       median {bare_median:.2} ms · p95 {bare_p95:.2} ms");
    println!("  the deduction costs     median {:.2} ms · p95 {:.2} ms", median - bare_median, p95 - bare_p95);
    println!("  budget / ceiling        10 ms / 25 ms, INSIDE B5's 150 ms");
    println!(
        "  NOTE: an SSD. On the reference machine the fsync dominates and the\n\
         \x20      difference should be smaller still, because it is the SAME commit.\n"
    );

    // Everything moved, and it moved exactly once per bill.
    assert_eq!(balance(&db, "mat_rice"), grams(500_000 - 180 * 2 * 250));

    if cfg!(debug_assertions) {
        return;
    }
    let added = p95 - bare_p95;
    assert!(added < 25.0, "the deduction added {added:.1} ms to the p95 settle, over I1's 25 ms ceiling");
}

/// An empty recipe is refused in words, because a recipe with nothing in it
/// would take nothing off the shelf and look like it was working.
#[test]
fn an_empty_recipe_is_refused_in_words() {
    let scratch = Scratch::new("stock_empty");
    let db = scratch.open();
    shop::build(&db);

    let empty = Recipe::for_one(RecipeOwner::Item(ItemId::new("itm_dosa")), vec![]);
    let refused = db
        .transaction(|tx| Repos::new(tx).stock().save_recipe(OUTLET, &empty, at(1)))
        .expect_err("refused");
    assert!(refused.to_string().contains("Add at least one material"), "{refused}");
}
