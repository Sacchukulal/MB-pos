//! How a bill gets paid: several modes on one bill, change, and tips.
//!
//! v1 allowed **one payment mode per bill** (audit B9). Part cash and part UPI
//! is extremely common in an Indian restaurant, and on v1 the cashier had to
//! lie about it — which quietly corrupted every payment-mode report.
//!
//! **A settlement is not an input to `compute_bill`.** A bill is computed, then
//! it is paid. Keeping the two apart is what lets P12 void a paid bill and P18
//! report on payment modes without either of them touching the tax engine.

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
    /// You cannot hand change back out of a card machine.
    #[error("card, UPI and credit payments come to ₹{non_cash}, which is more than the ₹{due} owed — take the extra in cash or reduce the amount")]
    CannotOverpayWithoutCash { non_cash: Money, due: Money },
    #[error("an amount on this settlement is too large to handle: {0}")]
    Money(#[from] MoneyError),
}

type Result<T> = std::result::Result<T, PaymentError>;

/// How one payment was made.
///
/// **`Credit` holds the customer id inside the variant**, so a credit sale with
/// nobody to bill is not a state this program can be in. It is made
/// unrepresentable rather than validated:
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
    /// On the customer's credit. The id is not optional.
    Credit(CustomerId),
    /// Cheque, meal card, a wallet the shop uses. Kept narrow — free text
    /// syncs to the cloud forever (D16).
    Other(String),
}

impl PaymentMode {
    /// Cash is the only mode that can produce change.
    #[must_use]
    pub const fn is_cash(&self) -> bool {
        matches!(self, PaymentMode::Cash)
    }

    /// The label a payment-mode report groups by.
    ///
    /// **Every `Credit` groups together**, whichever customer it was. The
    /// report the owner wants is "how much went on credit this month", not one
    /// row per customer — that is a credit statement, which is P15's job.
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
    /// A UPI reference, a card approval code, a cheque number. Short.
    pub reference: Option<String>,
    /// **Audit B12.** v1 recorded a credit settlement with the payment mode
    /// `"Full Settlement"`, which is not a payment mode and polluted every
    /// payment-mode report. A credit settlement is a real payment — cash, UPI
    /// or card — that happens to clear a balance. So `mode` says what it
    /// **was** and this flag says what it **did**.
    pub settles_credit: bool,
    /// **P29, scope 8.3 — did the money actually arrive?**
    ///
    /// False on every electronic payment this product takes today, because the
    /// provider that ships ([`crate::provider::Manual`]) cannot check a bank
    /// and will not pretend to. Cash and credit are true the moment they are
    /// taken: the notes are in the drawer, and a promise IS the record.
    ///
    /// The point of the field is the LIST it makes possible — "show me
    /// tonight's unconfirmed payments" — because a shop cannot chase what it
    /// cannot list.
    #[serde(default)]
    pub confirmed: bool,
    /// Which provider answered. `None` on the modes nobody has to be asked
    /// about.
    #[serde(default)]
    pub provider: Option<String>,
}

impl Payment {
    pub fn new(mode: PaymentMode, amount: Money) -> Result<Self> {
        if !amount.is_positive() {
            // A zero-rupee payment row is noise in every report downstream.
            return Err(PaymentError::NonPositiveAmount);
        }
        // **Cash and credit start confirmed, everything else does not.** The
        // default is the safe direction: a mode this product has not thought
        // about yet lands on the list a person reads, rather than quietly
        // counting as money in hand.
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

    /// What a provider said about it (P29).
    #[must_use]
    pub fn answered_by(mut self, provider: impl Into<String>, confirmed: bool) -> Self {
        self.provider = Some(provider.into());
        self.confirmed = confirmed;
        self
    }

    /// Mark this as clearing a credit balance. The mode stays what it was.
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
    /// Scope 8.5. Money the customer adds on top. It is **not** part of the
    /// bill's taxable value and never enters the GST summary — a tip is not a
    /// supply by the restaurant, it is a gift to the staff.
    tip: Money,
}

impl Settlement {
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
            self.payments.iter().filter(|p| !p.mode.is_cash()).map(|p| p.amount),
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

