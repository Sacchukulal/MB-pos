//! Quantity.
//!
//! **A quantity is an `i64` count of thousandths of a unit.**
//!
//! Indian restaurants sell by weight — sweets, biryani by the kilo, mutton by
//! the half kilo. v1 held a quantity as a plain whole number and so could not
//! express half a kilo at all; the counter staff worked around it by inventing
//! a second menu item. Thousandths make 0.5, 0.25 and 0.333 exact.
//!
//! This module also owns **price × quantity**, because that multiplication is
//! where a rounding error would be born and it needs to happen exactly once.
//! Keeping it here leaves `money.rs` a leaf module that knows nothing about
//! quantities.

use crate::money::{Money, MoneyError};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Something a quantity could not represent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QtyError {
    #[error("that quantity is too large")]
    Overflow,
    #[error("`{0}` is not a valid quantity")]
    NotANumber(String),
    /// Refused rather than rounded, for the same reason `Money::parse` refuses
    /// a third decimal: it means the caller believes it can express something
    /// we cannot, and quietly rounding hides that from them.
    #[error("`{0}` has more than three decimal places")]
    TooPrecise(String),
    /// A cart line cannot hold minus half a kilo. Returning goods is a void
    /// (P12), which is a different operation with its own reason and audit
    /// entry — not a negative quantity slipped into a normal bill.
    #[error("a quantity cannot be negative")]
    Negative,
}

type Result<T> = std::result::Result<T, QtyError>;

/// A quantity, held as a whole number of thousandths of a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Qty(i64);

/// One whole unit, in thousandths.
const THOUSAND: i64 = 1_000;

impl Qty {
    pub const ZERO: Qty = Qty(0);
    pub const ONE: Qty = Qty(THOUSAND);

    /// Whole units. Checked rather than `× 1000`, because a silent wrap in a
    /// quantity is a silent wrap in a bill total (D7).
    pub fn from_whole(units: i64) -> Result<Self> {
        units.checked_mul(THOUSAND).map(Qty).ok_or(QtyError::Overflow)
    }

    #[must_use]
    pub const fn from_thousandths(thousandths: i64) -> Self {
        Qty(thousandths)
    }

