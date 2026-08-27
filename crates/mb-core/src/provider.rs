//! Did the money actually arrive?

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::money::Money;
use crate::payment::PaymentMode;

/// What the counter wants to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub mode: PaymentMode,
    pub amount: Money,
    /// What the person typed, if they typed anything — a UPI reference, an approval code from
    /// the card slip.
    pub reference: Option<String>,
}

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Answer {
    /// The money is there.
    Approved { reference: String },
    /// The money is not there, and this is why — in the provider's words, which a cashier has
    /// to be able to read out to a customer.
    Declined { because: String },
    /// Nobody can say yet.
    Waiting { because: String },
}

impl Answer {
    #[must_use]
    pub fn is_approved(&self) -> bool {
        matches!(self, Answer::Approved { .. })
    }

    /// What the counter shows.
    #[must_use]
    pub fn words(&self) -> String {
        match self {
            Answer::Approved { reference } => format!("Confirmed — {reference}"),
            Answer::Declined { because } => format!("Refused — {because}"),
            Answer::Waiting { because } => format!("Not confirmed yet — {because}"),
        }
    }
}

/// Somebody, or something, that can answer `Ask`.
pub trait Provider: Send + Sync + std::fmt::Debug {
    /// What the settings screen calls it.
    fn name(&self) -> &'static str;

    /// Never blocks the sale.
    fn ask(&self, ask: &Ask) -> Answer;
}

/// What this product ships with: a person, recorded.
#[derive(Debug, Default, Clone, Copy)]
pub struct Manual;

impl Provider for Manual {
    fn name(&self) -> &'static str {
        "Typed in by hand"
    }

    fn ask(&self, ask: &Ask) -> Answer {
        match ask.mode {
            // Cash is the one mode nobody has to be asked about: the notes are in the drawer,
            // and a shop that marks cash "unconfirmed" has a list nobody will ever read.
            PaymentMode::Cash => Answer::Approved {
                reference: String::new(),
            },
            // A credit sale is a promise, and the promise IS the record.
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

/// A provider that answers from a list — the test double.
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
        // The list of unconfirmed payments has to be short enough to read, or nobody reads it.
        assert!(Manual.ask(&ask(PaymentMode::Cash, None)).is_approved());
    }

    #[test]
    fn a_upi_payment_is_waiting_even_with_a_reference() {
        // This is the honest part.
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

    /// The one that matters: the two are interchangeable.
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
        // A stand-in that approved by accident would make every test that used it up pass for
        // the wrong reason.
        let empty = Scripted::new(Vec::new());
        assert!(matches!(
            empty.ask(&ask(PaymentMode::Upi, None)),
            Answer::Waiting { .. }
        ));
    }
}
