//! **What the gate is able to refuse — and the proof that billing is not on
//! the list.**
//!
//! # D86 — billing is not gateable, and it is a type that says so
//!
//! Requirement 3 of the ten is that a shop can always bill. Every product that
//! has ever broken that promise broke it the same way: somebody wrote "we do
//! not block billing" in a design document, and eighteen months later a new
//! feature added one more call to a function called `check_licence()` and
//! nobody noticed which call site it was.
//!
//! So the promise is not written down here. [`Feature`] is the **complete list
//! of things the gate can be asked about**, and there is no `Billing` variant,
//! no `Printing` variant and no `LocalBackup` variant. It is not that we choose
//! not to refuse them — **there is nowhere to write the refusal.**
//! [`crate::Entitlement::may`] takes a `Feature` and nothing else, so a call
//! that would stop a cashier taking money for food does not typecheck.
//!
//! A future session that genuinely wants to stop a shop billing has to add a
//! variant, and `the_gate_cannot_reach_billing` below fails the build when one
//! appears. That is D40's rule — *the rules that erode are enforced by scripts,
//! not by agreement* — applied to the most important rule in the product.

use serde::{Deserialize, Serialize};

/// **Everything the licence gate can refuse. All of it.**
///
/// See the module note before adding a variant. If you are adding one because a
/// plan should include or exclude something, that is the right reason. If you
/// are adding one because a shop that has not paid should stop being able to
/// take an order, read D86 and then read requirement 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Feature {
    /// The reports screen and its exports. **Not the day close** — see
    /// `Feature::REPORTS_DOES_NOT_MEAN_THE_DAY_CLOSE`.
    Reports,
    /// Bills copied to the cloud (Phase 8). A seam today.
    CloudBackup,
    /// Waiters taking orders on their phones — P19's server and P20's intents.
    MobileOrdering,
    /// More than one till on the same shop (P27). A seam today.
    MultiTerminal,
}

impl Feature {
    /// Every feature, for the tests and for the account screen's limits panel.
    pub const ALL: &'static [Feature] = &[
        Feature::Reports,
        Feature::CloudBackup,
        Feature::MobileOrdering,
        Feature::MultiTerminal,
    ];

    /// **Closing the day is not a report, and this constant is where that
    /// decision is written.**
    ///
    /// The day close reads like a report — it is a total, on a screen, behind
    /// `reports.view`. It is not one. It is how a shop reconciles the cash in
    /// its drawer against what the till says, and a shop locked out of it at
    /// 11 pm has money it cannot account for and no way to open tomorrow
    /// cleanly. Analytics can wait for a payment; the drawer cannot.
    ///
    /// So `day_close`, `count_cash`, `close_day` and `reopen_day` are NOT
    /// behind [`Feature::Reports`], and `src-tauri`'s gate list asserts it.
    pub const REPORTS_DOES_NOT_MEAN_THE_DAY_CLOSE: &'static [&'static str] =
        &["day_close", "count_cash", "close_day", "reopen_day"];

    /// The stable code. This is what a plan carries, so adding a plan in the
    /// admin panel never needs a counter release.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Feature::Reports => "reports",
            Feature::CloudBackup => "cloud-backup",
            Feature::MobileOrdering => "mobile-ordering",
            Feature::MultiTerminal => "multi-terminal",
        }
    }

    /// What a shopkeeper calls it. Used in every refusal, so it has to read
    /// inside a sentence: "Your plan does not include *phone ordering*."
    #[must_use]
    pub const fn in_words(self) -> &'static str {
        match self {
            Feature::Reports => "reports",
            Feature::CloudBackup => "cloud backup",
            Feature::MobileOrdering => "phone ordering",
            Feature::MultiTerminal => "extra tills",
        }
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Feature> {
        Feature::ALL.iter().copied().find(|f| f.code() == code)
    }
}

/// Why the gate said no.
///
/// **Two reasons, and they are not the same conversation.** "Your plan has
/// expired" is a bill to pay; "your plan does not include phone ordering" is a
/// plan to upgrade. v1 showed one message for both, and every shopkeeper who
/// saw it phoned support to ask whether they had been cut off.
///
/// # There is no sentence in here, and that is deliberate
///
/// `src-tauri/src/words.rs` is *"the one place a machine state becomes words"*
/// — crown jewel 14 — and D78 says a number only goes next to a noun through
/// `words::count`. A refusal that carried its own prose would be a second place
/// composing sentences about days and dates, and the two would drift the first
/// time somebody changed one of them.
///
/// So this type carries **facts**: which feature, and what standing refused it.
/// `words::licence_refusal` turns it into the sentence, and D75 then applies at
/// the `src-tauri` boundary — the refusal is returned as a value and becomes
/// the `UiError` message, rather than a storage error `words::from_db` would
/// rewrite into "The shop's data could not be read".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    pub feature: Feature,
    pub why: Why,
}

/// The two conversations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The shop is not entitled to operate: expired, suspended, revoked,
    /// cancelled, never activated, or a trial that ended. The standing carries
    /// which, and the sentence differs for every one of them.
    NotOperating(crate::status::Standing),
    /// The shop is perfectly fine. This plan simply does not have that in it.
    NotInThePlan,
}

impl Refusal {
    /// The stable code, for a support call and for the front end's tests. Never
    /// shown to a shopkeeper.
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

    /// **T11, and it is the most important test in this crate.**
    ///
    /// It is written as a list of names rather than a count, because a count
    /// passes when somebody swaps one variant for another. If this test fails
    /// because you added a feature, add its name. If it fails because you added
    /// `Billing`, read D86 — and then read requirement 3, which says a shop
    /// must be able to trade.
    #[test]
    fn the_gate_cannot_reach_billing() {
        let codes: Vec<&str> = Feature::ALL.iter().map(|f| f.code()).collect();
        assert_eq!(
            codes,
            vec!["reports", "cloud-backup", "mobile-ordering", "multi-terminal"],
            "the list of gateable features changed"
        );
        for banned in ["billing", "bill", "printing", "print", "local-backup", "drawer"] {
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

    /// The two refusals are two different conversations, and they must stay
    /// distinguishable all the way out to the screen.
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

    /// And there is no prose in here at all — crown jewel 14 owns that.
    #[test]
    fn a_refusal_carries_facts_and_not_a_sentence() {
        // **Only the shipped half of the file.** The test module below names
        // the very phrases it is looking for, so scanning itself finds them
        // every time — which is a scan that fails on its own evidence rather
        // than on the code.
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
