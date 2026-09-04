//! The ordered migrations, and the engine that applies them exactly once.
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY);
//! SELECT COALESCE(MAX(version), 1) AS version FROM schema_version;
//! ```

use rusqlite::Connection;

use crate::error::DbError;

/// One ordered, forward-only change to the schema.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Ascending, contiguous, and never reused.
    pub version: u32,
    /// Shown in the ledger and in errors, so a refusal names something a human can find on
    /// disk.
    pub name: &'static str,
    pub sql: &'static str,
}

/// Every migration this build knows, in order.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "0001_initial",
        sql: include_str!("migrations/0001_initial.sql"),
    },
    // The first migration after the initial one, and it is worth saying why it is a second file
    // rather than an edit to the first.
    Migration {
        version: 2,
        name: "0002_recovery_slip",
        sql: include_str!("migrations/0002_recovery_slip.sql"),
    },
    // A kitchen ticket gets a running number of its own, so a cook can say "KOT 14".
    Migration {
        version: 3,
        name: "0003_kot_numbers",
        sql: include_str!("migrations/0003_kot_numbers.sql"),
    },
    // The tax rework.
    Migration {
        version: 4,
        name: "0004_tax_rework",
        sql: include_str!("migrations/0004_tax_rework.sql"),
    },
    // A table belongs to a dine-in order only.
    Migration {
        version: 5,
        name: "0005_placement",
        sql: include_str!("migrations/0005_placement.sql"),
    },
    // Old kitchen tickets for finished orders are closed.
    Migration {
        version: 6,
        name: "0006_kitchen_closed",
        sql: include_str!("migrations/0006_kitchen_closed.sql"),
    },
    Migration {
        version: 7,
        name: "0007_cloud_notices",
        sql: include_str!("migrations/0007_cloud_notices.sql"),
    },
    // The tax book: slabs are the one place tax lives; items stop carrying a copy.
    Migration {
        version: 8,
        name: "0008_tax_book",
        sql: include_str!("migrations/0008_tax_book.sql"),
    },
    // One scan: the staff code and the phone-login switch leave; Allow at the counter is the gate.
    Migration {
        version: 9,
        name: "0009_one_scan",
        sql: include_str!("migrations/0009_one_scan.sql"),
    },
    // One phone, one seat: the install id on a device row.
    Migration {
        version: 10,
        name: "0010_one_seat",
        sql: include_str!("migrations/0010_one_seat.sql"),
    },
    // A business day is a thing: the lock, the kind (trading or holiday) and the frozen totals
    // live on the day, not on a drawer count.
    Migration {
        version: 11,
        name: "0011_business_days",
        sql: include_str!("migrations/0011_business_days.sql"),
    },
    // A credit collection and a rider's handback name the till whose drawer they landed in,
    // like every other money row.
    Migration {
        version: 12,
        name: "0012_money_has_a_till",
        sql: include_str!("migrations/0012_money_has_a_till.sql"),
    },
];

/// The highest version this build understands.
#[must_use]
pub fn latest_version() -> u32 {
    MIGRATIONS.last().map_or(0, |m| m.version)
}

/// What `apply_all` did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// Versions applied by this call, in order.
    pub ran: Vec<u32>,
    /// Versions that were already in the ledger and were left alone.
    pub already: Vec<u32>,
}

