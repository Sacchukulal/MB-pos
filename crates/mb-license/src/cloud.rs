//! **The seam Phase 8 implements, and the stub this session builds against.**
//!
//! Requirement 7: *"this session defines the interface the cloud will
//! implement. Behind it, a local stub is enough to build and test against.
//! Nothing here may hang."*
//!
//! # No method takes a deadline, on purpose
//!
//! It would be the obvious signature and it would be the wrong one. See
//! [`crate::deadline`]: a deadline the callee promises to honour is a request,
//! not a deadline. Every **call site** wraps the call in
//! [`crate::deadline::within`], so an implementation cannot forget one — and
//! `no_cloud_call_is_unwrapped` in `state.rs` reads the source and proves no
//! call site skipped it, which is the same shape as `guard`'s coverage test.
//!
//! # What the cloud must do that the counter cannot make it do
//!
//! Two of these findings are only half fixable from here, and
//! `MB-pos/docs/LICENCE_PROTOCOL.md` says so in a box so P34 cannot miss it:
//!
//! * **BACKEND-C4** — when a binding moves, the old machine's **cloud
//!   credential must be revoked**, not merely unbound. *"That old machine can
//!   still push bills, write live orders and read the whole menu — for as long
//!   as the subscription lasts."* The counter can ask; only the cloud can do it.
//! * **BACKEND-C6** — first activation must verify **something the buyer has**.
//!   That is why [`Cloud::activate`] takes a `proof` and why there is no way to
//!   call it without one.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::machine::MachineId;
use crate::snapshot::SignedSnapshot;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CloudError {
    /// No network, DNS failure, the server is down. **Never fatal** — the
    /// counter falls back to its cached snapshot and keeps working.
    #[error("we could not reach our server")]
    Unreachable,
    /// The server answered, and said no. The sentence is the SERVER'S, and it
    /// is shown as-is: D84's rule ("a refusal a waiter reads") applied to a
    /// refusal an owner reads. The counter composes nothing here, because the
    /// reason a licence was refused is a thing only the cloud knows.
    #[error("{0}")]
    Refused(String),
    /// The key or the proof was wrong.
    #[error("that licence key and code did not match")]
    NotRecognised,
    /// The licence is already on another machine and the caller did not ask to
    /// move it.
    #[error("this licence is already in use on another computer")]
    BoundElsewhere { machine: String },
    /// The server answered with something this build could not read.
    #[error("our server sent something this version could not read")]
    Unreadable,
}

/// What the counter sends when it asks for a fresh answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    pub key: String,
    pub machine: MachineId,
    /// The counter's version, so the cloud can tell an old till from a new one
    /// without guessing from behaviour.
    pub counter_version: String,
}

/// **Everything that has to leave this machine.** Five calls, and no more.
pub trait Cloud: Send + Sync + 'static {
    /// First activation. **`proof` is BACKEND-C6**: the code sent to the
    /// registered mobile or email. A key alone is not enough, because in v1
    /// *"whoever types the key first becomes the counter"*.
    ///
    /// # Errors
    ///
    /// [`CloudError`].
    fn activate(&self, ask: &Ask, proof: &str) -> Result<SignedSnapshot, CloudError>;

    /// The routine check. Returns the current truth, signed.
    ///
    /// # Errors
    ///
    /// [`CloudError`].
    fn refresh(&self, ask: &Ask) -> Result<SignedSnapshot, CloudError>;

    /// **Release the binding, server-side.** BACKEND-C5 is that this did not
    /// exist: *"'Deactivate' on the counter is a trap. It only clears the key
    /// locally."*
    ///
    /// The cloud must also revoke this machine's cloud credential (C4).
    ///
    /// # Errors
    ///
    /// [`CloudError`].
    fn release(&self, ask: &Ask) -> Result<(), CloudError>;

    /// Move a licence to this machine from wherever it was. POS-A4's
    /// self-service path. `proof` again, for the same reason as `activate`.
    ///
    /// # Errors
    ///
    /// [`CloudError`].
    fn transfer(&self, ask: &Ask, proof: &str) -> Result<SignedSnapshot, CloudError>;

    /// Requirement 4: a real self-service trial with an end date.
    ///
    /// # Errors
    ///
    /// [`CloudError`].
    fn start_trial(&self, ask: &Ask, contact: &str) -> Result<SignedSnapshot, CloudError>;
}

// ---------------------------------------------------------------------------
// The stub.
// ---------------------------------------------------------------------------

use std::sync::Mutex;
use std::time::Duration;

use mb_core::{BusinessDay, Timestamp};

use crate::plan::Plan;
use crate::snapshot::{self, Snapshot};
use crate::status::{Licence, Status};

/// **A cloud that lives in a `Mutex`.** Every test in this crate and in
/// `src-tauri` runs against it; there is no HTTP anywhere in P21.
///
/// It can be told to be slow, to be unreachable, or to **never answer at all**
/// — which is T9, and which is the state v1 deadlocked in.
pub struct Stub {
    inner: Mutex<Inner>,
}

