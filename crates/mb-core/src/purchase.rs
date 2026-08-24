//! **What a delivery actually cost** — P26, scope 4.5.
//!
//! # The sentence this module exists for
//!
//! **A cost is what the money bought, not what the rate column said.**
//!
//! The rate on an invoice is not what a kilo cost. What a kilo cost is the rate,
//! minus the discount, minus the free bag they threw in, plus the ₹200 tempo,
//! plus the GST the shop cannot claim back — divided by what actually came off
//! the vehicle. Every one of those five is ordinary in an Indian mandi and
//! **four of them move the figure away from the printed rate**. A product that
//! stores the printed rate as the cost is wrong on the very first delivery, and
//! nothing downstream can recover it: the food-cost report, the margin, the
//! stock valuation and the menu-engineering quadrant are all built on it.
//!
//! # Pure, like every other engine in this crate
//!
//! No database, no clock, no settings lookup. It is handed what a person typed
//! and it answers with money and quantities — which is what makes the
//! arithmetic an owner will argue with provable in a millisecond.
//!
//! # The three rules it does not get to re-decide
//!
//! * **D14 — one split, largest remainder.** The transport charge and the
//!   whole-invoice discount are both spread by [`crate::discount::spread`], the
//!   function a bill discount already uses. A second answer to "how do I split
//!   ₹200 across seven lines" is a second answer, and one of them is wrong.
//! * **D118/D122 — this module does not average anything.** It produces the
//!   landed cost of THIS delivery. `UnitCost::blend`, called inside
//!   `StockRepo::record`, is what turns a sequence of deliveries into the
//!   material's cost, and there is no setting to make it do otherwise.
//! * **D123 — the free quantity is a denominator.** Buy 10 bags at ₹1,000 and
//!   get 1 free: eleven bags arrived, ₹10,000 was paid, and a bag cost ₹909.09.
//!   Booking the free bag at zero cost makes a material's average sag after
//!   every scheme and a stock valuation wrong by the same amount.

use std::fmt;

use crate::discount::spread;
use crate::money::{Money, MoneyError, RoundingMode};
use crate::qty::{Qty, QtyError};
use crate::units::{Pack, UnitCost, UnitError};

/// Something about the paper that cannot be turned into numbers.
///
/// Every one of these is a refusal a person reads and fixes on the screen in
/// front of them, which is why they carry the figures rather than a code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurchaseError {
    /// An invoice with nothing on it.
    NoLines,
    /// A line for none of something.
    NothingReceived { seq: usize },
    /// A pack that converts to nothing — a "bag" defined as zero.
    EmptyPack { seq: usize },
    /// A line discount larger than the line.
    DiscountTooBig { seq: usize, discount: Money, value: Money },
    /// A whole-invoice discount larger than the invoice.
    InvoiceDiscountTooBig { discount: Money, value: Money },
    /// A negative anything.
    Negative { what: &'static str },
    /// Money or quantity ran out of room.
    Overflow,
}

impl fmt::Display for PurchaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PurchaseError::NoLines => write!(f, "There is nothing on this invoice yet."),
            PurchaseError::NothingReceived { seq } => {
                write!(f, "Line {} has no quantity on it.", seq + 1)
            }
            PurchaseError::EmptyPack { seq } => write!(
                f,
                "Line {}'s unit is defined as nothing. Fix the pack size on the material.",
                seq + 1
            ),
            PurchaseError::DiscountTooBig { seq, discount, value } => write!(
                f,
                "Line {} has a discount of {} on a line worth {}.",
                seq + 1,
                discount.to_indian_string(),
                value.to_indian_string()
            ),
            PurchaseError::InvoiceDiscountTooBig { discount, value } => write!(
                f,
                "The invoice discount of {} is more than the {} of goods on it.",
                discount.to_indian_string(),
                value.to_indian_string()
            ),
            PurchaseError::Negative { what } => write!(f, "The {what} cannot be a negative amount."),
            PurchaseError::Overflow => write!(f, "That number is too large to record."),
        }
    }
}

