//! What happened to an order, and when — `order_events`.

use mb_core::{BusinessDay, StaffId, Timestamp};
use rusqlite::{OptionalExtension, Transaction};

use crate::encode;
use crate::error::DbError;

/// A kitchen ticket went to the printer.
pub const KITCHEN_TICKET: &str = "kitchen_ticket";
/// The order moved to another table.
pub const MOVED: &str = "moved";
/// This order's food was folded into another bill.
pub const MERGED: &str = "merged";
/// Part of this order left for a bill of its own.
pub const SPLIT: &str = "split";

#[derive(Debug)]
pub struct EventsRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> EventsRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        EventsRepo { tx }
    }

    /// Write one down. Append only — there is no update and no delete here, because an event
    /// that can be edited is not evidence.
    pub fn record(
        &self,
        order_id: &str,
        at: Timestamp,
        day: BusinessDay,
        event: &str,
        staff: Option<&StaffId>,
        detail: Option<&str>,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO order_events (id, order_id, at, business_day, event, staff_id, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                format!("evt_{}_{}_{}", order_id, event, at.millis()),
                order_id,
                encode::timestamp_to_sql(at),
                encode::business_day_to_sql(day),
                event,
                staff.map(StaffId::as_str),
                detail,
            ],
        )?;
        Ok(())
    }

    /// What the counter answered the first time, if this intent has been seen.
    pub fn recall(&self, event_id: &str) -> Result<Option<String>, DbError> {
        let mut stmt = self
            .tx
            .prepare_cached("SELECT result FROM applied_events WHERE event_id = ?1")?;
        let mut rows = stmt.query(rusqlite::params![event_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get::<_, Option<String>>(0)?.unwrap_or_default())),
            None => Ok(None),
        }
    }

    /// Remember that this intent was applied, and what was said about it.
    pub fn remember(
        &self,
        outlet: &str,
        event_id: &str,
        source: &str,
        result: &str,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO applied_events (event_id, outlet_id, applied_at, source, result)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                event_id,
                outlet,
                encode::timestamp_to_sql(at),
                source,
                result,
            ],
        )?;
        Ok(())
    }

    /// When this order last did that.
    pub fn last_at(&self, order_id: &str, event: &str) -> Result<Option<Timestamp>, DbError> {
        Ok(self
            .tx
            .query_row(
                "SELECT max(at) FROM order_events WHERE order_id = ?1 AND event = ?2",
                rusqlite::params![order_id, event],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .map(encode::timestamp_from_sql))
    }

    /// The same question asked of every open order at once — one query for a floor of sixty
    /// tables rather than sixty.
    pub fn last_for_each(&self, event: &str) -> Result<Vec<(String, Timestamp)>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT order_id, max(at) FROM order_events WHERE event = ?1 GROUP BY order_id",
        )?;
        let rows = stmt.query_map([event], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, at) = row?;
            out.push((id, encode::timestamp_from_sql(at)));
        }
        Ok(out)
    }

    /// Everything that happened to one order, oldest first.
    pub fn for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<(Timestamp, String, Option<String>)>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT at, event, detail FROM order_events WHERE order_id = ?1 ORDER BY at",
        )?;
        let rows = stmt.query_map([order_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (at, event, detail) = row?;
            out.push((encode::timestamp_from_sql(at), event, detail));
        }
        Ok(out)
    }
}
