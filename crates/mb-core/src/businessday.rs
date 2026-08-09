//! The business day — decision D5.
//!
//! **A restaurant's day is not a calendar day.** A shop that closes at 1 am
//! thinks of that last bill as belonging to the evening it came from, and so
//! does its cash drawer, its staff shift and its owner.
//!
//! v1 had no such concept, and the audit's B1 is what happened:
//!
//! > "Bills are stored in international (UTC) time. The date-range filter
//! > correctly converts to local time, but the Day-wise Sales report and the
//! > Dashboard's daily chart group by the UTC date. For a restaurant open past
//! > 11:30 pm, a bill made at 12:15 am on Sunday is counted under Saturday in
//! > one place and Sunday in another. **Your totals will not tie.**"
//!
//! The fix is not better conversion. It is to compute the day **once**, when
//! the order is created, and store it — so every report on the counter, the
//! phone and the cloud groups by the same one value and cannot disagree.

use crate::time::{Timestamp, UtcOffset, civil_from_days, days_from_civil};
use serde::{Deserialize, Serialize};
use std::fmt;

/// When a shop's day begins, in local minutes past midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DayRule {
    starts_at_minutes: u16,
}

impl DayRule {
    /// 05:00 — late enough that a shop closing at 1 am still books the night's
    /// takings against the evening it worked.
    pub const DEFAULT: DayRule = DayRule { starts_at_minutes: 300 };

    /// `None` at or beyond midnight-plus-a-day; a day cannot start tomorrow.
    #[must_use]
    pub const fn new(minutes_past_midnight: u16) -> Option<Self> {
        if minutes_past_midnight >= 1_440 {
            None
        } else {
            Some(DayRule { starts_at_minutes: minutes_past_midnight })
        }
    }

    #[must_use]
    pub const fn starts_at_minutes(self) -> u16 {
        self.starts_at_minutes
    }
}

impl Default for DayRule {
    fn default() -> Self {
        DayRule::DEFAULT
    }
}

/// One trading day, as a count of days since 1970-01-01.
///
/// An integer rather than a formatted string so it is `Ord`, cheap to compare,
/// cheap to index in a database, and impossible to parse wrongly. It prints as
/// `2026-08-02` when a human needs to read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BusinessDay(i32);

impl BusinessDay {
    /// Which business day an instant falls in.
    ///
    /// **This is the only place in the entire system where a day may be
    /// derived from a timestamp** (decision D5). It is called once, when the
    /// order is created, and the answer is stored on the order. Every report,
    /// every screen, every sync reads that stored value.
    ///
    /// If you are about to call this with an order's `created_at` in order to
    /// find out which day it belongs to — don't. Read `order.business_day`.
    /// That is exactly the re-derivation that made v1's totals disagree.
    #[must_use]
    pub fn of(at: Timestamp, rule: DayRule, offset: UtcOffset) -> Self {
        let (day, seconds) = at.to_local_parts(offset);
        let starts_at = u32::from(rule.starts_at_minutes()) * 60;
        // Before the shop's day began, this instant still belongs to yesterday.
        BusinessDay(if seconds < starts_at { day - 1 } else { day })
    }

    #[must_use]
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Self {
        BusinessDay(days_from_civil(year, month, day))
    }

    #[must_use]
    pub const fn from_days_since_epoch(days: i32) -> Self {
        BusinessDay(days)
    }

    #[must_use]
    pub const fn days_since_epoch(self) -> i32 {
        self.0
    }

    #[must_use]
    pub fn to_ymd(self) -> (i32, u32, u32) {
        civil_from_days(self.0)
    }

    #[must_use]
    pub const fn next(self) -> Self {
        BusinessDay(self.0.saturating_add(1))
    }

    #[must_use]
    pub const fn previous(self) -> Self {
        BusinessDay(self.0.saturating_sub(1))
    }

    /// `other − self`, in days.
    #[must_use]
    pub const fn days_until(self, other: Self) -> i32 {
        other.0.saturating_sub(self.0)
    }

