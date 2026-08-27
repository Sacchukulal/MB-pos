//! Who is at the counter, and what they are allowed to do.
//!
//! ```text
//!   mb-core  ->  mb-auth  ->  mb-db  ->  magic-bill
//! ```

pub mod actor;
pub mod audit;
pub mod device;
pub mod error;
pub mod lockout;
pub mod permission;
pub mod pin;
pub mod recovery;
pub mod role;

pub use actor::Actor;
pub use audit::{AuditAction, AuditEntry, AuditRow, Broken, chain_hash, sha256, verify_chain};
pub use device::{DeviceSecret, new_device_secret, random_token, short_code, verify_device_secret};
pub use error::AuthError;
pub use lockout::{LOCKOUT_FREE_ATTEMPTS, lockout_after};
pub use permission::{Permission, PermissionSet};
pub use pin::{PIN_DIGITS, Pin, PinHash, hash_pin, hash_secret, verify_pin, verify_secret};
pub use recovery::{RecoveryCode, new_recovery_code, verify_recovery_code};
pub use role::{RolePreset, RoleShape};
