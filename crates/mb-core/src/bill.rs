//! The bill pipeline — decision D4, in the fixed order, in one place.
//!
//! This is the only module that knows the whole computation. Everything it uses
//! lives below it: `money` for arithmetic, `tax` for GST, `qty` for price ×
//! quantity, `discount` for the spread.
//!
//! **The order is fixed and the steps are numbered in the code.** Reordering
//! them silently changes what a customer pays and what a CA files, so a session
//! that wants to reorder has to delete a comment that says what it is doing.

use crate::cart::Cart;
use crate::charge::{BillCharge, Charge};
use crate::discount::{self, DiscountEntry};
use crate::item::{ItemSnapshot, Modifier, OrderType};
use crate::money::{Money, MoneyError, RoundingMode};
use crate::qty::Qty;
use crate::tax::{self, PlaceOfSupply, TaxAmounts, TaxRate, TaxSummary, TaxTreatment};
use serde::{Deserialize, Serialize};

/// Something that stopped a bill from being computed.
///
/// Every message is a sentence a shop owner could read. That convention starts
/// here; P08 formalises it across the IPC boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BillError {
    #[error("an amount on this bill is too large to handle: {0}")]
    Money(#[from] MoneyError),
    /// Modifier deltas below the item's own price. Refused rather than clamped
    /// to zero: a line that costs less than nothing is a mistake in the menu,
    /// and hiding it would put a wrong total on a real receipt (D7).
    #[error("line {index} ({name}) works out to a negative price — check its modifiers")]
    NegativeLinePrice { index: usize, name: String },
}

type Result<T> = std::result::Result<T, BillError>;

/// Everything the pipeline needs to compute one bill.
#[derive(Debug, Clone)]
pub struct BillInput<'a> {
    pub cart: &'a Cart,
    pub bill_discount: Option<DiscountEntry>,
    /// Service, packing, delivery — each with its own tax rate (scope 1.14).
    /// They enter the pipeline at step 5, after the discount, because you
    /// discount the food and not the delivery.
    pub charges: &'a [Charge],
    pub place_of_supply: PlaceOfSupply,
    pub order_type: OrderType,
    /// Scope 1.13.
    pub rounding: RoundingMode,
}

/// So `BillInput::new` can hand out a `&[]` with the right lifetime.
const NO_CHARGES: &[Charge] = &[];

impl<'a> BillInput<'a> {
    /// A plain bill: no discount, no charges, same-state supply, dine-in,
    /// rounded to the nearest rupee.
    #[must_use]
    pub fn new(cart: &'a Cart) -> Self {
        BillInput {
            cart,
            bill_discount: None,
            charges: NO_CHARGES,
            place_of_supply: PlaceOfSupply::Intra,
            order_type: OrderType::DineIn,
            rounding: RoundingMode::NearestRupee,
        }
    }

    #[must_use]
    pub fn with_bill_discount(mut self, discount: DiscountEntry) -> Self {
        self.bill_discount = Some(discount);
        self
    }

    #[must_use]
    pub fn with_charges(mut self, charges: &'a [Charge]) -> Self {
        self.charges = charges;
        self
    }

    #[must_use]
    pub fn with_place_of_supply(mut self, place: PlaceOfSupply) -> Self {
        self.place_of_supply = place;
        self
    }

    #[must_use]
    pub fn with_order_type(mut self, order_type: OrderType) -> Self {
        self.order_type = order_type;
        self
    }

    #[must_use]
    pub fn with_rounding(mut self, rounding: RoundingMode) -> Self {
        self.rounding = rounding;
        self
    }
}

/// One computed line, carrying every intermediate figure the bill was built
/// from — so the receipt, the screen and the GST return all read the same
/// numbers rather than each recomputing them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillLine {
    pub snapshot: ItemSnapshot,
    pub qty: Qty,
    pub note: Option<String>,
    pub modifiers: Vec<Modifier>,
    /// Step 1: effective unit price × quantity.
    pub gross: Money,
    /// Step 2.
    pub line_discount: Money,
    /// Step 3: this line's share of the bill-level discount.
    pub bill_discount_share: Money,
    /// `gross` less both discounts. What tax is computed from.
    pub net: Money,
    /// Step 4.
    pub taxable: Money,
    pub tax: TaxAmounts,
    /// `taxable + tax`. For an inclusive-priced line this equals `net` exactly.
    pub gross_including_tax: Money,
    pub rate: TaxRate,
    pub treatment: TaxTreatment,
}

/// A computed bill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bill {
    pub lines: Vec<BillLine>,
    /// Step 5. Computed and taxed like lines, and in the same `summary`.
    pub charges: Vec<BillCharge>,
    pub summary: TaxSummary,
    /// Sum of line gross, before any discount.
    pub subtotal: Money,
    pub total_line_discount: Money,
    pub total_bill_discount: Money,
    pub total_discount: Money,
    /// Charges before their own tax (scope 1.14).
    pub total_charges: Money,
    /// The bill discount could not take everything it was asked for. Carried so
    /// the screen can tell the cashier and P11 can audit it — never silent
    /// (D7, D15).
    pub bill_discount_capped: bool,
    pub total_taxable: Money,
    pub total_tax: TaxAmounts,
    pub non_gst_value: Money,
    pub exempt_value: Money,
    /// Step 7-8. Recorded as its own figure so the printed lines always sum to
    /// the printed total.
    pub round_off: Money,
    pub grand_total: Money,
    pub order_type: OrderType,
    pub place_of_supply: PlaceOfSupply,
    pub rounding: RoundingMode,
}

