//! What the gate is able to refuse — and the proof that billing is not on the list.

use serde::{Deserialize, Serialize};

/// Everything the licence gate can refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Feature {
    /// The reports screen and its exports.
    Reports,
    /// Waiters taking orders on their phones.
    MobileOrdering,
    /// More than one till on the same shop.
    MultiTerminal,
    /// The stock book — materials, recipes, food cost and the variance report.
    Inventory,
}

impl Feature {
    /// Every feature, for the tests and for the account screen's limits panel.
    pub const ALL: &'static [Feature] = &[
        Feature::Reports,
        Feature::MobileOrdering,
        Feature::MultiTerminal,
        Feature::Inventory,
    ];

    /// Closing the day is not a report, and this constant is where that decision is written.
    pub const REPORTS_DOES_NOT_MEAN_THE_DAY_CLOSE: &'static [&'static str] = &[
        "day_state",
        "days",
        "close_pending",
        "close_day",
        "mark_holiday",
        "unmark_holiday",
        "reopen_day",
        "count_cash",
        "count_drawer",
    ];

    /// The stable code. This is what a plan carries, so adding a plan in the admin panel never
    /// needs a counter release.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Feature::Reports => "reports",
            Feature::MobileOrdering => "mobile-ordering",
            Feature::MultiTerminal => "multi-terminal",
            Feature::Inventory => "inventory",
        }
    }

    /// What a shopkeeper calls it.
    #[must_use]
    pub const fn in_words(self) -> &'static str {
        match self {
            Feature::Reports => "reports",
            Feature::MobileOrdering => "phone ordering",
            Feature::MultiTerminal => "extra tills",
            Feature::Inventory => "stock and recipes",
        }
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Feature> {
        Feature::ALL.iter().copied().find(|f| f.code() == code)
    }
}

/// Why the gate said no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    pub feature: Feature,
    pub why: Why,
}

/// The two conversations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The shop is not entitled to operate: expired, suspended, revoked, cancelled, never
    /// activated, or a trial that ended.
    NotOperating(crate::status::Standing),
    /// The shop is perfectly fine.
    NotInThePlan,
}

impl Refusal {
    /// The stable code, for a support call and for the front end's tests.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self.why {
            Why::NotOperating(_) => "licence.not_operating",
            Why::NotInThePlan => "licence.not_in_plan",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_cannot_reach_billing() {
        let codes: Vec<&str> = Feature::ALL.iter().map(|f| f.code()).collect();
        assert_eq!(
            codes,
            vec!["reports", "mobile-ordering", "multi-terminal", "inventory"],
            "the list of gateable features changed"
        );
        for banned in [
            "billing",
            "bill",
            "printing",
            "print",
            "local-backup",
            "drawer",
        ] {
            assert!(
                Feature::from_code(banned).is_none(),
                "{banned} is gateable, and it must never be — see D86"
            );
        }
    }

    #[test]
    fn every_feature_round_trips_through_its_code() {
        for feature in Feature::ALL {
            assert_eq!(Feature::from_code(feature.code()), Some(*feature));
        }
    }

    /// The two refusals are two different conversations, and they must stay distinguishable all
    /// the way out to the screen.
    #[test]
    fn the_two_refusals_do_not_read_alike() {
        let expired = Refusal {
            feature: Feature::MobileOrdering,
            why: Why::NotOperating(crate::status::Standing::Expired),
        };
        let missing = Refusal {
            feature: Feature::MobileOrdering,
            why: Why::NotInThePlan,
        };
        assert_ne!(expired.code(), missing.code());
    }

    /// And there is no prose in here at all.
    #[test]
    fn a_refusal_carries_facts_and_not_a_sentence() {
        // Only the shipped half of the file.
        let source = crate::shipped_part_of(include_str!("gate.rs"));
        for prose in ["Your plan ran out", "please call", "days remaining"] {
            let in_code = source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .any(|line| line.contains(prose));
            assert!(!in_code, "{prose:?} is being composed in gate.rs");
        }
    }

    /// The day close is named, so deleting the constant is a visible act.
    #[test]
    fn the_day_close_is_not_a_report() {
        assert!(
            Feature::REPORTS_DOES_NOT_MEAN_THE_DAY_CLOSE.contains(&"close_day"),
            "closing the day is how a shop counts its drawer"
        );
    }
}
