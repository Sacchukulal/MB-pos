//! **What a customer owes, how old it is, and whether they may owe more.**
//!
//! Scope 5.1, 5.2, 5.3. The owner renamed this from "khata" on 2026-08-08.
//!
//! # An account, not a balance
//!
//! v1 kept `credit_balance REAL` on the customer row, beside the payments that
//! made it — two sources of truth for what somebody owes, one of them a
//! floating-point number. mb-db already refuses to store a balance
//! ([`MoneyRepo::customer_balance`] is a `SUM` every time); this module is the
//! other half: **everything a screen wants to know about an account is derived
//! here, from the movements, by a pure function.**
//!
//! Nothing in this file touches a database or a clock. That is what makes the
//! ageing arithmetic — which is the part an owner will argue with — provable.
//!
//! # The part that is actually hard: ageing
//!
//! "He owes me ₹4,200" is not useful. "He has owed me ₹4,200 for 74 days" is
//! what an owner acts on, and getting there means deciding **which sale a
//! repayment paid off**. There is no invoice reference on a repayment in a
//! shop like this — somebody hands over ₹500 against a running account — so
//! this module applies **oldest first (FIFO)**, which is what every shopkeeper
//! means and what every accountant expects.

use serde::{Deserialize, Serialize};

use crate::businessday::BusinessDay;
use crate::money::{Money, MoneyError};

/// One thing that happened to an account.
///
/// Deliberately not an enum of rows-in-tables: a sale, a repayment, an opening
/// balance and a write-off all move the same number, and the day a fifth kind
/// appears it should not need a new function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Movement {
    pub day: BusinessDay,
    pub kind: MovementKind,
    /// **Always positive.** The direction is the kind's business, not the
    /// sign's — a negative "amount" in a ledger is how a subtraction becomes an
    /// addition in somebody's report six months later.
    pub amount: Money,
    /// The bill number, the repayment's reference, or the adjustment's reason.
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovementKind {
    /// A bill settled on credit. Increases what is owed.
    Sale,
    /// Money handed over against the account. Decreases it.
    Repayment,
    /// What was owed before this product existed. Increases it.
    Opening,
    /// A correction or a write-off, with a reason and a name against it.
    /// `increases` says which way, because both directions are real: a
    /// forgotten sale added later, and money written off as unrecoverable.
    Adjustment { increases: bool },
}

impl MovementKind {
    /// Does this add to what the customer owes?
    #[must_use]
    pub const fn adds(self) -> bool {
        match self {
            MovementKind::Sale | MovementKind::Opening => true,
            MovementKind::Repayment => false,
            MovementKind::Adjustment { increases } => increases,
        }
    }
}

/// What is owed, from the movements and nothing else.
pub fn balance(movements: &[Movement]) -> Result<Money, MoneyError> {
    movements.iter().try_fold(Money::ZERO, |running, m| {
        if m.kind.adds() { running.add(m.amount) } else { running.sub(m.amount) }
    })
}

/// How old the outstanding money is — scope 5.1's ageing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ageing {
    /// Owed for less than 30 days.
    pub current: Money,
    pub days_30: Money,
    pub days_60: Money,
    /// Ninety days and older. The bucket an owner phones about.
    pub days_90: Money,
    /// How long the oldest unpaid money has been outstanding. `None` when
    /// nothing is owed.
    pub oldest_days: Option<i32>,
}

impl Ageing {
    pub fn total(&self) -> Result<Money, MoneyError> {
        Money::try_sum([self.current, self.days_30, self.days_60, self.days_90])
    }
}

