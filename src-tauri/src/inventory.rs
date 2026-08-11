//! **The stock book, as a screen sees it** — P25, scope 4.2 to 4.9.
//!
//! Bodies over `&App` (D46). Nothing here computes: every figure, every unit
//! conversion and every sentence is made in Rust and crosses as a string (R8,
//! D39). A screen that divided a quantity by a pack size would be a second
//! answer to D108, and the two would disagree the first time a shop corrected a
//! bag.
//!
//! # What is gated and what is not
//!
//! `Feature::Inventory` guards the SCREENS. It is never consulted on the settle
//! path — a shop whose licence lapsed on Tuesday must not have Wednesday's
//! stock quietly stop moving, because the day they pay again the book would be
//! a week wrong with nothing saying so. Deducting costs nothing; looking is
//! what is sold.

use std::collections::BTreeMap;

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::recipe::{Recipe, RecipeLine, RecipeOwner};
use mb_core::{
    Dimension, ItemId, MaterialId, Money, ModifierId, Qty, Timestamp, UnitCost, Units,
};
use mb_db::repo::stock::{Material, Movement, MovementKind};
use mb_license::Feature;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::log_info;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// ---------------------------------------------------------------------------
// What the screen sees.
// ---------------------------------------------------------------------------

/// One unit a material can be spoken about in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct UnitView {
    pub name: String,
    /// Thousandths of the base unit, as a string the screen never does
    /// arithmetic on — it round-trips back when the shop saves.
    pub base_per_unit: String,
    /// A kilo is not the shop's to redefine.
    pub is_standard: bool,
}

/// A material, and what is on the shelf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MaterialView {
    pub id: String,
    pub name: String,
    /// `weight`, `volume` or `count`.
    pub dimension: String,
    /// "Weight".
    pub dimension_label: String,
    /// "g", "ml", "piece".
    pub base_unit: String,
    pub category: String,
    /// D116 — where you buy it. The buy list groups by this.
    pub buy_from: String,
    /// **"1.712 bag"** — the balance in the biggest unit one of it fits into,
    /// which is what a person would say (D108).
    pub on_hand: String,
    /// The same figure in base units, for a screen that wants to sort.
    pub on_hand_base: String,
    pub is_negative: bool,
    pub value: MoneyView,
    /// "₹60.00 per kg", or empty when nothing has ever been priced.
    pub cost: String,
    /// "changed 3 days ago", or "never priced" — half of whether to believe it.
    pub cost_when: String,
    /// D115 — **"never counted"** when nobody has.
    pub last_counted: String,
    pub is_low: bool,
    /// "Buy 2 bag" — the shortfall in the pack the shop buys in.
    pub buy: String,
    pub reorder_level: String,
    pub reorder_qty: String,
    pub is_perishable: bool,
    pub shelf_life_days: Option<u32>,
    /// "has not moved in 6 days" — D117's warning, empty when there is none.
    pub warning: String,
    pub is_active: bool,
    /// True when this is a made material: it has a recipe of its own (D111).
    pub is_made: bool,
    pub units: Vec<UnitView>,
    pub purchase_unit: String,
    pub recipe_unit: String,
    /// How many recipes use it, so a shop knows before retiring one.
    pub used_by: u32,
}

/// One line of a recipe, with its live cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RecipeLineView {
    pub material_id: String,
    pub material: String,
    /// **D109** — what the person typed, shown back exactly as typed.
    pub qty: String,
    pub unit: String,
    pub units: Vec<UnitView>,
    /// D110 — how much of it survives. 100 is the everyday value.
    pub yield_percent: u32,
    /// "222 g" — what actually leaves the shelf, once the yield is applied.
    pub issued: String,
    pub cost: MoneyView,
    /// True when this material has never been priced, so the cost is a lie by
    /// omission and the screen says so on the line.
    pub is_unpriced: bool,
}

/// A whole recipe, with the cost that updates as you type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RecipeView {
    /// `item`, `modifier` or `material`.
    pub owner_kind: String,
    pub owner_id: String,
    pub owner: String,
    /// Empty for a dish; "this batch makes 4 kg" for a made material.
    pub batch: String,
    pub batch_qty: String,
    pub batch_unit: String,
    pub lines: Vec<RecipeLineView>,
    pub cost: MoneyView,
    /// **What P13's typed cost price says**, beside what the recipe says. D119
    /// — the gap between the two is itself the finding, so both are shown.
    pub typed_cost: Option<MoneyView>,
    pub sells_for: Option<MoneyView>,
    /// "62% margin", or empty when there is no price to compare with.
    pub margin: String,
    /// "Masala has never been priced" — named, never silently free.
    pub unpriced: Vec<String>,
    pub exists: bool,
}

/// One ledger row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct StockMovementView {
    pub id: String,
    pub material: String,
    /// "Bought", "Sold", "Wasted" — the word, never the tag.
    pub kind: String,
    pub kind_tag: String,
    /// "+2 bag", "−180 g" — signed, in the unit it was typed in.
    pub qty: String,
    pub takes_out: bool,
    pub value: MoneyView,
    pub when: String,
    pub who: String,
    pub reason: String,
    pub note: String,
    /// D111 — this happened because a sale needed it and nobody had recorded
    /// making any.
    pub was_automatic: bool,
}

/// Something the stock book could not do, which did not stop a sale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ProblemView {
    pub id: String,
    pub kind: String,
    /// **The whole sentence** — D100: an unhealthy row carries its own fix.
    pub sentence: String,
    pub times: u32,
    pub when: String,
}

/// One line of the theoretical-against-actual report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct VarianceView {
    pub material: String,
    pub theoretical: String,
    pub actual: String,
    pub variance: String,
    /// "3.0%", or "—" when nothing should have been used.
    pub percent: String,
    pub value: MoneyView,
    pub is_over: bool,
    /// **D115** — "never counted", and the screen says it rather than implying
    /// the figure has been checked against a shelf.
    pub counted: String,
    pub is_unchecked: bool,
}

