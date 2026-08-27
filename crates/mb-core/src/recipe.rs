//! Recipes, and what a sale takes off the shelf.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::ids::{ItemId, MaterialId, ModifierId};
use crate::money::Money;
use crate::qty::Qty;
use crate::units::UnitCost;

/// How deep a recipe may nest before this stops walking.
pub const MAX_DEPTH: u8 = 10;

/// What a recipe belongs to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum RecipeOwner {
    /// A dish, or one size of a dish.
    Item(ItemId),
    /// "Extra cheese" — a modifier that consumes something.
    Modifier(ModifierId),
    /// A made material: gravy base, masala mix, dough.
    Material(MaterialId),
}

impl RecipeOwner {
    /// The tag stored in the database, spelled the way serde spells it.
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        match self {
            RecipeOwner::Item(_) => "item",
            RecipeOwner::Modifier(_) => "modifier",
            RecipeOwner::Material(_) => "material",
        }
    }

    #[must_use]
    pub fn subject(&self) -> &str {
        match self {
            RecipeOwner::Item(id) => id.as_str(),
            RecipeOwner::Modifier(id) => id.as_str(),
            RecipeOwner::Material(id) => id.as_str(),
        }
    }
}

/// One material a recipe uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeLine {
    pub material: MaterialId,
    /// How much reaches the dish, in the material's base units.
    pub base_qty: Qty,
    /// How much of what you issue survives, 1 to 100.
    pub yield_percent: u32,
    /// What the person typed and in which unit, kept only so the screen can show it back the
    /// way it was entered.
    pub typed_qty: Qty,
    pub typed_unit: String,
}

impl RecipeLine {
    /// The everyday line: this much, all of it reaching the dish.
    pub fn new(
        material: MaterialId,
        base_qty: Qty,
        typed_qty: Qty,
        typed_unit: impl Into<String>,
    ) -> Self {
        RecipeLine {
            material,
            base_qty,
            yield_percent: 100,
            typed_qty,
            typed_unit: typed_unit.into(),
        }
    }

    #[must_use]
    pub fn with_yield(mut self, percent: u32) -> Self {
        self.yield_percent = percent;
        self
    }

    /// What actually leaves the shelf for `base_qty` to reach the dish.
    pub fn issued(&self, times: Qty) -> Option<Qty> {
        self.issued_scaled(i128::from(times.thousandths()), 1_000)
    }

    /// What leaves the shelf when this recipe is made `num ÷ den` times.
    #[allow(
        clippy::integer_division,
        reason = "integer division IS the operation; the `/ 2` terms are the \
                  standard half-away-from-zero bias and every value here is a \
                  positive count"
    )]
    pub fn issued_scaled(&self, num: i128, den: i128) -> Option<Qty> {
        if self.yield_percent == 0 || self.yield_percent > 100 || den == 0 {
            return None;
        }
        // Base_qty × num × 100 ÷ (den × yield), rounded once.
        let top = i128::from(self.base_qty.thousandths())
            .checked_mul(num)?
            .checked_mul(100)?;
        let bottom = den.checked_mul(i128::from(self.yield_percent))?;
        let issued = top.checked_add(bottom / 2)? / bottom;
        i64::try_from(issued).ok().map(Qty::from_thousandths)
    }
}

/// A recipe: what one of something is made of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    pub owner: RecipeOwner,
    pub batch_yield: Qty,
    pub lines: Vec<RecipeLine>,
}

impl Recipe {
    /// A dish's or a modifier's recipe: one serving.
    #[must_use]
    pub fn for_one(owner: RecipeOwner, lines: Vec<RecipeLine>) -> Self {
        Recipe {
            owner,
            batch_yield: Qty::ONE,
            lines,
        }
    }

    /// A made material's recipe: this batch makes `batch_yield` of it.
    #[must_use]
    pub fn batch(material: MaterialId, batch_yield: Qty, lines: Vec<RecipeLine>) -> Self {
        Recipe {
            owner: RecipeOwner::Material(material),
            batch_yield,
            lines,
        }
    }
}

/// Everything the explosion is allowed to know, handed in.
#[derive(Debug, Clone, Default)]
pub struct Recipes {
    by_owner: BTreeMap<RecipeOwner, Recipe>,
}

impl Recipes {
    #[must_use]
    pub fn new() -> Self {
        Recipes::default()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_owner.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_owner.len()
    }

    /// Add one. Returns what was there before, if anything.
    pub fn insert(&mut self, recipe: Recipe) -> Option<Recipe> {
        self.by_owner.insert(recipe.owner.clone(), recipe)
    }

    /// Take one out.
    pub fn remove(&mut self, owner: &RecipeOwner) -> Option<Recipe> {
        self.by_owner.remove(owner)
    }