/// Age the account **oldest first**.
///
/// Repayments are applied to the oldest outstanding sale, then the next, and
/// the remainder — money handed over against nothing yet owed — sits as a
/// credit that reduces the newest bucket. That last case is real: a customer
/// who overpays is in credit, and the account then shows a negative balance
/// rather than pretending the money was not received.
///
/// `today` is passed in, never read from a clock, because ageing has to be
/// testable across a month boundary and because D5 says the business day is
/// stamped, not derived.
pub fn ageing(movements: &[Movement], today: BusinessDay) -> Result<Ageing, MoneyError> {
    // Oldest first, so a repayment meets the sales in the order a shopkeeper
    // would clear them.
    let mut owed: Vec<(BusinessDay, Money)> = Vec::new();
    let mut sorted: Vec<&Movement> = movements.iter().collect();
    sorted.sort_by_key(|m| m.day.days_since_epoch());

    let mut credit = Money::ZERO;
    for movement in sorted {
        if movement.kind.adds() {
            let mut amount = movement.amount;
            // Anything paid in advance is used up before the new debt lands.
            if credit.is_positive() {
                let used = if credit < amount { credit } else { amount };
                credit = credit.sub(used)?;
                amount = amount.sub(used)?;
            }
            if amount.is_positive() {
                owed.push((movement.day, amount));
            }
        } else {
            let mut left = movement.amount;
            while left.is_positive() {
                let Some((_, oldest)) = owed.first_mut() else {
                    // Paid more than was owed: the shop is holding money.
                    credit = credit.add(left)?;
                    break;
                };
                let taken = if *oldest < left { *oldest } else { left };
                *oldest = oldest.sub(taken)?;
                left = left.sub(taken)?;
                if !owed[0].1.is_positive() {
                    owed.remove(0);
                }
            }
        }
    }

    let mut buckets = Ageing::default();
    let mut oldest_days: Option<i32> = None;
    for (day, amount) in &owed {
        let age = today.days_since_epoch() - day.days_since_epoch();
        oldest_days = Some(oldest_days.map_or(age, |known: i32| known.max(age)));
        let bucket = match age {
            ..30 => &mut buckets.current,
            30..60 => &mut buckets.days_30,
            60..90 => &mut buckets.days_60,
            _ => &mut buckets.days_90,
        };
        *bucket = bucket.add(*amount)?;
    }
    // Money held on account shows as a negative "current" rather than
    // vanishing — the balance and the ageing must agree.
    if credit.is_positive() {
        buckets.current = buckets.current.sub(credit)?;
    }
    buckets.oldest_days = oldest_days;
    Ok(buckets)
}

/// A statement over a date range — scope 5.3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    pub from: BusinessDay,
    pub to: BusinessDay,
    /// What was owed the day before `from`.
    pub opening: Money,
    pub rows: Vec<Movement>,
    pub closing: Money,
}

/// Everything between two days, with the balance either side of it.
///
/// **opening + movements = closing, exactly**, which is the only property of a
/// statement that matters and is asserted for any range.
pub fn statement(
    movements: &[Movement],
    from: BusinessDay,
    to: BusinessDay,
) -> Result<Statement, MoneyError> {
    let before: Vec<Movement> = movements
        .iter()
        .filter(|m| m.day.days_since_epoch() < from.days_since_epoch())
        .cloned()
        .collect();
    let mut rows: Vec<Movement> = movements
        .iter()
        .filter(|m| {
            let day = m.day.days_since_epoch();
            day >= from.days_since_epoch() && day <= to.days_since_epoch()
        })
        .cloned()
        .collect();
    rows.sort_by_key(|m| m.day.days_since_epoch());

    let opening = balance(&before)?;
    let closing = rows.iter().try_fold(opening, |running, m| {
        if m.kind.adds() { running.add(m.amount) } else { running.sub(m.amount) }
    })?;

    Ok(Statement { from, to, opening, rows, closing })
}

/// Whether this bill may go on the account — scope 5.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitVerdict {
    /// No limit set, or comfortably inside it. **A customer with no limit is
    /// not a customer with a limit of zero.**
    Fine,
    /// Inside the limit but within a tenth of it — worth saying, not worth
    /// stopping. A cashier who is told at 4,900 of 5,000 can ask; one who is
    /// told at 5,001 has already served the food.
    Close,
    /// Over. Whether this stops the sale is the caller's decision (D18: the
    /// policy is checked by the caller), and overriding it needs a permission
    /// and writes an audit row.
    Over,
}

/// What the sale would do to the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Headroom {
    pub limit: Option<Money>,
    pub balance: Money,
    pub after: Money,
    pub verdict: LimitVerdict,
}