/// **What a dish costs to make, and what it earns** — scope 4.9.
///
/// **D119 — both cost figures are here on purpose.** Scope 4.1 shipped at P13:
/// an item carries a typed cost price and reports use it. This session produces
/// a second, better answer to the same question, and quietly replacing the
/// first would hide the most valuable thing in the module: **the gap between
/// what an owner thinks a dish costs and what its recipe says it costs is
/// itself the finding.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DishCostView {
    pub item_id: String,
    pub name: String,
    pub sells_for: MoneyView,
    pub has_recipe: bool,
    /// What the recipe says, or ₹0.00 when there is no recipe — and
    /// `has_recipe` is how a screen tells the two apart, because a dish nobody
    /// has costed is not a dish that costs nothing.
    pub recipe_cost: MoneyView,
    /// What P13's typed cost price says.
    pub typed_cost: Option<MoneyView>,
    /// "62.5% margin", or empty.
    pub margin: String,
    /// **"₹8.20 more than you thought"** — the gap, said out loud, or empty.
    pub gap: String,
    /// Some material in this recipe has never been priced, so the cost is short
    /// by an unknown amount and the screen must not present it as final.
    pub is_incomplete: bool,
}

/// The whole screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct InventoryView {
    pub materials: Vec<MaterialView>,
    /// Scope 4.9 — every dish on the menu, costed from its recipe.
    pub dishes: Vec<DishCostView>,
    /// Scope 4.6 — grouped by where you buy it (D116).
    pub buy_list: Vec<BuyGroupView>,
    pub problems: Vec<ProblemView>,
    pub movements: Vec<StockMovementView>,
    /// The reasons a shop offers for wastage — editable, like every other list.
    pub wastage_reasons: Vec<WastageReasonView>,
    pub total_value: MoneyView,
    /// "12 materials · 3 low · 1 problem" — the summary, made here.
    pub summary: String,
    /// Empty when the balance cache agrees with the ledger, which it should
    /// always. D114: a cache nobody verifies is a stored balance with extra
    /// words.
    pub cache_warning: String,
    pub may_manage: bool,
    pub may_waste: bool,
    pub may_adjust: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BuyGroupView {
    /// "Metro", "The milk van", or "Not said" when the shop has not filled it
    /// in — which is a real answer and not an empty heading.
    pub buy_from: String,
    pub lines: Vec<BuyLineView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BuyLineView {
    pub material_id: String,
    pub material: String,
    /// "0.48 bag" — what is there now.
    pub have: String,
    /// "2 bag" — what to buy, in the pack the shop buys in.
    pub buy: String,
    /// The whole line as one sentence, for the WhatsApp text.
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct WastageReasonView {
    pub id: String,
    pub text: String,
}

// ---------------------------------------------------------------------------
// What a screen sends back.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MaterialEdit {
    pub id: String,
    pub name: String,
    pub dimension: String,
    pub category: String,
    pub buy_from: String,
    /// Typed in the shop's own pack, with the unit beside it. Converted here,
    /// never in the screen (R8, D109).
    pub reorder_level: String,
    pub reorder_qty: String,
    pub reorder_unit: String,
    pub is_perishable: bool,
    pub shelf_life_days: Option<u32>,
    pub is_active: bool,
    pub packs: Vec<PackEdit>,
    pub purchase_unit: String,
    pub recipe_unit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PackEdit {
    pub name: String,
    /// How many of the material's base unit one of these holds — "25000" for a
    /// 25 kg bag of rice measured in grams, or "25" with `unit` = "kg".
    pub size: String,
    pub unit: String,
}

/// One movement a person typed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MovementEdit {
    pub material_id: String,
    /// `opening`, `purchase`, `wastage`, `adjustment`, `production_in`.
    pub kind: String,
    /// Typed in `unit`. A leading minus takes stock away, which is what an
    /// adjustment downwards is; a wastage is always negative whatever is typed.
    pub qty: String,
    pub unit: String,
    pub reason_id: Option<String>,
    pub note: Option<String>,
    /// **The price per PACK**, because that is what a shop knows: "a bag is
    /// ₹1,500". Converted to a cost per base unit here (D118).
    pub cost: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RecipeEdit {
    pub owner_kind: String,
    pub owner_id: String,
    pub batch_qty: String,
    pub batch_unit: String,
    pub lines: Vec<RecipeLineEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RecipeLineEdit {
    pub material_id: String,
    pub qty: String,
    pub unit: String,
    pub yield_percent: u32,
}

// ---------------------------------------------------------------------------
// Reading it.
// ---------------------------------------------------------------------------

pub fn inventory_on(app: &App, material: Option<String>) -> UiResult<InventoryView> {
    let who = guard::require(app, Permission::InventoryView)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();

    let (on_hand, movements, problems, reasons, recipes, drift, items, costs) = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let repos = mb_db::Repos::new(tx);
                    let stock = repos.stock();
                    let on_hand = stock.on_hand(OUTLET, true)?;
                    let chosen = material.as_ref().map(|id| MaterialId::new(id.clone()));
                    let movements = stock.movements(OUTLET, chosen.as_ref(), None, None, 200)?;
                    let problems = stock.problems(OUTLET)?;
                    let reasons = repos.corrections().reasons(OUTLET, "wastage")?;
                    let recipes = stock.recipes(OUTLET)?;
                    // **D114's check, on every open.** Summing the ledger for a
                    // handful of materials is cheap; being wrong about stock for
                    // a year is not.
                    let drift = stock.drifted(OUTLET)?;
                    let items = repos.menu().list_items(OUTLET, false)?;
                    let costs = stock.costs(OUTLET)?;
                    Ok((on_hand, movements, problems, reasons, recipes, drift, items, costs))
                })
                .map_err(|e| words::from_db(&e))
        })?;

    let made: Vec<MaterialId> = recipes
        .iter()
        .filter_map(|r| match &r.owner {
            RecipeOwner::Material(id) => Some(id.clone()),
            _ => None,
        })
        .collect();
    let mut used: BTreeMap<String, u32> = BTreeMap::new();
    for recipe in recipes.iter() {
        for line in &recipe.lines {
            *used.entry(line.material.to_string()).or_insert(0) += 1;
        }
    }

    let mut total = Money::ZERO;
    let mut low = 0_u32;
    let mut materials = Vec::with_capacity(on_hand.len());
    for held in &on_hand {
        total = total.add(held.value()).unwrap_or(total);
        if held.is_low() {
            low += 1;
        }
        materials.push(material_view(
            held,
            made.contains(&held.material.id),
            used.get(held.material.id.as_str()).copied().unwrap_or(0),
            at,
        ));
    }

    // Scope 4.9 — every dish, costed from its recipe, with D119's gap beside it.
    let names: BTreeMap<MaterialId, String> =
        on_hand.iter().map(|h| (h.material.id.clone(), h.material.name.clone())).collect();
    let dishes: Vec<DishCostView> = items
        .iter()
        .map(|item| {
            let owner = RecipeOwner::Item(item.id.clone());
            let costed = mb_core::cost_of(&owner, &recipes, &costs, &names);
            DishCostView {
                item_id: item.id.to_string(),
                name: item.name.clone(),
                sells_for: item.unit_price.into(),
                has_recipe: !costed.no_recipe,
                recipe_cost: costed.total.into(),
                typed_cost: item.cost_price.map(Into::into),
                margin: margin_of(item.unit_price, costed.total, costed.no_recipe),
                gap: gap_of(item.cost_price, costed.total, costed.no_recipe),
                is_incomplete: !costed.unpriced.is_empty(),
            }
        })
        .collect();

    Ok(InventoryView {
        buy_list: buy_list(&on_hand),
        dishes,
        summary: format!(
            "{} · {} · {}",
            words::count(i64::from(crate::ipc::count(materials.len() as i64)), "material", "materials"),
            words::count(i64::from(low), "low", "low"),
            words::count(i64::from(crate::ipc::count(problems.len() as i64)), "problem", "problems"),
        ),
        materials,
        problems: problems
            .iter()
            .map(|p| ProblemView {
                id: p.id.clone(),
                kind: p.kind.clone(),
                sentence: p.sentence.clone(),
                times: crate::ipc::count(p.occurrences),
                when: words::when(p.last_at),
            })
            .collect(),
        movements: movements.iter().map(movement_view).collect(),
        wastage_reasons: reasons
            .iter()
            .map(|r| WastageReasonView { id: r.id.clone(), text: r.text.clone() })
            .collect(),
        total_value: total.into(),
        cache_warning: if drift.is_empty() {
            String::new()
        } else {
            format!(
                "{} do not match the movement list. Press Rebuild to work them out again from the movements.",
                words::count(i64::from(crate::ipc::count(drift.len() as i64)), "material", "materials"),
            )
        },
        may_manage: who.can(Permission::InventoryManage),
        may_waste: who.can(Permission::StockWaste),
        may_adjust: who.can(Permission::StockAdjust),
    })
}


