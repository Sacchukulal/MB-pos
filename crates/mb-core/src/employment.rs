//! **What a person is owed, and how much leave they have left.**
//!
//! Scope 9.7, 9.9, 9.10, 9.11. P28.
//!
//! # Nothing here touches a database or a clock
//!
//! Same shape as [`crate::credit`], and for the same reason: the arithmetic an
//! owner will argue with is the arithmetic that has to be provable. A payroll
//! figure somebody cannot check by hand is a payroll figure they will not
//! trust, and payroll they do not trust is payroll they keep doing in a
//! notebook — which is the notebook this product exists to replace.
//!
//! So every function in this file is pure. The database supplies the rows, the
//! caller supplies the day, and this decides what the numbers are.
//!
//! # Days are counted in HALVES
//!
//! [`HalfDays`] is an integer count of half-days: a full day is 2, a half day
//! is 1, and those are the only two units a restaurant actually grants. It is
//! the same argument as paise (D2) — a fraction stored as a float is a
//! fraction that will not add up, and `0.5 + 0.5 + 0.5 = 1.4999999` in a leave
//! balance is an argument with an employee that nobody can win.
//!
//! # The three balances are all sums of ledgers
//!
//! Leave, advances, and what a run owes. Not one of them is stored anywhere.
//! The schema has no `leave_balance` column and no `outstanding` column,
//! because a stored total is a number that will one day disagree with its own
//! history and there will be nothing to check it against. Same rule as credit
//! (D120) and stock (D127).

use serde::{Deserialize, Serialize};

use crate::businessday::BusinessDay;
use crate::money::{Money, MoneyError};

// ---------------------------------------------------------------------------
// Half-days
// ---------------------------------------------------------------------------

/// A count of half-days. A full day is 2.
///
/// A newtype rather than a bare `i32` for the same reason [`Money`] is one: the
/// unit is the thing that gets forgotten, and "12" meaning six days is exactly
/// the kind of quiet halving that survives review.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct HalfDays(i32);

impl HalfDays {
    pub const ZERO: HalfDays = HalfDays(0);

    #[must_use]
    pub const fn new(halves: i32) -> Self {
        HalfDays(halves)
    }

    #[must_use]
    pub const fn from_days(days: i32) -> Self {
        HalfDays(days * 2)
    }

    #[must_use]
    pub const fn halves(self) -> i32 {
        self.0
    }

    /// Whole days, rounded DOWN, plus whether there is a half left over.
    ///
    /// Returned together rather than as two functions, because every caller
    /// that wants one wants the other in the next line — and a caller that
    /// takes only the whole days has silently thrown away half a day of
    /// somebody's leave.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "halves to whole days IS a flooring division, and the \n                  remainder is returned beside it — nothing is discarded"
    )]
    pub const fn split(self) -> (i32, bool) {
        (self.0 / 2, self.0 % 2 != 0)
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// "3 days", "3½ days", "half a day".
    ///
    /// Written here rather than in React, because it is the SAME sentence on
    /// the screen, on a payslip and in a refusal — and three copies of a
    /// sentence is three chances for one of them to say something different
    /// (§6: one place turns a machine state into words).
    #[must_use]
    pub fn say(self) -> String {
        let negative = self.0 < 0;
        let (days, half) = HalfDays(self.0.abs()).split();
        let body = match (days, half) {
            (0, false) => "none".to_owned(),
            (0, true) => "half a day".to_owned(),
            (1, false) => "1 day".to_owned(),
            (1, true) => "1½ days".to_owned(),
            (d, false) => format!("{d} days"),
            (d, true) => format!("{d}½ days"),
        };
        if negative && !body.starts_with("none") {
            format!("−{body}")
        } else {
            body
        }
    }
}

impl std::ops::Add for HalfDays {
    type Output = HalfDays;
    fn add(self, other: HalfDays) -> HalfDays {
        HalfDays(self.0.saturating_add(other.0))
    }
}

impl std::ops::Sub for HalfDays {
    type Output = HalfDays;
    fn sub(self, other: HalfDays) -> HalfDays {
        HalfDays(self.0.saturating_sub(other.0))
    }
}

