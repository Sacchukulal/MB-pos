//! May this shop operate right now, and what may it use?

/// A source file with its test module cut off.
#[cfg(test)]
#[must_use]
pub(crate) fn shipped_part_of(source: &str) -> &str {
    match source.find("\n#[cfg(test)]") {
        Some(at) => &source[..at],
        None => source,
    }
}

pub mod clock;
pub mod cloud;
pub mod deadline;
pub mod emergency;
pub mod entitlement;
pub mod error;
pub mod gate;
pub mod machine;
pub mod plan;
pub mod snapshot;
pub mod state;
pub mod status;

pub use clock::{ClockSays, Watch};
pub use cloud::{Cloud, CloudError};
pub use deadline::{Timedout, within};
pub use emergency::{Code, EmergencyError, mint, redeem};
pub use entitlement::{DEFAULT_GRACE_DAYS, Entitlement, billing_is_always_allowed, decide};
pub use error::LicenceError;
pub use gate::{Feature, Refusal, Why};
pub use machine::{Derivation, MachineId};
pub use plan::{FeatureSet, Limits, Plan};
pub use snapshot::{SignedSnapshot, Snapshot, VerifyError, verify_detached};
pub use state::{LicenceFile, Licensing, PendingRelease, TRANSFER_COOLDOWN_DAYS};
pub use status::{Licence, Standing, Status};
