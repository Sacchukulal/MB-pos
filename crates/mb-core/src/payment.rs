//! How a bill gets paid: several modes on one bill, change, and tips.

use crate::ids::CustomerId;
use crate::money::{Money, MoneyError};
use serde::{Deserialize, Serialize};

/// Something wrong with how a bill was paid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaymentError {
    #[error("a payment has to be more than zero")]
    NonPositiveAmount,
    #[error("a tip cannot be negative")]
    NegativeTip,
    #[error("that is more than was received through this payment mode")]
    RefundTooLarge,
    #[error("that payment label is ambiguous; choose the original payment method again")]
    AmbiguousRefundMode,
    /// You cannot hand change back out of a card machine.
    #[error(
        "card, UPI and credit payments come to ₹{non_cash}, which is more than the ₹{due} owed — take the extra in cash or reduce the amount"
    )]
    CannotOverpayWithoutCash { non_cash: Money, due: Money },
    #[error("an amount on this settlement is too large to handle: {0}")]
    Money(#[from] MoneyError),
}

type Result<T> = std::result::Result<T, PaymentError>;

/// How one payment was made.
///
/// ```
/// # use mb_core::{payment::PaymentMode, CustomerId};
/// // This is the only way to build a credit payment — there is no
/// // `PaymentMode::Credit` without an id to reach for.
/// let mode = PaymentMode::Credit(CustomerId::new("cus_42"));
/// assert!(matches!(mode, PaymentMode::Credit(_)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentMode {
    Cash,
    Card,
    Upi,
    /// On the customer's credit.
    Credit(CustomerId),
    /// Cheque, meal card, a wallet the shop uses.
    Other(String),
}

impl PaymentMode {
    /// Stable financial identity, separate from a display label. A custom method named
    /// Cash must never authorize physical cash leaving the drawer.
    #[must_use]
    pub fn refund_code(&self) -> String {
        match self {
            PaymentMode::Cash => "cash".to_owned(),
            PaymentMode::Card => "card".to_owned(),
            PaymentMode::Upi => "upi".to_owned(),
            PaymentMode::Credit(_) => "credit".to_owned(),
            PaymentMode::Other(label) => format!("other:{}", label.trim().to_ascii_lowercase()),
        }
    }
    /// Cash is the only mode that can produce change.
    #[must_use]
    pub const fn is_cash(&self) -> bool {
        matches!(self, PaymentMode::Cash)
    }

    /// The label a payment-mode report groups by.
    #[must_use]
    pub fn report_label(&self) -> &str {
        match self {
            PaymentMode::Cash => "Cash",
            PaymentMode::Card => "Card",
            PaymentMode::Upi => "UPI",
            PaymentMode::Credit(_) => "Credit",
            PaymentMode::Other(name) => name,
        }
    }
}

/// One payment against one bill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    pub mode: PaymentMode,
    pub amount: Money,
    /// A UPI reference, a card approval code, a cheque number. Left off the wire when there
    /// is none, like every optional field a bill carries (see `CartLine`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    pub settles_credit: bool,
    #[serde(default)]
    pub confirmed: bool,
    /// Which provider answered. `None` on the modes nobody has to be asked about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

impl Payment {
    pub fn new(mode: PaymentMode, amount: Money) -> Result<Self> {
        if !amount.is_positive() {
            // A zero-rupee payment row is noise in every report downstream.
            return Err(PaymentError::NonPositiveAmount);
        }
        // Cash and credit start confirmed, everything else does not.
        let confirmed = matches!(mode, PaymentMode::Cash | PaymentMode::Credit(_));
        Ok(Payment {
            mode,
            amount,
            reference: None,
            settles_credit: false,
            confirmed,
            provider: None,
        })
    }

    #[must_use]
    pub fn with_reference(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }

    /// What a provider said about it.
    #[must_use]
    pub fn answered_by(mut self, provider: impl Into<String>, confirmed: bool) -> Self {
        self.provider = Some(provider.into());
        self.confirmed = confirmed;
        self
    }

