//! One error type, and every variant is a sentence somebody at a counter can
//! act on (UI_GUIDELINES §6 — *"errors say what went wrong and what to do"*).

use crate::permission::Permission;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// The PIN rule, in the words the person typing it needs.
    #[error("a PIN is 6 to 8 digits — {what}")]
    BadPin { what: &'static str },

    /// BACKEND-G7's fix, as an error rather than a silent denial.
    #[error("\"{code}\" is not a permission this program has")]
    UnknownPermission { code: String },

    /// The refusal itself. Carries the permission so the screen can say which
    /// one, and so the audit row records what was attempted.
    #[error("you do not have permission to {what}")]
    Denied { what: &'static str, need: Permission },

    /// Somebody is logged in, but not as anybody the shop still employs.
    #[error("{name} is no longer active at this shop")]
    NotActive { name: String },

    /// Removing the last person who can manage staff would leave a shop that
    /// nobody can ever administer again.
    #[error("this is the last person who can manage staff — give somebody else that permission first")]
    LastAdministrator,

    /// A hash that will not parse: a truncated column, or a file somebody has
    /// been editing. It is refused rather than treated as "no PIN set", which
    /// would turn a corrupted row into an open door.
    #[error("this staff member's PIN could not be read")]
    BadHash,

    #[error("the PIN could not be secured")]
    HashFailed,

    /// Scope 1.12 — the discount ceiling, typed into a box.
    #[error("\"{typed}\" is not a percentage — try 10, or 12.5")]
    BadPercent { typed: String },
}
