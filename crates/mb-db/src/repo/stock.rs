//! **The stock book** — P25, scope 4.2, 4.3, 4.4, 4.6, 4.7 and 4.9.
//!
//! # The two sentences this file exists to obey
//!
//! **The till is sacred; the stock book is not.** [`StockRepo::deduct_for_bill`]
//! runs inside the settle transaction and cannot refuse a bill. The pure part
//! of it — `mb_core::recipe::explode` — has no error return at all (D112), for
//! the same reason `Feature` has no `Billing` variant (D86): a call that would
//! stop a cashier taking money must not typecheck.
//!
//! **A number nobody can trace is worse than no number.** Every quantity here
//! is the sum of `stock_movements` rows that each say who, when and why. That
//! is D67 (*the cash position is a QUERY*) and P15's credit ledger (*an account
//! is a SUM of rows*) applied a third time.
//!
//! # Why the deduction is INSIDE the settle transaction
//!
//! "It must never slow a sale" reads like an argument for doing it afterwards.
//! It is the opposite:
//!
//! * It is the **same commit and therefore the same single fsync** (D23, and
//!   `settle.rs`'s own opening paragraph). On the reference machine's 5400 rpm
//!   disk that fsync is the entire cost of a settle; twenty more INSERTs riding
//!   in it are microseconds of CPU. A background deduction would cost a
//!   **second** fsync and be slower.
//! * Idempotency is free: one commit, one `applied_events` row (D82).
//! * A crash between "bill settled" and "stock deducted" cannot exist.
//!
//! The thing that made "afterwards" look attractive — that a bug in deduction
//! could roll a bill back — is answered by the type, not by the placement.
//!
//! **A shop that has never opened this module pays one cached boolean.** Most
//! shops will never write a recipe and must not pay a query per settle;
//! [`StockRepo::has_any_recipe`] is that query and `src-tauri` caches its answer
//! exactly as D70 caches the day rule.

use std::collections::{BTreeMap, BTreeSet};

use mb_core::recipe::{Problem, Recipe, RecipeLine, RecipeOwner, Recipes};
use mb_core::{
    BusinessDay, Dimension, MaterialFacts, MaterialId, Money, OrderId, Qty, SettledOrder, Sold,
    StaffId, Timestamp, UnitCost, Units,
};
use rusqlite::{Transaction, params};

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

/// A raw material or a made material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Material {
    pub id: MaterialId,
    pub name: String,
    pub dimension: Dimension,
    pub category: String,
    /// **D116** — where you buy it. The buy list groups by this, and it stays
    /// the answer even after P26 brought suppliers: a shop with one owner and
    /// one scooter buys from "the vegetable market".
    pub buy_from: String,
    /// **P26.** Who you buy it from, when the shop has said. `None` is the
    /// everyday answer; setting it is what lets a purchase order be raised.
    pub supplier_id: Option<String>,
    pub reorder_level: Qty,
    pub reorder_qty: Qty,
    /// **D117** — a property that warns, not batch tracking.
    pub is_perishable: bool,
    pub shelf_life_days: Option<u32>,
    /// Scope 4.10, DESIGN.
    pub location: String,
    /// **D118** — a weighted average of what actually came in.
    pub avg_cost: UnitCost,
    pub cost_changed_at: Option<Timestamp>,
    /// **D115** — `None` means *never counted*, and the variance report says so.
    pub last_counted_at: Option<Timestamp>,
    pub is_active: bool,
    pub sort_order: i64,
    /// The shop's own packs. The standard units are not stored (D108).
    pub packs: Vec<(String, Qty)>,
    /// Which pack the buy list defaults to.
    pub purchase_unit: Option<String>,
    /// Which pack the recipe screen defaults to. Rice is BOUGHT in bags and
    /// COOKED in grams, so these are two answers and not one.
    pub recipe_unit: Option<String>,
}

impl Material {
    /// The everyday material: a weight, bought and cooked in its base unit.
    pub fn new(id: MaterialId, name: impl Into<String>, dimension: Dimension) -> Self {
        Material {
            id,
            name: name.into(),
            dimension,
            category: String::new(),
            buy_from: String::new(),
            supplier_id: None,
            reorder_level: Qty::ZERO,
            reorder_qty: Qty::ZERO,
            is_perishable: false,
            shelf_life_days: None,
            location: DEFAULT_LOCATION.to_owned(),
            avg_cost: UnitCost::ZERO,
            cost_changed_at: None,
            last_counted_at: None,
            is_active: true,
            sort_order: 0,
            packs: Vec::new(),
            purchase_unit: None,
            recipe_unit: None,
        }
    }

    /// Every unit this material may be spoken about in: the dimension's
    /// standards plus the shop's own packs (D108).
    ///
    /// A pack the shop has defined twice, or defined as nothing, is **dropped
    /// rather than raised**: this is called to draw a screen and to convert a
    /// row that is already on disk, and a units list that can fail is a stock
    /// page that goes blank because somebody typed a zero into a pack once.
    #[must_use]
    pub fn units(&self) -> Units {
        let mut units = Units::standard(self.dimension);
        for (name, base_per_unit) in &self.packs {
            units = units.clone().with_pack(name.clone(), *base_per_unit).unwrap_or(units);
        }
        units
    }

    /// The unit a recipe line should default to.
    #[must_use]
    pub fn default_recipe_unit(&self) -> String {
        self.recipe_unit.clone().unwrap_or_else(|| self.dimension.base_unit().to_owned())
    }

    /// The unit the buy list should count in.
    #[must_use]
    pub fn default_purchase_unit(&self) -> String {
        self.purchase_unit.clone().unwrap_or_else(|| self.dimension.base_unit().to_owned())
    }
}

/// Where in the shop something is kept. Scope 4.10 is DESIGN, so there is one.
pub const DEFAULT_LOCATION: &str = "Store";

/// A material and what is on the shelf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnHand {
    pub material: Material,
    /// **Signed.** A stock balance is the one quantity in this product allowed
    /// to go below zero — a shop that sold food it never recorded buying —
    /// because refusing would have refused the sale.
    pub base_qty: Qty,
    pub last_movement_at: Option<Timestamp>,
}

impl OnHand {
    /// What this holding is worth, at the material's current cost.
    #[must_use]
    pub fn value(&self) -> Money {
        self.material.avg_cost.cost_of(self.base_qty).unwrap_or(Money::ZERO)
    }

    /// Scope 4.6 — is this on the buy list?
    #[must_use]
    pub fn is_low(&self) -> bool {
        self.material.reorder_level.is_positive() && self.base_qty <= self.material.reorder_level
    }
}

/// Why something moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementKind {
    /// Somebody said what was on the shelf on day one.
    Opening,
    /// **P26 writes these.** The kind is defined here so that P26 adds no
    /// column to `stock_movements`.
    Purchase,
    /// A bill took it (D112).
    Sale,
    /// A void put it back — D113, by negating a row.
    Reversal,
    /// Scope 4.7, the report that catches theft.
    Wastage,
    /// A person changed the figure, with a reason.
    Adjustment,
    /// A made material came into existence (D111).
    ProductionIn,
    /// An input consumed by making one.
    ProductionOut,
    /// Scope 4.10, DESIGN.
    TransferIn,
    /// Scope 4.10, DESIGN.
    TransferOut,
}

