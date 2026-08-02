//! Token and bill numbers — decision D6.
//!
//! Two v1 bugs live here, and both are fixed by the shape of the API rather
//! than by remembering to be careful.
//!
//! **B3 — the reset that never ran.**
//!
//! > "The daily token/bill reset only happens when the app starts. Your counter
//! > PC is set to never sleep and stays on for days. The reset check runs only
//! > when the app opens or when you leave a settings page — so if the app is
//! > never closed, the token number never resets at midnight."
//!
//! So [`Counter::claim`] evaluates the reset **every single time**, against the
//! business day it is given. There is no separate "roll over" call that
//! something has to remember to make.
//!
//! **B4 — the number handed out twice.**
//!
//! > "The app reads the current number, then increases it in a separate
//! > command. A phone order arriving at the exact moment the cashier presses
//! > Complete Bill could get the same number."
//!
//! So there is **no way to read the next number without consuming it**. No
//! `peek`, no `next_value`, no `current`. That read-then-increment pair *is*
//! the bug: it opens a window that two callers can both walk through.

use crate::businessday::BusinessDay;
use serde::{Deserialize, Serialize};

/// A number that has been handed out and can never be handed out again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claimed {
    pub value: u64,
    /// The number as it will be printed — `"BIR/0042"`.
    ///
    /// Stored rather than formatted on demand, and that is deliberate: a bill
    /// number that has been **printed** must never change because someone
    /// edited the prefix setting six months later. The value is the identity;
    /// this string is the historical fact.
    pub formatted: String,
    /// The day this number belongs to — the day the ORDER belongs to, which is
    /// not always today (D5).
    pub business_day: BusinessDay,
}

/// One number series: tokens, or bills.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counter {
    current: u64,
    start: u64,
    reset_daily: bool,
    prefix: String,
    pad_width: u8,
    last_reset_day: Option<BusinessDay>,
}

impl Counter {
    /// A counter that restarts from `start` each business day.
    #[must_use]
    pub fn daily(start: u64) -> Self {
        Counter {
            current: start,
            start,
            reset_daily: true,
            prefix: String::new(),
            pad_width: 0,
            last_reset_day: None,
        }
    }

    /// A counter that runs on forever — what a bill number usually is, because
    /// a shop's invoice series is expected to be continuous.
    #[must_use]
    pub fn continuous(start: u64) -> Self {
        Counter { reset_daily: false, ..Counter::daily(start) }
    }

    /// Take the next number.
    ///
    /// The daily reset is evaluated FIRST, against `today`, and then the
    /// increment happens — in one call, so nothing can slip between them.
    pub fn claim(&mut self, today: BusinessDay) -> Claimed {
        // B3: checked here, on every claim, rather than once when the app
        // opened on a PC that has not been restarted since March.
        if self.reset_daily && self.last_reset_day != Some(today) {
            self.current = self.start;
            self.last_reset_day = Some(today);
        }

        let value = self.current;
        // A counter at u64::MAX has been running since before the Big Bang;
        // saturating here rather than wrapping keeps the series monotonic even
        // in that impossible case, and a repeated number is caught by P04's
        // uniqueness constraint rather than silently reused.
        self.current = self.current.saturating_add(1);

        Claimed {
            value,
            formatted: self.format(value),
            business_day: today,
        }
    }

    /// What was already handed out, or `None` if nothing has been yet.
    ///
    /// Named for the past on purpose. It cannot be mistaken for "the number I
    /// am about to use", which is the mistake B4 was made of.
    #[must_use]
    pub fn last_issued(&self) -> Option<u64> {
        if self.current > self.start || self.last_reset_day.is_some() {
            self.current.checked_sub(1).filter(|v| *v >= self.start)
        } else {
            None
        }
    }

    /// The settings edit: "the current value can be edited by hand".
    ///
    /// Names itself as a write, so it reads nothing like a claim.
    pub fn set_next(&mut self, value: u64) {
        self.current = value;
    }

    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn set_prefix(&mut self, prefix: impl Into<String>) {
        self.prefix = prefix.into();
    }

    #[must_use]
    pub const fn pad_width(&self) -> u8 {
        self.pad_width
    }

    pub const fn set_pad_width(&mut self, width: u8) {
        self.pad_width = width;
    }

    #[must_use]
    pub const fn reset_daily(&self) -> bool {
        self.reset_daily
    }

    pub const fn set_reset_daily(&mut self, reset: bool) {
        self.reset_daily = reset;
    }

    #[must_use]
    pub const fn start(&self) -> u64 {
        self.start
    }

    pub const fn set_start(&mut self, start: u64) {
        self.start = start;
    }

    fn format(&self, value: u64) -> String {
        let width = usize::from(self.pad_width);
        format!("{}{value:0width$}", self.prefix)
    }
}

/// The counters an order needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Numbering {
    pub token: Counter,
    pub bill: Counter,
}

impl Numbering {
    /// The defaults a new shop gets: tokens restart at 1 each day, bills run
    /// continuously.
    #[must_use]
    pub fn new() -> Self {
        Numbering { token: Counter::daily(1), bill: Counter::continuous(1) }
    }

    /// Both numbers for a new order, taken together.
    ///
    /// One call rather than two, because a new order gets a token *and* a bill
    /// number at the same moment (audit 2.3) and two calls is a gap a caller
    /// can interleave something into.
    ///
    /// `today` is the ORDER's business day, not the wall clock's — an order
    /// created at 00:15 belongs to yesterday and takes yesterday's series.
    pub fn claim_for_new_order(&mut self, today: BusinessDay) -> (Claimed, Claimed) {
        (self.token.claim(today), self.bill.claim(today))
    }
}

