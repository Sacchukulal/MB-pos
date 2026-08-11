//! Reading the schema back out of the database.
//!
//! These helpers exist so the rules this schema claims to obey are *checked*
//! rather than *believed*. Almost every caller is a test — that is the point.
//! A comment saying "no REAL columns" is a wish; a function that walks
//! `sqlite_master` and a test that fails the build is a rule.

use rusqlite::Connection;

use crate::error::DbError;

/// One column, as SQLite reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// The declared type, upper-cased. In a STRICT table this is one of INT,
    /// INTEGER, REAL, TEXT, BLOB or ANY — SQLite refuses to create the table
    /// otherwise, which is what makes `BOOLEAN` and `VARCHAR` impossible here.
    pub decl_type: String,
    pub not_null: bool,
    pub is_primary_key: bool,
}

/// One foreign key, as SQLite reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKey {
    pub from_column: String,
    pub to_table: String,
    pub to_column: Option<String>,
    /// `NO ACTION`, `CASCADE`, `RESTRICT`, `SET NULL`, `SET DEFAULT`.
    pub on_delete: String,
}

/// Every ordinary table, in name order. Excludes views and SQLite's own
/// `sqlite_*` bookkeeping.
pub fn tables(conn: &Connection) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master
          WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
          ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Every view, in name order.
pub fn views(conn: &Connection) -> Result<Vec<String>, DbError> {
    let mut stmt =
        conn.prepare("SELECT name FROM sqlite_master WHERE type = 'view' ORDER BY name")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Every index this schema created, by name. Excludes the automatic indexes
/// SQLite makes for UNIQUE and PRIMARY KEY, which have no name of ours.
pub fn indexes(conn: &Connection) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master
          WHERE type = 'index' AND name NOT LIKE 'sqlite_%'
          ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// The columns of one table, in declaration order.
pub fn columns(conn: &Connection, table: &str) -> Result<Vec<Column>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT name, type, \"notnull\", pk FROM pragma_table_info(?1) ORDER BY cid",
    )?;
    let rows = stmt.query_map([table], |row| {
        Ok(Column {
            name: row.get::<_, String>(0)?,
            decl_type: row.get::<_, String>(1)?.to_uppercase(),
            not_null: row.get::<_, i64>(2)? != 0,
            is_primary_key: row.get::<_, i64>(3)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// The foreign keys of one table.
pub fn foreign_keys(conn: &Connection, table: &str) -> Result<Vec<ForeignKey>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT \"from\", \"table\", \"to\", on_delete FROM pragma_foreign_key_list(?1)",
    )?;
    let rows = stmt.query_map([table], |row| {
        Ok(ForeignKey {
            from_column: row.get::<_, String>(0)?,
            to_table: row.get::<_, String>(1)?,
            to_column: row.get::<_, Option<String>>(2)?,
            on_delete: row.get::<_, String>(3)?.to_uppercase(),
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// The `CREATE TABLE` text SQLite stored for one table.
///
/// Used to check for things `pragma_table_info` does not expose: `STRICT`,
/// `AUTOINCREMENT`, and whether a boolean column carries a CHECK naming it.
pub fn create_sql(conn: &Connection, name: &str) -> Result<String, DbError> {
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .map_err(DbError::from)?;
    sql.ok_or_else(|| DbError::invariant(format!("{name} is not in this database")))
}

/// True if a boolean column's name follows the convention the schema promises.
///
/// The convention is load-bearing: it is what lets a test enumerate every
/// boolean in the whole database and check that each one is INTEGER, NOT NULL
/// and constrained to 0/1, without carrying a hand-written list that will drift.
#[must_use]
pub fn is_boolean_name(name: &str) -> bool {
    const PREFIXES: [&str; 4] = ["is_", "has_", "was_", "can_"];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

/// The tables that own their own rows, as opposed to child tables that reach an
/// outlet through their parent.
///
/// This list is deliberately hand-written. Adding a table without an
/// `outlet_id` should be a decision somebody makes on purpose, with a diff
/// against it — scope 11.4 is the dimension that cannot be retro-fitted.
pub const ROOT_TABLES: &[&str] = &[
    "audit_log",
    "applied_events",
    "categories",
    "combos",
    "customer_payments",
    "customers",
    "day_closes",
    "dining_tables",
    "expense_categories",
    "expenses",
    "items",
    "material_balances",
    "materials",
    "modifier_groups",
    "orders",
    "recipes",
    "stock_day_closes",
    "stock_movements",
    "stock_problems",
    "printers",
    "reservations",
    "roles",
    "sections",
    "staff",
    "sync_outbox",
    "terminals",
    "waitlist",
];

/// Tables whose rows are money or evidence, and which therefore may never be
/// reached by an `ON DELETE CASCADE`.
///
/// A bill is not deleted. It is voided, which is a state, not an absence.
pub const MONEY_PATH_TABLES: &[&str] = &[
    "bill_charges",
    "bill_lines",
    "bill_tax_rows",
    "bills",
    "customer_payments",
    "expenses",
    "order_lines",
    "orders",
    "payments",
];
