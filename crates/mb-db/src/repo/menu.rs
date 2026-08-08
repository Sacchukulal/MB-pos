//! The live menu.
//!
//! mb-core has [`ItemSnapshot`] — what an item *was* when it was sold — but no
//! type for what an item *is* today, because the billing rules never need one:
//! a bill reads the snapshot on its own line and never joins back to the menu
//! (crown jewel 4). So the live-menu types live here, and they are still typed
//! values — [`Money`], [`TaxRate`], [`TaxTreatment`] — not rows.
//!
//! P13 owns the screens. This owns the rows.

use mb_core::{
    CategoryId, ItemId, ItemSnapshot, Money, TaxClassId, TaxRate, TaxTreatment, Timestamp,
};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: CategoryId,
    pub name: String,
    pub sort_order: i64,
    pub is_active: bool,
}

/// An item as it stands on today's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: ItemId,
    pub category_id: Option<CategoryId>,
    pub name: String,
    pub unit_price: Money,
    /// **What the owner CHOSE** (P13). The rate and treatment below are what
    /// that choice currently resolves to â denormalised so the billing path
    /// never joins for a rate, and rewritten by `TaxClassRepo::save` when the
    /// class changes. A past bill is untouched by any of it: its line froze
    /// its own copy (crown jewel 4, D52).
    pub tax_class_id: Option<TaxClassId>,
    pub tax_rate: TaxRate,
    pub tax_treatment: TaxTreatment,
    pub hsn: Option<String>,
    /// Scope 4.1. `None`, not zero — a shop that has not costed its menu must
    /// not be shown a margin of 100%.
    pub cost_price: Option<Money>,
    /// Scope 1.3, typed at the counter instead of the name.
    pub short_code: Option<String>,
    /// Scope 3.6, the KDS prep-time target.
    pub prep_minutes: Option<i64>,
    pub is_open_price: bool,
    pub is_available: bool,
    pub sort_order: i64,
}

impl MenuItem {
    /// Freeze this item onto a line.
    ///
    /// The one place the live menu turns into a snapshot. After this the bill
    /// never looks at the menu again, which is why renaming or repricing an
    /// item tomorrow cannot change a bill printed today.
    #[must_use]
    pub fn snapshot(&self) -> ItemSnapshot {
        let mut snapshot = ItemSnapshot::new(
            self.id.clone(),
            self.name.clone(),
            self.unit_price,
            self.tax_rate,
        )
        .with_treatment(self.tax_treatment);
        if let Some(hsn) = &self.hsn {
            snapshot = snapshot.with_hsn(hsn.clone());
        }
        if let Some(category) = &self.category_id {
            snapshot = snapshot.with_category(category.clone());
        }
        snapshot
    }
}

