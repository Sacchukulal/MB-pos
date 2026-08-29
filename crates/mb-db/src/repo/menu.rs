//! The live menu.

use mb_core::{
    CategoryId, ItemId, ItemSnapshot, Money, PriceBasis, TaxBook, TaxClassId, TaxSpec, Timestamp,
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
    /// Which kitchen screen this category's food goes to.
    pub station: Option<String>,
    /// The slab a new item in this category starts on.
    pub default_tax_class_id: Option<TaxClassId>,
}

/// An item as it stands on today's menu. It says WHICH slab; the slab says what the tax is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: ItemId,
    pub category_id: Option<CategoryId>,
    pub name: String,
    pub unit_price: Money,
    pub tax_class_id: TaxClassId,
    /// The item's own say on whether its price contains the tax. `None` = the slab, then the
    /// shop, decide.
    pub price_basis: Option<PriceBasis>,
    pub hsn: Option<String>,
    /// `None`, not zero — a shop that has not costed its menu must not be shown a margin of
    /// 100%.
    pub cost_price: Option<Money>,
    /// 3, typed at the counter instead of the name.
    pub short_code: Option<String>,
    /// 6, the KDS prep-time target.
    pub prep_minutes: Option<i64>,
    /// Which course this dish belongs to.
    pub course: Option<String>,
    pub is_open_price: bool,
    pub is_available: bool,
    pub sort_order: i64,
}

impl MenuItem {
    /// The whole tax question for this item, answered by the book.
    pub fn tax(&self, book: &TaxBook) -> Result<TaxSpec, DbError> {
        book.spec_for(&self.tax_class_id, self.price_basis)
            .map_err(|e| DbError::invariant(format!("{}: {e}", self.name)))
    }

    /// Freeze this item onto a line.
    pub fn snapshot(&self, book: &TaxBook) -> Result<ItemSnapshot, DbError> {
        let tax = self.tax(book)?;
        let mut snapshot =
            ItemSnapshot::new(self.id.clone(), self.name.clone(), self.unit_price, tax.rate)
                .with_tax(tax);
        if let Some(hsn) = &self.hsn {
            snapshot = snapshot.with_hsn(hsn.clone());
        }
        if let Some(category) = &self.category_id {
            snapshot = snapshot.with_category(category.clone());
        }
        snapshot.course = self.course.clone();
        // A negative or absurd number in the column becomes "no target" rather than a panic —
        // the timer is a help, never a reason a bill cannot be rung up.
        snapshot.prep_minutes = self.prep_minutes.and_then(|m| u32::try_from(m).ok());
        Ok(snapshot)
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
            "INSERT INTO categories (id, outlet_id, name, sort_order, is_active, station,
                                     default_tax_class_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?7, ?8, ?6, ?6)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name,
                                            sort_order = excluded.sort_order,
                                            is_active = excluded.is_active,
                                            station = excluded.station,
                                            default_tax_class_id = excluded.default_tax_class_id,
                                            updated_at = excluded.updated_at",
            rusqlite::params![
                category.id.as_str(),
                outlet,
                category.name,
                category.sort_order,
                encode::bool_to_sql(category.is_active),
                encode::timestamp_to_sql(at),
                category.station,
                category.default_tax_class_id.as_ref().map(TaxClassId::as_str),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "categories", category.id.as_str(), Op::Upsert, at)
    }

    pub fn list_categories(&self, outlet: &str) -> Result<Vec<Category>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, sort_order, is_active, station, default_tax_class_id FROM categories
              WHERE outlet_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, sort_order, is_active, station, default_tax_class_id) = row?;
            out.push(Category {
                id: CategoryId::new(id),
                name,
                sort_order,
                is_active: encode::bool_from_sql(is_active, "categories.is_active")?,
                station,
                default_tax_class_id: default_tax_class_id.map(TaxClassId::new),
            });
        }
        Ok(out)
    }

    pub fn save_item(&self, outlet: &str, item: &MenuItem, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO items (id, outlet_id, category_id, name, unit_price, tax_class_id, price_basis,
                                hsn, cost_price, short_code, prep_minutes, course,
                                is_open_price, is_available, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)
             ON CONFLICT (id) DO UPDATE SET category_id   = excluded.category_id,
                                            name          = excluded.name,
                                            unit_price    = excluded.unit_price,
                                            tax_class_id  = excluded.tax_class_id,
                                            price_basis   = excluded.price_basis,
                                            hsn           = excluded.hsn,
                                            cost_price    = excluded.cost_price,
                                            short_code    = excluded.short_code,
                                            prep_minutes  = excluded.prep_minutes,
                                            course        = excluded.course,
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
                item.tax_class_id.as_str(),
                encode::price_basis_opt_to_sql(item.price_basis),
                item.hsn,
                item.cost_price.map(encode::money_to_sql),
                item.short_code,
                item.prep_minutes,
                item.course,
                encode::bool_to_sql(item.is_open_price),
                encode::bool_to_sql(item.is_available),
                item.sort_order,
                encode::timestamp_to_sql(at),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "items", item.id.as_str(), Op::Upsert, at)
    }

    pub fn list_items(&self, outlet: &str, available_only: bool) -> Result<Vec<MenuItem>, DbError> {
        let sql = if available_only {
            "SELECT id, category_id, name, unit_price, tax_class_id, price_basis, hsn,
                    cost_price, short_code, prep_minutes, course, is_open_price, is_available,
                    sort_order
               FROM items WHERE outlet_id = ?1 AND is_available = 1 ORDER BY sort_order, name"
        } else {
            "SELECT id, category_id, name, unit_price, tax_class_id, price_basis, hsn,
                    cost_price, short_code, prep_minutes, course, is_open_price, is_available,
                    sort_order
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
                price_basis: row.get(5)?,
                hsn: row.get(6)?,
                cost_price: row.get(7)?,
                short_code: row.get(8)?,
                prep_minutes: row.get(9)?,
                course: row.get(10)?,
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
                tax_class_id: TaxClassId::new(row.tax_class_id),
                price_basis: encode::price_basis_opt_from_sql(row.price_basis.as_deref())?,
                hsn: row.hsn,
                cost_price: row.cost_price.map(encode::money_from_sql),
                short_code: row.short_code,
                prep_minutes: row.prep_minutes,
                course: row.course,
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
            .query_row(
                "SELECT outlet_id FROM items WHERE id = ?1",
                [id.as_str()],
                |r| r.get(0),
            )
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
    pub fn find_by_code(
        &self,
        outlet: &str,
        code: &str,
    ) -> Result<Option<(String, String)>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name FROM items
              WHERE outlet_id = ?1 AND short_code IS NOT NULL
                AND upper(short_code) = upper(?2)
              LIMIT 1",
        )?;
        let mut rows = stmt.query(rusqlite::params![outlet, code])?;
        match rows.next()? {
            Some(row) => Ok(Some((row.get(0)?, row.get(1)?))),
            None => Ok(None),
        }
    }

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
    tax_class_id: String,
    price_basis: Option<String>,
    hsn: Option<String>,
    cost_price: Option<i64>,
    short_code: Option<String>,
    prep_minutes: Option<i64>,
    course: Option<String>,
    is_open_price: i64,
    is_available: i64,
    sort_order: i64,
}
