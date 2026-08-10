//! **May this shop operate right now, and what may it use?**
//!
//! One question, answered in one place, by everything that asks.
//!
//! # Why this crate exists
//!
//! > **BACKEND-C1:** *"'Suspend' and 'Revoke' in the admin panel do nothing.
//! > Both set `licenses.status`. **Nothing anywhere reads `status` as a gate.**
//! > The counter's lock is based purely on the next billing date; the cloud's
//! > resolver is based purely on the next billing date. The admin screen even
//! > says 'the POS locks at its next status check' — **it does not.**"*
//!
//! v1 had three buttons that lied: Suspend, Revoke and Deactivate. They lied
//! because the counter, the cloud and the phone each had their own idea of
//! whether a shop was allowed to work, and nobody ever tested what the screens
//! claimed. [`decide`] is the one idea, [`Entitlement`] is its answer, and
//! every claim in this crate has a test with the finding's name on it.
//!
//! # The principle that decides every detail
//!
//! **A billing system may never hold a restaurant hostage.** A shop that cannot
//! bill cannot trade. Not for an expired plan, not for no internet, not for a
//! failed check, not for a corrupt cache, not for a clock that has gone
//! backwards.
//!
//! That is not a policy written in a comment and hoped for. It is [`Feature`],
//! which is the complete list of what the gate is able to refuse, and there is
//! no billing in it — see [`gate`] and **D86**.
//!
//! # What is where
//!
//! * [`plan`] — what a plan includes and what it limits.
//! * [`status`] — the licence's status, and the standing it produces.
//! * [`entitlement`] — [`decide`], and the answer everything else reads.
//! * [`gate`] — what may be refused, and the sentence a shopkeeper reads.
//! * [`machine`] — which computer this is, and how sure we are.
//! * [`snapshot`] — the signed, twice-expiring offline cache.
//! * [`clock`] — the high-water mark a stopped clock cannot walk back.
//! * [`emergency`] — the code support reads out when a PC has died.
//! * [`cloud`] — the trait Phase 8 implements, and the stub this session
//!   builds against.
//! * [`deadline`] — why no call in here can hang the counter.
//! * [`state`] — `licence.json`, which lives beside the config and **never** in
//!   the shop's database (D85).

/// **A source file with its test module cut off.**
///
/// Two tests in this crate read their own source to prove a rule — that no
/// sentence is composed in `gate.rs`, and that no cloud call in `state.rs`
/// skips its deadline. Both would fail on themselves, because a test that
/// looks for a phrase has to name the phrase. This trims at the `#[cfg(test)]`
/// line, so a scan sees exactly what ships.
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
