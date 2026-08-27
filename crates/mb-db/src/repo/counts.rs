//! The physical stock count.

use mb_core::{BusinessDay, MaterialId, Money, Qty, StaffId, Timestamp, UnitCost};
use rusqlite::{Transaction, params};

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};
use crate::repo::stock::{Movement, MovementKind, StockRepo};

/// Where a count has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountState {
    Draft,
    /// Sealed for ever. Its adjustments are ordinary ledger rows carrying its id, so the
    /// variance history is a query and not a second store.
    Approved,
    /// Given up on, with a reason.
    Abandoned,
}

impl CountState {
    pub const ALL: &'static [CountState] = &[
        CountState::Draft,
        CountState::Approved,
        CountState::Abandoned,
    ];

    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            CountState::Draft => "draft",
            CountState::Approved => "approved",
            CountState::Abandoned => "abandoned",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            CountState::Draft => "Being counted",
            CountState::Approved => "Approved",
            CountState::Abandoned => "Given up",
        }
    }

    pub fn from_tag(tag: &str) -> Result<Self, DbError> {
        CountState::ALL
            .iter()
            .copied()
            .find(|s| s.tag() == tag)
            .ok_or_else(|| {
                DbError::invariant(format!("stock_counts.state holds an unknown value `{tag}`"))
            })
    }
}

/// One material on the sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountLine {
    pub seq: i64,
    pub material_id: MaterialId,
    pub material_name: String,
    /// The book as it was when this line was counted, and when that was.
    pub book_qty: Qty,
    pub book_at: Timestamp,
    pub counted_qty: Qty,
    /// What the person actually wrote on the sheet.
    pub typed_qty: Qty,
    pub typed_unit: String,
    /// `counted − book`. Negative means less on the shelf than the book claims.
    pub variance_qty: Qty,
    pub unit_cost: UnitCost,
    /// A variance in kilos is one nobody reads.
    pub variance_value: Money,
    pub reason_id: Option<String>,
    pub note: Option<String>,
    /// The adjustment this line posted when the count was approved.
    pub movement_id: Option<String>,
}

/// A walk round the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockCount {
    pub id: String,
    pub location: String,
    pub state: CountState,
    pub business_day: BusinessDay,
    pub opened_at: Timestamp,
    pub opened_by: Option<StaffId>,
    pub approved_at: Option<Timestamp>,
    pub approved_by: Option<StaffId>,
    pub ended_reason: Option<String>,
    pub note: Option<String>,
    pub lines: Vec<CountLine>,
}

impl StockCount {
    /// What approving would do, as two numbers the screen says out loud before anybody presses
    /// it: "This will add 2.4 kg and remove 800 g.".
    #[must_use]
    pub fn effect(&self) -> (usize, usize) {
        let up = self
            .lines
            .iter()
            .filter(|l| l.variance_qty.is_positive())
            .count();
        let down = self
            .lines
            .iter()
            .filter(|l| l.variance_qty.is_negative())
            .count();
        (up, down)
    }

    /// What the whole count is worth, plus or minus.
    #[must_use]
    pub fn variance_value(&self) -> Money {
        Money::try_sum(self.lines.iter().map(|l| l.variance_value)).unwrap_or(Money::ZERO)
    }
}

/// What somebody wrote on the sheet, as one thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The truth, in base units.
    pub base: Qty,
    /// What the person actually wrote, in `unit`.
    pub typed: Qty,
    pub unit: String,
}

/// One material's history of being counted — the report that answers "is this always short, or
/// was it one bad month".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarianceRow {
    pub name: String,
    pub unit: String,
    pub counts: i64,
    pub variance_qty: Qty,
    pub variance_value: Money,
    pub last_counted: BusinessDay,
}

#[derive(Debug)]
pub struct CountRepo<'a> {
    tx: &'a Transaction<'a>,
}

const COUNT_COLUMNS: &str = "id, location, state, business_day, opened_at, opened_by, \
     approved_at, approved_by, ended_reason, note";

