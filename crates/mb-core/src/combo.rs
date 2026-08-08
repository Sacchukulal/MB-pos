//! **Combos — one line on the bill, several dishes in the kitchen, and the tax
//! right to the paisa.**
//!
//! Scope 6.3. A combo bills as one line at one price, and explodes into its
//! components on the kitchen ticket, because the kitchen cooks items and has
//! never heard of a combo.
//!
//! # The tax is where combos go wrong
//!
//! A meal deal containing a 5% dish and an 18% drink cannot be taxed at one
//! rate — the GST return needs both rows, correctly. So the combo price is
//! apportioned across its components **in proportion to their standalone
//! price**, and each component is then taxed at its own item's rate.
//!
//! # And that does not divide evenly
//!
//! ₹199 across three components is not three whole numbers of paise, and a lost
//! paisa in a combo is a lost paisa in a GST return.
//!
//! **This does not invent a rounding rule.** D14 already settled one for
//! spreading a bill discount across lines — floor each share, then hand the
//! remainders out largest-first — and the property needed here is identical:
//! *the shares add back to the whole, exactly.* So [`apportion`] is
//! [`discount::spread`](crate::discount::spread) with a combo's vocabulary
//! around it. A second rounding rule in the same product is a second answer to
//! the same question, and one of them would eventually be wrong.

use serde::{Deserialize, Serialize};

use crate::discount;
use crate::ids::ItemId;
use crate::money::{Money, MoneyError};
use crate::qty::{Qty, QtyError};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ComboError {
    #[error("a combo needs at least one thing in it")]
    Empty,
    #[error("every part of a combo needs a price of its own to share by")]
    NoStandalonePrice,
    #[error("that combo's amounts are out of range")]
    Money(#[from] MoneyError),
    #[error("a quantity in that combo is out of range")]
    Qty(#[from] QtyError),
}

/// One thing inside a combo, and what it would cost on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComboComponent {
    pub item_id: ItemId,
    pub qty: Qty,
    /// What this component sells for by itself. **The basis of the share**, and
    /// the reason a combo cannot contain something the shop does not otherwise
    /// sell without a price being set for it.
    pub standalone: Money,
}

/// A component with the slice of the combo price it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Apportioned {
    pub item_id: ItemId,
    pub qty: Qty,
    pub share: Money,
}

/// Split a combo's price across what is in it.
///
/// The shares always sum to `combo_price` exactly, and each share is
/// proportional to that component's standalone value (price × quantity).
pub fn apportion(
    combo_price: Money,
    components: &[ComboComponent],
) -> Result<Vec<Apportioned>, ComboError> {
    if components.is_empty() {
        return Err(ComboError::Empty);
    }

    // The basis: what each component is worth on its own, quantity included —
    // two ₹50 sides carry twice the share of one.
    let mut values = Vec::with_capacity(components.len());
    for component in components {
        values.push(component.qty.extend(component.standalone)?);
    }

    // A combo whose parts are all free has nothing to share BY. Refusing is the
    // honest answer: any split would be arbitrary, and an arbitrary split shows
    // up as the wrong rate on a return.
    if Money::try_sum(values.iter().copied())?.is_zero() {
        return Err(ComboError::NoStandalonePrice);
    }

    let shares = discount::spread(combo_price, &values)?;

    Ok(components
        .iter()
        .zip(shares)
        .map(|(component, share)| Apportioned {
            item_id: component.item_id.clone(),
            qty: component.qty,
            share,
        })
        .collect())
}