impl MovementKind {
    pub const ALL: &'static [MovementKind] = &[
        MovementKind::Opening,
        MovementKind::Purchase,
        MovementKind::Sale,
        MovementKind::Reversal,
        MovementKind::Wastage,
        MovementKind::Adjustment,
        MovementKind::ProductionIn,
        MovementKind::ProductionOut,
        MovementKind::TransferIn,
        MovementKind::TransferOut,
    ];

    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            MovementKind::Opening => "opening",
            MovementKind::Purchase => "purchase",
            MovementKind::Sale => "sale",
            MovementKind::Reversal => "reversal",
            MovementKind::Wastage => "wastage",
            MovementKind::Adjustment => "adjustment",
            MovementKind::ProductionIn => "production_in",
            MovementKind::ProductionOut => "production_out",
            MovementKind::TransferIn => "transfer_in",
            MovementKind::TransferOut => "transfer_out",
        }
    }

    /// What a person reads on a movements list.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            MovementKind::Opening => "Opening stock",
            MovementKind::Purchase => "Bought",
            MovementKind::Sale => "Sold",
            MovementKind::Reversal => "Put back after a void",
            MovementKind::Wastage => "Wasted",
            MovementKind::Adjustment => "Adjusted",
            MovementKind::ProductionIn => "Made",
            MovementKind::ProductionOut => "Used to make something",
            MovementKind::TransferIn => "Received",
            MovementKind::TransferOut => "Sent out",
        }
    }

    pub fn from_tag(tag: &str) -> Result<Self, DbError> {
        MovementKind::ALL.iter().copied().find(|k| k.tag() == tag).ok_or_else(|| {
            DbError::invariant(format!("stock_movements.kind holds an unknown value `{tag}`"))
        })
    }

    /// **Does this kind set the material's average cost?** D118.
    ///
    /// A reversal deliberately does not: putting back the goods that left is
    /// the same goods returning, and re-blending them at today's price would
    /// move an average for a transaction that added nothing to the shelf.
    #[must_use]
    pub const fn revalues(self) -> bool {
        matches!(
            self,
            MovementKind::Opening
                | MovementKind::Purchase
                | MovementKind::Adjustment
                | MovementKind::ProductionIn
                | MovementKind::TransferIn
        )
    }
}

/// One row on its way into the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Movement {
    pub id: String,
    pub material: MaterialId,
    pub kind: MovementKind,
    /// **Signed** base units. Out is negative, in is positive.
    pub base_qty: Qty,
    /// **D109** — what the person typed, and in which unit. The label half.
    pub typed_qty: Qty,
    pub typed_unit: String,
    /// `None` means *"whatever this material currently costs"*, which is the
    /// right answer for everything leaving the shelf and is resolved here so
    /// that no caller can forget to value a wastage row.
    pub unit_cost: Option<UnitCost>,
    pub at: Timestamp,
    pub business_day: BusinessDay,
    pub staff: Option<StaffId>,
    pub order_id: Option<OrderId>,
    pub order_line_id: Option<String>,
    pub reason_id: Option<String>,
    pub note: Option<String>,
    /// **D113** — the row this puts back.
    pub reverses_id: Option<String>,
    pub produced_for: Option<MaterialId>,
    /// **D111** — a sale needed a made material nobody had recorded making.
    pub was_automatic: bool,
    pub location: String,
}

impl Movement {
    /// The everyday movement: this much of this material, right now.
    pub fn new(
        id: impl Into<String>,
        material: MaterialId,
        kind: MovementKind,
        base_qty: Qty,
        at: Timestamp,
        business_day: BusinessDay,
    ) -> Self {
        Movement {
            id: id.into(),
            material,
            kind,
            base_qty,
            typed_qty: base_qty,
            typed_unit: String::new(),
            unit_cost: None,
            at,
            business_day,
            staff: None,
            order_id: None,
            order_line_id: None,
            reason_id: None,
            note: None,
            reverses_id: None,
            produced_for: None,
            was_automatic: false,
            location: DEFAULT_LOCATION.to_owned(),
        }
    }

    /// **D109** — record what the person actually typed beside the truth.
    #[must_use]
    pub fn typed(mut self, qty: Qty, unit: impl Into<String>) -> Self {
        self.typed_qty = qty;
        self.typed_unit = unit.into();
        self
    }

    #[must_use]
    pub fn costing(mut self, unit_cost: UnitCost) -> Self {
        self.unit_cost = Some(unit_cost);
        self
    }

    #[must_use]
    pub fn by(mut self, staff: StaffId) -> Self {
        self.staff = Some(staff);
        self
    }
}

/// A ledger row read back, with the words a screen needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovementRow {
    pub id: String,
    pub material: MaterialId,
    pub material_name: String,
    pub kind: MovementKind,
    pub base_qty: Qty,
    pub typed_qty: Qty,
    pub typed_unit: String,
    pub unit_cost: UnitCost,
    pub total_cost: Money,
    pub business_day: BusinessDay,
    pub at: Timestamp,
    pub staff: Option<StaffId>,
    pub order_id: Option<OrderId>,
    pub reason: Option<String>,
    pub note: Option<String>,
    pub was_automatic: bool,
    /// Which made material a `production_out` fed, by name. It is what lets the
    /// ledger read "used to make Gravy base" instead of leaving somebody to
    /// work out why the tomato moved.
    pub produced_for: Option<String>,
}

/// Something the stock book could not do, which did not stop the sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemRow {
    pub id: String,
    pub kind: String,
    pub subject: String,
    /// **A whole sentence an owner can act on** — D100.
    pub sentence: String,
    pub occurrences: i64,
    pub first_at: Timestamp,
    pub last_at: Timestamp,
    pub last_order_id: Option<OrderId>,
}

/// What a period consumed, theoretical against actual — scope 4.9, D115.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumptionRow {
    pub material: MaterialId,
    pub name: String,
    /// What the recipes say the sales should have used.
    pub theoretical: Qty,
    /// What the ledger says actually left, wastage and adjustments included.
    pub actual: Qty,
    /// `actual - theoretical`. Positive means more went than should have.
    pub variance: Qty,
    pub variance_value: Money,
    /// **D115** — `None` means nobody has ever counted this, and the screen
    /// says so instead of showing a confident 0.0%.
    pub last_counted_at: Option<Timestamp>,
}

impl ConsumptionRow {
    /// The variance as a percentage of what should have been used, in basis
    /// points so there is no float. `None` when nothing should have been used,
    /// because a percentage of nothing is not a number.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "a percentage in basis points IS a division; the guard above is \
                  the only case that could lose anything"
    )]
    pub fn variance_bp(&self) -> Option<i64> {
        if self.theoretical.thousandths() == 0 {
            return None;
        }
        let scaled = i128::from(self.variance.thousandths()) * 10_000;
        i64::try_from(scaled / i128::from(self.theoretical.thousandths())).ok()
    }
}

#[derive(Debug)]
pub struct StockRepo<'a> {
    tx: &'a Transaction<'a>,
}

