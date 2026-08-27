//! Claiming a token or a bill number, on disk, in one statement.

use mb_core::{BusinessDay, Claimed};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;

/// Which series to claim from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterKind {
    Token,
    Bill,
    /// The kitchen ticket's own running number.
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

/// Take the next number in a series, for the order's own business day.
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
        // Formatted here and STORED on the order.
        formatted: format!("{prefix}{value:0width$}"),
        business_day: today,
    })
}

/// What was already handed out, or `None` if nothing has been yet.
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

/// A counter, whole, for the settings screen.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    pub prefix: String,
    pub pad_width: i64,
    pub reset_daily: bool,
    pub start: i64,
}

/// Write the shape of a series — prefix, padding, daily reset, starting number.
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

/// The settings edit. Names itself as a write, so it reads nothing like a claim.
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
