//! Service, packing and delivery charges — each with its own GST rate.
//!
//! v1 had none of them. The audit's B13 is one line: *"No service charge, no
//! packing/parcel charge, no delivery charge, no tip."*
//!
//! **A charge carries its own rate and treatment**, and that is the whole
//! point. A service charge is taxed at 18% in most states even on a bill of 5%
//! food — the mixed-rate case v1 could not express at all (B10). So a charge
//! goes through `tax::compute_line` exactly like a line does and lands in the
//! same `TaxSummary`. It is a supply; it is not a special case in the tax
//! engine and must never become one.

use crate::money::Money;
use crate::tax::{TaxAmounts, TaxRate, TaxTreatment};
use serde::{Deserialize, Serialize};

/// What kind of charge it is. Reports group by this; the bill prints `name`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeKind {
    Service,
    Packing,
    Delivery,
    /// For a charge a shop invents. Kept narrow on purpose — every free-text
    /// string here is bytes that sync to the cloud forever (D16).
    Other(String),
}

/// How the charge is worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeBasis {
    /// Basis points of the **charge base** — see [`Charge`].
    Percent(u32),
    Flat(Money),
}

/// A charge as configured by the shop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Charge {
    pub kind: ChargeKind,
    /// What prints on the bill: "Service Charge", "Packing".
    pub name: String,
    pub basis: ChargeBasis,
    /// Its **own** rate — not the bill's, and not any line's.
    pub tax_rate: TaxRate,
    /// Its own treatment. There is deliberately no separate `taxable: bool`:
    /// `Exempt` and `NonGst` already say it, and two fields meaning the same
    /// thing is a bug waiting for the day they disagree.
    pub tax_treatment: TaxTreatment,
}

impl Charge {
    /// The everyday case: a percentage, taxed, exclusive of the price.
    #[must_use]
    pub fn percent(kind: ChargeKind, name: impl Into<String>, basis_points: u32, rate: TaxRate) -> Self {
        Charge {
            kind,
            name: name.into(),
            basis: ChargeBasis::Percent(basis_points),
            tax_rate: rate,
            tax_treatment: TaxTreatment::Exclusive,
        }
    }

    #[must_use]
    pub fn flat(kind: ChargeKind, name: impl Into<String>, amount: Money, rate: TaxRate) -> Self {
        Charge {
            kind,
            name: name.into(),
            basis: ChargeBasis::Flat(amount),
            tax_rate: rate,
            tax_treatment: TaxTreatment::Exclusive,
        }
    }

    #[must_use]
    pub fn with_treatment(mut self, treatment: TaxTreatment) -> Self {
        self.tax_treatment = treatment;
        self
    }

    /// What this charge comes to, against `base`.
    ///
    /// **The base is the sum of the line nets after BOTH discounts** — the
    /// total after D4 step 3, before any charge. Three things follow, and each
    /// one is a test:
    ///
    /// * **A percentage charge never compounds on another charge.** Two 5%
    ///   charges on a ₹1,000 base are ₹50 and ₹50, never ₹50 and ₹52.50. No
    ///   Indian restaurant compounds service onto packing, and it would be
    ///   invisible on the printed bill if it did.
    /// * **It is computed on the DISCOUNTED total.** A shop that gives 10% off
    ///   charges service on what the customer actually owes; anything else
    ///   takes part of the discount back through the side door.
    /// * **The base includes non-GST lines.** Service applies to the whole
    ///   table, alcohol included. Food-only service charge would be a new
    ///   setting and a new scope line, not something to assume here.
    pub fn compute_on(&self, base: Money) -> Result<Money, crate::money::MoneyError> {
        match self.basis {
            ChargeBasis::Percent(bp) => base.percent_bp(bp),
            // A flat charge is a flat charge — a ₹40 delivery fee does not
            // shrink because the customer had a discount.
            ChargeBasis::Flat(amount) => Ok(amount),
        }
    }
}

/// A charge once it has been computed and taxed, ready to print.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillCharge {
    pub kind: ChargeKind,
    pub name: String,
    pub basis: ChargeBasis,
    /// The charge itself, before its own tax.
    pub amount: Money,
    pub taxable: Money,
    pub tax: TaxAmounts,
    /// `taxable + tax`. What this charge adds to the grand total.
    pub gross_including_tax: Money,
    pub rate: TaxRate,
    pub treatment: TaxTreatment,
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rs(rupees: i64) -> Money {
        Money::from_paise(rupees * 100)
    }

    #[test]
    fn a_percentage_charge_is_taken_on_the_base_it_is_given() {
        let service = Charge::percent(ChargeKind::Service, "Service Charge", 500, TaxRate::GST_18);
        assert_eq!(service.compute_on(rs(1_000)), Ok(rs(50)));
        assert_eq!(service.compute_on(rs(900)), Ok(rs(45)));
        assert_eq!(service.compute_on(Money::ZERO), Ok(Money::ZERO));
    }

    #[test]
    fn a_flat_charge_ignores_the_base_entirely() {
        // A ₹40 delivery fee does not shrink because the customer had 10% off.
        let delivery = Charge::flat(ChargeKind::Delivery, "Delivery", rs(40), TaxRate::GST_18);
        assert_eq!(delivery.compute_on(rs(1_000)), Ok(rs(40)));
        assert_eq!(delivery.compute_on(rs(500)), Ok(rs(40)));
        // Even on an empty bill it still computes rather than failing — a
        // delivery charge with no items is a mistake upstream, not a crash.
        assert_eq!(delivery.compute_on(Money::ZERO), Ok(rs(40)));
    }

    #[test]
    fn a_charge_can_be_untaxed_without_a_second_flag() {
        let tipless = Charge::flat(ChargeKind::Other("Donation".to_owned()), "Donation", rs(10), TaxRate::ZERO)
            .with_treatment(TaxTreatment::Exempt);
        assert_eq!(tipless.tax_treatment, TaxTreatment::Exempt);
        assert!(!tipless.tax_treatment.is_taxed());
    }
}
