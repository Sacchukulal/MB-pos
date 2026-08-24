//! **A shop's tax vocabulary** — the thing that lets a bar bill legally.
//!
//! > Audit **B10 / B11 / B14**, which are one finding from three sides: v1 had
//! > one tax rate for the whole shop, no per-item rate, and no way to mark
//! > anything as outside GST. *"It could not bill a bar, an AC/non-AC outlet or
//! > anyone selling packaged goods."*
//!
//! P00 built the engine — [`TaxRate`], [`TaxSpec`] and the rate-wise summary.
//! What was missing was a way for an **owner** to reach it: nobody sets up four
//! hundred items by choosing basis points each time. They pick *"Restaurant food
//! 5%"* once and put it on a category.
//!
//! # Why a class never reaches a bill
//!
//! What reaches a bill is [`ItemSnapshot`](crate::item::ItemSnapshot), carrying
//! the **resolved** [`TaxSpec`], frozen at the moment the line was added:
//!
//! > Crown jewel 4: *"frozen item snapshots on every order; old bills never
//! > change when you change a price."*
//!
//! So editing a class cannot rewrite history — and, the subtler case, cannot
//! change the lines already on an order that is still open. Both are true by
//! construction rather than by care, and P13's T2 and T3 keep them that way.
//!
//! # P33 — what changed here
//!
//! **The per-order-type rate override is gone.** `by_order_type`,
//! `OrderTypeRate`, `with_override` and `for_order_type` were modelled here,
//! given a database table, written by the repository — and **read by no caller
//! anywhere in the product** (audit §3.5). The belief that justified them, that
//! *"some states tax the same dish differently to take away"*, is not current
//! law either: parcel from a restaurant is restaurant service at the same rate
//! as dine-in. A rule that cannot fire, resting on a fact that is not true, is
//! deleted rather than wired up.
//!
//! **A class now carries a [`TaxSpec`]**, so it can say the thing the old model
//! could not: *outside GST, at 20% state VAT, priced tax-in*. That sentence is
//! what a bar needs and what the old four-valued treatment could not express.

use serde::{Deserialize, Serialize};

use crate::ids::ItemId;
use crate::tax::{TaxKind, TaxRate, TaxSpec};

/// Identifies a tax class. Text, like every other id (D13).
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
    /// Retired rather than deleted — old bills point at this id, and a class an
    /// item still uses cannot go.
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

    /// **A class that says two contradictory things is refused, not rounded
    /// off.** An exempt class carrying 5% has had two different answers typed
    /// into it, and the owner is the only one who knows which to keep (D7, D15).
    #[must_use]
    pub fn is_coherent(&self) -> bool {
        self.tax.is_coherent()
    }

    /// Is this the vocabulary for alcohol? Asked by the settings screen, because
    /// a composition dealer may not sell it at all.
    #[must_use]
    pub fn is_alcohol(&self) -> bool {
        self.tax.kind == TaxKind::OutsideGst
    }
}

/// The five a new shop starts with.
///
/// A starting point a shop adds to, not a list a support call has to change.
/// Migration 0004 seeds the same five, so the two must agree.
///
/// The 12% slab was abolished on 22 September 2025 and is not seeded. Liquor
/// carries a state VAT rate the shop sets; 0 means "not told yet".
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
        TaxClass::new(
            TaxClassId::new("tax_exempt"),
            "Exempt",
            TaxSpec::exempt(),
        ),
    ]
}

/// What a menu item is, as the menu screen edits it.
///
/// Deliberately **not** [`ItemSnapshot`](crate::item::ItemSnapshot): that is
/// what a line froze, and this is what the shop currently sells. Keeping them
/// apart is crown jewel 4 made structural — there is no way to hand a live
/// menu row to a bill by accident, because the types do not fit.
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

    /// **The commercial test.** A shop starts able to describe food, packaged
    /// goods, something outside GST entirely, and a nil-rated supply. v1 could
    /// express exactly one of these.
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
        assert_eq!(liquor.tax.basis, PriceBasis::Inclusive, "a bar quotes the price paid");
        assert_eq!(liquor.tax.kind, TaxKind::OutsideGst);

        let exempt = classes
            .iter()
            .find(|c| c.tax.kind == TaxKind::Exempt)
            .expect("exempt exists");
        // Exempt and outside-GST are DIFFERENT things and a return treats them
        // differently: exempt is a nil-rated supply, liquor is not a supply
        // under GST at all.
        assert_ne!(exempt.tax.kind, liquor.tax.kind);
    }

    /// No abolished slab is seeded. 12% and 28% ended on 22 September 2025.
    #[test]
    fn no_seeded_class_uses_an_abolished_slab() {
        for class in starting_classes() {
            let bp = class.tax.rate.basis_points();
            assert!(bp != 1_200 && bp != 2_800, "{} seeds {bp}bp", class.name);
        }
    }

    /// Every seed must make sense on its own terms — no rate on a kind that
    /// cannot carry one.
    #[test]
    fn every_seeded_class_is_coherent() {
        for class in starting_classes() {
            assert!(class.is_coherent(), "{} contradicts itself", class.name);
        }
    }

    /// Liquor is seeded with no rate: VAT is set by the state, so the shop
    /// must say. The name has to make that obvious.
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
            TaxSpec { kind: TaxKind::Exempt, rate: pc(5), basis: PriceBasis::Exclusive },
        );
        assert!(!wrong.is_coherent());
    }

    #[test]
    fn a_class_round_trips_through_serde() {
        // D20: nothing reachable from an order may serialise with a non-string
        // map key.
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