impl Bill {
    /// Total tax as one figure.
    pub fn tax_total(&self) -> std::result::Result<Money, MoneyError> {
        self.total_tax.total()
    }
}

/// Compute a bill from a cart, following decision D4 exactly.
pub fn compute_bill(input: BillInput<'_>) -> Result<Bill> {
    let lines = input.cart.lines();
    let count = lines.len();

    // ---- Step 1: line gross = effective unit price × quantity ----------
    //
    // Modifier deltas fold into the unit price BEFORE the multiplication.
    // Extending the price and each modifier separately rounds once per
    // modifier, for no reason, and can disagree with the single-rounding
    // answer by a paisa.
    let mut gross = Vec::with_capacity(count);
    for (index, line) in lines.iter().enumerate() {
        let mut unit = line.snapshot.unit_price;
        for modifier in &line.modifiers {
            unit = unit.add(modifier.price_delta)?;
        }
        if unit.is_negative() {
            return Err(BillError::NegativeLinePrice {
                index,
                name: line.snapshot.name.clone(),
            });
        }
        gross.push(line.qty.extend(unit)?);
    }

    // ---- Step 2: line discount -> line net ------------------------------
    let mut line_discounts = Vec::with_capacity(count);
    let mut nets_after_line_discount = Vec::with_capacity(count);
    for (line, line_gross) in lines.iter().zip(&gross) {
        let taken = match &line.line_discount {
            Some(entry) => entry.discount.compute_on(*line_gross)?.applied,
            None => Money::ZERO,
        };
        line_discounts.push(taken);
        nets_after_line_discount.push(line_gross.sub(taken)?);
    }

    // ---- Step 3: bill discount spread across lines, in proportion to net --
    //
    // BEFORE tax, and this is the step people get wrong. On a mixed-rate bill
    // a discount taken off the grand total after tax leaves every rate's tax
    // overstated and the rate-wise summary unable to tie (audit B10/B11).
    let mut bill_discount_capped = false;
    let bill_discount_shares = match &input.bill_discount {
        Some(entry) => {
            let base = Money::try_sum(nets_after_line_discount.iter().copied())?;
            let outcome = entry.discount.compute_on(base)?;
            bill_discount_capped = outcome.was_capped;
            discount::spread(outcome.applied, &nets_after_line_discount)?
        }
        None => vec![Money::ZERO; count],
    };

    // ---- Step 4: per-line taxable + tax, from the discounted net ---------
    let mut summary = TaxSummary::new();
    let mut computed = Vec::with_capacity(count);
    for (index, line) in lines.iter().enumerate() {
        let share = bill_discount_shares[index];
        let net = nets_after_line_discount[index].sub(share)?;

        let outcome = tax::compute_line(
            net,
            line.snapshot.tax_rate,
            line.snapshot.tax_treatment,
            input.place_of_supply,
        )?;
        summary.add(outcome)?;

        computed.push(BillLine {
            snapshot: line.snapshot.clone(),
            qty: line.qty,
            note: line.note.clone(),
            modifiers: line.modifiers.clone(),
            gross: gross[index],
            line_discount: line_discounts[index],
            bill_discount_share: share,
            net,
            taxable: outcome.taxable,
            tax: outcome.tax,
            gross_including_tax: outcome.gross,
            rate: outcome.rate,
            treatment: outcome.treatment,
        });
    }

    // ---- Step 5: charges -------------------------------------------------
    //
    // Service, packing and delivery, each with its OWN rate (scope 1.14). They
    // are supplies, so they go through the same `tax::compute_line` and into
    // the same summary as any line — a charge is not a special case in the tax
    // engine and must not become one.
    //
    // THE BASE for a percentage charge is the sum of the line nets AFTER both
    // discounts: everything above this point, and nothing below it. So:
    //   * a percentage charge never compounds onto another charge — two 5%
    //     charges on a ₹1,000 base are ₹50 and ₹50, not ₹50 and ₹52.50;
    //   * it is taken on what the customer actually owes after a discount,
    //     rather than quietly taking part of the discount back;
    //   * it includes non-GST lines, because service applies to the whole
    //     table, alcohol included.
    // Charges themselves are never discounted — that is why D4 puts the
    // discount at step 3 and charges at step 5, in that order.
    let charge_base = Money::try_sum(computed.iter().map(|l| l.net))?;
    let mut bill_charges = Vec::with_capacity(input.charges.len());
    for charge in input.charges {
        let amount = charge.compute_on(charge_base)?;
        let outcome = tax::compute_line(
            amount,
            charge.tax_rate,
            charge.tax_treatment,
            input.place_of_supply,
        )?;
        summary.add(outcome)?;

        bill_charges.push(BillCharge {
            kind: charge.kind.clone(),
            name: charge.name.clone(),
            basis: charge.basis,
            amount,
            taxable: outcome.taxable,
            tax: outcome.tax,
            gross_including_tax: outcome.gross,
            rate: outcome.rate,
            treatment: outcome.treatment,
        });
    }

    // ---- Step 6: totals --------------------------------------------------
    let subtotal = Money::try_sum(gross.iter().copied())?;
    let total_line_discount = Money::try_sum(line_discounts.iter().copied())?;
    let total_bill_discount = Money::try_sum(bill_discount_shares.iter().copied())?;
    let total_discount = total_line_discount.add(total_bill_discount)?;
    let total_charges = Money::try_sum(bill_charges.iter().map(|c| c.amount))?;
    let total_taxable = summary.total_taxable()?;
    let total_tax = summary.total_tax()?;

    // The grand total is built from the lines and charges themselves, not from
    // subtotal − discount + charges + tax. Summing what will actually be
    // printed is what makes requirement 7 ("the printed lines always sum to the
    // printed total") true by construction rather than by coincidence.
    let before_round_off = Money::try_sum(
        computed
            .iter()
            .map(|l| l.gross_including_tax)
            .chain(bill_charges.iter().map(|c| c.gross_including_tax)),
    )?;

    // ---- Step 7: round-off, on the grand total ONLY ----------------------
    //
    // Audit B8: v1 printed ₹487.35 and Indian counters round to the rupee.
    let round_off = before_round_off.round_adjustment(input.rounding);

    // ---- Step 8: round-off recorded as its own figure --------------------
    //
    // Kept as a value on the bill rather than folded into the total, so the
    // receipt can print it as a line and the lines reconcile to the total
    // exactly.
    let grand_total = before_round_off.add(round_off)?;

    Ok(Bill {
        lines: computed,
        charges: bill_charges,
        non_gst_value: summary.non_gst_value,
        exempt_value: summary.exempt_value,
        summary,
        subtotal,
        total_line_discount,
        total_bill_discount,
        total_discount,
        total_charges,
        bill_discount_capped,
        total_taxable,
        total_tax,
        round_off,
        grand_total,
        order_type: input.order_type,
        place_of_supply: input.place_of_supply,
        rounding: input.rounding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charge::{ChargeBasis, ChargeKind};
    use crate::discount::Discount;
    use crate::ids::{ItemId, ModifierId, StaffId};

    const fn rs(rupees: i64) -> Money {
        Money::from_paise(rupees * 100)
    }

    fn item(id: &str, paise: i64, rate: TaxRate) -> ItemSnapshot {
        ItemSnapshot::new(ItemId::new(id), id, Money::from_paise(paise), rate)
    }

    fn one_line(snapshot: ItemSnapshot, qty: Qty) -> Cart {
        let mut cart = Cart::new();
        cart.add(snapshot, qty, None, vec![]).expect("adds");
        cart
    }

    /// Both reconciliation identities. Called by nearly every test here,
    /// because a bill that does not reconcile cannot be printed honestly.
    fn assert_reconciles(bill: &Bill) {
        let from_lines = Money::try_sum(
            bill.lines
                .iter()
                .map(|l| l.gross_including_tax)
                .chain(bill.charges.iter().map(|c| c.gross_including_tax)),
        )
        .expect("sums")
        .add(bill.round_off)
        .expect("adds");
        assert_eq!(
            from_lines, bill.grand_total,
            "the printed lines and charges do not sum to the printed total"
        );

        // Note `exempt_value`: TaxSummary::total_taxable() deliberately
        // excludes BOTH non-GST and exempt supplies, so an exempt line would
        // vanish from this identity if it were left out.
        let from_summary = Money::try_sum([
            bill.total_taxable,
            bill.tax_total().expect("sums"),
            bill.non_gst_value,
            bill.exempt_value,
            bill.round_off,
        ])
        .expect("sums");
        assert_eq!(
            from_summary, bill.grand_total,
            "the tax summary does not tie to the total"
        );
    }

    /// A deterministic generator, written here rather than pulled in as a
    /// dependency (R6). xorshift64*, seeded with a constant so a failure is
    /// always reproducible.
    struct Rng(u64);

    impl Rng {
        const fn new(seed: u64) -> Self {
            Rng(seed)
        }

        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, limit: u64) -> u64 {
            if limit == 0 { 0 } else { self.next() % limit }
        }
    }

    #[test]
    fn every_generated_bill_reconciles_both_ways() {
        // T1 — the single most important test in the crate. Several thousand
        // carts, every shape a real counter produces.
        let rates = [TaxRate::ZERO, TaxRate::GST_5, TaxRate::GST_12, TaxRate::GST_18, TaxRate::GST_28];
        let treatments = [
            TaxTreatment::Exclusive,
            TaxTreatment::Inclusive,
            TaxTreatment::Exempt,
            TaxTreatment::NonGst,
        ];
        let mut rng = Rng::new(0x5EED_1234_ABCD_0001);

        for case in 0..3_000_u32 {
            let line_count = 1 + rng.below(20);
            let mut cart = Cart::new();

            for line in 0..line_count {
                let paise = 100 + i64::try_from(rng.below(500_000)).unwrap_or(100);
                let rate = rates[usize::try_from(rng.below(5)).unwrap_or(0)];
                let treatment = treatments[usize::try_from(rng.below(4)).unwrap_or(0)];
                // A quantity from 0.001 to ~20, so fractional weights are well
                // represented and not just an occasional half.
                let qty = Qty::from_thousandths(1 + i64::try_from(rng.below(20_000)).unwrap_or(0));

                let snapshot = item(&format!("itm_{case}_{line}"), paise, rate)
                    .with_treatment(treatment);

                let modifiers = if rng.below(4) == 0 {
                    vec![Modifier::new(
                        ModifierId::new("mod_x"),
                        "Extra",
                        Money::from_paise(i64::try_from(rng.below(5_000)).unwrap_or(0)),
                    )]
                } else {
                    vec![]
                };

                let index = cart.add(snapshot, qty, None, modifiers).expect("adds");
                if rng.below(3) == 0 {
                    let discount = if rng.below(2) == 0 {
                        Discount::percent_bp(u32::try_from(rng.below(10_001)).unwrap_or(0))
                    } else {
                        Discount::amount(Money::from_paise(
                            i64::try_from(rng.below(100_000)).unwrap_or(0),
                        ))
                    };
                    if let Some(discount) = discount {
                        cart.set_line_discount(index, Some(DiscountEntry::new(discount)))
                            .expect("sets");
                    }
                }
            }

            let bill_discount = match rng.below(3) {
                0 => Discount::percent_bp(u32::try_from(rng.below(10_001)).unwrap_or(0)),
                1 => Discount::amount(Money::from_paise(
                    i64::try_from(rng.below(200_000)).unwrap_or(0),
                )),
                _ => None,
            };

            // P02: zero to three charges, each with its own rate and treatment.
            let mut charges = Vec::new();
            for slot in 0..rng.below(4) {
                let basis = if rng.below(2) == 0 {
                    ChargeBasis::Percent(u32::try_from(rng.below(2_001)).unwrap_or(0))
                } else {
                    ChargeBasis::Flat(Money::from_paise(
                        i64::try_from(rng.below(20_000)).unwrap_or(0),
                    ))
                };
                charges.push(Charge {
                    kind: match slot {
                        0 => ChargeKind::Service,
                        1 => ChargeKind::Packing,
                        _ => ChargeKind::Delivery,
                    },
                    name: format!("Charge {slot}"),
                    basis,
                    tax_rate: rates[usize::try_from(rng.below(5)).unwrap_or(0)],
                    tax_treatment: treatments[usize::try_from(rng.below(4)).unwrap_or(0)],
                });
            }

            let place = if rng.below(4) == 0 { PlaceOfSupply::Inter } else { PlaceOfSupply::Intra };
            let rounding = [
                RoundingMode::None,
                RoundingMode::NearestRupee,
                RoundingMode::Up,
                RoundingMode::Down,
            ][usize::try_from(rng.below(4)).unwrap_or(0)];

            let mut input = BillInput::new(&cart)
                .with_place_of_supply(place)
                .with_charges(&charges)
                .with_rounding(rounding);
            input.bill_discount = bill_discount.map(DiscountEntry::new);

            let bill = compute_bill(input).expect("computes");
            assert_reconciles(&bill);

            // No line may come out negative, whatever the discounts did.
            for line in &bill.lines {
                assert!(!line.net.is_negative(), "case {case} produced a negative line");
                assert!(!line.gross_including_tax.is_negative());
            }
            // Nor may a charge — a charge is never discounted, so a negative
            // one would mean the base leaked a discount into it.
            for charge in &bill.charges {
                assert!(!charge.amount.is_negative(), "case {case} produced a negative charge");
            }
            // Whatever the mode, the total lands where the mode says.
            if rounding != RoundingMode::None {
                assert_eq!(
                    bill.grand_total.paise() % 100,
                    0,
                    "case {case} rounded under {rounding:?} but kept paise"
                );
            }
        }
    }

    #[test]
    fn a_bill_discount_on_a_mixed_rate_bill_taxes_correctly() {
        // T2 — the audit's B10/B11 case. A 5% dish and an 18% packaged item.
        let mut cart = Cart::new();
        cart.add(item("food", 100_000, TaxRate::GST_5), Qty::ONE, None, vec![])
            .expect("adds");
        cart.add(item("packaged", 100_000, TaxRate::GST_18), Qty::ONE, None, vec![])
            .expect("adds");

        let plain = compute_bill(BillInput::new(&cart).with_rounding(RoundingMode::None)).expect("computes");
        let discounted = compute_bill(
            BillInput::new(&cart)
                .with_rounding(RoundingMode::None)
                .with_bill_discount(DiscountEntry::new(Discount::percent_bp(1_000).expect("valid"))),
        )
        .expect("computes");

        // ₹1000 each. 5% tax = ₹50, 18% tax = ₹180.
        assert_eq!(plain.lines[0].tax.total(), Ok(rs(50)));
        assert_eq!(plain.lines[1].tax.total(), Ok(rs(180)));

        // After 10% off, each line's tax must be exactly 10% lower — because
        // the discount came off the net BEFORE tax, per rate.
        assert_eq!(discounted.lines[0].tax.total(), Ok(rs(45)));
        assert_eq!(discounted.lines[1].tax.total(), Ok(rs(162)));
        assert_eq!(discounted.lines[0].net, rs(900));
        assert_eq!(discounted.lines[1].net, rs(900));

        // And the summary ties per rate, which is what a CA files from.
        let rows: Vec<_> = discounted.summary.rows().copied().collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].taxable, rs(900));
        assert_eq!(rows[1].taxable, rs(900));

        // The regression guard: discounting AFTER tax gives a different total,
        // and it is the wrong one. plain total = 2000 + 230 = 2230; less 10%
        // is 2007. The correct pipeline gives 1800 + 207 = 2007 as well — the
        // totals coincide here, but the TAX does not, and the tax is what is
        // filed. Post-tax discounting leaves ₹50 and ₹180 on the return
        // against ₹1800 of taxable value, which cannot be right.
        let post_tax_wrong_tax = rs(50).add(rs(180)).expect("adds");
        let correct_tax = discounted.tax_total().expect("sums");
        assert_ne!(
            correct_tax, post_tax_wrong_tax,
            "if these are equal the discount is not reaching the tax computation"
        );
        assert_eq!(correct_tax, rs(207));

        assert_reconciles(&discounted);
    }

    #[test]
    fn a_service_charge_carries_its_own_rate_into_the_summary() {
        // T3, and the audit's B13 — the reason charges have their own rate.
        // ₹1,000 of 5% food, with an 18% service charge on top.
        let cart = one_line(item("food", 100_000, TaxRate::GST_5), Qty::ONE);
        let charges = vec![Charge::percent(
            ChargeKind::Service,
            "Service Charge",
            1_000,
            TaxRate::GST_18,
        )];

        let bill = compute_bill(
            BillInput::new(&cart)
                .with_charges(&charges)
                .with_rounding(RoundingMode::None),
        )
        .expect("computes");

        assert_eq!(bill.charges.len(), 1);
        assert_eq!(bill.charges[0].amount, rs(100), "10% of ₹1,000");
        assert_eq!(bill.charges[0].tax.total(), Ok(rs(18)), "the charge is 18%, not 5%");

        // Two rate rows, and the food's 5% row is untouched by the charge.
        let rows: Vec<_> = bill.summary.rows().copied().collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].rate, TaxRate::GST_5);
        assert_eq!(rows[0].taxable, rs(1_000));
        assert_eq!(rows[0].tax.total(), Ok(rs(50)));
        assert_eq!(rows[1].rate, TaxRate::GST_18);
        assert_eq!(rows[1].taxable, rs(100));

        // 1000 + 50 + 100 + 18
        assert_eq!(bill.grand_total, rs(1_168));
        assert_eq!(bill.total_charges, rs(100));
        assert_reconciles(&bill);
    }

    #[test]
    fn the_charge_base_is_the_discounted_line_total_and_never_compounds() {
        // T4 — the ambiguous number, pinned.
        // ₹1,000 of food, 10% bill discount, then two 5% charges.
        let cart = one_line(item("food", 100_000, TaxRate::GST_5), Qty::ONE);
        let charges = vec![
            Charge::percent(ChargeKind::Service, "Service", 500, TaxRate::GST_5),
            Charge::percent(ChargeKind::Packing, "Packing", 500, TaxRate::GST_5),
            Charge::flat(ChargeKind::Delivery, "Delivery", rs(40), TaxRate::GST_5),
        ];

        let bill = compute_bill(
            BillInput::new(&cart)
                .with_charges(&charges)
                .with_rounding(RoundingMode::None)
                .with_bill_discount(DiscountEntry::new(Discount::percent_bp(1_000).expect("valid"))),
        )
        .expect("computes");

        // Base is ₹900 — the DISCOUNTED line total. Not ₹1,000, or the shop
        // takes part of its own discount back through the service charge.
        assert_eq!(bill.charges[0].amount, rs(45), "5% of ₹900, not of ₹1,000");
        // And the second charge is on the same ₹900 — it does NOT compound
        // onto the first (which would give ₹47.25).
        assert_eq!(bill.charges[1].amount, rs(45), "charges must not compound");
        // A flat charge does not move at all.
        assert_eq!(bill.charges[2].amount, rs(40));

        assert_eq!(bill.total_charges, rs(130));
        assert_reconciles(&bill);
    }

    #[test]
    fn the_charge_base_includes_alcohol() {
        // Service applies to the whole table, non-GST lines included. Stated
        // as a test because the alternative (food-only) is a plausible reading
        // and would silently under-charge every bar bill.
        let mut cart = Cart::new();
        cart.add(item("curry", 50_000, TaxRate::GST_5), Qty::ONE, None, vec![])
            .expect("adds");
        cart.add(
            item("beer", 50_000, TaxRate::ZERO).with_treatment(TaxTreatment::NonGst),
            Qty::ONE,
            None,
            vec![],
        )
        .expect("adds");

        let charges = vec![Charge::percent(ChargeKind::Service, "Service", 1_000, TaxRate::GST_18)];
        let bill = compute_bill(
            BillInput::new(&cart)
                .with_charges(&charges)
                .with_rounding(RoundingMode::None),
        )
        .expect("computes");

        assert_eq!(bill.charges[0].amount, rs(100), "10% of the whole ₹1,000 table");
        assert_reconciles(&bill);
    }

    #[test]
    fn a_discount_bigger_than_the_bill_cannot_go_negative_even_with_charges() {
        // T2. The naive implementation subtracts the discount from the total
        // AFTER charges and lands below zero.
        let cart = one_line(item("dish", 30_000, TaxRate::GST_5), Qty::ONE);
        let charges = vec![Charge::flat(ChargeKind::Delivery, "Delivery", rs(40), TaxRate::GST_18)];

        let bill = compute_bill(
            BillInput::new(&cart)
                .with_charges(&charges)
                .with_rounding(RoundingMode::None)
                .with_bill_discount(DiscountEntry::new(Discount::amount(rs(500)).expect("valid"))),
        )
        .expect("computes");

        assert_eq!(bill.total_bill_discount, rs(300), "it can only take the lines");
        assert!(bill.bill_discount_capped);
        assert_eq!(bill.lines[0].net, Money::ZERO);
        // The delivery charge is still owed — a discount on the food does not
        // pay for the rider. ₹40 + 18% tax.
        assert_eq!(bill.grand_total, Money::from_paise(4_720));
        assert!(!bill.grand_total.is_negative());
        assert_reconciles(&bill);
    }

    #[test]
    fn a_charge_on_an_empty_bill_computes_rather_than_panics() {
        // T13. A delivery charge with no items is a mistake upstream — but it
        // must produce a bill, not a crash.
        let cart = Cart::new();
        let charges = vec![
            Charge::percent(ChargeKind::Service, "Service", 1_000, TaxRate::GST_18),
            Charge::flat(ChargeKind::Delivery, "Delivery", rs(40), TaxRate::GST_18),
        ];
        let bill = compute_bill(
            BillInput::new(&cart)
                .with_charges(&charges)
                .with_rounding(RoundingMode::None),
        )
        .expect("computes");

        assert_eq!(bill.charges[0].amount, Money::ZERO, "10% of nothing is nothing");
        assert_eq!(bill.charges[1].amount, rs(40), "a flat charge still applies");
        assert_eq!(bill.grand_total, Money::from_paise(4_720));
        assert_reconciles(&bill);
    }

    #[test]
    fn a_percentage_and_a_flat_discount_that_should_match_do_match() {
        // T9 — to the paisa, through the whole pipeline.
        let cart = one_line(item("dish", 100_000, TaxRate::GST_5), Qty::ONE);

        let by_percent = compute_bill(
            BillInput::new(&cart)
                .with_rounding(RoundingMode::None)
                .with_bill_discount(DiscountEntry::new(Discount::percent_bp(1_000).expect("valid"))),
        )
        .expect("computes");
        let by_amount = compute_bill(
            BillInput::new(&cart)
                .with_rounding(RoundingMode::None)
                .with_bill_discount(DiscountEntry::new(Discount::amount(rs(100)).expect("valid"))),
        )
        .expect("computes");

        assert_eq!(by_percent.total_bill_discount, by_amount.total_bill_discount);
        assert_eq!(by_percent.grand_total, by_amount.grand_total);
        assert_eq!(by_percent.tax_total(), by_amount.tax_total());
    }

    #[test]
    fn the_rounding_mode_reaches_the_bill_and_is_recorded_on_it() {
        // T5, through the pipeline rather than on a bare Money.
        let cart = one_line(
            item("meal", 48_735, TaxRate::ZERO).with_treatment(TaxTreatment::Exempt),
            Qty::ONE,
        );
        let total = |mode| {
            let bill = compute_bill(BillInput::new(&cart).with_rounding(mode)).expect("computes");
            assert_reconciles(&bill);
            assert_eq!(bill.rounding, mode, "the mode must be recorded on the bill");
            // The round-off is always exactly the gap it closed.
            assert_eq!(
                bill.grand_total.sub(bill.round_off),
                Ok(Money::from_paise(48_735))
            );
            bill.grand_total
        };

        assert_eq!(total(RoundingMode::NearestRupee), rs(487));
        assert_eq!(total(RoundingMode::Up), rs(488));
        assert_eq!(total(RoundingMode::Down), rs(487));
        assert_eq!(total(RoundingMode::None), Money::from_paise(48_735));
    }

    #[test]
    fn a_line_discount_carries_its_reason_and_who_gave_it() {
        // Scope 1.12 — the metadata rides with the discount and does not touch
        // the arithmetic.
        let mut cart = Cart::new();
        let index = cart
            .add(item("dish", 100_000, TaxRate::GST_5), Qty::ONE, None, vec![])
            .expect("adds");
        let entry = DiscountEntry::new(Discount::percent_bp(1_000).expect("valid"))
            .with_reason("regular customer")
            .authorised_by(crate::ids::StaffId::new("stf_owner"));
        cart.set_line_discount(index, Some(entry)).expect("sets");

        let stored = cart.lines()[0].line_discount.as_ref().expect("present");
        assert_eq!(stored.reason.as_deref(), Some("regular customer"));
        assert_eq!(stored.authorised_by.as_ref().map(StaffId::as_str), Some("stf_owner"));

        let bill = compute_bill(BillInput::new(&cart).with_rounding(RoundingMode::None))
            .expect("computes");
        assert_eq!(bill.lines[0].line_discount, rs(100));
    }

    #[test]
    fn an_empty_cart_produces_a_zero_bill_not_an_error() {
        // T8
        let cart = Cart::new();
        let bill = compute_bill(BillInput::new(&cart)).expect("computes");
        assert_eq!(bill.grand_total, Money::ZERO);
        assert_eq!(bill.subtotal, Money::ZERO);
        assert_eq!(bill.round_off, Money::ZERO);
        assert!(bill.summary.is_empty());
        assert!(bill.lines.is_empty());
        assert_reconciles(&bill);
    }

    #[test]
    fn an_all_complimentary_bill_with_a_discount_does_not_divide_by_zero() {
        // T9
        let mut cart = Cart::new();
        cart.add(item("free_a", 0, TaxRate::GST_5), Qty::ONE, None, vec![])
            .expect("adds");
        cart.add(item("free_b", 0, TaxRate::GST_18), Qty::ONE, None, vec![])
            .expect("adds");

        let bill = compute_bill(
            BillInput::new(&cart).with_bill_discount(DiscountEntry::new(Discount::percent_bp(1_000).expect("valid"))),
        )
        .expect("computes");

        assert_eq!(bill.grand_total, Money::ZERO);
        assert_eq!(bill.total_bill_discount, Money::ZERO);
        assert_reconciles(&bill);
    }

    #[test]
    fn a_bar_bill_keeps_alcohol_out_of_the_gst_return() {
        // T10 — the master plan's requirement 2, and what v1 could not do.
        let mut cart = Cart::new();
        cart.add(item("beer", 30_000, TaxRate::ZERO).with_treatment(TaxTreatment::NonGst), Qty::ONE, None, vec![])
            .expect("adds");
        cart.add(item("curry", 20_000, TaxRate::GST_5), Qty::ONE, None, vec![])
            .expect("adds");
        cart.add(item("bread", 5_000, TaxRate::ZERO).with_treatment(TaxTreatment::Exempt), Qty::ONE, None, vec![])
            .expect("adds");

        let bill = compute_bill(BillInput::new(&cart).with_rounding(RoundingMode::None)).expect("computes");

        assert_eq!(bill.non_gst_value, rs(300));
        assert_eq!(bill.exempt_value, rs(50));
        assert_eq!(bill.summary.rows().count(), 1, "only the 5% dish is a GST rate");
        assert_eq!(bill.total_taxable, rs(200));
        assert_eq!(bill.tax_total(), Ok(rs(10)));
        // 300 + 50 + 200 + 10
        assert_eq!(bill.grand_total, rs(560));
        assert_reconciles(&bill);
    }

    #[test]
    fn an_inclusive_line_never_drifts_from_its_menu_price() {
        // T11 — proved end to end, not just inside tax.rs.
        for paise in [10_500_i64, 9_999, 1, 33_333, 100] {
            let cart = one_line(
                item("incl", paise, TaxRate::GST_5).with_treatment(TaxTreatment::Inclusive),
                Qty::ONE,
            );
            let bill = compute_bill(BillInput::new(&cart).with_rounding(RoundingMode::None)).expect("computes");
            assert_eq!(
                bill.lines[0].gross_including_tax,
                Money::from_paise(paise),
                "an inclusive line at {paise} paise did not total to its price"
            );
            assert_reconciles(&bill);
        }

        // Still true after a discount takes part of it away.
        let cart = one_line(
            item("incl", 10_500, TaxRate::GST_5).with_treatment(TaxTreatment::Inclusive),
            Qty::ONE,
        );
        let bill = compute_bill(
            BillInput::new(&cart)
                .with_rounding(RoundingMode::None)
                .with_bill_discount(DiscountEntry::new(Discount::percent_bp(1_000).expect("valid"))),
        )
        .expect("computes");
        assert_eq!(bill.lines[0].net, Money::from_paise(9_450));
        assert_eq!(bill.lines[0].gross_including_tax, Money::from_paise(9_450));
        assert_reconciles(&bill);
    }

    #[test]
    fn a_capped_bill_discount_reaches_the_bill() {
        // T12 — the flag, not just the amount.
        let cart = one_line(item("dish", 30_000, TaxRate::GST_5), Qty::ONE);
        let bill = compute_bill(
            BillInput::new(&cart)
                .with_rounding(RoundingMode::None)
                .with_bill_discount(DiscountEntry::new(Discount::amount(rs(500)).expect("valid"))),
        )
        .expect("computes");

        assert_eq!(bill.total_bill_discount, rs(300), "it can only take the whole line");
        assert!(bill.bill_discount_capped, "a silent cap is the bug D7 prevents");
        assert_eq!(bill.grand_total, Money::ZERO);
        assert_reconciles(&bill);
    }

    #[test]
    fn modifiers_multiply_with_the_quantity_and_inherit_the_rate() {
        // T13
        let mut cart = Cart::new();
        cart.add(
            item("pizza", 20_000, TaxRate::GST_5),
            Qty::from_whole(2).expect("in range"),
            None,
            vec![Modifier::new(ModifierId::new("mod_cheese"), "Extra Cheese", rs(30))],
        )
        .expect("adds");

        let bill = compute_bill(BillInput::new(&cart).with_rounding(RoundingMode::None)).expect("computes");
        // (₹200 + ₹30) × 2 = ₹460, taxed at the line's 5%, not at some rate of
        // the modifier's own.
        assert_eq!(bill.lines[0].gross, rs(460));
        assert_eq!(bill.lines[0].rate, TaxRate::GST_5);
        assert_eq!(bill.lines[0].tax.total(), Ok(rs(23)));
        assert_reconciles(&bill);
    }

    #[test]
    fn a_negative_modifier_reduces_the_line_but_cannot_take_it_below_zero() {
        // T13, second half.
        let mut cart = Cart::new();
        cart.add(
            item("burger", 15_000, TaxRate::GST_5),
            Qty::ONE,
            None,
            vec![Modifier::new(ModifierId::new("mod_nocheese"), "No Cheese", rs(-10))],
        )
        .expect("adds");
        let bill = compute_bill(BillInput::new(&cart).with_rounding(RoundingMode::None)).expect("computes");
        assert_eq!(bill.lines[0].gross, rs(140));

        // A delta below the item's own price is refused, not clamped to zero.
        let mut cart = Cart::new();
        cart.add(
            item("burger", 15_000, TaxRate::GST_5),
            Qty::ONE,
            None,
            vec![Modifier::new(ModifierId::new("mod_absurd"), "Absurd", rs(-200))],
        )
        .expect("adds");
        assert!(matches!(
            compute_bill(BillInput::new(&cart)),
            Err(BillError::NegativeLinePrice { index: 0, .. })
        ));
    }

    #[test]
    fn fractional_quantities_reach_the_bill_exactly() {
        // T6, through the pipeline.
        let cart = one_line(item("mutton", 24_000, TaxRate::GST_5), Qty::from_thousandths(500));
        let bill = compute_bill(BillInput::new(&cart).with_rounding(RoundingMode::None)).expect("computes");
        assert_eq!(bill.lines[0].gross, rs(120));
        assert_eq!(bill.lines[0].qty.to_string(), "0.5");
        assert_reconciles(&bill);
    }

    #[test]
    fn round_off_reaches_the_rupee_and_is_recorded_separately() {
        // T16 — the audit's B8: v1 printed ₹487.35.
        let cart = one_line(item("meal", 48_735, TaxRate::ZERO).with_treatment(TaxTreatment::Exempt), Qty::ONE);

        let rounded = compute_bill(BillInput::new(&cart).with_rounding(RoundingMode::NearestRupee)).expect("computes");
        assert_eq!(rounded.round_off, Money::from_paise(-35));
        assert_eq!(rounded.grand_total, rs(487));
        assert_reconciles(&rounded);

        let unrounded = compute_bill(BillInput::new(&cart).with_rounding(RoundingMode::None)).expect("computes");
        assert_eq!(unrounded.round_off, Money::ZERO);
        assert_eq!(unrounded.grand_total, Money::from_paise(48_735));
        assert_reconciles(&unrounded);
    }

    #[test]
    fn an_absurd_quantity_is_an_error_not_a_wrapped_total() {
        // T15 — nothing panics, the error travels out of compute_bill.
        let cart = one_line(item("dish", 100_000, TaxRate::GST_5), Qty::from_thousandths(i64::MAX));
        assert!(matches!(compute_bill(BillInput::new(&cart)), Err(BillError::Money(_))));
    }

    #[test]
    fn the_order_type_and_place_of_supply_reach_the_bill() {
        // Scope 1.5 — carried, not yet acted on.
        let cart = one_line(item("dish", 10_000, TaxRate::GST_5), Qty::ONE);
        let bill = compute_bill(
            BillInput::new(&cart)
                .with_order_type(OrderType::Delivery)
                .with_place_of_supply(PlaceOfSupply::Inter),
        )
        .expect("computes");
        assert_eq!(bill.order_type, OrderType::Delivery);
        assert_eq!(bill.place_of_supply, PlaceOfSupply::Inter);
        // Inter-state: the whole tax is IGST.
        assert_eq!(bill.total_tax.cgst, Money::ZERO);
        assert_eq!(bill.total_tax.igst, rs(5));
    }

    #[test]
    fn both_discounts_apply_in_the_right_order() {
        // Line discount first, then the bill discount on what is left (D4
        // steps 2 and 3). ₹1000 line, ₹100 off, then 10% of ₹900 = ₹90.
        let mut cart = Cart::new();
        let index = cart
            .add(item("dish", 100_000, TaxRate::GST_5), Qty::ONE, None, vec![])
            .expect("adds");
        cart.set_line_discount(
            index,
            Some(DiscountEntry::new(Discount::amount(rs(100)).expect("valid"))),
        )
        .expect("sets");

        let bill = compute_bill(
            BillInput::new(&cart)
                .with_rounding(RoundingMode::None)
                .with_bill_discount(DiscountEntry::new(Discount::percent_bp(1_000).expect("valid"))),
        )
        .expect("computes");

        assert_eq!(bill.lines[0].line_discount, rs(100));
        assert_eq!(bill.lines[0].bill_discount_share, rs(90));
        assert_eq!(bill.lines[0].net, rs(810));
        assert_eq!(bill.total_discount, rs(190));
        assert_eq!(bill.subtotal, rs(1_000), "subtotal is before any discount");
        assert_reconciles(&bill);
    }
}
