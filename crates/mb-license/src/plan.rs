//! What a plan includes, and what it limits.

use serde::{Deserialize, Serialize};

use crate::gate::Feature;

/// What a plan lets a shop use.
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

    /// Everything this build knows about — the plan a shop on the top tier has, and what the
    /// stub hands out in tests.
    #[must_use]
    pub fn everything() -> Self {
        FeatureSet(Feature::ALL.iter().map(|f| f.code().to_owned()).collect())
    }

    #[must_use]
    pub fn includes(&self, feature: Feature) -> bool {
        self.0.iter().any(|code| code == feature.code())
    }

    /// The features this build understands, for the account screen's list.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Phones that may be paired to this counter.
    pub devices: u32,
    /// Tills on this shop.
    pub terminals: u32,
}

impl Default for Limits {
    fn default() -> Self {
        // What an unactivated counter answers with.
        Limits {
            devices: 2,
            terminals: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Stable, from the cloud.
    pub code: String,
    /// What the account screen shows: "Restaurant Standard".
    pub name: String,
    pub features: FeatureSet,
    pub limits: Limits,
}

impl Plan {
    /// The plan a shop has before it has one — a trial's shape, and what the stub starts from.
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
        // A newer cloud talking to an older till.
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

    #[test]
    fn an_unactivated_counter_is_not_capped_at_zero_phones() {
        assert!(Limits::default().devices > 0);
        assert_eq!(Limits::default().terminals, 1);
    }
}