const MATERIAL_COLUMNS: &str = "id, name, dimension, category, buy_from, reorder_level, \
     reorder_qty, is_perishable, shelf_life_days, location, avg_cost, cost_changed_at, \
     last_counted_at, is_active, sort_order, supplier_id";

impl<'a> StockRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        StockRepo { tx }
    }

    // =======================================================================
    // MATERIALS — scope 4.2
    // =======================================================================

    /// Add or change a material and its packs.
    pub fn save_material(
        &self,
        outlet: &str,
        material: &Material,
        at: Timestamp,
    ) -> Result<(), DbError> {
        if material.name.trim().is_empty() {
            return Err(DbError::invariant("a material needs a name"));
        }
        self.tx.execute(
            "INSERT INTO materials
                 (id, outlet_id, name, dimension, category, buy_from, reorder_level, reorder_qty,
                  is_perishable, shelf_life_days, location, avg_cost, cost_changed_at,
                  last_counted_at, is_active, sort_order, created_at, supplier_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 dimension = excluded.dimension,
                 category = excluded.category,
                 buy_from = excluded.buy_from,
                 supplier_id = excluded.supplier_id,
                 reorder_level = excluded.reorder_level,
                 reorder_qty = excluded.reorder_qty,
                 is_perishable = excluded.is_perishable,
                 shelf_life_days = excluded.shelf_life_days,
                 location = excluded.location,
                 is_active = excluded.is_active,
                 sort_order = excluded.sort_order",
            params![
                material.id.as_str(),
                outlet,
                material.name.trim(),
                material.dimension.tag(),
                material.category,
                material.buy_from,
                encode::qty_to_sql(material.reorder_level),
                encode::qty_to_sql(material.reorder_qty),
                encode::bool_to_sql(material.is_perishable),
                material.shelf_life_days,
                material.location,
                material.avg_cost.paise_per_thousand(),
                material.cost_changed_at.map(encode::timestamp_to_sql),
                material.last_counted_at.map(encode::timestamp_to_sql),
                encode::bool_to_sql(material.is_active),
                material.sort_order,
                encode::timestamp_to_sql(at),
                material.supplier_id,
            ],
        )?;

        // **The average cost is NOT written here on an update.** It is the
        // ledger's answer (D118), and a screen that could type over it would be
        // the "current price from 2019" this decision exists to prevent.

        // The packs are replaced wholesale: they are a small list edited as one
        // thing, exactly like a modifier group's choices.
        self.tx.execute("DELETE FROM material_units WHERE material_id = ?1", [material.id.as_str()])?;
        for (seq, (name, base_per_unit)) in material.packs.iter().enumerate() {
            if name.trim().is_empty() || !base_per_unit.is_positive() {
                return Err(DbError::invariant(format!(
                    "the pack `{name}` needs a name and a size bigger than nothing"
                )));
            }
            self.tx.execute(
                "INSERT INTO material_units
                     (material_id, name, base_per_unit, is_purchase_default, is_recipe_default, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    material.id.as_str(),
                    name.trim(),
                    encode::qty_to_sql(*base_per_unit),
                    encode::bool_to_sql(material.purchase_unit.as_deref() == Some(name.trim())),
                    encode::bool_to_sql(material.recipe_unit.as_deref() == Some(name.trim())),
                    i64::try_from(seq).unwrap_or(0),
                ],
            )?;
        }

        OutboxRepo::new(self.tx).enqueue(outlet, "materials", material.id.as_str(), Op::Upsert, at)
    }

    /// Every material, in the order a screen shows them.
    pub fn materials(&self, outlet: &str, include_retired: bool) -> Result<Vec<Material>, DbError> {
        let sql = format!(
            "SELECT {MATERIAL_COLUMNS} FROM materials
              WHERE outlet_id = ?1 AND (?2 = 1 OR is_active = 1)
              ORDER BY sort_order, name"
        );
        let mut stmt = self.tx.prepare(&sql)?;
        let rows = stmt.query_map(params![outlet, i64::from(include_retired)], read_material)?;
        let mut out: Vec<Material> = rows.collect::<Result<_, _>>()?;

        // One query for every pack, rather than one per material: a shop with
        // 300 materials would otherwise open its stock screen with 301 round
        // trips, which is R4's mistake in a different module.
        let mut packs = self.tx.prepare(
            "SELECT material_id, name, base_per_unit, is_purchase_default, is_recipe_default
               FROM material_units ORDER BY material_id, sort_order",
        )?;
        let mut by_material: BTreeMap<String, Vec<(String, Qty, bool, bool)>> = BTreeMap::new();
        let mut cursor = packs.query([])?;
        while let Some(row) = cursor.next()? {
            let material: String = row.get(0)?;
            by_material.entry(material).or_default().push((
                row.get(1)?,
                encode::qty_from_sql(row.get(2)?),
                encode::bool_from_sql(row.get(3)?, "material_units.is_purchase_default")?,
                encode::bool_from_sql(row.get(4)?, "material_units.is_recipe_default")?,
            ));
        }
        for material in &mut out {
            if let Some(found) = by_material.remove(material.id.as_str()) {
                for (name, base, is_purchase, is_recipe) in found {
                    if is_purchase {
                        material.purchase_unit = Some(name.clone());
                    }
                    if is_recipe {
                        material.recipe_unit = Some(name.clone());
                    }
                    material.packs.push((name, base));
                }
            }
        }
        Ok(out)
    }

    pub fn material(&self, outlet: &str, id: &MaterialId) -> Result<Option<Material>, DbError> {
        Ok(self.materials(outlet, true)?.into_iter().find(|m| &m.id == id))
    }

    /// Every material with what is on the shelf beside it.
    pub fn on_hand(&self, outlet: &str, include_retired: bool) -> Result<Vec<OnHand>, DbError> {
        let materials = self.materials(outlet, include_retired)?;
        let balances = self.balances(outlet)?;
        Ok(materials
            .into_iter()
            .map(|material| {
                let (base_qty, last_movement_at) =
                    balances.get(&material.id).copied().unwrap_or((Qty::ZERO, None));
                OnHand { material, base_qty, last_movement_at }
            })
            .collect())
    }

    /// The balance cache (D114), as a map.
    pub fn balances(
        &self,
        outlet: &str,
    ) -> Result<BTreeMap<MaterialId, (Qty, Option<Timestamp>)>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT material_id, base_qty, last_movement_at FROM material_balances
              WHERE outlet_id = ?1",
        )?;
        let mut cursor = stmt.query([outlet])?;
        let mut out = BTreeMap::new();
        while let Some(row) = cursor.next()? {
            let id: String = row.get(0)?;
            let at: Option<i64> = row.get(2)?;
            out.insert(
                MaterialId::new(id),
                (encode::qty_from_sql(row.get(1)?), at.map(encode::timestamp_from_sql)),
            );
        }
        Ok(out)
    }

    // =======================================================================
    // RECIPES — scope 4.3
    // =======================================================================

    /// Save a recipe, **refusing a loop by name** (D111).
    pub fn save_recipe(
        &self,
        outlet: &str,
        recipe: &Recipe,
        at: Timestamp,
    ) -> Result<(), DbError> {
        if recipe.lines.is_empty() {
            return Err(DbError::invariant(
                "a recipe with nothing in it would take nothing off the shelf. \
                 Add at least one material, or delete the recipe.",
            ));
        }
        // Checked at SAVE time, in words that name the loop, because the
        // alternative is discovering it at 8 pm inside a settle. The candidate
        // is checked against every OTHER recipe, so re-saving one that is
        // already stored does not accuse itself.
        let mut others = self.recipes(outlet)?;
        others.remove(&recipe.owner);
        if let Some(chain) = others.cycle_if_saved(recipe) {
            let names = self.material_names(outlet)?;
            let readable: Vec<String> = chain
                .iter()
                .map(|m| names.get(m).cloned().unwrap_or_else(|| m.to_string()))
                .collect();
            return Err(DbError::invariant(format!(
                "this recipe goes round in circles: {}. Take one of those out.",
                readable.join(" → ")
            )));
        }

        let id = recipe_id(outlet, &recipe.owner);
        let (item, modifier, material) = owner_columns(&recipe.owner);
        self.tx.execute(
            "INSERT INTO recipes
                 (id, outlet_id, owner_kind, item_id, modifier_id, material_id,
                  batch_yield, notes, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '', ?8)
             ON CONFLICT (id) DO UPDATE SET
                 batch_yield = excluded.batch_yield,
                 updated_at = excluded.updated_at",
            params![
                id,
                outlet,
                recipe.owner.tag(),
                item,
                modifier,
                material,
                encode::qty_to_sql(recipe.batch_yield),
                encode::timestamp_to_sql(at),
            ],
        )?;

        self.tx.execute("DELETE FROM recipe_lines WHERE recipe_id = ?1", [&id])?;
        for (seq, line) in recipe.lines.iter().enumerate() {
            if !line.base_qty.is_positive() {
                return Err(DbError::invariant(
                    "a recipe line has to use more than nothing. Type an amount, or remove the line.",
                ));
            }
            self.tx.execute(
                "INSERT INTO recipe_lines
                     (recipe_id, seq, material_id, base_qty, yield_percent, typed_qty, typed_unit)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    i64::try_from(seq).unwrap_or(0),
                    line.material.as_str(),
                    encode::qty_to_sql(line.base_qty),
                    i64::from(line.yield_percent),
                    encode::qty_to_sql(line.typed_qty),
                    line.typed_unit,
                ],
            )?;
        }
        OutboxRepo::new(self.tx).enqueue(outlet, "recipes", &id, Op::Upsert, at)
    }

    /// Take a recipe away. The dish stays on the menu and simply stops
    /// deducting, which is what a shop wants when it has stopped costing one.
    pub fn delete_recipe(&self, outlet: &str, owner: &RecipeOwner) -> Result<(), DbError> {
        let id = recipe_id(outlet, owner);
        self.tx.execute("DELETE FROM recipe_lines WHERE recipe_id = ?1", [&id])?;
        self.tx.execute("DELETE FROM recipes WHERE id = ?1", [&id])?;
        Ok(())
    }

    /// Every recipe the shop has.
    ///
    /// Loaded whole, because there are tens of them and not thousands, and
    /// because `mb_core::recipe::explode` is pure — handing it a map is what
    /// keeps a database query out of the settle transaction's inner loop.
    pub fn recipes(&self, outlet: &str) -> Result<Recipes, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT r.id, r.owner_kind, r.item_id, r.modifier_id, r.material_id, r.batch_yield,
                    l.material_id, l.base_qty, l.yield_percent, l.typed_qty, l.typed_unit
               FROM recipes r
               LEFT JOIN recipe_lines l ON l.recipe_id = r.id
              WHERE r.outlet_id = ?1
              ORDER BY r.id, l.seq",
        )?;
        let mut cursor = stmt.query([outlet])?;
        let mut out = Recipes::new();
        let mut current: Option<Recipe> = None;
        let mut current_id = String::new();

        while let Some(row) = cursor.next()? {
            let id: String = row.get(0)?;
            if id != current_id {
                if let Some(done) = current.take() {
                    out.insert(done);
                }
                current_id = id;
                current = Some(Recipe {
                    owner: owner_from_row(
                        &row.get::<_, String>(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    )?,
                    batch_yield: encode::qty_from_sql(row.get(5)?),
                    lines: Vec::new(),
                });
            }
            let material: Option<String> = row.get(6)?;
            if let (Some(material), Some(recipe)) = (material, current.as_mut()) {
                let percent: i64 = row.get(8)?;
                recipe.lines.push(RecipeLine {
                    material: MaterialId::new(material),
                    base_qty: encode::qty_from_sql(row.get(7)?),
                    yield_percent: u32::try_from(percent).unwrap_or(100),
                    typed_qty: encode::qty_from_sql(row.get(9)?),
                    typed_unit: row.get(10)?,
                });
            }
        }
        if let Some(done) = current.take() {
            out.insert(done);
        }
        Ok(out)
    }

    /// **The cheap gate.** A shop with no recipes must not pay a query per
    /// settle, so `src-tauri` caches this answer the way D70 caches the day
    /// rule and refreshes it when a recipe is saved.
    pub fn has_any_recipe(&self, outlet: &str) -> Result<bool, DbError> {
        let count: i64 = self.tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM recipes WHERE outlet_id = ?1)",
            [outlet],
            |row| row.get(0),
        )?;
        Ok(count == 1)
    }

    /// Which recipes use this material — asked before retiring one.
    pub fn where_used(
        &self,
        outlet: &str,
        material: &MaterialId,
    ) -> Result<Vec<RecipeOwner>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT r.owner_kind, r.item_id, r.modifier_id, r.material_id
               FROM recipe_lines l JOIN recipes r ON r.id = l.recipe_id
              WHERE r.outlet_id = ?1 AND l.material_id = ?2
              ORDER BY r.id",
        )?;
        let mut cursor = stmt.query(params![outlet, material.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = cursor.next()? {
            out.push(owner_from_row(
                &row.get::<_, String>(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
            )?);
        }
        Ok(out)
    }

    // =======================================================================
    // THE LEDGER
    // =======================================================================

    /// **Write one movement, and keep the balance cache and the average cost in
    /// step — in this transaction.**
    ///
    /// Idempotent by id, like every other write in this product since D82: a
    /// retried settle must move stock once.
    pub fn record(&self, outlet: &str, movement: &Movement) -> Result<(), DbError> {
        if movement.base_qty.is_zero() {
            return Err(DbError::invariant("a movement of nothing is not a movement"));
        }

        // **The cost of what leaves is what the shelf holds.** Resolved here
        // rather than at the call site, so no caller can forget to value a
        // wastage row and produce a theft report reading ₹0.
        let unit_cost = match movement.unit_cost {
            Some(given) => given,
            None => self.avg_cost(outlet, &movement.material)?,
        };
        let total_cost = unit_cost.cost_of(movement.base_qty).unwrap_or(Money::ZERO);

        let inserted = self.tx.execute(
            "INSERT INTO stock_movements
                 (id, outlet_id, material_id, kind, base_qty, typed_qty, typed_unit,
                  unit_cost, total_cost, business_day, at, staff_id, order_id, order_line_id,
                  reason_id, note, reverses_id, produced_for, was_automatic,
                  transfer_id, counterpart_outlet_id, location)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, ?19, NULL, NULL, ?20)
             ON CONFLICT (id) DO NOTHING",
            params![
                movement.id,
                outlet,
                movement.material.as_str(),
                movement.kind.tag(),
                encode::qty_to_sql(movement.base_qty),
                encode::qty_to_sql(movement.typed_qty),
                movement.typed_unit,
                unit_cost.paise_per_thousand(),
                encode::money_to_sql(total_cost),
                encode::business_day_to_sql(movement.business_day),
                encode::timestamp_to_sql(movement.at),
                movement.staff.as_ref().map(StaffId::as_str),
                movement.order_id.as_ref().map(OrderId::as_str),
                movement.order_line_id,
                movement.reason_id,
                movement.note,
                movement.reverses_id,
                movement.produced_for.as_ref().map(MaterialId::as_str),
                encode::bool_to_sql(movement.was_automatic),
                movement.location,
            ],
        )?;
        if inserted == 0 {
            // The same movement, twice. D82: applied once.
            return Ok(());
        }

        // **D118 — blend before the balance moves**, because the weight is the
        // holding as it was BEFORE this delivery arrived.
        if movement.kind.revalues() && movement.base_qty.is_positive() {
            let holding = self.balance(outlet, &movement.material)?;
            let blended = self
                .avg_cost(outlet, &movement.material)?
                .blend(holding, movement.base_qty, unit_cost)
                .map_err(|e| DbError::invariant(format!("that cost cannot be averaged: {e}")))?;
            self.tx.execute(
                "UPDATE materials SET avg_cost = ?2, cost_changed_at = ?3 WHERE id = ?1",
                params![
                    movement.material.as_str(),
                    blended.paise_per_thousand(),
                    encode::timestamp_to_sql(movement.at),
                ],
            )?;
        }

        self.tx.execute(
            "INSERT INTO material_balances (outlet_id, material_id, base_qty, last_movement_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (outlet_id, material_id) DO UPDATE SET
                 base_qty = base_qty + excluded.base_qty,
                 last_movement_at = excluded.last_movement_at",
            params![
                outlet,
                movement.material.as_str(),
                encode::qty_to_sql(movement.base_qty),
                encode::timestamp_to_sql(movement.at),
            ],
        )?;
        Ok(())
    }

    pub fn balance(&self, outlet: &str, material: &MaterialId) -> Result<Qty, DbError> {
        let thousandths: i64 = self
            .tx
            .query_row(
                "SELECT base_qty FROM material_balances WHERE outlet_id = ?1 AND material_id = ?2",
                params![outlet, material.as_str()],
                |row| row.get(0),
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(0),
                other => Err(other),
            })?;
        Ok(encode::qty_from_sql(thousandths))
    }

    fn avg_cost(&self, outlet: &str, material: &MaterialId) -> Result<UnitCost, DbError> {
        let paise: i64 = self
            .tx
            .query_row(
                "SELECT avg_cost FROM materials WHERE outlet_id = ?1 AND id = ?2",
                params![outlet, material.as_str()],
                |row| row.get(0),
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(0),
                other => Err(other),
            })?;
        Ok(UnitCost::from_paise_per_thousand(paise))
    }

    /// **D114 — rebuild the cache from the ledger, which is the truth.**
    ///
    /// Returns how many materials it corrected. A cache nobody verifies is a
    /// stored balance with extra words, so this is a visible action and Health
    /// runs the same comparison.
    pub fn rebuild_balances(&self, outlet: &str, at: Timestamp) -> Result<usize, DbError> {
        let before = self.balances(outlet)?;
        self.tx.execute("DELETE FROM material_balances WHERE outlet_id = ?1", [outlet])?;
        self.tx.execute(
            "INSERT INTO material_balances (outlet_id, material_id, base_qty, last_movement_at)
             SELECT outlet_id, material_id, SUM(base_qty), MAX(at)
               FROM stock_movements WHERE outlet_id = ?1
              GROUP BY outlet_id, material_id",
            [outlet],
        )?;
        let _ = at;
        let after = self.balances(outlet)?;
        let mut corrected = 0;
        for (material, (qty, _)) in &after {
            if before.get(material).map(|(q, _)| *q) != Some(*qty) {
                corrected += 1;
            }
        }
        // A material that had a cached row and now has none is also a
        // correction, and it is the one a naive comparison misses.
        for material in before.keys() {
            if !after.contains_key(material) {
                corrected += 1;
            }
        }
        Ok(corrected)
    }

    /// **D114's verification** — which materials' cached balances disagree with
    /// the sum of their movements.
    ///
    /// Run every time the stock screen opens. Summing the ledger for a handful
    /// of materials is cheap; being quietly wrong about stock for a year is
    /// not, and a cache nobody verifies is a stored balance with extra words.
    pub fn drifted(&self, outlet: &str) -> Result<Vec<MaterialId>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT b.material_id
               FROM material_balances b
               LEFT JOIN (SELECT material_id, SUM(base_qty) AS total FROM stock_movements
                           WHERE outlet_id = ?1 GROUP BY material_id) m
                 ON m.material_id = b.material_id
              WHERE b.outlet_id = ?1 AND b.base_qty <> COALESCE(m.total, 0)",
        )?;
        let mut cursor = stmt.query([outlet])?;
        let mut out = Vec::new();
        while let Some(row) = cursor.next()? {
            out.push(MaterialId::new(row.get::<_, String>(0)?));
        }
        Ok(out)
    }

    /// Names, balances and what has been retired, for
    /// `mb_core::recipe::explode`.
    pub fn facts(&self, outlet: &str) -> Result<MaterialFacts, DbError> {
        let mut names = BTreeMap::new();
        let mut retired = BTreeSet::new();
        let mut base_units = BTreeMap::new();
        let mut stmt = self
            .tx
            .prepare("SELECT id, name, is_active, dimension FROM materials WHERE outlet_id = ?1")?;
        let mut cursor = stmt.query([outlet])?;
        while let Some(row) = cursor.next()? {
            let id = MaterialId::new(row.get::<_, String>(0)?);
            if row.get::<_, i64>(2)? == 0 {
                retired.insert(id.clone());
            }
            // So a shortfall reads "120 g" rather than "120".
            if let Some(dimension) = Dimension::from_tag(&row.get::<_, String>(3)?) {
                base_units.insert(id.clone(), dimension.base_unit().to_owned());
            }
            names.insert(id, row.get(1)?);
        }
        Ok(MaterialFacts {
            names,
            balances: self
                .balances(outlet)?
                .into_iter()
                .map(|(id, (qty, _))| (id, qty))
                .collect(),
            base_units,
            retired,
        })
    }

    fn material_names(&self, outlet: &str) -> Result<BTreeMap<MaterialId, String>, DbError> {
        let mut stmt =
            self.tx.prepare("SELECT id, name FROM materials WHERE outlet_id = ?1")?;
        let mut cursor = stmt.query([outlet])?;
        let mut out = BTreeMap::new();
        while let Some(row) = cursor.next()? {
            let id: String = row.get(0)?;
            out.insert(MaterialId::new(id), row.get(1)?);
        }
        Ok(out)
    }

    /// What each material currently costs, for the food-cost walk.
    pub fn costs(&self, outlet: &str) -> Result<BTreeMap<MaterialId, UnitCost>, DbError> {
        let mut stmt =
            self.tx.prepare("SELECT id, avg_cost FROM materials WHERE outlet_id = ?1")?;
        let mut cursor = stmt.query([outlet])?;
        let mut out = BTreeMap::new();
        while let Some(row) = cursor.next()? {
            let id: String = row.get(0)?;
            out.insert(MaterialId::new(id), UnitCost::from_paise_per_thousand(row.get(1)?));
        }
        Ok(out)
    }

    // =======================================================================
    // THE SALE PATH — D112 and D113
    // =======================================================================

    /// **Deduct what a settled bill used. This cannot refuse the bill.**
    ///
    /// It returns `Result` only because writing a row can fail on a disk that
    /// is full — the same failure that would have rolled the bill back anyway.
    /// **No business problem reaches this return value**: a missing material, a
    /// negative shelf, a deleted recipe, a loop, an ad-hoc line with no item id
    /// — every one of them becomes a `stock_problems` row and the bill settles.
    /// Read D112 before changing that.
    pub fn deduct_for_bill(
        &self,
        outlet: &str,
        order: &SettledOrder,
        at: Timestamp,
    ) -> Result<(), DbError> {
        // **The cheap gate**, first, before anything else touches the disk.
        if !self.has_any_recipe(outlet)? {
            return Ok(());
        }
        // **D82** — the id, the effect and the outcome in one transaction. A
        // retried settle deducts once.
        let event = format!("stock:{}", order.core.id);
        let claimed = self.tx.execute(
            "INSERT INTO applied_events (event_id, outlet_id, applied_at, source, result)
             VALUES (?1, ?2, ?3, 'stock', 'deducted')
             ON CONFLICT (event_id) DO NOTHING",
            params![event, outlet, encode::timestamp_to_sql(at)],
        )?;
        if claimed == 0 {
            return Ok(());
        }

        let recipes = self.recipes(outlet)?;
        let facts = self.facts(outlet)?;
        let sold = sold_from(order);
        let explosion = mb_core::explode(&sold, &recipes, &facts);

        let day = order.core.business_day;
        let by = order.settled_by.clone();

        // **The rows go in the order the kitchen did them**, and it matters for
        // more than tidiness. `record` values anything leaving the shelf at the
        // material's CURRENT average cost, so a gravy sold before the batch
        // that supplied it had been written was valued at nothing — the first
        // curry of a shop's life recorded "sold 150 g of gravy, worth ₹0.00".
        // Found by settling thirty bills and reading the ledger back with
        // `peek`; every test passed throughout.
        //
        // So: the inputs come off the shelf, then the batch comes into
        // existence carrying what those inputs cost, and only then does the
        // dish draw it down.
        let inputs = explosion.draws.iter().filter(|d| d.for_production.is_some());
        let sales = explosion.draws.iter().filter(|d| d.for_production.is_none());

        for (seq, draw) in inputs.enumerate() {
            let mut movement = Movement::new(
                format!("stk_{}_i{seq}", order.core.id),
                draw.material.clone(),
                MovementKind::ProductionOut,
                draw.base_qty,
                at,
                day,
            )
            .by(by.clone());
            movement.order_id = Some(order.core.id.clone());
            movement.order_line_id = draw.line_key.clone();
            movement.produced_for = draw.for_production.clone();
            movement.was_automatic = true;
            movement.typed_unit = self.base_unit_of(outlet, &draw.material)?;
            self.record(outlet, &movement)?;
        }

        for (seq, made) in explosion.productions.iter().enumerate() {
            // **The made material's cost is what its inputs cost.** Anything
            // else would invent a price for something nobody bought.
            let unit_cost = self.production_cost(outlet, made, &recipes)?;
            let mut movement = Movement::new(
                format!("stk_{}_p{seq}", order.core.id),
                made.material.clone(),
                MovementKind::ProductionIn,
                made.base_qty,
                at,
                day,
            )
            .by(by.clone())
            .costing(unit_cost);
            movement.order_id = Some(order.core.id.clone());
            movement.order_line_id = made.line_key.clone();
            movement.was_automatic = true;
            movement.note = Some("Made automatically, because a sale needed it".to_owned());
            movement.typed_unit = self.base_unit_of(outlet, &made.material)?;
            self.record(outlet, &movement)?;
        }

        for (seq, draw) in sales.enumerate() {
            let mut movement = Movement::new(
                format!("stk_{}_{seq}", order.core.id),
                draw.material.clone(),
                MovementKind::Sale,
                draw.base_qty,
                at,
                day,
            )
            .by(by.clone());
            movement.order_id = Some(order.core.id.clone());
            movement.order_line_id = draw.line_key.clone();
            movement.typed_unit = self.base_unit_of(outlet, &draw.material)?;
            self.record(outlet, &movement)?;
        }

        for problem in &explosion.problems {
            self.record_problem(outlet, problem, Some(&order.core.id), at)?;
        }
        Ok(())
    }

    /// **D113 — put back exactly what was taken, by negating the rows.**
    ///
    /// Not by re-running the recipe. Voiding Tuesday's bill on Friday must
    /// return Tuesday's quantities at Tuesday's costs; re-exploding would use
    /// Friday's recipe, and if the chef changed the gravy on Wednesday the rice
    /// balance would permanently gain the difference.
    pub fn reverse_for_bill(
        &self,
        outlet: &str,
        order_id: &OrderId,
        at: Timestamp,
        day: BusinessDay,
        by: Option<&StaffId>,
    ) -> Result<(), DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT id, material_id, base_qty, typed_qty, typed_unit, unit_cost, order_line_id
               FROM stock_movements
              WHERE order_id = ?1 AND kind <> 'reversal'
                AND id NOT IN (SELECT reverses_id FROM stock_movements
                                WHERE order_id = ?1 AND reverses_id IS NOT NULL)
              ORDER BY id",
        )?;
        let mut cursor = stmt.query([order_id.as_str()])?;
        let mut originals = Vec::new();
        while let Some(row) = cursor.next()? {
            originals.push((
                row.get::<_, String>(0)?,
                MaterialId::new(row.get::<_, String>(1)?),
                encode::qty_from_sql(row.get(2)?),
                encode::qty_from_sql(row.get(3)?),
                row.get::<_, String>(4)?,
                UnitCost::from_paise_per_thousand(row.get(5)?),
                row.get::<_, Option<String>>(6)?,
            ));
        }
        drop(cursor);
        drop(stmt);

        for (id, material, base_qty, typed_qty, typed_unit, unit_cost, line) in originals {
            let mut movement = Movement::new(
                format!("stkrev_{id}"),
                material,
                MovementKind::Reversal,
                Qty::from_thousandths(-base_qty.thousandths()),
                at,
                day,
            )
            .typed(Qty::from_thousandths(-typed_qty.thousandths()), typed_unit)
            .costing(unit_cost);
            movement.order_id = Some(order_id.clone());
            movement.order_line_id = line;
            movement.reverses_id = Some(id);
            movement.staff = by.cloned();
            movement.note = Some("The bill was voided".to_owned());
            self.record(outlet, &movement)?;
        }

        // The problems this bill raised are no longer this bill's, so they stop
        // counting against it.
        self.tx.execute(
            "UPDATE stock_problems SET resolved_at = ?2
              WHERE last_order_id = ?1 AND resolved_at IS NULL",
            params![order_id.as_str(), encode::timestamp_to_sql(at)],
        )?;
        Ok(())
    }

    fn base_unit_of(&self, outlet: &str, material: &MaterialId) -> Result<String, DbError> {
        let tag: String = self
            .tx
            .query_row(
                "SELECT dimension FROM materials WHERE outlet_id = ?1 AND id = ?2",
                params![outlet, material.as_str()],
                |row| row.get(0),
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(String::new()),
                other => Err(other),
            })?;
        Ok(Dimension::from_tag(&tag).map_or_else(String::new, |d| d.base_unit().to_owned()))
    }

    /// What a batch of a made material cost, from what went into it.
    fn production_cost(
        &self,
        outlet: &str,
        made: &mb_core::Production,
        recipes: &Recipes,
    ) -> Result<UnitCost, DbError> {
        let costs = self.costs(outlet)?;
        let names = self.material_names(outlet)?;
        let owner = RecipeOwner::Material(made.material.clone());
        let costed = mb_core::cost_of(&owner, recipes, &costs, &names);
        let batch = recipes.get(&owner).map_or(Qty::ONE, |r| r.batch_yield);
        Ok(UnitCost::from_batch(costed.total, batch).unwrap_or(UnitCost::ZERO))
    }

    // =======================================================================
    // PROBLEMS
    // =======================================================================

    /// Record a problem, **grouped rather than one row per bill**.
    ///
    /// A shop with 400 items and 3 recipes would otherwise write four rows
    /// every bill for ever, and the one thing an owner needs to read would be
    /// buried under its own repetitions.
    pub fn record_problem(
        &self,
        outlet: &str,
        problem: &Problem,
        order: Option<&OrderId>,
        at: Timestamp,
    ) -> Result<(), DbError> {
        let subject = problem.subject();
        self.tx.execute(
            "INSERT INTO stock_problems
                 (id, outlet_id, kind, subject, sentence, occurrences, first_at, last_at,
                  last_order_id, resolved_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6, ?7, NULL)
             ON CONFLICT (outlet_id, kind, subject) DO UPDATE SET
                 occurrences = occurrences + 1,
                 last_at = excluded.last_at,
                 last_order_id = excluded.last_order_id,
                 sentence = excluded.sentence,
                 resolved_at = NULL",
            params![
                format!("prb_{}_{subject}", problem.kind()),
                outlet,
                problem.kind(),
                subject,
                problem.sentence(),
                encode::timestamp_to_sql(at),
                order.map(OrderId::as_str),
            ],
        )?;
        Ok(())
    }

    /// Everything the owner has not dealt with.
    pub fn problems(&self, outlet: &str) -> Result<Vec<ProblemRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT id, kind, subject, sentence, occurrences, first_at, last_at, last_order_id
               FROM stock_problems
              WHERE outlet_id = ?1 AND resolved_at IS NULL
              ORDER BY occurrences DESC, last_at DESC",
        )?;
        let mut cursor = stmt.query([outlet])?;
        let mut out = Vec::new();
        while let Some(row) = cursor.next()? {
            let order: Option<String> = row.get(7)?;
            out.push(ProblemRow {
                id: row.get(0)?,
                kind: row.get(1)?,
                subject: row.get(2)?,
                sentence: row.get(3)?,
                occurrences: row.get(4)?,
                first_at: encode::timestamp_from_sql(row.get(5)?),
                last_at: encode::timestamp_from_sql(row.get(6)?),
                last_order_id: order.map(OrderId::new),
            });
        }
        Ok(out)
    }

    /// Mark one as dealt with, after the owner has fixed the thing.
    pub fn resolve_problem(&self, outlet: &str, id: &str, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "UPDATE stock_problems SET resolved_at = ?3 WHERE outlet_id = ?1 AND id = ?2",
            params![outlet, id, encode::timestamp_to_sql(at)],
        )?;
        Ok(())
    }

    // =======================================================================
    // READING IT BACK
    // =======================================================================

    /// One material's history, newest first.
    pub fn movements(
        &self,
        outlet: &str,
        material: Option<&MaterialId>,
        from: Option<BusinessDay>,
        to: Option<BusinessDay>,
        limit: u32,
    ) -> Result<Vec<MovementRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT m.id, m.material_id, mt.name, m.kind, m.base_qty, m.typed_qty, m.typed_unit,
                    m.unit_cost, m.total_cost, m.business_day, m.at, m.staff_id, m.order_id,
                    r.text, m.note, m.was_automatic, made.name
               FROM stock_movements m
               JOIN materials mt ON mt.id = m.material_id
               LEFT JOIN reasons r ON r.id = m.reason_id
               LEFT JOIN materials made ON made.id = m.produced_for
              WHERE m.outlet_id = ?1
                AND (?2 IS NULL OR m.material_id = ?2)
                AND (?3 IS NULL OR m.business_day >= ?3)
                AND (?4 IS NULL OR m.business_day <= ?4)
              ORDER BY m.at DESC, m.id DESC
              LIMIT ?5",
        )?;
        let mut cursor = stmt.query(params![
            outlet,
            material.map(MaterialId::as_str),
            from.map(encode::business_day_to_sql),
            to.map(encode::business_day_to_sql),
            limit,
        ])?;
        let mut out = Vec::new();
        while let Some(row) = cursor.next()? {
            let staff: Option<String> = row.get(11)?;
            let order: Option<String> = row.get(12)?;
            out.push(MovementRow {
                id: row.get(0)?,
                material: MaterialId::new(row.get::<_, String>(1)?),
                material_name: row.get(2)?,
                kind: MovementKind::from_tag(&row.get::<_, String>(3)?)?,
                base_qty: encode::qty_from_sql(row.get(4)?),
                typed_qty: encode::qty_from_sql(row.get(5)?),
                typed_unit: row.get(6)?,
                unit_cost: UnitCost::from_paise_per_thousand(row.get(7)?),
                total_cost: encode::money_from_sql(row.get(8)?),
                business_day: encode::business_day_from_sql(
                    row.get(9)?,
                    "stock_movements.business_day",
                )?,
                at: encode::timestamp_from_sql(row.get(10)?),
                staff: staff.map(StaffId::new),
                order_id: order.map(OrderId::new),
                reason: row.get(13)?,
                note: row.get(14)?,
                was_automatic: encode::bool_from_sql(
                    row.get(15)?,
                    "stock_movements.was_automatic",
                )?,
                produced_for: row.get(16)?,
            });
        }
        Ok(out)
    }

    /// **Theoretical against actual** — scope 4.9, D115.
    ///
    /// Theoretical is what the recipes say the sales used (`sale` and
    /// `production_out`, which are the same food). Actual adds wastage and
    /// adjustments — and until P26's physical count exists, "actual" is only as
    /// good as what a person typed, which is why `last_counted_at` travels with
    /// every row and the screen says **"never counted"** rather than showing a
    /// confident 0.0%.
    pub fn consumption(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<ConsumptionRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT m.material_id, mt.name, mt.avg_cost, mt.last_counted_at,
                    SUM(CASE WHEN m.kind IN ('sale', 'production_out') THEN -m.base_qty
                             WHEN m.kind = 'reversal' THEN -m.base_qty
                             ELSE 0 END) AS theoretical,
                    SUM(CASE WHEN m.base_qty < 0 THEN -m.base_qty ELSE 0 END)
                      - SUM(CASE WHEN m.kind = 'reversal' AND m.base_qty > 0 THEN m.base_qty
                                 ELSE 0 END) AS actual
               FROM stock_movements m
               JOIN materials mt ON mt.id = m.material_id
              WHERE m.outlet_id = ?1 AND m.business_day BETWEEN ?2 AND ?3
                AND m.kind <> 'production_in'
              GROUP BY m.material_id
              ORDER BY mt.name",
        )?;
        let mut cursor = stmt.query(params![
            outlet,
            encode::business_day_to_sql(from),
            encode::business_day_to_sql(to)
        ])?;
        let mut out = Vec::new();
        while let Some(row) = cursor.next()? {
            let counted: Option<i64> = row.get(3)?;
            let theoretical = encode::qty_from_sql(row.get(4)?);
            let actual = encode::qty_from_sql(row.get(5)?);
            let variance = actual.sub(theoretical).unwrap_or(Qty::ZERO);
            let cost = UnitCost::from_paise_per_thousand(row.get(2)?);
            out.push(ConsumptionRow {
                material: MaterialId::new(row.get::<_, String>(0)?),
                name: row.get(1)?,
                theoretical,
                actual,
                variance,
                variance_value: cost.cost_of(variance).unwrap_or(Money::ZERO),
                last_counted_at: counted.map(encode::timestamp_from_sql),
            });
        }
        Ok(out)
    }

    /// The closing figure per material, written by the day close (P18).
    ///
    /// Idempotent: closing the same day twice writes the same rows, and
    /// reopening and re-closing overwrites them, which is what D77's door
    /// requires.
    pub fn close_day(&self, outlet: &str, day: BusinessDay) -> Result<usize, DbError> {
        let written = self.tx.execute(
            "INSERT INTO stock_day_closes (outlet_id, business_day, material_id, closing_qty, unit_cost)
             SELECT b.outlet_id, ?2, b.material_id, b.base_qty, m.avg_cost
               FROM material_balances b JOIN materials m ON m.id = b.material_id
              WHERE b.outlet_id = ?1
             ON CONFLICT (outlet_id, business_day, material_id) DO UPDATE SET
                 closing_qty = excluded.closing_qty,
                 unit_cost = excluded.unit_cost",
            params![outlet, encode::business_day_to_sql(day)],
        )?;
        Ok(written)
    }
}

