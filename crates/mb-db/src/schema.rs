//! Reading the schema back out of the database.

use rusqlite::Connection;

use crate::error::DbError;

/// One column, as SQLite reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// The declared type, upper-cased.
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

/// Every ordinary table, in name order.
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

/// Every index this schema created, by name.
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
    let mut stmt =
        conn.prepare("SELECT name, type, \"notnull\", pk FROM pragma_table_info(?1) ORDER BY cid")?;
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
#[must_use]
pub fn is_boolean_name(name: &str) -> bool {
    const PREFIXES: [&str; 4] = ["is_", "has_", "was_", "can_"];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

/// The tables that own their own rows, as opposed to child tables that reach an outlet through
/// their parent.
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

/// Tables whose rows are money or evidence, and which therefore may never be reached by an `ON
/// DELETE CASCADE`.
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
