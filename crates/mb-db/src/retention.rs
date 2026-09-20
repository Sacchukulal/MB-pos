//! What the counter keeps for a while, and what it keeps forever.
//!
//! Every money table is kept forever — a bill, its lines, its payments, a refund, a revert, a
//! ledger line. Four tables are logs: what a phone asked (`applied_events`), what happened to
//! an order (`order_events`), what a payment provider said (`payment_attempts`) and what the
//! kitchen was told (`kitchen_ledger`). A log older than its retention is gone. The list is
//! the whole rule: a table not in it cannot be pruned by this module, whatever a caller says.

use mb_core::Timestamp;
use rusqlite::Transaction;

use crate::conn::Db;
use crate::error::DbError;

/// The four logs: table, its time column, days kept.
pub const RETENTION: &[(&str, &str, u32)] = &[
    // The LAN's idempotency record: a phone retries within minutes, never within days.
    ("applied_events", "applied_at", 7),
    ("order_events", "at", 7),
    ("payment_attempts", "at", 90),
    ("kitchen_ledger", "updated_at", 90),
];

/// Rows deleted per statement, so the writer is never held long.
const BATCH: usize = 5_000;


/// What one run deleted, table by table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pruned {
    pub rows: Vec<(String, usize)>,
}

impl Pruned {
    #[must_use]
    pub fn total(&self) -> usize {
        self.rows.iter().map(|(_, n)| n).sum()
    }
}

/// Delete what is older than its retention from every table in `RETENTION`, in bounded
/// batches, one transaction per batch, then write the WAL back into the file.
pub fn prune(db: &Db, now: Timestamp) -> Result<Pruned, DbError> {
    let mut pruned = Pruned::default();
    for (table, column, days) in RETENTION {
        let before = now
            .millis()
            .saturating_sub(i64::from(*days).saturating_mul(86_400_000));
        let mut total = 0_usize;
        loop {
            let n = db.transaction(|tx| prune_batch(tx, table, column, before))?;
            total = total.saturating_add(n);
            if n < BATCH {
                break;
            }
            // Between batches the writer is free: a bill being settled goes first.
            std::thread::yield_now();
        }
        pruned.rows.push(((*table).to_owned(), total));
    }
    db.checkpoint()?;
    Ok(pruned)
}

/// One batch from one table. Refuses a table that is not in the list, so no caller can ever
/// point this at a money table.
fn prune_batch(tx: &Transaction<'_>, table: &str, column: &str, before_ms: i64) -> Result<usize, DbError> {
    if !RETENTION.iter().any(|(t, c, _)| *t == table && *c == column) {
        return Err(DbError::invariant(format!(
            "{table} is not a log table — nothing outside the retention list is ever pruned"
        )));
    }
    let n = tx.execute(
        &format!(
            "DELETE FROM \"{table}\" WHERE rowid IN (SELECT rowid FROM \"{table}\" WHERE \"{column}\" < ?1 LIMIT ?2)"
        ),
        rusqlite::params![before_ms, i64::try_from(BATCH).unwrap_or(i64::MAX)],
    )?;
    Ok(n)
}
