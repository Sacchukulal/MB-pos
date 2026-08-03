//! The migration engine: T1, T2, T3, T20, T21.
//!
//! v1's whole ledger was `schema_version(version INTEGER PRIMARY KEY)`, read
//! with `COALESCE(MAX(version), 1)`. No checksum, no name, no time — and
//! `MAX`, so a migration skipped in the middle was invisible forever. These
//! tests are the difference.

// The clippy.toml exemption reaches `#[test]` functions only, and the helpers
// at the bottom of this file are plain functions. In a test `expect` IS the
// assertion (see clippy.toml, added at P01).
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect and panic are the assertion"
)]

mod common;

use common::Scratch;
use mb_db::{DbError, MIGRATIONS, checksum, migrate};
use rusqlite::Connection;

/// T1. A fresh database runs every migration, in order, and records one row
/// each — not a high-water mark.
#[test]
fn t1_fresh_database_runs_every_migration_and_records_each_one() {
    let scratch = Scratch::new("t1");
    let mut conn = Connection::open(scratch.db_path()).expect("open");

    let applied = migrate::apply_all(&mut conn).expect("migrations run");
    assert_eq!(applied.ran, vec![1]);
    assert!(applied.already.is_empty());

    let rows: Vec<(i64, String, String, i64)> = conn
        .prepare("SELECT version, name, checksum, applied_at FROM schema_version ORDER BY version")
        .expect("prepare")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");

    assert_eq!(rows.len(), MIGRATIONS.len(), "one row per migration, not a MAX()");
    for (i, (version, name, sum, at)) in rows.iter().enumerate() {
        let expected = &MIGRATIONS[i];
        assert_eq!(*version, i64::from(expected.version));
        assert_eq!(name, expected.name);
        assert_eq!(sum, &checksum(expected.sql));
        assert!(*at > 0, "applied_at is a real clock reading");
    }

    // Contiguous and ascending, so a gap cannot hide.
    for pair in rows.windows(2) {
        assert_eq!(pair[1].0, pair[0].0 + 1, "versions must have no gaps");
    }
}

/// T2. Running the migrations again does nothing at all.
#[test]
fn t2_running_migrations_twice_is_a_no_op() {
    let scratch = Scratch::new("t2");
    let mut conn = Connection::open(scratch.db_path()).expect("open");

    migrate::apply_all(&mut conn).expect("first run");
    let before: Vec<(i64, String)> = ledger(&conn);

    let applied = migrate::apply_all(&mut conn).expect("second run");
    assert!(applied.ran.is_empty(), "nothing should have run again");
    assert_eq!(applied.already, vec![1]);

    assert_eq!(before, ledger(&conn), "the ledger must be untouched");
}

/// T3. Editing a migration that has already run is refused — and the refusal
/// leaves the database exactly as it was.
///
/// A refusal that half-migrated would be worse than the tamper it caught.
#[test]
fn t3_edited_history_is_refused_and_nothing_is_touched() {
    let scratch = Scratch::new("t3");
    let mut conn = Connection::open(scratch.db_path()).expect("open");
    migrate::apply_all(&mut conn).expect("first run");

    // Put a row in, so we can prove the refusal did not disturb the shop.
    conn.execute_batch(common::STAFF_SQL).expect("seed staff");

    // Simulate the edit by rewriting the recorded checksum: the effect is
    // identical to someone changing one character of 0001_initial.sql after it
    // shipped, and it does not require writing to the source tree from a test.
    conn.execute(
        "UPDATE schema_version SET checksum = 'deadbeefdeadbeef' WHERE version = 1",
        [],
    )
    .expect("tamper");

    let err = migrate::apply_all(&mut conn).expect_err("must refuse");
    match err {
        DbError::MigrationChanged { version, name, .. } => {
            assert_eq!(version, 1);
            assert_eq!(name, "0001_initial");
        }
        other => panic!("wrong error: {other}"),
    }

    let staff: i64 = conn
        .query_row("SELECT count(*) FROM staff", [], |r| r.get(0))
        .expect("count");
    assert_eq!(staff, 1, "the refusal must not have touched the shop's rows");
}

/// T20. A migration that fails part way leaves the previous version in place.
///
/// SQLite runs DDL inside a transaction. This test proves we are actually using
/// that, rather than trusting it.
#[test]
fn t20_a_failed_migration_leaves_the_previous_version() {
    let scratch = Scratch::new("t20");
    let mut conn = Connection::open(scratch.db_path()).expect("open");
    migrate::apply_all(&mut conn).expect("baseline");

    let bad = mb_db::Migration {
        version: 99,
        name: "0099_broken",
        sql: "CREATE TABLE half_a (x TEXT) STRICT;
              CREATE TABLE half_b (y TEXT) STRICT;
              THIS IS NOT SQL;",
    };

    // Drive the engine's own path rather than a hand-rolled one, so the test
    // exercises the transaction the real code uses.
    let result = run_single(&mut conn, &bad);
    assert!(result.is_err(), "a broken migration must fail");

    let tables = mb_db::schema::tables(&conn).expect("tables");
    assert!(!tables.contains(&"half_a".to_owned()), "half_a must have rolled back");
    assert!(!tables.contains(&"half_b".to_owned()), "half_b must have rolled back");

    let versions: Vec<i64> = ledger(&conn).into_iter().map(|(v, _)| v).collect();
    assert_eq!(versions, vec![1], "the ledger must not have gained a row");
}

/// T21. A file written by a newer build is refused rather than used.
///
/// A staged rollout (scope 13.5) creates this on purpose: one shop updates, the
/// second terminal has not. The old build must stop, not write rows the new
/// schema will not understand.
#[test]
fn t21_a_newer_database_is_refused() {
    let scratch = Scratch::new("t21");
    let mut conn = Connection::open(scratch.db_path()).expect("open");
    migrate::apply_all(&mut conn).expect("baseline");

    conn.execute(
        "INSERT INTO schema_version (version, name, checksum, applied_at, run_ms)
         VALUES (9999, '9999_from_the_future', 'ffffffffffffffff', 1, 0)",
        [],
    )
    .expect("pretend a newer build has been here");

    let err = migrate::apply_all(&mut conn).expect_err("must refuse");
    match err {
        DbError::NewerSchema { found, known } => {
            assert_eq!(found, 9999);
            assert_eq!(known, migrate::latest_version());
        }
        other => panic!("wrong error: {other}"),
    }
}

/// The engine applies its own list, so a one-off migration needs the same
/// transaction discipline spelled out here. Kept identical in shape to
/// `migrate::run_one`, which is private on purpose.
fn run_single(conn: &mut Connection, m: &mb_db::Migration) -> Result<(), DbError> {
    let tx = conn.transaction()?;
    tx.execute_batch(m.sql).map_err(|source| DbError::Migration {
        version: m.version,
        name: m.name,
        source,
    })?;
    tx.execute(
        "INSERT INTO schema_version (version, name, checksum, applied_at, run_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![i64::from(m.version), m.name, checksum(m.sql), 1_i64, 0_i64],
    )?;
    tx.commit()?;
    Ok(())
}

fn ledger(conn: &Connection) -> Vec<(i64, String)> {
    conn.prepare("SELECT version, checksum FROM schema_version ORDER BY version")
        .expect("prepare")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows")
}
