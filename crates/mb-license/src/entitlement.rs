//! **`decide` — the one place this product answers "may this shop work?"**
//!
//! Everything else reads the answer. The counter, and later the cloud and the
//! phone, run this same order of questions, which is the whole of BACKEND-C1's
//! suggested fix: *"one shared idea of 'is this restaurant allowed to work
//! right now', honoured identically by the counter, the cloud and the phone —
//! and it must include status."*

use mb_core::{BusinessDay, Timestamp};
use serde::{Deserialize, Serialize};

use crate::gate::{Feature, Refusal, Why};
use crate::plan::{FeatureSet, Limits};
use crate::status::{Licence, Standing, Status};

/// **The cloud's own last resort, and the counter must never disagree with
/// it.**
///
/// > **BACKEND-C3:** *"The counter has 10 days hard-coded. The cloud uses the
/// > per-licence override, then the global setting, then 10. So if you set a
/// > customer's grace to 30 days in the admin panel, the counter still locks at
/// > 10 while the phones keep working."*
///
/// The bug was never the number. It was that there were two of them, in two
/// programs, and only one of them could be changed. This constant is the third
/// step of [`resolve_grace`] and it appears **exactly once in this crate** —
/// `no_bare_grace_period_anywhere` asserts that, because a 10 that creeps back
/// into a comparison somewhere is precisely how the finding happened the first
/// time. `LICENCE_PROTOCOL.md` states the same three steps as the algorithm the
/// cloud must match.
pub const DEFAULT_GRACE_DAYS: u16 = 10;

/// **The answer to requirement 3, written as a function so it can be grepped
/// for.**
///
/// It takes nothing, it returns `true`, and it is never called in a condition.
/// It exists so that a session searching this codebase for "where do we decide
/// whether billing is allowed" finds one hit, reads the doc comment, and stops
/// looking for the check that does not exist.
///
/// A shop that cannot bill cannot trade. There is no licence state, no clock
/// state and no network state in which this returns anything else, and
/// [`Feature`] has no variant that could make one — D86.
#[must_use]
pub const fn billing_is_always_allowed() -> bool {
    true
}

/// What the licence means today, and what it lets the shop use.
///
/// **Decided once and held**, not computed per call: PERFORMANCE §2.2 says
/// *"nothing in this table may ever be blocked by a report, a sync, a print job,
/// a licence check or a backup"*, and the cheapest way to keep that promise is
/// for the billing path to have nothing to call. `src-tauri` holds one of these
/// behind an `RwLock` and refreshes it on a timer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entitlement {
    pub standing: Standing,
    pub plan_name: String,
    pub limits: Limits,
    pub renews_on: Option<BusinessDay>,
    pub shop_name: Option<String>,
    /// **The last time this counter actually reached the cloud.** `EPOCH` means
    /// never.
    ///
    /// Found by looking at the screen: the first version set this to *now*,
    /// because that is when the decision was made — so a counter that had never
    /// been activated, and had never spoken to anything, told the owner it had
    /// been "checked" four seconds ago. The label on the screen says "last
    /// checked", and an owner reads that as *"when did you last talk to your
    /// server"*, which is the only reading that is any use to them when the
    /// internet is down.
    ///
    /// A decision made from a cache is not a check. This is the check.
    pub last_checked: Timestamp,
    /// When it has to be made again. See D89: this is the EARLIER of the
    /// snapshot's wall-clock expiry and its offline allowance measured from the
    /// clock's high-water mark, so stopping the clock cannot extend it.
    pub good_until: Timestamp,
    features: FeatureSet,
}

impl Entitlement {
    /// What a counter answers with before anybody has typed a key, and what it
    /// falls back to when `licence.json` is missing, corrupt or signed by
    /// somebody else.
    ///
    /// **Note what this is not: it is not a refusal to start.** A first run is
    /// a shop that has just installed the product and wants to print a bill in
    /// the next three minutes (budget S5). It gets the default limits and no
    /// gated features, and it bills.
    #[must_use]
    pub fn unactivated(now: Timestamp) -> Entitlement {
        Entitlement {
            standing: Standing::NeverActivated,
            plan_name: "No plan".to_owned(),
            limits: Limits::default(),
            renews_on: None,
            shop_name: None,
            // **Never**, and not `now`. Deciding "there is no licence here" is
            // not a check — nothing was asked and nothing answered.
            last_checked: Timestamp::EPOCH,
            good_until: now,
            features: FeatureSet::default(),
        }
    }