struct Inner {
    licence: Licence,
    global_grace_days: Option<u16>,
    /// What `now` the stub stamps its snapshots with.
    issued_at: Timestamp,
    not_after: Timestamp,
    max_offline_days: u16,
    behaviour: Behaviour,
    /// So a test can assert that `release` really was called — BACKEND-C5's
    /// whole content is that it was not.
    pub released: Vec<String>,
    pub expected_proof: String,
}

/// How the stub misbehaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    Normal,
    Unreachable,
    /// Sleeps far past any deadline. T9.
    NeverAnswers,
    Slow(Duration),
}

impl Stub {
    /// A working cloud with an active licence on the top plan.
    #[must_use]
    pub fn active(machine: &MachineId, renews_on: BusinessDay, now: Timestamp) -> Stub {
        Stub::with(
            Licence {
                key: "MB-STUB-0001".to_owned(),
                shop_name: "Anna's Kitchen".to_owned(),
                plan: Plan::trial(),
                status: Status::Active,
                renews_on,
                grace_days: None,
                bound_to: Some(machine.clone()),
                trial_ends_on: None,
                registered_contact: "+91 98••••••10".to_owned(),
            },
            now,
        )
    }

    #[must_use]
    pub fn with(licence: Licence, now: Timestamp) -> Stub {
        Stub {
            inner: Mutex::new(Inner {
                licence,
                global_grace_days: None,
                issued_at: now,
                not_after: Timestamp::from_millis(now.millis().saturating_add(7 * 86_400_000)),
                max_offline_days: 14,
                behaviour: Behaviour::Normal,
                released: Vec::new(),
                expected_proof: "123456".to_owned(),
            }),
        }
    }

    /// Poisoning is not a case worth modelling in a test double: a poisoned
    /// mutex means a test thread panicked, and the failure that matters is that
    /// panic rather than this lock.
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn behave(&self, behaviour: Behaviour) {
        self.inner().behaviour = behaviour;
    }

    pub fn set_status(&self, status: Status) {
        self.inner().licence.status = status;
    }

    pub fn set_licence(&self, licence: Licence) {
        self.inner().licence = licence;
    }

    pub fn set_global_grace(&self, days: Option<u16>) {
        self.inner().global_grace_days = days;
    }

    pub fn set_clock(&self, now: Timestamp) {
        let mut inner = self.inner();
        inner.issued_at = now;
        inner.not_after = Timestamp::from_millis(now.millis().saturating_add(7 * 86_400_000));
    }

    pub fn set_max_offline_days(&self, days: u16) {
        self.inner().max_offline_days = days;
    }

    /// **Did anybody actually release the binding?** BACKEND-C5.
    #[must_use]
    pub fn released(&self) -> Vec<String> {
        self.inner().released.clone()
    }

    #[must_use]
    pub fn licence(&self) -> Licence {
        self.inner().licence.clone()
    }

    fn misbehave(&self) -> Option<CloudError> {
        let behaviour = self.inner().behaviour;
        match behaviour {
            Behaviour::Normal => None,
            Behaviour::Unreachable => Some(CloudError::Unreachable),
            Behaviour::NeverAnswers => {
                // The socket a PC suspend killed. The caller's deadline is the
                // only thing that gets anybody out of here.
                std::thread::sleep(Duration::from_secs(3_600));
                Some(CloudError::Unreachable)
            }
            Behaviour::Slow(how_long) => {
                std::thread::sleep(how_long);
                None
            }
        }
    }

    fn sign_current(&self) -> Result<SignedSnapshot, CloudError> {
        let inner = self.inner();
        let snapshot = Snapshot {
            licence: inner.licence.clone(),
            global_grace_days: inner.global_grace_days,
            issued_at: inner.issued_at,
            not_after: inner.not_after,
            max_offline_days: inner.max_offline_days,
        };
        drop(inner);
        let key = snapshot::development_keypair().map_err(|_| CloudError::Unreadable)?;
        snapshot::sign(&snapshot, &key).map_err(|_| CloudError::Unreadable)
    }
}

impl Cloud for Stub {
    fn activate(&self, ask: &Ask, proof: &str) -> Result<SignedSnapshot, CloudError> {
        if let Some(problem) = self.misbehave() {
            return Err(problem);
        }
        {
            let mut inner = self.inner();
            if proof != inner.expected_proof {
                // BACKEND-C6: the key alone is not enough.
                return Err(CloudError::NotRecognised);
            }
            if inner.licence.key != ask.key {
                return Err(CloudError::NotRecognised);
            }
            match &inner.licence.bound_to {
                Some(bound) if bound != &ask.machine => {
                    return Err(CloudError::BoundElsewhere {
                        machine: bound.short(),
                    });
                }
                _ => {}
            }
            inner.licence.bound_to = Some(ask.machine.clone());
        }
        self.sign_current()
    }

    fn refresh(&self, ask: &Ask) -> Result<SignedSnapshot, CloudError> {
        if let Some(problem) = self.misbehave() {
            return Err(problem);
        }
        {
            let inner = self.inner();
            // **The binding is checked on every refresh**, which is BACKEND-C4
            // from the counter's side: a machine that is no longer the bound
            // one stops being told it is entitled.
            if let Some(bound) = &inner.licence.bound_to
                && bound != &ask.machine
            {
                return Err(CloudError::BoundElsewhere {
                    machine: bound.short(),
                });
            }
        }
        self.sign_current()
    }

