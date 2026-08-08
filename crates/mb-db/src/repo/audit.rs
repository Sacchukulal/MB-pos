//! **The audit trail — write it, read it, and check whether it can be
//! believed.**
//!
//! > Audit **C4**: *"No audit trail on the counter. Nothing records who deleted
//! > an item, who changed a price, who reprinted a bill, who edited the bill
//! > counter, who changed a payment mode. The cloud admin panel has a full
//! > audit log; the till has none."*
//!
//! # The rule that makes it an audit trail rather than a log
//!
//! **An entry is written in the SAME transaction as the thing it describes.**
//! That is the outbox's rule for the same reason (audit A2 and A3 were each
//! exactly one forgotten enqueue): a row that can be committed without its
//! audit entry, or an entry without its subject, is not evidence of anything.
//!
//! `settle` is already one transaction (P05, budget B5); the audit row joins
//! it rather than following it.
//!
//! # It does not sync
//!
//! No outbox row, ever. `audit_log` is unbounded, it is the widest row in the
//! product, and nothing on the phone reads it — the same reasoning D16 applied
//! to the print spool in D35. P33 may choose to sync a narrowed projection of
//! it; that is P33's decision to take with the quota in front of it.

use mb_auth::audit::{AuditEntry, AuditRow, Broken, chain_hash, verify_chain};
use mb_core::{BusinessDay, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;

/// What the history screen is asking for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditFilter {
    /// Inclusive, in business days.
    pub from_day: Option<BusinessDay>,
    pub to_day: Option<BusinessDay>,
    pub staff_id: Option<String>,
    pub action: Option<String>,
    /// The screen shows a page, not a year. There is no "all" — a query with no
    /// limit against a table with no ceiling is how a report blocks billing
    /// (§2.3: reports may be slower; they may never slow down billing).
    pub limit: u32,
}