fn material_view(
    held: &mb_db::repo::stock::OnHand,
    is_made: bool,
    used_by: u32,
    at: Timestamp,
) -> MaterialView {
    let m = &held.material;
    let units = m.units();
    let purchase = m.default_purchase_unit();
    MaterialView {
        id: m.id.to_string(),
        name: m.name.clone(),
        dimension: m.dimension.tag().to_owned(),
        dimension_label: m.dimension.label().to_owned(),
        base_unit: m.dimension.base_unit().to_owned(),
        category: m.category.clone(),
        buy_from: m.buy_from.clone(),
        on_hand: minus(&units.say(held.base_qty)),
        on_hand_base: held.base_qty.to_string(),
        is_negative: held.base_qty.is_negative(),
        value: held.value().into(),
        // **Priced in the unit a shopkeeper says out loud** — the pack they buy
        // it in when they have named one, and otherwise the dimension's price
        // unit. Not the base unit: "₹0.04 per g" is what the base unit produces
        // and it is not a sentence anybody uses.
        cost: cost_sentence(
            m.avg_cost,
            &units,
            m.purchase_unit.as_deref().unwrap_or_else(|| m.dimension.price_unit()),
        ),
        cost_when: match m.cost_changed_at {
            Some(when) => format!("changed {}", words::when(when)),
            None => "never priced".to_owned(),
        },
        last_counted: match m.last_counted_at {
            Some(when) => words::when(when),
            // **D115.** A blank here would read as "recently"; this reads as
            // what it is.
            None => "never counted".to_owned(),
        },
        is_low: held.is_low(),
        buy: if held.is_low() && m.reorder_qty.is_positive() {
            in_purchase_unit(m.reorder_qty, m, &units)
        } else {
            String::new()
        },
        reorder_level: in_purchase_unit(m.reorder_level, m, &units),
        reorder_qty: in_purchase_unit(m.reorder_qty, m, &units),
        is_perishable: m.is_perishable,
        shelf_life_days: m.shelf_life_days,
        warning: stale_warning(held, at),
        is_active: m.is_active,
        is_made,
        units: units
            .all()
            .map(|p| UnitView {
                name: p.name.clone(),
                base_per_unit: p.base_per_unit.to_string(),
                is_standard: p.is_standard,
            })
            .collect(),
        purchase_unit: purchase,
        recipe_unit: m.default_recipe_unit(),
        used_by,
    }
}