    fn release(&self, ask: &Ask) -> Result<(), CloudError> {
        if let Some(problem) = self.misbehave() {
            return Err(problem);
        }
        let mut inner = self.inner();
        inner.released.push(ask.machine.value().to_owned());
        inner.licence.bound_to = None;
        Ok(())
    }

    fn transfer(&self, ask: &Ask, proof: &str) -> Result<SignedSnapshot, CloudError> {
        if let Some(problem) = self.misbehave() {
            return Err(problem);
        }
        {
            let mut inner = self.inner();
            if proof != inner.expected_proof {
                return Err(CloudError::NotRecognised);
            }
            // The old machine is released, and P34 must also revoke its cloud
            // credential — BACKEND-C4, in the document, in a box.
            if let Some(old) = inner.licence.bound_to.clone() {
                inner.released.push(old.value().to_owned());
            }
            inner.licence.bound_to = Some(ask.machine.clone());
        }
        self.sign_current()
    }

    fn start_trial(&self, ask: &Ask, _contact: &str) -> Result<SignedSnapshot, CloudError> {
        if let Some(problem) = self.misbehave() {
            return Err(problem);
        }
        {
            let mut inner = self.inner();
            let ends = BusinessDay::from_days_since_epoch(
                i32::try_from(inner.issued_at.millis().div_euclid(86_400_000)).unwrap_or(0) + 14,
            );
            inner.licence.status = Status::Trial;
            inner.licence.trial_ends_on = Some(ends);
            inner.licence.bound_to = Some(ask.machine.clone());
        }
        self.sign_current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn an_ask(machine: &MachineId) -> Ask {
        Ask {
            key: "MB-STUB-0001".to_owned(),
            machine: machine.clone(),
            counter_version: "0.1.0".to_owned(),
        }
    }

    fn a_stub() -> (Stub, MachineId) {
        let machine = MachineId::for_tests("machine-a-0001");
        let stub = Stub::active(
            &machine,
            BusinessDay::from_ymd(2026, 9, 12),
            Timestamp::from_millis(20_000 * 86_400_000),
        );
        (stub, machine)
    }

    /// **BACKEND-C6.** The key alone does not activate anything.
    #[test]
    fn activation_needs_something_the_buyer_has() {
        let (stub, machine) = a_stub();
        stub.inner().licence.bound_to = None;
        assert_eq!(
            stub.activate(&an_ask(&machine), "000000"),
            Err(CloudError::NotRecognised)
        );
        assert!(stub.activate(&an_ask(&machine), "123456").is_ok());
    }

    /// **BACKEND-C4, from this side.** A machine that is not the bound one is
    /// refused on every refresh, not merely at enrolment.
    #[test]
    fn a_machine_that_is_no_longer_bound_is_refused_on_refresh() {
        let (stub, machine) = a_stub();
        assert!(stub.refresh(&an_ask(&machine)).is_ok());

        let other = MachineId::for_tests("machine-b-0002");
        match stub.refresh(&an_ask(&other)) {
            Err(CloudError::BoundElsewhere { machine: m }) => assert_eq!(m, machine.short()),
            other => panic!("{other:?}"),
        }
    }

    /// **BACKEND-C5.** Release actually releases, and a later activation on a
    /// different machine works — which in v1 needed a support call.
    #[test]
    fn release_frees_the_licence_for_another_computer() {
        let (stub, machine) = a_stub();
        stub.release(&an_ask(&machine)).expect("releases");
        assert_eq!(stub.released(), vec![machine.value().to_owned()]);

        let new_pc = MachineId::for_tests("machine-b-0002");
        assert!(stub.activate(&an_ask(&new_pc), "123456").is_ok());
        assert_eq!(stub.licence().bound_to, Some(new_pc));
    }

    /// A transfer releases the old machine too — the half of C4 the counter
    /// can see.
    #[test]
    fn a_transfer_releases_the_machine_it_came_from() {
        let (stub, machine) = a_stub();
        let new_pc = MachineId::for_tests("machine-b-0002");
        stub.transfer(&an_ask(&new_pc), "123456").expect("transfers");
        assert!(stub.released().contains(&machine.value().to_owned()));
        assert_eq!(stub.licence().bound_to, Some(new_pc));
    }

    #[test]
    fn an_unreachable_cloud_says_so_rather_than_lying() {
        let (stub, machine) = a_stub();
        stub.behave(Behaviour::Unreachable);
        assert_eq!(stub.refresh(&an_ask(&machine)), Err(CloudError::Unreachable));
    }

    #[test]
    fn what_it_signs_verifies() {
        let (stub, machine) = a_stub();
        let signed = stub.refresh(&an_ask(&machine)).expect("refreshes");
        let back = snapshot::verify(&signed, &snapshot::trusted_keys()).expect("verifies");
        assert_eq!(back.licence.status, Status::Active);
    }
}