    /// Mark this as clearing a credit balance.
    #[must_use]
    pub fn settling_credit(mut self) -> Self {
        self.settles_credit = true;
        self
    }
}

/// Every payment against one bill, plus the tip.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Settlement {
    payments: Vec<Payment>,
    /// Money the customer adds on top.
    tip: Money,
}

/// A persisted receipt: actual retained money, with the included tip identified once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptPayment {
    pub seq: usize,
    pub payment: Payment,
    pub tip: Money,
}

impl Settlement {
    /// The original settlement keeps cash tendered for the receipt's change line.
    /// Ledgers use this projection so change cannot become income or be refunded twice.
    /// `payment.amount` includes `tip`; tip is metadata, never extra money received.
    pub fn receipts(&self, grand_total: Money) -> Result<Vec<ReceiptPayment>> {
        self.validate(grand_total)?;
        let mut change = self.change_due(grand_total)?;
        let mut tip = self.tip;
        let mut rows = Vec::new();
        for (seq, original) in self.payments.iter().enumerate() {
            let mut payment = original.clone();
            if payment.mode.is_cash() && change.is_positive() {
                let given_back = std::cmp::min(payment.amount, change);
                payment.amount = payment.amount.sub(given_back)?;
                change = change.sub(given_back)?;
            }
            if !payment.amount.is_positive() {
                continue;
            }
            let included_tip = std::cmp::min(payment.amount, tip);
            tip = tip.sub(included_tip)?;
            rows.push(ReceiptPayment {
                seq,
                payment,
                tip: included_tip,
            });
        }
        Ok(rows)
    }
    /// Adjust the current allocation after a separately recorded return. Issued versions
    /// retain the original receipts; this projection says how much still pays this bill.
    pub fn return_to(&mut self, mode: &str, amount: Money) -> Result<()> {
        if !amount.is_positive() {
            return Err(PaymentError::NonPositiveAmount);
        }
        let code = mode.trim().to_ascii_lowercase();
        let reserved = matches!(code.as_str(), "cash" | "card" | "upi" | "credit" | "other")
            || code.starts_with("other:");
        if mode.trim() != code && reserved && self.payments.iter().any(|p|
            matches!(&p.mode, PaymentMode::Other(label) if label.trim().eq_ignore_ascii_case(&code))) {
            return Err(PaymentError::AmbiguousRefundMode);
        }
        let accepts = |payment: &Payment| {
            payment.mode.refund_code() == code
                || (!reserved
                    && matches!(&payment.mode, PaymentMode::Other(label) if label.trim().eq_ignore_ascii_case(&code)))
        };
        let available = Money::try_sum(
            self.payments
                .iter()
                .filter(|p| accepts(p))
                .map(|p| p.amount),
        )?;
        if amount > available {
            return Err(PaymentError::RefundTooLarge);
        }
        let mut remaining = amount;
        for payment in self.payments.iter_mut().filter(|p| accepts(p)) {
            let take = if payment.amount < remaining {
                payment.amount
            } else {
                remaining
            };
            payment.amount = payment.amount.sub(take)?;
            remaining = remaining.sub(take)?;
        }
        self.payments.retain(|p| p.amount.is_positive());
        Ok(())
    }

    pub fn append(&mut self, other: &Settlement) -> Result<()> {
        self.set_tip(self.tip.add(other.tip)?)?;
        for payment in &other.payments {
            self.add(payment.clone())?;
        }
        Ok(())
    }
    #[must_use]
    pub fn new() -> Self {
        Settlement::default()
    }

    pub fn with_tip(tip: Money) -> Result<Self> {
        let mut settlement = Settlement::new();
        settlement.set_tip(tip)?;
        Ok(settlement)
    }

    pub fn add(&mut self, payment: Payment) -> Result<()> {
        if !payment.amount.is_positive() {
            return Err(PaymentError::NonPositiveAmount);
        }
        self.payments.push(payment);
        Ok(())
    }