    /// Build one from a decided standing and the licence it came from.
    #[must_use]
    pub fn from_licence(
        licence: &Licence,
        standing: Standing,
        last_checked: Timestamp,
        good_until: Timestamp,
    ) -> Entitlement {
        Entitlement {
            standing,
            plan_name: licence.plan.name.clone(),
            limits: licence.plan.limits,
            renews_on: Some(licence.renews_on),
            shop_name: Some(licence.shop_name.clone()),
            last_checked,
            good_until,
            features: licence.plan.features.clone(),
        }
    }

    /// **The gate.**
    ///
    /// Two questions in a fixed order, and the order is the finding: is the
    /// shop operating at all, and then does its plan include this. Asking them
    /// the other way round would tell a suspended shop on the top tier that
    /// everything was fine.
    ///
    /// # Errors
    ///
    /// A [`Refusal`] carrying the feature and why. It carries no prose — see
    /// the note on that type.
    pub fn may(&self, feature: Feature) -> Result<(), Refusal> {
        if !self.standing.operating() {
            return Err(Refusal {
                feature,
                why: Why::NotOperating(self.standing),
            });
        }
        if !self.features.includes(feature) {
            return Err(Refusal {
                feature,
                why: Why::NotInThePlan,
            });
        }
        Ok(())
    }

    #[must_use]
    pub const fn operating(&self) -> bool {
        self.standing.operating()
    }

    /// For the account screen's "what you have" list.
    #[must_use]
    pub fn features(&self) -> &FeatureSet {
        &self.features
    }

    /// Has this decision gone stale? The caller re-checks with the cloud when
    /// it has, and **carries on with the old answer if the cloud cannot be
    /// reached** — see `state`, and see requirement 3.
    #[must_use]
    pub const fn is_stale(&self, now: Timestamp) -> bool {
        now.millis() >= self.good_until.millis()
    }
}

/// **D88 — the grace period, resolved in the cloud's own order.**
///
/// The licence's override, then the shop-wide setting, then
/// [`DEFAULT_GRACE_DAYS`]. Both of the first two arrive inside the signed
/// snapshot, so the counter cannot be using a stale global while the cloud uses
/// a fresh one.
#[must_use]
pub fn resolve_grace(licence: &Licence, global: Option<u16>) -> u16 {
    licence
        .grace_days
        .or(global)
        .unwrap_or(DEFAULT_GRACE_DAYS)
}

