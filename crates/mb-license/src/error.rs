//! The one error type this crate hands upwards.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LicenceError {
    /// The cloud refused, or could not be reached.
    #[error("{0}")]
    Cloud(#[from] crate::cloud::CloudError),

    /// The snapshot did not verify.
    #[error("{0}")]
    Snapshot(#[from] crate::snapshot::VerifyError),

    /// The emergency code was wrong, used, expired, or for another machine.
    #[error("{0}")]
    Emergency(#[from] crate::emergency::EmergencyError),

    /// A call ran past its deadline.
    #[error("that took too long, so we stopped waiting")]
    Timedout,

    /// `licence.json` could not be read or written.
    #[error("the licence file could not be {doing}: {why}")]
    File { doing: &'static str, why: String },

    /// A transfer inside its cooldown.
    #[error("this licence was moved recently")]
    TooSoon { days_left: u16 },
}

impl From<crate::deadline::Timedout> for LicenceError {
    fn from(_: crate::deadline::Timedout) -> Self {
        LicenceError::Timedout
    }
}

/// The stable code that crosses the IPC boundary.
impl LicenceError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            LicenceError::Cloud(_) => "licence.cloud",
            LicenceError::Snapshot(_) => "licence.snapshot",
            LicenceError::Emergency(_) => "licence.emergency",
            LicenceError::Timedout => "licence.timedout",
            LicenceError::File { .. } => "licence.file",
            LicenceError::TooSoon { .. } => "licence.too_soon",
        }
    }
}
