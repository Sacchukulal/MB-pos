//! The sync outbox.

use mb_core::{BusinessDay, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;

/// What happened to the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Upsert,
    Delete,
}

impl Op {
    const fn as_sql(self) -> &'static str {
        match self {
            Op::Upsert => "upsert",
            Op::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRow {
    pub id: String,
    pub table_name: String,
    pub row_id: String,
    pub op: Op,
    /// Present only for a delete — there is nothing left to read for those.
    pub tombstone: Option<String>,
    pub created_at: Timestamp,
    pub attempts: i64,
}

/// Derived from the table and the row, never from the clock — see
/// `OutboxRepo::enqueue_with_tombstone` for why.
fn outbox_id(table: &str, row_id: &str) -> String {
    format!("ob\u{1f}{table}\u{1f}{row_id}")
}

#[derive(Debug)]
pub struct OutboxRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> OutboxRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        OutboxRepo { tx }
    }

    /// Queue a row for the cloud.
    pub fn enqueue(
        &self,
        outlet: &str,
        table: &str,
        row_id: &str,
        op: Op,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.enqueue_with_tombstone(outlet, table, row_id, op, None, at)
    }

    /// A delete carries a tombstone, because there is nothing left to read.
    pub fn enqueue_with_tombstone(
        &self,
        outlet: &str,
        table: &str,
        row_id: &str,
        op: Op,
        tombstone: Option<&str>,
        at: Timestamp,
    ) -> Result<(), DbError> {
        if op == Op::Upsert && tombstone.is_some() {
            return Err(DbError::invariant(
                "an upsert must not carry a tombstone — the sender reads the live row",
            ));
        }
        // One pending row per business row, coalesced.
        self.tx.execute(
            "INSERT INTO sync_outbox (id, outlet_id, table_name, row_id, op, tombstone, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET op         = excluded.op,
                                            tombstone  = excluded.tombstone,
                                            created_at = excluded.created_at,
                                            synced_at  = NULL,
                                            attempts   = 0,
                                            last_error = NULL",
            rusqlite::params![
                outbox_id(table, row_id),
                outlet,
                table,
                row_id,
                op.as_sql(),
                tombstone,
                encode::timestamp_to_sql(at),
            ],
        )?;
        Ok(())
    }

    /// The backlog, oldest first, capped.
    pub fn pending(&self, limit: usize) -> Result<Vec<OutboxRow>, DbError> {
        self.pending_in(None, limit)
    }

    /// The backlog for one table, oldest first, capped.
    pub fn pending_in(&self, table: Option<&str>, limit: usize) -> Result<Vec<OutboxRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT id, table_name, row_id, op, tombstone, created_at, attempts
               FROM sync_outbox
              WHERE synced_at IS NULL
                AND (?1 IS NULL OR table_name = ?1)
              ORDER BY created_at
              LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![table, i64::try_from(limit).unwrap_or(i64::MAX)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )?;