/// **D117 — a property that warns.** Not batch tracking: a perishable material
/// that has not moved for longer than it keeps is a sentence an owner can act
/// on, and it needs nothing a small kitchen will not actually record.
#[allow(
    clippy::integer_division,
    reason = "milliseconds into whole days — a remainder is not a loss on a clock"
)]
fn stale_warning(held: &mb_db::repo::stock::OnHand, at: Timestamp) -> String {
    let m = &held.material;
    if !m.is_perishable || !held.base_qty.is_positive() {
        return String::new();
    }
    let Some(days) = m.shelf_life_days else {
        return String::new();
    };
    let Some(last) = held.last_movement_at else {
        return String::new();
    };
    let elapsed = (at.millis() - last.millis()) / 86_400_000;
    if elapsed > i64::from(days) {
        format!(
            "{} keeps {} and has not moved in {}. Check it.",
            m.name,
            words::count(i64::from(days), "day", "days"),
            words::count(elapsed, "day", "days"),
        )
    } else {
        String::new()
    }
}

fn cost_sentence(cost: UnitCost, units: &Units, unit: &str) -> String {
    if cost.is_zero() {
        return String::new();
    }
    let Some(pack) = units.find(unit) else {
        return String::new();
    };
    match cost.per_pack(pack) {
        Ok(money) => format!("{} per {}", money.to_indian_string(), pack.name),
        Err(_) => String::new(),
    }
}

/// **What to say when a quantity is shown**, and it is never the base unit
/// unless the base unit is what a person would say.
///
/// A shop that has named a pack is counted in it — "2 bag". A shop that has not
/// gets the biggest unit one of it fits into — "5 kg", never "5000 g". Found by
/// looking at the buy list, which was telling somebody going to the market to
/// bring five thousand grams of paneer.
fn in_purchase_unit(base: Qty, material: &Material, units: &Units) -> String {
    match material.purchase_unit.as_deref().and_then(|name| units.find(name)) {
        Some(pack) => minus(&pack.from_base(base).map_or_else(|_| base.to_string(), |q| pack.say(q))),
        None => minus(&units.say(base)),
    }
}

/// **A minus sign, not a hyphen** (UI_GUIDELINES §6).
///
/// Done here and not in `Qty`'s `Display`, because that same `Display` prints a
/// quantity on a thermal receipt through `mb-print`, whose font is an ASCII
/// bitmap. A `−` there would come out as a blank on real paper.
fn minus(said: &str) -> String {
    match said.strip_prefix('-') {
        Some(rest) => format!("−{rest}"),
        None => said.to_owned(),
    }
}

/// Scope 4.6 — grouped by where you buy it, counted in the pack you buy in.
fn buy_list(on_hand: &[mb_db::repo::stock::OnHand]) -> Vec<BuyGroupView> {
    let mut groups: BTreeMap<String, Vec<BuyLineView>> = BTreeMap::new();
    for held in on_hand.iter().filter(|h| h.is_low() && h.material.is_active) {
        let m = &held.material;
        let units = m.units();
        let have = minus(&units.say(held.base_qty));
        let buy = in_purchase_unit(m.reorder_qty, m, &units);
        // "Not said" rather than an empty heading, because a shop that has not
        // filled this in still has to be able to read its own list.
        let key = if m.buy_from.trim().is_empty() {
            "Not said".to_owned()
        } else {
            m.buy_from.clone()
        };
        groups.entry(key).or_default().push(BuyLineView {
            material_id: m.id.to_string(),
            material: m.name.clone(),
            line: format!("{} — have {have}, buy {buy}", m.name),
            have,
            buy,
        });
    }
    groups.into_iter().map(|(buy_from, lines)| BuyGroupView { buy_from, lines }).collect()
}

fn movement_view(row: &mb_db::repo::stock::MovementRow) -> StockMovementView {
    StockMovementView {
        id: row.id.clone(),
        material: row.material_name.clone(),
        // **Name what it fed.** "Used to make something" leaves the reader to
        // work out why the tomato moved, which is the whole reason
        // `produced_for` is a column.
        kind: match (row.kind, row.produced_for.as_deref()) {
            (MovementKind::ProductionOut, Some(made)) => format!("Used to make {made}"),
            _ => row.kind.label().to_owned(),
        },
        kind_tag: row.kind.tag().to_owned(),
        qty: signed(row.typed_qty, &row.typed_unit),
        takes_out: row.base_qty.is_negative(),
        value: row.total_cost.abs().into(),
        when: words::when(row.at),
        who: row.staff.as_ref().map(ToString::to_string).unwrap_or_default(),
        reason: row.reason.clone().unwrap_or_default(),
        note: row.note.clone().unwrap_or_default(),
        was_automatic: row.was_automatic,
    }
}

/// "+2 bag", "−180 g". A minus sign, not a hyphen (UI_GUIDELINES §6).
fn signed(qty: Qty, unit: &str) -> String {
    let unit = if unit.is_empty() { String::new() } else { format!(" {unit}") };
    if qty.is_negative() {
        format!("−{}{unit}", qty.abs())
    } else {
        format!("+{qty}{unit}")
    }
}

// ---------------------------------------------------------------------------
// Recipes.
// ---------------------------------------------------------------------------

pub fn recipe_on(app: &App, owner_kind: String, owner_id: String) -> UiResult<RecipeView> {
    guard::require(app, Permission::InventoryView)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let owner = owner_from(&owner_kind, &owner_id)?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let stock = repos.stock();
                let recipes = stock.recipes(OUTLET)?;
                let costs = stock.costs(OUTLET)?;
                let materials = stock.materials(OUTLET, true)?;
                let names: BTreeMap<MaterialId, String> =
                    materials.iter().map(|m| (m.id.clone(), m.name.clone())).collect();
                let costed = mb_core::cost_of(&owner, &recipes, &costs, &names);

                let (typed_cost, sells_for, owner_name) = match &owner {
                    RecipeOwner::Item(id) => {
                        let item = repos.menu().find_item(id)?;
                        (
                            item.as_ref().and_then(|i| i.cost_price),
                            item.as_ref().map(|i| i.unit_price),
                            item.map_or_else(|| id.to_string(), |i| i.name),
                        )
                    }
                    RecipeOwner::Modifier(id) => (None, None, id.to_string()),
                    RecipeOwner::Material(id) => (
                        None,
                        None,
                        names.get(id).cloned().unwrap_or_else(|| id.to_string()),
                    ),
                };

                Ok(build_recipe_view(
                    &owner,
                    owner_name,
                    recipes.get(&owner),
                    &costed,
                    &materials,
                    typed_cost,
                    sells_for,
                ))
            })
            .map_err(|e| words::from_db(&e))
    })
}

