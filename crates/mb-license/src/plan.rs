//! What a plan includes, and what it limits.
//!
//! # A plan is DATA
//!
//! The same ruling the theme system got (D21, and the owner's words were *"so
//! that in future it can be changed easily with my suggestion without touching
//! any functionality of the app"*). A new plan — "Restaurant Plus, four phones,
//! reports and cloud backup" — is a row in the admin panel and a field in a
//! signed snapshot. It is **never** a Rust release, because the alternative is
//! that every price change the owner makes waits for a build, an update and a
//! shop that agrees to install it.
//!
//! So [`FeatureSet`] holds codes, an unknown code is ignored rather than
//! rejected, and there is no `enum PlanCode` anywhere in this product.

use serde::{Deserialize, Serialize};

use crate::gate::Feature;

/// What a plan lets a shop use.
///
/// Stored and transmitted as a list of stable codes. **An unknown code is
/// dropped, not an error**: a cloud that has learned about a feature this
/// counter has never heard of must not make the counter refuse to start. R3
/// says a failure is never silent, and this is not a failure — it is a newer
/// server talking to an older till, which is the normal state of a fleet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureSet(Vec<String>);

impl FeatureSet {
    #[must_use]
    pub fn from_codes<I, S>(codes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        FeatureSet(codes.into_iter().map(Into::into).collect())
    }

    /// Everything this build knows about — the plan a shop on the top tier has,
    /// and what the stub hands out in tests.
    #[must_use]
    pub fn everything() -> Self {
        FeatureSet(Feature::ALL.iter().map(|f| f.code().to_owned()).collect())
    }

    #[must_use]
    pub fn includes(&self, feature: Feature) -> bool {
        self.0.iter().any(|code| code == feature.code())
    }

    /// The features this build understands, for the account screen's list. A
    /// code we do not recognise is not shown, because a line reading
    /// "kitchen-display-v2" helps nobody.
    #[must_use]
    pub fn known(&self) -> Vec<Feature> {
        Feature::ALL
            .iter()
            .copied()
            .filter(|f| self.includes(*f))
            .collect()
    }

    #[must_use]
    pub fn codes(&self) -> &[String] {
        &self.0
    }
}

/// The numbers a plan caps.
///
/// **WEBSITE-C5 is why these are read on every check and not at enrolment:**
/// *"The phone limit is shown and stored but only checked at the moment a new
/// phone first joins — never afterwards. Lowering a customer's limit does not
/// cut off phones already enrolled."* `mb_lan::Counter::device_limit` asks the
/// live entitlement for this number on every pairing attempt, and the network
/// panel shows when a shop is over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Phones that may be paired to this counter.
    pub devices: u32,
    /// Tills on this shop (P27). One, today, for everybody.
    pub terminals: u32,
}

impl Default for Limits {
    fn default() -> Self {
        // What an unactivated counter answers with. Not zero: a shop on its
        // first day, before anybody has typed a key, still has one till and can
        // still pair the owner's own phone to try it out. Zero here would mean
        // a trial that cannot demonstrate the feature it exists to sell.
        Limits {
            devices: 2,
            terminals: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Stable, from the cloud. Never matched on in Rust.
    pub code: String,
    /// What the account screen shows: "Restaurant Standard".
    pub name: String,
    pub features: FeatureSet,
    pub limits: Limits,
}

impl Plan {
    /// The plan a shop has before it has one — a trial's shape, and what the
    /// stub starts from.
    #[must_use]
    pub fn trial() -> Self {
        Plan {
            code: "trial".to_owned(),
            name: "Free trial".to_owned(),
            features: FeatureSet::everything(),
            limits: Limits {
                devices: 2,
                terminals: 1,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_this_build_has_never_heard_of_is_ignored_and_not_fatal() {
        // A newer cloud talking to an older till. It must not stop the counter.
        let set = FeatureSet::from_codes(["reports", "kitchen-display-v2"]);
        assert!(set.includes(Feature::Reports));
        assert!(!set.includes(Feature::MobileOrdering));
        assert_eq!(set.known(), vec![Feature::Reports]);
    }

    #[test]
    fn everything_means_every_feature_this_build_knows() {
        let set = FeatureSet::everything();
        for feature in Feature::ALL {
            assert!(set.includes(*feature), "{feature:?} was missing");
        }
    }

    #[test]
    fn a_plan_survives_a_round_trip_through_json() {
        let plan = Plan::trial();
        let text = serde_json::to_string(&plan).expect("serialises");
        let back: Plan = serde_json::from_str(&text).expect("parses");
        assert_eq!(plan, back);
    }

    /// A shop with no licence can still pair the owner's phone to look at it.
    #[test]
    fn an_unactivated_counter_is_not_capped_at_zero_phones() {
        assert!(Limits::default().devices > 0);
        assert_eq!(Limits::default().terminals, 1);
    }
}