#[derive(Debug)]
pub struct AuditRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> AuditRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        AuditRepo { tx }
    }

    /// Append one entry, and link it to the one before it.
    ///
    /// Returns the `seq` it was given, which is what a test asserts on and what
    /// a support call can be pointed at.
    pub fn append(&self, outlet: &str, entry: &AuditEntry) -> Result<i64, DbError> {
        // MAX(seq) + 1, inside this transaction. Exact because P04 gave this
        // database exactly one writer; see the module note in `conn.rs`.
        let previous: Option<(i64, String)> = self
            .tx
            .query_row(
                "SELECT seq, hash FROM audit_log WHERE outlet_id = ?1
                  ORDER BY seq DESC LIMIT 1",
                [outlet],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .ok();

        let (seq, prev_hash) = match previous {
            Some((last, hash)) => (last + 1, Some(hash)),
            None => (1, None),
        };

        let at = encode::timestamp_to_sql(entry.at);
        let business_day = encode::business_day_to_sql(entry.business_day);
        let staff_id = entry.staff_id.as_ref().map(|s| s.as_str().to_owned());
        let before_json = entry.before.as_ref().map(ToString::to_string);
        let after_json = entry.after.as_ref().map(ToString::to_string);

        let hash = chain_hash(&mb_auth::audit::Chained {
            prev_hash: prev_hash.as_deref(),
            seq,
            at,
            business_day,
            staff_id: staff_id.as_deref(),
            action: entry.action,
            entity: entry.entity,
            entity_id: entry.entity_id.as_deref(),
            before_json: before_json.as_deref(),
            after_json: after_json.as_deref(),
        });

        // The id is derived from the outlet and the sequence rather than from a
        // clock: two entries written in the same millisecond are ordinary, and
        // an id that could collide in the one table nobody may edit afterwards
        // is not a risk worth taking for a shorter string.
        let id = format!("aud_{outlet}_{seq}");

        self.tx.execute(
            "INSERT INTO audit_log
                (id, outlet_id, seq, at, business_day, staff_id, action, entity,
                 entity_id, before_json, after_json, prev_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                id,
                outlet,
                seq,
                at,
                business_day,
                staff_id,
                entry.action,
                entry.entity,
                entry.entity_id,
                before_json,
                after_json,
                prev_hash,
                hash,
            ],
        )?;

        // NO OUTBOX ROW. See the module note.
        Ok(seq)
    }

    /// What the history screen shows, newest first.
    pub fn list(&self, outlet: &str, filter: &AuditFilter) -> Result<Vec<AuditRow>, DbError> {
        let limit = i64::from(filter.limit.max(1));
        let mut stmt = self.tx.prepare_cached(
            "SELECT a.id, a.seq, a.at, a.business_day, a.staff_id, s.name, a.action,
                    a.entity, a.entity_id, a.before_json, a.after_json, a.prev_hash, a.hash
               FROM audit_log a LEFT JOIN staff s ON s.id = a.staff_id
              WHERE a.outlet_id = ?1
                AND (?2 IS NULL OR a.business_day >= ?2)
                AND (?3 IS NULL OR a.business_day <= ?3)
                AND (?4 IS NULL OR a.staff_id = ?4)
                AND (?5 IS NULL OR a.action = ?5)
              ORDER BY a.seq DESC
              LIMIT ?6",
        )?;

        let rows = stmt.query_map(
            rusqlite::params![
                outlet,
                filter.from_day.map(encode::business_day_to_sql),
                filter.to_day.map(encode::business_day_to_sql),
                filter.staff_id,
                filter.action,
                limit,
            ],
            read_row,
        )?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Walk the whole chain and report the first break.
    ///
    /// Reads in `seq` order, because [`verify_chain`] requires it and sorting
    /// inside the verifier would hide a repository bug behind a sort.
    ///
    /// **This is a §2.3 query, not a §2.2 one.** It reads every row a shop has
    /// ever written, so the screen runs it on demand and never on the billing
    /// path.
    pub fn verify(&self, outlet: &str) -> Result<Result<(), Broken>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT a.id, a.seq, a.at, a.business_day, a.staff_id, NULL, a.action,
                    a.entity, a.entity_id, a.before_json, a.after_json, a.prev_hash, a.hash
               FROM audit_log a
              WHERE a.outlet_id = ?1
              ORDER BY a.seq ASC",
        )?;
        let rows = stmt.query_map([outlet], read_row)?;
        let mut all = Vec::new();
        for row in rows {
            all.push(row?);
        }
        Ok(verify_chain(&all))
    }

    /// How many entries this shop has. For the screen's empty state, and for
    /// the M5 measurement.
    pub fn count(&self, outlet: &str) -> Result<i64, DbError> {
        Ok(self.tx.query_row(
            "SELECT COUNT(*) FROM audit_log WHERE outlet_id = ?1",
            [outlet],
            |row| row.get(0),
        )?)
    }

    /// Failed logins by this person since they last got in — **the lockout
    /// count**, and the reason it needs no column of its own.
    ///
    /// It also survives a restart, which an in-memory counter does not, and
    /// which is the first thing anybody trying PINs would discover.
    pub fn failed_logins_since_success(
        &self,
        outlet: &str,
        staff_id: &str,
    ) -> Result<u32, DbError> {
        let last_ok: i64 = self.tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM audit_log
              WHERE outlet_id = ?1 AND staff_id = ?2 AND action = 'login.ok'",
            rusqlite::params![outlet, staff_id],
            |row| row.get(0),
        )?;
        let failures: i64 = self.tx.query_row(
            "SELECT COUNT(*) FROM audit_log
              WHERE outlet_id = ?1 AND staff_id = ?2 AND action = 'login.failed'
                AND seq > ?3",
            rusqlite::params![outlet, staff_id, last_ok],
            |row| row.get(0),
        )?;
        Ok(u32::try_from(failures).unwrap_or(u32::MAX))
    }

    /// When the most recent failure was, so the screen can count down.
    pub fn last_failed_login(
        &self,
        outlet: &str,
        staff_id: &str,
    ) -> Result<Option<Timestamp>, DbError> {
        let at: Option<i64> = self.tx.query_row(
            "SELECT MAX(at) FROM audit_log
              WHERE outlet_id = ?1 AND staff_id = ?2 AND action = 'login.failed'",
            rusqlite::params![outlet, staff_id],
            |row| row.get(0),
        )?;
        Ok(at.map(encode::timestamp_from_sql))
    }
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditRow> {
    Ok(AuditRow {
        id: row.get(0)?,
        seq: row.get(1)?,
        at: row.get(2)?,
        business_day: row.get(3)?,
        staff_id: row.get(4)?,
        staff_name: row.get(5)?,
        action: row.get(6)?,
        entity: row.get(7)?,
        entity_id: row.get(8)?,
        before_json: row.get(9)?,
        after_json: row.get(10)?,
        prev_hash: row.get(11)?,
        hash: row.get(12)?,
    })
}
