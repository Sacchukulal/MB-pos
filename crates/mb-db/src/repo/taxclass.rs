//! **The shop's tax vocabulary, and what happens when it changes.**
//!
//! > Audit **B10 / B11 / B14**: v1 had one tax rate for the whole shop, so
//! > *"it could not bill a bar, an AC/non-AC outlet or anyone selling packaged
//! > goods."*
//!
//! `mb-core`'s [`TaxClass`] is the rule; this is where a shop's own classes
//! live, and where the one interesting operation happens.
//!
//! # Editing a class rewrites the live menu and nothing else
//!
//! An item carries **both** the class it points at and the rate that choice
//! resolves to (`items.tax_class_id` and `items.tax_rate_bp`). The second is
//! denormalised on purpose — the billing path never joins to work out a rate,
//! the same reason `payments.business_day` is denormalised.
//!
//! So [`TaxClassRepo::save`] rewrites every item pointing at the class, in the
//! same transaction. **And it cannot reach a bill**: a line froze its own rate
//! and treatment when it was added (crown jewel 4, D52), so a past bill and an
//! order that is already open both keep the numbers they were billed at. That
//! is P13's T2 and T3, and it is true by construction rather than by care.

use mb_auth::audit::action;
use mb_core::{OrderType, TaxClass, TaxClassId, TaxRate, TaxTreatment, Timestamp};
use mb_core::taxclass::OrderTypeRate;
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
            "SELECT id, name, rate_bp, treatment, is_active FROM tax_classes
              WHERE outlet_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;

        let mut classes = Vec::new();
        for row in rows {
            let (id, name, rate_bp, treatment, active) = row?;
            let mut class = TaxClass::new(
                TaxClassId::new(id.clone()),
                name,
                encode::tax_rate_from_sql(rate_bp, "tax_classes.rate_bp")?,
                encode::tax_treatment_from_sql(&treatment)?,
            );
            class.is_active = encode::bool_from_sql(active, "tax_classes.is_active")?;
            class.by_order_type = self.overrides_for(&id)?;
            classes.push(class);
        }
        Ok(classes)
    }

    fn overrides_for(&self, class_id: &str) -> Result<Vec<OrderTypeRate>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT order_type, rate_bp, treatment FROM tax_class_rates
              WHERE class_id = ?1 ORDER BY order_type",
        )?;
        let rows = stmt.query_map([class_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (order_type, rate_bp, treatment) = row?;
            out.push(OrderTypeRate {
                order_type: encode::order_type_from_sql(&order_type)?,
                rate: encode::tax_rate_from_sql(rate_bp, "tax_class_rates.rate_bp")?,
                treatment: encode::tax_treatment_from_sql(&treatment)?,
            });
        }
        Ok(out)
    }

    pub fn find(&self, outlet: &str, id: &TaxClassId) -> Result<Option<TaxClass>, DbError> {
        Ok(self.list(outlet)?.into_iter().find(|c| c.id == *id))
    }

    /// Save a class, **and bring every item that points at it up to date.**
    ///
    /// Returns how many items were repriced, so the screen can say *"14 items
    /// now charge 18%"* rather than leaving an owner to wonder what a rate
    /// change did.
    pub fn save(
        &self,
        outlet: &str,
        class: &TaxClass,
        at: Timestamp,
    ) -> Result<usize, DbError> {
        self.tx.execute(
            "INSERT INTO tax_classes (id, outlet_id, name, rate_bp, treatment, is_active,
                                      sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)
             ON CONFLICT (id) DO UPDATE SET name      = excluded.name,
                                            rate_bp   = excluded.rate_bp,
                                            treatment = excluded.treatment,
                                            is_active = excluded.is_active",
            rusqlite::params![
                class.id.as_str(),
                outlet,
                class.name,
                encode::tax_rate_to_sql(class.rate),
                encode::tax_treatment_to_sql(class.treatment),
                encode::bool_to_sql(class.is_active),
            ],
        )?;

        // The overrides are replaced wholesale — four rows at most, and a diff
        // is where "the box looked unticked but the row was still there" comes
        // from (the same argument `save_role` makes).
        self.tx
            .execute("DELETE FROM tax_class_rates WHERE class_id = ?1", [class.id.as_str()])?;
        for over in &class.by_order_type {
            self.tx.execute(
                "INSERT INTO tax_class_rates (class_id, order_type, rate_bp, treatment)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    class.id.as_str(),
                    encode::order_type_to_sql(over.order_type),
                    encode::tax_rate_to_sql(over.rate),
                    encode::tax_treatment_to_sql(over.treatment),
                ],
            )?;
        }

        // **And the live menu follows.** Not past bills, and not the lines
        // already on an open order — those froze their own copies.
        let repriced = self.tx.execute(
            "UPDATE items SET tax_rate_bp = ?2, tax_treatment = ?3, updated_at = ?4
              WHERE tax_class_id = ?1",
            rusqlite::params![
                class.id.as_str(),
                encode::tax_rate_to_sql(class.rate),
                encode::tax_treatment_to_sql(class.treatment),
                encode::timestamp_to_sql(at),
            ],
        )?;

        OutboxRepo::new(self.tx).enqueue(outlet, "tax_classes", class.id.as_str(), Op::Upsert, at)?;
        Ok(repriced)
    }

    /// What a class means for one kind of order — the rule, read from storage.
    ///
    /// It exists here as well as on `TaxClass` because the caller usually has
    /// an id and an order type, and making it fetch the whole class first would
    /// be three lines at every call site.
    pub fn resolve(
        &self,
        outlet: &str,
        id: &TaxClassId,
        kind: OrderType,
    ) -> Result<Option<(TaxRate, TaxTreatment)>, DbError> {
        Ok(self.find(outlet, id)?.map(|class| class.for_order_type(kind)))
    }

    /// How many items point at this class. **Nothing in the menu is deleted**
    /// (P13 item 6), and a class still in use cannot even be retired without
    /// the shop being told what it would affect.
    pub fn items_using(&self, id: &TaxClassId) -> Result<i64, DbError> {
        Ok(self.tx.query_row(
            "SELECT COUNT(*) FROM items WHERE tax_class_id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )?)
    }

    /// The action name a menu change is recorded under, so every caller writes
    /// the same one.
    pub const CHANGED: &'static str = action::PRICE_CHANGED;
}