/// Which columns an owner fills in. Exactly one, enforced by the schema.
fn owner_columns(owner: &RecipeOwner) -> (Option<&str>, Option<&str>, Option<&str>) {
    match owner {
        RecipeOwner::Item(id) => (Some(id.as_str()), None, None),
        RecipeOwner::Modifier(id) => (None, Some(id.as_str()), None),
        RecipeOwner::Material(id) => (None, None, Some(id.as_str())),
    }
}

fn owner_from_row(
    kind: &str,
    item: Option<String>,
    modifier: Option<String>,
    material: Option<String>,
) -> Result<RecipeOwner, DbError> {
    match (kind, item, modifier, material) {
        ("item", Some(id), _, _) => Ok(RecipeOwner::Item(mb_core::ItemId::new(id))),
        ("modifier", _, Some(id), _) => Ok(RecipeOwner::Modifier(mb_core::ModifierId::new(id))),
        ("material", _, _, Some(id)) => Ok(RecipeOwner::Material(MaterialId::new(id))),
        _ => Err(DbError::invariant(format!(
            "a recipe says it belongs to a `{kind}` and does not say which one"
        ))),
    }
}

/// Deterministic, so saving the same recipe twice updates rather than
/// duplicates, and so a test can name the row.
fn recipe_id(outlet: &str, owner: &RecipeOwner) -> String {
    format!("rcp_{outlet}_{}_{}", owner.tag(), owner.subject())
}

