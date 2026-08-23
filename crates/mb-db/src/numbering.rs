//! Claiming a token or a bill number, on disk, in **one statement**.
//!
//! This is audit finding B4 one layer below where P03 fixed it:
//!
//! > *"Bill and token numbers are claimed in two steps, not one. The app reads
//! > the current number, then increases it in a separate command. A phone order
//! > arriving at the exact moment the cashier presses Complete Bill could get
//! > the same number."*
//! >
//! > **Fix:** *"one atomic database operation that increments and returns in a
//! > single step. Non-negotiable for a bill number."*
//!
//! And B3 lives here too — the daily reset is evaluated **inside** the same
//! statement, not by a SELECT the caller does first, for exactly the reason a
//! check at app start was not enough on a counter PC that never restarts.
//!
//! P03 left this module a note in `mb-core/src/numbering.rs` naming the SQL.
//! This is that SQL.
//!
//! # Why there is no `peek`
//!
//! The same rule P03 wrote in types, kept here in SQL: there is no way to read
//! the number that is about to be issued. [`last_issued`] describes the past
//! and cannot be mistaken for a claim; [`set_next`] names itself as a write.
//! That pair — read, then increment, with a gap between them — *is* the bug.

use mb_core::{BusinessDay, Claimed};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;

/// Which series to claim from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterKind {
    Token,
    Bill,
    /// **P32 — the kitchen ticket's own running number.** Resets daily, like
    /// the token: a kitchen talks about "KOT 14" within a shift.
    Kot,
}

impl CounterKind {
    #[must_use]
    pub const fn as_sql(self) -> &'static str {
        match self {
            CounterKind::Token => "token",
            CounterKind::Bill => "bill",
            CounterKind::Kot => "kot",
        }
    }
}

/// The one statement.
///
/// Reset, increment and return, indivisibly. There is no `SELECT` in front of
/// it and there must never be one: two callers can both walk through the gap
/// between a read and a write, and at a counter those two callers are the
/// cashier pressing Complete Bill and a waiter's phone arriving at the same
/// instant.
const CLAIM: &str = "
    UPDATE counters
       SET last_issued = CASE
               WHEN reset_daily = 1
                    AND (last_reset_day IS NULL OR last_reset_day < :today)
               THEN start
               ELSE COALESCE(last_issued + 1, start)
           END,
           last_reset_day = CASE
               WHEN reset_daily = 1
                    AND (last_reset_day IS NULL OR last_reset_day < :today)
               THEN :today
               ELSE last_reset_day
           END
     WHERE outlet_id = :outlet AND terminal_id = :terminal AND kind = :kind
 RETURNING last_issued, prefix, pad_width
";

/// Take the next number in a series, for the order's **own** business day.
///
/// `today` is the day the ORDER belongs to, which is not always the calendar
/// day: an order created at 00:15 under a 05:00 day rule belongs to yesterday
/// and must take yesterday's series (D5, and it is B1 and B3 meeting).
///
/// The whole claim happens inside the caller's transaction, so a settle that
/// fails afterwards does not burn a bill number — and a settle that succeeds
/// wrote the number and the bill in the same commit (`PERFORMANCE.md` §5
/// rule 1, budget B5).
pub fn claim(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    kind: CounterKind,
    today: BusinessDay,
) -> Result<Claimed, DbError> {
    let day = encode::business_day_to_sql(today);

    let mut stmt = tx.prepare_cached(CLAIM)?;
    let (value, prefix, pad_width) = stmt.query_row(
        rusqlite::named_params! {
            ":today": day,
            ":outlet": outlet,
            ":terminal": terminal,
            ":kind": kind.as_sql(),
        },
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )?;

    let value = u64::try_from(value).map_err(|_| DbError::OutOfRange {
        column: "counters.last_issued",
        expected: "a number that has been issued",
    })?;
    let width = usize::try_from(pad_width.max(0)).unwrap_or(0);

    Ok(Claimed {
        value,
        // Formatted here and STORED on the order. P03's reasoning, unchanged:
        // a bill number that has been printed must not change because somebody
        // edited the prefix setting six months later. The value is the
        // identity; this string is the historical fact.
        formatted: format!("{prefix}{value:0width$}"),
        business_day: today,
    })
}

