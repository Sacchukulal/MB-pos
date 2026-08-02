//! Money.
//!
//! **A rupee amount is an `i64` count of paise. There is no floating point
//! anywhere in this file, and none is permitted anywhere in the money path.**
//!
//! v1 stored `subtotal`, `gst` and `total` as SQLite REAL. In binary floating
//! point `0.1 + 0.2` is not `0.3`, so a long bill accumulates error and the
//! printed grand total can disagree with the sum of its own printed lines. No
//! customer had reported it yet; that is luck, not correctness.
//!
//! Every operation that can lose information returns a `Result`. Nothing here
//! saturates, truncates or defaults silently (decision D7).
//!
//! **Rounding rule: half away from zero.** ₹0.005 becomes ₹0.01 and ₹-0.005
//! becomes ₹-0.01. That is what Indian commercial practice expects, and it is
//! applied once per line — never to a running total.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Something a money value could not represent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    #[error("amount is too large to represent")]
    Overflow,
    #[error("cannot divide by zero")]
    DivideByZero,
    #[error("`{0}` is not a valid amount")]
    NotANumber(String),
    /// Rejected rather than truncated: paise is the smallest unit we have, so
    /// a price of `12.345` is a mistake somewhere upstream, not a rounding
    /// opportunity.
    #[error("`{0}` has more than two decimal places")]
    TooPrecise(String),
}

type Result<T> = std::result::Result<T, MoneyError>;

/// How a grand total is brought to a whole rupee.
///
/// Audit B8: v1 had no round-off at all and printed bills like ₹487.35. Indian
/// counters round to the rupee, and which way is a shop's own choice — some
/// always round down as a courtesy, some always up.
///
/// This lives in `money.rs` rather than with the bill because it describes an
/// arithmetic operation, and the arithmetic has to have exactly one home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundingMode {
    /// Print the paise. No adjustment.
    None,
    /// To the nearest rupee, ties away from zero. The Indian default.
    #[default]
    NearestRupee,
    /// Always to the rupee above — ceiling, including for negatives.
    Up,
    /// Always to the rupee below — floor, including for negatives.
    Down,
}