    pub fn set_tip(&mut self, tip: Money) -> Result<()> {
        if tip.is_negative() {
            return Err(PaymentError::NegativeTip);
        }
        self.tip = tip;
        Ok(())
    }

    #[must_use]
    pub fn payments(&self) -> &[Payment] {
        &self.payments
    }

    #[must_use]
    pub const fn tip(&self) -> Money {
        self.tip
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.payments.is_empty()
    }

    pub fn total_paid(&self) -> Result<Money> {
        Ok(Money::try_sum(self.payments.iter().map(|p| p.amount))?)
    }

    fn total_non_cash(&self) -> Result<Money> {
        Ok(Money::try_sum(
            self.payments
                .iter()
                .filter(|p| !p.mode.is_cash())
                .map(|p| p.amount),
        )?)
    }

    /// What the customer owes: the bill plus any tip.
    pub fn amount_due(&self, grand_total: Money) -> Result<Money> {
        Ok(grand_total.add(self.tip)?)
    }

    /// Positive means still owed; negative means overpaid.
    pub fn balance(&self, grand_total: Money) -> Result<Money> {
        Ok(self.amount_due(grand_total)?.sub(self.total_paid()?)?)
    }

    pub fn is_settled(&self, grand_total: Money) -> Result<bool> {
        Ok(!self.balance(grand_total)?.is_positive())
    }

    /// What to hand back.
    pub fn change_due(&self, grand_total: Money) -> Result<Money> {
        let balance = self.balance(grand_total)?;
        Ok(if balance.is_negative() {
            balance.neg()
        } else {
            Money::ZERO
        })
    }

    /// You cannot get change out of a card machine.
    pub fn validate(&self, grand_total: Money) -> Result<()> {
        let due = self.amount_due(grand_total)?;
        let non_cash = self.total_non_cash()?;
        if non_cash > due {
            return Err(PaymentError::CannotOverpayWithoutCash { non_cash, due });
        }
        Ok(())
    }

    /// Totals by mode, for the payment-mode report, in the order first seen.
    pub fn total_by_mode(&self) -> Result<Vec<(String, Money)>> {
        let mut totals: Vec<(String, Money)> = Vec::new();
        for payment in &self.payments {
            let label = payment.mode.report_label();
            match totals.iter_mut().find(|(existing, _)| existing == label) {
                Some((_, running)) => *running = running.add(payment.amount)?,
                None => totals.push((label.to_owned(), payment.amount)),
            }
        }
        Ok(totals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rs(rupees: i64) -> Money {
        Money::from_paise(rupees * 100)
    }

    fn pay(mode: PaymentMode, rupees: i64) -> Payment {
        Payment::new(mode, rs(rupees)).expect("valid payment")
    }

    #[test]
    fn ledger_receipts_remove_cash_change_and_include_tip_only_once() {
        let mut settlement = Settlement::with_tip(rs(5)).expect("tip");
        settlement.add(pay(PaymentMode::Cash, 200)).expect("cash");
        let rows = settlement.receipts(rs(105)).expect("net receipts");
        assert_eq!(rows[0].payment.amount, rs(110));
        assert_eq!(rows[0].tip, rs(5));
        assert_eq!(settlement.total_paid().expect("original"), rs(200));
        assert_eq!(
            settlement.change_due(rs(105)).expect("original change"),
            rs(90)
        );
    }

    #[test]
    fn ledger_keeps_payment_identity_when_cash_is_all_returned_as_change() {
        let mut settlement = Settlement::new();
        settlement.add(pay(PaymentMode::Cash, 50)).expect("cash");
        settlement.add(pay(PaymentMode::Card, 105)).expect("card");
        let rows = settlement.receipts(rs(105)).expect("net receipts");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].seq, 1);
        assert_eq!(rows[0].payment.mode, PaymentMode::Card);
        assert_eq!(rows[0].payment.amount, rs(105));
    }