/// What was already handed out, or `None` if nothing has been yet.
///
/// For the settings screen (audit Part 3: "current value can be edited by
/// hand"). Named for the past so it cannot be read as "the number I am about
/// to use".
pub fn last_issued(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    kind: CounterKind,
) -> Result<Option<u64>, DbError> {
    let value: Option<i64> = tx.query_row(
        "SELECT last_issued FROM counters
          WHERE outlet_id = ?1 AND terminal_id = ?2 AND kind = ?3",
        rusqlite::params![outlet, terminal, kind.as_sql()],
        |row| row.get(0),
    )?;
    match value {
        None => Ok(None),
        Some(v) => u64::try_from(v).map(Some).map_err(|_| DbError::OutOfRange {
            column: "counters.last_issued",
            expected: "a number that has been issued",
        }),
    }
}

/// A counter, whole, for the settings screen (P17).
///
/// **`last_issued` is `None` until something has been issued**, which is not
/// the same as zero — see the column's own comment. The screen shows "nothing
/// yet" rather than "0", because "0" reads as a number that was used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counter {
    pub kind: CounterKind,
    pub prefix: String,
    pub pad_width: i64,
    pub reset_daily: bool,
    pub start: i64,
    pub last_issued: Option<i64>,
}

/// Both counters for a terminal.
pub fn counters(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
) -> Result<Vec<Counter>, DbError> {
    let mut stmt = tx.prepare_cached(
        "SELECT kind, prefix, pad_width, reset_daily, start, last_issued
           FROM counters
          WHERE outlet_id = ?1 AND terminal_id = ?2
          ORDER BY kind DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![outlet, terminal], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<i64>>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (kind, prefix, pad_width, reset_daily, start, last_issued) = row?;
        out.push(Counter {
            kind: match kind.as_str() {
                "token" => CounterKind::Token,
                "kot" => CounterKind::Kot,
                _ => CounterKind::Bill,
            },
            prefix,
            pad_width,
            reset_daily: reset_daily != 0,
            start,
            last_issued,
        });
    }
    Ok(out)
}

/// The shape of a series: what a number LOOKS like and when it starts over.
///
/// Four values in a struct rather than four arguments, because
/// `set_format(tx, outlet, terminal, kind, "", 4, false, 1)` is a line nobody
/// can read and two of those numbers are interchangeable at the call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    pub prefix: String,
    pub pad_width: i64,
    pub reset_daily: bool,
    pub start: i64,
}

/// Write the shape of a series — prefix, padding, daily reset, starting number.
///
/// **Not `last_issued`.** That is what has been handed out, and moving it is a
/// different and far more dangerous act: [`set_next`] owns it, and P17's screen
/// refuses to move it backwards past a number a customer is holding.
pub fn set_format(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    kind: CounterKind,
    format: &Format,
) -> Result<(), DbError> {
    let Format {
        prefix,
        pad_width,
        reset_daily,
        start,
    } = format;
    let changed = tx.execute(
        "UPDATE counters
            SET prefix = ?4, pad_width = ?5, reset_daily = ?6, start = ?7
          WHERE outlet_id = ?1 AND terminal_id = ?2 AND kind = ?3",
        rusqlite::params![
            outlet,
            terminal,
            kind.as_sql(),
            prefix,
            pad_width,
            i64::from(*reset_daily),
            start,
        ],
    )?;
    if changed == 0 {
        return Err(DbError::invariant(format!(
            "there is no {} counter for terminal {terminal}",
            kind.as_sql()
        )));
    }
    Ok(())
}

/// The settings edit. Names itself as a write, so it reads nothing like a
/// claim.
///
/// `value` is the next number to hand out, which is what the owner types on the
/// settings screen — so what is stored is the one before it.
pub fn set_next(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    kind: CounterKind,
    value: u64,
) -> Result<(), DbError> {
    let stored = i64::try_from(value)
        .map_err(|_| DbError::OutOfRange {
            column: "counters.last_issued",
            expected: "a bill number",
        })?
        .checked_sub(1)
        .ok_or(DbError::OutOfRange {
            column: "counters.last_issued",
            expected: "a bill number of at least 1",
        })?;

    let changed = tx.execute(
        "UPDATE counters SET last_issued = ?4
          WHERE outlet_id = ?1 AND terminal_id = ?2 AND kind = ?3",
        rusqlite::params![outlet, terminal, kind.as_sql(), stored],
    )?;
    if changed == 0 {
        return Err(DbError::invariant(format!(
            "there is no {} counter for terminal {terminal}",
            kind.as_sql()
        )));
    }
    Ok(())
}