    #[must_use]
    pub fn get(&self, owner: &RecipeOwner) -> Option<&Recipe> {
        self.by_owner.get(owner)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Recipe> {
        self.by_owner.values()
    }

    /// Would saving `candidate` create a loop?
    #[must_use]
    pub fn cycle_if_saved(&self, candidate: &Recipe) -> Option<Vec<MaterialId>> {
        let RecipeOwner::Material(root) = &candidate.owner else {
            // A dish or a modifier is a leaf of nothing — no recipe may reference it, so adding
            // one cannot close a loop.
            return None;
        };
        let mut walked = BTreeSet::new();
        let mut chain = vec![root.clone()];
        if self.walk_for_cycle(root, candidate, &mut walked, &mut chain) {
            Some(chain)
        } else {
            None
        }
    }

    fn walk_for_cycle(
        &self,
        root: &MaterialId,
        recipe: &Recipe,
        walked: &mut BTreeSet<MaterialId>,
        chain: &mut Vec<MaterialId>,
    ) -> bool {
        for line in &recipe.lines {
            chain.push(line.material.clone());
            if &line.material == root {
                return true;
            }
            // A material already walked cannot reach the root by a path we have not already
            // taken, so this both terminates and stays linear.
            if walked.insert(line.material.clone())
                && let Some(next) = self.get(&RecipeOwner::Material(line.material.clone()))
                && self.walk_for_cycle(root, next, walked, chain)
            {
                return true;
            }
            chain.pop();
        }
        false
    }
}

/// One thing that was sold, and how many of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sold {
    /// The order line this came from, so a problem can name it and a reversal can find it.
    pub line_key: String,
    /// What a cook would call it.
    pub name: String,
    pub owner: RecipeOwner,
    pub qty: Qty,
}

/// A material coming off the shelf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draw {
    pub material: MaterialId,
    /// Signed and negative: this is what the ledger row will hold.
    pub base_qty: Qty,
    /// The order line it is on behalf of.
    pub line_key: Option<String>,
    /// Which made material this fed, when it is an input to an automatic production rather than
    /// something a dish used directly.
    pub for_production: Option<MaterialId>,
}

/// A made material that made itself because a sale needed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Production {
    pub material: MaterialId,
    /// Positive: what came into existence.
    pub base_qty: Qty,
    /// Which order line ultimately asked for it.
    pub line_key: Option<String>,
    /// How far down the tree this was, for the ledger row's note.
    pub depth: u8,
}

/// Something the stock book could not do, which did not stop the sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Sold, and nothing says what it is made of.
    NoRecipe { subject: RecipeOwner, name: String },
    /// A recipe uses a material the shop has retired.
    RetiredMaterial { material: MaterialId, name: String },
    /// A recipe line points at a material that is not in the shop at all.
    UnknownMaterial { material: MaterialId, name: String },
    /// The shelf went below zero.
    WentNegative {
        material: MaterialId,
        name: String,
        shortfall: Qty,
        unit: String,
    },
    /// A line said nothing survives, which needs infinite rice.
    ZeroYield { material: MaterialId, name: String },
    /// The tree is deeper than `MAX_DEPTH`, which in practice means a loop the save-time check
    /// did not see.
    TooDeep { material: MaterialId, name: String },
    /// The arithmetic could not be represented.
    Absurd { material: MaterialId, name: String },
}

impl Problem {
    /// The tag stored in the database.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Problem::NoRecipe { .. } => "no_recipe",
            Problem::RetiredMaterial { .. } => "retired_material",
            Problem::UnknownMaterial { .. } => "unknown_material",
            Problem::WentNegative { .. } => "went_negative",
            Problem::ZeroYield { .. } => "zero_yield",
            Problem::TooDeep { .. } => "too_deep",
            Problem::Absurd { .. } => "absurd",
        }
    }

    /// What this is about, so two of the same thing group into one row.
    #[must_use]
    pub fn subject(&self) -> String {
        match self {
            Problem::NoRecipe { subject, .. } => format!("{}:{}", subject.tag(), subject.subject()),
            Problem::RetiredMaterial { material, .. }
            | Problem::UnknownMaterial { material, .. }
            | Problem::WentNegative { material, .. }
            | Problem::ZeroYield { material, .. }
            | Problem::TooDeep { material, .. }
            | Problem::Absurd { material, .. } => material.to_string(),
        }
    }

    /// A sentence an owner reads and can act on.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Problem::NoRecipe { name, .. } => format!(
                "{name} has no recipe, so selling it takes nothing off the shelf. \
                 Add a recipe to include it in stock and food cost."
            ),
            Problem::RetiredMaterial { name, .. } => format!(
                "A recipe still uses {name}, which you have retired. \
                 Bring it back, or take it out of the recipes that use it."
            ),
            Problem::UnknownMaterial { name, .. } => format!(
                "A recipe uses {name}, which is not in the materials list at all. \
                 Add it back, or take it out of the recipes that use it."
            ),
            Problem::WentNegative {
                name,
                shortfall,
                unit,
                ..
            } => format!(
                "{name} went below zero by {shortfall} {unit}. \
                 Either some was bought and never entered, or a recipe uses more than it should."
            ),
            Problem::ZeroYield { name, .. } => format!(
                "A recipe says none of the {name} survives, which cannot be costed. \
                 Set how much of it reaches the dish."
            ),
            Problem::TooDeep { name, .. } => format!(
                "The recipe for {name} goes round in circles or is nested too deep, \
                 so it was left alone. Open it and check what it is made of."
            ),
            Problem::Absurd { name, .. } => format!(
                "The quantity of {name} in a recipe is too large to work with. \
                 Check the amount and the unit on that line."
            ),
        }
    }
}