#[allow(
    clippy::too_many_arguments,
    clippy::integer_division,
    reason = "one view assembled from what it shows; a margin in tenths of a \
              percent IS a division, and the denominator is guarded"
)]
fn build_recipe_view(
    owner: &RecipeOwner,
    owner_name: String,
    recipe: Option<&Recipe>,
    costed: &mb_core::Costed,
    materials: &[Material],
    typed_cost: Option<Money>,
    sells_for: Option<Money>,
) -> RecipeView {
    let by_id: BTreeMap<&str, &Material> =
        materials.iter().map(|m| (m.id.as_str(), m)).collect();

    let mut lines = Vec::new();
    if let Some(recipe) = recipe {
        for (n, line) in recipe.lines.iter().enumerate() {
            let material = by_id.get(line.material.as_str());
            let units = material.map_or_else(
                || Units::standard(Dimension::Weight),
                |m| m.units(),
            );
            let costed_line = costed.lines.get(n);
            lines.push(RecipeLineView {
                material_id: line.material.to_string(),
                material: material.map_or_else(|| line.material.to_string(), |m| m.name.clone()),
                qty: line.typed_qty.to_string(),
                unit: line.typed_unit.clone(),
                units: units
                    .all()
                    .map(|p| UnitView {
                        name: p.name.clone(),
                        base_per_unit: p.base_per_unit.to_string(),
                        is_standard: p.is_standard,
                    })
                    .collect(),
                yield_percent: line.yield_percent,
                issued: format!(
                    "{} {}",
                    costed_line.map_or(line.base_qty, |c| c.issued),
                    units.base_unit()
                ),
                cost: costed_line.map_or(Money::ZERO, |c| c.cost).into(),
                is_unpriced: costed.unpriced.contains(&line.material),
            });
        }
    }

    let margin = match (sells_for, costed.no_recipe) {
        (Some(price), false) if price.is_positive() => {
            let profit = price.sub(costed.total).unwrap_or(Money::ZERO);
            let bp = i64::from(profit.paise() != 0)
                * (profit.paise().saturating_mul(1_000) / price.paise().max(1));
            format!("{}.{}% margin", bp / 10, (bp % 10).abs())
        }
        _ => String::new(),
    };

    let batch = recipe.map_or(Qty::ONE, |r| r.batch_yield);
    let batch_unit = match owner {
        RecipeOwner::Material(id) => by_id
            .get(id.as_str())
            .map_or_else(String::new, |m| m.dimension.base_unit().to_owned()),
        _ => String::new(),
    };

    RecipeView {
        owner_kind: owner.tag().to_owned(),
        owner_id: owner.subject().to_owned(),
        owner: owner_name,
        batch: if batch_unit.is_empty() {
            String::new()
        } else {
            format!("This batch makes {batch} {batch_unit}")
        },
        batch_qty: batch.to_string(),
        batch_unit,
        lines,
        cost: costed.total.into(),
        typed_cost: typed_cost.map(Into::into),
        sells_for: sells_for.map(Into::into),
        margin,
        unpriced: costed
            .unpriced
            .iter()
            .map(|id| {
                format!(
                    "{} has never been priced, so it counts as free here.",
                    by_id.get(id.as_str()).map_or_else(|| id.to_string(), |m| m.name.clone())
                )
            })
            .collect(),
        exists: recipe.is_some(),
    }
}

fn owner_from(kind: &str, id: &str) -> UiResult<RecipeOwner> {
    match kind {
        "item" => Ok(RecipeOwner::Item(ItemId::new(id))),
        "modifier" => Ok(RecipeOwner::Modifier(ModifierId::new(id))),
        "material" => Ok(RecipeOwner::Material(MaterialId::new(id))),
        other => Err(UiError::new(
            "recipe.owner",
            "That is not something a recipe can belong to.",
        )
        .with_detail(format!("owner kind `{other}`"))),
    }
}

// ---------------------------------------------------------------------------
// Writing it.
// ---------------------------------------------------------------------------