/// A 64-bit FNV-1a of the migration text, as lowercase hex.
#[must_use]
pub fn checksum(sql: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET;
    for byte in sql.bytes().filter(|b| *b != b'\r') {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}

/// The ledger table. Created outside the migration list because the engine needs it before it
/// can decide what to run.
const LEDGER: &str = "
    CREATE TABLE IF NOT EXISTS schema_version (
        version    INTEGER NOT NULL PRIMARY KEY,
        name       TEXT    NOT NULL,
        checksum   TEXT    NOT NULL,
        applied_at INTEGER NOT NULL,
        run_ms     INTEGER NOT NULL
    ) STRICT;
";

/// Applies every migration this build knows that the file has not seen.
pub fn apply_all(conn: &mut Connection) -> Result<Applied, DbError> {
    conn.execute_batch(LEDGER)?;

    let known = read_ledger(conn)?;

    // Refuse a newer file BEFORE running anything.
    let latest = latest_version();
    if let Some(&(found, _, _)) = known.last()
        && found > latest
    {
        return Err(DbError::NewerSchema {
            found,
            known: latest,
        });
    }

    // Refuse edited history BEFORE running anything, for the same reason.
    for migration in MIGRATIONS {
        if let Some((_, name, applied)) = known.iter().find(|(v, _, _)| *v == migration.version) {
            let expected = checksum(migration.sql);
            if applied != &expected {
                return Err(DbError::MigrationChanged {
                    version: migration.version,
                    name: migration.name,
                    applied: applied.clone(),
                    expected,
                });
            }
            debug_assert_eq!(name, migration.name);
        }
    }

    let mut applied = Applied::default();
    for migration in MIGRATIONS {
        if known.iter().any(|(v, _, _)| *v == migration.version) {
            applied.already.push(migration.version);
            continue;
        }
        run_one_with_fks_off(conn, migration)?;
        applied.ran.push(migration.version);
    }
    Ok(applied)
}

/// `(version, name, checksum)` for every migration the file says has run.
fn read_ledger(conn: &Connection) -> Result<Vec<(u32, String, String)>, DbError> {
    let mut stmt =
        conn.prepare("SELECT version, name, checksum FROM schema_version ORDER BY version")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (version, name, sum) = row?;
        let version = u32::try_from(version).map_err(|_| DbError::OutOfRange {
            column: "schema_version.version",
            expected: "migration version",
        })?;
        out.push((version, name, sum));
    }
    Ok(out)
}

/// Run one migration with foreign keys suspended, then check them.
fn run_one_with_fks_off(conn: &mut Connection, migration: &Migration) -> Result<(), DbError> {
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let outcome = run_one(conn, migration);
    let checked = outcome.and_then(|()| {
        let broken: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);
        if broken == 0 {
            Ok(())
        } else {
            Err(DbError::invariant(format!(
                "migration {} left {broken} broken foreign key row(s)",
                migration.name
            )))
        }
    });
    conn.pragma_update(None, "foreign_keys", "ON")?;
    checked
}

fn run_one(conn: &mut Connection, migration: &Migration) -> Result<(), DbError> {
    let started = std::time::Instant::now();
    let tx = conn.transaction()?;

    tx.execute_batch(migration.sql)
        .map_err(|source| DbError::Migration {
            version: migration.version,
            name: migration.name,
            source,
        })?;

    // Elapsed is recorded because a migration that takes four minutes on a 5400 rpm HDD is
    // something the next release needs to know before it ships another one.
    let run_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);

    tx.execute(
        "INSERT INTO schema_version (version, name, checksum, applied_at, run_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            i64::from(migration.version),
            migration.name,
            checksum(migration.sql),
            now_millis(),
            run_ms,
        ],
    )
    .map_err(|source| DbError::Migration {
        version: migration.version,
        name: migration.name,
        source,
    })?;

    tx.commit().map_err(|source| DbError::Migration {
        version: migration.version,
        name: migration.name,
        source,
    })?;
    Ok(())
}

/// Wall-clock milliseconds, UTC.
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_contiguous_and_ascending() {
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            let expected = u32::try_from(index + 1).expect("small");
            assert_eq!(
                migration.version, expected,
                "migration versions must start at 1 and have no gaps"
            );
        }
    }

    #[test]
    fn the_checksum_ignores_line_endings() {
        // Same commit, two machines, two git autocrlf settings.
        assert_eq!(
            checksum("CREATE TABLE a(b);\n"),
            checksum("CREATE TABLE a(b);\r\n")
        );
    }

    #[test]
    fn the_checksum_notices_a_one_character_edit() {
        assert_ne!(
            checksum("CREATE TABLE a(b);"),
            checksum("CREATE TABLE a(c);")
        );
    }

    /// The schema uses STRICT tables, `RETURNING` and DDL inside a transaction.
    #[test]
    fn the_bundled_sqlite_is_new_enough_for_this_schema() {
        // STRICT arrived in 3.37, RETURNING in 3.35.
        assert!(
            rusqlite::version_number() >= 3_037_000,
            "bundled SQLite is {}, which is too old for STRICT tables",
            rusqlite::version()
        );
    }
}
