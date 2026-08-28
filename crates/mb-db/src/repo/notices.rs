//! Notices from Magic Bill, for the bell.

use mb_core::Timestamp;
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;

/// One notice, as it came down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudNotice {
    pub id: String,
    pub title: String,
    pub body: String,
    pub starts_at: Timestamp,
    pub ends_at: Option<Timestamp>,
    pub updated_at: Timestamp,
    pub is_deleted: bool,
}

/// A notice, as the bell shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeRow {
    pub id: String,
    pub title: String,
    pub body: String,
    pub starts_at: Timestamp,
    pub is_seen: bool,
}

#[derive(Debug)]
pub struct NoticesRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> NoticesRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        NoticesRepo { tx }
    }

    /// Store what came down. A notice already here keeps its seen mark.
    pub fn apply(&self, outlet: &str, notices: &[CloudNotice]) -> Result<usize, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "INSERT INTO cloud_notices (id, outlet_id, title, body, starts_at, ends_at, updated_at, is_deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET title      = excluded.title,
                                            body       = excluded.body,
                                            starts_at  = excluded.starts_at,
                                            ends_at    = excluded.ends_at,
                                            updated_at = excluded.updated_at,
                                            is_deleted = excluded.is_deleted
             WHERE cloud_notices.updated_at <= excluded.updated_at",
        )?;
        let mut applied = 0;
        for n in notices {
            applied += stmt.execute(rusqlite::params![
                n.id,
                outlet,
                n.title,
                n.body,
                encode::timestamp_to_sql(n.starts_at),
                n.ends_at.map(encode::timestamp_to_sql),
                encode::timestamp_to_sql(n.updated_at),
                encode::bool_to_sql(n.is_deleted),
            ])?;
        }
        Ok(applied)
    }

    /// Everything current, newest first.
    pub fn list(&self, outlet: &str, now: Timestamp) -> Result<Vec<NoticeRow>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, title, body, starts_at, seen_at
               FROM cloud_notices
              WHERE outlet_id = ?1 AND is_deleted = 0 AND starts_at <= ?2
                AND (ends_at IS NULL OR ends_at > ?2)
              ORDER BY starts_at DESC
              LIMIT 100",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![outlet, encode::timestamp_to_sql(now)],
            |row| {
                Ok(NoticeRow {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    starts_at: encode::timestamp_from_sql(row.get(3)?),
                    is_seen: row.get::<_, Option<i64>>(4)?.is_some(),
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    /// How many the bell should show.
    pub fn unseen(&self, outlet: &str, now: Timestamp) -> Result<u32, DbError> {
        let n: i64 = self.tx.query_row(
            "SELECT COUNT(*) FROM cloud_notices
              WHERE outlet_id = ?1 AND is_deleted = 0 AND seen_at IS NULL AND starts_at <= ?2
                AND (ends_at IS NULL OR ends_at > ?2)",
            rusqlite::params![outlet, encode::timestamp_to_sql(now)],
            |row| row.get(0),
        )?;
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }

    /// The bell was opened: everything current is seen.
    pub fn mark_all_seen(&self, outlet: &str, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "UPDATE cloud_notices SET seen_at = ?2 WHERE outlet_id = ?1 AND seen_at IS NULL",
            rusqlite::params![outlet, encode::timestamp_to_sql(at)],
        )?;
        Ok(())
    }
}
