//! Opening the shop's data file, and the rules that keep a report from ever
//! standing in front of a cashier.
//!
//! One writer, several readers. SQLite allows exactly one writer at a time, and
//! scope 16.6 says reports must never block billing — so the billing path owns
//! the writer and everything that only reads takes a connection from the pool.
//! Audit E1 is what happens without it: "a heavy report on a slow PC can make
//! the search box stutter mid-rush."

use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard};

use rusqlite::{Connection, Transaction};

use crate::error::DbError;
use crate::migrate;

/// How hard a commit works to survive a power cut.
///
/// **The default is [`Synchronous::Full`], and that overrules
/// `docs/PERFORMANCE.md` §5 rule 2 as written.** In WAL mode `NORMAL` does not
/// fsync on commit, so a power cut loses the last committed transactions — the
/// file survives, the last bills do not. Requirement 1 of the ten is that a
/// failure loses NOTHING, and an Indian restaurant counter loses mains power as
/// a matter of routine rather than as an accident.
///
/// The cost is one fsync per commit, and §5 rule 1 already says a settle is one
/// transaction, so it is one fsync and not four. `tests/perf.rs` measures both
/// settings and prints them; the numbers are in `docs/PERFORMANCE.md` §4.
///
/// `Normal` stays available on purpose: an import, a restore or a bulk rebuild
/// is not a bill, and lowering it deliberately for that work is correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Synchronous {
    #[default]
    Full,
    Normal,
}

impl Synchronous {
    const fn pragma(self) -> &'static str {
        match self {
            Synchronous::Full => "FULL",
            Synchronous::Normal => "NORMAL",
        }
    }
}

/// Where the database is and how it should behave.
#[derive(Debug, Clone)]
pub struct DbConfig {
    /// **This comes from the caller and this crate never guesses it.**
    ///
    /// It must be read from an application config file on disk, never from web
    /// local storage. Audit A5: v1 kept it in the browser's storage, so
    /// clearing that storage — or an external drive changing its letter —
    /// showed the owner a first-run wizard with their live shop sitting three
    /// folders away. P08 owns finding the path; P22 owns the "we found a
    /// database here — use it?" recovery.
    pub path: PathBuf,
    /// How long a contended write waits before giving up. Without it SQLite
    /// returns SQLITE_BUSY at once and the caller sees a failure that was only
    /// ever a wait.
    pub busy_timeout_ms: u32,
    /// How many read connections to pre-open.
    pub readers: usize,
    pub synchronous: Synchronous,
}

impl DbConfig {
    /// The shipped defaults.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        DbConfig {
            path: path.into(),
            busy_timeout_ms: 5_000,
            readers: 4,
            synchronous: Synchronous::Full,
        }
    }
}

/// The shop's data file: one writer, a small pool of readers.
///
/// [`Db::transaction`] is the **only** way to write. Not the recommended way —
/// there is no other public route to the writer connection at all. A write that
/// escapes a transaction is a write that can half-happen, and half a settled
/// bill is the worst row in the product.
#[derive(Debug)]
pub struct Db {
    path: PathBuf,
    writer: Mutex<Connection>,
    readers: ReaderPool,
}

impl Db {
    /// Opens or creates the file, sets the pragmas, and brings the schema up to
    /// date.
    ///
    /// Migrating on open is deliberate: there is no state in which the program
    /// holds a connection to a database whose schema it has not checked.
    pub fn open(config: &DbConfig) -> Result<Db, DbError> {
        if let Some(parent) = config.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                DbError::invariant(format!(
                    "could not create the folder for the shop's data file at {}: {e}",
                    parent.display()
                ))
            })?;
        }

        let mut writer = open_one(&config.path, config)?;
        // Persistent, set once, and it is the whole of scope 16.6: a report
        // reading for two seconds must not stop a cashier settling a bill.
        writer
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(|source| DbError::Open {
                path: config.path.clone(),
                source,
            })?;
        // Only the writer commits, so this is one connection's setting.
        writer
            .pragma_update(None, "synchronous", config.synchronous.pragma())
            .map_err(|source| DbError::Open {
                path: config.path.clone(),
                source,
            })?;

        migrate::apply_all(&mut writer)?;

        let mut readers = Vec::with_capacity(config.readers);
        for _ in 0..config.readers {
            readers.push(open_one(&config.path, config)?);
        }

        Ok(Db {
            path: config.path.clone(),
            writer: Mutex::new(writer),
            readers: ReaderPool::new(readers),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `f` on a read connection from the pool.
    ///
    /// Blocks only if every reader is busy — never on the writer, which is the
    /// point of the pool existing.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T, DbError>) -> Result<T, DbError> {
        let lease = self.readers.acquire();
        f(lease.conn())
    }

    /// Runs `f` inside a transaction on the single writer connection.
    ///
    /// Commits if `f` returns `Ok`, rolls back otherwise. A settle is ONE
    /// transaction — the number claim, the order rows, the payment rows and the
    /// kitchen ledger together — which is `PERFORMANCE.md` §5 rule 1 and also
    /// what makes D6's atomic numbering true.
    pub fn transaction<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        let mut writer = lock(&self.writer);
        let tx = writer.transaction()?;
        match f(&tx) {
            Ok(value) => {
                tx.commit()?;
                Ok(value)
            }
            Err(e) => {
                // Dropping the transaction rolls it back; being explicit means
                // a rollback failure is not swallowed by a destructor.
                tx.rollback()?;
                Err(e)
            }
        }
    }

    /// Writes the WAL back into the main file.
    ///
    /// Used by the size measurement (budget M5) and, later, by P05's backup —
    /// copying the main file while a WAL is outstanding copies a database that
    /// is missing its most recent bills.
    pub fn checkpoint(&self) -> Result<(), DbError> {
        let writer = lock(&self.writer);
        writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }
}

