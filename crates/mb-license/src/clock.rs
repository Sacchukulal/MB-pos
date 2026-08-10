//! **D90 — a clock that went backwards warns; it does not lock.**
//!
//! Requirement 6: *"a clock moved backwards is detected, and the response is a
//! warning plus a required online check, NOT an instant lock. A shop whose CMOS
//! battery died must not lose its evening."*
//!
//! # The two jobs this does, and they are opposites
//!
//! 1. **Stop a stopped clock buying free software.** An offline entitlement has
//!    to expire, and the only thing it could expire against is the clock — which
//!    is the one number the person being gated controls. So [`Watch::high_water`]
//!    only ever moves **forward**, and D89's offline allowance is measured from
//!    it rather than from `now`. Setting the PC to March buys nothing: the high
//!    water mark is still in August. That is T8.
//!
//! 2. **Not punish a dead CMOS battery.** Which is the far more common cause, by
//!    a wide margin, and it happens to the oldest machines in the fleet — the
//!    ones running a restaurant on a Friday night. The response to a rollback is
//!    a banner and an online check at the next opportunity. It is never a stop,
//!    and it can never be, because [`crate::Feature`] cannot name billing.
//!
//! The two jobs pull in opposite directions and both are satisfied by the same
//! rule: **the high-water mark decides expiry; the wall clock decides nothing.**

use std::time::Duration;

use mb_core::Timestamp;
use serde::{Deserialize, Serialize};

/// NTP nudges a clock by a few seconds routinely, and a shop PC that has been
/// off all night can come back a minute out before it syncs. Five minutes is
/// wide enough that neither is an incident and narrow enough that a real
/// rollback — which is measured in days — is never inside it.
pub const SKEW: Duration = Duration::from_secs(5 * 60);

/// The furthest forward this counter has ever seen the clock, and the last time
/// it actually reached the cloud.
///
/// Persisted in `licence.json`. Losing the file loses the mark, which is a
/// deliberate non-problem: losing the file also loses the snapshot the mark
/// exists to expire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Watch {
    /// **Monotonic by construction.** [`Watch::saw`] is the only way it moves
    /// and it never moves down.
    pub high_water: Timestamp,
    /// The last successful cloud check. Distinct from `high_water`: one is
    /// "the latest moment we are sure has happened", the other is "when we last
    /// had the truth".
    pub last_online: Timestamp,
}

/// What the clock looks like against the mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockSays {
    Fine,
    /// The wall clock is behind the high-water mark by more than [`SKEW`].
    WentBackwards { by: Duration },
}

impl ClockSays {
    /// A rollback means the next check has to be a real one — a cached answer
    /// cannot be trusted to have been fetched when it says it was.
    #[must_use]
    pub const fn needs_an_online_check(self) -> bool {
        matches!(self, ClockSays::WentBackwards { .. })
    }
}

impl Watch {
    /// Record that this instant has happened. **Only moves forward.**
    pub fn saw(&mut self, now: Timestamp) {
        if now > self.high_water {
            self.high_water = now;
        }
    }

    /// Record a successful conversation with the cloud.
    pub fn reached_the_cloud(&mut self, now: Timestamp) {
        self.saw(now);
        self.last_online = now;
    }

    /// Has the clock gone backwards?
    #[must_use]
    pub fn check(&self, now: Timestamp) -> ClockSays {
        let behind = self.high_water.millis().saturating_sub(now.millis());
        // `SKEW` is minutes, so this conversion cannot lose anything a clock
        // cares about; it is written out because the workspace denies
        // `cast_possible_truncation`.
        let skew_millis = i64::try_from(SKEW.as_millis()).unwrap_or(i64::MAX);
        if behind > skew_millis {
            return ClockSays::WentBackwards {
                by: Duration::from_millis(u64::try_from(behind).unwrap_or(u64::MAX)),
            };
        }
        ClockSays::Fine
    }

    /// **When an offline snapshot runs out** — D89's second expiry, and the
    /// whole of T8.
    ///
    /// Measured from [`Watch::last_online`], which is fixed at the last time we
    /// actually had the truth; **judged against [`Watch::high_water`]**, which
    /// only moves forward. That pairing is the load-bearing part, and the first
    /// version of this function got it wrong in a way every other test passed:
    /// measuring from the high-water mark meant the allowance slid forward one
    /// day for every day the shop stayed offline, so a counter that never
    /// reached the cloud again was entitled forever.
    ///
    /// Neither number alone is enough:
    ///
    /// * from `last_online` against the **wall clock** — freeze the clock and
    ///   the fortnight never passes.
    /// * from `high_water` against anything — the deadline runs away from you.
    /// * from `last_online` against `high_water` — the deadline is fixed the
    ///   moment the cloud was last reached, and the only thing that can reach
    ///   it moves in one direction.
    #[must_use]
    pub fn offline_deadline(&self, days: u16) -> Timestamp {
        let millis = i64::from(days).saturating_mul(86_400_000);
        Timestamp::from_millis(self.last_online.millis().saturating_add(millis))
    }

