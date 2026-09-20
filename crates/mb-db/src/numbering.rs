//! Claiming a token or a bill number, on disk, in one statement.
//!
//! Two rules keep the three series honest, whatever happens to the file:
//!
//! - **The orders are the proof of what was issued.** A counter never sits behind the
//!   highest number its terminal's orders already carry; `catch_up` says so, and runs every
//!   time the shop's file is opened and every time a shop is brought down from the cloud.
//! - **The shape of a series is part of the shop.** A counter row travels to the cloud like
//!   any other setting, so a new computer prints `BILL//0063` after `BILL//0062`.

use mb_core::{BusinessDay, Claimed, Timestamp};
use rusqlite::{OptionalExtension as _, Transaction};

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

/// The table, as the outbox and the cloud name it.
pub const TABLE: &str = "counters";

/// Which series to claim from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterKind {
    Token,
    Bill,
    /// The kitchen ticket's own running number.
    Kot,
}

impl CounterKind {
    /// Every series a till has.
    pub const ALL: [CounterKind; 3] = [CounterKind::Token, CounterKind::Bill, CounterKind::Kot];

    #[must_use]
    pub const fn as_sql(self) -> &'static str {
        match self {
            CounterKind::Token => "token",
            CounterKind::Bill => "bill",
            CounterKind::Kot => "kot",
        }
    }

    #[must_use]
    pub fn from_sql(text: &str) -> Option<CounterKind> {
        CounterKind::ALL.into_iter().find(|k| k.as_sql() == text)
    }

    /// Migration 0001's shape for a new till's series: whether it starts again every day,
    /// and how many digits it is padded to.
    const fn seed_shape(self) -> (bool, i64) {
        match self {
            CounterKind::Token | CounterKind::Kot => (true, 0),
            CounterKind::Bill => (false, 4),
        }
    }

    /// The columns on `orders` that carry this series' number, as a value and as printed.
    /// The kitchen ticket's number is printed and never stored, so its orders prove nothing.
    const fn order_columns(self) -> Option<(&'static str, &'static str)> {
        match self {
            CounterKind::Bill => Some(("bill_number_value", "bill_number_formatted")),
            CounterKind::Token => Some(("token_value", "token_formatted")),
            CounterKind::Kot => None,
        }
    }
}

/// The outbox's name for one counter row: `terminal_default:bill`.
#[must_use]
pub fn row_id(terminal: &str, kind: CounterKind) -> String {
    format!("{terminal}:{}", kind.as_sql())
}

/// The reverse: which counter an outbox row names.
#[must_use]
pub fn parse_row_id(id: &str) -> Option<(&str, CounterKind)> {
    let (terminal, kind) = id.rsplit_once(':')?;
    Some((terminal, CounterKind::from_sql(kind)?))
}

/// Queue one counter row for the cloud.
pub(crate) fn queue(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    kind: CounterKind,
    at: Timestamp,
) -> Result<(), DbError> {
    OutboxRepo::new(tx).enqueue(outlet, TABLE, &row_id(terminal, kind), Op::Upsert, at)
}

/// Queue every counter row of the shop for the cloud. A restore calls this once the outbox
/// is otherwise empty, so the cloud learns the series it never held.
pub fn queue_all(tx: &Transaction<'_>, at: Timestamp) -> Result<usize, DbError> {
    let mut stmt = tx.prepare("SELECT outlet_id, terminal_id, kind FROM counters")?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (outlet, terminal, kind) in &rows {
        let Some(kind) = CounterKind::from_sql(kind) else {
            continue;
        };
        queue(tx, outlet, terminal, kind, at)?;
    }
    Ok(rows.len())
}