        let mut out = Vec::new();
        for row in rows {
            let (id, table_name, row_id, op, tombstone, created_at, attempts) = row?;
            let op = match op.as_str() {
                "upsert" => Op::Upsert,
                "delete" => Op::Delete,
                other => {
                    return Err(DbError::BadValue {
                        column: "sync_outbox.op",
                        value: other.to_owned(),
                    });
                }
            };
            out.push(OutboxRow {
                id,
                table_name,
                row_id,
                op,
                tombstone,
                created_at: encode::timestamp_from_sql(created_at),
                attempts,
            });
        }
        Ok(out)
    }

    /// A row the cloud has: its entry leaves the outbox. Refused rows leave too — a refusal is
    /// wrong, not late, and the sender keeps the last sentence for Health.
    pub fn mark_synced(&self, ids: &[&str], _at: Timestamp) -> Result<(), DbError> {
        let mut stmt = self.tx.prepare_cached("DELETE FROM sync_outbox WHERE id = ?1")?;
        for id in ids {
            stmt.execute([id])?;
        }
        Ok(())
    }

    /// A failure bumps the attempt count and records why.
    pub fn record_failure(&self, id: &str, error: &str) -> Result<(), DbError> {
        self.tx.execute(
            "UPDATE sync_outbox SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
            rusqlite::params![id, error],
        )?;
        Ok(())
    }

    /// Everything that is still waiting.
    pub fn pending_count(&self) -> Result<i64, DbError> {
        Ok(self.tx.query_row(
            "SELECT count(*) FROM sync_outbox WHERE synced_at IS NULL",
            [],
            |r| r.get(0),
        )?)
    }

    /// The id an entry for this row has, derived the same way `enqueue` derives it.
    #[must_use]
    pub fn entry_id(table: &str, row_id: &str) -> String {
        outbox_id(table, row_id)
    }

    /// Everything is already in the cloud — a shop just brought down from it.
    pub fn clear_backlog(&self, _at: Timestamp) -> Result<usize, DbError> {
        let n = self.tx.execute("DELETE FROM sync_outbox", [])?;
        Ok(n)
    }

    /// Queue the whole shop from its live rows, freshly stamped: every synced table, the bills
    /// of the days that are not yet sealed (a sealed day travels in its day file), the totals
    /// of those days, and the counters. The ONE re-queue: a shop that billed before it had a
    /// cloud, a pen-drive restore, and a licence moved to a new PC all call this.
    pub fn queue_shop(&self, outlet: &str, unsealed_from: BusinessDay, at: Timestamp) -> Result<usize, DbError> {
        let at_sql = encode::timestamp_to_sql(at);
        let mut n = 0_usize;
        for (table, key) in SYNCED {
            n = n.saturating_add(self.tx.execute(
                &format!(
                    "INSERT INTO sync_outbox (id, outlet_id, table_name, row_id, op, tombstone, created_at)
                     SELECT 'ob' || char(31) || '{table}' || char(31) || \"{key}\", ?1, '{table}', \"{key}\", 'upsert', NULL, ?2
                       FROM \"{table}\"
                      WHERE true
                     ON CONFLICT (id) DO UPDATE SET op = 'upsert', tombstone = NULL, created_at = excluded.created_at,
                                                    synced_at = NULL, attempts = 0, last_error = NULL"
                ),
                rusqlite::params![outlet, at_sql],
            )?);
        }
        let from = encode::business_day_to_sql(unsealed_from);
        n = n.saturating_add(self.tx.execute(
            "INSERT INTO sync_outbox (id, outlet_id, table_name, row_id, op, tombstone, created_at)
             SELECT 'ob' || char(31) || 'orders' || char(31) || id, ?1, 'orders', id, 'upsert', NULL, ?2
               FROM orders
              WHERE outlet_id = ?1 AND business_day >= ?3
                AND (state IN ('settled', 'voided') OR (state = 'cancelled' AND bill_number_value IS NOT NULL))
             ON CONFLICT (id) DO UPDATE SET op = 'upsert', tombstone = NULL, created_at = excluded.created_at,
                                            synced_at = NULL, attempts = 0, last_error = NULL",
            rusqlite::params![outlet, at_sql, from],
        )?);
        for table in crate::repo::wire::TOTALS_TABLES {
            n = n.saturating_add(self.tx.execute(
                &format!(
                    "INSERT INTO sync_outbox (id, outlet_id, table_name, row_id, op, tombstone, created_at)
                     SELECT DISTINCT 'ob' || char(31) || '{table}' || char(31) || business_day, ?1, '{table}', CAST(business_day AS TEXT), 'upsert', NULL, ?2
                       FROM orders
                      WHERE outlet_id = ?1 AND business_day >= ?3 AND state IN ('settled', 'voided')
                     ON CONFLICT (id) DO UPDATE SET op = 'upsert', tombstone = NULL, created_at = excluded.created_at,
                                                    synced_at = NULL, attempts = 0, last_error = NULL"
                ),
                rusqlite::params![outlet, at_sql, from],
            )?);
        }
        n = n.saturating_add(crate::numbering::queue_all(self.tx, at)?);
        Ok(n)
    }
}

/// Every table the counter copies to the cloud whole, with the column that names a row.
/// Bills, totals and counters are queued by their own rules above.
pub const SYNCED: &[(&str, &str)] = &[
    ("expenses", "id"),
    ("expense_categories", "id"),
    ("cash_movements", "id"),
    ("customers", "id"),
    ("credit_adjustments", "id"),
    ("customer_payments", "id"),
    ("categories", "id"),
    ("items", "id"),
    ("item_variants", "id"),
    ("modifier_groups", "id"),
    ("combos", "id"),
    ("staff", "id"),
    ("roles", "id"),
    ("settings", "key"),
    ("store_profile", "outlet_id"),
    ("printers", "id"),
    ("category_printers", "category_id"),
    ("sections", "id"),
    ("dining_tables", "id"),
    ("terminals", "id"),
    ("lan_devices", "id"),
    ("tax_classes", "id"),
    ("reasons", "id"),
    ("reprints", "id"),
    ("refunds", "id"),
    ("bill_reverts", "id"),
    ("recurring_expenses", "id"),
    ("day_closes", "id"),
    ("materials", "id"),
    ("recipes", "id"),
    ("suppliers", "id"),
    ("purchases", "id"),
    ("purchase_orders", "id"),
    ("supplier_payments", "id"),
    ("supplier_adjustments", "id"),
    ("stock_counts", "id"),
];

/// Which column names a row in a table the outbox carries whole.
#[must_use]
pub fn key_column(table: &str) -> &'static str {
    SYNCED
        .iter()
        .find(|(t, _)| *t == table)
        .map_or("id", |(_, key)| key)
}
