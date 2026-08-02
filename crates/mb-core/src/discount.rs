//! Discounts, and the proportional spread that keeps a mixed-rate bill honest.
//!
//! v1 had **no discount of any kind** — the audit's finding B7 is one line long
//! because there was nothing to describe. So every rule here is new, and the
//! one that matters is the spread.
//!
//! **A bill-level discount is spread across the lines before tax.** v1 charged
//! one GST rate for the whole bill (audit B10), so nobody ever had to think
//! about it. v2 mixes 5% food, 18% packaged goods and 0% alcohol on one bill:
//! take 10% off the grand total after tax and the tax already printed is wrong
//! for every rate, the rate-wise summary does not tie, and a CA cannot file
//! from the bill (audit B11).

use crate::money::{Money, MoneyError};
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, MoneyError>;

/// Money off — either a percentage or a flat amount.
///
/// Basis points rather than a float percentage, for the same reason `TaxRate`
/// uses them: 2.5% and 12.5% are ordinary discounts and neither survives a
/// binary float exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Discount {
    /// Basis points: `1_000` is 10.00%.
    Percent(u32),
    Amount(Money),
}

impl Discount {
    /// `None` above 100% — a discount larger than the bill is a typo, not an
    /// offer.
    #[must_use]
    pub const fn percent_bp(basis_points: u32) -> Option<Self> {
        if basis_points > 10_000 { None } else { Some(Discount::Percent(basis_points)) }
    }

    /// `None` for a negative amount. A "negative discount" is a charge, and
    /// charges are their own thing with their own tax rate (P02, scope 1.14).
    #[must_use]
    pub fn amount(amount: Money) -> Option<Self> {
        if amount.is_negative() { None } else { Some(Discount::Amount(amount)) }
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        match self {
            Discount::Percent(bp) => bp == 0,
            Discount::Amount(m) => m.is_zero(),
        }
    }

    /// What this discount takes off `base`.
    ///
    /// A discount can never exceed its base and can never be negative. When it
    /// is asked for more than the base it takes the base — and says so, which
    /// is the part that matters (see [`DiscountOutcome`]).
    pub fn compute_on(self, base: Money) -> Result<DiscountOutcome> {
        let requested = match self {
            Discount::Percent(bp) => base.percent_bp(bp)?,
            Discount::Amount(amount) => amount,
        };

        // A base of zero (a fully complimentary line) can only ever give back
        // zero, whatever was asked for.
        let ceiling = if base.is_negative() { Money::ZERO } else { base };
        let applied = if requested > ceiling { ceiling } else { requested };

        Ok(DiscountOutcome { applied, requested, was_capped: applied != requested })
    }
}

/// What a discount actually did, including when it could not do what was asked.
///
/// **`was_capped` is the reason this is a struct and not a `Money`.** A ₹500
/// flat discount on a ₹300 line takes ₹300 — but silently taking less than the
/// cashier typed is exactly the class of bug D7 exists to prevent. The flag
/// travels to the `Bill`, so P02 can tell the cashier "discount reduced to
/// ₹300" and P11 can put it in the audit trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DiscountOutcome {
    /// What was taken off.
    pub applied: Money,
    /// What the discount asked for.
    pub requested: Money,
    /// `applied` is less than `requested`.
    pub was_capped: bool,
}

