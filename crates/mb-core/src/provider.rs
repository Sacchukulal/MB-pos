//! **Did the money actually arrive?** — P29, scope 8.3 and 8.4.
//!
//! # The problem, and it is the most common way a small restaurant loses money
//!
//! Today the cashier looks at a phone, sees a green tick, and presses UPI. On a
//! busy evening they see a green tick from the *previous* customer, or a
//! screenshot, or nothing at all and take the word of somebody holding a queue
//! up behind them. The bill settles either way. Nobody finds out until the bank
//! statement, and by then nobody can say which bill it was.
//!
//! This module is the seam that makes "did it arrive?" a question the software
//! can ask, and — far more importantly today — a question it can RECORD having
//! failed to ask.
//!
//! # What ships now, and what it is worth
//!
//! [`Manual`] is the provider this product ships with, and it is honest about
//! what it is: it never confirms anything. What it does is force the reference
//! to be typed and the payment to be marked **unconfirmed**, so that:
//!
//! * every electronic payment has a reference somebody can search the bank
//!   statement for;
//! * "show me tonight's unconfirmed payments" is a question with an answer;
//! * and the shop finds out at close, not at the end of the month.
//!
//! That is worth more than it sounds. A shop cannot chase what it cannot list.
//!
//! # And a real provider
//!
//! A real one — a UPI aggregator, a card terminal on the counter — implements
//! this same trait in the shell crate, where network calls belong, and
//! **nothing in the billing path changes**. That is what [`Scripted`] proves:
//! the tests drive an approval and a decline through the exact code the manual
//! provider goes through.
//!
//! Choosing an aggregator is a commercial decision the shop's owner makes
//! (FEATURE_SCOPE §15). This module is what makes that decision cost a
//! configuration screen instead of a rewrite.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::money::Money;
use crate::payment::PaymentMode;

/// What the counter wants to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub mode: PaymentMode,
    pub amount: Money,
    /// What the person typed, if they typed anything — a UPI reference, an
    /// approval code from the card slip.
    pub reference: Option<String>,
}

/// What came back.
///
/// **There are three answers, not two.** "I do not know" is the ordinary case
/// for a manual provider and the ordinary case for a card terminal that timed
/// out, and collapsing it into either yes or no is how a shop ends up with
/// bills it believes were paid and bills it refuses to hand food over for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Answer {
    /// The money is there. The reference is what a bank statement will show.
    Approved { reference: String },
    /// The money is not there, and this is why — in the provider's words,
    /// which a cashier has to be able to read out to a customer.
    Declined { because: String },
    /// Nobody can say yet. **The payment is taken and marked unconfirmed**, and
    /// it stays on the unconfirmed list until a person or a provider settles
    /// it.
    Waiting { because: String },
}

impl Answer {
    #[must_use]
    pub fn is_approved(&self) -> bool {
        matches!(self, Answer::Approved { .. })
    }

    /// What the counter shows. UI_GUIDELINES §6: the shop's words, not ours.
    #[must_use]
    pub fn words(&self) -> String {
        match self {
            Answer::Approved { reference } => format!("Confirmed — {reference}"),
            Answer::Declined { because } => format!("Refused — {because}"),
            Answer::Waiting { because } => format!("Not confirmed yet — {because}"),
        }
    }
}

/// Somebody, or something, that can answer [`Ask`].
///
/// One method. A provider that needs to talk to a network does it inside
/// `ask`, in the shell crate — this crate has no I/O and this trait does not
/// give it any.
pub trait Provider: Send + Sync + std::fmt::Debug {
    /// What the settings screen calls it.
    fn name(&self) -> &'static str;

    /// **Never blocks the sale.** A provider that cannot answer inside its own
    /// deadline returns [`Answer::Waiting`]; it does not hold the counter.
    fn ask(&self, ask: &Ask) -> Answer;
}

/// **What this product ships with: a person, recorded.**
///
/// It confirms nothing, because nothing here can. What it does is make the
/// reference compulsory for an electronic payment and leave the payment
/// visibly unconfirmed.
#[derive(Debug, Default, Clone, Copy)]
pub struct Manual;

