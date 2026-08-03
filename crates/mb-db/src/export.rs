//! "Give me my whole shop's data" — audit **A6** — and the CSV bug from **G7**.
//!
//! > A6: *"No export of the raw data. There is CSV export per report, but there
//! > is no 'give me my whole shop's data' button. An owner who leaves has no
//! > way to take their data with them; this is also a data-protection weak
//! > point."*
//!
//! One folder: one CSV per table, plus a copy of the database itself, plus the
//! manifest. **Not a zip** — that is a dependency for a convenience, and a
//! folder the owner can open and read beats an archive they have to extract.
//! If P22 later wants one file to attach to an email it can zip the folder.
//!
//! # The CSV writer, and why it is not a crate
//!
//! > G7: *"CSV export builds text by joining with commas. An item name
//! > containing a comma ('Chicken Biryani, Half') will break the columns of
//! > that row."*
//! >
//! > **Fix:** *"proper CSV escaping. Small bug, silently wrong data."*
//!
//! Doing it correctly is about thirty lines and four rules, and every one of
//! the four is a bug v1 had or would have had:
//!
//! 1. A field containing a comma, a double quote, a CR or an LF is wrapped in
//!    double quotes. (The G7 bug itself.)
//! 2. A double quote inside a quoted field is doubled.
//! 3. `NULL` is an **empty unquoted** field; an empty string is `""`. Those are
//!    different values and a round trip must keep them different — which is the
//!    one thing a naive writer always loses, and the reason a shop's optional
//!    note column comes back as an empty string on every row.
//! 4. Line endings are CRLF, per RFC 4180, because the owner will open this in
//!    Excel.
//!
//! And a fifth rule that is ours rather than the RFC's: **money is written as
//! integer paise, not as `123.45`.** A spreadsheet will happily reinterpret
//! `123.45` as a float and hand it back rounded, which is D2 undone by
//! Excel. The paise go in the file and the word "paise" goes in the column
//! header.

use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;

use crate::conn::Db;
use crate::error::DbError;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub folder: PathBuf,
    /// `(table, rows written)`.
    pub tables: Vec<(String, usize)>,
    pub database_copy: PathBuf,
}

