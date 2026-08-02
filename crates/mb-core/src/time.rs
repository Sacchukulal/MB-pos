//! Instants, local offsets, and the calendar arithmetic underneath them.
//!
//! # Why there is no timezone crate here
//!
//! **India has one timezone and no daylight saving.** A fixed +05:30 offset is
//! not an approximation for this product — it is exactly correct, for every
//! date, forever. Adding chrono, time or jiff would bring in a timezone
//! database of hundreds of kilobytes that this app cannot use, against a
//! startup and installer budget that D12 makes real (`docs/PERFORMANCE.md`).
//!
//! The calendar arithmetic that remains is about fifteen lines, and it is
//! below.
//!
//! **What would have to change for a second country:** `UtcOffset` becomes a
//! zone identifier, [`Timestamp::to_local_parts`] consults a tz database, and
//! [`crate::businessday::BusinessDay::of`] can then return two different days
//! for the same instant depending on where the shop is. Nothing else in the
//! crate moves. That is the whole cost of this decision.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Something a time value could not represent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimeError {
    #[error("that date or time is outside the range this program can handle")]
    Overflow,
}

type Result<T> = std::result::Result<T, TimeError>;

/// An instant, as milliseconds since the Unix epoch, **in UTC**.
///
/// Stored in UTC and only ever converted for display. v1 also stored UTC — the
/// bug in B1 was not the storage, it was that reports re-derived a *day* from
/// it in one place and from local time in another. That is why the day is a
/// stored value of its own (D5) rather than something computed from this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const EPOCH: Timestamp = Timestamp(0);

    #[must_use]
    pub const fn from_millis(millis: i64) -> Self {
        Timestamp(millis)
    }

    #[must_use]
    pub const fn millis(self) -> i64 {
        self.0
    }

    pub fn add_millis(self, millis: i64) -> Result<Self> {
        self.0.checked_add(millis).map(Timestamp).ok_or(TimeError::Overflow)
    }

    /// Days since 1970-01-01 and the seconds within that day, **in local time**
    /// under `offset`.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "splitting a local instant into a day and the seconds within it"
    )]
    pub fn to_local_parts(self, offset: UtcOffset) -> (i32, u32) {
        let local_millis = self.0 + i64::from(offset.minutes()) * 60_000;
        // Euclidean division so an instant before the epoch still yields a
        // non-negative seconds-within-day rather than a negative one.
        let day = local_millis.div_euclid(86_400_000);
        let within = local_millis.rem_euclid(86_400_000) / 1_000;
        // Both fit their types by construction: `within` is under 86,400 and a
        // day index outside i32 is a date over five million years away.
        (
            i32::try_from(day).unwrap_or(i32::MAX),
            u32::try_from(within).unwrap_or(0),
        )
    }

    /// The inverse of [`Timestamp::to_local_parts`].
    pub fn from_local_parts(days: i32, seconds: u32, offset: UtcOffset) -> Result<Self> {
        let local = i64::from(days)
            .checked_mul(86_400_000)
            .and_then(|d| d.checked_add(i64::from(seconds) * 1_000))
            .ok_or(TimeError::Overflow)?;
        local
            .checked_sub(i64::from(offset.minutes()) * 60_000)
            .map(Timestamp)
            .ok_or(TimeError::Overflow)
    }
}

/// Minutes east of UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UtcOffset(i32);

impl UtcOffset {
    /// +05:30. The only offset this product needs today.
    pub const INDIA: UtcOffset = UtcOffset(330);
    pub const UTC: UtcOffset = UtcOffset(0);

    /// Anything beyond ±14 hours is a data-entry error, not a place.
    #[must_use]
    pub const fn from_minutes(minutes: i32) -> Option<Self> {
        if minutes < -840 || minutes > 840 { None } else { Some(UtcOffset(minutes)) }
    }

    #[must_use]
    pub const fn minutes(self) -> i32 {
        self.0
    }
}

impl Default for UtcOffset {
    fn default() -> Self {
        UtcOffset::INDIA
    }
}

impl fmt::Display for UtcOffset {
    #[allow(clippy::integer_division, reason = "splitting minutes into hours and minutes")]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { '-' } else { '+' };
        let magnitude = self.0.unsigned_abs();
        write!(f, "{sign}{:02}:{:02}", magnitude / 60, magnitude % 60)
    }
}

