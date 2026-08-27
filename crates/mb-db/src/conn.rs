//! Opening the shop's data file, and the rules that keep a report from ever standing in front
//! of a cashier.

use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard};

use rusqlite::{Connection, Transaction};

use crate::error::DbError;
use crate::migrate;

/// How hard a commit works to survive a power cut.
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
    /// This comes from the caller and this crate never guesses it.
    pub path: PathBuf,
    /// How long a contended write waits before giving up.
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
#[derive(Debug)]
pub struct Db {
    path: PathBuf,
    writer: Mutex<Connection>,
    readers: ReaderPool,
}

impl Db {
    /// Opens or creates the file, sets the pragmas, and brings the schema up to date.
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
        // Persistent, set once, and it is the whole of scope 16.6: a report reading for two
        // seconds must not stop a cashier settling a bill.
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
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T, DbError>) -> Result<T, DbError> {
        let lease = self.readers.acquire();
        f(lease.conn())
    }

    /// Runs `f` inside a read-only transaction on a pooled reader.
    pub fn read_transaction<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        let lease = self.readers.acquire();
        let tx = lease.conn().unchecked_transaction()?;
        match f(&tx) {
            // Explicit, so a rollback failure surfaces instead of being eaten by a destructor.
            Ok(value) => {
                tx.rollback()?;
                Ok(value)
            }
            // Dropping the transaction rolls it back; returning the caller's error unchanged
            // matters more than reporting the rollback.
            Err(e) => Err(e),
        }
    }

    /// Runs `f` inside a transaction on the single writer connection.
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
                // Dropping the transaction rolls it back; being explicit means a rollback
                // failure is not swallowed by a destructor.
                tx.rollback()?;
                Err(e)
            }
        }
    }

    /// Writes the WAL back into the main file.
    pub fn checkpoint(&self) -> Result<(), DbError> {
        let writer = lock(&self.writer);
        writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// Write a consistent copy of the whole database to `to`, while the shop keeps billing.
    pub fn backup_to(&self, to: &Path) -> Result<(), DbError> {
        // VACUUM INTO refuses to overwrite.
        if to.exists() {
            return Err(DbError::invariant(format!(
                "there is already a file at {} — a backup never overwrites one",
                to.display()
            )));
        }
        self.read(|conn| {
            conn.execute("VACUUM INTO ?1", [to.to_string_lossy().as_ref()])?;
            Ok(())
        })
    }
}

/// Opens one connection and applies the pragmas that are per-connection.
fn open_one(path: &Path, config: &DbConfig) -> Result<Connection, DbError> {
    let conn = Connection::open(path).map_err(|source| DbError::Open {
        path: path.to_path_buf(),
        source,
    })?;

    let setup = || -> Result<(), rusqlite::Error> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_millis(u64::from(
            config.busy_timeout_ms,
        )))?;
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

// The reader pool.

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

/// A borrowed read connection.
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
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// A note on encryption, which is a SEAM and not an implementation.