impl std::error::Error for PurchaseError {}

impl From<MoneyError> for PurchaseError {
    fn from(_: MoneyError) -> Self {
        PurchaseError::Overflow
    }
}

impl From<QtyError> for PurchaseError {
    fn from(_: QtyError) -> Self {
        PurchaseError::Overflow
    }
}

impl From<UnitError> for PurchaseError {
    fn from(_: UnitError) -> Self {
        PurchaseError::Overflow
    }
}

type Result<T> = std::result::Result<T, PurchaseError>;

/// One line of the paper, exactly as somebody typed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The quantity CHARGED for, in `pack`.
    pub typed_qty: Qty,
    /// The scheme's free quantity, in the same pack. Received, never charged.
    pub free_typed_qty: Qty,
    /// **The pack the quantity and the rate are both quoted in** — the material's
    /// own bag, tin or tray, or its base unit (D108). Passing it in rather than
    /// looking it up is what keeps this module free of the database.
    pub pack: Pack,
    /// Paise per whole `pack`: ₹1,000 a bag is `100000`.
    pub rate: Money,
    /// A discount printed against this line.
    pub discount: Money,
    /// The GST rate on the line, in basis points. 500 is 5%.
    pub tax_rate_bp: u32,
}

/// The whole paper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invoice {
    pub lines: Vec<Entry>,
    /// A discount on the whole bill, apportioned across the lines by value.
    pub invoice_discount: Money,
    /// Transport, loading, hamali — apportioned across the lines by value and
    /// **becoming part of the cost of the food**, which is the entire point of
    /// recording them.
    ///
    /// Treated as tax-free here. A freight line that carries its own GST is
    /// entered as an ordinary line against a material, because that is what it
    /// is on the paper and because guessing which is which is how a tax report
    /// stops matching an invoice.
    pub charges: Money,
    /// **D124 — a property of the SHOP, not of the line.** Most Indian
    /// restaurants bill at 5% under notification 11/2017 and may not claim input
    /// credit; for them the GST on a delivery is simply part of what the paneer
    /// cost. `store_profile.registration` is where the answer lives.
    pub tax_is_creditable: bool,
    /// Applied to the grand total only, exactly as D4 step 7 applies it to a
    /// bill. An invoice's own round-off is what makes the typed total match.
    pub rounding: RoundingMode,
}

/// One line, costed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Costed {
    /// The charged quantity in base units.
    pub base_qty: Qty,
    /// The free quantity in base units.
    pub free_base_qty: Qty,
    /// **What actually came off the vehicle** — charged plus free. The
    /// denominator of D123 and the quantity that goes on the shelf.
    pub received_base_qty: Qty,
    /// rate × charged quantity.
    pub line_value: Money,
    /// This line's share of the whole-invoice discount (D14).
    pub discount_share: Money,
    /// This line's share of the transport and loading (D14).
    pub charge_share: Money,
    /// The taxable value: line value less both discounts.
    pub taxable: Money,
    pub tax_amount: Money,
    /// The tax that is a cost rather than a credit — zero for a claiming shop,
    /// the whole of it for a 5%-scheme shop (D124).
    pub tax_in_cost: Money,
    /// **The numerator of D123**: taxable + charge share + non-creditable tax.
    pub landed_value: Money,
    /// Paise per 1,000 base units, the same scale as `materials.avg_cost`, so
    /// ₹40 a kilo reads `4000`.
    pub landed_unit_cost: UnitCost,
}

/// The paper, costed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostedInvoice {
    pub lines: Vec<Costed>,
    pub lines_value: Money,
    pub line_discounts: Money,
    pub invoice_discount: Money,
    pub charges: Money,
    /// Value after both discounts — what the tax was charged on.
    pub taxable: Money,
    pub tax_total: Money,
    /// **D124.** How much of `tax_total` the shop may actually claim back.
    pub tax_creditable: Money,
    pub round_off: Money,
    /// goods − discounts + charges + tax, then rounded.
    pub total: Money,
}