pub fn save_material_on(app: &App, edit: MaterialEdit) -> UiResult<InventoryView> {
    let who = guard::require(app, Permission::InventoryManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let day = today(at);

    let dimension = Dimension::from_tag(&edit.dimension).ok_or_else(|| {
        UiError::new("material.dimension", "Choose whether it is weighed, poured or counted.")
    })?;
    if edit.name.trim().is_empty() {
        return Err(UiError::new("material.name", "Give the material a name."));
    }

    // The packs first, because the reorder figures are typed in one of them.
    let mut units = Units::standard(dimension);
    let mut packs = Vec::new();
    for pack in &edit.packs {
        if pack.name.trim().is_empty() {
            continue;
        }
        let typed = Qty::parse(pack.size.trim()).map_err(|e| {
            UiError::new("material.pack", format!("`{}` is not a size.", pack.size))
                .with_detail(e.to_string())
        })?;
        // A pack may be described in another unit — "a bag is 25 kg" — so the
        // conversion happens here and the stored number is base units (D108).
        let base = units.to_base(typed, unit_or_base(&pack.unit, dimension)).map_err(|e| {
            UiError::new("material.pack", format!("`{}` is not a unit.", pack.unit))
                .with_detail(e.to_string())
        })?;
        units = units.clone().with_pack(pack.name.trim(), base).map_err(|e| {
            UiError::new("material.pack", format!("`{}` is already a unit here.", pack.name))
                .with_detail(e.to_string())
        })?;
        packs.push((pack.name.trim().to_owned(), base));
    }

    let reorder_unit = unit_or_base(&edit.reorder_unit, dimension);
    let level = parse_in(&edit.reorder_level, &units, reorder_unit, "reorder level")?;
    let qty = parse_in(&edit.reorder_qty, &units, reorder_unit, "reorder quantity")?;

    let existing = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).stock().material(OUTLET, &MaterialId::new(edit.id.clone()))
            })
            .map_err(|e| words::from_db(&e))
    })?;

    let mut material = Material::new(MaterialId::new(edit.id.clone()), edit.name.trim(), dimension);
    material.category = edit.category.trim().to_owned();
    material.buy_from = edit.buy_from.trim().to_owned();
    material.reorder_level = level;
    material.reorder_qty = qty;
    material.is_perishable = edit.is_perishable;
    material.shelf_life_days = edit.shelf_life_days;
    material.is_active = edit.is_active;
    material.packs = packs;
    material.purchase_unit = non_empty(&edit.purchase_unit);
    material.recipe_unit = non_empty(&edit.recipe_unit);
    // **The average cost is the ledger's answer (D118)** and is carried over
    // untouched, never taken from the screen.
    if let Some(before) = &existing {
        material.avg_cost = before.avg_cost;
        material.cost_changed_at = before.cost_changed_at;
        material.last_counted_at = before.last_counted_at;
        material.sort_order = before.sort_order;
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.stock().save_material(OUTLET, &material, at)?;
                // **R11** — in the same transaction as the thing it records.
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(at, day, Some(who.staff_id.clone()), action::MATERIAL_SAVED, "material")
                        .about(material.id.to_string())
                        .with_after(serde_json::json!({
                            "name": material.name,
                            "dimension": material.dimension.tag(),
                            "is_active": material.is_active,
                        })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!("material {} saved by {}", material.name, who.name);
    inventory_on(app, None)
}

/// "62.5% margin", in tenths of a percent so there is no float.
#[allow(
    clippy::integer_division,
    reason = "a margin in tenths of a percent IS a division; the denominator is guarded"
)]
fn margin_of(price: Money, cost: Money, no_recipe: bool) -> String {
    if no_recipe || !price.is_positive() {
        return String::new();
    }
    let profit = price.sub(cost).unwrap_or(Money::ZERO);
    let tenths = profit.paise().saturating_mul(1_000) / price.paise();
    format!("{}.{}% margin", tenths / 10, (tenths % 10).abs())
}

/// **D119's finding, as a sentence.** The gap between what an owner had a dish
/// down as and what its recipe actually says — which is the number they bought
/// this module to see.
fn gap_of(typed: Option<Money>, cost: Money, no_recipe: bool) -> String {
    if no_recipe {
        return String::new();
    }
    let Some(typed) = typed else {
        return String::new();
    };
    let difference = cost.sub(typed).unwrap_or(Money::ZERO);
    if difference.is_zero() {
        return String::new();
    }
    if difference.is_positive() {
        format!("{} more than you thought", difference.to_indian_string())
    } else {
        format!("{} less than you thought", difference.abs().to_indian_string())
    }
}

fn non_empty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

fn unit_or_base(unit: &str, dimension: Dimension) -> &str {
    if unit.trim().is_empty() { dimension.base_unit() } else { unit.trim() }
}

fn parse_in(text: &str, units: &Units, unit: &str, what: &str) -> UiResult<Qty> {
    if text.trim().is_empty() {
        return Ok(Qty::ZERO);
    }
    let typed = Qty::parse(text.trim()).map_err(|e| {
        UiError::new("material.qty", format!("`{text}` is not a {what}."))
            .with_detail(e.to_string())
    })?;
    units.to_base(typed, unit).map_err(|e| {
        UiError::new("material.unit", format!("`{unit}` is not a unit of this material."))
            .with_detail(e.to_string())
    })
}

pub fn save_recipe_on(app: &App, edit: RecipeEdit) -> UiResult<RecipeView> {
    let who = guard::require(app, Permission::InventoryManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let day = today(at);
    let owner = owner_from(&edit.owner_kind, &edit.owner_id)?;

    let materials = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).stock().materials(OUTLET, true))
            .map_err(|e| words::from_db(&e))
    })?;
    let by_id: BTreeMap<&str, &Material> = materials.iter().map(|m| (m.id.as_str(), m)).collect();

    let mut lines = Vec::new();
    for line in &edit.lines {
        if line.material_id.trim().is_empty() {
            continue;
        }
        let material = by_id.get(line.material_id.as_str()).ok_or_else(|| {
            UiError::new("recipe.material", "That material is not in the list any more.")
        })?;
        let units = material.units();
        let unit = unit_or_base(&line.unit, material.dimension);
        let typed = Qty::parse(line.qty.trim()).map_err(|e| {
            UiError::new(
                "recipe.qty",
                format!("`{}` is not an amount of {}.", line.qty, material.name),
            )
            .with_detail(e.to_string())
        })?;
        let base = units.to_base(typed, unit).map_err(|e| {
            UiError::new("recipe.unit", format!("`{unit}` is not a unit of {}.", material.name))
                .with_detail(e.to_string())
        })?;
        if !base.is_positive() {
            return Err(UiError::new(
                "recipe.qty",
                format!("A recipe has to use some {}. Type an amount, or remove the line.", material.name),
            ));
        }
        if line.yield_percent == 0 || line.yield_percent > 100 {
            return Err(UiError::new(
                "recipe.yield",
                format!(
                    "Say how much of the {} reaches the dish — between 1 and 100.",
                    material.name
                ),
            ));
        }
        lines.push(RecipeLine {
            material: material.id.clone(),
            base_qty: base,
            yield_percent: line.yield_percent,
            typed_qty: typed,
            typed_unit: unit.to_owned(),
        });
    }

    // A made material's batch is in its OWN base units; a dish is one serving.
    let batch = match &owner {
        RecipeOwner::Material(id) => {
            let material = by_id.get(id.as_str()).ok_or_else(|| {
                UiError::new("recipe.owner", "That material is not in the list any more.")
            })?;
            let typed = Qty::parse(edit.batch_qty.trim()).map_err(|e| {
                UiError::new("recipe.batch", "Say how much one batch makes.")
                    .with_detail(e.to_string())
            })?;
            material
                .units()
                .to_base(typed, unit_or_base(&edit.batch_unit, material.dimension))
                .map_err(|e| {
                    UiError::new("recipe.batch", "That is not a unit of this material.")
                        .with_detail(e.to_string())
                })?
        }
        _ => Qty::ONE,
    };
    if !batch.is_positive() {
        return Err(UiError::new(
            "recipe.batch",
            "A batch has to make more than nothing.",
        ));
    }

    let recipe = Recipe { owner: owner.clone(), batch_yield: batch, lines };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                // **D75** — a refusal a person must act on is returned as a
                // value, so the cycle message reaches the screen as itself
                // rather than as "The shop's data could not be read".
                repos.stock().save_recipe(OUTLET, &recipe, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(at, day, Some(who.staff_id.clone()), action::RECIPE_SAVED, "recipe")
                        .about(format!("{}:{}", recipe.owner.tag(), recipe.owner.subject()))
                        .with_after(serde_json::json!({ "lines": recipe.lines.len() })),
                )?;
                Ok(())
            })
            .map_err(|e| refusal(&e))
    })?;

    log_info!("recipe for {} saved by {}", owner.subject(), who.name);
    recipe_on(app, edit.owner_kind, edit.owner_id)
}

