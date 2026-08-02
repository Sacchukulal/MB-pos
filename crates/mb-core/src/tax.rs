//! GST.
//!
//! **Tax belongs to a line, never to a bill** (decision D3). One bill can hold
//! 5% food, 18% packaged water and 0% alcohol at the same time, because that is
//! what a real restaurant sells.
//!
//! v1 had a single rate for the whole bill, chosen from 5/12/18, split 50/50
//! into CGST and SGST with no IGST and no concept of a non-GST item. A bar, an
//! AC/non-AC mixed outlet, or anyone doing B2B catering could not bill legally
//! on it at all.
//!
//! What this module owns:
//!   * the rate, as basis points, so 2.5% is representable exactly;
//!   * the treatment (price includes tax, excludes tax, exempt, or outside GST);
//!   * the CGST/SGST vs IGST split, decided by place of supply;
//!   * the rate-wise summary a GSTR-1 return is built from.

use crate::money::{Money, MoneyError};
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, MoneyError>;

/// A GST rate held in basis points: `500` is 5.00%, `250` is 2.50%.
///
/// Basis points rather than a percentage float because 2.5% and 0.25% are both
/// real Indian rates and neither survives a binary float exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaxRate(u32);

impl TaxRate {
    pub const ZERO: TaxRate = TaxRate(0);
    pub const GST_5: TaxRate = TaxRate(500);
    pub const GST_12: TaxRate = TaxRate(1_200);
    pub const GST_18: TaxRate = TaxRate(1_800);
    pub const GST_28: TaxRate = TaxRate(2_800);

    /// Rates above 100% are always a data-entry error, so they are refused.
    #[must_use]
    pub const fn from_basis_points(bp: u32) -> Option<Self> {
        if bp > 10_000 { None } else { Some(TaxRate(bp)) }
    }

    /// `5` -> 5%. The everyday case.
    #[must_use]
    pub const fn from_percent(percent: u32) -> Option<Self> {
        match percent.checked_mul(100) {
            Some(bp) => Self::from_basis_points(bp),
            None => None,
        }
    }

    #[must_use]
    pub const fn basis_points(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// `"5%"`, `"2.5%"`, `"18%"` — for the receipt and the summary table.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "splitting basis points into whole and fractional percent"
    )]
    pub fn label(self) -> String {
        let whole = self.0 / 100;
        let frac = self.0 % 100;
        if frac == 0 {
            format!("{whole}%")
        } else if frac.is_multiple_of(10) {
            format!("{whole}.{}%", frac / 10)
        } else {
            format!("{whole}.{frac:02}%")
        }
    }
}

/// How a line's price relates to its tax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaxTreatment {
    /// The menu price excludes GST; tax is added on top. The common case.
    #[default]
    Exclusive,
    /// The menu price already contains GST; tax is extracted from it.
    Inclusive,
    /// Inside GST but at nil rate — appears in the return with taxable value.
    Exempt,
    /// **Outside GST entirely — alcohol.** State excise/VAT is charged
    /// separately and must never appear in a GST return. This is the treatment
    /// v1 had no way to express, which is why a bar could not use it.
    NonGst,
}

impl TaxTreatment {
    #[must_use]
    pub const fn is_taxed(self) -> bool {
        matches!(self, TaxTreatment::Exclusive | TaxTreatment::Inclusive)
    }
}

/// Where the supply happens, which decides how the tax is named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceOfSupply {
    /// Same state as the restaurant: the tax splits into CGST + SGST.
    #[default]
    Intra,
    /// Another state: the whole tax is IGST. Needed for B2B catering and
    /// interstate delivery, and absent from v1 entirely.
    Inter,
}

/// One line's tax, already named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaxAmounts {
    pub cgst: Money,
    pub sgst: Money,
    pub igst: Money,
}

impl TaxAmounts {
    pub fn total(self) -> Result<Money> {
        Money::try_sum([self.cgst, self.sgst, self.igst])
    }

