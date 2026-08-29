//! A shop's tax slabs, and the ONE place an item's tax is worked out.
//!
//! A slab is a rate and a kind. Whether the price already contains the tax is decided in
//! three layers — the item, then the slab, then the shop — and `TaxBook::spec_for` is the only
//! function that reads those layers. Nothing else in the product stores a resolved tax
//! against an item; a line freezes its `TaxSpec` when it is sold, and that is the only copy.

use serde::{Deserialize, Serialize};

use crate::tax::{PriceBasis, TaxKind, TaxRate, TaxSpec};

/// Identifies a tax slab.
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

/// A slab, as an owner edits it on the Tax page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxClass {
    pub id: TaxClassId,
    /// What an owner picks from a list: "GST 5%".
    pub name: String,
    pub kind: TaxKind,
    pub rate: TaxRate,
    /// `None` = the shop's own setting decides. `Some` = this slab always prices this way
    /// (liquor is quoted tax-in whatever the shop does with food).
    pub basis: Option<PriceBasis>,
    /// Retired rather than deleted while anything points at it.
    pub is_active: bool,
}

impl TaxClass {
    #[must_use]
    pub fn new(id: TaxClassId, name: impl Into<String>, kind: TaxKind, rate: TaxRate) -> Self {
        TaxClass {
            id,
            name: name.into(),
            kind,
            rate,
            basis: None,
            is_active: true,
        }
    }

    #[must_use]
    pub fn with_basis(mut self, basis: PriceBasis) -> Self {
        self.basis = Some(basis);
        self
    }

    /// A slab that says two contradictory things is refused, not rounded off.
    #[must_use]
    pub fn is_coherent(&self) -> bool {
        match self.kind {
            TaxKind::Gst | TaxKind::OutsideGst => true,
            TaxKind::Exempt | TaxKind::Untaxed => self.rate.is_zero(),
        }
    }

    /// Is this the vocabulary for alcohol?
    #[must_use]
    pub fn is_alcohol(&self) -> bool {
        self.kind == TaxKind::OutsideGst
    }

    /// The tax for something on this slab, given the shop's default and the item's own say.
    #[must_use]
    pub fn spec(&self, shop: PriceBasis, item: Option<PriceBasis>) -> TaxSpec {
        TaxSpec {
            kind: self.kind,
            rate: self.rate,
            basis: item.or(self.basis).unwrap_or(shop),
        }
    }
}

/// The slabs a new shop starts with. Every one can be renamed, re-rated or removed.
#[must_use]
pub fn starting_classes() -> Vec<TaxClass> {
    let pc = |percent: u32| TaxRate::from_percent(percent).unwrap_or(TaxRate::ZERO);
    vec![
        TaxClass::new(TaxClassId::new("tax_gst_0"), "GST 0%", TaxKind::Gst, pc(0)),
        TaxClass::new(TaxClassId::new("tax_food_5"), "GST 5%", TaxKind::Gst, pc(5)),
        TaxClass::new(TaxClassId::new("tax_packaged_18"), "GST 18%", TaxKind::Gst, pc(18)),
        TaxClass::new(TaxClassId::new("tax_gst_40"), "GST 40%", TaxKind::Gst, pc(40)),
        TaxClass::new(TaxClassId::new("tax_exempt"), "Exempt", TaxKind::Exempt, pc(0)),
        TaxClass::new(
            TaxClassId::new("tax_liquor"),
            "Liquor — state VAT",
            TaxKind::OutsideGst,
            pc(0),
        )
        .with_basis(PriceBasis::Inclusive),
    ]
}

