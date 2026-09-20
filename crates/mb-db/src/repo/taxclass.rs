//! The shop's tax slabs — the one place tax lives — and the book the counter reads them from.

use mb_auth::audit::action;
use mb_core::{CategoryId, ItemId, PriceBasis, TaxBook, TaxClass, TaxClassId, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

#[derive(Debug)]
pub struct TaxClassRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> TaxClassRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        TaxClassRepo { tx }
    }

    /// Every slab, retired ones included — an old item may still point at one.
    pub fn list(&self, outlet: &str) -> Result<Vec<TaxClass>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, rate_bp, kind, basis, is_active FROM tax_classes
              WHERE outlet_id = ?1 ORDER BY is_active DESC, sort_order, rate_bp, name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;

        let mut classes = Vec::new();
        for row in rows {
            let (id, name, rate_bp, kind, basis, active) = row?;
            classes.push(TaxClass {
                id: TaxClassId::new(id),
                name,
                kind: encode::tax_kind_from_sql(&kind)?,
                rate: encode::tax_rate_from_sql(rate_bp, "tax_classes.rate_bp")?,
                basis: encode::price_basis_opt_from_sql(basis.as_deref())?,
                is_active: encode::bool_from_sql(active, "tax_classes.is_active")?,
            });
        }
        Ok(classes)
    }

    pub fn find(&self, outlet: &str, id: &TaxClassId) -> Result<Option<TaxClass>, DbError> {
        Ok(self.list(outlet)?.into_iter().find(|c| c.id == *id))
    }

    /// The slabs, the shop's pricing default and the shop's own rate, together — what every
    /// tax question is asked of.
    pub fn book(&self, outlet: &str) -> Result<TaxBook, DbError> {
        let profile: Option<(String, Option<String>)> = self
            .tx
            .query_row(
                "SELECT price_basis, default_tax_class_id FROM store_profile WHERE outlet_id = ?1",
                [outlet],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        let (shop_basis, shop_class) = match profile {
            Some((text, class)) => (
                encode::price_basis_from_sql(&text)?,
                class.map(TaxClassId::new),
            ),
            None => (PriceBasis::Exclusive, None),
        };
        Ok(TaxBook::new(self.list(outlet)?, shop_basis).with_shop_class(shop_class))
    }

    /// The shop's rate, as the slab it resolves to today.
    pub fn shop_slab(&self, outlet: &str) -> Result<TaxClassId, DbError> {
        self.book(outlet)?
            .shop_slab()
            .map(|c| c.id.clone())
            .ok_or_else(|| DbError::invariant("this shop has no tax slab at all"))
    }

    /// What an item in this category is taxed at unless it says otherwise: the category's own
    /// rate, else the shop's.
    pub fn rate_for(
        &self,
        outlet: &str,
        category: Option<&CategoryId>,
    ) -> Result<TaxClassId, DbError> {
        if let Some(category) = category {
            let own: Option<Option<String>> = self
                .tx
                .query_row(
                    "SELECT default_tax_class_id FROM categories
                      WHERE outlet_id = ?1 AND id = ?2 AND is_active = 1",
                    rusqlite::params![outlet, category.as_str()],
                    |r| r.get(0),
                )
                .ok();
            if let Some(Some(id)) = own {
                let live = self.find(outlet, &TaxClassId::new(id.clone()))?;
                if live.is_some_and(|c| c.is_active) {
                    return Ok(TaxClassId::new(id));
                }
            }
        }
        self.shop_slab(outlet)
    }

    /// The shop's rate becomes `to`. Every item that was on the shop's rate follows it; an item
    /// on its own rate, or in a category with a rate of its own, stays where it is.
    pub fn set_shop_slab(
        &self,
        outlet: &str,
        to: &TaxClassId,
        at: Timestamp,
    ) -> Result<usize, DbError> {
        let live = self
            .find(outlet, to)?
            .ok_or_else(|| DbError::invariant("that tax slab is not one this shop has"))?;
        if !live.is_active {
            return Err(DbError::invariant("that tax slab has been removed"));
        }
        let from = self.shop_slab(outlet)?;
        let changed = self.tx.execute(
            "UPDATE store_profile SET default_tax_class_id = ?2 WHERE outlet_id = ?1",
            rusqlite::params![outlet, to.as_str()],
        )?;
        if changed == 0 {
            self.tx.execute(
                "INSERT INTO store_profile (outlet_id, default_tax_class_id, updated_at)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![outlet, to.as_str(), encode::timestamp_to_sql(at)],
            )?;
        }
        // The profile changed: stamped and queued for the cloud in the one place it is.
        crate::repo::settings::SettingsRepo::new(self.tx).queue_store_profile(outlet, at)?;
        let followers = self.items_on(
            outlet,
            &from,
            "(category_id IS NULL OR category_id NOT IN
                (SELECT id FROM categories WHERE default_tax_class_id IS NOT NULL))",
            rusqlite::params![outlet, from.as_str()],
        )?;
        self.assign(outlet, &followers, Some(to), None, at)
    }

    /// The category's own rate becomes `to` (`None` = the shop's rate again). Every item in it
    /// that was on the category's rate follows; an item on its own rate stays.
    pub fn set_category_slab(
        &self,
        outlet: &str,
        category: &CategoryId,
        to: Option<&TaxClassId>,
        at: Timestamp,
    ) -> Result<usize, DbError> {
        if let Some(to) = to {
            let live = self
                .find(outlet, to)?
                .ok_or_else(|| DbError::invariant("that tax slab is not one this shop has"))?;
            if !live.is_active {
                return Err(DbError::invariant("that tax slab has been removed"));
            }
        }
        let from = self.rate_for(outlet, Some(category))?;
        let n = self.tx.execute(
            "UPDATE categories SET default_tax_class_id = ?3 WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, category.as_str(), to.map(TaxClassId::as_str)],
        )?;
        if n == 0 {
            return Err(DbError::invariant("that category is gone"));
        }
        crate::repo::menu::MenuRepo::new(self.tx).queue_category(outlet, category.as_str(), at)?;
        let now = self.rate_for(outlet, Some(category))?;
        if now == from {
            return Ok(0);
        }
        let followers = self.items_on(
            outlet,
            &from,
            "category_id = ?3",
            rusqlite::params![outlet, from.as_str(), category.as_str()],
        )?;
        self.assign(outlet, &followers, Some(&now), None, at)
    }

    /// The items on a slab that also match `clause`. `params` binds the outlet as ?1, the slab
    /// as ?2, and whatever the clause needs after that.
    fn items_on(
        &self,
        outlet: &str,
        slab: &TaxClassId,
        clause: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<ItemId>, DbError> {
        debug_assert!(!outlet.is_empty() && !slab.as_str().is_empty());
        let sql =
            format!("SELECT id FROM items WHERE outlet_id = ?1 AND tax_class_id = ?2 AND {clause}");
        let mut stmt = self.tx.prepare(&sql)?;
        let rows = stmt.query_map(params, |r| r.get::<_, String>(0))?;
        rows.map(|r| r.map(ItemId::new).map_err(DbError::from))
            .collect()
    }

    /// Save a slab. Items pointing at it need nothing done — they read it.
    pub fn save(&self, outlet: &str, class: &TaxClass, at: Timestamp) -> Result<(), DbError> {
        if !class.is_coherent() {
            return Err(DbError::invariant(format!(
                "the tax slab {} contradicts itself",
                class.name
            )));
        }
        self.tx.execute(
            "INSERT INTO tax_classes (id, outlet_id, name, rate_bp, kind, basis, is_active,
                                      sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                     COALESCE((SELECT MAX(sort_order) + 1 FROM tax_classes WHERE outlet_id = ?2), 0))
             ON CONFLICT (id) DO UPDATE SET name      = excluded.name,
                                            rate_bp   = excluded.rate_bp,
                                            kind      = excluded.kind,
                                            basis     = excluded.basis,
                                            is_active = excluded.is_active",
            rusqlite::params![
                class.id.as_str(),
                outlet,
                class.name,
                encode::tax_rate_to_sql(class.rate),
                encode::tax_kind_to_sql(class.kind),
                encode::price_basis_opt_to_sql(class.basis),
                encode::bool_to_sql(class.is_active),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "tax_classes", class.id.as_str(), Op::Upsert, at)
    }

    /// Take a slab away. One that items still use is refused with the count; one nothing uses
    /// is deleted outright — a slab is vocabulary, not money, and every bill froze its own tax.
    pub fn remove(&self, outlet: &str, id: &TaxClassId, at: Timestamp) -> Result<(), DbError> {
        let using = self.items_using(id)?;
        if using > 0 {
            return Err(DbError::invariant(format!(
                "{using} item(s) still use this slab. Move them to another one first."
            )));
        }
        let charged: i64 = self.tx.query_row(
            "SELECT COUNT(*) FROM settings
              WHERE outlet_id = ?1 AND value = ?2
                AND key IN ('billing.service_charge_tax', 'billing.packing_charge_tax',
                            'billing.delivery_charge_tax')",
            rusqlite::params![outlet, id.as_str()],
            |r| r.get(0),
        )?;
        if charged > 0 {
            return Err(DbError::invariant(
                "A charge (service, packing or delivery) still uses this slab. Change that first.",
            ));
        }
        let mut stmt = self.tx.prepare(
            "SELECT id FROM categories WHERE outlet_id = ?1 AND default_tax_class_id = ?2",
        )?;
        let followers = stmt
            .query_map(rusqlite::params![outlet, id.as_str()], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        self.tx.execute(
            "UPDATE categories SET default_tax_class_id = NULL
              WHERE outlet_id = ?1 AND default_tax_class_id = ?2",
            rusqlite::params![outlet, id.as_str()],
        )?;
        for category in &followers {
            crate::repo::menu::MenuRepo::new(self.tx).queue_category(outlet, category, at)?;
        }
        let n = self.tx.execute(
            "DELETE FROM tax_classes WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, id.as_str()],
        )?;
        if n == 0 {
            return Err(DbError::invariant(format!(
                "there is no tax slab {}",
                id.as_str()
            )));
        }
        OutboxRepo::new(self.tx).enqueue_with_tombstone(
            outlet,
            "tax_classes",
            id.as_str(),
            Op::Delete,
            Some(id.as_str()),
            at,
        )
    }

    /// How many items point at this slab.
    pub fn items_using(&self, id: &TaxClassId) -> Result<i64, DbError> {
        Ok(self.tx.query_row(
            "SELECT COUNT(*) FROM items WHERE tax_class_id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )?)
    }

    /// Point many items at a slab and/or give them a pricing say, in one go. `None` leaves
    /// that half alone; `Some(None)` for the basis means "follow the slab and the shop again".
    pub fn assign(
        &self,
        outlet: &str,
        items: &[ItemId],
        class: Option<&TaxClassId>,
        basis: Option<Option<PriceBasis>>,
        at: Timestamp,
    ) -> Result<usize, DbError> {
        if let Some(class) = class {
            let live = self
                .find(outlet, class)?
                .ok_or_else(|| DbError::invariant("that tax slab is not one this shop has"))?;
            if !live.is_active {
                return Err(DbError::invariant("that tax slab has been removed"));
            }
        }
        let mut changed = 0;
        for item in items {
            let n = match (class, basis) {
                (Some(class), Some(basis)) => self.tx.execute(
                    "UPDATE items SET tax_class_id = ?3, price_basis = ?4, updated_at = ?2
                      WHERE outlet_id = ?1 AND id = ?5",
                    rusqlite::params![
                        outlet,
                        encode::timestamp_to_sql(at),
                        class.as_str(),
                        encode::price_basis_opt_to_sql(basis),
                        item.as_str()
                    ],
                )?,
                (Some(class), None) => self.tx.execute(
                    "UPDATE items SET tax_class_id = ?3, updated_at = ?2
                      WHERE outlet_id = ?1 AND id = ?4",
                    rusqlite::params![
                        outlet,
                        encode::timestamp_to_sql(at),
                        class.as_str(),
                        item.as_str()
                    ],
                )?,
                (None, Some(basis)) => self.tx.execute(
                    "UPDATE items SET price_basis = ?3, updated_at = ?2
                      WHERE outlet_id = ?1 AND id = ?4",
                    rusqlite::params![
                        outlet,
                        encode::timestamp_to_sql(at),
                        encode::price_basis_opt_to_sql(basis),
                        item.as_str()
                    ],
                )?,
                (None, None) => 0,
            };
            if n > 0 {
                changed += 1;
                OutboxRepo::new(self.tx).enqueue(outlet, "items", item.as_str(), Op::Upsert, at)?;
            }
        }
        Ok(changed)
    }

    /// The action name a menu change is recorded under, so every caller writes the same one.
    pub const CHANGED: &'static str = action::PRICE_CHANGED;
}
