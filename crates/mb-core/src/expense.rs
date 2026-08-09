//! **Money going out** — the two rules an expense has that are worth proving.
//!
//! Everything else about an expense is a row. These two are arithmetic, and
//! arithmetic in a screen is arithmetic with a second answer.

use serde::{Deserialize, Serialize};

use crate::businessday::BusinessDay;
use crate::money::{Money, MoneyError};
use crate::tax::TaxRate;

/// **The input credit hiding inside a bill somebody paid** — scope 2.9.
///
/// A shop buys ₹1,180 of vegetables at 18%. It did not pay ₹1,180 plus tax; it
/// paid ₹1,180 **including** ₹180 of tax, and that ₹180 is money it can claim
/// back. So the extraction is the inclusive one — the same arithmetic
/// `tax::extract` does for an inclusive-priced dosa, reached through the same
/// function so there is exactly one answer in the product.
///
/// Returns zero for a zero rate rather than an error: a purchase from an
/// unregistered vendor has no input credit, and that is an ordinary Tuesday.
pub fn input_credit(paid: Money, rate: TaxRate) -> Result<Money, MoneyError> {
    if rate.is_zero() || !paid.is_positive() {
        return Ok(Money::ZERO);
    }
    // gross × rate ÷ (100% + rate), in one rounding step (D2's single path).
    let bp = i64::from(rate.basis_points());
    paid.mul_ratio(bp, 10_000 + bp)
}

/// How often a template repeats — scope 10.6's recurring expenses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Every {
    Week,
    Month,
}

/// When the next one falls due.
///
/// **Month means the same date next month**, and the last day of a short month
/// when there is no such date: a rent due on the 31st is due on the 30th in
/// April and on the 28th in February. Sliding it to the 1st of the next month
/// would move rent out of the month it is rent for, which is worse than a day
/// of imprecision — and letting it silently skip February entirely, which is
/// what "add 31 days" does twice a year, is worse than both.
#[must_use]
pub fn next_due(from: BusinessDay, every: Every) -> BusinessDay {
    match every {
        Every::Week => BusinessDay::from_days_since_epoch(from.days_since_epoch() + 7),
        Every::Month => {
            let (year, month, day) = from.to_ymd();
            let (next_year, next_month) =
                if month == 12 { (year + 1, 1) } else { (year, month + 1) };
            let last = last_day_of(next_year, next_month);
            BusinessDay::from_ymd(next_year, next_month, day.min(last))
        }
    }
}

/// The length of a month, including February in a leap year — the one piece of
/// calendar arithmetic this crate needs and the reason it does not take a date
/// library for it (R6).
#[must_use]
pub const fn last_day_of(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        // A month number that is not a month is a bug elsewhere; 30 is the
        // answer that fails least loudly and cannot skip a due date.
        _ => 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_input_credit_is_extracted_from_what_was_paid_not_added_to_it() {
        // ₹1,180 at 18% contains ₹180 of tax. Adding 18% instead would claim
        // ₹212.40 the shop never paid.
        let paid = Money::from_rupees(1_180).expect("money");
        let rate = TaxRate::from_basis_points(1_800).expect("18%");
        assert_eq!(
            input_credit(paid, rate).expect("credit"),
            Money::from_rupees(180).expect("money"),
        );
    }

    #[test]
    fn a_purchase_with_no_tax_has_no_input_credit() {
        let paid = Money::from_rupees(500).expect("money");
        assert_eq!(
            input_credit(paid, TaxRate::ZERO).expect("credit"),
            Money::ZERO,
        );
        assert_eq!(
            input_credit(Money::ZERO, TaxRate::from_basis_points(500).expect("5%"))
                .expect("credit"),
            Money::ZERO,
        );
    }

    /// The credit never exceeds what was spent, at any rate, for any amount —
    /// which is the invariant the database CHECK also states.
    #[test]
    fn the_credit_is_never_more_than_the_money() {
        for paise in 1..=2_000_i64 {
            for bp in [0_u32, 500, 1_200, 1_800, 2_800] {
                let paid = Money::from_paise(paise);
                let rate = TaxRate::from_basis_points(bp).expect("a rate");
                let credit = input_credit(paid, rate).expect("credit");
                assert!(credit <= paid, "{paise} at {bp}bp gave {credit:?}");
                assert!(!credit.is_negative());
            }
        }
    }

    #[test]
    fn a_weekly_template_is_due_seven_days_later() {
        let day = BusinessDay::from_ymd(2026, 8, 9);
        assert_eq!(next_due(day, Every::Week), BusinessDay::from_ymd(2026, 8, 16));
    }

    /// **Rent due on the 31st.** Sliding to the 1st would move rent out of the
    /// month it is rent for; adding 31 days would skip February.
    #[test]
    fn a_monthly_template_lands_on_the_same_date_or_the_last_one_there_is() {
        assert_eq!(
            next_due(BusinessDay::from_ymd(2026, 1, 31), Every::Month),
            BusinessDay::from_ymd(2026, 2, 28),
        );
        assert_eq!(
            next_due(BusinessDay::from_ymd(2028, 1, 31), Every::Month),
            BusinessDay::from_ymd(2028, 2, 29),
            "2028 is a leap year",
        );
        assert_eq!(
            next_due(BusinessDay::from_ymd(2026, 3, 31), Every::Month),
            BusinessDay::from_ymd(2026, 4, 30),
        );
        assert_eq!(
            next_due(BusinessDay::from_ymd(2026, 12, 15), Every::Month),
            BusinessDay::from_ymd(2027, 1, 15),
            "and it crosses a year",
        );
    }

    /// Twelve months of rent land on twelve different months — the property
    /// that "add 30 days" quietly breaks.
    #[test]
    fn a_year_of_a_monthly_template_hits_every_month_once() {
        let mut day = BusinessDay::from_ymd(2026, 1, 31);
        let mut months = Vec::new();
        for _ in 0..12 {
            day = next_due(day, Every::Month);
            months.push(day.to_ymd().1);
        }
        assert_eq!(months, [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 1]);
    }
}