impl std::iter::Sum for HalfDays {
    fn sum<I: Iterator<Item = HalfDays>>(iter: I) -> HalfDays {
        iter.fold(HalfDays::ZERO, |a, b| a + b)
    }
}

// ---------------------------------------------------------------------------
// Leave
// ---------------------------------------------------------------------------

/// One row of the leave ledger.
///
/// Four kinds, and the sign is the kind's business — see [`LeaveKind`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaveEntry {
    pub day: BusinessDay,
    pub kind: LeaveKind,
    /// **Signed**, and this is the one place in the module where that is true:
    /// the sign is what makes the balance a `sum` rather than a case analysis.
    /// The database CHECKs tie each kind to its direction, so a row that
    /// arrives here with the wrong sign never existed.
    pub half_days: HalfDays,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaveKind {
    /// The entitlement, granted. Positive.
    Accrued,
    /// A day off that was approved. Negative.
    Taken,
    /// A correction, with a reason and a name. Either direction.
    Adjusted,
    /// What was not used by the end of the year. Negative.
    Lapsed,
}

/// One person's balance in one leave type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaveBalance {
    pub accrued: HalfDays,
    pub taken: HalfDays,
    pub adjusted: HalfDays,
    pub lapsed: HalfDays,
    /// The sum of the four. **Never stored anywhere** — computed here, every
    /// time, from the rows.
    pub left: HalfDays,
}

/// **The balance is the sum of the ledger.** That is the whole function, and
/// the fact that it is this short is the point of the design.
#[must_use]
pub fn leave_balance(entries: &[LeaveEntry]) -> LeaveBalance {
    let of = |want: LeaveKind| -> HalfDays {
        entries
            .iter()
            .filter(|e| e.kind == want)
            .map(|e| e.half_days)
            .sum()
    };
    let accrued = of(LeaveKind::Accrued);
    let taken = of(LeaveKind::Taken);
    let adjusted = of(LeaveKind::Adjusted);
    let lapsed = of(LeaveKind::Lapsed);
    LeaveBalance {
        accrued,
        taken,
        adjusted,
        lapsed,
        left: accrued + taken + adjusted + lapsed,
    }
}

/// Whether two leave requests cover any of the same days.
///
/// Inclusive at both ends, because a shop that says "14th to 16th" means three
/// days including the 16th, and every off-by-one in a leave system is this one.
#[must_use]
pub fn overlaps(a: (BusinessDay, BusinessDay), b: (BusinessDay, BusinessDay)) -> bool {
    a.0 <= b.1 && b.0 <= a.1
}

// ---------------------------------------------------------------------------
// Salary
// ---------------------------------------------------------------------------

/// How somebody is paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// A fixed amount per month, whatever the days — the ordinary arrangement
    /// for a manager or a cook on the payroll.
    Monthly,
    /// So much per day worked. A helper who comes in when the shop is busy.
    Daily,
    /// So much per hour worked. Part-time and casual staff.
    Hourly,
}

/// What a person was paid under, from when. Effective-dated (D52's argument,
/// applied to a salary rather than to a price).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Structure {
    pub effective_from: BusinessDay,
    pub basis: Basis,
    /// Per month, per day worked, or per hour worked, by `basis`.
    pub amount: Money,
    /// Always positive; `kind` carries the direction.
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub kind: ComponentKind,
    pub amount: Money,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Allowance,
    Deduction,
}

/// **The structure that applied on a day.**
///
/// The LATEST one whose `effective_from` is on or before that day. This one
/// function is what makes T5 true: a raise dated the 1st of this month cannot
/// reach last month's run, because last month's run asks this question with
/// last month's days and gets the old row.
///
/// Returns `None` when the person had no salary structure at all on that day —
/// which is not an error. Somebody who joined on the 15th has no structure on
/// the 14th, and a run over that month must simply not pay them for it.
#[must_use]
pub fn structure_on(structures: &[Structure], day: BusinessDay) -> Option<&Structure> {
    structures
        .iter()
        .filter(|s| s.effective_from <= day)
        .max_by_key(|s| s.effective_from)
}