/// What one bill did to the stock book.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Explosion {
    /// Materials leaving the shelf, aggregated per material per order line.
    pub draws: Vec<Draw>,
    /// Made materials that made themselves.
    pub productions: Vec<Production>,
    /// Everything that went wrong and did not stop the sale.
    pub problems: Vec<Problem>,
}

impl Explosion {
    /// Nothing happened, because the shop has no recipes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.draws.is_empty() && self.productions.is_empty() && self.problems.is_empty()
    }
}

/// What the explosion is allowed to look up about a material.
#[derive(Debug, Clone, Default)]
pub struct MaterialFacts {
    /// Material id → the name a person would read, for the sentences.
    pub names: BTreeMap<MaterialId, String>,
    /// Material id → what is on the shelf right now, in base units.
    pub balances: BTreeMap<MaterialId, Qty>,
    /// Material id → its base unit ("g", "ml", "piece"), so a sentence about a shortfall says
    /// "120 g" and not "120".
    pub base_units: BTreeMap<MaterialId, String>,
    /// The ones the shop has switched off.
    pub retired: BTreeSet<MaterialId>,
}

impl MaterialFacts {
    fn name(&self, material: &MaterialId) -> String {
        self.names
            .get(material)
            .cloned()
            .unwrap_or_else(|| material.to_string())
    }

    fn is_known(&self, material: &MaterialId) -> bool {
        self.names.contains_key(material)
    }

    fn is_retired(&self, material: &MaterialId) -> bool {
        self.retired.contains(material)
    }

    fn base_unit(&self, material: &MaterialId) -> String {
        self.base_units.get(material).cloned().unwrap_or_default()
    }

    fn balance(&self, material: &MaterialId) -> Qty {
        self.balances.get(material).copied().unwrap_or(Qty::ZERO)
    }
}

/// What a bill takes off the shelf.
#[must_use]
pub fn explode(sold: &[Sold], recipes: &Recipes, facts: &MaterialFacts) -> Explosion {
    let mut out = Explosion::default();
    // The shelf as this bill runs, so two lines of the same dish see each other's consumption
    // and the second one is the one that goes negative.
    let mut running: BTreeMap<MaterialId, Qty> = BTreeMap::new();

    for item in sold {
        let Some(recipe) = recipes.get(&item.owner) else {
            out.problems.push(Problem::NoRecipe {
                subject: item.owner.clone(),
                name: item.name.clone(),
            });
            continue;
        };
        draw_from(
            recipe,
            i128::from(item.qty.thousandths()),
            1_000,
            Some(item.line_key.as_str()),
            None,
            0,
            recipes,
            facts,
            &mut running,
            &mut out,
        );
    }

    dedupe(&mut out);
    out
}

/// Walk one recipe `times` over, adding draws and productions.
#[allow(
    clippy::too_many_arguments,
    reason = "a pure walk with nowhere to hide state"
)]
fn draw_from(
    recipe: &Recipe,
    num: i128,
    den: i128,
    line_key: Option<&str>,
    for_production: Option<&MaterialId>,
    depth: u8,
    recipes: &Recipes,
    facts: &MaterialFacts,
    running: &mut BTreeMap<MaterialId, Qty>,
    out: &mut Explosion,
) {
    for line in &recipe.lines {
        let name = facts.name(&line.material);

        if !facts.is_known(&line.material) {
            out.problems.push(Problem::UnknownMaterial {
                material: line.material.clone(),
                name,
            });
            continue;
        }
        // Retired is not missing.
        if facts.is_retired(&line.material) {
            out.problems.push(Problem::RetiredMaterial {
                material: line.material.clone(),
                name: name.clone(),
            });
        }
        let Some(issued) = line.issued_scaled(num, den) else {
            if line.yield_percent == 0 || line.yield_percent > 100 {
                out.problems.push(Problem::ZeroYield {
                    material: line.material.clone(),
                    name,
                });
            } else {
                out.problems.push(Problem::Absurd {
                    material: line.material.clone(),
                    name,
                });
            }
            continue;
        };
        if issued.is_zero() {
            continue;
        }

        take(
            &line.material,
            issued,
            line_key,
            for_production,
            depth,
            recipes,
            facts,
            running,
            out,
        );
    }
}

