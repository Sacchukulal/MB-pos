//! **A shop's tax vocabulary** — the thing that lets a bar bill legally.
//!
//! > Audit **B10 / B11 / B14**, which are one finding from three sides: v1 had
//! > one tax rate for the whole shop, no per-item rate, and no way to mark
//! > anything as outside GST. *"It could not bill a bar, an AC/non-AC outlet or
//! > anyone selling packaged goods."*
//!
//! P00 built the engine — [`TaxRate`], [`TaxTreatment`] and the rate-wise
//! summary. What was missing was a way for an **owner** to reach it: nobody
//! sets up four hundred items by choosing basis points each time. They pick
//! *"Restaurant food 5%"* once and put it on a category.
//!
//! # Why a class lives here and never reaches a bill
//!
//! [`TaxClass::for_order_type`] is a **rule** — some states tax the same dish
//! differently to take away — and rules live in mb-core, not in SQL and not in
//! React (R8, D1).
//!
//! But the class itself is never on a bill. What reaches a bill is
//! [`ItemSnapshot`](crate::item::ItemSnapshot), carrying the **resolved** rate
//! and treatment, frozen at the moment the line was added:
//!
//! > Crown jewel 4: *"frozen item snapshots on every order; old bills never
//! > change when you change a price."*
//!
//! So editing a class cannot rewrite history — and, the subtler case, cannot
//! change the lines already on an order that is still open. Both are true by
//! construction rather than by care, and P13's T2 and T3 keep them that way.

use serde::{Deserialize, Serialize};

use crate::ids::ItemId;
use crate::item::OrderType;
use crate::tax::{TaxRate, TaxTreatment};

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

/// A named rate and treatment, with the overrides a state's rules need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxClass {
    pub id: TaxClassId,
    /// What an owner picks from a list: "Restaurant food 5%".
    pub name: String,
    pub rate: TaxRate,
    pub treatment: TaxTreatment,
    /// **Scope 6.8's half that belongs to the menu.** Some states tax the same
    /// dish differently to take away, so this is the RATE by order type; P13b
    /// owns the PRICE by order type, and reads this rather than duplicating it.
    ///
    /// A `Vec` rather than a map: it is never more than four entries, it has to
    /// round-trip through an internally-tagged enum, and **D20** says nothing
    /// reachable from an order may serialise with a non-string map key.
    pub by_order_type: Vec<OrderTypeRate>,
    /// Retired rather than deleted — old bills point at this id, and a class an
    /// item still uses cannot go.
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderTypeRate {
    pub order_type: OrderType,
    pub rate: TaxRate,
    pub treatment: TaxTreatment,
}

impl TaxClass {
    #[must_use]
    pub fn new(
        id: TaxClassId,
        name: impl Into<String>,
        rate: TaxRate,
        treatment: TaxTreatment,
    ) -> Self {
        TaxClass {
            id,
            name: name.into(),
            rate,
            treatment,
            by_order_type: Vec::new(),
            is_active: true,
        }
    }

    #[must_use]
    pub fn with_override(
        mut self,
        order_type: OrderType,
        rate: TaxRate,
        treatment: TaxTreatment,
    ) -> Self {
        self.by_order_type.retain(|o| o.order_type != order_type);
        self.by_order_type.push(OrderTypeRate {
            order_type,
            rate,
            treatment,
        });
        self
    }

    /// **The rule.** What this class means for this kind of order.
    #[must_use]
    pub fn for_order_type(&self, kind: OrderType) -> (TaxRate, TaxTreatment) {
        self.by_order_type
            .iter()
            .find(|o| o.order_type == kind)
            .map_or((self.rate, self.treatment), |o| (o.rate, o.treatment))
    }
}