/// **The decision. Status first, date second.**
///
/// `today` is the shop's business day, which is a stored value everywhere else
/// in this product (D5) and is passed in here rather than derived, for the same
/// reason.
#[must_use]
pub fn decide(licence: &Licence, global_grace: Option<u16>, today: BusinessDay) -> Standing {
    // ---- The first question. BACKEND-C1, and it is one line. ----------------
    //
    // v1 never asked it. The admin panel's Suspend button set a column that
    // nothing read, so a suspended restaurant kept billing, kept syncing and
    // kept taking phone orders until its billing date happened to pass.
    if !licence.status.lets_the_shop_work() {
        return match licence.status {
            Status::Suspended => Standing::Suspended,
            Status::Revoked => Standing::Revoked,
            // The shop chose this. The copy must not scold them for it.
            Status::Cancelled => Standing::Cancelled,
            // Unreachable by `lets_the_shop_work`, and written as a value
            // rather than an `unreachable!()` because the workspace denies
            // `panic` and because a wrong answer here must still be a working
            // shop rather than a crashed one.
            Status::Active | Status::Trial => Standing::Fine,
        };
    }

    // ---- A trial is its own end date, and it is not a billing date. ---------
    //
    // Requirement 4: a trial converts to paid **without reactivating**. That
    // falls out of this shape — the cloud sets `status` to Active and sends a
    // new snapshot, and nothing on the counter has to be re-typed.
    if licence.status == Status::Trial {
        if let Some(ends) = licence.trial_ends_on
            && today.days_until(ends) < 0
        {
            return Standing::TrialEnded;
        }
        return Standing::Fine;
    }

    // ---- The second question: the date, and the grace that follows it. ------
    let days_past_renewal = licence.renews_on.days_until(today);
    if days_past_renewal <= 0 {
        return Standing::Fine;
    }

    let grace = i32::from(resolve_grace(licence, global_grace));
    if days_past_renewal <= grace {
        // Saturating and then narrowed: `grace` is a u16 and
        // `days_past_renewal` is positive and no larger, so the difference fits
        // — but the conversion is written out rather than cast, because the
        // workspace denies `cast_possible_truncation` and a licence date is
        // exactly the kind of field that arrives wrong one day.
        let left = grace.saturating_sub(days_past_renewal).saturating_add(1);
        return Standing::InGrace {
            days_left: u16::try_from(left).unwrap_or(u16::MAX),
        };
    }

    Standing::Expired
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::MachineId;
    use crate::plan::Plan;

    fn day(year: i32, month: u32, d: u32) -> BusinessDay {
        BusinessDay::from_ymd(year, month, d)
    }

    fn a_licence(status: Status, renews_on: BusinessDay) -> Licence {
        Licence {
            key: "MB-TEST-0001".to_owned(),
            shop_name: "Anna's Kitchen".to_owned(),
            plan: Plan::trial(),
            status,
            renews_on,
            grace_days: None,
            bound_to: Some(MachineId::for_tests("machine-a")),
            trial_ends_on: None,
            registered_contact: "+91 98••••••10".to_owned(),
        }
    }

    /// **T2 — BACKEND-C1 BY NAME, and this is the test that finding did not
    /// have.**
    ///
    /// A suspended licence whose billing date is a year away is not entitled.
    /// In v1 it was, because nothing read the status column at all.
    #[test]
    fn a_suspended_licence_with_a_future_billing_date_is_not_entitled() {
        let today = day(2026, 8, 10);
        let a_year_away = day(2027, 8, 10);

        for (status, expected) in [
            (Status::Suspended, Standing::Suspended),
            (Status::Revoked, Standing::Revoked),
            (Status::Cancelled, Standing::Cancelled),
        ] {
            let licence = a_licence(status, a_year_away);
            let standing = decide(&licence, None, today);
            assert_eq!(standing, expected, "{status:?}");
            assert!(!standing.operating(), "{status:?} kept working");
        }

        // And the control: the same date, active, is fine.
        assert_eq!(
            decide(&a_licence(Status::Active, a_year_away), None, today),
            Standing::Fine
        );
    }

    /// **And the shop still bills.** The gate cannot reach it — there is no
    /// `Feature` to ask about — so this asserts the property that makes that
    /// true rather than driving a bill (which `src-tauri`'s T1 does).
    #[test]
    fn a_suspended_shop_can_still_bill() {
        assert!(billing_is_always_allowed());
        let entitlement = Entitlement::from_licence(
            &a_licence(Status::Suspended, day(2027, 1, 1)),
            Standing::Suspended,
            Timestamp::EPOCH,
            Timestamp::EPOCH,
        );
        // Everything the gate CAN be asked is refused...
        for feature in Feature::ALL {
            assert!(entitlement.may(*feature).is_err(), "{feature:?}");
        }
        // ...and billing is not one of the things it can be asked.
        assert_eq!(Feature::ALL.len(), 4);
    }

    /// **T3 — BACKEND-C3, all three steps.**
    #[test]
    fn the_grace_period_comes_from_the_licence_then_the_setting_then_the_default() {
        let renews = day(2026, 8, 1);
        let mut licence = a_licence(Status::Active, renews);

        // Step 3: nobody has set anything.
        assert_eq!(resolve_grace(&licence, None), DEFAULT_GRACE_DAYS);

        // Step 2: the shop-wide setting, which in v1 the counter ignored.
        assert_eq!(resolve_grace(&licence, Some(30)), 30);

        // Step 1: this customer's own override wins over both.
        licence.grace_days = Some(45);
        assert_eq!(resolve_grace(&licence, Some(30)), 45);
        assert_eq!(resolve_grace(&licence, None), 45);
    }

    /// The finding's actual scenario: grace set to 30 in the admin panel, and
    /// the counter locking at 10 anyway while the phones kept working.
    #[test]
    fn a_thirty_day_grace_reaches_the_counter() {
        let renews = day(2026, 8, 1);
        let licence = a_licence(Status::Active, renews);
        let twenty_days_later = day(2026, 8, 21);

        // With the default, twenty days past renewal is expired.
        assert_eq!(decide(&licence, None, twenty_days_later), Standing::Expired);

        // With the shop's 30, it is in grace — and the counter agrees with the
        // cloud, which is the entire content of C3.
        assert_eq!(
            decide(&licence, Some(30), twenty_days_later),
            Standing::InGrace { days_left: 11 }
        );
    }

    #[test]
    fn the_grace_boundary_is_inclusive_and_the_day_after_is_not() {
        let licence = a_licence(Status::Active, day(2026, 8, 1));
        // Ten days of grace: the tenth day still works, with one day left.
        assert_eq!(
            decide(&licence, None, day(2026, 8, 11)),
            Standing::InGrace { days_left: 1 }
        );
        assert_eq!(decide(&licence, None, day(2026, 8, 12)), Standing::Expired);
        // The renewal day itself is not past anything.
        assert_eq!(decide(&licence, None, day(2026, 8, 1)), Standing::Fine);
    }

    #[test]
    fn a_trial_ends_on_its_own_date_and_not_on_a_billing_date() {
        let mut licence = a_licence(Status::Trial, day(2027, 1, 1));
        licence.trial_ends_on = Some(day(2026, 8, 9));
        assert_eq!(decide(&licence, None, day(2026, 8, 9)), Standing::Fine);
        assert_eq!(decide(&licence, None, day(2026, 8, 10)), Standing::TrialEnded);
    }

    /// Requirement 4: converting a trial to paid re-types nothing.
    #[test]
    fn a_trial_becomes_paid_without_being_reactivated() {
        let mut licence = a_licence(Status::Trial, day(2026, 9, 12));
        licence.trial_ends_on = Some(day(2026, 8, 9));
        assert_eq!(decide(&licence, None, day(2026, 8, 20)), Standing::TrialEnded);

        // The cloud flips the status and sends a new snapshot. Nothing on the
        // counter is re-entered, and the machine binding is untouched.
        let bound_before = licence.bound_to.clone();
        licence.status = Status::Active;
        assert_eq!(decide(&licence, None, day(2026, 8, 20)), Standing::Fine);
        assert_eq!(licence.bound_to, bound_before);
    }

    #[test]
    fn a_plan_that_does_not_include_a_feature_refuses_it_even_when_fine() {
        let mut licence = a_licence(Status::Active, day(2027, 1, 1));
        licence.plan.features = FeatureSet::from_codes(["reports"]);
        let entitlement = Entitlement::from_licence(
            &licence,
            Standing::Fine,
            Timestamp::EPOCH,
            Timestamp::EPOCH,
        );
        assert!(entitlement.may(Feature::Reports).is_ok());
        let refusal = entitlement
            .may(Feature::MobileOrdering)
            .expect_err("it was allowed");
        assert_eq!(refusal.why, Why::NotInThePlan);
        assert_eq!(refusal.code(), "licence.not_in_plan");
    }

    /// The order of the two questions matters: a suspended shop on the top tier
    /// must hear "suspended", not "not in your plan".
    #[test]
    fn standing_is_asked_before_the_plan() {
        let licence = a_licence(Status::Suspended, day(2027, 1, 1));
        let entitlement = Entitlement::from_licence(
            &licence,
            Standing::Suspended,
            Timestamp::EPOCH,
            Timestamp::EPOCH,
        );
        let refusal = entitlement
            .may(Feature::MobileOrdering)
            .expect_err("it was allowed");
        assert_eq!(refusal.why, Why::NotOperating(Standing::Suspended));
    }

    /// **Found by looking at the screen.**
    ///
    /// The account screen's label is "last checked", and the first version put
    /// `now` in here — so a counter that had never been activated, and had
    /// never spoken to anything, told its owner it had been checked four
    /// seconds ago. Deciding "there is no licence here" is not a check.
    #[test]
    fn a_counter_that_has_never_been_activated_has_never_been_checked() {
        let entitlement = Entitlement::unactivated(Timestamp::from_millis(1_700_000_000_000));
        assert_eq!(
            entitlement.last_checked,
            Timestamp::EPOCH,
            "an unactivated counter claimed to have checked its licence"
        );
    }

    #[test]
    fn an_unactivated_counter_refuses_every_feature_and_still_has_limits() {
        let entitlement = Entitlement::unactivated(Timestamp::EPOCH);
        for feature in Feature::ALL {
            assert!(entitlement.may(*feature).is_err(), "{feature:?}");
        }
        assert_eq!(entitlement.limits, Limits::default());
        assert!(!entitlement.operating());
    }

    /// **T3's fourth part.** The number 10 lives in one constant. A comparison
    /// somewhere with a bare 10 in it is how C3 happened, and it would pass
    /// every other test in this file.
    #[test]
    fn no_bare_grace_period_anywhere() {
        for (name, source) in [
            ("entitlement.rs", include_str!("entitlement.rs")),
            ("status.rs", include_str!("status.rs")),
            ("snapshot.rs", include_str!("snapshot.rs")),
            ("state.rs", include_str!("state.rs")),
        ] {
            for line in source.lines() {
                let code = line.split("//").next().unwrap_or("");
                if !code.to_lowercase().contains("grace") {
                    continue;
                }
                // The one place the number is allowed to appear.
                if code.contains("pub const DEFAULT_GRACE_DAYS") {
                    continue;
                }
                assert!(
                    !code.contains("10"),
                    "{name} has a grace period with a number in it, which is \
                     BACKEND-C3: {code}"
                );
            }
        }
    }
}
