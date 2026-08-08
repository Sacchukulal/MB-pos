//! **What happened to an order, and when** — `order_events`.
//!
//! P04 modelled this table and nothing has written to it until now. P14 is its
//! first writer, and the reason is scope 14.2:
//!
//! > *"time since the last kitchen ticket — 'food ordered 18 minutes ago and
//! > nothing since'"*
//!
//! # Why not the kitchen ledger
//!
//! Because the ledger is **what the kitchen currently believes**, not a
//! history — `KitchenLedger::mark_cancelled` says so in those words. Its rows
//! are rewritten wholesale every time the order is saved, so a timestamp on
//! them would move when a cashier edited a line, and "18 minutes since the
//! ticket" would silently become "0 minutes since somebody typed".
//!
//! An event is the honest shape: it happened, at a moment, and nothing later
//! changes it.
//!
//! # The vocabulary
//!
//! Spelled once, here, as constants. A string literal typed at two call sites
//! is a filter that silently matches nothing — the same argument
//! `mb_auth::audit::action` makes about audit codes.

use mb_core::{BusinessDay, StaffId, Timestamp};
use rusqlite::{OptionalExtension, Transaction};

use crate::encode;
use crate::error::DbError;

/// A kitchen ticket went to the printer (crown jewel 2's delta KOT).
pub const KITCHEN_TICKET: &str = "kitchen_ticket";
/// The order moved to another table (scope 1.23).
pub const MOVED: &str = "moved";
/// This order's food was folded into another bill (scope 1.22).
pub const MERGED: &str = "merged";
/// Part of this order left for a bill of its own (scope 1.21).
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

    /// Write one down. **Append only** — there is no update and no delete
    /// here, because an event that can be edited is not evidence.
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

    /// When this order last did that. `None` if it never has.
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

    /// The same question asked of every open order at once — one query for a
    /// floor of sixty tables rather than sixty (budget B8's neighbourhood).
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

    /// Everything that happened to one order, oldest first. P18's bill history
    /// and P24's kitchen screen both want this; it is here now because writing
    /// events without a way to read them back is how a table gets forgotten.
    pub fn for_order(&self, order_id: &str) -> Result<Vec<(Timestamp, String, Option<String>)>, DbError> {
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
