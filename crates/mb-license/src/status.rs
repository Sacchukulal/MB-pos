//! The licence, its status, and the standing that comes out of both.

use mb_core::{BusinessDay, Timestamp};
use serde::{Deserialize, Serialize};

use crate::machine::MachineId;
use crate::plan::Plan;

/// What the cloud says about this licence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Paid for, and running.
    Active,
    /// A self-service trial with an end date.
    Trial,
    /// An admin pressed Suspend, or a payment failed.
    Suspended,
    /// An admin pressed Revoke.
    Revoked,
    /// The shop cancelled. Their choice, and the copy must not scold them.
    Cancelled,
}

impl Status {
    /// The stable code, for the snapshot and for a support call.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Trial => "trial",
            Status::Suspended => "suspended",
            Status::Revoked => "revoked",
            Status::Cancelled => "cancelled",
        }
    }

    /// Does this status let the shop operate at all, before any date is considered? A cancelled
    /// plan is a date question: it runs to the day that was paid for.
    #[must_use]
    pub const fn lets_the_shop_work(self) -> bool {
        match self {
            Status::Active | Status::Trial | Status::Cancelled => true,
            Status::Suspended | Status::Revoked => false,
        }
    }
}

/// A licence, as the cloud describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Licence {
    pub key: String,
    pub shop_name: String,
    pub plan: Plan,
    pub status: Status,
    /// The next billing date.
    pub renews_on: BusinessDay,
    /// This licence's OWN grace override, set per customer in the admin panel.
    pub grace_days: Option<u16>,
    /// The machine this licence is bound to, if any.
    pub bound_to: Option<MachineId>,
    pub trial_ends_on: Option<BusinessDay>,
    /// Razorpay is still charging. Off after a cancellation.
    #[serde(default)]
    pub auto_renews: Option<bool>,
    /// Already masked by the cloud — `+91 98••••••10`.
    pub registered_contact: String,
    /// The name on the account that holds the licence. Blank from a cloud before 0025.
    #[serde(default)]
    pub owner_name: String,
    /// That account's mobile as the cloud writes it, `+919840011223`.
    #[serde(default)]
    pub owner_phone: String,
    /// The cloud's id for this shop. What a release rollout names.
    #[serde(default)]
    pub restaurant_id: Option<String>,
    /// What a staff member types to log in on a phone. Shown on the Account screen.
    #[serde(default)]
    pub short_code: Option<String>,
}

/// What the licence means today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Standing {
    /// Paid, in date, running.
    Fine,
    /// Past the billing date and inside the grace period.
    InGrace {
        days_left: u16,
    },
    /// Past the billing date and past the grace period.
    Expired,
    Suspended,
    Revoked,
    /// Cancelled, and still inside the paid period: running, with an end in sight.
    Ending {
        days_left: u16,
    },
    /// Cancelled, and past the paid period.
    Cancelled,
    /// No licence on this machine at all: a first run, or after a deactivate.
    NeverActivated,
    TrialEnded,
    /// We have not been able to ask for too long.
    NeedsChecking,
    /// This licence belongs to a different computer.
    BoundElsewhere,
    /// The 72-hour offline unlock support read out over the phone.
    Emergency {
        until: Timestamp,
    },
}

impl Standing {
    /// May the shop use the things a plan pays for?
    #[must_use]
    pub const fn operating(self) -> bool {
        match self {
            Standing::Fine
            | Standing::InGrace { .. }
            | Standing::Ending { .. }
            | Standing::Emergency { .. } => true,
            Standing::Expired
            | Standing::Suspended
            | Standing::Revoked
            | Standing::Cancelled
            | Standing::NeverActivated
            | Standing::TrialEnded
            | Standing::NeedsChecking
            | Standing::BoundElsewhere => false,
        }
    }

    /// The chip on the account screen.
    #[must_use]
    pub const fn chip(self) -> &'static str {
        match self {
            Standing::Fine => "Active",
            Standing::InGrace { .. } => "Grace period",
            Standing::Expired => "Expired",
            Standing::Suspended => "Suspended",
            Standing::Revoked => "Stopped",
            Standing::Ending { .. } => "Ends soon",
            Standing::Cancelled => "Cancelled",
            Standing::NeverActivated => "Not activated",
            Standing::TrialEnded => "Trial ended",
            Standing::NeedsChecking => "Needs checking",
            Standing::BoundElsewhere => "Another computer",
            Standing::Emergency { .. } => "Emergency unlock",
        }
    }

    /// The stable code the front end switches on.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Standing::Fine => "fine",
            Standing::InGrace { .. } => "grace",
            Standing::Expired => "expired",
            Standing::Suspended => "suspended",
            Standing::Revoked => "revoked",
            Standing::Ending { .. } => "ending",
            Standing::Cancelled => "cancelled",
            Standing::NeverActivated => "never-activated",
            Standing::TrialEnded => "trial-ended",
            Standing::NeedsChecking => "needs-checking",
            Standing::BoundElsewhere => "bound-elsewhere",
            Standing::Emergency { .. } => "emergency",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_a_gate() {
        assert!(Status::Active.lets_the_shop_work());
        assert!(Status::Trial.lets_the_shop_work());
        assert!(!Status::Suspended.lets_the_shop_work());
        assert!(!Status::Revoked.lets_the_shop_work());
        // A cancelled plan is a date question, answered by `decide`.
        assert!(Status::Cancelled.lets_the_shop_work());
    }

    #[test]
    fn a_plan_that_is_ending_still_works_and_a_cancelled_one_does_not() {
        assert!(Standing::Ending { days_left: 3 }.operating());
        assert!(!Standing::Cancelled.operating());
        assert_eq!(Standing::Ending { days_left: 3 }.code(), "ending");
    }

    #[test]
    fn a_grace_period_does_not_quietly_remove_features() {
        assert!(Standing::InGrace { days_left: 1 }.operating());
        assert!(Standing::InGrace { days_left: 30 }.operating());
    }

    #[test]
    fn an_emergency_unlock_is_a_working_shop() {
        assert!(
            Standing::Emergency {
                until: Timestamp::from_millis(1),
            }
            .operating()
        );
    }

    #[test]
    fn every_standing_has_a_chip_and_a_code_and_they_are_distinct() {
        let all = [
            Standing::Fine,
            Standing::InGrace { days_left: 3 },
            Standing::Expired,
            Standing::Suspended,
            Standing::Revoked,
            Standing::Cancelled,
            Standing::NeverActivated,
            Standing::TrialEnded,
            Standing::NeedsChecking,
            Standing::BoundElsewhere,
            Standing::Emergency {
                until: Timestamp::EPOCH,
            },
        ];
        let mut codes: Vec<&str> = all.iter().map(|s| s.code()).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), before, "two standings share a code");
        for standing in all {
            assert!(!standing.chip().is_empty());
        }
    }
}