/// A till's three rows, in migration 0001's shape, where it has none. A row it already has
/// is left exactly as it is: the shape is the till's own setting.
pub(crate) fn ensure_rows(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    prefix: &str,
) -> Result<(), DbError> {
    for kind in CounterKind::ALL {
        let (reset_daily, pad) = kind.seed_shape();
        tx.execute(
            "INSERT INTO counters
                 (outlet_id, terminal_id, kind, last_issued, start, reset_daily,
                  prefix, pad_width, last_reset_day)
             VALUES (?1, ?2, ?3, NULL, 1, ?4, ?5, ?6, NULL)
             ON CONFLICT (outlet_id, terminal_id, kind) DO NOTHING",
            rusqlite::params![
                outlet,
                terminal,
                kind.as_sql(),
                i64::from(reset_daily),
                prefix,
                pad
            ],
        )?;
    }
    Ok(())
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

/// The bill is being made: an open order takes its bill number, claimed against the order's
/// own business day in THIS transaction, so a failure after it cannot spend one. An order
/// that already has a number keeps it — the number on a printed bill never moves.
pub fn number_the_bill(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    order: &mut mb_core::OpenOrder,
) -> Result<(), DbError> {
    if order.bill_number.is_some() {
        return Ok(());
    }
    let number = claim(
        tx,
        outlet,
        terminal,
        CounterKind::Bill,
        order.core.business_day,
    )?;
    order
        .take_bill_number(number)
        .map_err(|e| DbError::invariant(e.to_string()))
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

/// An UPDATE that touched no row: the till has no such counter.
fn must_exist(changed: usize, kind: CounterKind, terminal: &str) -> Result<(), DbError> {
    if changed == 0 {
        return Err(DbError::invariant(format!(
            "there is no {} counter for terminal {terminal}",
            kind.as_sql()
        )));
    }
    Ok(())
}

/// Write what a series has issued. A series that starts again every day is told which day
/// the figure belongs to, or the next claim would start the day over and lose it.
fn write_issued(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    kind: CounterKind,
    issued: i64,
    day: BusinessDay,
) -> Result<(), DbError> {
    let changed = tx.execute(
        "UPDATE counters
            SET last_issued = ?4,
                last_reset_day = CASE WHEN reset_daily = 1 THEN ?5 ELSE last_reset_day END
          WHERE outlet_id = ?1 AND terminal_id = ?2 AND kind = ?3",
        rusqlite::params![
            outlet,
            terminal,
            kind.as_sql(),
            issued,
            encode::business_day_to_sql(day)
        ],
    )?;
    must_exist(changed, kind, terminal)?;
    Ok(())
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
    /// The day `last_issued` belongs to, for a series that starts again every day.
    pub last_reset_day: Option<BusinessDay>,
}

impl Counter {
    /// What this series has issued in its CURRENT run: a daily series that has not started
    /// today has issued nothing today, whatever yesterday's figure says.
    #[must_use]
    pub fn issued_as_of(&self, today: BusinessDay) -> Option<i64> {
        if self.reset_daily && self.last_reset_day != Some(today) {
            return None;
        }
        self.last_issued
    }
}

/// Both counters for a terminal.
pub fn counters(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
) -> Result<Vec<Counter>, DbError> {
    let mut stmt = tx.prepare_cached(
        "SELECT kind, prefix, pad_width, reset_daily, start, last_issued, last_reset_day
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
            row.get::<_, Option<i64>>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (kind, prefix, pad_width, reset_daily, start, last_issued, last_reset_day) = row?;
        let kind = CounterKind::from_sql(&kind).ok_or_else(|| DbError::BadValue {
            column: "counters.kind",
            value: kind.clone(),
        })?;
        let last_reset_day = last_reset_day
            .map(|d| encode::business_day_from_sql(d, "counters.last_reset_day"))
            .transpose()?;
        out.push(Counter {
            kind,
            prefix,
            pad_width,
            reset_daily: reset_daily != 0,
            start,
            last_issued,
            last_reset_day,
        });
    }
    Ok(out)
}

/// The newest run the ORDERS carry for a series: the day and the highest number on it. For
/// a series that starts again every day that is `on_day`'s run, or the newest day's when no
/// day is named; for one that runs on it is the whole run. `None` when the orders carry none.
fn on_orders(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    kind: CounterKind,
    reset_daily: bool,
    on_day: Option<BusinessDay>,
) -> Result<Option<(BusinessDay, i64)>, DbError> {
    let Some((column, _)) = kind.order_columns() else {
        return Ok(None);
    };
    let read =
        |r: &rusqlite::Row<'_>| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?));
    let found: Option<(Option<i64>, Option<i64>)> = match (reset_daily, on_day) {
        (true, Some(day)) => tx
            .query_row(
                &format!(
                    "SELECT business_day, MAX({column}) FROM orders
                      WHERE outlet_id = ?1 AND terminal_id = ?2 AND business_day = ?3
                        AND {column} IS NOT NULL
                      GROUP BY business_day"
                ),
                rusqlite::params![outlet, terminal, encode::business_day_to_sql(day)],
                read,
            )
            .optional()?,
        (true, None) => tx
            .query_row(
                &format!(
                    "SELECT business_day, MAX({column}) FROM orders
                      WHERE outlet_id = ?1 AND terminal_id = ?2 AND {column} IS NOT NULL
                      GROUP BY business_day ORDER BY business_day DESC LIMIT 1"
                ),
                rusqlite::params![outlet, terminal],
                read,
            )
            .optional()?,
        (false, _) => tx
            .query_row(
                &format!(
                    "SELECT MAX(business_day), MAX({column}) FROM orders
                      WHERE outlet_id = ?1 AND terminal_id = ?2 AND {column} IS NOT NULL"
                ),
                rusqlite::params![outlet, terminal],
                read,
            )
            .optional()?,
    };
    let Some((Some(day), Some(highest))) = found else {
        return Ok(None);
    };
    Ok(Some((
        encode::business_day_from_sql(day, "orders.business_day")?,
        highest,
    )))
}

/// The last number the shop can PROVE was issued in this series' current run: the higher of
/// what the counter remembers and what the orders carry. This is what an edit must stay
/// above, and what a counter is caught up to.
pub fn proven(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    kind: CounterKind,
    today: BusinessDay,
) -> Result<Option<i64>, DbError> {
    let Some(counter) = counters(tx, outlet, terminal)?
        .into_iter()
        .find(|c| c.kind == kind)
    else {
        return Ok(None);
    };
    let remembered = counter.issued_as_of(today);
    let carried = on_orders(tx, outlet, terminal, kind, counter.reset_daily, Some(today))?
        .map(|(_, highest)| highest);
    Ok(remembered.max(carried))
}

/// One counter that was found behind its orders, and where it was moved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaughtUp {
    pub terminal: String,
    pub kind: CounterKind,
    pub from: Option<i64>,
    pub to: i64,
}

/// Every terminal's counters, moved up to the highest number its orders carry. A file that
/// came down from the cloud, or was written by any road that saved orders without claiming
/// their numbers, is made honest here; a file that is already honest is left alone. Safe to
/// run at every open: it only ever moves a counter FORWARD.
pub fn catch_up(tx: &Transaction<'_>) -> Result<Vec<CaughtUp>, DbError> {
    let mut stmt = tx.prepare("SELECT outlet_id, id, series_prefix FROM terminals ORDER BY id")?;
    let terminals: Vec<(String, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;

    let mut moved = Vec::new();
    for (outlet, terminal, prefix) in &terminals {
        // A till that came back without its rows gets them.
        ensure_rows(tx, outlet, terminal, prefix.trim())?;

        for counter in counters(tx, outlet, terminal)? {
            let Some((day, highest)) = on_orders(
                tx,
                outlet,
                terminal,
                counter.kind,
                counter.reset_daily,
                None,
            )?
            else {
                continue;
            };
            let behind = if counter.reset_daily {
                match counter.last_reset_day {
                    // The counter's run is a later day than any order's: it is ahead.
                    Some(reset) if reset > day => false,
                    // The same day: behind only if its figure is lower.
                    Some(reset) if reset == day => counter.last_issued.is_none_or(|n| n < highest),
                    // Never reset, or reset on an earlier day than the orders show.
                    _ => true,
                }
            } else {
                counter.last_issued.is_none_or(|n| n < highest)
            };
            if !behind {
                continue;
            }
            write_issued(tx, outlet, terminal, counter.kind, highest, day)?;
            moved.push(CaughtUp {
                terminal: terminal.clone(),
                kind: counter.kind,
                from: counter.last_issued,
                to: highest,
            });
        }
    }
    Ok(moved)
}

/// One series whose printed shape was read back from its newest number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adopted {
    pub terminal: String,
    pub kind: CounterKind,
    pub prefix: String,
    pub pad_width: Option<i64>,
}

/// A cloud written before the counters travelled holds no shape for a series — only the
/// numbers as they were printed. Read the shape back from the newest printed number, for
/// every series whose counter still wears the migration's empty prefix. Nothing else is
/// touched, and a series whose numbers were printed bare stays bare.
pub fn adopt_format_from_orders(tx: &Transaction<'_>) -> Result<Vec<Adopted>, DbError> {
    let mut stmt = tx.prepare("SELECT outlet_id, id FROM terminals ORDER BY id")?;
    let terminals: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;

    let mut adopted = Vec::new();
    for (outlet, terminal) in &terminals {
        for counter in counters(tx, outlet, terminal)? {
            if !counter.prefix.is_empty() {
                continue;
            }
            let Some((value, formatted)) = counter.kind.order_columns() else {
                continue;
            };
            let newest: Option<(i64, String)> = tx
                .query_row(
                    &format!(
                        "SELECT {value}, {formatted} FROM orders
                          WHERE outlet_id = ?1 AND terminal_id = ?2 AND {value} IS NOT NULL
                          ORDER BY business_day DESC, {value} DESC LIMIT 1"
                    ),
                    rusqlite::params![outlet, terminal],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((value, formatted)) = newest else {
                continue;
            };
            let Some((prefix, pad_width)) = split_format(&formatted, value) else {
                continue;
            };
            if prefix.is_empty() && pad_width.is_none() {
                continue;
            }
            tx.execute(
                "UPDATE counters
                    SET prefix = ?4, pad_width = COALESCE(?5, pad_width)
                  WHERE outlet_id = ?1 AND terminal_id = ?2 AND kind = ?3",
                rusqlite::params![outlet, terminal, counter.kind.as_sql(), prefix, pad_width],
            )?;
            adopted.push(Adopted {
                terminal: terminal.clone(),
                kind: counter.kind,
                prefix,
                pad_width,
            });
        }
    }
    Ok(adopted)
}

/// `"BILL//0062"` with value 62 → prefix `BILL//`, padded to 4. The value is the longest run
/// of trailing digits that reads as the value, so a prefix that itself ends in a digit is
/// still found (`"A15"`, value 5 → prefix `A1`). Padding is only knowable from a leading
/// zero; without one it is `None`, and the caller keeps what it has.
#[must_use]
pub fn split_format(formatted: &str, value: i64) -> Option<(String, Option<i64>)> {
    let digits = formatted
        .bytes()
        .rev()
        .take_while(u8::is_ascii_digit)
        .count();
    if digits == 0 {
        return None;
    }
    let head = formatted.len();
    // Longest first: "A00062" reads as A + 00062 before it reads as A000 + 62.
    for width in (1..=digits).rev() {
        let start = head - width;
        let run = formatted.get(start..)?;
        let Ok(parsed) = run.parse::<i64>() else {
            continue;
        };
        if parsed != value {
            continue;
        }
        let prefix = formatted.get(..start)?.to_owned();
        let pad = if run.starts_with('0') && width > 1 {
            i64::try_from(width).ok()
        } else {
            None
        };
        return Some((prefix, pad));
    }
    None
}

/// The shape of a series: what a number LOOKS like and when it starts over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    pub prefix: String,
    pub pad_width: i64,
    pub reset_daily: bool,
    pub start: i64,
}

/// Write the shape of a series — prefix, padding, daily reset, starting number. The shape is
/// part of the shop, so the row is queued for the cloud.
pub fn set_format(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    kind: CounterKind,
    format: &Format,
    at: Timestamp,
) -> Result<(), DbError> {
    let Format {
        prefix,
        pad_width,
        reset_daily,
        start,
    } = format;
    if *reset_daily && kind == CounterKind::Bill {
        return Err(DbError::invariant(
            "the bill number runs on and never starts again: a GST return is one list",
        ));
    }
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
    must_exist(changed, kind, terminal)?;
    queue(tx, outlet, terminal, kind, at)
}

/// The settings edit. Names itself as a write, so it reads nothing like a claim.
pub fn set_next(
    tx: &Transaction<'_>,
    outlet: &str,
    terminal: &str,
    kind: CounterKind,
    value: u64,
    today: BusinessDay,
    at: Timestamp,
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
    write_issued(tx, outlet, terminal, kind, stored, today)?;
    queue(tx, outlet, terminal, kind, at)
}