/// Write the whole shop out as CSV plus a copy of the database.
pub fn export_all(db: &Db, to: &Path, app_version: &str) -> Result<ExportReport, DbError> {
    std::fs::create_dir_all(to)
        .map_err(|e| DbError::invariant(format!("could not create {}: {e}", to.display())))?;

    let tables = db.read(schema::tables)?;
    let mut written = Vec::with_capacity(tables.len());

    for table in &tables {
        let rows = db.read(|conn| export_table(conn, table))?;
        let path = to.join(format!("{table}.csv"));
        std::fs::write(&path, rows.text.as_bytes()).map_err(|e| {
            DbError::invariant(format!("could not write {}: {e}", path.display()))
        })?;
        written.push((table.clone(), rows.count));
    }

    // The raw file too. A CSV is what an owner reads; the database is what
    // actually restores.
    let database_copy = to.join("shop.db");
    if database_copy.exists() {
        std::fs::remove_file(&database_copy).map_err(|e| {
            DbError::invariant(format!("could not replace the exported database: {e}"))
        })?;
    }
    db.backup_to(&database_copy)?;

    let readme = format!(
        "Magic Bill export\n\
         app version {app_version}\n\
         \n\
         One CSV per table, plus shop.db which is the database itself.\n\
         Amounts are whole PAISE, not rupees: 12345 means Rs 123.45.\n\
         Quantities are THOUSANDTHS: 500 means 0.5.\n\
         Times are milliseconds since 1970-01-01 UTC.\n\
         Business days are whole days since 1970-01-01.\n\
         An empty field is NULL; \"\" is an empty string. They are different.\n"
    );
    std::fs::write(to.join("README.txt"), readme.as_bytes())
        .map_err(|e| DbError::invariant(format!("could not write the export readme: {e}")))?;

    Ok(ExportReport {
        folder: to.to_path_buf(),
        tables: written,
        database_copy,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub dry_run: bool,
    /// `(table, rows that would be / were imported)`.
    pub tables: Vec<(String, usize)>,
}

/// Bring a shop back from an export folder.
///
/// **Dry run first, by default in the caller's hands**: `dry_run` reports what
/// would happen and touches nothing. And an import into a database that already
/// has orders in it is refused unless `force` — importing a shop over a trading
/// shop is not a thing anybody means to do.
pub fn import_all(
    db: &Db,
    from: &Path,
    dry_run: bool,
    force: bool,
) -> Result<ImportReport, DbError> {
    let existing: i64 = db.read(|conn| {
        Ok(conn.query_row("SELECT count(*) FROM orders", [], |r| r.get::<_, i64>(0))?)
    })?;
    if existing > 0 && !force {
        return Err(DbError::invariant(format!(
            "this database already has {existing} order(s) — importing over a \
             trading shop would mix two shops together. Restore a backup \
             instead, or pass force if you really mean it."
        )));
    }

    let tables = db.read(schema::tables)?;

    // Read and parse everything BEFORE touching the database, so a malformed
    // CSV halfway through the folder cannot leave a half-imported shop.
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut report = Vec::new();
    for table in &tables {
        if table == "schema_version" {
            // The engine owns the ledger. Importing someone else's is how a
            // database ends up claiming migrations it has not run.
            continue;
        }
        let Ok(text) = std::fs::read_to_string(from.join(format!("{table}.csv"))) else {
            continue;
        };
        let rows = parse_csv(&text);
        let Some((header, body)) = rows.split_first() else {
            continue;
        };
        report.push((table.clone(), body.len()));
        sheets.push((table.clone(), header.clone(), body.to_vec()));
    }

    if dry_run {
        return Ok(ImportReport {
            dry_run,
            tables: report,
        });
    }

    // **One transaction for the whole import**, with foreign keys DEFERRED to
    // the commit rather than switched off.
    //
    // The tables arrive in name order, so `bill_charges` is written long before
    // the `orders` it points at. Checking each row as it lands would fail on
    // the ordering and prove nothing; switching foreign keys off would import a
    // broken shop in silence. Deferring checks every constraint at COMMIT, once
    // every row is present — so a genuinely inconsistent export still fails,
    // and fails as a whole.
    db.transaction(|tx| {
        tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;

        // Children first, so the deletes do not trip a constraint either.
        for (table, _, _) in sheets.iter().rev() {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }

        for (table, header, body) in &sheets {
            if body.is_empty() {
                continue;
            }
            let columns = header
                .iter()
                .map(|c| c.clone().unwrap_or_default())
                .collect::<Vec<_>>()
                .join(", ");
            let placeholders = (1..=header.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!("INSERT INTO {table} ({columns}) VALUES ({placeholders})");
            let mut stmt = tx.prepare(&sql)?;
            for row in body {
                let params: Vec<Option<&str>> = row.iter().map(|f| f.as_deref()).collect();
                stmt.execute(rusqlite::params_from_iter(params))?;
            }
        }
        Ok(())
    })?;

    Ok(ImportReport {
        dry_run,
        tables: report,
    })
}

// ---------------------------------------------------------------------------
// The CSV writer and reader.
// ---------------------------------------------------------------------------

struct Csv {
    text: String,
    count: usize,
}

/// One parsed CSV, held whole before anything is written: `(table, header,
/// rows)`. A field is `None` when the CSV said NULL and `Some` otherwise —
/// keeping those apart is rule 3 in the module header.
type Sheet = (String, Vec<Option<String>>, Vec<Vec<Option<String>>>);

fn export_table(conn: &rusqlite::Connection, table: &str) -> Result<Csv, DbError> {
    let columns = schema::columns(conn, table)?;
    let names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();

    let mut text = String::new();
    write_row(&mut text, names.iter().map(|n| Some(n.as_str())));

    let sql = format!("SELECT {} FROM {table}", names.join(", "));
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let mut cells: Vec<Option<String>> = Vec::with_capacity(names.len());
        for i in 0..names.len() {
            cells.push(match row.get_ref(i)? {
                ValueRef::Null => None,
                ValueRef::Integer(v) => Some(v.to_string()),
                // There is no REAL column in this schema (D25), so reaching
                // this arm means somebody has changed something they should
                // not have. Write it rather than lose it, and let the schema
                // test be the thing that fails.
                ValueRef::Real(v) => Some(v.to_string()),
                ValueRef::Text(v) => Some(String::from_utf8_lossy(v).into_owned()),
                ValueRef::Blob(v) => Some(hex(v)),
            });
        }
        write_row(&mut text, cells.iter().map(std::option::Option::as_deref));
        count += 1;
    }
    Ok(Csv { text, count })
}

/// Rule 3 lives here: `None` writes nothing at all, `Some("")` writes `""`.
fn write_row<'a>(out: &mut String, cells: impl Iterator<Item = Option<&'a str>>) {
    let mut first = true;
    for cell in cells {
        if !first {
            out.push(',');
        }
        first = false;
        match cell {
            None => {}
            Some(value) => out.push_str(&escape(value)),
        }
    }
    // RFC 4180, and Excel.
    out.push_str("\r\n");
}

fn escape(value: &str) -> String {
    let needs_quotes = value.is_empty()
        || value.contains([',', '"', '\r', '\n'])
        || value.starts_with(' ')
        || value.ends_with(' ');
    if !needs_quotes {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// The inverse. `None` for an unquoted empty field, `Some("")` for `""`.
fn parse_csv(text: &str) -> Vec<Vec<Option<String>>> {
    let mut rows = Vec::new();
    let mut row: Vec<Option<String>> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut was_quoted = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => {
                quoted = true;
                was_quoted = true;
            }
            ',' => {
                row.push(finish(&mut field, &mut was_quoted));
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(finish(&mut field, &mut was_quoted));
                rows.push(std::mem::take(&mut row));
            }
            '\n' => {
                row.push(finish(&mut field, &mut was_quoted));
                rows.push(std::mem::take(&mut row));
            }
            other => field.push(other),
        }
    }
    if !field.is_empty() || was_quoted || !row.is_empty() {
        row.push(finish(&mut field, &mut was_quoted));
        rows.push(row);
    }
    rows
}

fn finish(field: &mut String, was_quoted: &mut bool) -> Option<String> {
    let value = std::mem::take(field);
    let quoted = std::mem::replace(was_quoted, false);
    if value.is_empty() && !quoted {
        None
    } else {
        Some(value)
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_g7_bug_cannot_happen() {
        // "Chicken Biryani, Half" is the auditor's own example.
        let mut out = String::new();
        write_row(
            &mut out,
            [Some("Chicken Biryani, Half"), Some("120"), None].into_iter(),
        );
        assert_eq!(out, "\"Chicken Biryani, Half\",120,\r\n");

        let parsed = parse_csv(&out);
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed[0],
            vec![
                Some("Chicken Biryani, Half".to_owned()),
                Some("120".to_owned()),
                None
            ]
        );
    }

    #[test]
    fn null_and_the_empty_string_stay_different() {
        // The one thing a naive CSV writer always loses.
        let mut out = String::new();
        write_row(&mut out, [None, Some("")].into_iter());
        assert_eq!(out, ",\"\"\r\n");
        assert_eq!(parse_csv(&out)[0], vec![None, Some(String::new())]);
    }

    #[test]
    fn quotes_and_newlines_survive() {
        let nasty = "Chicken \"Biryani\", Half\nSpecial";
        let mut out = String::new();
        write_row(&mut out, [Some(nasty)].into_iter());
        let parsed = parse_csv(&out);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0], vec![Some(nasty.to_owned())]);
    }

    #[test]
    fn leading_and_trailing_spaces_survive() {
        // A shop really does have an item called "  Water" because somebody
        // typed it that way, and losing the space silently renames it.
        let mut out = String::new();
        write_row(&mut out, [Some("  Water ")].into_iter());
        assert_eq!(parse_csv(&out)[0], vec![Some("  Water ".to_owned())]);
    }
}