    #[allow(clippy::should_implement_trait, reason = "addition here must be able to fail (D7)")]
    pub fn add(self, other: Self) -> Result<Self> {
        Ok(TaxAmounts {
            cgst: self.cgst.add(other.cgst)?,
            sgst: self.sgst.add(other.sgst)?,
            igst: self.igst.add(other.igst)?,
        })
    }
}

/// What one line contributes to the bill and to the return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaxOutcome {
    /// The value tax is charged on. For an inclusive line this is LESS than the
    /// price the customer sees.
    pub taxable: Money,
    pub tax: TaxAmounts,
    /// `taxable + tax`. For an inclusive line this equals the line net exactly.
    pub gross: Money,
    pub rate: TaxRate,
    pub treatment: TaxTreatment,
}

/// Compute the tax for one line from its **already discounted** net amount.
///
/// The input is the net *after* line discount and after that line's share of
/// any bill-level discount — see decision D4. Passing an undiscounted amount
/// here produces tax on money the customer never paid.
pub fn compute_line(
    net: Money,
    rate: TaxRate,
    treatment: TaxTreatment,
    place: PlaceOfSupply,
) -> Result<TaxOutcome> {
    let (taxable, tax_total) = match treatment {
        // Outside GST, or inside at nil rate: nothing to charge either way.
        // They differ only in the return, which reads `treatment`.
        TaxTreatment::NonGst | TaxTreatment::Exempt => (net, Money::ZERO),

        TaxTreatment::Exclusive => {
            let tax = net.percent_bp(rate.basis_points())?;
            (net, tax)
        }

        TaxTreatment::Inclusive => {
            // taxable = net × 10000 / (10000 + rate)
            //
            // The tax is then the REMAINDER, not a second rounded
            // multiplication. That guarantees `taxable + tax == net` exactly,
            // so an inclusive-priced item always totals to its menu price —
            // which is the entire point of inclusive pricing.
            let denominator = i64::from(rate.basis_points()).saturating_add(10_000);
            let taxable = net.mul_ratio(10_000, denominator)?;
            let tax = net.sub(taxable)?;
            (taxable, tax)
        }
    };

    let tax = match place {
        PlaceOfSupply::Intra => {
            // halve_exact so CGST + SGST is always the whole tax, to the paisa.
            let (cgst, sgst) = tax_total.halve_exact();
            TaxAmounts { cgst, sgst, igst: Money::ZERO }
        }
        PlaceOfSupply::Inter => TaxAmounts {
            cgst: Money::ZERO,
            sgst: Money::ZERO,
            igst: tax_total,
        },
    };

    Ok(TaxOutcome {
        taxable,
        tax,
        gross: taxable.add(tax_total)?,
        rate,
        treatment,
    })
}

/// One row of the rate-wise summary printed on the bill and filed in GSTR-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RateSummaryRow {
    pub rate: TaxRate,
    pub taxable: Money,
    pub tax: TaxAmounts,
}

/// Taxable value and tax, grouped by rate.
///
/// This is what a GST return is actually built from, and what v1 could not
/// produce: it reported one taxable figure and split it 50/50 with no rates,
/// no IGST and no separation of alcohol.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaxSummary {
    /// Kept sorted by rate, so the rows always print in ascending rate order.
    ///
    /// A `Vec` rather than a map keyed by rate, and the reason is
    /// serialisation: a JSON object's keys are strings, so a `BTreeMap<u32, _>`
    /// writes `"500"` and cannot be read back through serde's tag buffering —
    /// which is what an internally-tagged enum like
    /// [`AnyOrder`](crate::order::AnyOrder) uses. That failure only appears
    /// once a bill is stored inside an order, which is P04, and it is far
    /// cheaper to not have the map at all. A bill has at most a handful of
    /// rates, so a sorted `Vec` is also the faster structure here.
    rows: Vec<RateSummaryRow>,
    /// Value of supplies outside GST (alcohol). Never enters a GST return.
    pub non_gst_value: Money,
    /// Value of nil-rated supplies. Enters the return with zero tax.
    pub exempt_value: Money,
}

