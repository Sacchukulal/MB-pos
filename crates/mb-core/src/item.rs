//! What is being sold: the frozen item snapshot, its modifiers, and the type
//! of order it belongs to.

use crate::ids::{CategoryId, ItemId, ModifierId};
use crate::money::Money;
use crate::tax::{PriceBasis, TaxKind, TaxRate, TaxSpec};
use serde::{Deserialize, Serialize};

/// How the order is being served.
///
/// `FEATURE_SCOPE.md` 1.5. v1 had exactly three — Self Service, Table and
/// Parcel. `DineIn` is v1's "Table". `Delivery` is new: 14.5 needs it for rider
/// assignment and 1.14 needs it for a delivery charge with its own tax rate.
///
/// It carries no behaviour here beyond being recorded on the bill. Order-type
/// pricing (scope 2.9) belongs to P13; the table number belongs to the order
/// (P03) and the floor plan (P14). It sits on `BillInput` now so that neither
/// of those has to change this signature later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    /// Eaten at a table in the restaurant. v1 called this "Table".
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
///
/// **Frozen onto the line and never looked up again.** The audit lists this as
/// one of v1's crown jewels (Part 10, item 4) in as many words:
///
/// > "Frozen item snapshots on every order. Old bills never change when you
/// > change a price."
///
/// Renaming an item or raising its price must never rewrite a bill printed last
/// month. Nothing in this crate resolves an `ItemId` at bill time — the
/// snapshot is the truth, and that is the entire reason it exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSnapshot {
    pub item_id: ItemId,
    pub name: String,
    pub unit_price: Money,
    /// **The whole tax question, frozen** — kind, rate and pricing basis.
    /// P33 replaced a rate plus a four-valued treatment, which could not say
    /// "outside GST at 20% VAT" at all.
    pub tax: TaxSpec,
    /// The HSN/SAC code printed on the bill (scope 2.5). Optional because
    /// alcohol under `NonGst` has none, and a shop below the turnover threshold
    /// is not required to print one.
    pub hsn: Option<String>,
    pub category_id: Option<CategoryId>,
    /// **Which kitchen screen this dish belongs on** (P24), frozen at the
    /// moment it was added — crown jewel 4's rule, the same one the tax rate
    /// follows (D52).
    ///
    /// Moving a category to a different station tonight must not move food
    /// that is already cooking. `None` is the shop's one screen.
    #[serde(default)]
    pub station: Option<String>,
    /// **Which course** (scope 3.5), also frozen. `None` means no course, and a
    /// shop where every dish is `None` fires the whole order at once — which is
    /// how a kitchen works unless somebody has said otherwise.
    #[serde(default)]
    pub course: Option<String>,
    /// How long the kitchen is expected to take on this dish (scope 3.6, set in
    /// P13). The ticket's target is the slowest dish on it.
    #[serde(default)]
    pub prep_minutes: Option<u32>,
}

impl ItemSnapshot {
    /// The everyday case: a taxed item, exclusive pricing, no HSN yet.
    #[must_use]
    pub fn new(item_id: ItemId, name: impl Into<String>, unit_price: Money, tax_rate: TaxRate) -> Self {
        ItemSnapshot {
            item_id,
            name: name.into(),
            unit_price,
            tax: TaxSpec { kind: TaxKind::Gst, rate: tax_rate, basis: PriceBasis::Exclusive },
            hsn: None,
            category_id: None,
            // The shop's one screen, no course, no target — which is the
            // everyday case and the one a shop that never opens these settings
            // stays in for ever.
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
///
/// **A modifier inherits the line's tax rate and treatment.** Extra cheese on a
/// 5% dish is charged at 5% — it is part of the same supply, not a separate
/// one. That is a tax decision rather than a data-model one, which is why it is
/// written down here where someone changing this struct will read it.
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
        Modifier { modifier_id, name: name.into(), price_delta }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_does_not_change_when_the_menu_does() {
        // The crown-jewel behaviour, stated as a test so it cannot be lost:
        // the snapshot holds values, not a reference to a menu row.
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
        assert_ne!(sold_at, today, "the same id must not mean the same snapshot");
    }

    #[test]
    fn order_types_cover_every_way_a_shop_serves() {
        assert_eq!(OrderType::default(), OrderType::DineIn);
        assert_eq!(OrderType::DineIn.label(), "Dine In");
        assert_eq!(OrderType::Parcel.label(), "Parcel");
        assert_eq!(OrderType::SelfService.label(), "Self Service");
        // New in v2 — v1 had no way to record a delivery at all.
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
