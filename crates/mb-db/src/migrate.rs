//! The ordered migrations, and the engine that applies them exactly once.
//!
//! # Why there is only one migration
//!
//! D11 — there are no existing customers. Every licence, restaurant and bill in
//! v1 is test data the owner made himself, so there is nothing to preserve and
//! no parallel run. Migration 0001 is therefore the WHOLE schema, in one file.
//!
//! The alternative is what the backend audit found (BACKEND-G6): *"Migration
//! 0011 was completely replaced by 0012 a few weeks later, and 0012's original
//! rules were then rewritten by 0014/0015/0016 and again by 0021. The current
//! truth is spread across six files. Anybody reading the folder from the top
//! gets the wrong answer."*
//!
//! # Why the ledger carries a checksum
//!
//! v1's whole migration ledger was:
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY);
//! SELECT COALESCE(MAX(version), 1) AS version FROM schema_version;
//! ```
//!
//! No checksum, no name, no time — and `MAX(version)`, so a migration skipped
//! in the middle was invisible forever. Here every applied migration gets its
//! own row, and editing a shipped migration after it has run is refused rather
//! than silently letting the database drift away from the code that reads it.

use rusqlite::Connection;

use crate::error::DbError;

/// One ordered, forward-only change to the schema.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Ascending, contiguous, and never reused.
    pub version: u32,
    /// Shown in the ledger and in errors, so a refusal names something a human
    /// can find on disk.
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
    // **The first migration after the initial one**, and it is worth saying why
    // it is a second file rather than an edit to the first. 0001 has run on the
    // owner's laptop; `apply_all` checksums a shipped migration and refuses a
    // file that has changed since it ran, precisely so a real shop's disk
    // cannot drift away from the code that reads it. So the CHECK is widened
    // forwards, never in place.
    Migration {
        version: 2,
        name: "0002_recovery_slip",
        sql: include_str!("migrations/0002_recovery_slip.sql"),
    },
];

/// The highest version this build understands. A file above it is refused —
/// see [`DbError::NewerSchema`].
#[must_use]
pub fn latest_version() -> u32 {
    MIGRATIONS.last().map_or(0, |m| m.version)
}

/// What [`apply_all`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// Versions applied by this call, in order.
    pub ran: Vec<u32>,
    /// Versions that were already in the ledger and were left alone.
    pub already: Vec<u32>,
}

/// A 64-bit FNV-1a of the migration text, as lowercase hex.
///
/// **Why not SHA-256, and therefore why no `sha2` dependency (R6).** This is a
/// tripwire against a shipped migration being edited, not a security control.
/// An attacker who can edit `0001_initial.sql` is editing the source of the
/// program; they can edit the expected checksum in the same commit, so a
/// cryptographic hash buys nothing at all here. What it has to catch is a
/// developer changing a migration that has already run on a real shop's disk,
/// and sixteen lines of FNV catches that perfectly.
///
/// **Line endings are normalised first**, because git may check the .sql file
/// out with CRLF on one machine and LF on another. Without this, the same
/// commit would produce two different checksums and every second developer
/// would see a tampering error on a file nobody touched.
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

/// The ledger table. Created outside the migration list because the engine
/// needs it before it can decide what to run.
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
///
/// * Each migration runs **inside its own transaction**, together with the
///   ledger row that records it. SQLite does DDL inside a transaction, so a
///   migration that fails on its third statement leaves the database exactly
///   where it was — no half-created schema, no ledger row.
/// * An already-applied migration whose text has changed is refused before
///   anything is written.
/// * A file whose highest version is above [`latest_version`] is refused, so an
///   old build cannot write rows a newer schema will not understand.
pub fn apply_all(conn: &mut Connection) -> Result<Applied, DbError> {
    conn.execute_batch(LEDGER)?;

    let known = read_ledger(conn)?;

    // Refuse a newer file BEFORE running anything. An old build that has
    // already half-migrated a new database is a worse position than one that
    // never opened it.
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
        run_one(conn, migration)?;
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

fn run_one(conn: &mut Connection, migration: &Migration) -> Result<(), DbError> {
    let started = std::time::Instant::now();
    let tx = conn.transaction()?;

    tx.execute_batch(migration.sql)
        .map_err(|source| DbError::Migration {
            version: migration.version,
            name: migration.name,
            source,
        })?;

    // Elapsed is recorded because a migration that takes four minutes on a
    // 5400 rpm HDD is something the next release needs to know before it ships
    // another one.
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
///
/// The ledger is the one place in this crate that reads the clock itself:
/// everything else takes a [`mb_core::Timestamp`] from its caller, because D5
/// says a business day is stamped once by the code that creates the order, not
/// re-derived by whoever happens to be writing a row.
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
        // Same commit, two machines, two git autocrlf settings. Without this,
        // one of them sees a tampering error on a file nobody touched.
        assert_eq!(checksum("CREATE TABLE a(b);\n"), checksum("CREATE TABLE a(b);\r\n"));
    }

    #[test]
    fn the_checksum_notices_a_one_character_edit() {
        assert_ne!(checksum("CREATE TABLE a(b);"), checksum("CREATE TABLE a(c);"));
    }

    /// The schema uses STRICT tables, `RETURNING` and DDL inside a transaction.
    /// All three are the reason `rusqlite` is pulled in with `bundled` — a
    /// system SQLite old enough to lack them would fail at migration time on a
    /// customer's machine rather than here.
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