    #[test]
    fn ledger_never_takes_change_out_of_a_custom_tender_named_cash() {
        let mut settlement = Settlement::new();
        settlement
            .add(pay(PaymentMode::Other("Cash".to_owned()), 50))
            .expect("custom mode");
        settlement.add(pay(PaymentMode::Cash, 100)).expect("cash");
        let rows = settlement.receipts(rs(105)).expect("receipts");
        assert_eq!(rows[0].payment.amount, rs(50));
        assert_eq!(rows[1].payment.amount, rs(55));
    }

    #[test]
    fn canonical_returns_distinguish_custom_cash_and_reject_ambiguous_legacy_labels() {
        let mut settlement = Settlement::new();
        settlement
            .add(pay(PaymentMode::Other("Cash".to_owned()), 50))
            .expect("custom");
        settlement.add(pay(PaymentMode::Cash, 100)).expect("cash");
        let original = settlement.clone();
        assert_eq!(
            settlement.return_to("Cash", rs(10)),
            Err(PaymentError::AmbiguousRefundMode)
        );
        assert_eq!(settlement, original);
        settlement
            .return_to("other:cash", rs(20))
            .expect("custom return");
        assert_eq!(settlement.payments()[0].amount, rs(30));
        assert_eq!(settlement.payments()[1].amount, rs(100));
        settlement.return_to("cash", rs(40)).expect("physical cash");
        assert_eq!(settlement.payments()[0].amount, rs(30));
        assert_eq!(settlement.payments()[1].amount, rs(60));
    }

    #[test]
    fn legacy_unambiguous_custom_labels_still_return_their_own_money() {
        let mut settlement = Settlement::new();
        settlement
            .add(pay(PaymentMode::Other("Meal card".to_owned()), 50))
            .expect("custom");
        settlement
            .return_to("Meal card", rs(10))
            .expect("legacy saved correction");
        assert_eq!(settlement.payments()[0].amount, rs(40));
        assert_eq!(
            settlement.payments()[0].mode.refund_code(),
            "other:meal card"
        );
    }

    #[test]
    fn part_cash_and_part_upi_settles_a_bill_exactly() {
        let mut settlement = Settlement::new();
        settlement.add(pay(PaymentMode::Cash, 300)).expect("adds");
        settlement.add(pay(PaymentMode::Upi, 200)).expect("adds");

        assert_eq!(settlement.total_paid(), Ok(rs(500)));
        assert_eq!(settlement.balance(rs(500)), Ok(Money::ZERO));
        assert_eq!(settlement.is_settled(rs(500)), Ok(true));
        assert_eq!(settlement.change_due(rs(500)), Ok(Money::ZERO));
        assert_eq!(settlement.validate(rs(500)), Ok(()));
    }

    #[test]
    fn an_underpaid_bill_is_not_settled() {
        let mut settlement = Settlement::new();
        settlement.add(pay(PaymentMode::Cash, 450)).expect("adds");
        assert_eq!(
            settlement.balance(rs(500)),
            Ok(rs(50)),
            "positive means still owed"
        );
        assert_eq!(settlement.is_settled(rs(500)), Ok(false));
        assert_eq!(settlement.change_due(rs(500)), Ok(Money::ZERO));
    }

    #[test]
    fn cash_over_the_total_becomes_change() {
        let mut settlement = Settlement::new();
        settlement.add(pay(PaymentMode::Cash, 600)).expect("adds");
        assert_eq!(settlement.change_due(rs(500)), Ok(rs(100)));
        assert_eq!(settlement.is_settled(rs(500)), Ok(true));
        assert_eq!(settlement.validate(rs(500)), Ok(()));
    }

    #[test]
    fn you_cannot_get_change_out_of_a_card_machine() {
        let mut settlement = Settlement::new();
        settlement.add(pay(PaymentMode::Card, 600)).expect("adds");
        assert_eq!(
            settlement.validate(rs(500)),
            Err(PaymentError::CannotOverpayWithoutCash {
                non_cash: rs(600),
                due: rs(500)
            })
        );

        // But card up to the total, with cash on top, is fine — the change comes out of the
        // cash.
        let mut settlement = Settlement::new();
        settlement.add(pay(PaymentMode::Card, 400)).expect("adds");
        settlement.add(pay(PaymentMode::Cash, 200)).expect("adds");
        assert_eq!(settlement.validate(rs(500)), Ok(()));
        assert_eq!(settlement.change_due(rs(500)), Ok(rs(100)));
    }