/// The seeded slab that carries this kind and rate, if one does — for seeders and fixtures that
/// think in rates. The retired 12% seed is named too, so an old menu can still be described.
#[must_use]
pub fn seeded_slab_for(kind: TaxKind, rate: TaxRate) -> Option<TaxClassId> {
    let id = match (kind, rate.basis_points()) {
        (TaxKind::Gst, 0) => "tax_gst_0",
        (TaxKind::Gst, 500) => "tax_food_5",
        (TaxKind::Gst, 1_200) => "tax_packaged_12",
        (TaxKind::Gst, 1_800) => "tax_packaged_18",
        (TaxKind::Gst, 4_000) => "tax_gst_40",
        (TaxKind::Exempt, _) => "tax_exempt",
        (TaxKind::OutsideGst, _) => "tax_liquor",
        _ => return None,
    };
    Some(TaxClassId::new(id))
}

/// For a seeder or a fixture written in terms of a `TaxSpec`: the seeded slab that carries it,
/// and the item's own say when the spec prices tax-in. `None` when no seeded slab has that rate.
#[must_use]
pub fn seeded_placement(spec: TaxSpec) -> Option<(TaxClassId, Option<PriceBasis>)> {
    let slab = seeded_slab_for(spec.kind, spec.rate)?;
    let own = spec.basis.is_inclusive().then_some(PriceBasis::Inclusive);
    Some((slab, own))
}

/// Why a tax could not be worked out.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaxBookError {
    #[error("there is no tax slab {0}")]
    NoSuchSlab(String),
    #[error("the tax slab {0} has been removed")]
    Retired(String),
}

/// Every slab the shop has, and the shop's own pricing default — read once, asked many times.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaxBook {
    pub classes: Vec<TaxClass>,
    /// What "price" means on this shop's menu unless a slab or an item says otherwise.
    pub shop_basis: PriceBasis,
}

impl TaxBook {
    #[must_use]
    pub fn new(classes: Vec<TaxClass>, shop_basis: PriceBasis) -> Self {
        TaxBook {
            classes,
            shop_basis,
        }
    }

    #[must_use]
    pub fn find(&self, id: &TaxClassId) -> Option<&TaxClass> {
        self.classes.iter().find(|c| &c.id == id)
    }

    /// The slabs offered to somebody choosing one today.
    pub fn active(&self) -> impl Iterator<Item = &TaxClass> {
        self.classes.iter().filter(|c| c.is_active)
    }

    /// The whole tax question for one thing, answered from its slab and the layers above it.
    pub fn spec_for(
        &self,
        id: &TaxClassId,
        item_basis: Option<PriceBasis>,
    ) -> Result<TaxSpec, TaxBookError> {
        let class = self
            .find(id)
            .ok_or_else(|| TaxBookError::NoSuchSlab(id.as_str().to_owned()))?;
        Ok(class.spec(self.shop_basis, item_basis))
    }

