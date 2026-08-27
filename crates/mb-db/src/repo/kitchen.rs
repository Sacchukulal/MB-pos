//! What the kitchen was told, and what happened to it.

use mb_core::kitchen_delivery::{Delivery, State};
use mb_core::{StaffId, Timestamp};
use rusqlite::Transaction;

use crate::error::DbError;

/// A delivery, plus the things only the database knows about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub delivery: Delivery,
    /// Which course this firing is.
    pub course: Option<String>,
    /// How long the kitchen is expected to take on this firing — the slowest dish on it.
    pub expected_minutes: Option<u32>,
    pub bumped_by: Option<StaffId>,
    pub bumped_on: Option<String>,
    /// Lines a cook has ticked off one at a time.
    pub bumped_lines: Vec<String>,
    /// A cancellation the kitchen has not acknowledged.
    pub cancelled_at: Option<Timestamp>,
    pub acked_at: Option<Timestamp>,
}

impl Ticket {
    /// Does this need somebody to press "Got it"?
    #[must_use]
    pub const fn needs_acknowledging(&self) -> bool {
        self.cancelled_at.is_some() && self.acked_at.is_none()
    }
}

/// What the kitchen has already been told about one order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Fired {
    /// A firing that named no course.
    pub everything: bool,
    /// The courses fired by name.
    pub courses: Vec<String>,
}

impl Fired {
    /// Has the kitchen already been told about this course?
    #[must_use]
    pub fn covers(&self, course: &str) -> bool {
        self.everything || self.courses.iter().any(|c| c == course)
    }
}

#[derive(Debug)]
pub struct KitchenRepo<'a> {
    tx: &'a Transaction<'a>,
}

const COLUMNS: &str = "id, order_id, station, course, expected_minutes, state, sent_at, \
                       shown_at, bumped_at, bumped_by, bumped_on, bumped_lines, \
                       cancelled_at, acked_at";