    /// What to hand back. Zero unless more was tendered than was owed.
    pub fn change_due(&self, grand_total: Money) -> Result<Money> {
        let balance = self.balance(grand_total)?;
        Ok(if balance.is_negative() { balance.neg() } else { Money::ZERO })
    }

    /// **You cannot get change out of a card machine.**
    ///
    /// So the non-cash payments together may never exceed what is owed. Cash
    /// may, and the excess becomes [`Settlement::change_due`].
    ///
    /// This is checked here rather than in [`Settlement::add`] because `add`
    /// does not know the bill total — and asking it to would mean re-validating
    /// every earlier payment each time a new one arrives.
    pub fn validate(&self, grand_total: Money) -> Result<()> {
        let due = self.amount_due(grand_total)?;
        let non_cash = self.total_non_cash()?;
        if non_cash > due {
            return Err(PaymentError::CannotOverpayWithoutCash { non_cash, due });
        }
        Ok(())
    }

    /// Totals by mode, for the payment-mode report, in the order first seen.
    ///
    /// Every `Credit` collapses into one row — see [`PaymentMode::report_label`].
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
    fn part_cash_and_part_upi_settles_a_bill_exactly() {
        // The case v1 forced the cashier to lie about (audit B9).
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
        assert_eq!(settlement.balance(rs(500)), Ok(rs(50)), "positive means still owed");
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
            Err(PaymentError::CannotOverpayWithoutCash { non_cash: rs(600), due: rs(500) })
        );

        // But card up to the total, with cash on top, is fine — the change
        // comes out of the cash.
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
        // Scope 8.5. The tip changes what is DUE, not what the bill is.
        let mut settlement = Settlement::with_tip(rs(50)).expect("valid tip");
        assert_eq!(settlement.amount_due(rs(500)), Ok(rs(550)));

        settlement.add(pay(PaymentMode::Cash, 550)).expect("adds");
        assert_eq!(settlement.is_settled(rs(500)), Ok(true));
        assert_eq!(settlement.change_due(rs(500)), Ok(Money::ZERO));

        // The bill's own grand total is untouched — nothing here can reach the
        // tax summary, because a tip is not a supply by the restaurant.
        assert_eq!(settlement.tip(), rs(50));
        assert_eq!(Settlement::with_tip(rs(-1)), Err(PaymentError::NegativeTip));
    }

    #[test]
    fn credit_carries_its_customer_and_reports_as_one_credit_row() {
        // Two different customers on one bill is unusual but legal — a split
        // between two regulars. The report wants one Credit row, not two.
        let mut settlement = Settlement::new();
        settlement
            .add(pay(PaymentMode::Credit(CustomerId::new("cus_1")), 200))
            .expect("adds");
        settlement
            .add(pay(PaymentMode::Credit(CustomerId::new("cus_2")), 300))
            .expect("adds");

        let totals = settlement.total_by_mode().expect("totals");
        assert_eq!(totals, vec![("Credit".to_owned(), rs(500))]);

        // And the id is still reachable for P15's ledger.
        let PaymentMode::Credit(ref customer) = settlement.payments()[0].mode else {
            panic!("the first payment must be a credit payment");
        };
        assert_eq!(customer.as_str(), "cus_1");
    }

    #[test]
    fn a_credit_settlement_keeps_its_real_payment_mode() {
        // Audit B12: v1 wrote the mode as "Full Settlement". Here the mode is
        // Cash, and the flag records what the payment did.
        let mut settlement = Settlement::new();
        settlement.add(pay(PaymentMode::Cash, 500).settling_credit()).expect("adds");

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
            Ok(vec![("UPI".to_owned(), rs(125)), ("Cash".to_owned(), rs(50))])
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