    /// The same, for something that may only use a live slab — a new item, a charge.
    pub fn spec_for_live(
        &self,
        id: &TaxClassId,
        item_basis: Option<PriceBasis>,
    ) -> Result<TaxSpec, TaxBookError> {
        let class = self
            .find(id)
            .ok_or_else(|| TaxBookError::NoSuchSlab(id.as_str().to_owned()))?;
        if !class.is_active {
            return Err(TaxBookError::Retired(id.as_str().to_owned()));
        }
        Ok(class.spec(self.shop_basis, item_basis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(percent: u32) -> TaxRate {
        TaxRate::from_percent(percent).expect("a real rate")
    }

    fn book(shop: PriceBasis) -> TaxBook {
        TaxBook::new(starting_classes(), shop)
    }

    #[test]
    fn a_slab_is_a_rate_and_a_kind_and_nothing_else_until_asked() {
        let food = TaxClass::new(TaxClassId::new("tax_food_5"), "GST 5%", TaxKind::Gst, pc(5));
        assert_eq!(food.basis, None, "a plain slab follows the shop");
        assert!(food.is_coherent());
        assert!(!food.is_alcohol());
        assert_eq!(
            food.spec(PriceBasis::Exclusive, None),
            TaxSpec::gst(pc(5))
        );
        assert_eq!(
            food.spec(PriceBasis::Inclusive, None),
            TaxSpec::gst_inclusive(pc(5))
        );
    }

    #[test]
    fn the_item_beats_the_slab_and_the_slab_beats_the_shop() {
        let b = book(PriceBasis::Exclusive);
        let food = TaxClassId::new("tax_food_5");
        let liquor = TaxClassId::new("tax_liquor");
        // Shop says added; the food slab has no opinion; the item says nothing.
        assert_eq!(
            b.spec_for(&food, None).expect("food").basis,
            PriceBasis::Exclusive
        );
        // The item overrides the shop.
        assert_eq!(
            b.spec_for(&food, Some(PriceBasis::Inclusive))
                .expect("food")
                .basis,
            PriceBasis::Inclusive
        );
        // Liquor is quoted tax-in whatever the shop does with food.
        assert_eq!(
            b.spec_for(&liquor, None).expect("liquor").basis,
            PriceBasis::Inclusive
        );
        // …unless the item itself says otherwise.
        assert_eq!(
            b.spec_for(&liquor, Some(PriceBasis::Exclusive))
                .expect("liquor")
                .basis,
            PriceBasis::Exclusive
        );
    }

    #[test]
    fn an_inclusive_shop_makes_every_plain_slab_inclusive() {
        let b = book(PriceBasis::Inclusive);
        for class in b.active() {
            if class.basis.is_none() {
                assert_eq!(
                    b.spec_for(&class.id, None).expect("spec").basis,
                    PriceBasis::Inclusive,
                    "{}",
                    class.name
                );
            }
        }
    }

    #[test]
    fn a_missing_or_retired_slab_is_an_error_not_a_zero() {
        let mut b = book(PriceBasis::Exclusive);
        assert_eq!(
            b.spec_for(&TaxClassId::new("tax_nope"), None),
            Err(TaxBookError::NoSuchSlab("tax_nope".to_owned()))
        );
        b.classes[1].is_active = false;
        let id = b.classes[1].id.clone();
        assert!(b.spec_for(&id, None).is_ok(), "an old item may still read it");
        assert_eq!(
            b.spec_for_live(&id, None),
            Err(TaxBookError::Retired(id.as_str().to_owned())),
            "but nothing new may take it"
        );
    }

    #[test]
    fn a_shop_starts_with_enough_to_bill_a_bar() {
        let classes = starting_classes();
        assert_eq!(classes.len(), 6);
        let liquor = classes
            .iter()
            .find(|c| c.id == TaxClassId::new("tax_liquor"))
            .expect("a shop must be able to sell liquor");
        assert!(liquor.is_alcohol());
        assert_eq!(liquor.basis, Some(PriceBasis::Inclusive), "a bar quotes the price paid");
        assert!(liquor.rate.is_zero(), "the shop sets its own state's VAT");
        assert!(classes.iter().any(|c| c.kind == TaxKind::Exempt));
    }

    #[test]
    fn no_seeded_slab_uses_an_abolished_rate() {
        for class in starting_classes() {
            let bp = class.rate.basis_points();
            assert!(bp != 1_200 && bp != 2_800, "{} seeds {bp}bp", class.name);
        }
    }

    #[test]
    fn every_seeded_slab_is_coherent_and_live() {
        for class in starting_classes() {
            assert!(class.is_coherent(), "{} contradicts itself", class.name);
            assert!(class.is_active, "{} starts switched off", class.name);
        }
    }

    #[test]
    fn a_slab_that_contradicts_itself_is_not_coherent() {
        let wrong = TaxClass::new(
            TaxClassId::new("tax_bad"),
            "Exempt but five per cent",
            TaxKind::Exempt,
            pc(5),
        );
        assert!(!wrong.is_coherent());
        let fine = TaxClass::new(TaxClassId::new("tax_ok"), "Exempt", TaxKind::Exempt, pc(0));
        assert!(fine.is_coherent());
    }
}