/// Take `wanted` of one material off the shelf, producing it first if it is a made material and
/// there is not enough.
#[allow(clippy::too_many_arguments, reason = "see `draw_from`")]
fn take(
    material: &MaterialId,
    wanted: Qty,
    line_key: Option<&str>,
    for_production: Option<&MaterialId>,
    depth: u8,
    recipes: &Recipes,
    facts: &MaterialFacts,
    running: &mut BTreeMap<MaterialId, Qty>,
    out: &mut Explosion,
) {
    let name = facts.name(material);
    let on_shelf = running
        .get(material)
        .copied()
        .unwrap_or_else(|| facts.balance(material));

    // Make exactly the shortfall, not a whole batch: a kitchen that makes 4 kg of gravy when it
    // needed 150 ml would show 3.85 kg of gravy on a shelf that has none of it.
    let short = wanted.sub(on_shelf).unwrap_or(wanted).max(Qty::ZERO);
    if short.is_positive()
        && let Some(batch) = recipes.get(&RecipeOwner::Material(material.clone()))
    {
        if depth >= MAX_DEPTH {
            out.problems.push(Problem::TooDeep {
                material: material.clone(),
                name: name.clone(),
            });
        } else if !batch.batch_yield.is_positive() {
            out.problems.push(Problem::Absurd {
                material: material.clone(),
                name: name.clone(),
            });
        } else {
            // The scale stays a fraction.
            draw_from(
                batch,
                i128::from(short.thousandths()),
                i128::from(batch.batch_yield.thousandths()),
                line_key,
                Some(material),
                depth + 1,
                recipes,
                facts,
                running,
                out,
            );
            out.productions.push(Production {
                material: material.clone(),
                base_qty: short,
                line_key: line_key.map(str::to_owned),
                depth: depth + 1,
            });
            let at = running.entry(material.clone()).or_insert(on_shelf);
            *at = at.add(short).unwrap_or(*at);
        }
    }

    let before = running.get(material).copied().unwrap_or(on_shelf);
    let after = before.sub(wanted).unwrap_or(before);
    running.insert(material.clone(), after);

    // A shelf that goes below zero is INFORMATION, not a refusal — and it is only worth saying
    // once, at the moment it crosses.
    if after.is_negative() && !before.is_negative() {
        out.problems.push(Problem::WentNegative {
            material: material.clone(),
            name,
            shortfall: Qty::from_thousandths(-after.thousandths()),
            unit: facts.base_unit(material),
        });
    }

    out.draws.push(Draw {
        material: material.clone(),
        base_qty: Qty::from_thousandths(-wanted.thousandths()),
        line_key: line_key.map(str::to_owned),
        for_production: for_production.cloned(),
    });
}

/// One row per material per order line, and one problem per subject.
fn dedupe(out: &mut Explosion) {
    let mut merged: Vec<Draw> = Vec::with_capacity(out.draws.len());
    for draw in out.draws.drain(..) {
        if let Some(existing) = merged.iter_mut().find(|d| {
            d.material == draw.material
                && d.line_key == draw.line_key
                && d.for_production == draw.for_production
        }) {
            existing.base_qty = existing
                .base_qty
                .add(draw.base_qty)
                .unwrap_or(existing.base_qty);
        } else {
            merged.push(draw);
        }
    }
    out.draws = merged;

    let mut seen = BTreeSet::new();
    out.problems
        .retain(|p| seen.insert((p.kind(), p.subject())));
}

// FOOD COST.

/// One material's share of what a dish costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostedLine {
    pub material: MaterialId,
    pub name: String,
    /// What leaves the shelf, yield included.
    pub issued: Qty,
    pub cost: Money,
}

/// What one of something costs to make, at today's material prices.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Costed {
    pub total: Money,
    pub lines: Vec<CostedLine>,
    /// Materials with no cost recorded.
    pub unpriced: Vec<MaterialId>,
    /// True when there is no recipe at all, so `total` means nothing.
    pub no_recipe: bool,
}