/// What one person worked, and was away, over one payroll period.
///
/// Gathered by the caller from `attendance` and the leave ledger, because
/// deciding WHICH rows belong to the period is a database question and
/// deciding what they are WORTH is this module's.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Worked {
    /// Days present, in halves. A half-day's attendance is a half-day's pay on
    /// a daily basis.
    pub days: HalfDays,
    /// Total minutes clocked, for an hourly basis. Breaks already subtracted.
    pub minutes: i64,
    /// Approved leave on a type whose `is_paid` is false. Deducted.
    pub unpaid: HalfDays,
    /// **The days the period contains** — the denominator for a monthly
    /// basis's unpaid-leave deduction.
    ///
    /// Passed in rather than computed, because "how many days is a month" has
    /// three defensible answers (calendar days, working days, a fixed 30) and
    /// the shop has to be the one that picks. Calendar days is what this
    /// product uses and D148 says why.
    pub period_days: i32,
}

/// One person's line in a run, with every step of the arithmetic kept.
///
/// Every figure is a column so a payslip can print it and an owner can add it
/// up by hand. That is not decoration: it is the difference between payroll
/// somebody trusts and payroll they redo in a notebook to check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayLine {
    pub basis: Basis,
    pub basis_amount: Money,
    pub days_worked: HalfDays,
    pub minutes_worked: i64,
    pub unpaid: HalfDays,
    /// What the basis earned, before anything is added or taken off.
    pub earned: Money,
    pub allowances: Money,
    pub deductions: Money,
    pub unpaid_leave_deduction: Money,
    pub advance_recovered: Money,
    /// `earned + allowances − deductions − unpaid − advance`, floored at zero.
    pub net: Money,
}

/// **Compute one person's line.**
///
/// The arithmetic, in the order a payslip prints it:
///
/// | basis | earned |
/// |---|---|
/// | monthly | the full amount, then a deduction for unpaid days |
/// | daily | amount × days worked |
/// | hourly | amount × minutes ÷ 60 |
///
/// then `+ allowances − deductions − advance recovered`, floored at zero.
///
/// # Why unpaid leave is a DEDUCTION and not a smaller `earned`
///
/// On a monthly basis it could be either, and the two produce the same net. It
/// is a deduction because that is what the payslip has to show: an employee
/// asked to accept less than their agreed salary is owed the sentence "your
/// salary is ₹18,000 and two days were unpaid", not a smaller number with no
/// explanation. The line items are the explanation.
///
/// On a daily or hourly basis it is **not deducted at all**, and that is not an
/// oversight: a day not worked was never earned, so deducting it would take the
/// day off twice. The test that would catch a change here is
/// `an_unpaid_day_is_taken_off_a_monthly_salary_once_and_a_daily_one_never`.
///
/// # Why the floor is zero
///
/// An advance bigger than the month's pay would otherwise produce a negative
/// net, and a payslip that says −₹400 is a payslip that gets argued about at
/// the counter. What is not recovered stays outstanding on the advance and
/// comes off the next run — which is what the caller does with
/// [`recover_advances`].
pub fn pay_line(
    structure: &Structure,
    worked: &Worked,
    advance_recovered: Money,
) -> Result<PayLine, MoneyError> {
    let earned = match structure.basis {
        Basis::Monthly => structure.amount,
        // ONE `mul_ratio`, not a multiply followed by a divide. `days` is in
        // HALVES, so the ratio is halves-over-two and the rounding happens
        // once — two operations round twice and lose a paisa on half the
        // shifts in the shop.
        Basis::Daily => structure.amount.mul_ratio(i64::from(worked.days.halves()), 2)?,
        Basis::Hourly => structure.amount.mul_ratio(worked.minutes, 60)?,
    };

    let allowances = sum_components(structure, ComponentKind::Allowance)?;
    let deductions = sum_components(structure, ComponentKind::Deduction)?;

    // **Only a monthly salary has unpaid days deducted.** See the note above.
    //
    // Again ONE ratio: the month over (period days × 2), times the unpaid
    // halves. Computing a day rate first and then multiplying would round
    // twice, and on a 31-day month the two answers differ — which is the kind
    // of discrepancy an employee notices and nobody can explain.
    let unpaid_leave_deduction = match structure.basis {
        Basis::Monthly if worked.period_days > 0 => structure.amount.mul_ratio(
            i64::from(worked.unpaid.halves()),
            i64::from(worked.period_days) * 2,
        )?,
        Basis::Monthly | Basis::Daily | Basis::Hourly => Money::ZERO,
    };

    let plus = earned.add(allowances)?;
    let minus = deductions
        .add(unpaid_leave_deduction)?
        .add(advance_recovered)?;
    let net = if minus >= plus {
        Money::ZERO
    } else {
        plus.sub(minus)?
    };

    Ok(PayLine {
        basis: structure.basis,
        basis_amount: structure.amount,
        days_worked: worked.days,
        minutes_worked: worked.minutes,
        unpaid: worked.unpaid,
        earned,
        allowances,
        deductions,
        unpaid_leave_deduction,
        advance_recovered,
        net,
    })
}

