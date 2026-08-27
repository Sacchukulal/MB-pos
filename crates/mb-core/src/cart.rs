//! The cart: the lines the cashier has typed, before anything is computed.

use crate::discount::DiscountEntry;
use crate::ids::{ItemId, ModifierId};
use crate::item::{ItemSnapshot, Modifier};
use crate::qty::Qty;
use serde::{Deserialize, Serialize};

/// Something that could not be done to the cart.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CartError {
    #[error("there is no line {index} — the order has {len} line(s)")]
    NoSuchLine { index: usize, len: usize },
    #[error("a quantity must be more than zero")]
    NonPositiveQty,
}

type Result<T> = std::result::Result<T, CartError>;

/// One line of the order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CartLine {
    pub snapshot: ItemSnapshot,
    pub qty: Qty,
    pub note: Option<String>,
    pub modifiers: Vec<Modifier>,
    /// The discount as given, with its reason and who authorised it.
    pub line_discount: Option<DiscountEntry>,
}

impl CartLine {
    /// The rule that decides whether adding an item makes a new line or increases an existing
    /// one: item, note, and the set of modifiers.
    #[must_use]
    pub fn identity(&self) -> LineIdentity {
        let mut modifier_ids: Vec<ModifierId> = self
            .modifiers
            .iter()
            .map(|m| m.modifier_id.clone())
            .collect();
        // Sorted, so the order the waiter tapped the modifiers in cannot create a second line
        // for the same dish.
        modifier_ids.sort_unstable();
        LineIdentity {
            item_id: self.snapshot.item_id.clone(),
            note: self.note.clone(),
            modifier_ids,
        }
    }
}

/// What makes two lines "the same thing".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LineIdentity {
    pub item_id: ItemId,
    pub note: Option<String>,
    /// Sorted ascending, always. Built by `CartLine::identity`, which is the only thing that
    /// should build one.
    pub modifier_ids: Vec<ModifierId>,
}

/// Normalise a note the way line identity expects it.
fn normalise_note(note: Option<String>) -> Option<String> {
    note.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty())
}

/// The lines of one order, in the sequence they were called out.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cart {
    lines: Vec<CartLine>,
}

impl Cart {
    #[must_use]
    pub fn new() -> Self {
        Cart::default()
    }

    /// Add an item, merging into an existing line of the same identity.
    pub fn add(
        &mut self,
        snapshot: ItemSnapshot,
        qty: Qty,
        note: Option<String>,
        modifiers: Vec<Modifier>,
    ) -> Result<usize> {
        if !qty.is_positive() {
            return Err(CartError::NonPositiveQty);
        }

        let candidate = CartLine {
            snapshot,
            qty,
            note: normalise_note(note),
            modifiers,
            line_discount: None,
        };

        let key = candidate.identity();
        if let Some(index) = self.lines.iter().position(|line| line.identity() == key) {
            // Adding the same thing again increases the quantity.
            let merged = self.lines[index]
                .qty
                .add(candidate.qty)
                .map_err(|_| CartError::NonPositiveQty)?;
            self.lines[index].qty = merged;
            return Ok(index);
        }

        self.lines.push(candidate);
        Ok(self.lines.len() - 1)
    }

    /// Change a line's quantity.
    pub fn set_qty(&mut self, index: usize, qty: Qty) -> Result<()> {
        self.check(index)?;
        if qty.is_negative() {
            return Err(CartError::NonPositiveQty);
        }
        if qty.is_zero() {
            self.lines.remove(index);
        } else {
            self.lines[index].qty = qty;
        }
        Ok(())
    }

    pub fn set_line_discount(
        &mut self,
        index: usize,
        discount: Option<DiscountEntry>,
    ) -> Result<()> {
        self.check(index)?;
        self.lines[index].line_discount = discount;
        Ok(())
    }

    /// Change a line's note.
    pub fn set_note(&mut self, index: usize, note: Option<String>) -> Result<usize> {
        self.check(index)?;
        self.lines[index].note = normalise_note(note);

        let key = self.lines[index].identity();
        let twin = self
            .lines
            .iter()
            .position(|line| line.identity() == key)
            .filter(|found| *found != index);

        match twin {
            Some(target) => {
                let moved = self.lines.remove(index);
                // Removing a line before the target shifts it down by one.
                let target = if index < target { target - 1 } else { target };
                let merged = self.lines[target]
                    .qty
                    .add(moved.qty)
                    .map_err(|_| CartError::NonPositiveQty)?;
                self.lines[target].qty = merged;
                Ok(target)
            }
            None => Ok(index),
        }
    }

    /// Put a whole line on, merging it into an existing one where that is honest — the arrival
    /// half of a merge or a split.
    pub fn push(&mut self, line: CartLine) -> Result<usize> {
        if !line.qty.is_positive() {
            return Err(CartError::NonPositiveQty);
        }
        let key = line.identity();
        let twin = self.lines.iter().position(|existing| {
            existing.identity() == key && existing.line_discount == line.line_discount
        });

        match twin {
            Some(index) => {
                let merged = self.lines[index]
                    .qty
                    .add(line.qty)
                    .map_err(|_| CartError::NonPositiveQty)?;
                self.lines[index].qty = merged;
                Ok(index)
            }
            None => {
                self.lines.push(line);
                Ok(self.lines.len() - 1)
            }
        }
    }