    #[test]
    fn a_zero_rupee_payment_is_refused() {
        assert_eq!(
            Payment::new(PaymentMode::Cash, Money::ZERO),
            Err(PaymentError::NonPositiveAmount)
        );
        assert_eq!(
            Payment::new(PaymentMode::Cash, Money::from_paise(-1)),
            Err(PaymentError::NonPositiveAmount)
        );
    }

    #[test]
    fn a_tip_is_owed_on_top_and_is_never_taxed() {
        // The tip changes what is DUE, not what the bill is.
        let mut settlement = Settlement::with_tip(rs(50)).expect("valid tip");
        assert_eq!(settlement.amount_due(rs(500)), Ok(rs(550)));

        settlement.add(pay(PaymentMode::Cash, 550)).expect("adds");
        assert_eq!(settlement.is_settled(rs(500)), Ok(true));
        assert_eq!(settlement.change_due(rs(500)), Ok(Money::ZERO));

        // The bill's own grand total is untouched — nothing here can reach the tax summary,
        // because a tip is not a supply by the restaurant.
        assert_eq!(settlement.tip(), rs(50));
        assert_eq!(Settlement::with_tip(rs(-1)), Err(PaymentError::NegativeTip));
    }

    #[test]
    fn credit_carries_its_customer_and_reports_as_one_credit_row() {
        // Two different customers on one bill is unusual but legal — a split between two
        // regulars.
        let mut settlement = Settlement::new();
        settlement
            .add(pay(PaymentMode::Credit(CustomerId::new("cus_1")), 200))
            .expect("adds");
        settlement
            .add(pay(PaymentMode::Credit(CustomerId::new("cus_2")), 300))
            .expect("adds");

        let totals = settlement.total_by_mode().expect("totals");
        assert_eq!(totals, vec![("Credit".to_owned(), rs(500))]);

        let PaymentMode::Credit(ref customer) = settlement.payments()[0].mode else {
            panic!("the first payment must be a credit payment");
        };
        assert_eq!(customer.as_str(), "cus_1");
    }

    #[test]
    fn a_credit_settlement_keeps_its_real_payment_mode() {
        let mut settlement = Settlement::new();
        settlement
            .add(pay(PaymentMode::Cash, 500).settling_credit())
            .expect("adds");

        assert!(settlement.payments()[0].settles_credit);
        assert_eq!(settlement.payments()[0].mode, PaymentMode::Cash);
        assert_eq!(
            settlement.total_by_mode(),
            Ok(vec![("Cash".to_owned(), rs(500))]),
            "a credit settlement paid in cash counts as cash"
        );
    }

    #[test]
    fn totals_by_mode_group_and_keep_the_order_they_were_taken_in() {
        let mut settlement = Settlement::new();
        settlement.add(pay(PaymentMode::Upi, 100)).expect("adds");
        settlement.add(pay(PaymentMode::Cash, 50)).expect("adds");
        settlement.add(pay(PaymentMode::Upi, 25)).expect("adds");

        assert_eq!(
            settlement.total_by_mode(),
            Ok(vec![
                ("UPI".to_owned(), rs(125)),
                ("Cash".to_owned(), rs(50))
            ])
        );
    }

    #[test]
    fn an_unpaid_bill_owes_all_of_itself() {
        let settlement = Settlement::new();
        assert!(settlement.is_empty());
        assert_eq!(settlement.total_paid(), Ok(Money::ZERO));
        assert_eq!(settlement.balance(rs(500)), Ok(rs(500)));
        assert_eq!(settlement.is_settled(rs(500)), Ok(false));
        // A zero bill with no payments is settled — nothing is owed.
        assert_eq!(settlement.is_settled(Money::ZERO), Ok(true));
    }
}