impl<'a> CountRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        CountRepo { tx }
    }

    /// Start a walk. One open count per location at a time — a second one is refused naming the
    /// first, because two people counting the same shelf into two sheets produces two truths
    /// and one argument.
    pub fn open(
        &self,
        outlet: &str,
        id: &str,
        location: &str,
        day: BusinessDay,
        at: Timestamp,
        by: Option<&StaffId>,
    ) -> Result<StockCount, DbError> {
        if let Some(open) = self.open_at(outlet, location)? {
            return Err(DbError::invariant(format!(
                "a count of {location} is already open — finish or give up that one first \
                 (started {})",
                open.business_day
            )));
        }
        self.tx.execute(
            "INSERT INTO stock_counts
                 (id, outlet_id, location, state, business_day, opened_at, opened_by,
                  approved_at, approved_by, ended_reason, note)
             VALUES (?1, ?2, ?3, 'draft', ?4, ?5, ?6, NULL, NULL, NULL, NULL)",
            params![
                id,
                outlet,
                location,
                encode::business_day_to_sql(day),
                encode::timestamp_to_sql(at),
                by.map(StaffId::as_str),
            ],
        )?;
        self.count(outlet, id)?
            .ok_or_else(|| DbError::invariant("the count could not be read back"))
    }

    /// The count somebody is in the middle of, for this location.
    pub fn open_at(&self, outlet: &str, location: &str) -> Result<Option<StockCount>, DbError> {
        let sql = format!(
            "SELECT {COUNT_COLUMNS} FROM stock_counts
              WHERE outlet_id = ?1 AND location = ?2 AND state = 'draft'
              ORDER BY opened_at LIMIT 1"
        );
        let mut stmt = self.tx.prepare(&sql)?;
        let mut rows = stmt.query_map(params![outlet, location], read_count)?;
        let Some(mut count) = rows.next().transpose()? else {
            return Ok(None);
        };
        drop(rows);
        count.lines = self.lines_of(&count.id)?;
        Ok(Some(count))
    }

    pub fn count(&self, outlet: &str, id: &str) -> Result<Option<StockCount>, DbError> {
        let sql =
            format!("SELECT {COUNT_COLUMNS} FROM stock_counts WHERE outlet_id = ?1 AND id = ?2");
        let mut stmt = self.tx.prepare(&sql)?;
        let mut rows = stmt.query_map(params![outlet, id], read_count)?;
        let Some(mut count) = rows.next().transpose()? else {
            return Ok(None);
        };
        drop(rows);
        count.lines = self.lines_of(&count.id)?;
        Ok(Some(count))
    }

    /// Every count in a period, newest first — including the abandoned ones, because "we gave
    /// up counting three times last month" is itself a finding.
    pub fn counts(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<StockCount>, DbError> {
        let sql = format!(
            "SELECT {COUNT_COLUMNS} FROM stock_counts
              WHERE outlet_id = ?1 AND business_day BETWEEN ?2 AND ?3
              ORDER BY business_day DESC, opened_at DESC"
        );
        let mut stmt = self.tx.prepare(&sql)?;
        let rows = stmt.query_map(
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            read_count,
        )?;
        let mut out: Vec<StockCount> = rows.collect::<Result<_, _>>()?;
        drop(stmt);
        for count in &mut out {
            count.lines = self.lines_of(&count.id)?;
        }
        Ok(out)
    }

    fn lines_of(&self, count: &str) -> Result<Vec<CountLine>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT l.seq, l.material_id, m.name, l.book_qty, l.book_at, l.counted_qty,
                    l.typed_qty, l.typed_unit, l.variance_qty, l.unit_cost, l.variance_value,
                    l.reason_id, l.note, l.movement_id
               FROM stock_count_lines l
               JOIN materials m ON m.id = l.material_id
              WHERE l.count_id = ?1 ORDER BY m.name",
        )?;
        let rows = stmt.query_map([count], |row| {
            Ok(CountLine {
                seq: row.get(0)?,
                material_id: MaterialId::new(row.get::<_, String>(1)?),
                material_name: row.get(2)?,
                book_qty: encode::qty_from_sql(row.get(3)?),
                book_at: encode::timestamp_from_sql(row.get(4)?),
                counted_qty: encode::qty_from_sql(row.get(5)?),
                typed_qty: encode::qty_from_sql(row.get(6)?),
                typed_unit: row.get(7)?,
                variance_qty: encode::qty_from_sql(row.get(8)?),
                unit_cost: UnitCost::from_paise_per_thousand(row.get(9)?),
                variance_value: encode::money_from_sql(row.get(10)?),
                reason_id: row.get(11)?,
                note: row.get(12)?,
                movement_id: row.get(13)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// Write down what is on the shelf, and freeze the book against it.
    pub fn record_line(
        &self,
        outlet: &str,
        count_id: &str,
        material: &MaterialId,
        written: &Written,
        at: Timestamp,
    ) -> Result<CountLine, DbError> {
        let (counted, typed, typed_unit) = (written.base, written.typed, written.unit.as_str());
        let Some(count) = self.count(outlet, count_id)? else {
            return Err(DbError::invariant("that count is not on file"));
        };
        if count.state != CountState::Draft {
            return Err(DbError::invariant(format!(
                "that count was {} and cannot be changed",
                count.state.label().to_lowercase()
            )));
        }
        if counted.is_negative() {
            return Err(DbError::invariant(
                "a counted quantity cannot be less than nothing",
            ));
        }

        let stock = StockRepo::new(self.tx);
        let book = stock.balance(outlet, material)?;
        let unit_cost = stock
            .costs(outlet)?
            .get(material)
            .copied()
            .unwrap_or(UnitCost::ZERO);
        let variance = counted
            .sub(book)
            .map_err(|e| DbError::invariant(format!("that count cannot be compared: {e}")))?;
        let variance_value = unit_cost.cost_of(variance).unwrap_or(Money::ZERO);

        // The sequence is stable per material, so counting the same thing twice corrects the
        // line rather than adding a second one — a person walking a store re-counts, and two
        // rows for one shelf is two truths again.
        let seq: i64 = self
            .tx
            .query_row(
                "SELECT seq FROM stock_count_lines WHERE count_id = ?1 AND material_id = ?2",
                params![count_id, material.as_str()],
                |row| row.get(0),
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => self.tx.query_row(
                    "SELECT COALESCE(MAX(seq), 0) + 1 FROM stock_count_lines WHERE count_id = ?1",
                    [count_id],
                    |row| row.get(0),
                ),
                other => Err(other),
            })?;

        self.tx.execute(
            "INSERT INTO stock_count_lines
                 (count_id, seq, material_id, book_qty, book_at, counted_qty, typed_qty,
                  typed_unit, variance_qty, unit_cost, variance_value, reason_id, note,
                  movement_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, NULL, NULL)
             ON CONFLICT (count_id, seq) DO UPDATE SET
                 book_qty = excluded.book_qty,
                 book_at = excluded.book_at,
                 counted_qty = excluded.counted_qty,
                 typed_qty = excluded.typed_qty,
                 typed_unit = excluded.typed_unit,
                 variance_qty = excluded.variance_qty,
                 unit_cost = excluded.unit_cost,
                 variance_value = excluded.variance_value",
            params![
                count_id,
                seq,
                material.as_str(),
                encode::qty_to_sql(book),
                encode::timestamp_to_sql(at),
                encode::qty_to_sql(counted),
                encode::qty_to_sql(typed),
                typed_unit,
                encode::qty_to_sql(variance),
                unit_cost.paise_per_thousand(),
                encode::money_to_sql(variance_value),
            ],
        )?;

        self.lines_of(count_id)?
            .into_iter()
            .find(|l| &l.material_id == material)
            .ok_or_else(|| DbError::invariant("the count line could not be read back"))
    }

    /// Why the shelf and the book disagree.
    pub fn explain_line(
        &self,
        outlet: &str,
        count_id: &str,
        material: &MaterialId,
        reason_id: Option<&str>,
        note: Option<&str>,
    ) -> Result<(), DbError> {
        let Some(count) = self.count(outlet, count_id)? else {
            return Err(DbError::invariant("that count is not on file"));
        };
        if count.state != CountState::Draft {
            return Err(DbError::invariant("an approved count cannot be changed"));
        }
        self.tx.execute(
            "UPDATE stock_count_lines SET reason_id = ?3, note = ?4
              WHERE count_id = ?1 AND material_id = ?2",
            params![count_id, material.as_str(), reason_id, note],
        )?;
        Ok(())
    }

    pub fn remove_line(
        &self,
        outlet: &str,
        count_id: &str,
        material: &MaterialId,
    ) -> Result<(), DbError> {
        let Some(count) = self.count(outlet, count_id)? else {
            return Err(DbError::invariant("that count is not on file"));
        };
        if count.state != CountState::Draft {
            return Err(DbError::invariant("an approved count cannot be changed"));
        }
        self.tx.execute(
            "DELETE FROM stock_count_lines WHERE count_id = ?1 AND material_id = ?2",
            params![count_id, material.as_str()],
        )?;
        Ok(())
    }

    /// Approve: post the deltas, stamp the materials, seal the count.
    pub fn approve(
        &self,
        outlet: &str,
        count_id: &str,
        at: Timestamp,
        day: BusinessDay,
        by: Option<&StaffId>,
    ) -> Result<usize, DbError> {
        let Some(count) = self.count(outlet, count_id)? else {
            return Err(DbError::invariant("that count is not on file"));
        };
        if count.state != CountState::Draft {
            return Err(DbError::invariant(format!(
                "that count was already {}",
                count.state.label().to_lowercase()
            )));
        }
        if count.lines.is_empty() {
            return Err(DbError::invariant(
                "nothing was counted, so there is nothing to approve",
            ));
        }

        let stock = StockRepo::new(self.tx);
        let mut moved = 0;
        for line in &count.lines {
            if !line.variance_qty.is_zero() {
                let movement_id = format!("mov_cnt_{}_{}", count.id, line.seq);
                let mut movement = Movement::new(
                    movement_id.clone(),
                    line.material_id.clone(),
                    MovementKind::Adjustment,
                    line.variance_qty,
                    at,
                    day,
                )
                // Valued at the cost frozen onto the line, so that a report of last month's
                // count does not change when a price does.
                .costing(line.unit_cost);
                movement.reason_id = line.reason_id.clone();
                movement.note = Some(format!("Counted on {}", count.business_day));
                if let Some(staff) = by {
                    movement.staff = Some(staff.clone());
                }
                stock.record(outlet, &movement)?;
                self.tx.execute(
                    "UPDATE stock_count_lines SET movement_id = ?3
                      WHERE count_id = ?1 AND seq = ?2",
                    params![count.id, line.seq, movement_id],
                )?;
                moved += 1;
            }

            self.tx.execute(
                "UPDATE materials SET last_counted_at = ?2 WHERE id = ?1",
                params![line.material_id.as_str(), encode::timestamp_to_sql(at)],
            )?;
        }

        self.tx.execute(
            "UPDATE stock_counts SET state = 'approved', approved_at = ?2, approved_by = ?3
              WHERE id = ?1",
            params![
                count.id,
                encode::timestamp_to_sql(at),
                by.map(StaffId::as_str)
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "stock_counts", &count.id, Op::Upsert, at)?;
        Ok(moved)
    }

    /// The variance history.
    pub fn variance_history(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<VarianceRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT m.name, m.dimension,
                    COUNT(*),
                    COALESCE(SUM(l.variance_qty), 0),
                    COALESCE(SUM(l.variance_value), 0),
                    MAX(c.business_day)
               FROM stock_count_lines l
               JOIN stock_counts c ON c.id = l.count_id
               JOIN materials m ON m.id = l.material_id
              WHERE c.outlet_id = ?1 AND c.state = 'approved'
                AND c.business_day BETWEEN ?2 AND ?3
              GROUP BY m.id
              ORDER BY COALESCE(SUM(l.variance_value), 0)",
        )?;
        let rows = stmt.query_map(
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            |row| {
                let dimension: String = row.get(1)?;
                Ok(VarianceRow {
                    name: row.get(0)?,
                    unit: mb_core::Dimension::from_tag(&dimension)
                        .map(|d| d.base_unit().to_owned())
                        .unwrap_or_default(),
                    counts: row.get(2)?,
                    variance_qty: encode::qty_from_sql(row.get(3)?),
                    variance_value: encode::money_from_sql(row.get(4)?),
                    last_counted: BusinessDay::from_days_since_epoch(
                        i32::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
                    ),
                })
            },
        )?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// Give up on a count, with a reason.
    pub fn abandon(
        &self,
        outlet: &str,
        count_id: &str,
        reason: &str,
        at: Timestamp,
    ) -> Result<(), DbError> {
        if reason.trim().is_empty() {
            return Err(DbError::invariant("giving up on a count needs a reason"));
        }
        let Some(count) = self.count(outlet, count_id)? else {
            return Err(DbError::invariant("that count is not on file"));
        };
        if count.state != CountState::Draft {
            return Err(DbError::invariant("that count is already finished"));
        }
        self.tx.execute(
            "UPDATE stock_counts SET state = 'abandoned', ended_reason = ?2 WHERE id = ?1",
            params![count_id, reason.trim()],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "stock_counts", count_id, Op::Upsert, at)
    }
}

fn read_count(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockCount> {
    Ok(StockCount {
        id: row.get(0)?,
        location: row.get(1)?,
        state: match row.get::<_, String>(2)?.as_str() {
            "approved" => CountState::Approved,
            "abandoned" => CountState::Abandoned,
            _ => CountState::Draft,
        },
        business_day: BusinessDay::from_days_since_epoch(
            i32::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
        ),
        opened_at: encode::timestamp_from_sql(row.get(4)?),
        opened_by: row.get::<_, Option<String>>(5)?.map(StaffId::new),
        approved_at: row
            .get::<_, Option<i64>>(6)?
            .map(encode::timestamp_from_sql),
        approved_by: row.get::<_, Option<String>>(7)?.map(StaffId::new),
        ended_reason: row.get(8)?,
        note: row.get(9)?,
        lines: Vec::new(),
    })
}