    pub fn remove(&mut self, index: usize) -> Result<CartLine> {
        self.check(index)?;
        Ok(self.lines.remove(index))
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[must_use]
    pub fn lines(&self) -> &[CartLine] {
        &self.lines
    }

    /// How many lines make a ticket somebody should look at.
    pub const LONG_ORDER: usize = 40;

    /// True when this order is long enough to be worth mentioning.
    #[must_use]
    pub fn is_long(&self) -> bool {
        self.lines.len() >= Cart::LONG_ORDER
    }

    /// The sentence, or nothing.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "centimetres of paper, rounded down on purpose: it is a nudge                   and not a measurement"
    )]
    pub fn length_says(&self) -> Option<String> {
        self.is_long().then(|| {
            format!(
                "{} lines on this bill. The kitchen ticket will be about {} cm \
                 of paper — worth splitting the table if it is two parties.",
                self.lines.len(),
                // Roughly 8 lines to 10 cm on 80 mm paper at the standard row height.
                self.lines.len().saturating_mul(10) / 8
            )
        })
    }

    /// A bad index is an error the caller must handle, never a silent no-op and never a panic.
    fn check(&self, index: usize) -> Result<()> {
        if index >= self.lines.len() {
            return Err(CartError::NoSuchLine {
                index,
                len: self.lines.len(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemId;
    use crate::money::Money;
    use crate::tax::TaxRate;

    fn item(id: &str, name: &str, paise: i64) -> ItemSnapshot {
        ItemSnapshot::new(
            ItemId::new(id),
            name,
            Money::from_paise(paise),
            TaxRate::from_percent(5).expect("5%"),
        )
    }

    fn modifier(id: &str) -> Modifier {
        Modifier::new(ModifierId::new(id), id, Money::from_paise(1_000))
    }

    fn cart_with_one(note: Option<&str>, modifiers: Vec<Modifier>) -> Cart {
        let mut cart = Cart::new();
        cart.add(
            item("itm_1", "Paneer Tikka", 22_000),
            Qty::ONE,
            note.map(str::to_owned),
            modifiers,
        )
        .expect("adds");
        cart
    }

    /// A very long order is mentioned, never refused.
    #[test]
    fn a_very_long_order_is_mentioned_and_never_refused() {
        let mut cart = Cart::new();
        for n in 0..Cart::LONG_ORDER {
            cart.add(
                item(&format!("itm_{n}"), &format!("Dish {n}"), 10_000),
                Qty::from_whole(1).expect("in range"),
                None,
                vec![],
            )
            .expect("a long order is still an order");
        }
        assert!(cart.is_long());
        let says = cart.length_says().expect("it says something");
        assert!(says.contains("40 lines"), "{says}");
        assert!(says.contains("cm of paper"), "{says}");

        // One under the line says nothing at all — a warning on every ordinary bill is a
        // warning nobody reads.
        let mut ordinary = Cart::new();
        for n in 0..(Cart::LONG_ORDER - 1) {
            ordinary
                .add(
                    item(&format!("itm_{n}"), &format!("Dish {n}"), 10_000),
                    Qty::from_whole(1).expect("in range"),
                    None,
                    vec![],
                )
                .expect("adds");
        }
        assert!(!ordinary.is_long());
        assert!(ordinary.length_says().is_none());
    }

    #[test]
    fn the_same_dish_with_the_same_note_merges() {
        let mut cart = cart_with_one(Some("extra spicy"), vec![]);
        let index = cart
            .add(
                item("itm_1", "Paneer Tikka", 22_000),
                Qty::from_whole(2).expect("in range"),
                Some("extra spicy".to_owned()),
                vec![],
            )
            .expect("adds");

        assert_eq!(index, 0);
        assert_eq!(cart.len(), 1);
        assert_eq!(cart.lines()[0].qty, Qty::from_whole(3).expect("in range"));
    }

    #[test]
    fn the_same_dish_with_a_different_note_does_not_merge() {
        // The kitchen must see "extra spicy" as its own line.
        let mut cart = cart_with_one(Some("extra spicy"), vec![]);
        cart.add(
            item("itm_1", "Paneer Tikka", 22_000),
            Qty::ONE,
            None,
            vec![],
        )
        .expect("adds");
        assert_eq!(cart.len(), 2);
    }

    #[test]
    fn different_modifiers_do_not_merge_but_a_different_order_does() {
        let a = vec![modifier("mod_cheese"), modifier("mod_olives")];
        let b = vec![modifier("mod_olives"), modifier("mod_cheese")];
        let c = vec![modifier("mod_cheese")];

        let mut cart = cart_with_one(None, a);
        // Same set, tapped in the other order — one line, quantity 2.
        cart.add(item("itm_1", "Paneer Tikka", 22_000), Qty::ONE, None, b)
            .expect("adds");
        assert_eq!(cart.len(), 1);
        assert_eq!(cart.lines()[0].qty, Qty::from_whole(2).expect("in range"));

        // A different set — its own line.
        cart.add(item("itm_1", "Paneer Tikka", 22_000), Qty::ONE, None, c)
            .expect("adds");
        assert_eq!(cart.len(), 2);
    }

    #[test]
    fn notes_are_trimmed_but_not_case_folded() {
        let mut cart = cart_with_one(Some("no onion"), vec![]);

        // Trimmed: the same instruction typed with a stray space.
        cart.add(
            item("itm_1", "Paneer Tikka", 22_000),
            Qty::ONE,
            Some("  no onion  ".to_owned()),
            vec![],
        )
        .expect("adds");
        assert_eq!(cart.len(), 1, "a stray space must not split the line");

        // Not case-folded: the note is printed verbatim.
        cart.add(
            item("itm_1", "Paneer Tikka", 22_000),
            Qty::ONE,
            Some("NO ONION".to_owned()),
            vec![],
        )
        .expect("adds");
        assert_eq!(cart.len(), 2, "case is part of what the cook reads");
    }

    #[test]
    fn an_empty_note_is_the_same_as_no_note() {
        let mut cart = cart_with_one(None, vec![]);
        cart.add(
            item("itm_1", "Paneer Tikka", 22_000),
            Qty::ONE,
            Some("   ".to_owned()),
            vec![],
        )
        .expect("adds");
        assert_eq!(cart.len(), 1);
        assert_eq!(cart.lines()[0].note, None);
    }

    #[test]
    fn insertion_order_survives_add_merge_and_remove() {
        let mut cart = Cart::new();
        cart.add(item("itm_a", "A", 100), Qty::ONE, None, vec![])
            .expect("adds");
        cart.add(item("itm_b", "B", 200), Qty::ONE, None, vec![])
            .expect("adds");
        cart.add(item("itm_c", "C", 300), Qty::ONE, None, vec![])
            .expect("adds");

        // Re-adding A must increase A in place, not move it to the end.
        cart.add(item("itm_a", "A", 100), Qty::ONE, None, vec![])
            .expect("adds");
        cart.remove(1).expect("removes B");

        let names: Vec<&str> = cart
            .lines()
            .iter()
            .map(|l| l.snapshot.name.as_str())
            .collect();
        assert_eq!(names, ["A", "C"]);
        assert_eq!(cart.lines()[0].qty, Qty::from_whole(2).expect("in range"));
    }

    #[test]
    fn a_quantity_of_zero_removes_the_line() {
        let mut cart = cart_with_one(None, vec![]);
        cart.set_qty(0, Qty::ZERO).expect("sets");
        assert!(
            cart.is_empty(),
            "a zero-quantity ghost must not reach the kitchen"
        );
    }

    #[test]
    fn changing_a_note_into_a_twin_merges_the_two_lines() {
        let mut cart = Cart::new();
        cart.add(item("itm_1", "Dosa", 8_000), Qty::ONE, None, vec![])
            .expect("adds");
        cart.add(
            item("itm_1", "Dosa", 8_000),
            Qty::from_whole(2).expect("in range"),
            Some("crispy".to_owned()),
            vec![],
        )
        .expect("adds");
        assert_eq!(cart.len(), 2);

        // Clearing the note on line 1 makes it identical to line 0.
        let survivor = cart.set_note(1, None).expect("sets");
        assert_eq!(cart.len(), 1, "the bill must not show the same dish twice");
        assert_eq!(survivor, 0);
        assert_eq!(cart.lines()[0].qty, Qty::from_whole(3).expect("in range"));
    }

    #[test]
    fn a_bad_index_is_an_error_not_a_panic() {
        let mut cart = cart_with_one(None, vec![]);
        assert_eq!(
            cart.set_qty(5, Qty::ONE),
            Err(CartError::NoSuchLine { index: 5, len: 1 })
        );
        assert!(matches!(cart.remove(9), Err(CartError::NoSuchLine { .. })));
        assert!(matches!(
            cart.set_note(9, None),
            Err(CartError::NoSuchLine { .. })
        ));
        assert!(matches!(
            cart.set_line_discount(9, None),
            Err(CartError::NoSuchLine { .. })
        ));
    }

    #[test]
    fn a_line_cannot_start_with_no_quantity() {
        let mut cart = Cart::new();
        assert_eq!(
            cart.add(item("itm_1", "Dosa", 8_000), Qty::ZERO, None, vec![]),
            Err(CartError::NonPositiveQty)
        );
        assert_eq!(
            cart.add(
                item("itm_1", "Dosa", 8_000),
                Qty::from_thousandths(-500),
                None,
                vec![]
            ),
            Err(CartError::NonPositiveQty)
        );
        assert!(cart.is_empty());
    }

    #[test]
    fn clearing_empties_the_cart() {
        let mut cart = cart_with_one(None, vec![]);
        assert!(!cart.is_empty());
        cart.clear();
        assert!(cart.is_empty());
        assert_eq!(cart.len(), 0);
    }
}