impl CostedInvoice {
    /// What the shop pays for goods and carriage, before tax. Used by the
    /// screen's running total and by the reports.
    pub fn goods_value(&self) -> Result<Money> {
        Ok(self.taxable.add(self.charges)?)
    }
}

/// **Cost the paper.**
///
/// The order is fixed, and it is D4's order with the buying-side steps in it:
///
/// 1. every line's charged and free quantity, in base units;
/// 2. line value = rate × charged quantity, less the line's own discount;
/// 3. the whole-invoice discount spread across the lines by value (D14);
/// 4. tax per line, on what is left;
/// 5. the charges spread across the lines by value (D14);
/// 6. the landed value per line = taxable + charge share + non-creditable tax;
/// 7. **the landed unit cost = landed value ÷ (charged + FREE)** — D123;
/// 8. the totals, then the round-off on the grand total only.
///
/// Steps 3 and 5 are the same function as a bill discount, called twice.
pub fn cost_invoice(invoice: &Invoice) -> Result<CostedInvoice> {
    if invoice.lines.is_empty() {
        return Err(PurchaseError::NoLines);
    }
    if invoice.invoice_discount.is_negative() {
        return Err(PurchaseError::Negative { what: "invoice discount" });
    }
    if invoice.charges.is_negative() {
        return Err(PurchaseError::Negative { what: "transport or loading charge" });
    }

    // -- 1 and 2: quantities and line values -------------------------------
    let mut base = Vec::with_capacity(invoice.lines.len());
    let mut nets = Vec::with_capacity(invoice.lines.len());
    let mut lines_value = Money::ZERO;
    let mut line_discounts = Money::ZERO;

    for (seq, entry) in invoice.lines.iter().enumerate() {
        if !entry.typed_qty.is_positive() {
            return Err(PurchaseError::NothingReceived { seq });
        }
        if entry.free_typed_qty.is_negative() {
            return Err(PurchaseError::Negative { what: "free quantity" });
        }
        if entry.rate.is_negative() {
            return Err(PurchaseError::Negative { what: "rate" });
        }
        if entry.discount.is_negative() {
            return Err(PurchaseError::Negative { what: "line discount" });
        }
        if !entry.pack.base_per_unit.is_positive() {
            return Err(PurchaseError::EmptyPack { seq });
        }

        let charged = entry.pack.to_base(entry.typed_qty)?;
        let free = if entry.free_typed_qty.is_zero() {
            Qty::ZERO
        } else {
            entry.pack.to_base(entry.free_typed_qty)?
        };
        // The rate is per WHOLE pack and the quantity is in packs, so this is
        // `Qty::extend` — the same "price × quantity, rounded exactly once"
        // that every cart line has used since P01.
        let value = entry.typed_qty.extend(entry.rate)?;
        if entry.discount > value {
            return Err(PurchaseError::DiscountTooBig { seq, discount: entry.discount, value });
        }
        let net = value.sub(entry.discount)?;

        lines_value = lines_value.add(value)?;
        line_discounts = line_discounts.add(entry.discount)?;
        base.push((charged, free, value, net));
        nets.push(net);
    }

    // -- 3: the whole-invoice discount, spread by value ---------------------
    let net_total = Money::try_sum(nets.iter().copied())?;
    if invoice.invoice_discount > net_total {
        return Err(PurchaseError::InvoiceDiscountTooBig {
            discount: invoice.invoice_discount,
            value: net_total,
        });
    }
    let discount_shares = spread(invoice.invoice_discount, &nets)?;

    // -- 4 and 5: tax, then the charges, both on what is left ---------------
    //
    // The charges are spread over the same weights as the discount rather than
    // over the post-discount value, so that the two splits cannot disagree
    // about which line is the big one.
    let charge_shares = spread(invoice.charges, &nets)?;

    let mut lines = Vec::with_capacity(invoice.lines.len());
    let mut taxable_total = Money::ZERO;
    let mut tax_total = Money::ZERO;
    let mut tax_creditable = Money::ZERO;

    for (index, entry) in invoice.lines.iter().enumerate() {
        let (charged, free, value, net) = base[index];
        let discount_share = discount_shares[index];
        let charge_share = charge_shares[index];
        let taxable = net.sub(discount_share)?;
        let tax_amount = taxable.percent_bp(entry.tax_rate_bp)?;

        // **D124 in one line.** For a claiming shop the tax comes back, so it is
        // not a cost; for a 5%-scheme shop ₹100 of paneer at 5% cost ₹105 and
        // the dish has to say so.
        let tax_in_cost = if invoice.tax_is_creditable { Money::ZERO } else { tax_amount };
        if invoice.tax_is_creditable {
            tax_creditable = tax_creditable.add(tax_amount)?;
        }

        // -- 6 and 7: the landed value, and D123's denominator --------------
        let landed_value = taxable.add(charge_share)?.add(tax_in_cost)?;
        let received = charged.add(free)?;
        if !received.is_positive() {
            return Err(PurchaseError::NothingReceived { seq: index });
        }
        let landed_unit_cost = UnitCost::from_batch(landed_value, received)?;

        taxable_total = taxable_total.add(taxable)?;
        tax_total = tax_total.add(tax_amount)?;
        lines.push(Costed {
            base_qty: charged,
            free_base_qty: free,
            received_base_qty: received,
            line_value: value,
            discount_share,
            charge_share,
            taxable,
            tax_amount,
            tax_in_cost,
            landed_value,
            landed_unit_cost,
        });
        let _ = net;
    }

    // -- 8: the totals, and the round-off on the grand total only -----------
    let before_rounding = taxable_total.add(invoice.charges)?.add(tax_total)?;
    let round_off = before_rounding.round_adjustment(invoice.rounding);
    let total = before_rounding.add(round_off)?;

    Ok(CostedInvoice {
        lines,
        lines_value,
        line_discounts,
        invoice_discount: invoice.invoice_discount,
        charges: invoice.charges,
        taxable: taxable_total,
        tax_total,
        tax_creditable,
        round_off,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(name: &str, base_per_unit: i64) -> Pack {
        Pack::new(name, Qty::from_thousandths(base_per_unit)).expect("a pack")
    }

    /// A 25 kg bag, in grams: 25,000 g held as thousandths.
    fn bag() -> Pack {
        pack("bag", 25_000_000)
    }

    fn gram() -> Pack {
        pack("g", 1_000)
    }

    fn line(qty: i64, free: i64, rate_paise: i64, pack: Pack) -> Entry {
        Entry {
            typed_qty: Qty::from_thousandths(qty),
            free_typed_qty: Qty::from_thousandths(free),
            pack,
            rate: Money::from_paise(rate_paise),
            discount: Money::ZERO,
            tax_rate_bp: 0,
        }
    }

    fn plain(lines: Vec<Entry>) -> Invoice {
        Invoice {
            lines,
            invoice_discount: Money::ZERO,
            charges: Money::ZERO,
            tax_is_creditable: false,
            rounding: RoundingMode::None,
        }
    }

    #[test]
    fn two_bags_of_rice_at_a_thousand_a_bag() {
        // 2 bags × ₹1,000 = ₹2,000 for 50,000 g, so a kilo cost ₹40 and the
        // stored figure — paise per 1,000 base units — is 4000.
        let costed = cost_invoice(&plain(vec![line(2_000, 0, 100_000, bag())])).expect("costs");
        assert_eq!(costed.lines[0].received_base_qty, Qty::from_thousandths(50_000_000));
        assert_eq!(costed.total, Money::from_paise(200_000));
        assert_eq!(costed.lines[0].landed_unit_cost, UnitCost::from_paise_per_thousand(4_000));
    }

    #[test]
    fn a_free_bag_is_a_denominator_and_not_a_zero_priced_line() {
        // **D123.** 10 bags at ₹1,000 with 1 free: ₹10,000 bought 11 bags, so a
        // bag cost ₹909.0909… and a kilo cost ₹36.3636… — 3636 paise per 1,000 g
        // after one rounding, and NOT the ₹40 a naive reading gives.
        let costed = cost_invoice(&plain(vec![line(10_000, 1_000, 100_000, bag())])).expect("costs");
        assert_eq!(costed.lines[0].received_base_qty, Qty::from_thousandths(275_000_000));
        assert_eq!(costed.total, Money::from_paise(1_000_000));
        assert_eq!(costed.lines[0].landed_unit_cost, UnitCost::from_paise_per_thousand(3_636));
        // The shelf gains eleven bags, not ten. This is the assertion that
        // catches a rewrite that "simplifies" free goods away.
        assert_eq!(
            costed.lines[0].free_base_qty,
            Qty::from_thousandths(25_000_000),
            "the free bag must reach the shelf"
        );
    }

    #[test]
    fn transport_is_apportioned_by_value_and_the_parts_sum_exactly() {
        // ₹200 of tempo across three awkward lines. **D14**: the parts must sum
        // to ₹200 to the paisa, with no rupee invented and none lost.
        let invoice = Invoice {
            lines: vec![
                line(3_000, 0, 33_300, gram()),
                line(7_000, 0, 11_100, gram()),
                line(1_000, 0, 5_500, gram()),
            ],
            invoice_discount: Money::ZERO,
            charges: Money::from_paise(20_000),
            tax_is_creditable: false,
            rounding: RoundingMode::None,
        };
        let costed = cost_invoice(&invoice).expect("costs");
        let shares = Money::try_sum(costed.lines.iter().map(|l| l.charge_share)).expect("sums");
        assert_eq!(shares, Money::from_paise(20_000));
        assert!(costed.lines.iter().all(|l| !l.charge_share.is_negative()));
        // And the charge really is inside the cost: the landed value of every
        // line is more than its taxable value.
        assert!(costed.lines.iter().all(|l| l.landed_value > l.taxable));
    }

    #[test]
    fn the_invoice_discount_is_the_same_split_run_a_second_time() {
        let invoice = Invoice {
            lines: vec![line(1_000, 0, 10_000, gram()), line(1_000, 0, 20_000, gram())],
            invoice_discount: Money::from_paise(3_000),
            charges: Money::ZERO,
            tax_is_creditable: false,
            rounding: RoundingMode::None,
        };
        let costed = cost_invoice(&invoice).expect("costs");
        assert_eq!(costed.lines[0].discount_share, Money::from_paise(1_000));
        assert_eq!(costed.lines[1].discount_share, Money::from_paise(2_000));
        assert_eq!(costed.total, Money::from_paise(27_000));
    }

    #[test]
    fn the_same_invoice_costs_more_when_the_shop_cannot_claim_the_tax() {
        // **D124, and it is the decision that decides whether the food-cost
        // figure is right for ninety per cent of Indian restaurants.**
        let make = |creditable: bool| Invoice {
            lines: vec![Entry { tax_rate_bp: 500, ..line(1_000, 0, 10_000, gram()) }],
            invoice_discount: Money::ZERO,
            charges: Money::ZERO,
            tax_is_creditable: creditable,
            rounding: RoundingMode::None,
        };
        let claiming = cost_invoice(&make(true)).expect("costs");
        let scheme = cost_invoice(&make(false)).expect("costs");

        // Both pay the same money to the supplier.
        assert_eq!(claiming.total, scheme.total);
        assert_eq!(claiming.tax_total, scheme.tax_total);
        // Only one of them gets it back.
        assert_eq!(claiming.tax_creditable, Money::from_paise(500));
        assert_eq!(scheme.tax_creditable, Money::ZERO);
        // And the food costs exactly the tax more for the shop that does not.
        assert_eq!(
            scheme.lines[0].landed_value.sub(claiming.lines[0].landed_value),
            Ok(Money::from_paise(500))
        );
    }

    #[test]
    fn a_line_discount_lowers_the_cost_and_an_impossible_one_is_refused() {
        let costed = cost_invoice(&plain(vec![Entry {
            discount: Money::from_paise(2_000),
            ..line(1_000, 0, 10_000, gram())
        }]))
        .expect("costs");
        assert_eq!(costed.total, Money::from_paise(8_000));

        let refused = cost_invoice(&plain(vec![Entry {
            discount: Money::from_paise(20_000),
            ..line(1_000, 0, 10_000, gram())
        }]));
        assert!(matches!(refused, Err(PurchaseError::DiscountTooBig { seq: 0, .. })));
        // The refusal is a sentence somebody can act on, not a code.
        assert!(refused.unwrap_err().to_string().contains("Line 1"));
    }

    #[test]
    fn an_empty_invoice_and_an_empty_pack_are_both_refused_in_words() {
        assert_eq!(cost_invoice(&plain(vec![])), Err(PurchaseError::NoLines));

        let mut entry = line(1_000, 0, 10_000, gram());
        entry.pack.base_per_unit = Qty::ZERO;
        let refused = cost_invoice(&plain(vec![entry]));
        assert!(matches!(refused, Err(PurchaseError::EmptyPack { seq: 0 })));
        assert!(refused.unwrap_err().to_string().contains("pack size"));
    }

    #[test]
    fn the_round_off_lands_on_the_grand_total_only() {
        let invoice = Invoice {
            lines: vec![Entry { tax_rate_bp: 500, ..line(1_000, 0, 10_033, gram()) }],
            invoice_discount: Money::ZERO,
            charges: Money::ZERO,
            tax_is_creditable: true,
            rounding: RoundingMode::NearestRupee,
        };
        let costed = cost_invoice(&invoice).expect("costs");
        assert_eq!(costed.total.paise() % 100, 0, "a rounded total ends in whole rupees");
        // The line is untouched: D4 step 7 rounds the total and nothing else.
        assert_eq!(costed.lines[0].taxable, Money::from_paise(10_033));
    }

    #[test]
    fn everything_still_reconciles_when_all_four_happen_at_once() {
        // A real mandi invoice: two materials, a free bag, a line discount, an
        // invoice discount, transport, and tax the shop cannot claim.
        let invoice = Invoice {
            lines: vec![
                Entry {
                    discount: Money::from_paise(5_000),
                    tax_rate_bp: 500,
                    ..line(10_000, 1_000, 100_000, bag())
                },
                Entry { tax_rate_bp: 1_200, ..line(4_000, 0, 24_500, gram()) },
            ],
            invoice_discount: Money::from_paise(7_500),
            charges: Money::from_paise(20_000),
            tax_is_creditable: false,
            rounding: RoundingMode::NearestRupee,
        };
        let costed = cost_invoice(&invoice).expect("costs");

        // The invoice adds up: goods − discounts + charges + tax + round-off.
        let expected = costed
            .lines_value
            .sub(costed.line_discounts)
            .and_then(|v| v.sub(costed.invoice_discount))
            .and_then(|v| v.add(costed.charges))
            .and_then(|v| v.add(costed.tax_total))
            .and_then(|v| v.add(costed.round_off))
            .expect("adds up");
        assert_eq!(costed.total, expected);

        // And the money in the lines is the money on the invoice: every paisa
        // of goods, charge and non-creditable tax is inside some line's landed
        // value, which is what makes the stock valuation reconcile with the
        // supplier ledger.
        let landed = Money::try_sum(costed.lines.iter().map(|l| l.landed_value)).expect("sums");
        let expected_landed = costed
            .taxable
            .add(costed.charges)
            .and_then(|v| v.add(costed.tax_total))
            .expect("adds up");
        assert_eq!(landed, expected_landed);
    }
}