fn sum_components(structure: &Structure, want: ComponentKind) -> Result<Money, MoneyError> {
    Money::try_sum(
        structure
            .components
            .iter()
            .filter(|c| c.kind == want)
            .map(|c| c.amount),
    )
}

// ---------------------------------------------------------------------------
// Advances
// ---------------------------------------------------------------------------

/// An advance, and what has already been taken back off it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Advance {
    pub id: String,
    pub day: BusinessDay,
    pub amount: Money,
    /// Over how many runs it is meant to come back. 1 is "all of it next
    /// month", which is the common case.
    pub instalments: i32,
    /// The sum of `advance_recoveries` for this advance. **Passed in, not
    /// stored on the advance** — for the third time in this module, a stored
    /// total is a total that will one day be wrong.
    pub recovered: Money,
}

impl Advance {
    /// What is still owed on it.
    pub fn outstanding(&self) -> Result<Money, MoneyError> {
        if self.recovered >= self.amount {
            Ok(Money::ZERO)
        } else {
            self.amount.sub(self.recovered)
        }
    }

    /// One instalment of it, or whatever is left if that is less.
    fn instalment(&self) -> Result<Money, MoneyError> {
        let outstanding = self.outstanding()?;
        if self.instalments <= 1 {
            return Ok(outstanding);
        }
        let share = self.amount.mul_ratio(1, i64::from(self.instalments))?;
        Ok(if share > outstanding { outstanding } else { share })
    }
}

/// What one payroll run takes back, advance by advance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recovery {
    pub advance_id: String,
    pub amount: Money,
}