/// **What a bill sold, as the recipe walk wants it.**
///
/// A line with no item id — an ad-hoc price typed at the counter — produces no
/// `Sold` at all, deliberately. It is not a missing recipe; it is a thing that
/// was never on the menu, and telling an owner "₹50 has no recipe" every time
/// somebody types a custom price would be noise they learn to ignore.
fn sold_from(order: &SettledOrder) -> Vec<Sold> {
    let mut out = Vec::new();
    for (seq, line) in order.core.cart.lines().iter().enumerate() {
        if line.snapshot.item_id.is_empty() {
            continue;
        }
        let key = crate::repo::order::line_id(order.core.id.as_str(), seq);
        out.push(Sold {
            line_key: key.clone(),
            name: line.snapshot.name.clone(),
            owner: RecipeOwner::Item(line.snapshot.item_id.clone()),
            qty: line.qty,
        });
        for modifier in &line.modifiers {
            if modifier.modifier_id.is_empty() {
                continue;
            }
            out.push(Sold {
                line_key: key.clone(),
                name: modifier.name.clone(),
                owner: RecipeOwner::Modifier(modifier.modifier_id.clone()),
                // One modifier per dish, so "extra cheese" on two pizzas is
                // twice the cheese.
                qty: line.qty,
            });
        }
    }
    out
}

