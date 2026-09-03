//! The business day as a thing: what kind of day it was, whether it is locked, who closed it,
//! and what it came to. One row per day in `business_days`; the lock every money path checks
//! lives here and nowhere else.

use mb_core::{BusinessDay, Money, StaffId, Timestamp};
use rusqlite::{OptionalExtension, Transaction};

use crate::encode;
use crate::error::DbError;
use crate::repo::corrections::CorrectionsRepo;

/// What kind of day it was. Spelled exactly as the CHECK constraint spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayKind {
    Trading,
    Holiday,
}

impl DayKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            DayKind::Trading => "trading",
            DayKind::Holiday => "holiday",
        }
    }

    fn from_sql(text: &str) -> Result<Self, DbError> {
        match text {
            "trading" => Ok(DayKind::Trading),
            "holiday" => Ok(DayKind::Holiday),
            other => Err(DbError::invariant(format!(
                "business_days.kind holds `{other}`, which is neither trading nor holiday"
            ))),
        }
    }
}

/// One `business_days` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayRow {
    pub day: BusinessDay,
    pub kind: DayKind,
    pub is_locked: bool,
    pub closed_at: Option<Timestamp>,
    pub closed_by: Option<StaffId>,
    pub reopened_at: Option<Timestamp>,
    pub reopened_by: Option<StaffId>,
    pub note: Option<String>,
    /// Frozen at the close: every bill, including the ones later voided.
    pub bills: i64,
    /// Frozen at the close: gross − voids.
    pub net: Money,
    /// Frozen at the close: cash on settled bills.
    pub cash_taken: Money,
}

/// What a day came to, read from the rows as they stand now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DayFigures {
    pub bills: i64,
    pub net: Money,
    pub cash: Money,
    pub upi_and_card: Money,
    pub expenses: Money,
}

impl DayFigures {
    /// Nothing happened: no bill of any kind and no money spent. The only day that may be a
    /// holiday.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bills == 0 && self.expenses.is_zero()
    }
}