impl Default for Numbering {
    fn default() -> Self {
        Numbering::new()
    }
}

// P04 owns the persisted version of this, and it must be ONE statement:
//
//     UPDATE counters SET current = current + 1 WHERE id = ? RETURNING current
//
// never a SELECT followed by an UPDATE. Here, `&mut self` is what makes the
// claim indivisible; in SQLite it is the single statement. Same decision (D6),
// one layer down, and it is just as non-negotiable there.

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> BusinessDay {
        BusinessDay::from_ymd(2026, 8, d)
    }

    #[test]
    fn numbers_come_out_in_order_with_no_repeats() {
        // B4. Ten thousand claims, which is a busy month for a real shop.
        let mut counter = Counter::continuous(1);
        let today = day(1);
        let mut seen = Vec::with_capacity(10_000);

        for expected in 1..=10_000_u64 {
            let claimed = counter.claim(today);
            assert_eq!(claimed.value, expected, "a number was skipped or repeated");
            seen.push(claimed.value);
        }

        let mut sorted = seen.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), seen.len(), "a number was handed out twice");
        assert_eq!(seen.first(), Some(&1));
        assert_eq!(seen.last(), Some(&10_000));
    }

    #[test]
    fn a_counter_running_for_days_still_resets_every_morning() {
        // B3, and this is the test that would have caught it. The app never
        // "restarts" anywhere in here.
        let mut counter = Counter::daily(1);

        for offset in 1..=3_u32 {
            let today = day(offset);
            assert_eq!(counter.claim(today).value, 1, "day {offset} did not reset");
            assert_eq!(counter.claim(today).value, 2);
            assert_eq!(counter.claim(today).value, 3);
        }
    }

    #[test]
    fn the_rollover_happens_once_not_on_every_claim() {
        let mut counter = Counter::daily(1);
        assert_eq!(counter.claim(day(1)).value, 1);
        assert_eq!(counter.claim(day(1)).value, 2);
        assert_eq!(counter.claim(day(2)).value, 1, "a new day restarts the series");
        assert_eq!(counter.claim(day(2)).value, 2, "but only once");
        assert_eq!(counter.claim(day(2)).value, 3);
    }

    #[test]
    fn a_continuous_counter_never_resets() {
        // A bill series is expected to run unbroken — a GST invoice series
        // that restarts every morning is not a series.
        let mut counter = Counter::continuous(1);
        assert_eq!(counter.claim(day(1)).value, 1);
        assert_eq!(counter.claim(day(2)).value, 2);
        assert_eq!(counter.claim(day(3)).value, 3);
    }

    #[test]
    fn a_number_carries_the_day_it_belongs_to_not_the_day_it_was_asked_for() {
        // An order created at 00:15 belongs to yesterday and takes yesterday's
        // series — D5 and D6 meeting.
        let mut counter = Counter::daily(1);
        let yesterday = day(1);
        assert_eq!(counter.claim(yesterday).business_day, yesterday);
    }

    #[test]
    fn a_number_prints_the_way_the_shop_configured_it() {
        let mut counter = Counter::continuous(42);
        counter.set_prefix("BIR/");
        counter.set_pad_width(4);
        let claimed = counter.claim(day(1));
        assert_eq!(claimed.value, 42);
        assert_eq!(claimed.formatted, "BIR/0042");

        // No prefix and no padding is the plain case.
        let mut plain = Counter::continuous(7);
        assert_eq!(plain.claim(day(1)).formatted, "7");
    }

    #[test]
    fn the_settings_screen_can_see_and_move_the_counter_without_claiming() {
        let mut counter = Counter::continuous(1);
        assert_eq!(counter.last_issued(), None, "nothing has been issued yet");

        counter.claim(day(1));
        counter.claim(day(1));
        assert_eq!(counter.last_issued(), Some(2));

        // The owner sets the next bill number by hand.
        counter.set_next(500);
        assert_eq!(counter.claim(day(1)).value, 500);
        assert_eq!(counter.last_issued(), Some(500));
    }

    #[test]
    fn an_order_takes_its_token_and_bill_number_together() {
        let mut numbering = Numbering::new();
        let (token, bill) = numbering.claim_for_new_order(day(1));
        assert_eq!(token.value, 1);
        assert_eq!(bill.value, 1);

        let (token, bill) = numbering.claim_for_new_order(day(1));
        assert_eq!(token.value, 2);
        assert_eq!(bill.value, 2);

        // Next day: the token restarts, the bill series does not.
        let (token, bill) = numbering.claim_for_new_order(day(2));
        assert_eq!(token.value, 1, "tokens restart daily");
        assert_eq!(bill.value, 3, "the invoice series runs on");
    }

    #[test]
    fn counters_survive_a_round_trip_through_storage() {
        // They live in the database at P04, including the last reset day —
        // which is what lets a counter know it has not rolled over yet after
        // the app is reopened the next morning.
        let mut numbering = Numbering::new();
        numbering.claim_for_new_order(day(1));
        numbering.token.set_prefix("T-");

        let json = serde_json::to_string(&numbering).expect("serialises");
        let restored: Numbering = serde_json::from_str(&json).expect("reads");
        assert_eq!(restored, numbering);

        // And it carries on where it left off, without resetting again.
        let mut restored = restored;
        assert_eq!(restored.token.claim(day(1)).value, 2);
    }
}