/// A rupee amount, held as a whole number of paise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Money = Money(0);

    #[must_use]
    pub const fn from_paise(paise: i64) -> Self {
        Money(paise)
    }

    /// Whole rupees. Fails rather than wrapping on an absurd input.
    pub fn from_rupees(rupees: i64) -> Result<Self> {
        rupees.checked_mul(100).map(Money).ok_or(MoneyError::Overflow)
    }

    #[must_use]
    pub const fn paise(self) -> i64 {
        self.0
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    #[must_use]
    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    #[must_use]
    pub const fn abs(self) -> Self {
        Money(self.0.saturating_abs())
    }

    #[must_use]
    pub const fn neg(self) -> Self {
        Money(-self.0)
    }

    /// Deliberately NOT `std::ops::Add`. That trait returns `Self` and cannot
    /// fail, so implementing it would mean either wrapping or saturating on
    /// overflow — both of which are exactly what D7 forbids in the money path.
    /// The name stays `add` because `a.add(b)?` is what the call sites read
    /// like everywhere in this crate.
    #[allow(clippy::should_implement_trait, reason = "addition here must be able to fail (D7)")]
    pub fn add(self, other: Self) -> Result<Self> {
        self.0.checked_add(other.0).map(Money).ok_or(MoneyError::Overflow)
    }

    #[allow(clippy::should_implement_trait, reason = "see `add` — subtraction must be able to fail")]
    pub fn sub(self, other: Self) -> Result<Self> {
        self.0.checked_sub(other.0).map(Money).ok_or(MoneyError::Overflow)
    }

    /// Multiply by a whole count (a line quantity). Exact — no rounding.
    pub fn mul_qty(self, qty: i64) -> Result<Self> {
        self.0.checked_mul(qty).map(Money).ok_or(MoneyError::Overflow)
    }

    /// `self × numerator ÷ denominator`, rounded half away from zero.
    ///
    /// This is the ONE place a money value is ever rounded. Percentages,
    /// inclusive-tax extraction and proportional discount distribution all
    /// route through here, so there is exactly one rounding behaviour in the
    /// system and exactly one place to test it.
    #[allow(
        clippy::integer_division,
        reason = "integer division IS the operation: `den / 2` is the standard \
                  half-away-from-zero bias and is exact for both parities"
    )]
    pub fn mul_ratio(self, numerator: i64, denominator: i64) -> Result<Self> {
        if denominator == 0 {
            return Err(MoneyError::DivideByZero);
        }
        // i128 so the intermediate product of two i64 values cannot overflow.
        let product = i128::from(self.0) * i128::from(numerator);
        let den = i128::from(denominator);
        // Bias the numerator by half a denominator, in the direction of the
        // sign, so truncating division lands on the nearest integer and ties
        // go away from zero.
        let biased = if (product < 0) == (den < 0) {
            product + den / 2
        } else {
            product - den / 2
        };
        let quotient = biased / den;
        i64::try_from(quotient).map(Money).map_err(|_| MoneyError::Overflow)
    }

    /// `self × numerator ÷ denominator`, rounded toward negative infinity.
    ///
    /// The proportional discount spread needs floor, not nearest: it hands out
    /// the floors first and then distributes the leftover paise one at a time
    /// to the largest remainders. That is what lets the shares add back to the
    /// whole discount exactly AND keeps every share inside its own line. Round
    /// to nearest here and the shares can overshoot both ways at once.
    ///
    /// Tax and percentages must NOT use this — they use `mul_ratio`.
    #[allow(
        clippy::integer_division,
        reason = "flooring division IS the operation; div_euclid is exact"
    )]
    pub fn mul_ratio_floor(self, numerator: i64, denominator: i64) -> Result<Self> {
        if denominator == 0 {
            return Err(MoneyError::DivideByZero);
        }
        // i128 so the intermediate product of two i64 values cannot overflow.
        let product = i128::from(self.0) * i128::from(numerator);
        let den = i128::from(denominator);
        // div_euclid floors toward negative infinity for a positive divisor.
        // For a negative divisor it floors the other way, so normalise the
        // signs first rather than leaving a trap for whoever passes one.
        let (product, den) = if den < 0 { (-product, -den) } else { (product, den) };
        let quotient = product.div_euclid(den);
        i64::try_from(quotient).map(Money).map_err(|_| MoneyError::Overflow)
    }

    /// Apply a rate given in basis points (500 bp = 5.00%).
    pub fn percent_bp(self, basis_points: u32) -> Result<Self> {
        self.mul_ratio(i64::from(basis_points), 10_000)
    }

    /// Sum without an intermediate that can silently overflow.
    pub fn try_sum<I: IntoIterator<Item = Money>>(items: I) -> Result<Money> {
        items.into_iter().try_fold(Money::ZERO, Money::add)
    }

    /// Split into two halves that always add back to the original.
    ///
    /// Used for the CGST/SGST split. Halving twice and rounding twice can lose
    /// a paisa (₹0.05 → ₹0.03 + ₹0.03 = ₹0.06); taking the remainder as the
    /// second half makes the identity exact by construction.
    #[must_use]
    pub fn halve_exact(self) -> (Money, Money) {
        // mul_ratio(1, 2) cannot fail: denominator is non-zero and the
        // magnitude only shrinks.
        let first = self.mul_ratio(1, 2).unwrap_or(Money::ZERO);
        let second = Money(self.0 - first.0);
        (first, second)
    }

    /// Distance to the nearest whole rupee, as the adjustment that would get
    /// there. Positive means "add this to reach the rupee above".
    ///
    /// Round-off is applied to a grand total only, and is recorded as its own
    /// line so the printed bill reconciles exactly (decision D4, step 7-8).
    #[must_use]
    pub fn round_off_adjustment(self) -> Money {
        self.round_adjustment(RoundingMode::NearestRupee)
    }

    /// The adjustment that reaches a whole rupee under `mode`.
    ///
    /// **`Up` and `Down` are ceiling and floor**, not "away from zero" and
    /// "toward zero". A bill is never negative, but a refund line (P12) will
    /// be, and "round up" has to keep meaning the same thing when it is.
    ///
    /// An amount that is already on a whole rupee is left alone by every mode.
    /// That sounds obvious and it is where an off-by-one would quietly add a
    /// rupee to every round bill in the shop.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "flooring to a whole rupee IS the operation; the remainder is \
                  what the adjustment is computed from"
    )]
    pub fn round_adjustment(self, mode: RoundingMode) -> Money {
        // Euclidean remainder, so it is never negative and `floor` below is a
        // true floor for negative amounts too.
        let remainder = self.0.rem_euclid(100);
        if remainder == 0 {
            return Money::ZERO;
        }
        let floor = self.0 - remainder;
        match mode {
            RoundingMode::None => Money::ZERO,
            RoundingMode::Down => Money(floor - self.0),
            RoundingMode::Up => Money(floor + 100 - self.0),
            // Ties go AWAY FROM ZERO, matching the rule in the module doc and
            // `mul_ratio`: ₹487.50 becomes ₹488, and −₹487.50 becomes −₹488.
            // That is why this arm works on the magnitude instead of reusing
            // the Euclidean floor above — flooring would send a negative tie
            // toward zero and give this module two rounding rules.
            RoundingMode::NearestRupee => {
                let magnitude = self.0.saturating_abs();
                let rem = magnitude % 100;
                let target = if rem >= 50 { magnitude - rem + 100 } else { magnitude - rem };
                let signed = if self.0 < 0 { -target } else { target };
                Money(signed - self.0)
            }
        }
    }

    /// Parse a human amount: `"1234"`, `"12.5"`, `"₹1,234.50"`, `"-3.25"`.
    pub fn parse(input: &str) -> Result<Self> {
        let raw = input.trim();
        // '₹' is U+20B9; listing it twice is the same character.
        let cleaned: String = raw.chars().filter(|c| !matches!(c, '₹' | ',' | ' ')).collect();
        if cleaned.is_empty() {
            return Err(MoneyError::NotANumber(raw.to_owned()));
        }

        let (negative, digits) = match cleaned.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, cleaned.strip_prefix('+').unwrap_or(&cleaned)),
        };

        // Shape first, precision second. `1.2.3` is malformed, not "too
        // precise" — checking length before shape reported the wrong reason,
        // which is the kind of thing that sends a shopkeeper hunting for a
        // rounding setting that does not exist.
        let malformed = || MoneyError::NotANumber(raw.to_owned());
        if digits.is_empty() || digits.chars().filter(|c| *c == '.').count() > 1 {
            return Err(malformed());
        }
        if !digits.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Err(malformed());
        }

        let (whole_str, frac_str) = digits.split_once('.').unwrap_or((digits, ""));
        if whole_str.is_empty() && frac_str.is_empty() {
            return Err(malformed());
        }
        if frac_str.len() > 2 {
            return Err(MoneyError::TooPrecise(raw.to_owned()));
        }

        let whole: i64 = if whole_str.is_empty() {
            0
        } else {
            whole_str.parse().map_err(|_| MoneyError::Overflow)?
        };
        // "5" after the point means 50 paise, not 5.
        let frac: i64 = match frac_str.len() {
            0 => 0,
            1 => frac_str.parse::<i64>().map_err(|_| MoneyError::NotANumber(raw.to_owned()))? * 10,
            _ => frac_str.parse::<i64>().map_err(|_| MoneyError::NotANumber(raw.to_owned()))?,
        };

        let paise = whole
            .checked_mul(100)
            .and_then(|p| p.checked_add(frac))
            .ok_or(MoneyError::Overflow)?;
        Ok(Money(if negative { -paise } else { paise }))
    }

    /// `1234.50` — plain, two decimals, no symbol, no grouping.
    /// This is the receipt and CSV form.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "splitting paise into rupees and paise for display; both parts \
                  are kept, so nothing is lost"
    )]
    pub fn to_plain_string(self) -> String {
        let negative = self.0 < 0;
        let abs = self.0.unsigned_abs();
        let rupees = abs / 100;
        let paise = abs % 100;
        format!("{}{rupees}.{paise:02}", if negative { "-" } else { "" })
    }

    /// `₹1,23,456.78` — Indian digit grouping (last three, then twos).
    /// This is the on-screen form.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "splitting paise into rupees and paise for display; both parts \
                  are kept, so nothing is lost"
    )]
    pub fn to_indian_string(self) -> String {
        let negative = self.0 < 0;
        let abs = self.0.unsigned_abs();
        let rupees = abs / 100;
        let paise = abs % 100;

        let digits = rupees.to_string();
        let grouped = if digits.len() <= 3 {
            digits
        } else {
            let (head, tail) = digits.split_at(digits.len() - 3);
            let mut parts: Vec<String> = Vec::new();
            let head_bytes = head.as_bytes();
            let mut end = head_bytes.len();
            while end > 0 {
                let start = end.saturating_sub(2);
                parts.push(head[start..end].to_owned());
                end = start;
            }
            parts.reverse();
            parts.push(tail.to_owned());
            parts.join(",")
        };
        format!("{}₹{grouped}.{paise:02}", if negative { "-" } else { "" })
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_plain_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_float_bug_that_v1_had_cannot_happen_here() {
        // In v1 this was `0.1 + 0.2 == 0.30000000000000004`.
        let sum = Money::try_sum([Money::from_paise(10), Money::from_paise(20)]);
        assert_eq!(sum, Ok(Money::from_paise(30)));

        // A hundred lines of ₹0.07 must be exactly ₹7.00, not ₹6.999...
        let hundred = std::iter::repeat_n(Money::from_paise(7), 100);
        assert_eq!(Money::try_sum(hundred), Ok(Money::from_paise(700)));
    }

    #[test]
    fn rounds_half_away_from_zero() {
        // 0.5 paise up
        assert_eq!(Money::from_paise(1).mul_ratio(1, 2), Ok(Money::from_paise(1)));
        // -0.5 paise down (away from zero)
        assert_eq!(Money::from_paise(-1).mul_ratio(1, 2), Ok(Money::from_paise(-1)));
        // 1.5 -> 2
        assert_eq!(Money::from_paise(3).mul_ratio(1, 2), Ok(Money::from_paise(2)));
        // 2.5 -> 3
        assert_eq!(Money::from_paise(5).mul_ratio(1, 2), Ok(Money::from_paise(3)));
        // under half stays down
        assert_eq!(Money::from_paise(1).mul_ratio(1, 3), Ok(Money::ZERO));
        // over half goes up
        assert_eq!(Money::from_paise(2).mul_ratio(1, 3), Ok(Money::from_paise(1)));
        // negative, under half
        assert_eq!(Money::from_paise(-1).mul_ratio(1, 3), Ok(Money::ZERO));
        // negative, over half
        assert_eq!(Money::from_paise(-2).mul_ratio(1, 3), Ok(Money::from_paise(-1)));
    }

    #[test]
    fn flooring_always_rounds_down_never_to_nearest() {
        // The difference from mul_ratio: 2/3 of a paisa is 0 here and 1 there.
        assert_eq!(Money::from_paise(2).mul_ratio_floor(1, 3), Ok(Money::ZERO));
        assert_eq!(Money::from_paise(2).mul_ratio(1, 3), Ok(Money::from_paise(1)));

        assert_eq!(Money::from_paise(1).mul_ratio_floor(1, 2), Ok(Money::ZERO));
        assert_eq!(Money::from_paise(3).mul_ratio_floor(1, 2), Ok(Money::from_paise(1)));
        assert_eq!(Money::from_paise(100).mul_ratio_floor(1, 1), Ok(Money::from_paise(100)));

        // Toward negative infinity, not toward zero: -1/2 floors to -1.
        assert_eq!(Money::from_paise(-1).mul_ratio_floor(1, 2), Ok(Money::from_paise(-1)));
        assert_eq!(Money::from_paise(-2).mul_ratio_floor(1, 3), Ok(Money::from_paise(-1)));

        // A negative denominator must floor the same way, not the other way.
        assert_eq!(Money::from_paise(1).mul_ratio_floor(1, -2), Ok(Money::from_paise(-1)));
        assert_eq!(Money::from_paise(-1).mul_ratio_floor(1, -2), Ok(Money::ZERO));

        assert_eq!(Money::from_paise(1).mul_ratio_floor(1, 0), Err(MoneyError::DivideByZero));
    }

    #[test]
    fn flooring_never_hands_a_part_more_than_the_part_is_worth() {
        // The invariant the discount spread rests on: while the amount being
        // shared out is no bigger than the whole, a part's floored share can
        // never exceed the part itself — so a discount cannot make a line
        // negative. Rounding to nearest does NOT have this property.
        for total_net in 1..120_i64 {
            for part in 1..=total_net {
                for total in 0..=total_net {
                    let share = Money::from_paise(total)
                        .mul_ratio_floor(part, total_net)
                        .expect("in range");
                    assert!(
                        share.paise() <= part,
                        "{total} shared over {total_net} gave {part} more than {part}"
                    );
                }
            }
        }
    }

    #[test]
    fn percent_bp_matches_hand_arithmetic() {
        let hundred = Money::from_rupees(100).unwrap_or(Money::ZERO);
        assert_eq!(hundred.percent_bp(500), Ok(Money::from_paise(500))); // 5% of 100 = 5.00
        assert_eq!(hundred.percent_bp(1800), Ok(Money::from_paise(1800))); // 18%
        assert_eq!(hundred.percent_bp(0), Ok(Money::ZERO));

        // 5% of ₹99.99 = ₹4.9995 -> ₹5.00 (half away from zero)
        assert_eq!(Money::from_paise(9999).percent_bp(500), Ok(Money::from_paise(500)));
    }

    #[test]
    fn halving_never_loses_a_paisa() {
        for paise in 0..500_i64 {
            let m = Money::from_paise(paise);
            let (a, b) = m.halve_exact();
            assert_eq!(a.add(b), Ok(m), "halving {paise} paise lost money");
        }
        // The classic: 5 paise must split 3 + 2, not 3 + 3.
        assert_eq!(
            Money::from_paise(5).halve_exact(),
            (Money::from_paise(3), Money::from_paise(2))
        );
    }

    #[test]
    fn round_off_reaches_the_nearest_rupee() {
        // 487.35 -> needs -0.35 to reach 487.00
        let m = Money::from_paise(48_735);
        let adj = m.round_off_adjustment();
        assert_eq!(adj, Money::from_paise(-35));
        assert_eq!(m.add(adj), Ok(Money::from_paise(48_700)));

        // 487.65 -> needs +0.35 to reach 488.00
        let m = Money::from_paise(48_765);
        let adj = m.round_off_adjustment();
        assert_eq!(adj, Money::from_paise(35));
        assert_eq!(m.add(adj), Ok(Money::from_paise(48_800)));

        // Exactly on a rupee needs nothing.
        assert_eq!(Money::from_paise(48_700).round_off_adjustment(), Money::ZERO);

        // A half-rupee tie goes up (away from zero).
        let m = Money::from_paise(48_750);
        assert_eq!(m.add(m.round_off_adjustment()), Ok(Money::from_paise(48_800)));
    }

    #[test]
    fn every_rounding_mode_reaches_its_rupee() {
        // The audit's B8 bill, through all four modes.
        let m = Money::from_paise(48_735); // ₹487.35
        let after = |mode| m.add(m.round_adjustment(mode)).expect("adds");
        assert_eq!(after(RoundingMode::NearestRupee), Money::from_paise(48_700));
        assert_eq!(after(RoundingMode::Up), Money::from_paise(48_800));
        assert_eq!(after(RoundingMode::Down), Money::from_paise(48_700));
        assert_eq!(after(RoundingMode::None), m);

        let m = Money::from_paise(48_765); // ₹487.65
        let after = |mode| m.add(m.round_adjustment(mode)).expect("adds");
        assert_eq!(after(RoundingMode::NearestRupee), Money::from_paise(48_800));
        assert_eq!(after(RoundingMode::Up), Money::from_paise(48_800));
        assert_eq!(after(RoundingMode::Down), Money::from_paise(48_700));

        // A tie goes away from zero.
        let m = Money::from_paise(48_750);
        assert_eq!(
            m.add(m.round_adjustment(RoundingMode::NearestRupee)),
            Ok(Money::from_paise(48_800))
        );
    }

    #[test]
    fn an_amount_already_on_a_rupee_is_left_alone_by_every_mode() {
        // An off-by-one here would silently add a rupee to every round bill in
        // the shop, on every mode, forever.
        for paise in [0_i64, 100, 48_700, -100, -48_700] {
            let m = Money::from_paise(paise);
            for mode in [
                RoundingMode::None,
                RoundingMode::NearestRupee,
                RoundingMode::Up,
                RoundingMode::Down,
            ] {
                assert_eq!(
                    m.round_adjustment(mode),
                    Money::ZERO,
                    "{paise} paise moved under {mode:?}"
                );
            }
        }
    }

    #[test]
    fn up_and_down_are_ceiling_and_floor_even_below_zero() {
        // Pinned deliberately: a refund line (P12) is negative, and "round up"
        // has to keep meaning the same thing when it is.
        let m = Money::from_paise(-48_735); // −₹487.35
        let after = |mode| m.add(m.round_adjustment(mode)).expect("adds");
        assert_eq!(after(RoundingMode::Up), Money::from_paise(-48_700), "ceiling");
        assert_eq!(after(RoundingMode::Down), Money::from_paise(-48_800), "floor");
        // Nearest still goes away from zero, matching mul_ratio.
        assert_eq!(after(RoundingMode::NearestRupee), Money::from_paise(-48_700));

        let m = Money::from_paise(-48_750); // an exact negative tie
        assert_eq!(
            m.add(m.round_adjustment(RoundingMode::NearestRupee)),
            Ok(Money::from_paise(-48_800)),
            "a negative tie rounds away from zero, like every other rounding here"
        );
    }

    #[test]
    fn rounding_always_lands_on_a_whole_rupee() {
        for paise in -400..400_i64 {
            let m = Money::from_paise(paise);
            for mode in [RoundingMode::NearestRupee, RoundingMode::Up, RoundingMode::Down] {
                let landed = m.add(m.round_adjustment(mode)).expect("adds");
                assert_eq!(landed.paise() % 100, 0, "{paise} under {mode:?} kept paise");
                // And it never moves by a whole rupee or more.
                assert!(m.round_adjustment(mode).abs().paise() < 100);
            }
        }
    }

    #[test]
    fn parses_what_a_human_types() {
        assert_eq!(Money::parse("120"), Ok(Money::from_paise(12_000)));
        assert_eq!(Money::parse("120.5"), Ok(Money::from_paise(12_050)));
        assert_eq!(Money::parse("120.05"), Ok(Money::from_paise(12_005)));
        assert_eq!(Money::parse(" ₹1,234.50 "), Ok(Money::from_paise(123_450)));
        assert_eq!(Money::parse("-3.25"), Ok(Money::from_paise(-325)));
        assert_eq!(Money::parse(".75"), Ok(Money::from_paise(75)));
        assert_eq!(Money::parse("0"), Ok(Money::ZERO));
    }

    #[test]
    fn refuses_rather_than_truncates() {
        // D7: a third decimal is a mistake upstream, not a rounding chance.
        assert!(matches!(Money::parse("12.345"), Err(MoneyError::TooPrecise(_))));
        assert!(matches!(Money::parse("abc"), Err(MoneyError::NotANumber(_))));
        assert!(matches!(Money::parse(""), Err(MoneyError::NotANumber(_))));
        assert!(matches!(Money::parse("1.2.3"), Err(MoneyError::NotANumber(_))));
        assert!(matches!(Money::parse("12,,3x"), Err(MoneyError::NotANumber(_))));
        assert!(matches!(Money::parse("."), Err(MoneyError::NotANumber(_))));
        assert!(matches!(Money::parse("-"), Err(MoneyError::NotANumber(_))));
        assert!(matches!(Money::parse("1..2"), Err(MoneyError::NotANumber(_))));
        // Shape is judged before precision, so the message names the real fault.
        assert!(matches!(Money::parse("1.2.345"), Err(MoneyError::NotANumber(_))));
    }

    #[test]
    fn overflow_is_an_error_not_a_wrap() {
        let huge = Money::from_paise(i64::MAX);
        assert_eq!(huge.add(Money::from_paise(1)), Err(MoneyError::Overflow));
        assert_eq!(huge.mul_qty(2), Err(MoneyError::Overflow));
        assert_eq!(
            Money::from_paise(1).mul_ratio(1, 0),
            Err(MoneyError::DivideByZero)
        );
    }

    #[test]
    fn formats_for_receipt_and_for_screen() {
        assert_eq!(Money::from_paise(12_345_678).to_plain_string(), "123456.78");
        assert_eq!(Money::from_paise(12_345_678).to_indian_string(), "₹1,23,456.78");
        assert_eq!(Money::from_paise(50).to_indian_string(), "₹0.50");
        assert_eq!(Money::from_paise(100_000).to_indian_string(), "₹1,000.00");
        assert_eq!(Money::from_paise(1_000_000).to_indian_string(), "₹10,000.00");
        assert_eq!(Money::from_paise(10_000_000).to_indian_string(), "₹1,00,000.00");
        assert_eq!(Money::from_paise(-12_345).to_indian_string(), "-₹123.45");
        assert_eq!(Money::ZERO.to_plain_string(), "0.00");
    }

    #[test]
    fn round_trips_through_text() {
        for paise in [0_i64, 1, 99, 100, 12_345, -12_345, 99_999_999] {
            let m = Money::from_paise(paise);
            assert_eq!(Money::parse(&m.to_plain_string()), Ok(m));
            assert_eq!(Money::parse(&m.to_indian_string()), Ok(m));
        }
    }
}
