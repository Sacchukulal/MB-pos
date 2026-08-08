//! **Moving an order, merging two of them, and splitting one in half.**
//!
//! Scope 1.21, 1.22, 1.23 — the three things a restaurant does to an order
//! that is already on a table, and the three things v1 could not do at all.
//!
//! # Why this is a module and not three functions on the app layer
//!
//! Because every one of them moves lines between carts, and **the kitchen
//! ledger has to move with them or the kitchen is lied to.** That ledger is
//! crown jewel 2:
//!
//! > *"The delta KOT. Only what the kitchen has not seen gets printed, and
//! > what was printed is remembered in the database, not in the screen's
//! > memory."*
//!
//! Get the arithmetic wrong and the failure is not a wrong number on a screen.
//! It is three more dosas coming out of the kitchen, or a dosa nobody ever
//! cooks. So it lives here, next to the ledger, with tests carrying the exact
//! numbers.
//!
//! # What each one does to the ledger
//!
//! * **MOVE** — nothing. Only `table_id` changes; the order, its cart, its
//!   ledger, its bill number and its covers are the same order in a different
//!   chair. There is deliberately no function here for it: a move that creates
//!   a new order is a bug waiting to happen, and the way to make that mistake
//!   impossible is to not offer a tool for it.
//! * **MERGE** — the surviving ledger becomes the SUM of the two, per line
//!   identity. Table 4 was told two dosas and table 5 was told one; the merged
//!   order was told three, and `pending()` must come back empty.
//! * **SPLIT** — the hard one. See [`take_lines`].

use serde::{Deserialize, Serialize};