#[derive(Debug)]
pub struct DaysRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> DaysRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        DaysRepo { tx }
    }

    /// The day's row, if it has one.
    pub fn find(&self, outlet: &str, day: BusinessDay) -> Result<Option<DayRow>, DbError> {
        let mut stmt = self.tx.prepare_cached(&format!(
            "{SELECT_ROW} WHERE outlet_id = ?1 AND business_day = ?2"
        ))?;
        let mut rows = stmt.query_map(
            rusqlite::params![outlet, encode::business_day_to_sql(day)],
            read_row,
        )?;
        rows.next().transpose().map_err(DbError::from)
    }

    /// When the day was locked — or `None` while it is open. This is the one question every
    /// money path asks before it moves anything in a day.
    pub fn locked_at(&self, outlet: &str, day: BusinessDay) -> Result<Option<Timestamp>, DbError> {
        let at: Option<i64> = self
            .tx
            .query_row(
                "SELECT COALESCE(closed_at, 0) FROM business_days
                  WHERE outlet_id = ?1 AND business_day = ?2 AND is_locked = 1",
                rusqlite::params![outlet, encode::business_day_to_sql(day)],
                |row| row.get(0),
            )
            .optional()?;
        Ok(at.map(encode::timestamp_from_sql))
    }

    pub fn is_locked(&self, outlet: &str, day: BusinessDay) -> Result<bool, DbError> {
        Ok(self.locked_at(outlet, day)?.is_some())
    }

    /// Lock a day, as a trading day or a holiday, with the figures frozen as they are now. A
    /// day that already has a row keeps its reopen marks; everything else is overwritten.
    pub fn lock(&self, outlet: &str, row: &DayRow) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO business_days (outlet_id, business_day, kind, is_locked, closed_at,
                                        closed_by, reopened_at, reopened_by, note, bills, net,
                                        cash_taken)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (outlet_id, business_day)
             DO UPDATE SET kind       = excluded.kind,
                           is_locked  = 1,
                           closed_at  = excluded.closed_at,
                           closed_by  = excluded.closed_by,
                           note       = excluded.note,
                           bills      = excluded.bills,
                           net        = excluded.net,
                           cash_taken = excluded.cash_taken",
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(row.day),
                row.kind.as_str(),
                row.closed_at.map(encode::timestamp_to_sql),
                row.closed_by.as_ref().map(StaffId::as_str),
                row.reopened_at.map(encode::timestamp_to_sql),
                row.reopened_by.as_ref().map(StaffId::as_str),
                row.note,
                row.bills,
                encode::money_to_sql(row.net),
                encode::money_to_sql(row.cash_taken),
            ],
        )?;
        Ok(())
    }

    /// Open a locked day again. A reopened holiday is a trading day: it is open for billing,
    /// and a holiday somebody bills on was not one. `false` when the day was not locked.
    pub fn unlock(
        &self,
        outlet: &str,
        day: BusinessDay,
        at: Timestamp,
        by: Option<&StaffId>,
        note: Option<&str>,
    ) -> Result<bool, DbError> {
        let changed = self.tx.execute(
            "UPDATE business_days
                SET is_locked = 0, kind = 'trading', reopened_at = ?3, reopened_by = ?4,
                    note = COALESCE(?5, note)
              WHERE outlet_id = ?1 AND business_day = ?2 AND is_locked = 1",
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(day),
                encode::timestamp_to_sql(at),
                by.map(StaffId::as_str),
                note,
            ],
        )?;
        Ok(changed > 0)
    }

    /// The last locked day strictly before `day` — where the gate starts counting from.
    pub fn last_locked_before(
        &self,
        outlet: &str,
        day: BusinessDay,
    ) -> Result<Option<BusinessDay>, DbError> {
        let found: Option<i64> = self.tx.query_row(
            "SELECT MAX(business_day) FROM business_days
              WHERE outlet_id = ?1 AND business_day < ?2 AND is_locked = 1",
            rusqlite::params![outlet, encode::business_day_to_sql(day)],
            |row| row.get(0),
        )?;
        found
            .map(|d| encode::business_day_from_sql(d, "business_days.business_day"))
            .transpose()
    }

    /// Every row between two days inclusive, oldest first.
    pub fn rows_between(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<DayRow>, DbError> {
        let mut stmt = self.tx.prepare_cached(&format!(
            "{SELECT_ROW} WHERE outlet_id = ?1 AND business_day BETWEEN ?2 AND ?3
              ORDER BY business_day"
        ))?;
        let rows = stmt.query_map(
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            read_row,
        )?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// Every locked row after a day — the holidays already planned.
    pub fn locked_after(&self, outlet: &str, day: BusinessDay) -> Result<Vec<DayRow>, DbError> {
        let mut stmt = self.tx.prepare_cached(&format!(
            "{SELECT_ROW} WHERE outlet_id = ?1 AND business_day > ?2 AND is_locked = 1
              ORDER BY business_day"
        ))?;
        let rows = stmt.query_map(
            rusqlite::params![outlet, encode::business_day_to_sql(day)],
            read_row,
        )?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// The first day anything happened — an order of any kind or an expense — so a shop that
    /// has never closed a day is not asked about the weeks before it opened.
    pub fn first_activity(&self, outlet: &str) -> Result<Option<BusinessDay>, DbError> {
        let found: Option<i64> = self.tx.query_row(
            "SELECT MIN(day) FROM (
                SELECT MIN(business_day) AS day FROM orders WHERE outlet_id = ?1
                UNION ALL
                SELECT MIN(business_day) FROM expenses WHERE outlet_id = ?1
             )",
            rusqlite::params![outlet],
            |row| row.get(0),
        )?;
        found
            .map(|d| encode::business_day_from_sql(d, "orders.business_day"))
            .transpose()
    }

    /// What a day came to, from the rows: the bills and net the reconciliation already sums,
    /// the takings split by how they were paid, and what was spent.
    pub fn figures(&self, outlet: &str, day: BusinessDay) -> Result<DayFigures, DbError> {
        let totals = CorrectionsRepo::new(self.tx).day_totals(outlet, day)?;
        let day_sql = encode::business_day_to_sql(day);

        let mut stmt = self.tx.prepare_cached(
            "SELECT p.mode, COALESCE(SUM(p.amount), 0)
               FROM payments p JOIN orders o ON o.id = p.order_id
              WHERE o.outlet_id = ?1 AND o.business_day = ?2 AND o.state = 'settled'
              GROUP BY p.mode",
        )?;
        let mut rows = stmt.query(rusqlite::params![outlet, day_sql])?;
        let (mut cash, mut electronic) = (0_i64, 0_i64);
        while let Some(row) = rows.next()? {
            let mode: String = row.get(0)?;
            let amount: i64 = row.get(1)?;
            match mode.as_str() {
                "cash" => cash = cash.saturating_add(amount),
                "upi" | "card" => electronic = electronic.saturating_add(amount),
                _ => {}
            }
        }

        let expenses: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM expenses
              WHERE outlet_id = ?1 AND business_day = ?2",
            rusqlite::params![outlet, day_sql],
            |row| row.get(0),
        )?;

        Ok(DayFigures {
            bills: totals.bills,
            net: totals.net,
            cash: encode::money_from_sql(cash),
            upi_and_card: encode::money_from_sql(electronic),
            expenses: encode::money_from_sql(expenses),
        })
    }
}

const SELECT_ROW: &str = "SELECT business_day, kind, is_locked, closed_at, closed_by, reopened_at,
                                 reopened_by, note, bills, net, cash_taken
                            FROM business_days";

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DayRow> {
    let kind: String = row.get(1)?;
    Ok(DayRow {
        day: BusinessDay::from_days_since_epoch(i32::try_from(row.get::<_, i64>(0)?).unwrap_or(0)),
        kind: DayKind::from_sql(&kind).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?,
        is_locked: row.get::<_, i64>(2)? == 1,
        closed_at: row
            .get::<_, Option<i64>>(3)?
            .map(encode::timestamp_from_sql),
        closed_by: row.get::<_, Option<String>>(4)?.map(StaffId::new),
        reopened_at: row
            .get::<_, Option<i64>>(5)?
            .map(encode::timestamp_from_sql),
        reopened_by: row.get::<_, Option<String>>(6)?.map(StaffId::new),
        note: row.get(7)?,
        bills: row.get(8)?,
        net: encode::money_from_sql(row.get(9)?),
        cash_taken: encode::money_from_sql(row.get(10)?),
    })
}
