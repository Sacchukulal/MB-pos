//! Where an unfinished job lives — **the port, and its two implementations.**
//!
//! # Why a port and not just "write to the database"
//!
//! Because the two moments a shop most needs to print are moments when there is
//! no database open:
//!
//! * **first run.** The owner has installed the counter, plugged in a printer,
//!   and wants a test print before anything else exists;
//! * **after a restore.** D27 is explicit that a restore runs *before*
//!   `Db::open`, against a file, precisely so Windows refuses a restore aimed at
//!   an open database instead of corrupting it.
//!
//! A queue that requires storage cannot help the person whose storage is what
//! went wrong. So durability is a *choice* the caller makes — [`SqliteStore`]
//! when there is a shop, [`MemoryStore`] when there is not — and the queue
//! itself does not know the difference.
//!
//! # Why `StoredJob` is not `mb_db::PrintJobRow`
//!
//! They carry the same fields and they are deliberately separate types. This one
//! is the queue's idea of a job; that one is a row. Fifteen lines of conversion
//! in [`super::sqlite`] is the price of the queue not being shaped by SQLite,
//! and of P08 being able to implement this trait over something else if a
//! terminal ever spools to another terminal (scope 11.1).

use std::collections::BTreeMap;
use std::sync::Mutex;

/// A job as it survives a power cut.
///
/// Everything is a plain value: the payload is JSON, the state and kind are the
/// strings the schema's CHECK constraints allow, and nothing here needs the
/// crate that will print it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredJob {
    pub id: String,
    pub printer_id: String,
    pub kind: String,
    pub state: String,
    pub copies: i64,
    /// Lower is sooner.
    pub priority: i64,
    pub attempts: i64,
    /// The document and its flags, as JSON.
    pub payload: String,
    pub reason: Option<String>,
    pub last_error: Option<String>,
    pub engine_used: Option<String>,
    pub business_day: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("the print queue could not be written to: {0}")]
    Write(String),
    #[error("the print queue could not be read: {0}")]
    Read(String),
}

/// Somewhere unfinished jobs are kept.
pub trait JobStore: Send + Sync + std::fmt::Debug {
    /// Write a job **durably**, before the caller is told the ticket is on its
    /// way. That ordering is the whole of audit D4's fix.
    fn save(&self, job: &StoredJob) -> Result<(), StoreError>;

    /// Record what happened, without rewriting the payload.
    fn update(
        &self,
        id: &str,
        state: &str,
        attempts: i64,
        last_error: Option<&str>,
        engine_used: Option<&str>,
    ) -> Result<(), StoreError>;

    /// **A job that printed has no row** (D35). See the schema note on
    /// `print_jobs`: keeping them would cost more than every bill in the shop.
    fn remove(&self, id: &str) -> Result<(), StoreError>;

    /// Everything still waiting, most urgent first. What the queue reads at
    /// start-up to carry on after a crash.
    fn unfinished(&self) -> Result<Vec<StoredJob>, StoreError>;
}

/// A queue that does not survive a restart, and is the right answer anyway when
/// there is nothing to survive into.
///
/// Used by the test print during first-run setup, by every test in this crate,
/// and by a counter whose database is being restored.
#[derive(Debug, Default)]
pub struct MemoryStore {
    jobs: Mutex<BTreeMap<String, StoredJob>>,
}

impl MemoryStore {
    #[must_use]
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    /// How many jobs are outstanding. The in-memory equivalent of the row count
    /// D35 says must go back to zero.
    #[must_use]
    pub fn len(&self) -> usize {
        lock(&self.jobs).len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl JobStore for MemoryStore {
    fn save(&self, job: &StoredJob) -> Result<(), StoreError> {
        lock(&self.jobs).insert(job.id.clone(), job.clone());
        Ok(())
    }

    fn update(
        &self,
        id: &str,
        state: &str,
        attempts: i64,
        last_error: Option<&str>,
        engine_used: Option<&str>,
    ) -> Result<(), StoreError> {
        if let Some(job) = lock(&self.jobs).get_mut(id) {
            job.state = state.to_owned();
            job.attempts = attempts;
            job.last_error = last_error.map(str::to_owned);
            if engine_used.is_some() {
                job.engine_used = engine_used.map(str::to_owned);
            }
        }
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), StoreError> {
        lock(&self.jobs).remove(id);
        Ok(())
    }

    fn unfinished(&self) -> Result<Vec<StoredJob>, StoreError> {
        let mut out: Vec<StoredJob> = lock(&self.jobs).values().cloned().collect();
        out.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then(a.created_at.cmp(&b.created_at))
        });
        Ok(out)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, priority: i64, created: i64) -> StoredJob {
        StoredJob {
            id: id.to_owned(),
            printer_id: "prn".to_owned(),
            kind: "kitchen".to_owned(),
            state: "pending".to_owned(),
            copies: 1,
            priority,
            attempts: 0,
            payload: "{}".to_owned(),
            reason: None,
            last_error: None,
            engine_used: None,
            business_day: 20_669,
            created_at: created,
        }
    }

    #[test]
    fn a_bill_comes_out_ahead_of_forty_kitchen_tickets() {
        let store = MemoryStore::new();
        for n in 0..40 {
            store.save(&job(&format!("k{n}"), 20, n)).expect("saves");
        }
        store.save(&job("bill", 10, 99)).expect("saves");

        let order = store.unfinished().expect("reads");
        assert_eq!(
            order.first().map(|j| j.id.as_str()),
            Some("bill"),
            "a customer is standing at the counter"
        );
    }

    #[test]
    fn a_finished_job_leaves_nothing_behind() {
        let store = MemoryStore::new();
        store.save(&job("j1", 20, 1)).expect("saves");
        store.remove("j1").expect("removes");
        assert!(store.is_empty(), "D35: the spool is not a log");
    }
}