/// **D75** — a rule a person must act on becomes the message they read.
///
/// `words::from_db` rewrites a storage error into "The shop's data could not be
/// read", which is right for a corrupt row and wrong for *"this recipe goes
/// round in circles: Powder → Paste → Masala → Powder"*.
fn refusal(e: &mb_db::DbError) -> UiError {
    match e {
        mb_db::DbError::Invariant(said) => UiError::new("recipe.refused", said.clone()),
        other => words::from_db(other),
    }
}

pub fn delete_recipe_on(app: &App, owner_kind: String, owner_id: String) -> UiResult<RecipeView> {
    let who = guard::require(app, Permission::InventoryManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let owner = owner_from(&owner_kind, &owner_id)?;
    let at = now();
    let day = today(at);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.stock().delete_recipe(OUTLET, &owner)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(at, day, Some(who.staff_id.clone()), action::RECIPE_SAVED, "recipe")
                        .about(format!("{}:{}", owner.tag(), owner.subject()))
                        .with_after(serde_json::json!({ "deleted": true })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;
    recipe_on(app, owner_kind, owner_id)
}

/// One movement a person typed: an opening balance, a purchase before P26, a
/// wastage entry or an adjustment.
pub fn record_movement_on(app: &App, edit: MovementEdit) -> UiResult<InventoryView> {
    let MovementEdit { material_id, kind, qty, unit, reason_id, note, cost } = edit;
    let wanted = MovementKind::ALL
        .iter()
        .copied()
        .find(|k| k.tag() == kind)
        .ok_or_else(|| UiError::new("stock.kind", "That is not a kind of stock movement."))?;

    // **Wastage is a lower bar than an adjustment on purpose.** Writing down
    // that a pan was burnt is a normal evening, and a shop where only the owner
    // may record it is a shop where nobody does.
    let permission = match wanted {
        MovementKind::Wastage => Permission::StockWaste,
        _ => Permission::StockAdjust,
    };
    let who = guard::require(app, permission)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let day = today(at);
    let id = MaterialId::new(material_id);

    let material = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| mb_db::Repos::new(tx).stock().material(OUTLET, &id))
                .map_err(|e| words::from_db(&e))
        })?
        .ok_or_else(|| UiError::new("stock.material", "That material is not in the list."))?;

    let units = material.units();
    let unit = unit_or_base(&unit, material.dimension).to_owned();
    let typed = Qty::parse(qty.trim().trim_start_matches('-')).map_err(|e| {
        UiError::new("stock.qty", format!("`{qty}` is not an amount."))
            .with_detail(e.to_string())
    })?;
    if typed.is_zero() {
        return Err(UiError::new("stock.qty", "A movement of nothing is not a movement."));
    }
    let base = units.to_base(typed, &unit).map_err(|e| {
        UiError::new("stock.unit", format!("`{unit}` is not a unit of {}.", material.name))
            .with_detail(e.to_string())
    })?;

    // A wastage always takes away; an adjustment goes whichever way the person
    // typed; everything else adds.
    let negative = matches!(wanted, MovementKind::Wastage | MovementKind::ProductionOut)
        || qty.trim().starts_with('-');
    let signed_base = if negative { Qty::from_thousandths(-base.thousandths()) } else { base };
    let signed_typed = if negative { Qty::from_thousandths(-typed.thousandths()) } else { typed };

    // A price is per PACK, because that is what a shop knows: "a bag is ₹1,500".
    let unit_cost = match cost.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(text) => {
            let money = Money::parse(text).map_err(|e| {
                UiError::new("stock.cost", format!("`{text}` is not an amount of money."))
                    .with_detail(e.to_string())
            })?;
            let pack = units.find(&unit).ok_or_else(|| {
                UiError::new("stock.unit", "That is not a unit of this material.")
            })?;
            Some(UnitCost::from_pack_price(money, pack).map_err(|e| {
                UiError::new("stock.cost", "That price cannot be worked out per unit.")
                    .with_detail(e.to_string())
            })?)
        }
        None => None,
    };

    let mut movement = Movement::new(
        format!("stk_{}_{}", at.millis(), id),
        id.clone(),
        wanted,
        signed_base,
        at,
        day,
    )
    .typed(signed_typed, unit)
    .by(who.staff_id.clone());
    movement.reason_id = reason_id.filter(|r| !r.is_empty());
    movement.note = note.filter(|n| !n.trim().is_empty());
    movement.unit_cost = unit_cost;

    let audit_action = match wanted {
        MovementKind::Wastage => action::STOCK_WASTED,
        MovementKind::ProductionIn => action::STOCK_PRODUCED,
        _ => action::STOCK_ADJUSTED,
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.stock().record(OUTLET, &movement)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(at, day, Some(who.staff_id.clone()), audit_action, "material")
                        .about(id.to_string())
                        .with_after(serde_json::json!({
                            "kind": wanted.tag(),
                            "base_qty": signed_base.thousandths(),
                        })),
                )?;
                Ok(())
            })
            .map_err(|e| refusal(&e))
    })?;

    log_info!("{} {} by {}", wanted.label(), material.name, who.name);
    inventory_on(app, None)
}