impl<'a> KitchenRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        KitchenRepo { tx }
    }

    /// Send a ticket to a station.
    pub fn send(
        &self,
        outlet: &str,
        delivery: &Delivery,
        course: Option<&str>,
        expected_minutes: Option<u32>,
        business_day: mb_core::BusinessDay,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO kitchen_deliveries
                 (id, outlet_id, order_id, station, course, expected_minutes,
                  state, sent_at, business_day, bumped_lines)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '[]')
             ON CONFLICT (id) DO NOTHING",
            rusqlite::params![
                delivery.id,
                outlet,
                delivery.order_id,
                delivery.station,
                course,
                expected_minutes,
                state_code(delivery.state),
                delivery.sent_at.millis(),
                business_day.days_since_epoch(),
            ],
        )?;
        Ok(())
    }

    /// Every ticket the counter is still waiting on an ack for.
    pub fn awaiting_ack(&self, outlet: &str) -> Result<Vec<Ticket>, DbError> {
        let mut stmt = self.tx.prepare(&format!(
            "SELECT {COLUMNS} FROM kitchen_deliveries
             WHERE outlet_id = ?1 AND state = 'pending' ORDER BY sent_at",
        ))?;
        let mut rows = stmt.query(rusqlite::params![outlet])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(read(row)?);
        }
        Ok(out)
    }

    /// Which courses of an order have already been fired.
    pub fn courses_fired(&self, order_id: &str) -> Result<Fired, DbError> {
        let mut stmt = self
            .tx
            .prepare("SELECT course FROM kitchen_deliveries WHERE order_id = ?1")?;
        let mut rows = stmt.query(rusqlite::params![order_id])?;
        let mut fired = Fired::default();
        while let Some(row) = rows.next()? {
            match row.get::<_, Option<String>>(0)? {
                // A firing that named no course took the whole order with it, which is what
                // every shop that does not use courses does on every bill.
                None => fired.everything = true,
                Some(course) if course.is_empty() => fired.everything = true,
                Some(course) => {
                    if !fired.courses.contains(&course) {
                        fired.courses.push(course);
                    }
                }
            }
        }
        Ok(fired)
    }

    /// One ticket.
    pub fn get(&self, id: &str) -> Result<Option<Ticket>, DbError> {
        let mut stmt = self.tx.prepare(&format!(
            "SELECT {COLUMNS} FROM kitchen_deliveries WHERE id = ?1"
        ))?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        rows.next()?.map(read).transpose()
    }

    /// Everything still outstanding at a station, oldest first.
    pub fn outstanding(&self, outlet: &str, station: &str) -> Result<Vec<Ticket>, DbError> {
        let mut stmt = self.tx.prepare(&format!(
            "SELECT {COLUMNS} FROM kitchen_deliveries
             WHERE outlet_id = ?1 AND station = ?2 AND state NOT IN ('bumped', 'closed')
             ORDER BY sent_at",
        ))?;
        let mut rows = stmt.query(rusqlite::params![outlet, station])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(read(row)?);
        }
        Ok(out)
    }

    /// The card cleared most recently at this station.
    pub fn last_bumped(&self, outlet: &str, station: &str) -> Result<Option<Ticket>, DbError> {
        let mut stmt = self.tx.prepare(&format!(
            "SELECT {COLUMNS} FROM kitchen_deliveries
             WHERE outlet_id = ?1 AND station = ?2 AND state = 'bumped'
               AND bumped_at IS NOT NULL
             ORDER BY bumped_at DESC LIMIT 1",
        ))?;
        let mut rows = stmt.query(rusqlite::params![outlet, station])?;
        rows.next()?.map(read).transpose()
    }

    /// Every station that has a ticket on it — so a screen with no station set can still show
    /// something, and so the counter can list them.
    pub fn stations(&self, outlet: &str) -> Result<Vec<String>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT DISTINCT station FROM kitchen_deliveries
             WHERE outlet_id = ?1 ORDER BY station",
        )?;
        let mut rows = stmt.query(rusqlite::params![outlet])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row.get(0)?);
        }
        Ok(out)
    }

    /// Store whatever the state machine decided.
    pub fn save(&self, ticket: &Ticket) -> Result<(), DbError> {
        let lines = serde_json::to_string(&ticket.bumped_lines).unwrap_or_else(|_| "[]".to_owned());
        self.tx.execute(
            "UPDATE kitchen_deliveries
                SET state = ?2, shown_at = ?3, bumped_at = ?4, bumped_by = ?5,
                    bumped_on = ?6, bumped_lines = ?7, cancelled_at = ?8, acked_at = ?9
              WHERE id = ?1",
            rusqlite::params![
                ticket.delivery.id,
                state_code(ticket.delivery.state),
                ticket.delivery.shown_at.map(mb_core::Timestamp::millis),
                ticket.delivery.bumped_at.map(mb_core::Timestamp::millis),
                ticket.bumped_by.as_ref().map(mb_core::StaffId::as_str),
                ticket.bumped_on,
                lines,
                ticket.cancelled_at.map(mb_core::Timestamp::millis),
                ticket.acked_at.map(mb_core::Timestamp::millis),
            ],
        )?;
        Ok(())
    }

    /// Mark every outstanding ticket for an order as cancelled.
    pub fn cancel_order(&self, order_id: &str, at: Timestamp) -> Result<usize, DbError> {
        let changed = self.tx.execute(
            "UPDATE kitchen_deliveries
                SET cancelled_at = ?2
              WHERE order_id = ?1 AND cancelled_at IS NULL",
            rusqlite::params![order_id, at.millis()],
        )?;
        Ok(changed)
    }

    /// Nothing more for the kitchen on this order: a void, or a cancellation somebody saw.
    pub fn close_order(&self, order_id: &str) -> Result<usize, DbError> {
        let changed = self.tx.execute(
            "UPDATE kitchen_deliveries SET state = 'closed'
              WHERE order_id = ?1 AND state NOT IN ('bumped', 'closed')",
            rusqlite::params![order_id],
        )?;
        Ok(changed)
    }

    /// Every ticket still up for an order the counter has finished — the day close's sweep.
    pub fn close_finished(&self, outlet: &str) -> Result<usize, DbError> {
        let changed = self.tx.execute(
            "UPDATE kitchen_deliveries SET state = 'closed'
              WHERE outlet_id = ?1 AND state NOT IN ('bumped', 'closed')
                AND order_id IN (SELECT id FROM orders WHERE state <> 'open')",
            rusqlite::params![outlet],
        )?;
        Ok(changed)
    }

    /// Finished tickets in a period, for the kitchen-speed report.
    pub fn finished_between(
        &self,
        outlet: &str,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<Ticket>, DbError> {
        let mut stmt = self.tx.prepare(&format!(
            "SELECT {COLUMNS} FROM kitchen_deliveries
             WHERE outlet_id = ?1 AND bumped_at >= ?2 AND bumped_at < ?3
             ORDER BY bumped_at",
        ))?;
        let mut rows = stmt.query(rusqlite::params![outlet, from.millis(), to.millis()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(read(row)?);
        }
        Ok(out)
    }
}

/// One line of the kitchen-speed report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeedRow {
    /// The station, or the hour, depending on what was asked for.
    pub label: String,
    pub tickets: i64,
    /// Milliseconds. Formatted by the caller — this crate does no words.
    pub average_ms: i64,
    pub slowest_ms: i64,
    /// How many crossed their own target.
    pub late: i64,
}