    #[must_use]
    pub const fn thousandths(self) -> i64 {
        self.0
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Not `std::ops::Add`, for the same reason `Money::add` is not: that trait
    /// cannot fail, and a quantity that wraps is a bill that wraps (D7).
    #[allow(clippy::should_implement_trait, reason = "addition here must be able to fail (D7)")]
    pub fn add(self, other: Self) -> Result<Self> {
        self.0.checked_add(other.0).map(Qty).ok_or(QtyError::Overflow)
    }

    #[allow(clippy::should_implement_trait, reason = "see `add`")]
    pub fn sub(self, other: Self) -> Result<Self> {
        self.0.checked_sub(other.0).map(Qty).ok_or(QtyError::Overflow)
    }

    /// `unit_price × self`, rounded **exactly once**.
    ///
    /// "Extend" is the accounting word for price × quantity.
    ///
    /// The single `mul_ratio` call is the whole point. 0.333 kg at ₹100/kg is
    /// ₹33.30 rounded once; adding ₹33.33 three times, or dividing then
    /// multiplying, gives a different answer — and there is no honest way to
    /// explain a different answer to the customer holding the receipt.
    pub fn extend(self, unit_price: Money) -> std::result::Result<Money, MoneyError> {
        unit_price.mul_ratio(self.0, THOUSAND)
    }

    /// Parse what a cashier types: `"1"`, `"0.5"`, `".5"`, `"2.750"`.
    pub fn parse(input: &str) -> Result<Self> {
        let raw = input.trim();
        if raw.is_empty() {
            return Err(QtyError::NotANumber(raw.to_owned()));
        }
        if raw.starts_with('-') {
            return Err(QtyError::Negative);
        }
        let digits = raw.strip_prefix('+').unwrap_or(raw);

        // Shape before precision, so the error names the real fault. money.rs
        // learned this the same way: reporting "too precise" for "1.2.3" sends
        // someone hunting for a rounding setting that does not exist.
        let malformed = || QtyError::NotANumber(raw.to_owned());
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
        if frac_str.len() > 3 {
            return Err(QtyError::TooPrecise(raw.to_owned()));
        }

        let whole: i64 = if whole_str.is_empty() {
            0
        } else {
            whole_str.parse().map_err(|_| QtyError::Overflow)?
        };
        // "5" after the point is 500 thousandths, not 5.
        let frac: i64 = if frac_str.is_empty() {
            0
        } else {
            let parsed: i64 = frac_str.parse().map_err(|_| malformed())?;
            let scale = match frac_str.len() {
                1 => 100,
                2 => 10,
                _ => 1,
            };
            parsed * scale
        };

        whole
            .checked_mul(THOUSAND)
            .and_then(|t| t.checked_add(frac))
            .map(Qty)
            .ok_or(QtyError::Overflow)
    }
}

impl fmt::Display for Qty {
    /// `3`, `0.5`, `0.25`, `1.333` — trailing zeros trimmed.
    ///
    /// This string is printed on a customer's receipt beside the item name.
    /// "0.500 KG Mutton" reads like a machine artefact; "0.5 KG Mutton" reads
    /// like a shop.
    #[allow(
        clippy::integer_division,
        reason = "splitting thousandths into whole units and a fraction"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let negative = self.0 < 0;
        let abs = self.0.unsigned_abs();
        let whole = abs / 1_000;
        let frac = abs % 1_000;
        if negative {
            f.write_str("-")?;
        }
        if frac == 0 {
            write!(f, "{whole}")
        } else {
            let text = format!("{frac:03}");
            write!(f, "{whole}.{}", text.trim_end_matches('0'))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_a_kilo_is_exact_and_rounds_only_once() {
        // The case v1 could not express at all.
        let half = Qty::parse("0.5").expect("parses");
        let price = Money::from_paise(24_000); // ₹240 per kg
        assert_eq!(half.extend(price), Ok(Money::from_paise(12_000))); // ₹120.00

        let quarter = Qty::parse("0.25").expect("parses");
        assert_eq!(quarter.extend(Money::from_paise(9_900)), Ok(Money::from_paise(2_475)));

        // 0.333 kg at ₹100/kg is ₹33.30, rounded once.
        let third = Qty::parse("0.333").expect("parses");
        assert_eq!(third.extend(Money::from_paise(10_000)), Ok(Money::from_paise(3_330)));
    }

    #[test]
    fn dividing_before_multiplying_is_the_mistake_extend_prevents() {
        // ₹24.05 per kg — a price with paise in it, which is where the
        // difference shows up. Half a kilo is ₹12.025, which rounds once to
        // ₹12.03.
        let price = Money::from_paise(2_405);
        let half = Qty::from_thousandths(500);
        let correct = half.extend(price).expect("computes");
        assert_eq!(correct, Money::from_paise(1_203));

        // The tempting alternative — work out the price per thousandth first,
        // then multiply by the quantity. ₹24.05 / 1000 rounds to 2 paise, and
        // 2 × 500 is ₹10.00. Off by ₹2.03 on one line, and it compounds down
        // the bill.
        let per_thousandth = price.mul_ratio(1, 1_000).expect("computes");
        let wrong = per_thousandth.mul_qty(500).expect("computes");
        assert_eq!(wrong, Money::from_paise(1_000));
        assert_ne!(correct, wrong, "this is the whole reason extend is one call");
    }

    #[test]
    fn whole_quantities_behave_like_whole_numbers() {
        let three = Qty::from_whole(3).expect("in range");
        assert_eq!(three.thousandths(), 3_000);
        assert_eq!(three.extend(Money::from_paise(4_550)), Ok(Money::from_paise(13_650)));
        assert_eq!(Qty::ONE.extend(Money::from_paise(777)), Ok(Money::from_paise(777)));
        assert_eq!(Qty::ZERO.extend(Money::from_paise(777)), Ok(Money::ZERO));
    }

    #[test]
    fn parses_what_a_cashier_types() {
        assert_eq!(Qty::parse("1"), Ok(Qty::ONE));
        assert_eq!(Qty::parse("0.5"), Ok(Qty::from_thousandths(500)));
        assert_eq!(Qty::parse(".5"), Ok(Qty::from_thousandths(500)));
        assert_eq!(Qty::parse("2.750"), Ok(Qty::from_thousandths(2_750)));
        assert_eq!(Qty::parse(" 1.25 "), Ok(Qty::from_thousandths(1_250)));
        assert_eq!(Qty::parse("0.05"), Ok(Qty::from_thousandths(50)));
        assert_eq!(Qty::parse("0.005"), Ok(Qty::from_thousandths(5)));
        assert_eq!(Qty::parse("0"), Ok(Qty::ZERO));
    }

    #[test]
    fn refuses_rather_than_rounds_or_wraps() {
        assert!(matches!(Qty::parse("1.2345"), Err(QtyError::TooPrecise(_))));
        assert!(matches!(Qty::parse("-1"), Err(QtyError::Negative)));
        assert!(matches!(Qty::parse("abc"), Err(QtyError::NotANumber(_))));
        assert!(matches!(Qty::parse(""), Err(QtyError::NotANumber(_))));
        assert!(matches!(Qty::parse("   "), Err(QtyError::NotANumber(_))));
        assert!(matches!(Qty::parse("1.2.3"), Err(QtyError::NotANumber(_))));
        assert!(matches!(Qty::parse("."), Err(QtyError::NotANumber(_))));
        assert!(matches!(Qty::parse("1 2"), Err(QtyError::NotANumber(_))));
        // Shape is judged before precision, so the message names the real fault.
        assert!(matches!(Qty::parse("1.2.3456"), Err(QtyError::NotANumber(_))));

        assert_eq!(Qty::from_whole(i64::MAX), Err(QtyError::Overflow));
        assert_eq!(Qty::parse("99999999999999999999"), Err(QtyError::Overflow));
    }

    #[test]
    fn displays_the_way_a_receipt_should_read() {
        assert_eq!(Qty::from_whole(3).expect("in range").to_string(), "3");
        assert_eq!(Qty::from_thousandths(500).to_string(), "0.5");
        assert_eq!(Qty::from_thousandths(250).to_string(), "0.25");
        assert_eq!(Qty::from_thousandths(1_333).to_string(), "1.333");
        assert_eq!(Qty::from_thousandths(1_500).to_string(), "1.5");
        assert_eq!(Qty::from_thousandths(5).to_string(), "0.005");
        assert_eq!(Qty::from_thousandths(50).to_string(), "0.05");
        assert_eq!(Qty::ZERO.to_string(), "0");
        assert_eq!(Qty::from_thousandths(-500).to_string(), "-0.5");
    }

    #[test]
    fn round_trips_through_text() {
        for thousandths in [0_i64, 1, 5, 50, 500, 1_000, 1_333, 12_345, 1_000_000] {
            let q = Qty::from_thousandths(thousandths);
            assert_eq!(Qty::parse(&q.to_string()), Ok(q), "{thousandths} did not round trip");
        }
    }

    #[test]
    fn arithmetic_is_checked_not_wrapped() {
        let a = Qty::from_thousandths(i64::MAX);
        assert_eq!(a.add(Qty::ONE), Err(QtyError::Overflow));
        assert_eq!(
            Qty::from_thousandths(i64::MIN).sub(Qty::ONE),
            Err(QtyError::Overflow)
        );
        assert_eq!(Qty::ONE.add(Qty::ONE), Ok(Qty::from_thousandths(2_000)));
        assert_eq!(Qty::ONE.sub(Qty::from_thousandths(500)), Ok(Qty::from_thousandths(500)));
    }

    #[test]
    fn an_absurd_quantity_is_an_error_not_a_wrapped_total() {
        // A fat finger on the quantity box must not silently produce a
        // negative bill (D7).
        let huge = Qty::from_thousandths(i64::MAX);
        assert_eq!(huge.extend(Money::from_paise(10_000)), Err(MoneyError::Overflow));
    }
}