    /// **The latest moment this counter has any reason to believe has
    /// happened.**
    ///
    /// The high-water mark, or the wall clock if it is ahead of it — which it
    /// is on a freshly started process, before anything has ticked. Expiry is
    /// judged against this and never against `now` alone.
    #[must_use]
    pub fn as_late_as(&self, now: Timestamp) -> Timestamp {
        if now > self.high_water {
            now
        } else {
            self.high_water
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(millis: i64) -> Timestamp {
        Timestamp::from_millis(millis)
    }

    const DAY: i64 = 86_400_000;

    #[test]
    fn the_mark_only_moves_forward() {
        let mut watch = Watch::default();
        watch.saw(at(1_000));
        watch.saw(at(5_000));
        assert_eq!(watch.high_water, at(5_000));
        watch.saw(at(2_000));
        assert_eq!(watch.high_water, at(5_000), "the mark went backwards");
    }

    /// **T7's first half.** A year back is detected.
    #[test]
    fn a_clock_rolled_back_a_year_is_detected() {
        let mut watch = Watch::default();
        watch.saw(at(400 * DAY));
        let says = watch.check(at(35 * DAY));
        match says {
            ClockSays::WentBackwards { by } => {
                assert!(by > Duration::from_secs(300 * 86_400));
            }
            ClockSays::Fine => panic!("a year backwards was not noticed"),
        }
        assert!(says.needs_an_online_check());
    }

    /// **T7's second half, and it is the half that matters more.** An NTP nudge
    /// is not an incident. A shop PC that comes back a minute out after being
    /// off all night must not produce a banner.
    #[test]
    fn a_small_nudge_is_not_an_event() {
        let mut watch = Watch::default();
        watch.saw(at(100 * DAY));
        for behind_seconds in [1_i64, 30, 60, 4 * 60] {
            assert_eq!(
                watch.check(at(100 * DAY - behind_seconds * 1_000)),
                ClockSays::Fine,
                "{behind_seconds}s behind was called a rollback"
            );
        }
        // And just past the skew, it is.
        assert!(
            watch
                .check(at(100 * DAY - 6 * 60 * 1_000))
                .needs_an_online_check()
        );
    }

    /// **T8. Stopping the clock does not buy a single extra day.**
    #[test]
    fn a_stopped_clock_cannot_extend_an_offline_snapshot() {
        let mut watch = Watch::default();
        watch.reached_the_cloud(at(100 * DAY));

        // Fourteen days of offline allowance, fixed at the last time we had the
        // truth. It does not move again until we have it again.
        assert_eq!(watch.offline_deadline(14), at(114 * DAY));

        // The shop runs for thirteen days offline. The DEADLINE does not move.
        watch.saw(at(113 * DAY));
        assert_eq!(
            watch.offline_deadline(14),
            at(114 * DAY),
            "the allowance slid forward with the clock"
        );
        assert!(watch.as_late_as(at(113 * DAY)) < watch.offline_deadline(14));

        // Day fifteen: past it.
        watch.saw(at(115 * DAY));
        assert!(watch.as_late_as(at(115 * DAY)) > watch.offline_deadline(14));

        // Now somebody sets the clock back to day 100 to get another fortnight.
        assert!(
            watch.as_late_as(at(100 * DAY)) > watch.offline_deadline(14),
            "winding the clock back brought an expired snapshot back to life"
        );
        // And it is visible, rather than being silently absorbed.
        assert!(watch.check(at(100 * DAY)).needs_an_online_check());
    }

    /// **The bug the first version of this file had, kept as its own test.**
    ///
    /// Measuring the allowance from the high-water mark made it slide forward a
    /// day for every day the counter stayed offline — so a till that never
    /// reached the cloud again was entitled forever, which is the exact
    /// opposite of what the allowance is for. Every other test in the crate
    /// passed.
    #[test]
    fn the_allowance_does_not_slide_forward_while_the_shop_is_offline() {
        let mut watch = Watch::default();
        watch.reached_the_cloud(at(100 * DAY));
        let deadline = watch.offline_deadline(14);
        for day in 101..200 {
            watch.saw(at(day * DAY));
            assert_eq!(watch.offline_deadline(14), deadline, "day {day}");
        }
        assert!(watch.as_late_as(at(199 * DAY)) > deadline);
    }

    #[test]
    fn reaching_the_cloud_moves_both_numbers() {
        let mut watch = Watch::default();
        watch.reached_the_cloud(at(9 * DAY));
        assert_eq!(watch.last_online, at(9 * DAY));
        assert_eq!(watch.high_water, at(9 * DAY));
        // A later local tick moves the mark but not "when we last had truth".
        watch.saw(at(11 * DAY));
        assert_eq!(watch.last_online, at(9 * DAY));
        assert_eq!(watch.high_water, at(11 * DAY));
    }

    /// A brand-new counter has a zeroed mark, and that must be Fine rather than
    /// "the clock went forward by fifty-six years".
    #[test]
    fn a_fresh_watch_is_fine() {
        assert_eq!(Watch::default().check(at(1_800 * DAY)), ClockSays::Fine);
    }
}
