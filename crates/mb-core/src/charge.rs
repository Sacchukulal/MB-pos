//! Service, packing and delivery charges — each with its own GST rate.

use crate::money::Money;
use crate::tax::{GstAmounts, TaxKind, TaxRate, TaxSpec};
use serde::{Deserialize, Serialize};

/// What kind of charge it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeKind {
    Service,
    Packing,
    Delivery,
    /// For a charge a shop invents.
    Other(String),
}

/// How the charge is worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeBasis {
    /// Basis points of the charge base — see `Charge`.
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
    /// Its own tax — not the bill's, and not any line's.
    pub tax: TaxSpec,
}

impl Charge {
    /// The everyday case: a percentage, taxed, exclusive of the price.
    #[must_use]
    pub fn percent(
        kind: ChargeKind,
        name: impl Into<String>,
        basis_points: u32,
        rate: TaxRate,
    ) -> Self {
        Charge {
            kind,
            name: name.into(),
            basis: ChargeBasis::Percent(basis_points),
            tax: TaxSpec::gst(rate),
        }
    }

    #[must_use]
    pub fn flat(kind: ChargeKind, name: impl Into<String>, amount: Money, rate: TaxRate) -> Self {
        Charge {
            kind,
            name: name.into(),
            basis: ChargeBasis::Flat(amount),
            tax: TaxSpec::gst(rate),
        }
    }

    #[must_use]
    pub fn with_tax(mut self, tax: TaxSpec) -> Self {
        self.tax = tax;
        self
    }

    /// A charge may not be alcohol.
    #[must_use]
    pub fn is_coherent(&self) -> bool {
        self.tax.kind != TaxKind::OutsideGst && self.tax.is_coherent()
    }

    /// What this charge comes to, against `base`.
    pub fn compute_on(&self, base: Money) -> Result<Money, crate::money::MoneyError> {
        match self.basis {
            ChargeBasis::Percent(bp) => base.percent_bp(bp),
            // A flat charge is a flat charge — a ₹40 delivery fee does not shrink because the
            // customer had a discount.
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
    pub gst: GstAmounts,
    /// `taxable + tax`. What this charge adds to the grand total.
    pub gross_including_tax: Money,
    pub tax: TaxSpec,
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rs(rupees: i64) -> Money {
        Money::from_paise(rupees * 100)
    }

    #[test]
    fn a_percentage_charge_is_taken_on_the_base_it_is_given() {
        let service = Charge::percent(
            ChargeKind::Service,
            "Service Charge",
            500,
            TaxRate::from_percent(18).expect("18%"),
        );
        assert_eq!(service.compute_on(rs(1_000)), Ok(rs(50)));
        assert_eq!(service.compute_on(rs(900)), Ok(rs(45)));
        assert_eq!(service.compute_on(Money::ZERO), Ok(Money::ZERO));
    }

    #[test]
    fn a_flat_charge_ignores_the_base_entirely() {
        // A ₹40 delivery fee does not shrink because the customer had 10% off.
        let delivery = Charge::flat(
            ChargeKind::Delivery,
            "Delivery",
            rs(40),
            TaxRate::from_percent(18).expect("18%"),
        );
        assert_eq!(delivery.compute_on(rs(1_000)), Ok(rs(40)));
        assert_eq!(delivery.compute_on(rs(500)), Ok(rs(40)));
        // Even on an empty bill it still computes rather than failing — a delivery charge with
        // no items is a mistake upstream, not a crash.
        assert_eq!(delivery.compute_on(Money::ZERO), Ok(rs(40)));
    }

    #[test]
    fn a_charge_can_be_untaxed_without_a_second_flag() {
        let tipless = Charge::flat(
            ChargeKind::Other("Donation".to_owned()),
            "Donation",
            rs(10),
            TaxRate::ZERO,
        )
        .with_tax(TaxSpec::exempt());
        assert_eq!(tipless.tax.kind, TaxKind::Exempt);
        assert!(tipless.is_coherent());

        // And a charge may not be alcohol.
        let impossible = Charge::flat(ChargeKind::Service, "Service", rs(10), TaxRate::ZERO)
            .with_tax(TaxSpec::liquor(TaxRate::ZERO));
        assert!(!impossible.is_coherent(), "a charge cannot be outside GST");
    }
}
