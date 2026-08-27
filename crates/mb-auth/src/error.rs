//! One error type, and every variant is a sentence somebody at a counter can act on.

use crate::permission::Permission;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// The PIN rule, in the words the person typing it needs.
    #[error("a PIN is {} digits — {what}", crate::pin::PIN_DIGITS)]
    BadPin { what: &'static str },

    /// BACKEND-G7's fix, as an error rather than a silent denial.
    #[error("\"{code}\" is not a permission this program has")]
    UnknownPermission { code: String },

    #[error("you do not have permission to {what}")]
    Denied {
        what: &'static str,
        need: Permission,
    },

    /// Somebody is logged in, but not as anybody the shop still employs.
    #[error("{name} is no longer active at this shop")]
    NotActive { name: String },

    /// Removing the last person who can manage staff would leave a shop that nobody can ever
    /// administer again.
    #[error(
        "this is the last person who can manage staff — give somebody else that permission first"
    )]
    LastAdministrator,

    /// A hash that will not parse: a truncated column, or a file somebody has been editing.
    #[error("this staff member's PIN could not be read")]
    BadHash,

    #[error("the PIN could not be secured")]
    HashFailed,

    /// The discount ceiling, typed into a box.
    #[error("\"{typed}\" is not a percentage — try 10, or 12.5")]
    BadPercent { typed: String },
}
