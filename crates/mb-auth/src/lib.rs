//! **Who is at the counter, and what they are allowed to do.**
//!
//! # The finding this crate exists to close
//!
//! > Audit **C1**: *"There is no login on the POS at all. Anybody who walks
//! > behind the counter can open Reports and see the whole day's cash, change
//! > the bill number, delete menu items, delete khata customers, or deactivate
//! > the licence. For a money machine this is the single biggest control gap."*
//!
//! Every other crate so far makes the counter faster or safer at something it
//! already did. This one is the first that can tell a person **no** — so it is
//! also the first where being wrong stops a real shop trading.
//!
//! # Why it is its own crate
//!
//! Two dependencies live here and nowhere else: `argon2` and `sha2`. That is
//! the same argument D34 made for `mb-winprint` — *one crate owns the risky
//! edge* — and it keeps the pure billing rules in `mb-core` free of a crypto
//! stack that has nothing to do with money.
//!
//! It sits between `mb-core` and `mb-db`:
//!
//! ```text
//!   mb-core  ->  mb-auth  ->  mb-db  ->  magic-bill
//! ```
//!
//! `mb-db` depends on it so that a permission code read from a row becomes a
//! [`Permission`] **at the row**, and an unknown one is an error there rather
//! than a silent "denied" three layers up. That silent denial is BACKEND-G7,
//! and it is the reason a typo in v1 was undebuggable from the counter.
//!
//! # What a PIN defends against, stated plainly
//!
//! The person behind the counter, and the person who wanders behind it. It does
//! **not** defend against somebody who takes the computer or copies the
//! database file — nothing on an offline counter can, because the shop's own
//! machine has to be able to read the shop's own data. Argon2 makes that copy
//! expensive to attack; it does not make it impossible. A security claim
//! nobody has written down is a security claim nobody can check, so it is
//! written down here and in `docs/DECISIONS.md`.

pub mod actor;
pub mod audit;
pub mod error;
pub mod lockout;
pub mod permission;
pub mod pin;
pub mod recovery;
pub mod role;

pub use actor::Actor;
pub use audit::{AuditAction, AuditEntry, AuditRow, Broken, chain_hash, verify_chain};
pub use error::AuthError;
pub use lockout::{LOCKOUT_FREE_ATTEMPTS, lockout_after};
pub use permission::{Permission, PermissionSet};
pub use pin::{Pin, PinHash, hash_pin, verify_pin};
pub use recovery::{RecoveryCode, new_recovery_code, verify_recovery_code};
pub use role::{RolePreset, RoleShape};