#[derive(Debug)]
pub struct MenuRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> MenuRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        MenuRepo { tx }
    }

    pub fn save_category(
        &self,
        outlet: &str,
        category: &Category,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO categories (id, outlet_id, name, sort_order, is_active, created_at,
                                     updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name,
                                            sort_order = excluded.sort_order,
                                            is_active = excluded.is_active,
                                            updated_at = excluded.updated_at",
            rusqlite::params![
                category.id.as_str(),
                outlet,
                category.name,
                category.sort_order,
                encode::bool_to_sql(category.is_active),
                encode::timestamp_to_sql(at),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "categories", category.id.as_str(), Op::Upsert, at)
    }

    pub fn list_categories(&self, outlet: &str) -> Result<Vec<Category>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, sort_order, is_active FROM categories
              WHERE outlet_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, sort_order, is_active) = row?;
            out.push(Category {
                id: CategoryId::new(id),
                name,
                sort_order,
                is_active: encode::bool_from_sql(is_active, "categories.is_active")?,
            });
        }
        Ok(out)
    }

    pub fn save_item(&self, outlet: &str, item: &MenuItem, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO items (id, outlet_id, category_id, name, unit_price, tax_class_id, tax_rate_bp,
                                tax_treatment, hsn, cost_price, short_code, prep_minutes,
                                is_open_price, is_available, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?16, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
             ON CONFLICT (id) DO UPDATE SET category_id   = excluded.category_id,
                                            name          = excluded.name,
                                            unit_price    = excluded.unit_price,
                                            tax_class_id  = excluded.tax_class_id,
                                            tax_rate_bp   = excluded.tax_rate_bp,
                                            tax_treatment = excluded.tax_treatment,
                                            hsn           = excluded.hsn,
                                            cost_price    = excluded.cost_price,
                                            short_code    = excluded.short_code,
                                            prep_minutes  = excluded.prep_minutes,
                                            is_open_price = excluded.is_open_price,
                                            is_available  = excluded.is_available,
                                            sort_order    = excluded.sort_order,
                                            updated_at    = excluded.updated_at",
            rusqlite::params![
                item.id.as_str(),
                outlet,
                item.category_id.as_ref().map(CategoryId::as_str),
                item.name,
                encode::money_to_sql(item.unit_price),
                encode::tax_rate_to_sql(item.tax_rate),
                encode::tax_treatment_to_sql(item.tax_treatment),
                item.hsn,
                item.cost_price.map(encode::money_to_sql),
                item.short_code,
                item.prep_minutes,
                encode::bool_to_sql(item.is_open_price),
                encode::bool_to_sql(item.is_available),
                item.sort_order,
                encode::timestamp_to_sql(at),
                item.tax_class_id.as_ref().map(TaxClassId::as_str),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "items", item.id.as_str(), Op::Upsert, at)
    }

    pub fn list_items(&self, outlet: &str, available_only: bool) -> Result<Vec<MenuItem>, DbError> {
        let sql = if available_only {
            "SELECT id, category_id, name, unit_price, tax_class_id, tax_rate_bp, tax_treatment, hsn,
                    cost_price, short_code, prep_minutes, is_open_price, is_available, sort_order
               FROM items WHERE outlet_id = ?1 AND is_available = 1 ORDER BY sort_order, name"
        } else {
            "SELECT id, category_id, name, unit_price, tax_class_id, tax_rate_bp, tax_treatment, hsn,
                    cost_price, short_code, prep_minutes, is_open_price, is_available, sort_order
               FROM items WHERE outlet_id = ?1 ORDER BY sort_order, name"
        };
        let mut stmt = self.tx.prepare_cached(sql)?;
        let rows = stmt.query_map([outlet], |row| {
            Ok(ItemRow {
                id: row.get(0)?,
                category_id: row.get(1)?,
                name: row.get(2)?,
                unit_price: row.get(3)?,
                tax_class_id: row.get(4)?,
                tax_rate_bp: row.get(5)?,
                tax_treatment: row.get(6)?,
                hsn: row.get(7)?,
                cost_price: row.get(8)?,
                short_code: row.get(9)?,
                prep_minutes: row.get(10)?,
                is_open_price: row.get(11)?,
                is_available: row.get(12)?,
                sort_order: row.get(13)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            let row = row?;
            out.push(MenuItem {
                id: ItemId::new(row.id),
                category_id: row.category_id.map(CategoryId::new),
                name: row.name,
                unit_price: encode::money_from_sql(row.unit_price),
                tax_class_id: row.tax_class_id.map(TaxClassId::new),
                tax_rate: encode::tax_rate_from_sql(row.tax_rate_bp, "items.tax_rate_bp")?,
                tax_treatment: encode::tax_treatment_from_sql(&row.tax_treatment)?,
                hsn: row.hsn,
                cost_price: row.cost_price.map(encode::money_from_sql),
                short_code: row.short_code,
                prep_minutes: row.prep_minutes,
                is_open_price: encode::bool_from_sql(row.is_open_price, "items.is_open_price")?,
                is_available: encode::bool_from_sql(row.is_available, "items.is_available")?,
                sort_order: row.sort_order,
            });
        }
        Ok(out)
    }

    pub fn find_item(&self, id: &ItemId) -> Result<Option<MenuItem>, DbError> {
        let outlet: Option<String> = self
            .tx
            .query_row("SELECT outlet_id FROM items WHERE id = ?1", [id.as_str()], |r| {
                r.get(0)
            })
            .ok();
        let Some(outlet) = outlet else {
            return Ok(None);
        };
        Ok(self
            .list_items(&outlet, false)?
            .into_iter()
            .find(|i| &i.id == id))
    }

    /// Take an item off the menu.
    ///
    /// This is the operation P13 offers, and it is not a DELETE. An item that
    /// has ever been billed cannot be deleted — `order_lines.item_id` is what
    /// item-wise sales (10.2) reads a year later, so a sold item is history in
    /// the same way a staff member who has left is (9.15).
    pub fn set_available(
        &self,
        outlet: &str,
        id: &ItemId,
        available: bool,
        at: Timestamp,
    ) -> Result<(), DbError> {
        let n = self.tx.execute(
            "UPDATE items SET is_available = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![
                id.as_str(),
                encode::bool_to_sql(available),
                encode::timestamp_to_sql(at)
            ],
        )?;
        if n == 0 {
            return Err(DbError::invariant(format!("there is no item {id}")));
        }
        OutboxRepo::new(self.tx).enqueue(outlet, "items", id.as_str(), Op::Upsert, at)
    }

    /// Delete an item that has never been sold.
    ///
    /// Refuses, in words a cashier could act on, if the item is on any order —
    /// the foreign key would refuse it anyway, but a leaked constraint name is
    /// not an error message.
    pub fn delete_item(&self, outlet: &str, id: &ItemId, at: Timestamp) -> Result<(), DbError> {
        let sold: i64 = self.tx.query_row(
            "SELECT count(*) FROM order_lines WHERE item_id = ?1",
            [id.as_str()],
            |r| r.get(0),
        )?;
        if sold > 0 {
            return Err(DbError::invariant(format!(
                "{id} has been sold {sold} time(s) and cannot be deleted — \
                 take it off the menu instead, so old bills and sales reports stay correct"
            )));
        }
        self.tx
            .execute("DELETE FROM items WHERE id = ?1", [id.as_str()])?;
        OutboxRepo::new(self.tx).enqueue_with_tombstone(
            outlet,
            "items",
            id.as_str(),
            Op::Delete,
            Some(id.as_str()),
            at,
        )
    }
}

struct ItemRow {
    id: String,
    category_id: Option<String>,
    name: String,
    unit_price: i64,
    tax_class_id: Option<String>,
    tax_rate_bp: i64,
    tax_treatment: String,
    hsn: Option<String>,
    cost_price: Option<i64>,
    short_code: Option<String>,
    prep_minutes: Option<i64>,
    is_open_price: i64,
    is_available: i64,
    sort_order: i64,
}