/// Days since 1970-01-01, from a proleptic Gregorian date.
///
/// Howard Hinnant's `days_from_civil`. It works by shifting the year to start
/// in March, which puts the leap day at the *end* of the year and makes the
/// month-length pattern regular enough to compute without a table or a branch.
#[must_use]
#[allow(
    clippy::integer_division,
    reason = "the algorithm is integer division throughout; that is what makes it exact"
)]
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = i64::from(month);
    let doy = ((153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5) + i64::from(day) - 1;
    let doy = i32::try_from(doy).unwrap_or(0); // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The inverse of [`days_from_civil`].
#[must_use]
#[allow(clippy::integer_division, reason = "see `days_from_civil`")]
pub fn civil_from_days(days: i32) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (
        if m <= 2 { y + 1 } else { y },
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_is_where_it_should_be() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn known_dates_land_on_their_known_days() {
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(2000, 1, 1), 10_957);
        assert_eq!(days_from_civil(2026, 8, 2), 20_667);

        // A leap day in a leap century.
        assert_eq!(civil_from_days(days_from_civil(2000, 2, 29)), (2000, 2, 29));
        // 1900 was NOT a leap year — the case a naive "divisible by 4" gets
        // wrong, and the reason this arithmetic is written out rather than
        // guessed at.
        assert_eq!(days_from_civil(1900, 3, 1) - days_from_civil(1900, 2, 28), 1);
        assert_eq!(days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28), 2);
    }

    #[test]
    fn civil_dates_round_trip_over_a_century_and_a_half() {
        // 1900 through 2100, every day.
        for day in days_from_civil(1900, 1, 1)..days_from_civil(2100, 1, 1) {
            let (y, m, d) = civil_from_days(day);
            assert_eq!(days_from_civil(y, m, d), day, "{y}-{m}-{d} did not round trip");
            assert!((1..=12).contains(&m));
            assert!((1..=31).contains(&d));
        }
    }

    #[test]
    fn an_instant_splits_into_the_local_day_and_time() {
        // 2026-08-02 18:30:00 UTC is 2026-08-03 00:00:00 IST.
        let midnight_ist = Timestamp::from_millis(
            i64::from(days_from_civil(2026, 8, 2)) * 86_400_000 + 18 * 3_600_000 + 30 * 60_000,
        );
        let (day, seconds) = midnight_ist.to_local_parts(UtcOffset::INDIA);
        assert_eq!(civil_from_days(day), (2026, 8, 3));
        assert_eq!(seconds, 0);

        // The same instant read in UTC is still the 2nd, late in the evening.
        let (day, seconds) = midnight_ist.to_local_parts(UtcOffset::UTC);
        assert_eq!(civil_from_days(day), (2026, 8, 2));
        assert_eq!(seconds, 18 * 3_600 + 30 * 60);
    }

    #[test]
    fn local_parts_round_trip() {
        for day in [0_i32, 1, 20_667, -1, -365] {
            for seconds in [0_u32, 1, 3_600, 86_399] {
                let at = Timestamp::from_local_parts(day, seconds, UtcOffset::INDIA)
                    .expect("in range");
                assert_eq!(at.to_local_parts(UtcOffset::INDIA), (day, seconds));
            }
        }
    }

    #[test]
    fn an_offset_beyond_the_world_is_refused() {
        assert!(UtcOffset::from_minutes(841).is_none());
        assert!(UtcOffset::from_minutes(-841).is_none());
        assert_eq!(UtcOffset::from_minutes(330), Some(UtcOffset::INDIA));
        assert_eq!(UtcOffset::INDIA.to_string(), "+05:30");
        assert_eq!(UtcOffset::from_minutes(-330).map(|o| o.to_string()), Some("-05:30".to_owned()));
        assert_eq!(UtcOffset::UTC.to_string(), "+00:00");
        assert_eq!(UtcOffset::default(), UtcOffset::INDIA);
    }

    #[test]
    fn arithmetic_is_checked_not_wrapped() {
        assert_eq!(Timestamp::from_millis(i64::MAX).add_millis(1), Err(TimeError::Overflow));
        assert_eq!(Timestamp::from_millis(i64::MIN).add_millis(-1), Err(TimeError::Overflow));
    }

    #[test]
    fn the_representable_range_is_far_larger_than_any_shop_will_need() {
        // `from_local_parts` returns a Result for the sake of the caller and
        // of a future zone-aware version, but there is no i32 day index that
        // can actually overflow an i64 of milliseconds — the largest is a date
        // roughly five million years away. Asserted rather than assumed, so
        // nobody later "fixes" the Result away on a guess.
        assert!(Timestamp::from_local_parts(i32::MAX, 86_399, UtcOffset::INDIA).is_ok());
        assert!(Timestamp::from_local_parts(i32::MIN, 0, UtcOffset::INDIA).is_ok());
    }
}