impl TaxSummary {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, outcome: TaxOutcome) -> Result<()> {
        match outcome.treatment {
            TaxTreatment::NonGst => {
                self.non_gst_value = self.non_gst_value.add(outcome.taxable)?;
            }
            TaxTreatment::Exempt => {
                self.exempt_value = self.exempt_value.add(outcome.taxable)?;
            }
            TaxTreatment::Exclusive | TaxTreatment::Inclusive => {
                let bp = outcome.rate.basis_points();
                let index = match self.rows.binary_search_by_key(&bp, |r| r.rate.basis_points()) {
                    Ok(found) => found,
                    Err(insert_at) => {
                        // Inserted in place, so `rows` stays in ascending rate
                        // order without ever being sorted.
                        self.rows.insert(
                            insert_at,
                            RateSummaryRow { rate: outcome.rate, ..RateSummaryRow::default() },
                        );
                        insert_at
                    }
                };
                let row = &mut self.rows[index];
                row.taxable = row.taxable.add(outcome.taxable)?;
                row.tax = row.tax.add(outcome.tax)?;
            }
        }
        Ok(())
    }

    /// Rows in ascending rate order.
    pub fn rows(&self) -> impl Iterator<Item = &RateSummaryRow> {
        self.rows.iter()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.non_gst_value.is_zero() && self.exempt_value.is_zero()
    }

    /// Total taxable value across every taxed rate (excludes non-GST).
    pub fn total_taxable(&self) -> Result<Money> {
        Money::try_sum(self.rows.iter().map(|r| r.taxable))
    }

    pub fn total_tax(&self) -> Result<TaxAmounts> {
        self.rows
            .iter()
            .try_fold(TaxAmounts::default(), |acc, row| acc.add(row.tax))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rs(rupees: i64) -> Money {
        Money::from_paise(rupees * 100)
    }

    #[test]
    fn exclusive_adds_tax_on_top() {
        let out = compute_line(rs(100), TaxRate::GST_5, TaxTreatment::Exclusive, PlaceOfSupply::Intra)
            .expect("computes");
        assert_eq!(out.taxable, rs(100));
        assert_eq!(out.tax.cgst, Money::from_paise(250));
        assert_eq!(out.tax.sgst, Money::from_paise(250));
        assert_eq!(out.tax.igst, Money::ZERO);
        assert_eq!(out.gross, Money::from_paise(10_500));
    }

    #[test]
    fn inclusive_extracts_tax_and_always_totals_to_the_menu_price() {
        // ₹105 inclusive of 5% -> taxable ₹100, tax ₹5, gross back to ₹105.
        let out = compute_line(
            Money::from_paise(10_500),
            TaxRate::GST_5,
            TaxTreatment::Inclusive,
            PlaceOfSupply::Intra,
        )
        .expect("computes");
        assert_eq!(out.taxable, rs(100));
        assert_eq!(out.tax.total(), Ok(rs(5)));
        assert_eq!(out.gross, Money::from_paise(10_500));
    }

    #[test]
    fn an_inclusive_line_never_drifts_from_its_price() {
        // The property that matters: for ANY amount and ANY rate, an inclusive
        // line's taxable + tax is exactly what the customer was quoted.
        for paise in 1..2_000_i64 {
            for rate in [TaxRate::GST_5, TaxRate::GST_12, TaxRate::GST_18, TaxRate::GST_28] {
                let net = Money::from_paise(paise);
                let out = compute_line(net, rate, TaxTreatment::Inclusive, PlaceOfSupply::Intra)
                    .expect("computes");
                assert_eq!(out.gross, net, "{paise} paise at {}", rate.label());
                assert_eq!(
                    out.taxable.add(out.tax.total().expect("sums")),
                    Ok(net),
                    "{paise} paise at {} did not reconcile",
                    rate.label()
                );
            }
        }
    }

    #[test]
    fn cgst_and_sgst_always_add_to_the_whole_tax() {
        for paise in 1..3_000_i64 {
            let out = compute_line(
                Money::from_paise(paise),
                TaxRate::GST_5,
                TaxTreatment::Exclusive,
                PlaceOfSupply::Intra,
            )
            .expect("computes");
            let halves = out.tax.cgst.add(out.tax.sgst).expect("sums");
            assert_eq!(halves, out.tax.total().expect("sums"), "at {paise} paise");
            assert_eq!(out.taxable.add(halves), Ok(out.gross));
        }
    }

    #[test]
    fn interstate_is_all_igst() {
        let out = compute_line(rs(100), TaxRate::GST_18, TaxTreatment::Exclusive, PlaceOfSupply::Inter)
            .expect("computes");
        assert_eq!(out.tax.cgst, Money::ZERO);
        assert_eq!(out.tax.sgst, Money::ZERO);
        assert_eq!(out.tax.igst, rs(18));
    }

    #[test]
    fn alcohol_carries_no_gst_and_stays_out_of_the_return() {
        let out = compute_line(rs(500), TaxRate::ZERO, TaxTreatment::NonGst, PlaceOfSupply::Intra)
            .expect("computes");
        assert_eq!(out.tax.total(), Ok(Money::ZERO));
        assert_eq!(out.gross, rs(500));

        let mut summary = TaxSummary::new();
        summary.add(out).expect("adds");
        assert_eq!(summary.non_gst_value, rs(500));
        assert_eq!(summary.rows().count(), 0, "alcohol must not appear as a GST rate");
        assert_eq!(summary.total_taxable(), Ok(Money::ZERO));
    }

    #[test]
    fn exempt_is_in_the_return_with_no_tax() {
        let out = compute_line(rs(200), TaxRate::ZERO, TaxTreatment::Exempt, PlaceOfSupply::Intra)
            .expect("computes");
        let mut summary = TaxSummary::new();
        summary.add(out).expect("adds");
        assert_eq!(summary.exempt_value, rs(200));
        assert_eq!(summary.non_gst_value, Money::ZERO);
    }

    #[test]
    fn a_mixed_bill_groups_by_rate_in_order() {
        // The bill v1 could not produce: food at 5%, a soft drink at 12%,
        // cigarettes at 28% and a beer outside GST.
        let mut summary = TaxSummary::new();
        for (net, rate, treatment) in [
            (rs(400), TaxRate::GST_5, TaxTreatment::Exclusive),
            (rs(200), TaxRate::GST_5, TaxTreatment::Exclusive),
            (rs(100), TaxRate::GST_12, TaxTreatment::Exclusive),
            (rs(50), TaxRate::GST_28, TaxTreatment::Exclusive),
            (rs(300), TaxRate::ZERO, TaxTreatment::NonGst),
        ] {
            let out = compute_line(net, rate, treatment, PlaceOfSupply::Intra).expect("computes");
            summary.add(out).expect("adds");
        }

        let rows: Vec<_> = summary.rows().copied().collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].rate, TaxRate::GST_5);
        assert_eq!(rows[0].taxable, rs(600), "the two 5% lines must merge");
        assert_eq!(rows[0].tax.total(), Ok(rs(30)));
        assert_eq!(rows[1].rate, TaxRate::GST_12);
        assert_eq!(rows[2].rate, TaxRate::GST_28);

        assert_eq!(summary.total_taxable(), Ok(rs(750)), "non-GST value is excluded");
        assert_eq!(summary.non_gst_value, rs(300));
        assert_eq!(summary.total_tax().and_then(TaxAmounts::total), Ok(rs(56))); // 30 + 12 + 14
    }

    #[test]
    fn rate_labels_read_the_way_a_receipt_should() {
        assert_eq!(TaxRate::GST_5.label(), "5%");
        assert_eq!(TaxRate::GST_18.label(), "18%");
        assert_eq!(TaxRate::from_basis_points(250).map(TaxRate::label), Some("2.5%".to_owned()));
        assert_eq!(TaxRate::from_basis_points(25).map(TaxRate::label), Some("0.25%".to_owned()));
        assert_eq!(TaxRate::ZERO.label(), "0%");
    }

    #[test]
    fn absurd_rates_are_refused() {
        assert!(TaxRate::from_basis_points(10_001).is_none());
        assert!(TaxRate::from_percent(101).is_none());
        assert!(TaxRate::from_percent(u32::MAX).is_none());
        assert_eq!(TaxRate::from_percent(18), Some(TaxRate::GST_18));
    }
}
