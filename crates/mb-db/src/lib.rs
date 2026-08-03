//! Magic Bill — storage.
//!
//! One SQLite file holds the whole shop. This crate owns the schema, the
//! migration engine that puts it there, and the connection rules that keep a
//! report from ever standing in front of a cashier.
//!
//! It owns no queries. `find_order`, `list_open_orders`, `save_settled_order`
//! and every other row-shaped function are P05. The line is drawn in
//! [`encode`]: **P04 owns the VALUE mapping, P05 owns the ROW mapping.**
//!
//! The decisions that shape every line of it:
//!
//! * **D2 — money is an integer count of paise.** There is no REAL column in
//!   this database and a test walks the whole schema proving it. v1 declared
//!   nine, and every rupee the product ever touched went through one.
//! * **D5 — the business day is stored, not derived.** Every row that a report
//!   groups by a day carries `business_day` as an INTEGER count of days.
//! * **D6 — a number is claimed by ONE statement** that increments and
//!   returns. There is no `SELECT` followed by an `UPDATE` in this crate, and
//!   the daily reset is evaluated inside that same statement.
//! * **D11 — there is nothing to migrate from.** Migration 0001 is the whole
//!   schema, in one file, because v1's truth ended up spread across six.
//! * **D13 — ids are text.** Two terminals in one shop collide on integers and
//!   there is no repair.
//! * **D16 — the cloud is on the free plan.** [`sync_outbox`] stores no payload
//!   for an upsert, so a row edited five times between connections syncs once.
//!
//! [`sync_outbox`]: crate#the-outbox
//!
//! # The layers
//!
//! | module | owns |
//! |---|---|
//! | [`error`] | one error type, and what each variant means to a shopkeeper |
//! | [`encode`] | mb-core values <-> SQLite values. The contract every later session reads |
//! | [`migrate`] | the ordered migrations, their checksums, and the refusal to run on edited history |
//! | [`conn`] | the pragmas, the single writer, the reader pool |
//! | [`numbering`] | the one statement that claims a token or a bill number (D6) |
//! | [`schema`] | introspection — what the tests assert against |
//!
//! # Where the database file lives
//!
//! [`conn::DbConfig::path`] comes from the caller and this crate never guesses
//! it. It must be read from an application config file on disk — **never from
//! web local storage.** v1 kept it there; clearing the browser storage, or an
//! external drive changing its letter, showed the owner a first-run wizard with
//! their live shop sitting three folders away (audit A5). P08 owns finding the
//! path and P22 owns the "we found a database here — use it?" recovery.

#![deny(missing_debug_implementations)]

pub mod conn;
pub mod encode;
pub mod error;
pub mod migrate;
pub mod numbering;
pub mod schema;

pub use conn::{Db, DbConfig, Synchronous};
pub use error::DbError;
pub use migrate::{Applied, MIGRATIONS, Migration, checksum};
pub use numbering::CounterKind;