/// The stored form: each component's share as basis points of the combo price.
///
/// `combo_components.share_bp` is where P04 put this. It is a **cache of the
/// arithmetic above**, kept so a saved combo does not have to re-read every
/// component's live price to be re-priced — and it is deliberately not the
/// source of truth, because a component's standalone price changes and the
/// stored share would then be stale in a way nobody would notice.
#[must_use]
#[allow(
    clippy::integer_division,
    reason = "a display proportion, not money — the money is `share`, which is \
              exact, and this is the figure a report shows beside it"
)]
pub fn share_basis_points(combo_price: Money, share: Money) -> u32 {
    if combo_price.is_zero() {
        return 0;
    }
    // The proportion in basis points, floored — a display and reporting figure,
    // never a money one. The money is `share`, which is exact.
    let bp = i128::from(share.paise()) * 10_000 / i128::from(combo_price.paise());
    u32::try_from(bp.clamp(0, 10_000)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rs(rupees: i64) -> Money {
        Money::from_paise(rupees * 100)
    }

    fn part(id: &str, standalone: Money, qty: i64) -> ComboComponent {
        ComboComponent {
            item_id: ItemId::new(id),
            qty: Qty::from_whole(qty).expect("in range"),
            standalone,
        }
    }

    #[test]
    fn an_even_split_is_even() {
        let shares = apportion(rs(100), &[part("a", rs(60), 1), part("b", rs(40), 1)])
            .expect("apportions");
        assert_eq!(shares[0].share, rs(60));
        assert_eq!(shares[1].share, rs(40));
    }

    /// **T4 — and the numbers are deliberately awkward.**
    ///
    /// ₹199 across three components does not divide, and the whole point of
    /// this function is what happens to the remainder.
    #[test]
    fn an_awkward_price_still_adds_back_exactly() {
        let combo = Money::from_paise(19_900); // ₹199
        let parts = [
            part("dosa", rs(120), 1),
            part("water", rs(20), 1),
            part("coffee", rs(70), 1),
        ];
        let shares = apportion(combo, &parts).expect("apportions");

        let total = Money::try_sum(shares.iter().map(|s| s.share)).expect("sums");
        assert_eq!(total, combo, "a paisa went missing in a combo");

        // And each share is in proportion — the biggest component takes the
        // biggest slice.
        assert!(shares[0].share > shares[2].share);
        assert!(shares[2].share > shares[1].share);
    }

    #[test]
    fn every_price_from_one_paisa_up_adds_back_exactly() {
        // Exhaustive over the awkward range rather than a chosen example: if
        // the remainder handling is wrong anywhere, it is wrong here.
        let parts = [
            part("a", rs(33), 1),
            part("b", rs(67), 1),
            part("c", rs(11), 2),
        ];
        for paise in 1..=2_000 {
            let combo = Money::from_paise(paise);
            let shares = apportion(combo, &parts).expect("apportions");
            let total = Money::try_sum(shares.iter().map(|s| s.share)).expect("sums");
            assert_eq!(total, combo, "at {paise} paise the shares did not add back");
        }
    }

    #[test]
    fn quantity_counts_towards_the_share() {
        // Two ₹50 sides carry twice the share of one.
        let shares = apportion(rs(150), &[part("main", rs(50), 1), part("side", rs(50), 2)])
            .expect("apportions");
        assert_eq!(shares[0].share, rs(50));
        assert_eq!(shares[1].share, rs(100));
    }

    #[test]
    fn a_combo_of_free_things_is_refused_rather_than_split_arbitrarily() {
        let refused = apportion(rs(100), &[part("a", Money::ZERO, 1)]);
        assert_eq!(refused, Err(ComboError::NoStandalonePrice));
        assert!(refused.expect_err("refused").to_string().contains("price of its own"));
    }

    #[test]
    fn an_empty_combo_is_refused() {
        assert_eq!(apportion(rs(100), &[]), Err(ComboError::Empty));
    }

    #[test]
    fn the_same_input_always_gives_the_same_answer() {
        // The remainder goes somewhere; it must go to the SAME place twice, or
        // two terminals apportion one combo differently.
        let parts = [part("a", rs(1), 1), part("b", rs(2), 1), part("c", rs(3), 1)];
        let once = apportion(Money::from_paise(101), &parts).expect("apportions");
        let twice = apportion(Money::from_paise(101), &parts).expect("apportions");
        assert_eq!(once, twice);
    }

    #[test]
    fn the_stored_share_is_a_proportion_not_a_price() {
        let combo = rs(200);
        assert_eq!(share_basis_points(combo, rs(50)), 2_500);
        assert_eq!(share_basis_points(combo, rs(200)), 10_000);
        assert_eq!(share_basis_points(Money::ZERO, rs(10)), 0);
    }
}
