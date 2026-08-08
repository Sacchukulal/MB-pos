//! **Variants, modifiers and combos** — the three ways one menu row becomes
//! several things a customer can order.
//!
//! Scope 6.1, 6.2 and 6.3. P04 reserved every table here and nothing has ever
//! written to one; this is where they start meaning something.
//!
//! > *"v1's menu was: category, name, price. That is all. It could not express
//! > a GST rate per item, an HSN code, a half/full portion, an add-on, or a
//! > cost price."*
//!
//! # Why the three live together
//!
//! They answer one question — *what exactly is on this line?* — and they share
//! its one hard rule: **[`LineIdentity`](mb_core::LineIdentity) already decides
//! what makes two lines the same thing**, and it does it with the item id, the
//! note and the *sorted* modifier ids (P01). Nothing here may invent a second
//! answer, because the kitchen ledger, the cart's merge and the bill's line all
//! key on that one.
//!
//! A variant is therefore **its own item id**, not a flag on a line: "Dosa
//! (Half)" and "Dosa (Full)" are two things to cook, two prices and two rows on
//! a rate summary. That is also why `item_variants` carries a price and not a
//! discount — a half dosa is not a discounted dosa.

use mb_core::{ComboComponent, ItemId, Money, Qty, Timestamp, combo};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

/// One size of an item — half/full, 250g/500g/1kg. Scope 6.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub id: ItemId,
    pub item_id: ItemId,
    /// "Half", "500g". Shown after the item's name.
    pub name: String,
    pub unit_price: Money,
    pub sort_order: i64,
    pub is_active: bool,
}

/// A set of choices offered on an item. Scope 6.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifierGroup {
    pub id: String,
    pub name: String,
    /// 0 for "any number"; 1 for "you must choose one".
    pub min_select: i64,
    /// `None` for "as many as you like".
    pub max_select: Option<i64>,
    pub sort_order: i64,
    pub modifiers: Vec<Modifier>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modifier {
    pub id: mb_core::ModifierId,
    pub name: String,
    /// **May be negative** — *"no cheese, −10"* is a real line on a real menu.
    /// Taxed at the LINE's rate, never its own (D54).
    pub price_delta: Money,
    pub sort_order: i64,
    pub is_active: bool,
}

impl ModifierGroup {
    /// **Scope 6.2's rule, and P13's T6.** Is this a legal set of choices?
    ///
    /// Checked here rather than on the screen for R8's reason: hiding a
    /// checkbox is a courtesy, and a group that says "choose one" has to mean
    /// it wherever the choice is made — including from a phone (Phase 11).
    pub fn check(&self, chosen: usize) -> Result<(), DbError> {
        let chosen = i64::try_from(chosen).unwrap_or(i64::MAX);
        if chosen < self.min_select {
            return Err(DbError::invariant(match self.min_select {
                1 => format!("choose a {}", self.name.to_lowercase()),
                n => format!("choose at least {n} from {}", self.name),
            }));
        }
        if let Some(max) = self.max_select
            && chosen > max
        {
            return Err(DbError::invariant(match max {
                1 => format!("only one {} can be chosen", self.name.to_lowercase()),
                n => format!("at most {n} can be chosen from {}", self.name),
            }));
        }
        Ok(())
    }
}

/// A named set sold at one price. Scope 6.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    pub id: String,
    pub name: String,
    pub unit_price: Money,
    pub is_active: bool,
    pub components: Vec<ComboPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComboPart {
    pub item_id: ItemId,
    pub qty: Qty,
    /// The stored proportion — a **cache** of the apportionment, for display
    /// and reporting. The money is recomputed from live prices when the combo
    /// is sold, because a component's price changes and a stored share would
    /// then be stale in a way nobody would notice (D53).
    pub share_bp: u32,
}

