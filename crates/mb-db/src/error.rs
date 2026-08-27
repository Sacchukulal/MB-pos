//! What can go wrong with the shop's disk, and what it means to the person standing at the
//! counter.

use std::path::PathBuf;

/// Anything this crate can refuse to do.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// The database file could not be opened or created.
    #[error("could not open the shop's data file at {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    /// A statement failed. The wrapped error carries SQLite's own words for anyone reading a
    /// log; the caller sees this sentence.
    #[error("the shop's data file rejected that: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// A migration failed. The database is left where it was — see `crate::migrate::apply_all`.
    #[error("could not apply migration {version} ({name}): {source}")]
    Migration {
        version: u32,
        name: &'static str,
        #[source]
        source: rusqlite::Error,
    },

    /// An already-applied migration's text has changed since it ran.
    #[error(
        "migration {version} ({name}) has been edited since it was applied to \
         this shop's data file — refusing to run. The file has not been touched."
    )]
    MigrationChanged {
        version: u32,
        name: &'static str,
        applied: String,
        expected: String,
    },

    /// The file was written by a newer build than this one.
    #[error(
        "this shop's data file was made by a newer version of Magic Bill \
         (schema {found}, this build knows {known}) — update before opening it"
    )]
    NewerSchema { found: u32, known: u32 },

    /// A stored value is not what its column promised.
    #[error("the value `{value}` stored in {column} is not one this program knows")]
    BadValue { column: &'static str, value: String },

    /// A number that does not fit the type it is being read into.
    #[error("the value stored in {column} is out of range for a {expected}")]
    OutOfRange {
        column: &'static str,
        expected: &'static str,
    },

    /// A row that must exist does not, or one that must be unique is not.
    #[error("{0}")]
    Invariant(String),
}

impl DbError {
    /// Shorthand for the invariant case, so call sites stay readable.
    pub fn invariant(message: impl Into<String>) -> Self {
        DbError::Invariant(message.into())
    }
}
