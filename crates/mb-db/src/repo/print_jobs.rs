//! The print spool — audit **D4**, and the one repository that is deliberately
//! not like the others.
//!
//! > *"A failed print is only a red message on screen. Nothing remembers it. In
//! > a rush the cashier misses the toast and the kitchen simply never gets the
//! > order."*
//!
//! A job is written here before the caller is let go, so a power cut in the
//! middle of a rush loses no kitchen ticket. mb-print's queue owns what a job
//! *means*; this file only owns the row.
//!
//! # Two things this repository does not do, on purpose
//!
//! **It does not enqueue an outbox row.** Every other repository in this crate
//! does, and somebody will eventually notice and "fix" it — so: a print job is
//! local to one counter, it is worthless to a phone or to the cloud, and syncing
//! one would spend real egress from D16's 10 MB monthly budget on a row nobody
//! will ever read. Requirement 3 of the ten says billing continues with no
//! cloud; the print queue is part of the machinery that makes that true, not
//! something the cloud has an opinion about.
//!
//! **It does not read `payload`.** That column is a `mb-print` `Document` as
//! JSON and this crate has no idea what one is. Storage stores; printing
//! prints. Decision D32 draws that line, and it is the reason mb-print may
//! depend on mb-db and not the other way round.

use mb_core::{BusinessDay, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;

/// One unfinished print job, exactly as the row holds it.
///
/// Strings rather than enums: the states and kinds belong to mb-print, and a
/// second copy of them here would be two lists to keep in step. The CHECK
/// constraints in the schema are what stops a typo reaching the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintJobRow {
    pub id: String,
    pub printer_id: String,
    pub kind: String,
    pub state: String,
    pub copies: i64,
    pub priority: i64,
    pub attempts: i64,
    /// The document, as JSON. Opaque here.
    pub payload: String,
    pub reason: Option<String>,
    pub last_error: Option<String>,
    pub engine_used: Option<String>,
    pub business_day: BusinessDay,
    pub created_at: Timestamp,
}

#[derive(Debug)]
pub struct PrintJobRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> PrintJobRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        PrintJobRepo { tx }
    }

    /// Write a job durably. The queue calls this before it tells the caller the
    /// ticket is on its way, which is the whole of D4's fix.
    pub fn save(&self, outlet: &str, job: &PrintJobRow, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO print_jobs (id, outlet_id, printer_id, kind, state, copies, priority,
                                     attempts, payload, reason, last_error, engine_used,
                                     business_day, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)
             ON CONFLICT (id) DO UPDATE SET state       = excluded.state,
                                            attempts    = excluded.attempts,
                                            last_error  = excluded.last_error,
                                            engine_used = excluded.engine_used,
                                            updated_at  = excluded.updated_at",
            rusqlite::params![
                job.id,
                outlet,
                job.printer_id,
                job.kind,
                job.state,
                job.copies,
                job.priority,
                job.attempts,
                job.payload,
                job.reason,
                job.last_error,
                job.engine_used,
                encode::business_day_to_sql(job.business_day),
                encode::timestamp_to_sql(at),
            ],
        )?;
        // NO OUTBOX ROW. See the module documentation — this is deliberate.
        Ok(())
    }

    /// Record what happened to a job, without rewriting its payload.
    pub fn update(
        &self,
        id: &str,
        state: &str,
        attempts: i64,
        last_error: Option<&str>,
        engine_used: Option<&str>,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "UPDATE print_jobs
                SET state = ?2, attempts = ?3, last_error = ?4,
                    engine_used = COALESCE(?5, engine_used), updated_at = ?6
              WHERE id = ?1",
            rusqlite::params![
                id,
                state,
                attempts,
                last_error,
                engine_used,
                encode::timestamp_to_sql(at),
            ],
        )?;
        Ok(())
    }

    /// **A job that printed has no row (D35).**
    ///
    /// This is the line that keeps budget M5 intact: the payload is kilobytes,
    /// a shop prints two of them per bill, and a year is 150,000 of them.
    pub fn remove(&self, id: &str) -> Result<(), DbError> {
        self.tx
            .execute("DELETE FROM print_jobs WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Everything still waiting, most urgent first.
    ///
    /// What the queue reads at start-up to resume after a crash, and what the
    /// cashier's screen shows. Ordered by priority and then by age, so a bill
    /// does not sit behind forty kitchen tickets.
    pub fn unfinished(&self, outlet: &str) -> Result<Vec<PrintJobRow>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, printer_id, kind, state, copies, priority, attempts, payload,
                    reason, last_error, engine_used, business_day, created_at
               FROM print_jobs
              WHERE outlet_id = ?1
              ORDER BY priority, created_at",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, i64>(11)?,
                row.get::<_, i64>(12)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (
                id,
                printer_id,
                kind,
                state,
                copies,
                priority,
                attempts,
                payload,
                reason,
                last_error,
                engine_used,
                day,
                created,
            ) = row?;
            out.push(PrintJobRow {
                id,
                printer_id,
                kind,
                state,
                copies,
                priority,
                attempts,
                payload,
                reason,
                last_error,
                engine_used,
                business_day: encode::business_day_from_sql(day, "print_jobs.business_day")?,
                created_at: encode::timestamp_from_sql(created),
            });
        }
        Ok(out)
    }

    /// How many jobs are outstanding. The number behind the cashier's indicator,
    /// and what a test asserts is zero after a healthy evening (D35).
    pub fn count(&self, outlet: &str) -> Result<i64, DbError> {
        let mut stmt = self
            .tx
            .prepare_cached("SELECT COUNT(*) FROM print_jobs WHERE outlet_id = ?1")?;
        let count: i64 = stmt.query_row([outlet], |row| row.get(0))?;
        Ok(count)
    }

    /// How much paper is still addressed to this printer.
    ///
    /// P17 asks before removing one. The foreign key would refuse anyway; the
    /// point of asking first is that "there are still 3 print jobs against this
    /// printer" is a sentence and `FOREIGN KEY constraint failed` is not
    /// (audit F8).
    pub fn count_for_printer(&self, printer_id: &str) -> Result<i64, DbError> {
        let mut stmt = self
            .tx
            .prepare_cached("SELECT COUNT(*) FROM print_jobs WHERE printer_id = ?1")?;
        let count: i64 = stmt.query_row([printer_id], |row| row.get(0))?;
        Ok(count)
    }
}
