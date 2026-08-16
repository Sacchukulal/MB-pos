//! **Did the money arrive, and what did the machine say?** — P29, scope 8.3.
//!
//! Two things live here, and neither of them is the payment itself — payments
//! are written by [`crate::repo::order`], inside the settle, and that does not
//! change.
//!
//! 1. **The attempts ledger.** Every time a provider is asked, the answer is
//!    written down. An approved attempt is nearly redundant; a DECLINED one is
//!    the only record that the event happened at all, because a declined card
//!    leaves no payment row and an unsettled bill.
//! 2. **The unconfirmed list.** Every electronic payment this product takes
//!    today is unconfirmed, because [`mb_core::provider::Manual`] cannot check
//!    a bank and will not pretend to. The list is the feature: a shop cannot
//!    chase what it cannot list, and "what have we not confirmed tonight?" is
//!    a question with an answer for the first time.

use mb_core::businessday::BusinessDay;
use mb_core::money::Money;
use mb_core::provider::Answer;
use mb_core::Timestamp;
use rusqlite::{Transaction, params};

use crate::encode;
use crate::error::DbError;

#[derive(Debug)]
pub struct PaymentsRepo<'a> {
    tx: &'a Transaction<'a>,
}

/// One ask, and what came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub id: String,
    pub order_id: Option<String>,
    pub provider: String,
    pub mode: String,
    pub amount: Money,
    pub reference: Option<String>,
    /// `approved`, `declined` or `waiting`.
    pub answer: String,
    pub because: Option<String>,
    pub at: Timestamp,
}

/// A payment nobody has confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unconfirmed {
    pub order_id: String,
    pub seq: i64,
    pub bill_number: Option<String>,
    pub mode: String,
    pub amount: Money,
    pub reference: Option<String>,
    pub provider: Option<String>,
    pub at: Timestamp,
}

impl<'a> PaymentsRepo<'a> {
    #[must_use]
    pub fn new(tx: &'a Transaction<'a>) -> Self {
        PaymentsRepo { tx }
    }

    /// Write down what a provider said.
    #[allow(clippy::too_many_arguments, reason = "an attempt IS this many facts")]
    pub fn record_attempt(
        &self,
        outlet: &str,
        id: &str,
        order_id: Option<&str>,
        provider: &str,
        mode: &str,
        amount: Money,
        reference: Option<&str>,
        answer: &Answer,
        at: Timestamp,
        day: BusinessDay,
        asked_by: Option<&str>,
    ) -> Result<(), DbError> {
        let (tag, because) = match answer {
            Answer::Approved { reference } => ("approved", Some(reference.clone())),
            Answer::Declined { because } => ("declined", Some(because.clone())),
            Answer::Waiting { because } => ("waiting", Some(because.clone())),
        };
        self.tx.execute(
            "INSERT INTO payment_attempts
                 (id, outlet_id, order_id, provider, mode, amount, reference,
                  answer, because, at, business_day, asked_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                outlet,
                order_id,
                provider,
                mode,
                encode::money_to_sql(amount),
                reference,
                tag,
                because,
                encode::timestamp_to_sql(at),
                encode::business_day_to_sql(day),
                asked_by,
            ],
        )?;
        Ok(())
    }

    /// Every ask on a day, newest first.
    pub fn attempts_on(&self, outlet: &str, day: BusinessDay) -> Result<Vec<Attempt>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, order_id, provider, mode, amount, reference, answer, because, at
               FROM payment_attempts
              WHERE outlet_id = ?1 AND business_day = ?2
           ORDER BY at DESC",
        )?;
        let mut rows = stmt.query(params![outlet, encode::business_day_to_sql(day)])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Attempt {
                id: row.get(0)?,
                order_id: row.get(1)?,
                provider: row.get(2)?,
                mode: row.get(3)?,
                amount: encode::money_from_sql(row.get(4)?),
                reference: row.get(5)?,
                answer: row.get(6)?,
                because: row.get(7)?,
                at: encode::timestamp_from_sql(row.get(8)?),
            });
        }
        Ok(out)
    }

    /// **The list a shop reads at close.**
    ///
    /// Only settled bills: an unsettled one is not money anybody is waiting
    /// for yet, and putting it here would bury the ones that matter.
    pub fn unconfirmed_on(
        &self,
        outlet: &str,
        day: BusinessDay,
    ) -> Result<Vec<Unconfirmed>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT p.order_id, p.seq, o.bill_number_formatted, p.mode, p.amount,
                    p.reference, p.provider, p.received_at
               FROM payments p JOIN orders o ON o.id = p.order_id
              WHERE o.outlet_id = ?1 AND p.business_day = ?2
                AND p.confirmed_at IS NULL
                AND o.state = 'settled'
           ORDER BY p.received_at DESC",
        )?;
        let mut rows = stmt.query(params![outlet, encode::business_day_to_sql(day)])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Unconfirmed {
                order_id: row.get(0)?,
                seq: row.get(1)?,
                bill_number: row.get(2)?,
                mode: row.get(3)?,
                amount: encode::money_from_sql(row.get(4)?),
                reference: row.get(5)?,
                provider: row.get(6)?,
                at: encode::timestamp_from_sql(row.get(7)?),
            });
        }
        Ok(out)
    }

    /// **Somebody says the money arrived.**
    ///
    /// One direction only. Un-confirming would be a second way to make money
    /// disappear from a day that has been closed; if a confirmation was wrong,
    /// the bill is corrected the way every other mistake is (D47).
    pub fn confirm(
        &self,
        outlet: &str,
        order_id: &str,
        seq: i64,
        reference: Option<&str>,
        at: Timestamp,
        by: &str,
    ) -> Result<(), DbError> {
        let changed = self.tx.execute(
            "UPDATE payments
                SET confirmed_at = ?4,
                    confirmed_by = ?5,
                    reference    = COALESCE(?6, reference)
              WHERE order_id = ?2 AND seq = ?3
                AND confirmed_at IS NULL
                AND order_id IN (SELECT id FROM orders WHERE outlet_id = ?1)",
            params![
                outlet,
                order_id,
                seq,
                encode::timestamp_to_sql(at),
                by,
                reference,
            ],
        )?;
        if changed == 0 {
            return Err(DbError::invariant(
                "that payment is already confirmed, or is not on this counter".to_owned(),
            ));
        }
        Ok(())
    }
}