/// Work out where this bill leaves the account.
pub fn headroom(balance: Money, limit: Option<Money>, bill: Money) -> Result<Headroom, MoneyError> {
    let after = balance.add(bill)?;
    let verdict = match limit {
        None => LimitVerdict::Fine,
        Some(limit) if after > limit => LimitVerdict::Over,
        // A tenth of the limit, computed by the one rounding path there is.
        Some(limit) => {
            let cushion = limit.mul_ratio(1, 10)?;
            if after.add(cushion)? > limit { LimitVerdict::Close } else { LimitVerdict::Fine }
        }
    };
    Ok(Headroom { limit, balance, after, verdict })
}

/// **The identity of a customer in this market is a phone number**, so two rows
/// with the same one are two balances for one person.
///
/// Compared as the last ten digits: `+91 98765 43210`, `098765-43210` and
/// `9876543210` are one customer. **What was typed is what is stored** — the
/// same argument `cart::normalise_note` makes about a kitchen note, and
/// `mb_core::table` about a table's label.
///
/// Returns `None` for something that is not a phone number at all, which is
/// how a caller tells "no phone given" from "that phone".
#[must_use]
pub fn phone_key(phone: &str) -> Option<String> {
    let digits: String = phone.chars().filter(char::is_ascii_digit).collect();
    if digits.len() < 10 {
        return None;
    }
    Some(digits[digits.len() - 10..].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(n: i32) -> BusinessDay {
        BusinessDay::from_days_since_epoch(20_000 + n)
    }

    fn sale(n: i32, rupees: i64) -> Movement {
        Movement {
            day: day(n),
            kind: MovementKind::Sale,
            amount: Money::from_rupees(rupees).expect("money"),
            note: "bill".to_owned(),
        }
    }

    fn repaid(n: i32, rupees: i64) -> Movement {
        Movement {
            day: day(n),
            kind: MovementKind::Repayment,
            amount: Money::from_rupees(rupees).expect("money"),
            note: "cash".to_owned(),
        }
    }

    #[test]
    fn the_balance_is_the_sum_of_the_movements() {
        let movements = vec![sale(0, 500), sale(10, 300), repaid(20, 200)];
        assert_eq!(
            balance(&movements).expect("balance"),
            Money::from_rupees(600).expect("money"),
        );
    }

    /// An adjustment goes both ways, and both are real: a sale somebody forgot
    /// to put on the account, and money written off.
    #[test]
    fn an_adjustment_can_go_either_way() {
        let up = Movement {
            day: day(1),
            kind: MovementKind::Adjustment { increases: true },
            amount: Money::from_rupees(100).expect("money"),
            note: "missed bill".to_owned(),
        };
        let down = Movement {
            kind: MovementKind::Adjustment { increases: false },
            ..up.clone()
        };
        assert_eq!(balance(&[up]).expect("b"), Money::from_rupees(100).expect("m"));
        assert_eq!(balance(&[down]).expect("b"), Money::from_rupees(-100).expect("m"));
    }

    /// **Oldest first.** ₹500 handed over against a ₹300 sale from January and
    /// a ₹400 sale from March clears January entirely and ₹200 of March.
    #[test]
    fn a_repayment_clears_the_oldest_debt_first() {
        let movements = vec![sale(0, 300), sale(60, 400), repaid(70, 500)];
        let aged = ageing(&movements, day(75)).expect("ageing");

        assert_eq!(aged.total().expect("total"), Money::from_rupees(200).expect("m"));
        // What is left is the March sale, 15 days old — NOT the January one.
        assert_eq!(aged.current, Money::from_rupees(200).expect("m"));
        assert_eq!(aged.days_90, Money::ZERO);
        assert_eq!(aged.oldest_days, Some(15));
    }

    /// The buckets, and the boundary. 30 days old belongs to 30-60, not to
    /// "current" — an off-by-one here is an owner phoning the wrong customer.
    #[test]
    fn the_buckets_land_on_the_right_side_of_their_boundaries() {
        let movements = vec![sale(0, 100), sale(29, 200), sale(30, 300), sale(59, 400), sale(60, 500)];
        let aged = ageing(&movements, day(89)).expect("ageing");

        assert_eq!(aged.days_90, Money::ZERO, "nothing is 90 days old yet");
        // Ages, in the order the sales were made: 89, 60, 59, 30, 29 days.
        assert_eq!(aged.days_60, Money::from_rupees(300).expect("m"), "the 89 and 60 day old money");
        assert_eq!(aged.days_30, Money::from_rupees(700).expect("m"), "the 59 and 30 day old money");
        assert_eq!(aged.current, Money::from_rupees(500).expect("m"), "29 days old");
        assert_eq!(aged.oldest_days, Some(89));

        // One more day and the oldest crosses into 90+.
        let later = ageing(&movements, day(90)).expect("ageing");
        assert_eq!(later.days_90, Money::from_rupees(100).expect("m"));
    }

    /// A customer who overpays is in credit, and the account says so rather
    /// than losing the money.
    #[test]
    fn money_paid_in_advance_shows_as_credit_and_pays_the_next_bill() {
        let movements = vec![sale(0, 100), repaid(1, 500)];
        let aged = ageing(&movements, day(2)).expect("ageing");
        assert_eq!(aged.total().expect("total"), Money::from_rupees(-400).expect("m"));
        assert_eq!(
            balance(&movements).expect("balance"),
            aged.total().expect("total"),
            "the ageing and the balance must agree, including in credit",
        );

        // And the next sale draws it down instead of ageing.
        let with_sale = vec![sale(0, 100), repaid(1, 500), sale(2, 300)];
        let aged = ageing(&with_sale, day(3)).expect("ageing");
        assert_eq!(aged.total().expect("total"), Money::from_rupees(-100).expect("m"));
    }

    /// **The property that makes a statement a statement.**
    #[test]
    fn opening_plus_movements_equals_closing_for_any_range() {
        let movements = vec![
            sale(0, 500),
            repaid(5, 200),
            sale(10, 300),
            repaid(40, 100),
            sale(50, 250),
        ];

        for from in 0..55 {
            for to in from..60 {
                let s = statement(&movements, day(from), day(to)).expect("statement");
                let moved = s.rows.iter().try_fold(s.opening, |running, m| {
                    if m.kind.adds() { running.add(m.amount) } else { running.sub(m.amount) }
                });
                assert_eq!(moved.expect("sum"), s.closing, "range {from}..{to}");
            }
        }

        // And the whole range closes at the balance.
        let all = statement(&movements, day(0), day(59)).expect("statement");
        assert_eq!(all.closing, balance(&movements).expect("balance"));
        assert_eq!(all.opening, Money::ZERO);
    }

    #[test]
    fn a_limit_warns_before_it_blocks_and_no_limit_is_not_zero() {
        let limit = Money::from_rupees(5_000).expect("m");
        let owed = Money::from_rupees(4_200).expect("m");

        // Comfortably inside.
        assert_eq!(
            headroom(owed, Some(limit), Money::from_rupees(100).expect("m"))
                .expect("headroom")
                .verdict,
            LimitVerdict::Fine,
        );
        // Within a tenth of it.
        assert_eq!(
            headroom(owed, Some(limit), Money::from_rupees(600).expect("m"))
                .expect("headroom")
                .verdict,
            LimitVerdict::Close,
        );
        // Over.
        let over = headroom(owed, Some(limit), Money::from_rupees(1_000).expect("m"))
            .expect("headroom");
        assert_eq!(over.verdict, LimitVerdict::Over);
        assert_eq!(over.after, Money::from_rupees(5_200).expect("m"));

        // **No limit is not a limit of zero.**
        assert_eq!(
            headroom(owed, None, Money::from_rupees(90_000).expect("m"))
                .expect("headroom")
                .verdict,
            LimitVerdict::Fine,
        );
    }

    #[test]
    fn one_phone_number_is_one_customer_however_it_is_typed() {
        let key = phone_key("9876543210");
        assert_eq!(phone_key("+91 98765 43210"), key);
        assert_eq!(phone_key("098765-43210"), key);
        assert_eq!(phone_key("  +91-9876543210  "), key);

        // Not a phone number at all.
        assert_eq!(phone_key("12345"), None);
        assert_eq!(phone_key(""), None);
    }
}