#[derive(Debug)]
pub struct CompositionRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> CompositionRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        CompositionRepo { tx }
    }

    // -----------------------------------------------------------------------
    // Variants — scope 6.1.
    // -----------------------------------------------------------------------

    pub fn save_variant(
        &self,
        outlet: &str,
        variant: &Variant,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO item_variants (id, item_id, name, unit_price, sort_order, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET name       = excluded.name,
                                            unit_price = excluded.unit_price,
                                            sort_order = excluded.sort_order,
                                            is_active  = excluded.is_active",
            rusqlite::params![
                variant.id.as_str(),
                variant.item_id.as_str(),
                variant.name,
                encode::money_to_sql(variant.unit_price),
                variant.sort_order,
                encode::bool_to_sql(variant.is_active),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "item_variants", variant.id.as_str(), Op::Upsert, at)
    }

    pub fn variants_of(&self, item: &ItemId) -> Result<Vec<Variant>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, unit_price, sort_order, is_active FROM item_variants
              WHERE item_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([item.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, price, sort_order, active) = row?;
            out.push(Variant {
                id: ItemId::new(id),
                item_id: item.clone(),
                name,
                unit_price: encode::money_from_sql(price),
                sort_order,
                is_active: encode::bool_from_sql(active, "item_variants.is_active")?,
            });
        }
        Ok(out)
    }

    // -----------------------------------------------------------------------
    // Modifiers — scope 6.2.
    // -----------------------------------------------------------------------

    pub fn save_group(
        &self,
        outlet: &str,
        group: &ModifierGroup,
        at: Timestamp,
    ) -> Result<(), DbError> {
        if let Some(max) = group.max_select
            && max < group.min_select
        {
            return Err(DbError::invariant(format!(
                "\"{}\" asks for at least {} and at most {max}, which nobody can satisfy",
                group.name, group.min_select
            )));
        }

        self.tx.execute(
            "INSERT INTO modifier_groups (id, outlet_id, name, min_select, max_select, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET name       = excluded.name,
                                            min_select = excluded.min_select,
                                            max_select = excluded.max_select,
                                            sort_order = excluded.sort_order",
            rusqlite::params![
                group.id,
                outlet,
                group.name,
                group.min_select,
                group.max_select,
                group.sort_order,
            ],
        )?;

        // The choices are replaced wholesale — a handful of rows, and a diff is
        // where "the box looked unticked but the row was still there" comes
        // from (`save_role` and `save` on a tax class make the same argument).
        self.tx
            .execute("DELETE FROM modifiers WHERE group_id = ?1", [group.id.as_str()])?;
        for modifier in &group.modifiers {
            self.tx.execute(
                "INSERT INTO modifiers (id, group_id, name, price_delta, sort_order, is_active)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    modifier.id.as_str(),
                    group.id,
                    modifier.name,
                    encode::money_to_sql(modifier.price_delta),
                    modifier.sort_order,
                    encode::bool_to_sql(modifier.is_active),
                ],
            )?;
        }
        OutboxRepo::new(self.tx).enqueue(outlet, "modifier_groups", &group.id, Op::Upsert, at)
    }

    pub fn groups(&self, outlet: &str) -> Result<Vec<ModifierGroup>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, min_select, max_select, sort_order FROM modifier_groups
              WHERE outlet_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;

        let mut groups = Vec::new();
        for row in rows {
            let (id, name, min_select, max_select, sort_order) = row?;
            let modifiers = self.modifiers_in(&id)?;
            groups.push(ModifierGroup {
                id,
                name,
                min_select,
                max_select,
                sort_order,
                modifiers,
            });
        }
        Ok(groups)
    }

    fn modifiers_in(&self, group_id: &str) -> Result<Vec<Modifier>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, price_delta, sort_order, is_active FROM modifiers
              WHERE group_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([group_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, delta, sort_order, active) = row?;
            out.push(Modifier {
                id: mb_core::ModifierId::new(id),
                name,
                price_delta: encode::money_from_sql(delta),
                sort_order,
                is_active: encode::bool_from_sql(active, "modifiers.is_active")?,
            });
        }
        Ok(out)
    }

    /// Which groups an item offers.
    pub fn groups_for_item(&self, outlet: &str, item: &ItemId) -> Result<Vec<ModifierGroup>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT group_id FROM item_modifier_groups WHERE item_id = ?1 ORDER BY sort_order",
        )?;
        let rows = stmt.query_map([item.as_str()], |row| row.get::<_, String>(0))?;
        let mut wanted = Vec::new();
        for row in rows {
            wanted.push(row?);
        }
        Ok(self
            .groups(outlet)?
            .into_iter()
            .filter(|g| wanted.contains(&g.id))
            .collect())
    }

    pub fn attach_group(
        &self,
        outlet: &str,
        item: &ItemId,
        group_id: &str,
        sort_order: i64,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO item_modifier_groups (item_id, group_id, sort_order)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (item_id, group_id) DO UPDATE SET sort_order = excluded.sort_order",
            rusqlite::params![item.as_str(), group_id, sort_order],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "items", item.as_str(), Op::Upsert, at)
    }

    // -----------------------------------------------------------------------
    // Combos — scope 6.3.
    // -----------------------------------------------------------------------

    /// Save a combo, **apportioning its price as it goes**.
    ///
    /// The shares are computed by `mb_core::combo::apportion`, which is D14's
    /// rounding rule — floor, then largest remainder — so they add back to the
    /// combo price exactly. The stored `share_bp` is a proportion for display
    /// and reporting; the money is recomputed at the moment of sale from live
    /// prices (D53).
    pub fn save_combo(
        &self,
        outlet: &str,
        combo: &Combo,
        standalone: &[(ItemId, Money)],
        at: Timestamp,
    ) -> Result<(), DbError> {
        let parts: Vec<ComboComponent> = combo
            .components
            .iter()
            .map(|part| {
                let price = standalone
                    .iter()
                    .find(|(id, _)| *id == part.item_id)
                    .map_or(Money::ZERO, |(_, price)| *price);
                ComboComponent {
                    item_id: part.item_id.clone(),
                    qty: part.qty,
                    standalone: price,
                }
            })
            .collect();

        let shares = combo::apportion(combo.unit_price, &parts)
            .map_err(|e| DbError::invariant(e.to_string()))?;

        self.tx.execute(
            "INSERT INTO combos (id, outlet_id, name, unit_price, is_active, created_at,
                                 updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT (id) DO UPDATE SET name       = excluded.name,
                                            unit_price = excluded.unit_price,
                                            is_active  = excluded.is_active,
                                            updated_at = excluded.updated_at",
            rusqlite::params![
                combo.id,
                outlet,
                combo.name,
                encode::money_to_sql(combo.unit_price),
                encode::bool_to_sql(combo.is_active),
                encode::timestamp_to_sql(at),
            ],
        )?;

        self.tx
            .execute("DELETE FROM combo_components WHERE combo_id = ?1", [combo.id.as_str()])?;
        for share in &shares {
            self.tx.execute(
                "INSERT INTO combo_components (combo_id, item_id, qty, share_bp)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    combo.id,
                    share.item_id.as_str(),
                    encode::qty_to_sql(share.qty),
                    mb_core::combo::share_basis_points(combo.unit_price, share.share),
                ],
            )?;
        }
        OutboxRepo::new(self.tx).enqueue(outlet, "combos", &combo.id, Op::Upsert, at)
    }

    pub fn combos(&self, outlet: &str) -> Result<Vec<Combo>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, unit_price, is_active FROM combos
              WHERE outlet_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;

        let mut combos = Vec::new();
        for row in rows {
            let (id, name, price, active) = row?;
            let components = self.parts_of(&id)?;
            combos.push(Combo {
                id,
                name,
                unit_price: encode::money_from_sql(price),
                is_active: encode::bool_from_sql(active, "combos.is_active")?,
                components,
            });
        }
        Ok(combos)
    }

    fn parts_of(&self, combo_id: &str) -> Result<Vec<ComboPart>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT item_id, qty, share_bp FROM combo_components
              WHERE combo_id = ?1 ORDER BY item_id",
        )?;
        let rows = stmt.query_map([combo_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (item_id, qty, share_bp) = row?;
            out.push(ComboPart {
                item_id: ItemId::new(item_id),
                qty: encode::qty_from_sql(qty),
                share_bp: u32::try_from(share_bp).unwrap_or(0),
            });
        }
        Ok(out)
    }
}
