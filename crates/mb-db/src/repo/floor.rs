//! Sections and tables — what P09's grid draws and P14 arranges.

use mb_core::{TableId, Timestamp};
use rusqlite::OptionalExtension;
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: String,
    pub name: String,
    pub sort_order: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiningTable {
    pub id: TableId,
    pub section_id: Option<String>,
    /// What the waiter says: "6", "AC 1".
    pub label: String,
    pub seats: i64,
    /// Scope 14.1, the floor plan. `None` until the table is placed.
    pub pos: Option<(i64, i64)>,
    pub sort_order: i64,
    pub is_active: bool,
}

#[derive(Debug)]
pub struct FloorRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> FloorRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        FloorRepo { tx }
    }

    pub fn save_section(
        &self,
        outlet: &str,
        section: &Section,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO sections (id, outlet_id, name, sort_order, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name,
                                            sort_order = excluded.sort_order,
                                            is_active = excluded.is_active",
            rusqlite::params![
                section.id,
                outlet,
                section.name,
                section.sort_order,
                encode::bool_to_sql(section.is_active),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "sections", &section.id, Op::Upsert, at)
    }

    pub fn list_sections(&self, outlet: &str) -> Result<Vec<Section>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, sort_order, is_active FROM sections
              WHERE outlet_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, sort_order, is_active) = row?;
            out.push(Section {
                id,
                name,
                sort_order,
                is_active: encode::bool_from_sql(is_active, "sections.is_active")?,
            });
        }
        Ok(out)
    }

    /// Refuses a label that another active table already prints.
    ///
    /// Crown jewel 19: v1's table master "refuses two tables that would print
    /// the same name", and it is worth keeping — two tables called `1` produce
    /// two kitchen tickets nobody can tell apart.
    ///
    /// # P14 replaced the rule that was here
    ///
    /// The first version compared bare labels across the whole outlet in SQL:
    ///
    /// ```sql
    /// AND lower(trim(label)) = lower(trim(?3))
    /// ```
    ///
    /// That is wrong in both directions. It **refused** table `1` in the AC
    /// room when a table `1` already existed in the Garden — which is the most
    /// ordinary table layout in India — and it **allowed** a table literally
    /// named `AC 1` beside section AC's table `1`, which is loophole I5 itself,
    /// the exact case it was written to stop.
    ///
    /// The rule now lives in `mb_core::table` and is section-aware, and the
    /// billing screen's free-text table box calls the same function — that
    /// second door being unguarded is the other half of I5.
    pub fn save_table(
        &self,
        outlet: &str,
        table: &DiningTable,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.refuse_clash(outlet, table)?;
        self.check_cell(outlet, &table.id, table.pos)?;

        let (x, y) = match table.pos {
            Some((x, y)) => (Some(x), Some(y)),
            None => (None, None),
        };
        self.tx.execute(
            "INSERT INTO dining_tables (id, outlet_id, section_id, label, seats, pos_x, pos_y,
                                        sort_order, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (id) DO UPDATE SET section_id = excluded.section_id,
                                            label      = excluded.label,
                                            seats      = excluded.seats,
                                            pos_x      = excluded.pos_x,
                                            pos_y      = excluded.pos_y,
                                            sort_order = excluded.sort_order,
                                            is_active  = excluded.is_active",
            rusqlite::params![
                table.id.as_str(),
                outlet,
                table.section_id,
                table.label,
                table.seats,
                x,
                y,
                table.sort_order,
                encode::bool_to_sql(table.is_active),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "dining_tables", table.id.as_str(), Op::Upsert, at)
    }

    pub fn list_tables(&self, outlet: &str) -> Result<Vec<DiningTable>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, section_id, label, seats, pos_x, pos_y, sort_order, is_active
               FROM dining_tables WHERE outlet_id = ?1 ORDER BY sort_order, label",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, section_id, label, seats, x, y, sort_order, is_active) = row?;
            out.push(DiningTable {
                id: TableId::new(id),
                section_id,
                label,
                seats,
                pos: match (x, y) {
                    (Some(x), Some(y)) => Some((x, y)),
                    _ => None,
                },
                sort_order,
                is_active: encode::bool_from_sql(is_active, "dining_tables.is_active")?,
            });
        }
        Ok(out)
    }

    /// The name check, section-aware, against everything the shop already has.
    ///
    /// Compares against **hidden tables too**. A hidden table is off the floor,
    /// not gone: it still prints its name on the bills it already has, and
    /// letting a second table take its name means two different tables share a
    /// name in one shop's history.
    fn refuse_clash(&self, outlet: &str, table: &DiningTable) -> Result<(), DbError> {
        let existing = self.list_tables(outlet)?;
        let sections = self.list_sections(outlet)?;
        let name_of = |id: &Option<String>| {
            id.as_ref()
                .and_then(|id| sections.iter().find(|s| &s.id == id))
                .map(|s| s.name.clone())
        };

        let mine = name_of(&table.section_id);
        let rows: Vec<(String, Option<String>, String)> = existing
            .iter()
            .filter(|other| other.id != table.id)
            .map(|other| (other.id.as_str().to_owned(), name_of(&other.section_id), other.label.clone()))
            .collect();

        let clash = mb_core::table::clashes_with(
            mine.as_deref(),
            &table.label,
            rows.iter().map(|(id, section, label)| {
                (id.as_str(), section.as_deref(), label.as_str())
            }),
        )
        .map_err(|e| DbError::invariant(e.to_string()))?;

        if let Some(id) = clash {
            // The id came out of `rows`, so this cannot miss — but an
            // `expect` in a repository is a panic in a shop, so the
            // unreachable branch produces a message instead.
            let Some((_, section, label)) = rows.iter().find(|(other, _, _)| other == id) else {
                return Err(DbError::invariant("that name is already taken"));
            };
            let printed = mb_core::table::printed_name(section.as_deref(), label);
            return Err(DbError::invariant(match section {
                Some(section) => format!(
                    "\"{printed}\" already exists in {section} — two tables that print the \
                     same name produce two kitchen tickets nobody can tell apart"
                ),
                None => format!(
                    "\"{printed}\" already exists — two tables that print the same name \
                     produce two kitchen tickets nobody can tell apart"
                ),
            }));
        }
        Ok(())
    }

    /// Is this square on the plan, and is it free?
    ///
    /// Called by BOTH doors — `place` (a drag) and `save_table` (an edit that
    /// happens to carry a position). One door guarded and one open is exactly
    /// the shape of loophole I5, which this file has already had to fix once.
    fn check_cell(
        &self,
        outlet: &str,
        table: &TableId,
        pos: Option<(i64, i64)>,
    ) -> Result<(), DbError> {
        let Some((x, y)) = pos else { return Ok(()) };

        if !(0..GRID_CELLS).contains(&x) || !(0..GRID_CELLS).contains(&y) {
            return Err(DbError::invariant("that is off the edge of the floor plan"));
        }
        let taken: Option<String> = self
            .tx
            .query_row(
                "SELECT label FROM dining_tables
                  WHERE outlet_id = ?1 AND id <> ?2 AND pos_x = ?3 AND pos_y = ?4",
                rusqlite::params![outlet, table.as_str(), x, y],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(label) = taken {
            return Err(DbError::invariant(format!(
                "{label} is already there — drop this one on an empty square"
            )));
        }
        Ok(())
    }

    /// Add a whole range in one go — "tables 1 to 20 in the AC room".
    ///
    /// **All or nothing.** If any label in the range would clash, none of them
    /// is created: a shop that asked for 1-20 and silently got 14 of them has
    /// to find out which six are missing by counting, and a half-done bulk
    /// action is the same failure the CSV import refuses at P13.
    pub fn add_range(
        &self,
        outlet: &str,
        range: &Range,
        at: Timestamp,
    ) -> Result<Vec<TableId>, DbError> {
        let Range { section_id, prefix, from, to, seats } = range;
        let (from, to, seats) = (*from, *to, *seats);
        let section_id = section_id.as_deref();
        let labels = mb_core::table::range_labels(from, to, prefix)
            .map_err(|_| DbError::invariant("that is not a range of tables — try 1 to 20"))?;

        let next_sort = self
            .list_tables(outlet)?
            .iter()
            .map(|t| t.sort_order)
            .max()
            .unwrap_or(0);

        let mut made = Vec::with_capacity(labels.len());
        for (n, label) in labels.into_iter().enumerate() {
            let table = DiningTable {
                // The id is derived from the label so running the same range
                // twice is idempotent rather than duplicating the room.
                id: TableId::new(format!("tbl_{}", slug(outlet, section_id, &label))),
                section_id: section_id.map(str::to_owned),
                label,
                seats,
                pos: None,
                sort_order: next_sort + 1 + i64::try_from(n).unwrap_or(0),
                is_active: true,
            };
            self.save_table(outlet, &table, at)?;
            made.push(table.id);
        }
        Ok(made)
    }

    /// Where a table sits on the floor plan (scope 14.1).
    ///
    /// `None` takes it off the plan and back into the section grid, which is
    /// the fallback every shop starts in.
    ///
    /// **Two tables may not share a cell.** The plan is a grid on purpose:
    /// "the same place" is then a comparison of two integers rather than a
    /// rectangle intersection, and a tile hidden underneath another tile is a
    /// table that has vanished from the floor.
    pub fn place(
        &self,
        outlet: &str,
        table: &TableId,
        pos: Option<(i64, i64)>,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.check_cell(outlet, table, pos)?;

        let (x, y) = match pos {
            Some((x, y)) => (Some(x), Some(y)),
            None => (None, None),
        };
        let changed = self.tx.execute(
            "UPDATE dining_tables SET pos_x = ?3, pos_y = ?4 WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, table.as_str(), x, y],
        )?;
        if changed == 0 {
            return Err(DbError::invariant("that table is not on the floor any more"));
        }
        OutboxRepo::new(self.tx).enqueue(outlet, "dining_tables", table.as_str(), Op::Upsert, at)
    }

    /// The first empty square, scanning left to right and top to bottom.
    ///
    /// A table added after a layout exists has to land somewhere **visible**:
    /// (0,0) would put it under an existing tile, and no position at all would
    /// drop it out of a plan the owner is looking at.
    pub fn first_free_cell(&self, outlet: &str) -> Result<(i64, i64), DbError> {
        let taken: Vec<(i64, i64)> = self
            .list_tables(outlet)?
            .into_iter()
            .filter_map(|t| t.pos)
            .collect();
        for y in 0..GRID_CELLS {
            for x in 0..GRID_CELLS {
                if !taken.contains(&(x, y)) {
                    return Ok((x, y));
                }
            }
        }
        Err(DbError::invariant("the floor plan is full"))
    }

    /// How many orders have ever pointed at this table.
    ///
    /// The number a refusal quotes: *"Table 6 has 38 bills against it. It can
    /// be hidden, which takes it off the floor and keeps its history."*
    pub fn orders_against(&self, table: &TableId) -> Result<i64, DbError> {
        Ok(self.tx.query_row(
            "SELECT count(*) FROM orders WHERE table_id = ?1",
            [table.as_str()],
            |r| r.get(0),
        )?)
    }

    /// The open order sitting at this table, if there is one — id and whatever
    /// it is called on screen (its token, falling back to its bill number).
    pub fn open_order_at(&self, table: &TableId) -> Result<Option<(String, String)>, DbError> {
        Ok(self
            .tx
            .query_row(
                "SELECT id, coalesce(token_formatted, bill_number_formatted, id)
                   FROM orders WHERE table_id = ?1 AND state IN ('draft', 'open')
                  ORDER BY created_at LIMIT 1",
                [table.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// Take a table off the floor, or put it back.
    ///
    /// **Hiding is what "delete" usually means here**, and the difference is
    /// explained where a shopkeeper meets it rather than left to a foreign key.
    pub fn set_active(
        &self,
        outlet: &str,
        table: &TableId,
        active: bool,
        at: Timestamp,
    ) -> Result<(), DbError> {
        if !active
            && let Some((_, called)) = self.open_order_at(table)?
        {
            return Err(DbError::invariant(format!(
                "there is an open order on this table ({called}) — settle or cancel it first"
            )));
        }
        self.tx.execute(
            "UPDATE dining_tables SET is_active = ?3 WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, table.as_str(), encode::bool_to_sql(active)],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "dining_tables", table.as_str(), Op::Upsert, at)
    }

    /// Really delete a table — **only ever possible for one nothing points at.**
    ///
    /// A table that has taken a single order keeps that order's history
    /// forever (`orders.table_id` is a foreign key), so this succeeds for a
    /// mistyped table added five minutes ago and refuses for a real one, with
    /// a message that says what to do instead.
    pub fn delete_table(&self, outlet: &str, table: &TableId, at: Timestamp) -> Result<(), DbError> {
        if let Some((_, called)) = self.open_order_at(table)? {
            return Err(DbError::invariant(format!(
                "there is an open order on this table ({called}) — settle or cancel it first"
            )));
        }
        let history = self.orders_against(table)?;
        if history > 0 {
            return Err(DbError::invariant(format!(
                "this table has {history} order(s) against it. Hide it instead — that takes \
                 it off the floor and keeps its history"
            )));
        }
        self.tx.execute(
            "DELETE FROM dining_tables WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, table.as_str()],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "dining_tables", table.as_str(), Op::Delete, at)
    }

    /// Delete a section. Refused while tables are still in it, and the refusal
    /// says how many so an owner knows what they are being asked to move.
    pub fn delete_section(&self, outlet: &str, section: &str, at: Timestamp) -> Result<(), DbError> {
        let holding: i64 = self.tx.query_row(
            "SELECT count(*) FROM dining_tables WHERE section_id = ?1",
            [section],
            |r| r.get(0),
        )?;
        if holding > 0 {
            return Err(DbError::invariant(format!(
                "{holding} table(s) are still in this section — move them somewhere else first"
            )));
        }
        self.tx.execute(
            "DELETE FROM sections WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, section],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "sections", section, Op::Delete, at)
    }

    /// What the floor looks like right now, in numbers — scope 14.3.
    ///
    /// Computed here in one pass rather than by the screen, for the reason R8
    /// gives and one more: **a summary that disagrees with the grid under it is
    /// worse than no summary**, so both read the same rows in the same
    /// transaction.
    pub fn occupancy(&self, outlet: &str, day: mb_core::BusinessDay) -> Result<Occupancy, DbError> {
        let tables: i64 = self.tx.query_row(
            "SELECT count(*) FROM dining_tables WHERE outlet_id = ?1 AND is_active = 1",
            [outlet],
            |r| r.get(0),
        )?;
        let (busy, covers): (i64, Option<i64>) = self.tx.query_row(
            "SELECT count(DISTINCT table_id), sum(covers) FROM orders
              WHERE outlet_id = ?1 AND state IN ('draft', 'open') AND table_id IS NOT NULL",
            [outlet],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        let day = encode::business_day_to_sql(day);
        // A "turn" is a dine-in order that finished today: the table was used
        // and freed. An open order is not a turn yet, which is why the two
        // figures are counted separately rather than added.
        let (turns, seated_covers): (i64, Option<i64>) = self.tx.query_row(
            "SELECT count(*), sum(covers) FROM orders
              WHERE outlet_id = ?1 AND business_day = ?2 AND order_type = 'dine_in'
                AND state = 'settled'",
            rusqlite::params![outlet, day],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        // Rounded to a whole minute BY SQLITE, so no float ever reaches Rust and
        // there is no cast to argue about. "Average time at table" is a number
        // an owner glances at; a fractional minute would be noise.
        let average_minutes: Option<i64> = self.tx.query_row(
            "SELECT CAST(round(avg(settled_at - created_at) / 60000.0) AS INTEGER) FROM orders
              WHERE outlet_id = ?1 AND business_day = ?2 AND order_type = 'dine_in'
                AND state = 'settled' AND settled_at IS NOT NULL",
            rusqlite::params![outlet, day],
            |r| r.get(0),
        )?;

        Ok(Occupancy {
            tables,
            busy,
            covers_now: covers.unwrap_or(0),
            turns,
            covers_today: seated_covers.unwrap_or(0),
            average_minutes,
        })
    }

    /// Record that one order's food went onto another's bill (scope 1.22).
    ///
    /// The link is a column rather than a sentence in the reason, so a report
    /// can tell a merge from a walkout without reading English.
    pub fn record_merge(&self, absorbed: &str, survivor: &str) -> Result<(), DbError> {
        let changed = self.tx.execute(
            "UPDATE orders SET merged_into = ?2 WHERE id = ?1",
            rusqlite::params![absorbed, survivor],
        )?;
        if changed == 0 {
            return Err(DbError::invariant("that order is not here any more"));
        }
        Ok(())
    }
}

/// The floor, in numbers a shopkeeper glances at — scope 14.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Occupancy {
    pub tables: i64,
    pub busy: i64,
    /// Covers sitting down right now. Orders with no cover count contribute
    /// nothing rather than one — see `OrderCore::covers`.
    pub covers_now: i64,
    pub turns: i64,
    pub covers_today: i64,
    /// `None` until something has been settled today; there is no honest
    /// average of nothing.
    pub average_minutes: Option<i64>,
}

/// How many squares the floor plan is, each way.
///
/// A fixed grid rather than free pixels: see [`FloorRepo::place`]. Sixteen is
/// enough for a 60-table room at four squares per table and small enough that
/// the whole plan fits a 1366x768 screen without scrolling, which is the
/// reference machine (D12).
pub const GRID_CELLS: i64 = 16;

/// A stable id from a label, so re-running "add 1 to 20" does not make a
/// second set of twenty tables.
fn slug(outlet: &str, section: Option<&str>, label: &str) -> String {
    let mut out = String::new();
    for part in [outlet, section.unwrap_or("none"), label] {
        for ch in part.chars() {
            out.push(if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '_' });
        }
        out.push('_');
    }
    out
}


/// What "add tables 1 to 20" means — one argument instead of six, because six
/// positional parameters of the same two types is a call site nobody can read
/// and clippy is right about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    pub section_id: Option<String>,
    /// "G" makes G1, G2, G3. Empty makes 1, 2, 3.
    pub prefix: String,
    pub from: i64,
    pub to: i64,
    pub seats: i64,
}