    /// The instants this business day covers, as a **half-open** range
    /// `[start, end)`.
    ///
    /// Half-open on purpose: a closed range counts the bill that lands exactly
    /// on the boundary twice, once in each day. That is B1's bug wearing a
    /// different hat, and it is just as hard to spot in a total.
    pub fn range(
        self,
        rule: DayRule,
        offset: UtcOffset,
    ) -> Result<(Timestamp, Timestamp), crate::time::TimeError> {
        let seconds = u32::from(rule.starts_at_minutes()) * 60;
        let start = Timestamp::from_local_parts(self.0, seconds, offset)?;
        let end = Timestamp::from_local_parts(self.0 + 1, seconds, offset)?;
        Ok((start, end))
    }
}

/// **`YYYY-MM-DD` back into a day**, so the screen never does date arithmetic.
///
/// P18: a report's period comes from two `<input type="date">` boxes, and those
/// produce exactly this format in every browser and every locale. The
/// alternative is TypeScript computing days-since-epoch — which is arithmetic
/// on a value the whole reporting layer is keyed by, in the one language this
/// product does not let do arithmetic (R8, §6).
///
/// Round-trips with [`fmt::Display`], and a test says so.
impl std::str::FromStr for BusinessDay {
    type Err = crate::time::TimeError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.split('-');
        let year: i32 = parts.next().ok_or(crate::time::TimeError::Overflow)?
            .parse().map_err(|_| crate::time::TimeError::Overflow)?;
        let month: u32 = parts.next().ok_or(crate::time::TimeError::Overflow)?
            .parse().map_err(|_| crate::time::TimeError::Overflow)?;
        let day: u32 = parts.next().ok_or(crate::time::TimeError::Overflow)?
            .parse().map_err(|_| crate::time::TimeError::Overflow)?;
        if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(crate::time::TimeError::Overflow);
        }
        Ok(BusinessDay::from_ymd(year, month, day))
    }
}