impl<'a> KitchenRepo<'a> {
    /// How fast the kitchen is, per station.
    pub fn speed_by_station(
        &self,
        outlet: &str,
        from: mb_core::BusinessDay,
        to: mb_core::BusinessDay,
    ) -> Result<Vec<SpeedRow>, DbError> {
        self.speed(outlet, from, to, "station")
    }

    /// The same, by hour of the day — which is how a shop finds out that seven o'clock is where
    /// it loses people.
    pub fn speed_by_hour(
        &self,
        outlet: &str,
        from: mb_core::BusinessDay,
        to: mb_core::BusinessDay,
    ) -> Result<Vec<SpeedRow>, DbError> {
        self.speed(
            outlet,
            from,
            to,
            "strftime('%H', (bumped_at + 19800000) / 1000, 'unixepoch')",
        )
    }

    fn speed(
        &self,
        outlet: &str,
        from: mb_core::BusinessDay,
        to: mb_core::BusinessDay,
        group: &str,
    ) -> Result<Vec<SpeedRow>, DbError> {
        // `group` is one of two string literals chosen above and never comes from a caller,
        // which is why it can be interpolated.
        let sql = format!(
            "SELECT {group} AS bucket,
                    COUNT(*),
                    CAST(AVG(bumped_at - sent_at) AS INTEGER),
                    MAX(bumped_at - sent_at),
                    SUM(CASE WHEN expected_minutes IS NOT NULL
                              AND (bumped_at - sent_at) > expected_minutes * 60000
                             THEN 1 ELSE 0 END)
               FROM kitchen_deliveries
              WHERE outlet_id = ?1 AND business_day BETWEEN ?2 AND ?3
                AND bumped_at IS NOT NULL
              GROUP BY bucket
              ORDER BY bucket"
        );
        let mut stmt = self.tx.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params![
            outlet,
            from.days_since_epoch(),
            to.days_since_epoch()
        ])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(SpeedRow {
                label: row.get(0)?,
                tickets: row.get(1)?,
                average_ms: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                slowest_ms: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                late: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
            });
        }
        Ok(out)
    }
}

fn state_code(state: State) -> &'static str {
    match state {
        State::Pending => "pending",
        State::Shown => "shown",
        State::Bumped => "bumped",
        State::Printed => "printed",
        State::Closed => "closed",
    }
}

/// An unknown state reads as `Pending`.
fn state_from(code: &str) -> State {
    match code {
        "shown" => State::Shown,
        "bumped" => State::Bumped,
        "printed" => State::Printed,
        "closed" => State::Closed,
        _ => State::Pending,
    }
}

fn read(row: &rusqlite::Row<'_>) -> Result<Ticket, DbError> {
    // The order is COLUMNS', and the two must be changed together.
    let state_code: String = row.get(5)?;
    let lines: String = row.get(11)?;
    Ok(Ticket {
        delivery: Delivery {
            id: row.get(0)?,
            order_id: row.get(1)?,
            station: row.get(2)?,
            state: state_from(&state_code),
            sent_at: Timestamp::from_millis(row.get(6)?),
            shown_at: row.get::<_, Option<i64>>(7)?.map(Timestamp::from_millis),
            bumped_at: row.get::<_, Option<i64>>(8)?.map(Timestamp::from_millis),
        },
        course: row.get(3)?,
        expected_minutes: row
            .get::<_, Option<i64>>(4)?
            .and_then(|m| u32::try_from(m).ok()),
        bumped_by: row.get::<_, Option<String>>(9)?.map(StaffId::new),
        bumped_on: row.get(10)?,
        bumped_lines: serde_json::from_str(&lines).unwrap_or_default(),
        cancelled_at: row.get::<_, Option<i64>>(12)?.map(Timestamp::from_millis),
        acked_at: row.get::<_, Option<i64>>(13)?.map(Timestamp::from_millis),
    })
}