use crate::cart::{Cart, CartError, CartLine, LineIdentity};
use crate::money::{Money, MoneyError};
use crate::order::KitchenLedger;
use crate::qty::{Qty, QtyError};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransferError {
    #[error("{0}")]
    Cart(#[from] CartError),
    #[error("{0}")]
    Qty(#[from] QtyError),
    #[error("{0}")]
    Money(#[from] MoneyError),
    #[error("there is no line {0} on this order")]
    NoSuchLine(usize),
    #[error("that is more than the line has")]
    MoreThanTheLineHas,
    #[error("nothing was chosen to move")]
    NothingChosen,
    #[error("a bill cannot be split fewer than two ways")]
    TooFewWays,
    #[error("that would leave the order empty — move the whole order instead")]
    WouldEmptyTheOrder,
}

type Result<T> = std::result::Result<T, TransferError>;

/// One line, and how much of it to move. Index into the ORIGIN's cart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pick {
    pub index: usize,
    pub qty: Qty,
}

/// An order's two moving parts, together, because they must never be moved
/// apart.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Portion {
    pub cart: Cart,
    pub kitchen: KitchenLedger,
}

/// **Take lines off one order and hand them back as another** — the line half
/// of a split (scope 1.21).
///
/// # The told-quantity arithmetic, which is the whole problem
///
/// A line of 3 with 2 already told to the kitchen has 1 outstanding. Split it
/// 2 away / 1 remaining and there are two defensible answers, both of which
/// conserve the total. This one is deliberate:
///
/// > **the told quantity stays with the origin as far as it will go, and what
/// > moves is the outstanding part first.**
///
/// So the origin keeps 1 of 1 (nothing outstanding) and the new order gets
/// 1 told of 2 (one dosa still to tell). The reason is the paper: a ticket for
/// the origin's food is already hanging in the kitchen under the origin's
/// name. If the told quantity followed the moved lines instead, the origin
/// would be left with an outstanding dosa it has already been served, and the
/// next ticket would ask the kitchen to cook it again. A duplicate on the pass
/// is worse than an extra line on the new party's first ticket.
///
/// The invariant either way, and it is asserted in the tests:
/// **told(origin) + told(new) is unchanged, and so is qty(origin) + qty(new).**
pub fn take_lines(from: &mut Portion, picks: &[Pick]) -> Result<Portion> {
    if picks.is_empty() {
        return Err(TransferError::NothingChosen);
    }

    // Highest index first, so removing a line cannot shift the ones still to
    // be taken. The alternative — recomputing indexes as we go — is the kind
    // of arithmetic that works until somebody picks the lines in a different
    // order.
    let mut ordered: Vec<Pick> = picks.to_vec();
    ordered.sort_unstable_by(|a, b| b.index.cmp(&a.index));
    if ordered.windows(2).any(|pair| pair[0].index == pair[1].index) {
        // Two picks for one line is ambiguous — is it the sum, or the second?
        // Refuse rather than guess.
        return Err(TransferError::MoreThanTheLineHas);
    }

    let mut moved = Portion::default();
    for pick in ordered {
        let line = from
            .cart
            .lines()
            .get(pick.index)
            .ok_or(TransferError::NoSuchLine(pick.index))?
            .clone();

        if !pick.qty.is_positive() {
            return Err(TransferError::MoreThanTheLineHas);
        }
        let remaining = line.qty.sub(pick.qty)?;
        if remaining.is_negative() {
            return Err(TransferError::MoreThanTheLineHas);
        }

        let identity = line.identity();
        let told = from.kitchen.quantity_told(&identity);
        // The rule, in one line: the origin keeps what it can, the rest moves.
        let stays_told = if told < remaining { told } else { remaining };
        let moves_told = told.sub(stays_told)?;

        // The cart first — a failure here must not leave the ledger changed.
        if remaining.is_zero() {
            from.cart.remove(pick.index)?;
        } else {
            from.cart.set_qty(pick.index, remaining)?;
        }
        let mut going = line;
        going.qty = pick.qty;
        moved.cart.push(going)?;

        from.kitchen.set_told(&identity, stays_told);
        if moves_told.is_positive() {
            moved.kitchen.mark_printed(&[(identity, moves_told)])?;
        }
    }

    if from.cart.is_empty() {
        return Err(TransferError::WouldEmptyTheOrder);
    }
    Ok(moved)
}

/// **Fold one order into another** — scope 1.22.
///
/// Lines of the same identity combine into one, exactly as they would if the
/// waiter had called them out on one order in the first place, and the ledgers
/// add per identity so nothing is re-told.
pub fn merge_into(survivor: &mut Portion, absorbed: Portion) -> Result<()> {
    for line in absorbed.cart.lines() {
        survivor.cart.push(line.clone())?;
    }
    survivor.kitchen.merge_from(&absorbed.kitchen)?;
    Ok(())
}

/// **How a bill-level discount follows a split** (scope 1.21).
///
/// This is the part that decides whether two halves still add up to the whole,
/// and it has two cases that look alike and are not:
///
/// * a **percentage** needs no dividing. Ten per cent off each half is ten per
///   cent off the whole, exactly, at any rounding — so the same
///   [`DiscountEntry`](crate::discount::DiscountEntry) goes to both sides and
///   nothing here is involved.
/// * a **fixed amount** must be divided, and dividing it in half is wrong. A
///   ₹100 off a bill where one guest ate ₹900 and the other ₹100 is not ₹50
///   each. It is spread in proportion to what each side is worth, by **D14's
///   rule** — floor, then largest remainder — which is the same rule the bill
///   discount already uses to reach the lines it discounts, and the same rule
///   `combo::apportion` uses. One rounding rule in the product, not three.
///
/// `nets` is what each side is worth before the bill discount. The returned
/// amounts sum to `amount` exactly.
pub fn split_bill_discount(amount: Money, nets: &[Money]) -> Result<Vec<Money>> {
    Ok(crate::discount::spread(amount, nets)?)
}

/// **An even split, n ways** — scope 1.21's other half.
///
/// > *"Any remainder paisa is assigned, never dropped."*
///
/// A 100.01 bill split three ways is 33.34 / 33.34 / 33.33, and the extra
/// paise go to the EARLIER shares. Which end they go to matters less than that
/// it is decided, deterministic and written down — the same reasoning D14
/// applies to a discount spread, and it is stated here so no future session
/// invents a second answer.
pub fn even_shares(total: Money, ways: u32) -> Result<Vec<Money>> {
    if ways < 2 {
        return Err(TransferError::TooFewWays);
    }
    let ways = i64::from(ways);
    let paise = total.paise();
    let base = paise.div_euclid(ways);
    let over = paise.rem_euclid(ways);

    Ok((0..ways)
        .map(|n| Money::from_paise(if n < over { base + 1 } else { base }))
        .collect())
}

impl Portion {
    /// What the kitchen has still to be told about this portion — the same
    /// question `KitchenLedger::pending` answers, asked of the pair so a caller
    /// cannot accidentally ask it of the wrong cart.
    pub fn pending(&self) -> std::result::Result<Vec<(LineIdentity, Qty)>, QtyError> {
        self.kitchen.pending(&self.cart)
    }

    /// The quantity on every line, added up. Used by the tests to assert the
    /// conservation law, and by the app layer to say "3 items move".
    pub fn total_qty(&self) -> std::result::Result<Qty, QtyError> {
        self.cart
            .lines()
            .iter()
            .try_fold(Qty::ZERO, |running, line| running.add(line.qty))
    }
}

impl From<(Cart, KitchenLedger)> for Portion {
    fn from((cart, kitchen): (Cart, KitchenLedger)) -> Self {
        Portion { cart, kitchen }
    }
}

/// So a caller can put the pieces back where they came from without naming the
/// fields in four places.
impl Portion {
    #[must_use]
    pub fn into_parts(self) -> (Cart, KitchenLedger) {
        (self.cart, self.kitchen)
    }
}

/// A line with its quantity, for a caller that wants to move a whole line and
/// does not want to look its quantity up first.
#[must_use]
pub fn whole_line(index: usize, line: &CartLine) -> Pick {
    Pick { index, qty: line.qty }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemId;
    use crate::item::ItemSnapshot;
    use crate::tax::TaxRate;

    fn snapshot(id: &str, paise: i64) -> ItemSnapshot {
        ItemSnapshot::new(
            ItemId::new(id),
            id,
            Money::from_paise(paise),
            TaxRate::from_basis_points(500).expect("5%"),
        )
    }

    fn portion(items: &[(&str, i64, i64)]) -> Portion {
        let mut cart = Cart::new();
        for (id, paise, qty) in items {
            cart.add(snapshot(id, *paise), Qty::from_whole(*qty).expect("qty"), None, Vec::new())
                .expect("added");
        }
        Portion { cart, kitchen: KitchenLedger::new() }
    }

    fn tell_kitchen(portion: &mut Portion, index: usize, qty: i64) {
        let identity = portion.cart.lines()[index].identity();
        portion
            .kitchen
            .mark_printed(&[(identity, Qty::from_whole(qty).expect("qty"))])
            .expect("told");
    }

    /// **The exact case from the prompt**: a line of 3 with 2 told, split 2/1.
    #[test]
    fn splitting_a_partly_told_line_leaves_no_duplicate_and_loses_nothing() {
        let mut origin = portion(&[("dosa", 12_000, 3), ("tea", 2_000, 1)]);
        tell_kitchen(&mut origin, 0, 2);

        let moved = take_lines(
            &mut origin,
            &[Pick { index: 0, qty: Qty::from_whole(2).expect("qty") }],
        )
        .expect("split");

        // The origin keeps one dosa, and the kitchen already knows about it —
        // so nothing on the origin is waiting to be cooked.
        assert_eq!(origin.cart.lines()[0].qty, Qty::from_whole(1).expect("qty"));
        let dosa = origin.cart.lines()[0].identity();
        assert!(
            !origin.pending().expect("pending").iter().any(|(id, _)| id == &dosa),
            "the origin must not ask the kitchen for a dosa it has already been served",
        );

        // The new party gets two dosas, one of which the kitchen has not heard
        // about yet.
        assert_eq!(moved.cart.lines()[0].qty, Qty::from_whole(2).expect("qty"));
        let pending = moved.pending().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1, Qty::from_whole(1).expect("qty"), "exactly one still to tell");

        // The conservation law, stated as an assertion.
        let total: i64 = origin.total_qty().expect("qty").thousandths()
            + moved.total_qty().expect("qty").thousandths();
        assert_eq!(total, Qty::from_whole(4).expect("qty").thousandths());
    }

    #[test]
    fn taking_a_whole_line_removes_it_from_the_origin() {
        let mut origin = portion(&[("dosa", 12_000, 2), ("tea", 2_000, 1)]);
        let moved = take_lines(
            &mut origin,
            &[Pick { index: 1, qty: Qty::from_whole(1).expect("qty") }],
        )
        .expect("split");

        assert_eq!(origin.cart.len(), 1);
        assert_eq!(moved.cart.len(), 1);
        assert_eq!(moved.cart.lines()[0].snapshot.name, "tea");
    }

    /// Taking everything is a MOVE, and a move must not be done by splitting —
    /// it would mint a new order and abandon a bill number.
    #[test]
    fn taking_everything_is_refused_and_says_what_to_do_instead() {
        let mut origin = portion(&[("dosa", 12_000, 1)]);
        let error = take_lines(
            &mut origin,
            &[Pick { index: 0, qty: Qty::from_whole(1).expect("qty") }],
        )
        .expect_err("refused");
        assert_eq!(error, TransferError::WouldEmptyTheOrder);
    }

    #[test]
    fn more_than_the_line_has_is_refused() {
        let mut origin = portion(&[("dosa", 12_000, 2), ("tea", 2_000, 1)]);
        assert!(
            take_lines(
                &mut origin,
                &[Pick { index: 0, qty: Qty::from_whole(5).expect("qty") }]
            )
            .is_err()
        );
        assert!(take_lines(&mut origin, &[]).is_err());
        assert!(take_lines(&mut origin, &[Pick { index: 9, qty: Qty::ONE }]).is_err());
    }

    /// **The three-dosa case.** Table 4 was told two, table 5 was told one; if
    /// the merged ledger does not add up, the kitchen cooks three more.
    #[test]
    fn merging_adds_the_ledgers_so_nothing_is_cooked_twice() {
        let mut four = portion(&[("dosa", 12_000, 2)]);
        tell_kitchen(&mut four, 0, 2);
        let mut five = portion(&[("dosa", 12_000, 1)]);
        tell_kitchen(&mut five, 0, 1);

        merge_into(&mut four, five).expect("merged");

        assert_eq!(four.cart.len(), 1, "one dish, one line");
        assert_eq!(four.cart.lines()[0].qty, Qty::from_whole(3).expect("qty"));
        assert!(
            four.pending().expect("pending").is_empty(),
            "the kitchen was told about all three already",
        );
    }

    #[test]
    fn merging_keeps_an_untold_item_untold() {
        let mut four = portion(&[("dosa", 12_000, 2)]);
        tell_kitchen(&mut four, 0, 2);
        let five = portion(&[("tea", 2_000, 1)]);

        merge_into(&mut four, five).expect("merged");
        let pending = four.pending().expect("pending");
        assert_eq!(pending.len(), 1, "the tea is still to be made");
    }

    #[test]
    fn an_even_split_assigns_the_remainder_and_never_drops_it() {
        let shares = even_shares(Money::from_paise(10_001), 3).expect("shares");
        assert_eq!(
            shares,
            [Money::from_paise(3_334), Money::from_paise(3_334), Money::from_paise(3_333)],
        );
        assert_eq!(
            Money::try_sum(shares.iter().copied()).expect("sum"),
            Money::from_paise(10_001),
        );

        // Every bill from 1 paisa to 20 rupees, every way from 2 to 8, adds
        // back exactly. The same shape of proof combo::apportion carries.
        for paise in 1..=2_000_i64 {
            for ways in 2..=8_u32 {
                let shares = even_shares(Money::from_paise(paise), ways).expect("shares");
                assert_eq!(
                    Money::try_sum(shares.iter().copied()).expect("sum").paise(),
                    paise,
                    "{paise} split {ways} ways",
                );
            }
        }
    }

    /// **The reconciliation P14 exists to prove.**
    ///
    /// One bill, two tax rates, a fixed-amount bill discount, split by line.
    /// The two halves must add back to the original **rate by rate** — not
    /// approximately, and not "within a paisa".
    #[test]
    fn a_split_bill_adds_back_to_the_original_rate_by_rate() {
        use crate::bill::{BillInput, compute_bill};
        use crate::discount::{Discount, DiscountEntry};
        use crate::tax::TaxTreatment;

        // Two dosas at 5% and a bottle of water at 18% — the mixed-rate case,
        // because a single-rate split would prove nothing about the summary.
        let mut cart = Cart::new();
        cart.add(snapshot("dosa", 12_000), Qty::from_whole(2).expect("qty"), None, Vec::new())
            .expect("added");
        let mut water = snapshot("water", 4_000);
        water.tax_rate = TaxRate::from_basis_points(1_800).expect("18%");
        water.tax_treatment = TaxTreatment::Exclusive;
        cart.add(water, Qty::from_whole(3).expect("qty"), None, Vec::new()).expect("added");

        // ₹100 off the bill. A fixed amount, which is the case that has to be
        // divided; a percentage would divide itself.
        let off = Money::from_paise(10_000);
        let whole = compute_bill(BillInput {
            bill_discount: Some(DiscountEntry::new(
                Discount::amount(off).expect("a discount"),
            )),
            ..BillInput::new(&cart)
        })
        .expect("the original bill");

        // Split: the water goes to the second guest, the dosas stay.
        let mut origin = Portion { cart, kitchen: KitchenLedger::new() };
        let moved = take_lines(
            &mut origin,
            &[Pick { index: 1, qty: Qty::from_whole(3).expect("qty") }],
        )
        .expect("split");

        // What each side is worth before the discount — the basis D14 spreads
        // by. Computing each half with NO discount is how you find it without
        // duplicating the pipeline.
        let bare = |portion: &Portion| {
            compute_bill(BillInput::new(&portion.cart)).expect("a bare bill")
        };
        let nets = [bare(&origin).subtotal, bare(&moved).subtotal];
        let shares = split_bill_discount(off, &nets).expect("shares");
        assert_eq!(
            Money::try_sum(shares.iter().copied()).expect("sum"),
            off,
            "the discount itself must not leak a paisa",
        );

        let half = |portion: &Portion, share: Money| {
            compute_bill(BillInput {
                bill_discount: Some(DiscountEntry::new(
                    Discount::amount(share).expect("a discount"),
                )),
                ..BillInput::new(&portion.cart)
            })
            .expect("a half bill")
        };
        let first = half(&origin, shares[0]);
        let second = half(&moved, shares[1]);

        // Rate by rate. This is the assertion that matters: a chartered
        // accountant adds the two halves' 5% rows and expects the original's.
        for row in whole.summary.rows() {
            let of = |bill: &crate::bill::Bill| {
                bill.summary
                    .rows()
                    .find(|r| r.rate == row.rate)
                    .map_or((Money::ZERO, Money::ZERO), |r| {
                        (r.taxable, r.tax.total().expect("tax"))
                    })
            };
            let (taxable_a, tax_a) = of(&first);
            let (taxable_b, tax_b) = of(&second);
            assert_eq!(
                taxable_a.add(taxable_b).expect("sum"),
                row.taxable,
                "taxable value at {}",
                row.rate.label(),
            );
            assert_eq!(
                tax_a.add(tax_b).expect("sum"),
                row.tax.total().expect("tax"),
                "tax at {}",
                row.rate.label(),
            );
        }

        // And the money a customer actually hands over. Round-off is computed
        // per bill, so two halves each rounded to the rupee can differ from one
        // bill rounded once — the TAXABLE and TAX reconcile exactly, which is
        // what a return is built from, and the round-off difference is real,
        // visible and at most a rupee per extra bill.
        let paid = first.grand_total.add(second.grand_total).expect("sum");
        let drift = paid.sub(whole.grand_total).expect("difference");
        assert!(
            drift.abs() <= Money::from_paise(100),
            "splitting must not move real money: {} vs {}",
            paid.to_plain_string(),
            whole.grand_total.to_plain_string(),
        );
    }

    #[test]
    fn a_bill_cannot_be_split_fewer_than_two_ways() {
        assert_eq!(even_shares(Money::from_paise(100), 1), Err(TransferError::TooFewWays));
    }
}
