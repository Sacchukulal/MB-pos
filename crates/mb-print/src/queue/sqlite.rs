//! The durable store — **and the one module in this crate that touches a row.**
//!
//! # Decision D32, narrowly
//!
//! P06 wrote in the crate documentation that this crate does not touch the
//! database, and it was right about everything it was describing: the layout,
//! the templates and the sinks still take structs and return bytes, and they
//! always will.
//!
//! A print queue is different, and audit D4 is why: *"a failed print is only a
//! red message on screen. Nothing remembers it."* A queue that cannot survive a
//! power cut is not a queue, it is a list. So mb-print depends on mb-db —
//! downhill, mb-core then mb-db then mb-print, nothing upside down — and the
//! dependency is confined to this file.
//!
//! mb-db's half of it knows nothing about printing: `payload` is TEXT and its
//! repository neither reads nor validates it.

use std::sync::Arc;

use mb_core::{BusinessDay, Timestamp};
use mb_db::{Db, DbError, Repos};

use crate::queue::store::{JobStore, StoreError, StoredJob};

/// The print spool, in the shop's own database file.
#[derive(Debug)]
pub struct SqliteStore {
    db: Arc<Db>,
    outlet: String,
}

impl SqliteStore {
    #[must_use]
    pub fn new(db: Arc<Db>, outlet: impl Into<String>) -> SqliteStore {
        SqliteStore {
            db,
            outlet: outlet.into(),
        }
    }
}

fn write(e: DbError) -> StoreError {
    StoreError::Write(e.to_string())
}

fn read(e: DbError) -> StoreError {
    StoreError::Read(e.to_string())
}

impl JobStore for SqliteStore {
    fn save(&self, job: &StoredJob) -> Result<(), StoreError> {
        // One transaction, one commit, one fsync — and it is inside budget B6's
        // 50 ms, which is measured with this store and a real file rather than
        // with the memory one, because on the reference machine's 5400 rpm disk
        // the fsync *is* the measurement.
        self.db
            .transaction(|tx| {
                Repos::new(tx).print_jobs().save(
                    &self.outlet,
                    &to_row(job),
                    Timestamp::from_millis(job.created_at),
                )
            })
            .map_err(write)
    }

    fn update(
        &self,
        id: &str,
        state: &str,
        attempts: i64,
        last_error: Option<&str>,
        engine_used: Option<&str>,
    ) -> Result<(), StoreError> {
        let at = Timestamp::from_millis(now_millis());
        self.db
            .transaction(|tx| {
                Repos::new(tx)
                    .print_jobs()
                    .update(id, state, attempts, last_error, engine_used, at)
            })
            .map_err(write)
    }

    fn remove(&self, id: &str) -> Result<(), StoreError> {
        self.db
            .transaction(|tx| Repos::new(tx).print_jobs().remove(id))
            .map_err(write)
    }

    fn unfinished(&self) -> Result<Vec<StoredJob>, StoreError> {
        self.db
            .transaction(|tx| Repos::new(tx).print_jobs().unfinished(&self.outlet))
            .map_err(read)
            .map(|rows| rows.iter().map(from_row).collect())
    }
}

fn to_row(job: &StoredJob) -> mb_db::repo::PrintJobRow {
    mb_db::repo::PrintJobRow {
        id: job.id.clone(),
        printer_id: job.printer_id.clone(),
        kind: job.kind.clone(),
        state: job.state.clone(),
        copies: job.copies,
        priority: job.priority,
        attempts: job.attempts,
        payload: job.payload.clone(),
        reason: job.reason.clone(),
        last_error: job.last_error.clone(),
        engine_used: job.engine_used.clone(),
        business_day: BusinessDay::from_days_since_epoch(
            i32::try_from(job.business_day).unwrap_or(0),
        ),
        created_at: Timestamp::from_millis(job.created_at),
    }
}

fn from_row(row: &mb_db::repo::PrintJobRow) -> StoredJob {
    StoredJob {
        id: row.id.clone(),
        printer_id: row.printer_id.clone(),
        kind: row.kind.clone(),
        state: row.state.clone(),
        copies: row.copies,
        priority: row.priority,
        attempts: row.attempts,
        payload: row.payload.clone(),
        reason: row.reason.clone(),
        last_error: row.last_error.clone(),
        engine_used: row.engine_used.clone(),
        business_day: i64::from(row.business_day.days_since_epoch()),
        created_at: row.created_at.millis(),
    }
}

/// Wall-clock milliseconds.
///
/// The queue reads the clock for `updated_at` and for nothing else. D5's rule —
/// a business day is stamped once by whoever created the thing — is obeyed by
/// carrying `business_day` on the job from the moment it was made.
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