/// What one of `owner` costs to make, walking sub-recipes.
#[must_use]
pub fn cost_of(
    owner: &RecipeOwner,
    recipes: &Recipes,
    costs: &BTreeMap<MaterialId, UnitCost>,
    names: &BTreeMap<MaterialId, String>,
) -> Costed {
    cost_walk(owner, recipes, costs, names, 0)
}

fn cost_walk(
    owner: &RecipeOwner,
    recipes: &Recipes,
    costs: &BTreeMap<MaterialId, UnitCost>,
    names: &BTreeMap<MaterialId, String>,
    depth: u8,
) -> Costed {
    let mut out = Costed::default();
    let Some(recipe) = recipes.get(owner) else {
        out.no_recipe = true;
        return out;
    };
    if depth >= MAX_DEPTH {
        return out;
    }

    for line in &recipe.lines {
        let Some(issued) = line.issued(Qty::ONE) else {
            out.unpriced.push(line.material.clone());
            continue;
        };
        let owned = RecipeOwner::Material(line.material.clone());
        // A made material is costed from what it is made of.
        let unit = if recipes.get(&owned).is_some() {
            let inner = cost_walk(&owned, recipes, costs, names, depth + 1);
            out.unpriced.extend(inner.unpriced.iter().cloned());
            let batch = recipes.get(&owned).map_or(Qty::ONE, |r| r.batch_yield);
            UnitCost::from_batch(inner.total, batch).unwrap_or(UnitCost::ZERO)
        } else {
            match costs.get(&line.material) {
                Some(c) if !c.is_zero() => *c,
                _ => {
                    out.unpriced.push(line.material.clone());
                    UnitCost::ZERO
                }
            }
        };

        let cost = unit.cost_of(issued).unwrap_or(Money::ZERO);
        out.total = out.total.add(cost).unwrap_or(out.total);
        out.lines.push(CostedLine {
            material: line.material.clone(),
            name: names
                .get(&line.material)
                .cloned()
                .unwrap_or_else(|| line.material.to_string()),
            issued,
            cost,
        });
    }

    out.unpriced.sort_unstable();
    out.unpriced.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(id: &str) -> MaterialId {
        MaterialId::new(id)
    }

    fn grams(n: i64) -> Qty {
        Qty::from_whole(n).expect("in range")
    }

    fn line(id: &str, base: i64) -> RecipeLine {
        RecipeLine::new(material(id), grams(base), grams(base), "g")
    }

    fn facts(names: &[(&str, &str)], balances: &[(&str, i64)]) -> MaterialFacts {
        MaterialFacts {
            names: names
                .iter()
                .map(|(id, n)| (material(id), (*n).to_owned()))
                .collect(),
            balances: balances
                .iter()
                .map(|(id, q)| (material(id), grams(*q)))
                .collect(),
            base_units: names
                .iter()
                .map(|(id, _)| (material(id), "g".to_owned()))
                .collect(),
            retired: BTreeSet::new(),
        }
    }

    fn sold(key: &str, item: &str, name: &str, qty: i64) -> Sold {
        Sold {
            line_key: key.to_owned(),
            name: name.to_owned(),
            owner: RecipeOwner::Item(ItemId::new(item)),
            qty: grams(qty),
        }
    }

    fn drawn(out: &Explosion, id: &str) -> Qty {
        out.draws
            .iter()
            .filter(|d| d.material == material(id))
            .fold(Qty::ZERO, |acc, d| acc.add(d.base_qty).expect("in range"))
    }

    #[test]
    fn a_plain_dish_takes_what_its_recipe_says() {
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_rice")),
            vec![line("mat_rice", 180)],
        ));
        let facts = facts(&[("mat_rice", "Rice")], &[("mat_rice", 50_000)]);

        let out = explode(
            &[sold("l1", "itm_rice", "Rice plate", 40)],
            &recipes,
            &facts,
        );
        assert!(out.problems.is_empty(), "{:?}", out.problems);
        assert_eq!(drawn(&out, "mat_rice"), grams(-7_200), "40 plates × 180 g");
    }

    #[test]
    fn d110_yield_is_how_much_survives() {
        // 200 g of peeled onion at 90% means 222.222 g leaves the shelf.
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_curry")),
            vec![line("mat_onion", 200).with_yield(90)],
        ));
        let facts = facts(&[("mat_onion", "Onion")], &[("mat_onion", 10_000)]);
        let out = explode(&[sold("l1", "itm_curry", "Curry", 1)], &recipes, &facts);
        assert_eq!(drawn(&out, "mat_onion"), Qty::from_thousandths(-222_222));

        // And the alternative convention, which would have given 220 g.
        assert_ne!(drawn(&out, "mat_onion"), grams(-220));
    }

    #[test]
    fn t2_a_dish_made_of_a_gravy_made_of_materials() {
        // The gravy is a MADE MATERIAL: one batch makes 4 kg out of 3 kg of tomato and 1 kg of
        // onion.
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::batch(
            material("mat_gravy"),
            grams(4_000),
            vec![line("mat_tomato", 3_000), line("mat_onion", 1_000)],
        ));
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_curry")),
            vec![line("mat_gravy", 150)],
        ));
        let facts = facts(
            &[
                ("mat_gravy", "Gravy base"),
                ("mat_tomato", "Tomato"),
                ("mat_onion", "Onion"),
            ],
            &[("mat_tomato", 20_000), ("mat_onion", 10_000)],
        );

        let out = explode(&[sold("l1", "itm_curry", "Curry", 10)], &recipes, &facts);
        assert!(out.problems.is_empty(), "{:?}", out.problems);

        // Ten curries need 1,500 g of gravy, which is 0.375 of a batch.
        assert_eq!(out.productions.len(), 1);
        assert_eq!(out.productions[0].base_qty, grams(1_500));
        // 0.375 × 3 kg tomato and 0.375 × 1 kg onion.
        assert_eq!(drawn(&out, "mat_tomato"), grams(-1_125));
        assert_eq!(drawn(&out, "mat_onion"), grams(-375));
        // And the gravy itself moved, both ways.
        assert_eq!(drawn(&out, "mat_gravy"), grams(-1_500));
    }

    #[test]
    fn a_recorded_batch_is_drawn_down_before_anything_is_made() {
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::batch(
            material("mat_gravy"),
            grams(4_000),
            vec![line("mat_tomato", 3_000)],
        ));
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_curry")),
            vec![line("mat_gravy", 150)],
        ));
        let facts = facts(
            &[("mat_gravy", "Gravy base"), ("mat_tomato", "Tomato")],
            &[("mat_gravy", 1_000), ("mat_tomato", 20_000)],
        );

        let out = explode(&[sold("l1", "itm_curry", "Curry", 10)], &recipes, &facts);
        // 1,500 needed, 1,000 on the shelf, so 500 is made — not 1,500.
        assert_eq!(out.productions.len(), 1);
        assert_eq!(out.productions[0].base_qty, grams(500));
        assert_eq!(drawn(&out, "mat_tomato"), grams(-375), "0.125 of a batch");
    }

    #[test]
    fn t11_the_walk_recurses_and_stops_rather_than_hanging() {
        // Dish → gravy → paste → chilli.
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::batch(
            material("mat_paste"),
            grams(1_000),
            vec![line("mat_chilli", 500)],
        ));
        recipes.insert(Recipe::batch(
            material("mat_gravy"),
            grams(4_000),
            vec![line("mat_paste", 200)],
        ));
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_curry")),
            vec![line("mat_gravy", 400)],
        ));
        let facts = facts(
            &[
                ("mat_gravy", "Gravy"),
                ("mat_paste", "Paste"),
                ("mat_chilli", "Chilli"),
            ],
            &[("mat_chilli", 5_000)],
        );

        let out = explode(&[sold("l1", "itm_curry", "Curry", 10)], &recipes, &facts);
        assert!(out.problems.is_empty(), "{:?}", out.problems);
        assert_eq!(
            out.productions.len(),
            2,
            "gravy and paste both made themselves"
        );
        // 4,000 g of gravy is one batch, needing 200 g of paste, needing 100 g of chilli.
        assert_eq!(drawn(&out, "mat_chilli"), grams(-100));
    }

    #[test]
    fn t3_a_recipe_that_eats_itself_is_refused_by_name() {
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::batch(
            material("mat_a"),
            grams(1_000),
            vec![line("mat_b", 100)],
        ));
        recipes.insert(Recipe::batch(
            material("mat_b"),
            grams(1_000),
            vec![line("mat_c", 100)],
        ));

        let direct = Recipe::batch(material("mat_x"), grams(1_000), vec![line("mat_x", 1)]);
        assert_eq!(
            recipes.cycle_if_saved(&direct),
            Some(vec![material("mat_x"), material("mat_x")])
        );

        // Through a chain of three: c → a → b → c.
        let chained = Recipe::batch(material("mat_c"), grams(1_000), vec![line("mat_a", 100)]);
        assert_eq!(
            recipes.cycle_if_saved(&chained),
            Some(vec![
                material("mat_c"),
                material("mat_a"),
                material("mat_b"),
                material("mat_c")
            ])
        );

        // And a recipe that is merely deep is fine.
        let fine = Recipe::batch(material("mat_d"), grams(1_000), vec![line("mat_a", 100)]);
        assert_eq!(recipes.cycle_if_saved(&fine), None);

        // A dish cannot close a loop, because nothing may reference a dish.
        let dish = Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_x")),
            vec![line("mat_a", 1)],
        );
        assert_eq!(recipes.cycle_if_saved(&dish), None);
    }

    #[test]
    fn a_loop_that_slipped_past_the_save_check_stops_at_the_depth_limit() {
        // Built by hand, because `cycle_if_saved` would have refused it.
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::batch(
            material("mat_a"),
            grams(1_000),
            vec![line("mat_b", 1_000)],
        ));
        recipes.insert(Recipe::batch(
            material("mat_b"),
            grams(1_000),
            vec![line("mat_a", 1_000)],
        ));
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_x")),
            vec![line("mat_a", 100)],
        ));
        let facts = facts(&[("mat_a", "A"), ("mat_b", "B")], &[]);

        let out = explode(&[sold("l1", "itm_x", "X", 1)], &recipes, &facts);
        assert!(
            out.problems
                .iter()
                .any(|p| matches!(p, Problem::TooDeep { .. })),
            "{:?}",
            out.problems
        );
    }

    #[test]
    fn a_loop_whose_quantities_shrink_settles_by_itself() {
        // Worth writing down, because it looks like the test above and is not: if each trip
        // round the loop asks for a tenth as much, the series converges and the walk stops on
        // its own when the next ask rounds to nothing.
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::batch(
            material("mat_a"),
            grams(1_000),
            vec![line("mat_b", 100)],
        ));
        recipes.insert(Recipe::batch(
            material("mat_b"),
            grams(1_000),
            vec![line("mat_a", 100)],
        ));
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_x")),
            vec![line("mat_a", 100)],
        ));
        let facts = facts(&[("mat_a", "A"), ("mat_b", "B")], &[]);

        let out = explode(&[sold("l1", "itm_x", "X", 1)], &recipes, &facts);
        assert!(
            !out.problems
                .iter()
                .any(|p| matches!(p, Problem::TooDeep { .. })),
            "{:?}",
            out.problems
        );
    }

    #[test]
    fn t6_the_sale_completes_whatever_the_stock_book_says() {
        // A material that is not there any more.
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_a")),
            vec![line("mat_gone", 100)],
        ));
        let out = explode(&[sold("l1", "itm_a", "A", 1)], &recipes, &facts(&[], &[]));
        assert!(out.draws.is_empty());
        assert!(matches!(out.problems[0], Problem::UnknownMaterial { .. }));
        assert!(out.problems[0].sentence().contains("materials list"));

        // 1b. And the REACHABLE half: a material the shop retired.
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_a")),
            vec![line("mat_ghee", 50)],
        ));
        let mut retired = facts(&[("mat_ghee", "Ghee")], &[("mat_ghee", 1_000)]);
        retired.retired.insert(material("mat_ghee"));
        let out = explode(&[sold("l1", "itm_a", "A", 1)], &recipes, &retired);
        assert_eq!(
            drawn(&out, "mat_ghee"),
            grams(-50),
            "a retired material still moves"
        );
        assert!(matches!(out.problems[0], Problem::RetiredMaterial { .. }));

        // A balance that goes below zero.
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_b")),
            vec![line("mat_rice", 1_000)],
        ));
        let facts = facts(&[("mat_rice", "Rice")], &[("mat_rice", 500)]);
        let out = explode(&[sold("l1", "itm_b", "B", 1)], &recipes, &facts);
        assert_eq!(drawn(&out, "mat_rice"), grams(-1_000));
        assert!(matches!(out.problems[0], Problem::WentNegative { .. }));

        // An item with no recipe at all — the coverage case.
        let out = explode(
            &[sold("l1", "itm_nothing", "Cold drink", 3)],
            &Recipes::new(),
            &MaterialFacts::default(),
        );
        assert!(out.draws.is_empty());
        assert!(matches!(out.problems[0], Problem::NoRecipe { .. }));
        assert!(out.problems[0].sentence().contains("Cold drink"));

        // An ad-hoc line with no item id at all.
    }

    #[test]
    fn a_shop_is_told_about_a_missing_recipe_once_not_twelve_times() {
        let out = explode(
            &[
                sold("l1", "itm_x", "Tea", 1),
                sold("l2", "itm_x", "Tea", 1),
                sold("l3", "itm_x", "Tea", 1),
            ],
            &Recipes::new(),
            &MaterialFacts::default(),
        );
        assert_eq!(out.problems.len(), 1);
    }

    #[test]
    fn two_lines_of_the_same_dish_see_each_others_consumption() {
        // The second one is what goes negative, and it is the only one that says so.
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_a")),
            vec![line("mat_rice", 400)],
        ));
        let facts = facts(&[("mat_rice", "Rice")], &[("mat_rice", 500)]);
        let out = explode(
            &[sold("l1", "itm_a", "A", 1), sold("l2", "itm_a", "A", 1)],
            &recipes,
            &facts,
        );
        assert_eq!(drawn(&out, "mat_rice"), grams(-800));
        assert_eq!(
            out.draws.len(),
            2,
            "one row per line, so a void can find them"
        );
        assert_eq!(
            out.problems
                .iter()
                .filter(|p| matches!(p, Problem::WentNegative { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn t8_a_modifier_consumes_its_own_material() {
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_pizza_half")),
            vec![line("mat_dough", 150)],
        ));
        recipes.insert(Recipe::for_one(
            RecipeOwner::Modifier(ModifierId::new("mod_cheese")),
            vec![line("mat_cheese", 30)],
        ));
        let facts = facts(
            &[("mat_dough", "Dough"), ("mat_cheese", "Cheese")],
            &[("mat_dough", 10_000), ("mat_cheese", 5_000)],
        );

        // A half pizza is its own item id, so a variant recipe needs no special case at all;
        // the modifier rides beside it.
        let out = explode(
            &[
                sold("l1", "itm_pizza_half", "Pizza (Half)", 2),
                Sold {
                    line_key: "l1".to_owned(),
                    name: "Extra cheese".to_owned(),
                    owner: RecipeOwner::Modifier(ModifierId::new("mod_cheese")),
                    qty: grams(2),
                },
            ],
            &recipes,
            &facts,
        );
        assert!(out.problems.is_empty(), "{:?}", out.problems);
        assert_eq!(drawn(&out, "mat_dough"), grams(-300));
        assert_eq!(drawn(&out, "mat_cheese"), grams(-60));
    }

    #[test]
    fn a_zero_yield_is_a_problem_and_not_a_division() {
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_a")),
            vec![line("mat_x", 100).with_yield(0)],
        ));
        let facts = facts(&[("mat_x", "X")], &[("mat_x", 1_000)]);
        let out = explode(&[sold("l1", "itm_a", "A", 1)], &recipes, &facts);
        assert!(out.draws.is_empty());
        assert!(matches!(out.problems[0], Problem::ZeroYield { .. }));
    }

    #[test]
    fn food_cost_walks_the_tree_and_names_what_it_could_not_price() {
        // A curry: 150 g of gravy base (a batch of 4 kg from 3 kg tomato at ₹40/kg and 1 kg
        // onion at ₹30/kg) plus 5 g of a masala nobody priced.
        let mut recipes = Recipes::new();
        recipes.insert(Recipe::batch(
            material("mat_gravy"),
            grams(4_000),
            vec![line("mat_tomato", 3_000), line("mat_onion", 1_000)],
        ));
        recipes.insert(Recipe::for_one(
            RecipeOwner::Item(ItemId::new("itm_curry")),
            vec![line("mat_gravy", 150), line("mat_masala", 5)],
        ));

        let costs = BTreeMap::from([
            (
                material("mat_tomato"),
                UnitCost::from_paise_per_thousand(4_000),
            ),
            (
                material("mat_onion"),
                UnitCost::from_paise_per_thousand(3_000),
            ),
        ]);
        let names = BTreeMap::from([
            (material("mat_gravy"), "Gravy base".to_owned()),
            (material("mat_masala"), "Masala".to_owned()),
        ]);

        let costed = cost_of(
            &RecipeOwner::Item(ItemId::new("itm_curry")),
            &recipes,
            &costs,
            &names,
        );
        // A 4 kg batch costs 3 kg × ₹40 + 1 kg × ₹30 = ₹150, so gravy is 3.75 paise a gram and
        // 150 g of it is ₹5.625, which rounds to ₹5.63.
        assert_eq!(costed.total, Money::from_paise(563));
        // And the masala is NAMED rather than silently treated as free.
        assert_eq!(costed.unpriced, vec![material("mat_masala")]);
        assert_eq!(costed.lines.len(), 2);
        assert_eq!(costed.lines[0].name, "Gravy base");
    }

    #[test]
    fn a_dish_with_no_recipe_says_so_rather_than_costing_nothing() {
        // Zero and "unknown" are different answers, and a margin report that treats them as the
        // same shows 100% margin on every uncosted dish.
        let costed = cost_of(
            &RecipeOwner::Item(ItemId::new("itm_tea")),
            &Recipes::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert!(costed.no_recipe);
        assert_eq!(costed.total, Money::ZERO);
    }

    #[test]
    fn nothing_here_can_return_an_error() {
        let out: Explosion = explode(&[], &Recipes::new(), &MaterialFacts::default());
        assert!(out.is_empty());
    }
}