impl Provider for Manual {
    fn name(&self) -> &'static str {
        "Typed in by hand"
    }

    fn ask(&self, ask: &Ask) -> Answer {
        match ask.mode {
            // Cash is the one mode nobody has to be asked about: the notes are
            // in the drawer, and a shop that marks cash "unconfirmed" has a
            // list nobody will ever read.
            PaymentMode::Cash => Answer::Approved {
                reference: String::new(),
            },
            // A credit sale is a promise, and the promise IS the record. P15
            // owns whether it was kept.
            PaymentMode::Credit(_) => Answer::Approved {
                reference: String::new(),
            },
            _ => match ask.reference.as_deref().map(str::trim) {
                Some(reference) if !reference.is_empty() => Answer::Waiting {
                    because: "nobody has checked the bank yet".to_owned(),
                },
                _ => Answer::Waiting {
                    because: "no reference was typed".to_owned(),
                },
            },
        }
    }
}

/// **A provider that answers from a list** — the test double.
///
/// It ships rather than hiding behind `#[cfg(test)]` because the thing worth
/// proving is that the billing path treats a real provider and this one
/// identically, and that proof lives in another crate.
#[derive(Debug, Default)]
pub struct Scripted {
    answers: Mutex<Vec<Answer>>,
    /// Every ask it was given, in order.
    pub seen: Mutex<Vec<Ask>>,
}

impl Scripted {
    #[must_use]
    pub fn new(answers: Vec<Answer>) -> Self {
        // Reversed, so `pop` hands them back in the order they were written.
        let mut answers = answers;
        answers.reverse();
        Scripted {
            answers: Mutex::new(answers),
            seen: Mutex::new(Vec::new()),
        }
    }
}

impl Provider for Scripted {
    fn name(&self) -> &'static str {
        "A stand-in, for testing"
    }

    fn ask(&self, ask: &Ask) -> Answer {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(ask.clone());
        }
        let next = self.answers.lock().ok().and_then(|mut a| a.pop());
        next.unwrap_or(Answer::Waiting {
            because: "the stand-in has run out of answers".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rs(rupees: i64) -> Money {
        Money::from_paise(rupees * 100)
    }

    fn ask(mode: PaymentMode, reference: Option<&str>) -> Ask {
        Ask {
            mode,
            amount: rs(640),
            reference: reference.map(str::to_owned),
        }
    }

    #[test]
    fn cash_is_never_unconfirmed() {
        // The list of unconfirmed payments has to be short enough to read, or
        // nobody reads it. Cash in the drawer is not on it.
        assert!(
            Manual
                .ask(&ask(PaymentMode::Cash, None))
                .is_approved()
        );
    }

    #[test]
    fn a_upi_payment_is_waiting_even_with_a_reference() {
        // **This is the honest part.** A typed reference is not a confirmation
        // — nothing checked it — and saying otherwise would be exactly the lie
        // this module exists to stop.
        let answer = Manual.ask(&ask(PaymentMode::Upi, Some("4477 1123")));
        assert!(matches!(answer, Answer::Waiting { .. }));
        assert!(answer.words().starts_with("Not confirmed yet"));
    }

    #[test]
    fn a_reference_that_is_missing_says_so_differently_from_one_that_is_there() {
        let with = Manual.ask(&ask(PaymentMode::Upi, Some("4477 1123")));
        let without = Manual.ask(&ask(PaymentMode::Upi, None));
        assert_ne!(with, without, "the two are different situations at close");
    }

    /// **The one that matters: the two are interchangeable.**
    ///
    /// The caller below is written against the trait and has no idea which
    /// provider it is holding — which is the whole claim being made about a
    /// real aggregator dropping in later.
    #[test]
    fn a_real_provider_and_the_manual_one_go_through_the_same_door() {
        fn take(provider: &dyn Provider, mode: PaymentMode) -> Answer {
            provider.ask(&ask(mode, Some("ref-1")))
        }

        let approved = Scripted::new(vec![Answer::Approved {
            reference: "APPROVAL 8891".to_owned(),
        }]);
        assert!(take(&approved, PaymentMode::Card).is_approved());
        assert!(!take(&Manual, PaymentMode::Card).is_approved());
    }

    #[test]
    fn a_decline_carries_the_reason_in_words_a_cashier_can_read_out() {
        let terminal = Scripted::new(vec![Answer::Declined {
            because: "the bank refused the card".to_owned(),
        }]);
        let answer = terminal.ask(&ask(PaymentMode::Card, None));
        assert!(!answer.is_approved());
        assert_eq!(answer.words(), "Refused — the bank refused the card");
    }

    #[test]
    fn a_provider_that_runs_out_of_answers_waits_rather_than_approving() {
        // A stand-in that approved by accident would make every test that used
        // it up pass for the wrong reason.
        let empty = Scripted::new(Vec::new());
        assert!(matches!(
            empty.ask(&ask(PaymentMode::Upi, None)),
            Answer::Waiting { .. }
        ));
    }
}
