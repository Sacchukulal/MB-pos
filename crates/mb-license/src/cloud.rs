use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::machine::MachineId;
use crate::snapshot::SignedSnapshot;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CloudError {
    /// No network, DNS failure, the server is down.
    #[error("we could not reach our server")]
    Unreachable,
    /// The server answered, and said no.
    #[error("{0}")]
    Refused(String),
    /// The key or the proof was wrong.
    #[error("that licence key and code did not match")]
    NotRecognised,
    /// The licence is already on another machine and the caller did not ask to move it.
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
    /// The counter's version, so the cloud can tell an old till from a new one without guessing
    /// from behaviour.
    pub counter_version: String,
}

/// Everything that has to leave this machine.
pub trait Cloud: Send + Sync + 'static {
    fn activate(&self, ask: &Ask, proof: &str) -> Result<SignedSnapshot, CloudError>;

    /// The routine check. Returns the current truth, signed.
    fn refresh(&self, ask: &Ask) -> Result<SignedSnapshot, CloudError>;

    /// Release the binding, server-side.
    fn release(&self, ask: &Ask) -> Result<(), CloudError>;

    /// Move a licence to this machine from wherever it was.
    fn transfer(&self, ask: &Ask, proof: &str) -> Result<SignedSnapshot, CloudError>;

    /// Requirement 4: a real self-service trial with an end date.
    fn start_trial(&self, ask: &Ask, contact: &str) -> Result<SignedSnapshot, CloudError>;
}

// The stub.

use std::sync::Mutex;
use std::time::Duration;

use mb_core::{BusinessDay, Timestamp};

use crate::plan::Plan;
use crate::snapshot::{self, Snapshot};
use crate::status::{Licence, Status};

/// A cloud that lives in a `Mutex`.
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
    /// So a test can assert that `release` really was called.
    pub released: Vec<String>,
    pub expected_proof: String,
}

/// How the stub misbehaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    Normal,
    Unreachable,
    /// Sleeps far past any deadline.
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

    /// Poisoning is not a case worth modelling in a test double: a poisoned mutex means a test
    /// thread panicked, and the failure that matters is that panic rather than this lock.
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

    /// Did anybody actually release the binding?
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
                // The socket a PC suspend killed.
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

    #[test]
    fn release_frees_the_licence_for_another_computer() {
        let (stub, machine) = a_stub();
        stub.release(&an_ask(&machine)).expect("releases");
        assert_eq!(stub.released(), vec![machine.value().to_owned()]);

        let new_pc = MachineId::for_tests("machine-b-0002");
        assert!(stub.activate(&an_ask(&new_pc), "123456").is_ok());
        assert_eq!(stub.licence().bound_to, Some(new_pc));
    }

    /// A transfer releases the old machine too.
    #[test]
    fn a_transfer_releases_the_machine_it_came_from() {
        let (stub, machine) = a_stub();
        let new_pc = MachineId::for_tests("machine-b-0002");
        stub.transfer(&an_ask(&new_pc), "123456")
            .expect("transfers");
        assert!(stub.released().contains(&machine.value().to_owned()));
        assert_eq!(stub.licence().bound_to, Some(new_pc));
    }

    #[test]
    fn an_unreachable_cloud_says_so_rather_than_lying() {
        let (stub, machine) = a_stub();
        stub.behave(Behaviour::Unreachable);
        assert_eq!(
            stub.refresh(&an_ask(&machine)),
            Err(CloudError::Unreachable)
        );
    }

    #[test]
    fn what_it_signs_verifies() {
        let (stub, machine) = a_stub();
        let signed = stub.refresh(&an_ask(&machine)).expect("refreshes");
        let back = snapshot::verify(&signed, &snapshot::trusted_keys()).expect("verifies");
        assert_eq!(back.licence.status, Status::Active);
    }
}
