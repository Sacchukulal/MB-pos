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
//! same transaction. **And it cannot reach a bill**: a line froze its own
//! [`TaxSpec`](mb_core::TaxSpec) when it was added (crown jewel 4, D52), so a
//! past bill and an order that is already open both keep what they were billed
//! at. That
//! is P13's T2 and T3, and it is true by construction rather than by care.
//!
//! # P33 — the per-order-type override is gone
//!
//! mb-core deleted `by_order_type`, `OrderTypeRate`, `with_override` and
//! `for_order_type`: they were modelled, given a `tax_class_rates` table,
//! written by this repository — and read by no caller anywhere in the product
//! (audit §3.5). The belief behind them, that parcel is taxed differently from
//! dine-in, is not current law either. Migration 0004 dropped the table.
//!
//! What a class carries instead is the whole [`TaxSpec`](mb_core::TaxSpec) —
//! kind, rate and pricing basis, in `kind`, `rate_bp` and `basis` — so it says
//! *"outside GST, at 20% state VAT, priced tax-in"*, the sentence a bar needs.

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

        // **And the live menu follows.** Not past bills, and not the lines
        // already on an open order — those froze their own copies.
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

        OutboxRepo::new(self.tx).enqueue(outlet, "tax_classes", class.id.as_str(), Op::Upsert, at)?;
        Ok(repriced)
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
