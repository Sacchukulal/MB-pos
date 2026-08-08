//! The sync outbox — audit A1, A2 and A3, in one table.
//!
//! v1's outbox knew about bills. That is why the owner's phone shows ₹0
//! expenses forever (A2), why credit repayments have never been backed up (A3),
//! and why A1 says the shop's real asset lives on one hard disk.
//!
//! **The one rule: enqueueing happens in the SAME transaction as the write it
//! describes.** A row written without its outbox entry is a row that never
//! reaches the cloud and nobody ever finds out. Both A2 and A3 were exactly one
//! forgotten enqueue.

use mb_core::Timestamp;
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

/// One pending change, as P33 will read it.
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
/// [`OutboxRepo::enqueue_with_tombstone`] for why.
///
/// The unit separator cannot occur in a table name or in an id, so two
/// different rows cannot produce the same entry.
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
    ///
    /// **No payload is stored for an upsert.** The sender reads the row at send
    /// time, which halves the write, keeps budget M5 down, and means a row
    /// edited five times between connections syncs ONCE instead of five times.
    /// That is D16's 10 MB monthly egress budget, decided here rather than at
    /// P33.
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
    /// That is the exception that proves the rule above.
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
        // **One pending row per business row, coalesced.** The id is derived
        // from the table and the row rather than from the clock, so editing an
        // item five times before the next connection leaves ONE entry and
        // syncs ONCE. That is D16's 10 MB monthly egress budget, and it is
        // decided here rather than at P33 — a clock-derived id would give five
        // rows and five sends of the same final state.
        //
        // Re-queuing after a sync is deliberate too: `synced_at` goes back to
        // NULL, because the row has changed since the cloud last saw it.
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
        let mut stmt = self.tx.prepare(
            "SELECT id, table_name, row_id, op, tombstone, created_at, attempts
               FROM sync_outbox
              WHERE synced_at IS NULL
              ORDER BY created_at
              LIMIT ?1",
        )?;
        let rows = stmt.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;

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

    pub fn mark_synced(&self, ids: &[&str], at: Timestamp) -> Result<(), DbError> {
        let mut stmt = self
            .tx
            .prepare_cached("UPDATE sync_outbox SET synced_at = ?2 WHERE id = ?1")?;
        for id in ids {
            stmt.execute(rusqlite::params![id, encode::timestamp_to_sql(at)])?;
        }
        Ok(())
    }

    /// A failure bumps the attempt count and records why. P33 owns the backoff
    /// and the ceiling; this only remembers.
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

    /// Mark **every** outbox row pending again.
    ///
    /// Called after a restore, and the reasoning matters enough to write down:
    ///
    /// A backup taken on Tuesday and restored on Friday brings Tuesday's outbox
    /// with it. Two things are wrong at once — rows the cloud already has are
    /// queued again, and rows written between Tuesday and Friday are gone from
    /// the counter but still in the cloud. The counter has just lost three days
    /// and does not know what the cloud holds, so **the only safe assumption is
    /// that nothing is in step.**
    ///
    /// That is affordable because P33's sync is idempotent by construction
    /// (scope 17.3, and the cloud upserts by id), so re-sending a row the cloud
    /// already has costs egress and nothing else — and because the outbox
    /// stores no payload, a full re-queue is one narrow row per business row,
    /// not a second copy of the shop.
    ///
    /// Rows the cloud has and the counter does not are P33's reconciliation
    /// problem, not this crate's.
    /// The id of the one pending entry for a row.
    ///
    /// Public so P33 can address an entry it has just sent without carrying the
    /// string around, and so the coalescing rule is stated once.
    #[must_use]
    pub fn entry_id(table: &str, row_id: &str) -> String {
        outbox_id(table, row_id)
    }

    pub fn requeue_everything(&self) -> Result<usize, DbError> {
        let n = self.tx.execute(
            "UPDATE sync_outbox SET synced_at = NULL, attempts = 0, last_error = NULL",
            [],
        )?;
        Ok(n)
    }
}