/// Opens one connection and applies the pragmas that are **per-connection**.
///
/// `foreign_keys` is the one that matters and the one v1 never set. SQLite
/// defaults it OFF, it is not stored in the file, and it must be turned on
/// again for every single connection — which is why the test for it uses a
/// connection taken from the reader pool and not just the writer.
fn open_one(path: &Path, config: &DbConfig) -> Result<Connection, DbError> {
    let conn = Connection::open(path).map_err(|source| DbError::Open {
        path: path.to_path_buf(),
        source,
    })?;

    let setup = || -> Result<(), rusqlite::Error> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_millis(u64::from(config.busy_timeout_ms)))?;
        // A report's sort should not touch a 5400 rpm disk.
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        Ok(())
    };
    setup().map_err(|source| DbError::Open {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(conn)
}

// ---------------------------------------------------------------------------
// The reader pool.
//
// NO POOL CRATE (R6). r2d2 and deadpool manage lifecycles, health checks, idle
// reaping and async — none of which applies to four connections to one local
// file that live exactly as long as the process. What is needed is "hand out
// one of four connections and put it back", and that is what this is.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ReaderPool {
    free: Mutex<Vec<Connection>>,
    available: Condvar,
}

impl ReaderPool {
    fn new(connections: Vec<Connection>) -> Self {
        ReaderPool {
            free: Mutex::new(connections),
            available: Condvar::new(),
        }
    }

    fn acquire(&self) -> Lease<'_> {
        let mut free = lock(&self.free);
        loop {
            if let Some(conn) = free.pop() {
                return Lease {
                    pool: self,
                    conn: Some(conn),
                };
            }
            free = self
                .available
                .wait(free)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn release(&self, conn: Connection) {
        lock(&self.free).push(conn);
        self.available.notify_one();
    }
}

/// A borrowed read connection. Returns itself to the pool on drop, including on
/// an early return or an unwind, so a pool cannot leak its way down to zero.
#[derive(Debug)]
struct Lease<'a> {
    pool: &'a ReaderPool,
    conn: Option<Connection>,
}

impl Lease<'_> {
    fn conn(&self) -> &Connection {
        // `conn` is Some for the whole life of the lease; only Drop takes it.
        self.conn.as_ref().unwrap_or_else(|| unreachable!())
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.release(conn);
        }
    }
}

/// Takes a lock, recovering from poisoning rather than propagating it.
///
/// A poisoned mutex here means some other thread panicked while holding a
/// connection. The connection itself is fine — an open transaction rolls back
/// when it is dropped — and refusing to bill for the rest of the shift because
/// a report thread fell over is the wrong trade at a counter.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// A note on encryption, which is a SEAM and not an implementation.
//
// Turning SQLCipher on would take: rusqlite's `bundled-sqlcipher` feature
// instead of `bundled`; a key supplied on DbConfig; a `PRAGMA key` issued in
// `open_one` BEFORE any other statement, including the pragmas above; and a
// decision about where the key lives. That last one is the actual hard part and
// it belongs with P21's device binding, not here. Nothing in this file assumes
// the file is unencrypted, so the change stays inside `open_one`.
// ---------------------------------------------------------------------------
