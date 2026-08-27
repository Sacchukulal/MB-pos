//! What is being sold: the frozen item snapshot, its modifiers, and the type of order it
//! belongs to.

use crate::ids::{CategoryId, ItemId, ModifierId};
use crate::money::Money;
use crate::tax::{PriceBasis, TaxKind, TaxRate, TaxSpec};
use serde::{Deserialize, Serialize};

/// How the order is being served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    /// Eaten at a table in the restaurant.
    #[default]
    DineIn,
    /// Packed and carried away by the customer.
    Parcel,
    /// The customer orders and collects at the counter.
    SelfService,
    /// Sent out with a rider.
    Delivery,
}

impl OrderType {
    /// For the receipt and the screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            OrderType::DineIn => "Dine In",
            OrderType::Parcel => "Parcel",
            OrderType::SelfService => "Self Service",
            OrderType::Delivery => "Delivery",
        }
    }
}

/// What an item was, at the moment it was sold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSnapshot {
    pub item_id: ItemId,
    pub name: String,
    pub unit_price: Money,
    /// The whole tax question, frozen — kind, rate and pricing basis.
    pub tax: TaxSpec,
    /// The HSN/SAC code printed on the bill.
    pub hsn: Option<String>,
    pub category_id: Option<CategoryId>,
    /// Which kitchen screen this dish belongs on, frozen at the moment it was added.
    #[serde(default)]
    pub station: Option<String>,
    /// Which course, also frozen.
    #[serde(default)]
    pub course: Option<String>,
    /// How long the kitchen is expected to take on this dish.
    #[serde(default)]
    pub prep_minutes: Option<u32>,
}

impl ItemSnapshot {
    /// The everyday case: a taxed item, exclusive pricing, no HSN yet.
    #[must_use]
    pub fn new(
        item_id: ItemId,
        name: impl Into<String>,
        unit_price: Money,
        tax_rate: TaxRate,
    ) -> Self {
        ItemSnapshot {
            item_id,
            name: name.into(),
            unit_price,
            tax: TaxSpec {
                kind: TaxKind::Gst,
                rate: tax_rate,
                basis: PriceBasis::Exclusive,
            },
            hsn: None,
            category_id: None,
            // The shop's one screen, no course, no target — which is the everyday case and the
            // one a shop that never opens these settings stays in for ever.
            station: None,
            course: None,
            prep_minutes: None,
        }
    }

    #[must_use]
    /// The whole tax question at once — the way a menu item hands it over.
    pub fn with_tax(mut self, tax: TaxSpec) -> Self {
        self.tax = tax;
        self
    }

    /// Is the price on this line already tax-in?
    #[must_use]
    pub fn is_inclusive(&self) -> bool {
        self.tax.basis.is_inclusive()
    }

    #[must_use]
    pub fn with_hsn(mut self, hsn: impl Into<String>) -> Self {
        self.hsn = Some(hsn.into());
        self
    }

    #[must_use]
    pub fn with_category(mut self, category_id: CategoryId) -> Self {
        self.category_id = Some(category_id);
        self
    }
}

/// A change to a line: "extra cheese", "no onion", "less spicy".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifier {
    pub modifier_id: ModifierId,
    pub name: String,
    /// May be negative — "no cheese, −₹10" is a real thing on a real menu.
    pub price_delta: Money,
}

impl Modifier {
    #[must_use]
    pub fn new(modifier_id: ModifierId, name: impl Into<String>, price_delta: Money) -> Self {
        Modifier {
            modifier_id,
            name: name.into(),
            price_delta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_does_not_change_when_the_menu_does() {
        // The crown-jewel behaviour, stated as a test so it cannot be lost: the snapshot holds
        // values, not a reference to a menu row.
        let sold_at = ItemSnapshot::new(
            ItemId::new("itm_biryani"),
            "Chicken Biryani",
            Money::from_paise(24_000),
            TaxRate::from_percent(5).expect("5%"),
        );

        // The shop raises the price and renames the dish tomorrow.
        let today = ItemSnapshot::new(
            ItemId::new("itm_biryani"),
            "Chicken Biryani (Special)",
            Money::from_paise(28_000),
            TaxRate::from_percent(5).expect("5%"),
        );

        assert_eq!(sold_at.unit_price, Money::from_paise(24_000));
        assert_eq!(sold_at.name, "Chicken Biryani");
        assert_ne!(
            sold_at, today,
            "the same id must not mean the same snapshot"
        );
    }

    #[test]
    fn order_types_cover_every_way_a_shop_serves() {
        assert_eq!(OrderType::default(), OrderType::DineIn);
        assert_eq!(OrderType::DineIn.label(), "Dine In");
        assert_eq!(OrderType::Parcel.label(), "Parcel");
        assert_eq!(OrderType::SelfService.label(), "Self Service");
        // New in v2.
        assert_eq!(OrderType::Delivery.label(), "Delivery");
    }

    #[test]
    fn a_modifier_may_reduce_the_price() {
        let no_cheese = Modifier::new(
            ModifierId::new("mod_nocheese"),
            "No Cheese",
            Money::from_paise(-1_000),
        );
        assert!(no_cheese.price_delta.is_negative());
    }
}
