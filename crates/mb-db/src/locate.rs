//! Where the shop's data file lives — and how to find it again.
//!
//! This module exists because of audit **A5**, which is not a hardware failure
//! and is the commonest way a shop believes it has lost everything:
//!
//! > *"The database folder path is stored in the browser's local storage. If
//! > that storage is cleared, or an external drive changes its letter, the app
//! > forgets where the shop's data is and shows the first-time setup screen —
//! > **with a live database sitting untouched on disk. Very frightening for an
//! > owner.**"*
//!
//! Two halves to the fix, and both are here:
//!
//! 1. The path lives in **an application config file on disk**, never in web
//!    local storage.
//! 2. If that file is missing, **search** the places it could be and report
//!    what was found, with enough detail for the caller to ask *"we found a
//!    Magic Bill database here, last used on Tuesday, 4,182 bills — use it?"*
//!    instead of showing a blank welcome screen.
//!
//! This module provides the functions. **P08 calls them at startup and P22
//! owns the screen** — said here so neither of them builds it twice.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::error::DbError;
use crate::migrate;

/// The folder the app keeps its own configuration in — not the shop's data.
///
/// Windows: `%APPDATA%\MagicBill`. Elsewhere: `$XDG_CONFIG_HOME/magicbill` or
/// `~/.config/magicbill`, which matters only because the tests and the
/// developers are not all on Windows.
#[must_use]
pub fn default_config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("MagicBill");
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("magicbill");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("magicbill");
    }
    PathBuf::from(".magicbill")
}

/// The file inside that folder which records where the shop's data is.
#[must_use]
pub fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join("database-location.txt")
}

/// Read the recorded location, if there is one.
///
/// A recorded path that no longer exists is reported as `Some` anyway — the
/// caller needs to know the difference between "we have never been set up" and
/// "the drive is not plugged in", and those are very different sentences to
/// show an owner.
pub fn read_config(config_dir: &Path) -> Result<Option<PathBuf>, DbError> {
    let path = config_path(config_dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(PathBuf::from(trimmed)))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(DbError::invariant(format!(
            "could not read {}: {e}",
            path.display()
        ))),
    }
}

pub fn write_config(config_dir: &Path, db_path: &Path) -> Result<(), DbError> {
    std::fs::create_dir_all(config_dir).map_err(|e| {
        DbError::invariant(format!(
            "could not create {}: {e}",
            config_dir.display()
        ))
    })?;
    let path = config_path(config_dir);
    std::fs::write(&path, db_path.to_string_lossy().as_bytes()).map_err(|e| {
        DbError::invariant(format!("could not write {}: {e}", path.display()))
    })
}

/// A database that was found by searching, with enough about it to describe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundDatabase {
    pub path: PathBuf,
    /// Milliseconds since the epoch, so the caller can say "last used on
    /// Tuesday".
    pub modified_ms: i64,
    pub schema_version: u32,
    /// Enough to tell a real shop from an empty test file.
    pub orders: i64,
    pub items: i64,
    pub customers: i64,
}

/// Look for a Magic Bill database in the places one could be.
///
/// **A stray file named `shop.db` is not a shop.** A candidate is reported only
/// if it opens read-only, its `schema_version` reads, and this build
/// understands that version — otherwise the caller would offer the owner
/// something that cannot be opened.
///
/// `extra` lets the caller add the path the config used to point at, which is
/// the single most likely answer when a drive letter has changed.
#[must_use]
pub fn search_usual_places(extra: &[PathBuf]) -> Vec<FoundDatabase> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    for path in extra {
        candidates.push(path.clone());
    }
    for dir in usual_directories() {
        candidates.push(dir.join("shop.db"));
        candidates.push(dir.join("magicbill.db"));
        candidates.push(dir.join("MagicBill").join("shop.db"));
    }

    let mut found = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for path in candidates {
        if !seen.insert(path.clone()) {
            continue;
        }
        if let Some(db) = inspect(&path) {
            found.push(db);
        }
    }
    // Most recently used first: that is almost always the one the owner wants.
    found.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    found
}

fn usual_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("MagicBill"));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let profile = PathBuf::from(profile);
        dirs.push(profile.join("Documents").join("MagicBill"));
        dirs.push(profile.join("MagicBill"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("MagicBill"));
    }
    // The root of every removable drive, because "the pen drive is in a
    // different port today" is a real Tuesday.
    for letter in 'D'..='Z' {
        let root = PathBuf::from(format!("{letter}:\\"));
        if root.exists() {
            dirs.push(root.join("MagicBill"));
        }
    }
    dirs
}

/// Open a candidate read-only and see whether it is really a shop.
///
/// Read-only on purpose: this runs on a file we have no reason to trust, at a
/// moment when the owner is already in trouble. Opening it for writing could
/// create an empty database at a typo'd path and make the situation worse.
#[must_use]
pub fn inspect(path: &Path) -> Option<FoundDatabase> {
    if !path.is_file() {
        return None;
    }
    let modified_ms = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0);

    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let version: i64 = conn
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| {
            r.get(0)
        })
        .ok()?;
    let version = u32::try_from(version).ok()?;
    if version == 0 || version > migrate::latest_version() {
        return None;
    }

    Some(FoundDatabase {
        path: path.to_path_buf(),
        modified_ms,
        schema_version: version,
        orders: count(&conn, "orders"),
        items: count(&conn, "items"),
        customers: count(&conn, "customers"),
    })
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap_or(0)
}
