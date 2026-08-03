//! Sections and tables — what P09's grid draws and P14 arranges.

use mb_core::{TableId, Timestamp};
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
    pub fn save_table(
        &self,
        outlet: &str,
        table: &DiningTable,
        at: Timestamp,
    ) -> Result<(), DbError> {
        let clash: i64 = self.tx.query_row(
            "SELECT count(*) FROM dining_tables
              WHERE outlet_id = ?1 AND id <> ?2 AND is_active = 1
                AND lower(trim(label)) = lower(trim(?3))",
            rusqlite::params![outlet, table.id.as_str(), table.label],
            |r| r.get(0),
        )?;
        if clash > 0 {
            return Err(DbError::invariant(format!(
                "there is already a table called \"{}\" — two tables with the same \
                 name print two tickets nobody can tell apart",
                table.label
            )));
        }

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
}
