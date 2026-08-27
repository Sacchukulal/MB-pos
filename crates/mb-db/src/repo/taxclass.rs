//! The shop's tax vocabulary, and what happens when it changes.

use mb_auth::audit::action;
use mb_core::{TaxClass, TaxClassId, Timestamp};
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

    pub fn list(&self, outlet: &str) -> Result<Vec<TaxClass>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, rate_bp, kind, basis, is_active FROM tax_classes
              WHERE outlet_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;

        let mut classes = Vec::new();
        for row in rows {
            let (id, name, rate_bp, kind, basis, active) = row?;
            let mut class = TaxClass::new(
                TaxClassId::new(id),
                name,
                encode::tax_spec_from_sql_parts(rate_bp, &kind, &basis, "tax_classes.rate_bp")?,
            );
            class.is_active = encode::bool_from_sql(active, "tax_classes.is_active")?;
            classes.push(class);
        }
        Ok(classes)
    }

    pub fn find(&self, outlet: &str, id: &TaxClassId) -> Result<Option<TaxClass>, DbError> {
        Ok(self.list(outlet)?.into_iter().find(|c| c.id == *id))
    }

    /// Save a class, and bring every item that points at it up to date.
    pub fn save(&self, outlet: &str, class: &TaxClass, at: Timestamp) -> Result<usize, DbError> {
        self.tx.execute(
            "INSERT INTO tax_classes (id, outlet_id, name, rate_bp, kind, basis, is_active,
                                      sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
             ON CONFLICT (id) DO UPDATE SET name      = excluded.name,
                                            rate_bp   = excluded.rate_bp,
                                            kind      = excluded.kind,
                                            basis     = excluded.basis,
                                            is_active = excluded.is_active",
            rusqlite::params![
                class.id.as_str(),
                outlet,
                class.name,
                encode::tax_rate_to_sql(class.tax.rate),
                encode::tax_kind_to_sql(class.tax.kind),
                encode::price_basis_to_sql(class.tax.basis),
                encode::bool_to_sql(class.is_active),
            ],
        )?;

        // And the live menu follows.
        let repriced = self.tx.execute(
            "UPDATE items SET tax_rate_bp = ?2, tax_kind = ?3, tax_basis = ?4, updated_at = ?5
              WHERE tax_class_id = ?1",
            rusqlite::params![
                class.id.as_str(),
                encode::tax_rate_to_sql(class.tax.rate),
                encode::tax_kind_to_sql(class.tax.kind),
                encode::price_basis_to_sql(class.tax.basis),
                encode::timestamp_to_sql(at),
            ],
        )?;

        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "tax_classes",
            class.id.as_str(),
            Op::Upsert,
            at,
        )?;
        Ok(repriced)
    }

    /// How many items point at this class.
    pub fn items_using(&self, id: &TaxClassId) -> Result<i64, DbError> {
        Ok(self.tx.query_row(
            "SELECT COUNT(*) FROM items WHERE tax_class_id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )?)
    }

    /// The action name a menu change is recorded under, so every caller writes the same one.
    pub const CHANGED: &'static str = action::PRICE_CHANGED;
}