impl fmt::Display for BusinessDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = self.to_ymd();
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The period picker's format, both ways. P18.
    #[test]
    fn a_date_box_round_trips_through_a_business_day() {
        for day in [
            BusinessDay::from_ymd(2026, 8, 9),
            BusinessDay::from_ymd(2024, 2, 29), // a leap day
            BusinessDay::from_ymd(1970, 1, 1),
            BusinessDay::from_ymd(2099, 12, 31),
        ] {
            let text = day.to_string();
            assert_eq!(text.parse::<BusinessDay>().expect("it parses"), day, "{text}");
        }
        // And nonsense is refused rather than silently becoming some day.
        for bad in ["", "2026", "2026-08", "2026-08-09-01", "2026-13-01", "2026-08-00", "x-y-z"] {
            assert!(bad.parse::<BusinessDay>().is_err(), "{bad} was accepted");
        }
    }

    /// An instant from a local wall-clock time in India.
    fn ist(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Timestamp {
        Timestamp::from_local_parts(
            days_from_civil(year, month, day),
            hour * 3_600 + minute * 60,
            UtcOffset::INDIA,
        )
        .expect("in range")
    }

    #[test]
    fn the_quarter_past_midnight_bill_belongs_to_last_night() {
        // Audit B1, exactly: a bill at 00:15 on Sunday is Saturday's takings.
        let at = ist(2026, 8, 2, 0, 15);
        let day = BusinessDay::of(at, DayRule::DEFAULT, UtcOffset::INDIA);
        assert_eq!(day, BusinessDay::from_ymd(2026, 8, 1));
        assert_eq!(day.to_string(), "2026-08-01");
    }

    #[test]
    fn the_day_boundary_is_pinned_to_the_minute() {
        let day_of = |hour, minute| {
            BusinessDay::of(ist(2026, 8, 2, hour, minute), DayRule::DEFAULT, UtcOffset::INDIA)
        };
        let yesterday = BusinessDay::from_ymd(2026, 8, 1);
        let today = BusinessDay::from_ymd(2026, 8, 2);

        assert_eq!(day_of(0, 0), yesterday, "midnight is still last night");
        assert_eq!(day_of(4, 59), yesterday);
        assert_eq!(day_of(5, 0), today, "the day starts AT 05:00, not after it");
        assert_eq!(day_of(5, 1), today);
        assert_eq!(day_of(23, 59), today);
    }

    #[test]
    fn the_two_answers_v1_gave_are_reproducible_and_the_stored_day_is_not() {
        // Audit B1, reconstructed. A bill at 00:15 IST on Sunday the 2nd.
        let at = ist(2026, 8, 2, 0, 15);

        // v1 had no business day. Its range filter worked in local time, so it
        // called this the 2nd...
        let calendar = DayRule::new(0).expect("valid");
        let as_the_filter_saw_it = BusinessDay::of(at, calendar, UtcOffset::INDIA);
        // ...while the day-wise report grouped by the UTC date, which is still
        // the 1st. Two screens, two answers, one bill.
        let as_the_report_saw_it = BusinessDay::of(at, calendar, UtcOffset::UTC);

        assert_eq!(as_the_filter_saw_it.to_string(), "2026-08-02");
        assert_eq!(as_the_report_saw_it.to_string(), "2026-08-01");
        assert_ne!(
            as_the_filter_saw_it, as_the_report_saw_it,
            "if these ever agree, this test has stopped reproducing the bug"
        );

        // D5's answer: compute it ONCE, with the shop's own rule and offset,
        // and store that. Every reader then reads the same value instead of
        // deriving its own, so there is no second answer to disagree with.
        let stored = BusinessDay::of(at, DayRule::DEFAULT, UtcOffset::INDIA);
        assert_eq!(stored.to_string(), "2026-08-01", "the night it was earned");
    }

    #[test]
    fn a_shop_can_choose_when_its_day_starts() {
        let midnight_rule = DayRule::new(0).expect("valid");
        assert_eq!(
            BusinessDay::of(ist(2026, 8, 2, 0, 15), midnight_rule, UtcOffset::INDIA),
            BusinessDay::from_ymd(2026, 8, 2),
            "with a midnight rule the calendar date is the business day"
        );

        // A bar that closes at 4 am and starts its day at 06:00.
        let late_rule = DayRule::new(360).expect("valid");
        assert_eq!(
            BusinessDay::of(ist(2026, 8, 2, 3, 30), late_rule, UtcOffset::INDIA),
            BusinessDay::from_ymd(2026, 8, 1)
        );

        assert!(DayRule::new(1_440).is_none(), "a day cannot start tomorrow");
        assert!(DayRule::new(1_439).is_some());
    }

    #[test]
    fn the_range_is_half_open_so_the_boundary_bill_is_counted_once() {
        let day = BusinessDay::from_ymd(2026, 8, 1);
        let (start, end) = day.range(DayRule::DEFAULT, UtcOffset::INDIA).expect("in range");

        assert_eq!(start, ist(2026, 8, 1, 5, 0));
        assert_eq!(end, ist(2026, 8, 2, 5, 0));

        // The instant at `end` belongs to the NEXT day, not to this one.
        assert_eq!(
            BusinessDay::of(end, DayRule::DEFAULT, UtcOffset::INDIA),
            day.next()
        );
        // And `start` belongs to this one.
        assert_eq!(BusinessDay::of(start, DayRule::DEFAULT, UtcOffset::INDIA), day);

        // The next day's range begins exactly where this one ends — no gap and
        // no overlap.
        let (next_start, _) = day.next().range(DayRule::DEFAULT, UtcOffset::INDIA).expect("in range");
        assert_eq!(next_start, end);
    }

    #[test]
    fn days_walk_forwards_and_backwards() {
        let day = BusinessDay::from_ymd(2026, 12, 31);
        assert_eq!(day.next(), BusinessDay::from_ymd(2027, 1, 1));
        assert_eq!(day.previous(), BusinessDay::from_ymd(2026, 12, 30));
        assert_eq!(day.days_until(day.next().next()), 2);
        assert_eq!(day.next().days_until(day), -1);
        assert!(day.previous() < day && day < day.next(), "days must sort");
    }

    #[test]
    fn a_business_day_serialises_as_a_plain_number() {
        // P04 stores it as an integer column and P08 hands it to TypeScript.
        let day = BusinessDay::from_ymd(2026, 8, 2);
        let json = serde_json::to_string(&day).expect("serialises");
        assert_eq!(json, "20667");
        assert_eq!(serde_json::from_str::<BusinessDay>(&json).expect("reads"), day);
    }
}