/// **D114** — work the balances out again from the movements.
pub fn rebuild_balances_on(app: &App) -> UiResult<InventoryView> {
    let who = guard::require(app, Permission::StockAdjust)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let day = today(at);

    let corrected = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let corrected = repos.stock().rebuild_balances(OUTLET, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(at, day, Some(who.staff_id.clone()), action::STOCK_REBUILT, "stock")
                        .with_after(serde_json::json!({ "corrected": corrected })),
                )?;
                Ok(corrected)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!("stock balances rebuilt by {} — {corrected} corrected", who.name);
    inventory_on(app, None)
}

pub fn resolve_problem_on(app: &App, id: String) -> UiResult<InventoryView> {
    guard::require(app, Permission::InventoryManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).stock().resolve_problem(OUTLET, &id, at))
            .map_err(|e| words::from_db(&e))
    })?;
    inventory_on(app, None)
}

/// Scope 4.9, D115 — theoretical against actual, for a period.
#[allow(
    clippy::integer_division,
    reason = "basis points into a percentage with one decimal; the `None` arm is \
              the only case that could lose anything and it is handled"
)]
pub fn variance_on(app: &App, from: String, to: String) -> UiResult<Vec<VarianceView>> {
    guard::require(app, Permission::InventoryView)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    // `<input type="date">` gives `YYYY-MM-DD`, and `BusinessDay: FromStr` is
    // what P18 added so that no TypeScript does date arithmetic on the value
    // every report is keyed by.
    let bad = |which: &str, text: &str| {
        UiError::new("stock.period", format!("The {which} date could not be read. Pick it again."))
            .with_detail(format!("{which} = {text:?}"))
    };
    let from: mb_core::BusinessDay = from.parse().map_err(|_| bad("start", &from))?;
    let to: mb_core::BusinessDay = to.parse().map_err(|_| bad("end", &to))?;
    if from.days_until(to) < 0 {
        return Err(UiError::new("stock.period", "The end date is before the start date."));
    }

    let rows = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).stock().consumption(OUTLET, from, to))
            .map_err(|e| words::from_db(&e))
    })?;

    Ok(rows
        .iter()
        .map(|row| VarianceView {
            material: row.name.clone(),
            theoretical: row.theoretical.to_string(),
            actual: row.actual.to_string(),
            variance: row.variance.to_string(),
            percent: match row.variance_bp() {
                Some(bp) => format!("{}.{}%", bp / 100, (bp % 100).abs() / 10),
                None => "—".to_owned(),
            },
            value: row.variance_value.abs().into(),
            is_over: row.variance.is_positive(),
            counted: match row.last_counted_at {
                Some(when) => words::when(when),
                None => "never counted".to_owned(),
            },
            is_unchecked: row.last_counted_at.is_none(),
        })
        .collect())
}

/// The buy list as text a person can send on WhatsApp (scope 4.6).
pub fn buy_list_text_on(app: &App) -> UiResult<String> {
    guard::require(app, Permission::InventoryView)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let view = inventory_on(app, None)?;
    let mut out = String::from("What to buy today\n");
    for group in &view.buy_list {
        out.push_str(&format!("\n{}\n", group.buy_from));
        for line in &group.lines {
            out.push_str(&format!("  {}\n", line.line));
        }
    }
    if view.buy_list.is_empty() {
        out.push_str("\nNothing is below its reorder level.\n");
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// The seats (D46).
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn inventory(app: tauri::State<'_, App>, material: Option<String>) -> UiResult<InventoryView> {
    inventory_on(&app, material)
}

#[tauri::command]
pub fn recipe(
    app: tauri::State<'_, App>,
    owner_kind: String,
    owner_id: String,
) -> UiResult<RecipeView> {
    recipe_on(&app, owner_kind, owner_id)
}

#[tauri::command]
pub fn save_material(app: tauri::State<'_, App>, edit: MaterialEdit) -> UiResult<InventoryView> {
    save_material_on(&app, edit)
}

#[tauri::command]
pub fn save_recipe(app: tauri::State<'_, App>, edit: RecipeEdit) -> UiResult<RecipeView> {
    save_recipe_on(&app, edit)
}

#[tauri::command]
pub fn delete_recipe(
    app: tauri::State<'_, App>,
    owner_kind: String,
    owner_id: String,
) -> UiResult<RecipeView> {
    delete_recipe_on(&app, owner_kind, owner_id)
}

#[tauri::command]
pub fn record_stock_movement(
    app: tauri::State<'_, App>,
    edit: MovementEdit,
) -> UiResult<InventoryView> {
    record_movement_on(&app, edit)
}

#[tauri::command]
pub fn rebuild_stock_balances(app: tauri::State<'_, App>) -> UiResult<InventoryView> {
    rebuild_balances_on(&app)
}

#[tauri::command]
pub fn resolve_stock_problem(app: tauri::State<'_, App>, id: String) -> UiResult<InventoryView> {
    resolve_problem_on(&app, id)
}

#[tauri::command]
pub fn stock_variance(
    app: tauri::State<'_, App>,
    from: String,
    to: String,
) -> UiResult<Vec<VarianceView>> {
    variance_on(&app, from, to)
}

#[tauri::command]
pub fn buy_list_text(app: tauri::State<'_, App>) -> UiResult<String> {
    buy_list_text_on(&app)
}