fn read_material(row: &rusqlite::Row<'_>) -> rusqlite::Result<Material> {
    let dimension: String = row.get(2)?;
    let shelf: Option<i64> = row.get(8)?;
    let cost_changed: Option<i64> = row.get(11)?;
    let counted: Option<i64> = row.get(12)?;
    Ok(Material {
        id: MaterialId::new(row.get::<_, String>(0)?),
        name: row.get(1)?,
        // A dimension the code does not recognise falls back to weight rather
        // than blanking the stock screen. It cannot happen — the column has a
        // CHECK — and if it ever does, a wrong unit label beats no page.
        dimension: Dimension::from_tag(&dimension).unwrap_or(Dimension::Weight),
        category: row.get(3)?,
        buy_from: row.get(4)?,
        reorder_level: encode::qty_from_sql(row.get(5)?),
        reorder_qty: encode::qty_from_sql(row.get(6)?),
        is_perishable: row.get::<_, i64>(7)? == 1,
        shelf_life_days: shelf.and_then(|d| u32::try_from(d).ok()),
        location: row.get(9)?,
        avg_cost: UnitCost::from_paise_per_thousand(row.get(10)?),
        cost_changed_at: cost_changed.map(encode::timestamp_from_sql),
        last_counted_at: counted.map(encode::timestamp_from_sql),
        is_active: row.get::<_, i64>(13)? == 1,
        sort_order: row.get(14)?,
        supplier_id: row.get(15)?,
        packs: Vec::new(),
        purchase_unit: None,
        recipe_unit: None,
    })
}
