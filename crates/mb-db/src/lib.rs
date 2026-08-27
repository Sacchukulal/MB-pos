//! Magic Bill — storage.

#![deny(missing_debug_implementations)]

pub mod backup;
pub mod conn;
pub mod encode;
pub mod error;
pub mod export;
pub mod locate;
pub mod migrate;
pub mod numbering;
pub mod repo;
pub mod schema;
pub mod settle;

pub use backup::{Backup, RestoreReport, VerifyReport};
pub use conn::{Db, DbConfig, Synchronous};
pub use error::DbError;
pub use migrate::{Applied, MIGRATIONS, Migration, checksum};
pub use numbering::CounterKind;
pub use repo::Repos;
pub use settle::{Till, open_draft, settle};