/// Spread `total` across the lines in proportion to `line_nets`.
///
/// Three things are true of the result at once, and all three are tested:
///
/// 1. `sum(result) == total`, exactly, to the paisa;
/// 2. `result[i] <= line_nets[i]` for every line — a line's share can never
///    exceed the line, or its net goes negative and the bill cannot be printed;
/// 3. it is deterministic — the same input always gives the same output.
///
/// **Largest remainder, not "give the last line the difference".** The obvious
/// approach — round each share and hand the rounding remainder to the final
/// line, the way `Money::halve_exact` does — is exact and safe for TWO parts.
/// For N parts it is not: the last line can be handed more than its own net (a
/// ₹0.50 line at the end of a ₹5,000 bill), which breaks (2).
///
/// Flooring first and then distributing the leftover paise satisfies (1) and
/// (2) together, and it is provable rather than lucky. With `total <=
/// total_net`, `floor(total × net_i / total_net) < net_i` whenever any leftover
/// exists at all, so the one extra paisa a line can receive can never carry it
/// past its own net. Where `total == total_net` the floors are exact and there
/// is no leftover to hand out.
pub fn spread(total: Money, line_nets: &[Money]) -> Result<Vec<Money>> {
    let mut shares = vec![Money::ZERO; line_nets.len()];
    if total.is_zero() || line_nets.is_empty() {
        return Ok(shares);
    }

    let total_net = Money::try_sum(line_nets.iter().copied())?;
    // An all-complimentary bill: there is nothing to take a proportion of.
    // This early return is also what stops the division below by zero.
    if total_net.is_zero() {
        return Ok(shares);
    }

    // Floor each share, and remember by how much each one fell short. The
    // remainder is `(total × net_i) mod total_net`, computed as the gap between
    // the exact product and the floored share scaled back up — done in i128 so
    // nothing can overflow on the way.
    let mut allotted = Money::ZERO;
    // (remainder, index), so a plain sort ranks by remainder and then by index.
    let mut remainders: Vec<(i128, usize)> = Vec::with_capacity(line_nets.len());

    for (index, net) in line_nets.iter().enumerate() {
        let share = total.mul_ratio_floor(net.paise(), total_net.paise())?;
        let exact = i128::from(total.paise()) * i128::from(net.paise());
        let taken = i128::from(share.paise()) * i128::from(total_net.paise());
        remainders.push((exact - taken, index));
        allotted = allotted.add(share)?;
        shares[index] = share;
    }

    // What the flooring left behind: strictly less than one paisa per line.
    let leftover = total.sub(allotted)?.paise();

    // Largest remainder first; ties go to the lower index so the result is
    // deterministic and a test can assert an exact vector.
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    for (_, index) in remainders.into_iter().take(
        usize::try_from(leftover.max(0)).unwrap_or(0),
    ) {
        shares[index] = shares[index].add(Money::from_paise(1))?;
    }

    Ok(shares)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rs(rupees: i64) -> Money {
        Money::from_paise(rupees * 100)
    }

    #[test]
    fn a_percentage_comes_off_the_base() {
        let ten_percent = Discount::percent_bp(1_000).expect("valid");
        let out = ten_percent.compute_on(rs(500)).expect("computes");
        assert_eq!(out.applied, rs(50));
        assert!(!out.was_capped);

        // 12.5% of ₹99.99 — the case a float would get wrong.
        let out = Discount::percent_bp(1_250)
            .expect("valid")
            .compute_on(Money::from_paise(9_999))
            .expect("computes");
        assert_eq!(out.applied, Money::from_paise(1_250)); // ₹12.4988 -> ₹12.50
    }

    #[test]
    fn a_cap_is_reported_never_hidden() {
        // ₹500 off a ₹300 line takes ₹300 — and says it did less than asked.
        let flat = Discount::amount(rs(500)).expect("valid");
        let out = flat.compute_on(rs(300)).expect("computes");
        assert_eq!(out.applied, rs(300));
        assert_eq!(out.requested, rs(500));
        assert!(out.was_capped, "a silent cap is the bug D7 exists to prevent");

        // Exactly the base is not a cap.
        let out = flat.compute_on(rs(500)).expect("computes");
        assert_eq!(out.applied, rs(500));
        assert!(!out.was_capped);
    }

    #[test]
    fn a_discount_is_never_negative_and_never_exceeds_its_base() {
        for base in 0..400_i64 {
            for bp in [0_u32, 1, 250, 1_000, 5_000, 10_000] {
                let out = Discount::percent_bp(bp)
                    .expect("valid")
                    .compute_on(Money::from_paise(base))
                    .expect("computes");
                assert!(!out.applied.is_negative(), "{bp}bp of {base} went negative");
                assert!(out.applied.paise() <= base, "{bp}bp of {base} exceeded the base");
            }
        }
        assert_eq!(
            Discount::amount(rs(100))
                .expect("valid")
                .compute_on(Money::ZERO)
                .expect("computes")
                .applied,
            Money::ZERO
        );
    }

    #[test]
    fn absurd_discounts_are_refused_at_construction() {
        assert!(Discount::percent_bp(10_001).is_none());
        assert!(Discount::amount(Money::from_paise(-1)).is_none());
        assert!(Discount::percent_bp(10_000).is_some()); // 100% off is legitimate
        assert!(Discount::amount(Money::ZERO).is_some());
    }

    #[test]
    fn the_spread_always_adds_back_to_the_whole() {
        // Every awkward shape at once: indivisible amounts, one huge line
        // among tiny ones, equal lines that do not divide evenly.
        let cases: Vec<(Money, Vec<Money>)> = vec![
            (Money::from_paise(1), vec![rs(1); 7]),
            (Money::from_paise(100), vec![rs(1), rs(1), rs(1)]),
            (rs(10), vec![rs(33), rs(33), rs(34)]),
            (rs(1), vec![rs(1_000), Money::from_paise(50), Money::from_paise(50)]),
            (Money::from_paise(7), vec![Money::from_paise(3); 11]),
            (rs(500), vec![rs(500)]),
            (Money::from_paise(999), vec![Money::from_paise(1); 999]),
        ];

        for (total, nets) in cases {
            let shares = spread(total, &nets).expect("spreads");
            assert_eq!(
                Money::try_sum(shares.iter().copied()),
                Ok(total),
                "spreading {total} over {} lines lost money",
                nets.len()
            );
        }
    }

    #[test]
    fn no_line_is_ever_given_more_than_it_is_worth() {
        // The requirement the last-line-remainder approach fails.
        for line_count in 1..40_usize {
            for total_paise in [1_i64, 7, 99, 100, 1_234, 50_000] {
                let nets: Vec<Money> = (1..=line_count)
                    .map(|i| {
                        let paise = i64::try_from(i).unwrap_or(1) * 37 % 991 + 1;
                        Money::from_paise(paise)
                    })
                    .collect();
                let total_net = Money::try_sum(nets.iter().copied()).expect("sums");
                let total = Money::from_paise(total_paise.min(total_net.paise()));

                let shares = spread(total, &nets).expect("spreads");
                assert_eq!(Money::try_sum(shares.iter().copied()), Ok(total));
                for (share, net) in shares.iter().zip(&nets) {
                    assert!(
                        share.paise() <= net.paise(),
                        "a line of {net} was given {share}"
                    );
                    assert!(!share.is_negative());
                }
            }
        }
    }

    #[test]
    fn the_spread_is_proportional_and_deterministic() {
        // Two equal lines split evenly; a line worth twice as much takes twice
        // as much off.
        assert_eq!(spread(rs(10), &[rs(50), rs(50)]), Ok(vec![rs(5), rs(5)]));
        assert_eq!(spread(rs(30), &[rs(100), rs(200)]), Ok(vec![rs(10), rs(20)]));

        // An indivisible paisa goes to the largest remainder, and the same
        // input always gives the same answer.
        let once = spread(Money::from_paise(1), &[rs(1), rs(2)]).expect("spreads");
        let twice = spread(Money::from_paise(1), &[rs(1), rs(2)]).expect("spreads");
        assert_eq!(once, twice);
        assert_eq!(Money::try_sum(once.iter().copied()), Ok(Money::from_paise(1)));
    }

    #[test]
    fn an_all_complimentary_bill_does_not_divide_by_zero() {
        assert_eq!(
            spread(rs(10), &[Money::ZERO, Money::ZERO]),
            Ok(vec![Money::ZERO, Money::ZERO])
        );
        assert_eq!(spread(rs(10), &[]), Ok(vec![]));
        assert_eq!(spread(Money::ZERO, &[rs(50)]), Ok(vec![Money::ZERO]));
    }

    #[test]
    fn a_zero_net_line_takes_no_share() {
        // A complimentary line sitting among paid ones must not absorb any of
        // the bill discount, or the paid lines get taxed on too much.
        let shares = spread(rs(10), &[rs(50), Money::ZERO, rs(50)]).expect("spreads");
        assert_eq!(shares[1], Money::ZERO);
        assert_eq!(Money::try_sum(shares.iter().copied()), Ok(rs(10)));
    }
}