/// The five a new shop starts with.
///
/// Seeded, like the permissions (P11) and the correction reasons (P12) before
/// them: a starting point a shop can add to, not a list in the source that a
/// support call has to change.
///
/// **"Liquor — outside GST" is the one that matters commercially.** State
/// excise is not GST at all, and v1 having no way to say so is why a bar could
/// not use it.
#[must_use]
pub fn starting_classes() -> Vec<TaxClass> {
    vec![
        TaxClass::new(
            TaxClassId::new("tax_food_5"),
            "Restaurant food 5%",
            TaxRate::GST_5,
            TaxTreatment::Exclusive,
        ),
        TaxClass::new(
            TaxClassId::new("tax_packaged_12"),
            "Packaged goods 12%",
            TaxRate::GST_12,
            TaxTreatment::Exclusive,
        ),
        TaxClass::new(
            TaxClassId::new("tax_packaged_18"),
            "Packaged goods 18%",
            TaxRate::GST_18,
            TaxTreatment::Exclusive,
        ),
        TaxClass::new(
            TaxClassId::new("tax_liquor"),
            "Liquor — outside GST",
            TaxRate::ZERO,
            TaxTreatment::NonGst,
        ),
        TaxClass::new(
            TaxClassId::new("tax_exempt"),
            "Exempt",
            TaxRate::ZERO,
            TaxTreatment::Exempt,
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

    #[test]
    fn a_class_answers_with_its_own_rate_by_default() {
        let food = TaxClass::new(
            TaxClassId::new("tax_food_5"),
            "Restaurant food 5%",
            TaxRate::GST_5,
            TaxTreatment::Exclusive,
        );
        for kind in [
            OrderType::DineIn,
            OrderType::Parcel,
            OrderType::SelfService,
            OrderType::Delivery,
        ] {
            assert_eq!(
                food.for_order_type(kind),
                (TaxRate::GST_5, TaxTreatment::Exclusive)
            );
        }
    }

    #[test]
    fn an_override_applies_to_its_own_order_type_and_no_other() {
        // The real case: some states tax the same dish differently to take
        // away, which is scope 6.8's tax half.
        let food = TaxClass::new(
            TaxClassId::new("tax_food_5"),
            "Restaurant food 5%",
            TaxRate::GST_5,
            TaxTreatment::Exclusive,
        )
        .with_override(OrderType::Parcel, TaxRate::GST_18, TaxTreatment::Exclusive);

        assert_eq!(
            food.for_order_type(OrderType::Parcel),
            (TaxRate::GST_18, TaxTreatment::Exclusive)
        );
        assert_eq!(
            food.for_order_type(OrderType::DineIn),
            (TaxRate::GST_5, TaxTreatment::Exclusive)
        );
    }

    #[test]
    fn setting_an_override_twice_replaces_it() {
        let class = TaxClass::new(
            TaxClassId::new("t"),
            "T",
            TaxRate::GST_5,
            TaxTreatment::Exclusive,
        )
        .with_override(OrderType::Parcel, TaxRate::GST_12, TaxTreatment::Exclusive)
        .with_override(OrderType::Parcel, TaxRate::GST_18, TaxTreatment::Exclusive);

        assert_eq!(class.by_order_type.len(), 1, "the override was duplicated");
        assert_eq!(
            class.for_order_type(OrderType::Parcel).0,
            TaxRate::GST_18
        );
    }

    #[test]
    fn a_shop_starts_with_enough_to_bill_a_bar() {
        // The commercial test: food, packaged goods and something outside GST
        // entirely. v1 could express exactly one of these.
        let classes = starting_classes();
        assert_eq!(classes.len(), 5);

        let liquor = classes
            .iter()
            .find(|c| c.id == TaxClassId::new("tax_liquor"))
            .expect("a shop must be able to sell liquor");
        assert_eq!(liquor.treatment, TaxTreatment::NonGst);

        let exempt = classes
            .iter()
            .find(|c| c.treatment == TaxTreatment::Exempt)
            .expect("exempt exists");
        // Exempt and non-GST are DIFFERENT things and a return treats them
        // differently: exempt is a nil-rated supply, liquor is not a supply
        // under GST at all.
        assert_ne!(exempt.treatment, liquor.treatment);

        let rates: Vec<u32> = classes.iter().map(|c| c.rate.basis_points()).collect();
        assert!(rates.contains(&500) && rates.contains(&1_200) && rates.contains(&1_800));
    }

    #[test]
    fn a_class_round_trips_through_serde() {
        // D20: nothing reachable from an order may serialise with a non-string
        // map key, which is why the overrides are a Vec.
        let class = starting_classes()
            .into_iter()
            .next()
            .expect("at least one")
            .with_override(OrderType::Delivery, TaxRate::GST_18, TaxTreatment::Inclusive);
        let json = serde_json::to_string(&class).expect("serialises");
        let back: TaxClass = serde_json::from_str(&json).expect("round trips");
        assert_eq!(back, class);
    }
}