/// **Decide what this run recovers, oldest advance first.**
///
/// Capped at `available` — a run can never recover more than the person earned,
/// because the alternative is a payslip with a negative net and a conversation
/// at the counter. What is not recovered stays outstanding and comes off the
/// next run, which is exactly what an instalment arrangement means anyway.
///
/// Oldest first for the same reason [`crate::credit`] ages FIFO: it is what a
/// shopkeeper means, and any other order needs explaining.
pub fn recover_advances(
    advances: &[Advance],
    available: Money,
) -> Result<Vec<Recovery>, MoneyError> {
    let mut ordered: Vec<&Advance> = advances.iter().collect();
    ordered.sort_by_key(|a| (a.day, a.id.clone()));

    let mut left = available;
    let mut out = Vec::new();
    for advance in ordered {
        if left.is_zero() {
            break;
        }
        let want = advance.instalment()?;
        if want.is_zero() {
            continue;
        }
        let take = if want > left { left } else { want };
        out.push(Recovery {
            advance_id: advance.id.clone(),
            amount: take,
        });
        left = left.sub(take)?;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Attendance
// ---------------------------------------------------------------------------

/// How a person's day compares to what was expected of them.
///
/// The whole point of a roster: without one, attendance is a list of times with
/// nothing to measure against, which is what v1's staff screen amounted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DayVerdict {
    /// On time, or within the grace period.
    Present,
    /// Turned up, but after the grace period.
    Late { by_minutes: i64 },
    /// Left before the shift ended, by more than the grace period.
    LeftEarly { by_minutes: i64 },
    /// Rostered, did not come, and has no approved leave. **The one a shop
    /// acts on.**
    Absent,
    /// Rostered off, or on approved leave. Not an absence.
    Away,
    /// Worked without being rostered. Not a problem — it is the ordinary case
    /// in a shop that has not built a roster — but it is not "on time" either,
    /// because there was nothing to be on time for.
    Unrostered,
}

/// Judge one person's one day.
///
/// `expected_start` and `expected_end` are minutes from midnight; `actual_*`
/// are the same, derived by the caller from the stamps. `None` for an actual
/// means they did not clock that end.
///
/// **Late beats left-early** when a day is both, because a shop chases the
/// first one and the second is usually a consequence of it.
#[must_use]
pub fn judge_day(
    expected: Option<(i64, i64)>,
    actual_start: Option<i64>,
    actual_end: Option<i64>,
    on_approved_leave: bool,
    grace_minutes: i64,
) -> DayVerdict {
    let Some((expected_start, expected_end)) = expected else {
        return if actual_start.is_some() {
            DayVerdict::Unrostered
        } else {
            // Not rostered and did not come. Nobody said either way, so there
            // is nothing to report — a shop with no roster must not wake up to
            // a screen full of absences it never asked about.
            DayVerdict::Away
        };
    };

    let Some(start) = actual_start else {
        return if on_approved_leave {
            DayVerdict::Away
        } else {
            DayVerdict::Absent
        };
    };

    // On leave AND present is not a contradiction worth an error — somebody
    // came in on their day off. It is simply not an absence.
    let late_by = start - expected_start;
    if late_by > grace_minutes {
        return DayVerdict::Late {
            by_minutes: late_by,
        };
    }

    if let Some(end) = actual_end {
        let early_by = expected_end - end;
        if early_by > grace_minutes {
            return DayVerdict::LeftEarly {
                by_minutes: early_by,
            };
        }
    }

    DayVerdict::Present
}

/// Minutes between two stamps on the clock face, **allowing for a wrap past
/// midnight**.
///
/// A night shift is `start_minute > end_minute` in `shift_patterns`, and the
/// arithmetic that forgets it produces a shift of minus six hours — which
/// would then be worth minus six hours of pay to an hourly employee.
#[must_use]
pub fn minutes_between(start_minute: i64, end_minute: i64) -> i64 {
    if end_minute >= start_minute {
        end_minute - start_minute
    } else {
        (24 * 60) - start_minute + end_minute
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(n: i32) -> BusinessDay {
        BusinessDay::from_days_since_epoch(20_600 + n)
    }

    fn rupees(n: i64) -> Money {
        Money::from_paise(n * 100)
    }

    // -- half-days ---------------------------------------------------------

    #[test]
    fn a_half_day_is_half_a_day_and_says_so() {
        assert_eq!(HalfDays::from_days(3).halves(), 6);
        assert_eq!(HalfDays::new(7).split(), (3, true));
        assert_eq!(HalfDays::new(7).say(), "3½ days");
        assert_eq!(HalfDays::new(1).say(), "half a day");
        assert_eq!(HalfDays::new(2).say(), "1 day");
        assert_eq!(HalfDays::ZERO.say(), "none");
        assert_eq!(HalfDays::new(-3).say(), "−1½ days");
    }

    /// **Three halves add up to exactly one and a half days.**
    ///
    /// The whole reason this is an integer type: the float version of this
    /// assertion is the one that fails in the fourth decimal place and turns
    /// into an argument with an employee about a quarter of an hour.
    #[test]
    fn halves_add_up_exactly() {
        let total: HalfDays = [HalfDays::new(1), HalfDays::new(1), HalfDays::new(1)]
            .into_iter()
            .sum();
        assert_eq!(total, HalfDays::new(3));
        assert_eq!(total.say(), "1½ days");
    }

    // -- leave -------------------------------------------------------------

    fn entry(kind: LeaveKind, halves: i32) -> LeaveEntry {
        LeaveEntry {
            day: day(0),
            kind,
            half_days: HalfDays::new(halves),
            note: String::new(),
        }
    }

    #[test]
    fn the_balance_is_the_sum_of_the_ledger() {
        let ledger = vec![
            entry(LeaveKind::Accrued, 24),
            entry(LeaveKind::Taken, -4),
            entry(LeaveKind::Taken, -1),
            entry(LeaveKind::Adjusted, 2),
            entry(LeaveKind::Lapsed, -6),
        ];
        let balance = leave_balance(&ledger);
        assert_eq!(balance.accrued, HalfDays::new(24));
        assert_eq!(balance.taken, HalfDays::new(-5));
        assert_eq!(balance.adjusted, HalfDays::new(2));
        assert_eq!(balance.lapsed, HalfDays::new(-6));
        // 24 − 5 + 2 − 6 = 15 halves = 7½ days.
        assert_eq!(balance.left, HalfDays::new(15));
        assert_eq!(balance.left.say(), "7½ days");
    }

    /// **The property, not an example.** A year of rows in any order sums to
    /// the same balance, and the balance equals the four parts added up — so
    /// there is no arrangement of a ledger that makes the total disagree with
    /// itself.
    #[test]
    fn the_balance_never_disagrees_with_its_own_parts() {
        let mut ledger = Vec::new();
        for n in 0..200_i32 {
            let kind = match n % 4 {
                0 => LeaveKind::Accrued,
                1 => LeaveKind::Taken,
                2 => LeaveKind::Adjusted,
                _ => LeaveKind::Lapsed,
            };
            // Deterministic, and covers both directions on `adjusted`.
            let size = 1 + (n % 5);
            let halves = match kind {
                LeaveKind::Accrued => size,
                LeaveKind::Taken | LeaveKind::Lapsed => -size,
                LeaveKind::Adjusted => {
                    if n % 8 < 4 {
                        size
                    } else {
                        -size
                    }
                }
            };
            ledger.push(entry(kind, halves));
        }

        let balance = leave_balance(&ledger);
        assert_eq!(
            balance.left,
            balance.accrued + balance.taken + balance.adjusted + balance.lapsed
        );

        // And it does not depend on the order the rows arrive in.
        let mut shuffled = ledger.clone();
        shuffled.reverse();
        assert_eq!(leave_balance(&shuffled).left, balance.left);
    }

    #[test]
    fn a_balance_can_go_negative_and_says_so() {
        // Somebody took more than they had. That is a real state — a shop
        // grants it and recovers it later — and hiding it behind a floor of
        // zero would be the software lying about what it knows.
        let ledger = vec![entry(LeaveKind::Accrued, 2), entry(LeaveKind::Taken, -6)];
        let balance = leave_balance(&ledger);
        assert_eq!(balance.left, HalfDays::new(-4));
        assert!(balance.left.is_negative());
    }

    #[test]
    fn overlapping_leave_is_recognised_at_both_ends() {
        // Inclusive at both ends: "the 14th to the 16th" is three days.
        assert!(overlaps((day(14), day(16)), (day(16), day(18))));
        assert!(overlaps((day(14), day(16)), (day(12), day(14))));
        assert!(overlaps((day(14), day(16)), (day(15), day(15))));
        assert!(overlaps((day(14), day(16)), (day(10), day(20))));
        assert!(!overlaps((day(14), day(16)), (day(17), day(18))));
        assert!(!overlaps((day(14), day(16)), (day(12), day(13))));
    }

    // -- the effective-dated structure -------------------------------------

    fn structure(from: i32, basis: Basis, amount: i64) -> Structure {
        Structure {
            effective_from: day(from),
            basis,
            amount: rupees(amount),
            components: Vec::new(),
        }
    }

    #[test]
    fn a_raise_applies_from_its_date_and_not_before() {
        let history = vec![
            structure(0, Basis::Monthly, 18_000),
            structure(30, Basis::Monthly, 20_000),
        ];
        assert_eq!(
            structure_on(&history, day(29)).map(|s| s.amount),
            Some(rupees(18_000))
        );
        assert_eq!(
            structure_on(&history, day(30)).map(|s| s.amount),
            Some(rupees(20_000))
        );
        // Before anybody was paid anything: not an error, just nothing.
        assert!(structure_on(&history, day(-1)).is_none());
    }

    // -- the payroll arithmetic --------------------------------------------

    fn worked(days: i32, unpaid: i32, period_days: i32) -> Worked {
        Worked {
            days: HalfDays::from_days(days),
            minutes: 0,
            unpaid: HalfDays::from_days(unpaid),
            period_days,
        }
    }

    /// **A monthly salary, worked by hand.**
    ///
    /// ₹18,000 a month, two unpaid days in a 30-day period, a ₹1,000 food
    /// allowance, a ₹500 room deduction, ₹2,000 of advance recovered.
    ///
    ///   day rate            18000 ÷ 30            =   600.00
    ///   unpaid deduction    600 × 2               =  1200.00
    ///   net    18000 + 1000 − 500 − 1200 − 2000   = 15300.00
    #[test]
    fn a_monthly_salary_adds_up_by_hand() {
        let mut s = structure(0, Basis::Monthly, 18_000);
        s.components = vec![
            Component {
                name: "Food".to_owned(),
                kind: ComponentKind::Allowance,
                amount: rupees(1_000),
            },
            Component {
                name: "Room".to_owned(),
                kind: ComponentKind::Deduction,
                amount: rupees(500),
            },
        ];

        let line = pay_line(&s, &worked(26, 2, 30), rupees(2_000)).expect("computes");

        assert_eq!(line.earned, rupees(18_000));
        assert_eq!(line.allowances, rupees(1_000));
        assert_eq!(line.deductions, rupees(500));
        assert_eq!(line.unpaid_leave_deduction, rupees(1_200));
        assert_eq!(line.advance_recovered, rupees(2_000));
        assert_eq!(line.net, rupees(15_300));
    }

    /// **A daily wage, worked by hand.** ₹700 a day, 24½ days worked.
    ///   700 × 24.5 = 17150.00, and no unpaid deduction because a day not
    ///   worked was never earned.
    #[test]
    fn a_daily_wage_adds_up_by_hand() {
        let s = structure(0, Basis::Daily, 700);
        let w = Worked {
            days: HalfDays::new(49),
            minutes: 0,
            unpaid: HalfDays::from_days(2),
            period_days: 30,
        };
        let line = pay_line(&s, &w, Money::ZERO).expect("computes");
        assert_eq!(line.earned, rupees(17_150));
        assert_eq!(line.unpaid_leave_deduction, Money::ZERO);
        assert_eq!(line.net, rupees(17_150));
    }

    /// **An hourly wage, worked by hand.** ₹90 an hour, 7,410 minutes.
    ///   9000 paise × 7410 ÷ 60 = 1,111,500 paise = ₹11,115.00
    #[test]
    fn an_hourly_wage_adds_up_by_hand() {
        let s = structure(0, Basis::Hourly, 90);
        let w = Worked {
            days: HalfDays::from_days(23),
            minutes: 7_410,
            unpaid: HalfDays::ZERO,
            period_days: 30,
        };
        let line = pay_line(&s, &w, Money::ZERO).expect("computes");
        assert_eq!(line.earned, rupees(11_115));
        assert_eq!(line.net, rupees(11_115));
    }

    /// **An unpaid day comes off a monthly salary once, and off a daily wage
    /// never.** The second half is the one that looks like a bug and is not:
    /// a day not worked was never earned on a daily basis, so deducting it
    /// would take the day off twice.
    #[test]
    fn an_unpaid_day_is_taken_off_a_monthly_salary_once_and_a_daily_one_never() {
        let monthly = structure(0, Basis::Monthly, 30_000);
        let with_none = pay_line(&monthly, &worked(30, 0, 30), Money::ZERO).expect("computes");
        let with_one = pay_line(&monthly, &worked(29, 1, 30), Money::ZERO).expect("computes");
        // Exactly one day's worth: 30000 ÷ 30 = 1000.
        assert_eq!(
            with_none.net.sub(with_one.net).expect("subtracts"),
            rupees(1_000)
        );

        let daily = structure(0, Basis::Daily, 1_000);
        let a = pay_line(&daily, &worked(29, 0, 30), Money::ZERO).expect("computes");
        let b = pay_line(&daily, &worked(29, 1, 30), Money::ZERO).expect("computes");
        assert_eq!(a.net, b.net, "a daily wage must not pay for a day twice");
    }

    /// **A net is never negative**, however big the advance. What is not
    /// recovered stays outstanding.
    #[test]
    fn an_advance_bigger_than_the_month_does_not_make_a_negative_payslip() {
        let s = structure(0, Basis::Monthly, 10_000);
        let line = pay_line(&s, &worked(30, 0, 30), rupees(25_000)).expect("computes");
        assert_eq!(line.net, Money::ZERO);
    }

    // -- advances ----------------------------------------------------------

    fn advance(id: &str, on: i32, amount: i64, instalments: i32, recovered: i64) -> Advance {
        Advance {
            id: id.to_owned(),
            day: day(on),
            amount: rupees(amount),
            instalments,
            recovered: rupees(recovered),
        }
    }

    #[test]
    fn advances_are_recovered_oldest_first_and_capped_at_what_was_earned() {
        let advances = vec![
            advance("adv_new", 20, 3_000, 1, 0),
            advance("adv_old", 5, 4_000, 1, 0),
        ];
        // Plenty available: both come back, oldest first.
        let full = recover_advances(&advances, rupees(20_000)).expect("recovers");
        assert_eq!(full.len(), 2);
        assert_eq!(full[0].advance_id, "adv_old");
        assert_eq!(full[0].amount, rupees(4_000));
        assert_eq!(full[1].amount, rupees(3_000));

        // Only ₹5,000 earned: the old one in full, then what is left.
        let partial = recover_advances(&advances, rupees(5_000)).expect("recovers");
        assert_eq!(partial[0].amount, rupees(4_000));
        assert_eq!(partial[1].amount, rupees(1_000));
    }

    #[test]
    fn an_instalment_takes_a_share_and_stops_at_what_is_left() {
        // ₹6,000 over three months: ₹2,000 a time.
        let first = advance("adv", 0, 6_000, 3, 0);
        assert_eq!(
            recover_advances(std::slice::from_ref(&first), rupees(50_000)).expect("recovers")[0]
                .amount,
            rupees(2_000)
        );

        // Two taken already: the third takes the last ₹2,000 and no more.
        let third = advance("adv", 0, 6_000, 3, 4_000);
        let taken = recover_advances(std::slice::from_ref(&third), rupees(50_000))
            .expect("recovers");
        assert_eq!(taken[0].amount, rupees(2_000));

        // Fully recovered: nothing at all, not a zero row.
        let done = advance("adv", 0, 6_000, 3, 6_000);
        assert!(
            recover_advances(std::slice::from_ref(&done), rupees(50_000))
                .expect("recovers")
                .is_empty()
        );
    }

    // -- attendance --------------------------------------------------------

    #[test]
    fn a_night_shift_is_not_minus_six_hours() {
        // 22:00 to 06:00 is eight hours, not −960 minutes. The version of this
        // that forgets the wrap pays an hourly employee a negative wage.
        assert_eq!(minutes_between(1_320, 360), 480);
        assert_eq!(minutes_between(420, 900), 480);
        // A shift that is exactly a day.
        assert_eq!(minutes_between(600, 600), 0);
    }

    #[test]
    fn the_roster_is_what_makes_late_and_absent_mean_anything() {
        let shift = Some((540_i64, 1_020_i64)); // 09:00 – 17:00
        let grace = 10;

        assert_eq!(
            judge_day(shift, Some(545), Some(1_020), false, grace),
            DayVerdict::Present
        );
        assert_eq!(
            judge_day(shift, Some(575), Some(1_020), false, grace),
            DayVerdict::Late { by_minutes: 35 }
        );
        assert_eq!(
            judge_day(shift, Some(540), Some(900), false, grace),
            DayVerdict::LeftEarly { by_minutes: 120 }
        );
        // Rostered, no leave, did not come.
        assert_eq!(judge_day(shift, None, None, false, grace), DayVerdict::Absent);
        // Rostered, but on approved leave. NOT an absence — T2.
        assert_eq!(judge_day(shift, None, None, true, grace), DayVerdict::Away);
        // Not rostered at all: neither present nor absent, because nobody said.
        assert_eq!(judge_day(None, None, None, false, grace), DayVerdict::Away);
        assert_eq!(
            judge_day(None, Some(600), Some(1_000), false, grace),
            DayVerdict::Unrostered
        );
    }

    #[test]
    fn late_beats_left_early_when_a_day_is_both() {
        // A shop chases the first one, and the second is usually a consequence.
        let shift = Some((540_i64, 1_020_i64));
        assert_eq!(
            judge_day(shift, Some(600), Some(900), false, 10),
            DayVerdict::Late { by_minutes: 60 }
        );
    }
}
