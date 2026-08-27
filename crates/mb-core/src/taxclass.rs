//! A shop's tax vocabulary — the thing that lets a bar bill legally.

use serde::{Deserialize, Serialize};

use crate::ids::ItemId;
use crate::tax::{TaxKind, TaxRate, TaxSpec};

/// Identifies a tax class.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaxClassId(String);

impl TaxClassId {
    pub fn new(id: impl Into<String>) -> Self {
        TaxClassId(id.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A named tax, as an owner edits it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxClass {
    pub id: TaxClassId,
    /// What an owner picks from a list: "Restaurant food 5%".
    pub name: String,
    /// The kind, the rate and the pricing basis, together.
    pub tax: TaxSpec,
    /// Retired rather than deleted — old bills point at this id, and a class an item still uses
    /// cannot go.
    pub is_active: bool,
}

impl TaxClass {
    #[must_use]
    pub fn new(id: TaxClassId, name: impl Into<String>, tax: TaxSpec) -> Self {
        TaxClass {
            id,
            name: name.into(),
            tax,
            is_active: true,
        }
    }

    /// A class that says two contradictory things is refused, not rounded off.
    #[must_use]
    pub fn is_coherent(&self) -> bool {
        self.tax.is_coherent()
    }

    /// Is this the vocabulary for alcohol?
    #[must_use]
    pub fn is_alcohol(&self) -> bool {
        self.tax.kind == TaxKind::OutsideGst
    }
}

/// The five a new shop starts with.
#[must_use]
pub fn starting_classes() -> Vec<TaxClass> {
    let pc = |percent: u32| TaxRate::from_percent(percent).unwrap_or(TaxRate::ZERO);
    vec![
        TaxClass::new(
            TaxClassId::new("tax_food_5"),
            "Restaurant food 5%",
            TaxSpec::gst(pc(5)),
        ),
        TaxClass::new(
            TaxClassId::new("tax_goods_5"),
            "Packaged goods 5%",
            TaxSpec::gst(pc(5)),
        ),
        TaxClass::new(
            TaxClassId::new("tax_packaged_18"),
            "Packaged goods 18%",
            TaxSpec::gst(pc(18)),
        ),
        TaxClass::new(
            TaxClassId::new("tax_liquor"),
            "Liquor — state VAT",
            TaxSpec::liquor(TaxRate::ZERO),
        ),
        TaxClass::new(TaxClassId::new("tax_exempt"), "Exempt", TaxSpec::exempt()),
    ]
}

/// What a menu item is, as the menu screen edits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: ItemId,
    pub name: String,
    pub tax_class: TaxClassId,
    pub is_available: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tax::PriceBasis;

    fn pc(percent: u32) -> TaxRate {
        TaxRate::from_percent(percent).expect("a real rate")
    }

    #[test]
    fn a_class_carries_one_answer_about_tax() {
        let food = TaxClass::new(
            TaxClassId::new("tax_food_5"),
            "Restaurant food 5%",
            TaxSpec::gst(pc(5)),
        );
        assert_eq!(food.tax.kind, TaxKind::Gst);
        assert_eq!(food.tax.rate, pc(5));
        assert_eq!(food.tax.basis, PriceBasis::Exclusive);
        assert!(food.is_coherent());
        assert!(!food.is_alcohol());
    }

    /// The commercial test. A shop starts able to describe food, packaged goods, something
    /// outside GST entirely, and a nil-rated supply.
    #[test]
    fn a_shop_starts_with_enough_to_bill_a_bar() {
        let classes = starting_classes();
        assert_eq!(classes.len(), 5);

        let liquor = classes
            .iter()
            .find(|c| c.id == TaxClassId::new("tax_liquor"))
            .expect("a shop must be able to sell liquor");
        assert!(liquor.is_alcohol());
        assert_eq!(liquor.tax.kind, TaxKind::OutsideGst);
        assert_eq!(
            liquor.tax.basis,
            PriceBasis::Inclusive,
            "a bar quotes the price paid"
        );
        assert_eq!(liquor.tax.kind, TaxKind::OutsideGst);

        let exempt = classes
            .iter()
            .find(|c| c.tax.kind == TaxKind::Exempt)
            .expect("exempt exists");
        // Exempt and outside-GST are DIFFERENT things and a return treats them differently:
        // exempt is a nil-rated supply, liquor is not a supply under GST at all.
        assert_ne!(exempt.tax.kind, liquor.tax.kind);
    }

    /// No abolished slab is seeded.
    #[test]
    fn no_seeded_class_uses_an_abolished_slab() {
        for class in starting_classes() {
            let bp = class.tax.rate.basis_points();
            assert!(bp != 1_200 && bp != 2_800, "{} seeds {bp}bp", class.name);
        }
    }

    /// Every seed must make sense on its own terms — no rate on a kind that cannot carry one.
    #[test]
    fn every_seeded_class_is_coherent() {
        for class in starting_classes() {
            assert!(class.is_coherent(), "{} contradicts itself", class.name);
        }
    }

    /// Liquor is seeded with no rate: VAT is set by the state, so the shop must say.
    #[test]
    fn the_liquor_seed_asks_the_shop_for_its_vat_rate() {
        let liquor = starting_classes()
            .into_iter()
            .find(|c| c.is_alcohol())
            .expect("liquor exists");
        assert!(liquor.tax.rate.is_zero());
        assert!(liquor.name.contains("VAT"), "{}", liquor.name);
    }

    #[test]
    fn a_class_that_contradicts_itself_is_not_coherent() {
        let wrong = TaxClass::new(
            TaxClassId::new("tax_bad"),
            "Exempt but five per cent",
            TaxSpec {
                kind: TaxKind::Exempt,
                rate: pc(5),
                basis: PriceBasis::Exclusive,
            },
        );
        assert!(!wrong.is_coherent());
    }

    #[test]
    fn a_class_round_trips_through_serde() {
        // Nothing reachable from an order may serialise with a non-string map key.
        let class = TaxClass::new(
            TaxClassId::new("tax_liquor"),
            "Liquor — state VAT",
            TaxSpec::liquor(pc(20)),
        );
        let json = serde_json::to_string(&class).expect("serialises");
        let back: TaxClass = serde_json::from_str(&json).expect("round trips");
        assert_eq!(back, class);
    }
}
