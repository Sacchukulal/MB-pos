//! Reprints, refunds, reasons — and the sum that has to tie.

use mb_core::{BusinessDay, Money, OrderId, StaffId, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

/// One of the shop's own reasons, offered in the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub sort_order: i64,
    pub is_active: bool,
}

/// Money that went back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refund {
    pub id: String,
    pub order_id: OrderId,
    pub amount: Money,
    pub mode: String,
    pub reason: String,
    pub refunded_at: Timestamp,
    pub refunded_by: Option<StaffId>,
}

/// One piece of paper that was not the first one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReprintRow {
    pub id: String,
    pub order_id: OrderId,
    pub printed_at: Timestamp,
    pub printed_by: Option<StaffId>,
    pub reason: Option<String>,
    /// 2 for the second piece of paper, which is what somebody holding two of them needs it to
    /// mean.
    pub copy: u32,
}

/// The three figures that must always tie, for one business day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayTotals {
    /// Every settled bill, including the ones later voided.
    pub gross: Money,
    /// What was taken back out by voiding.
    pub voids: Money,
    /// `gross - voids`. Computed here so nothing else has to remember to.
    pub net: Money,
    /// Money physically handed back.
    pub refunded: Money,
    pub bills: i64,
    pub voided_bills: i64,
    pub cancelled_orders: i64,
}

#[derive(Debug)]
pub struct CorrectionsRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> CorrectionsRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        CorrectionsRepo { tx }
    }

    // Reasons — the shop's own list.

    pub fn reasons(&self, outlet: &str, kind: &str) -> Result<Vec<Reason>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, kind, text, sort_order, is_active FROM reasons
              WHERE outlet_id = ?1 AND kind = ?2 AND is_active = 1
              ORDER BY sort_order, text",
        )?;
        let rows = stmt.query_map(rusqlite::params![outlet, kind], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, kind, text, sort_order, active) = row?;
            out.push(Reason {
                id,
                kind,
                text,
                sort_order,
                is_active: encode::bool_from_sql(active, "reasons.is_active")?,
            });
        }
        Ok(out)
    }

    /// Add or edit one.
    pub fn save_reason(&self, outlet: &str, reason: &Reason, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO reasons (id, outlet_id, kind, text, sort_order, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET text       = excluded.text,
                                            sort_order = excluded.sort_order,
                                            is_active  = excluded.is_active",
            rusqlite::params![
                reason.id,
                outlet,
                reason.kind,
                reason.text,
                reason.sort_order,
                encode::bool_to_sql(reason.is_active),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "reasons", &reason.id, Op::Upsert, at)
    }

    /// How many pieces of paper this bill has already produced after the first one.
    pub fn reprint_count(&self, order_id: &OrderId) -> Result<u32, DbError> {
        let count: i64 = self.tx.query_row(
            "SELECT COUNT(*) FROM reprints WHERE order_id = ?1",
            [order_id.as_str()],
            |row| row.get(0),
        )?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    /// Record one, and say which copy it is.
    pub fn record_reprint(
        &self,
        outlet: &str,
        order_id: &OrderId,
        by: Option<&StaffId>,
        reason: Option<&str>,
        at: Timestamp,
        business_day: BusinessDay,
    ) -> Result<u32, DbError> {
        let copy = self.reprint_count(order_id)?.saturating_add(2);
        let id = format!("rpr_{}_{copy}", order_id.as_str());
        self.tx.execute(
            "INSERT INTO reprints (id, order_id, printed_at, printed_by, business_day, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                order_id.as_str(),
                encode::timestamp_to_sql(at),
                by.map(StaffId::as_str),
                encode::business_day_to_sql(business_day),
                reason,
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "reprints", &id, Op::Upsert, at)?;
        Ok(copy)
    }

    pub fn reprints_for(&self, order_id: &OrderId) -> Result<Vec<ReprintRow>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, printed_at, printed_by, reason FROM reprints
              WHERE order_id = ?1 ORDER BY printed_at",
        )?;
        let rows = stmt.query_map([order_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for (index, row) in rows.enumerate() {
            let (id, at, by, reason) = row?;
            out.push(ReprintRow {
                id,
                order_id: order_id.clone(),
                printed_at: encode::timestamp_from_sql(at),
                printed_by: by.map(StaffId::new),
                reason,
                // The original is copy 1.
                copy: u32::try_from(index).unwrap_or(0).saturating_add(2),
            });
        }
        Ok(out)
    }

    /// Record money going back.
    pub fn record_refund(
        &self,
        outlet: &str,
        refund: &Refund,
        business_day: BusinessDay,
    ) -> Result<(), DbError> {
        let state: Option<String> = self
            .tx
            .query_row(
                "SELECT state FROM orders WHERE id = ?1",
                [refund.order_id.as_str()],
                |row| row.get(0),
            )
            .ok();

        match state.as_deref() {
            Some("voided") => {}
            Some(other) => {
                return Err(DbError::invariant(format!(
                    "this bill is {other}, and money is only given back against a bill \
                     that has been voided"
                )));
            }
            None => return Err(DbError::invariant("there is no such bill")),
        }

        // What was taken, and what has already gone back.
        let taken: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM payments WHERE order_id = ?1",
            [refund.order_id.as_str()],
            |row| row.get(0),
        )?;
        let already: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM refunds WHERE order_id = ?1",
            [refund.order_id.as_str()],
            |row| row.get(0),
        )?;

        let wanted = encode::money_to_sql(refund.amount);
        if already.saturating_add(wanted) > taken {
            let left = encode::money_from_sql(taken.saturating_sub(already));
            return Err(DbError::invariant(format!(
                "only {} is left to give back on this bill",
                left.to_plain_string()
            )));
        }

        self.tx.execute(
            "INSERT INTO refunds (id, outlet_id, order_id, amount, mode, reason,
                                  refunded_at, refunded_by, business_day)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                refund.id,
                outlet,
                refund.order_id.as_str(),
                wanted,
                refund.mode,
                refund.reason,
                encode::timestamp_to_sql(refund.refunded_at),
                refund.refunded_by.as_ref().map(StaffId::as_str),
                encode::business_day_to_sql(business_day),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "refunds",
            &refund.id,
            Op::Upsert,
            refund.refunded_at,
        )
    }

    /// What has already gone back on this bill.
    pub fn refunded_so_far(&self, order_id: &OrderId) -> Result<Money, DbError> {
        let paise: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM refunds WHERE order_id = ?1",
            [order_id.as_str()],
            |row| row.get(0),
        )?;
        Ok(encode::money_from_sql(paise))
    }

    /// 8's seam. Has this day been closed and locked?
    pub fn order_by_bill_number(
        &self,
        outlet: &str,
        number: &str,
    ) -> Result<Option<String>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id FROM orders
              WHERE outlet_id = ?1 AND bill_number_formatted = ?2
              ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(rusqlite::params![outlet, number])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn day_is_locked(&self, outlet: &str, day: BusinessDay) -> Result<bool, DbError> {
        let locked: Option<i64> = self
            .tx
            .query_row(
                // `terminal_id IS NULL` is the SHOP's row, and leaving it out is a real bug
                // this caught: a till's own drawer close is not locked, so without the filter
                // this answered from whichever row SQLite reached first and a closed day
                // accepted a void.
                "SELECT is_locked FROM day_closes
                  WHERE outlet_id = ?1 AND business_day = ?2 AND terminal_id IS NULL",
                rusqlite::params![outlet, encode::business_day_to_sql(day)],
                |row| row.get(0),
            )
            .ok();
        Ok(locked == Some(1))
    }

    // The sum that has to tie.

    /// Gross − voids = net, for one business day, from the rows.
    pub fn day_totals(&self, outlet: &str, day: BusinessDay) -> Result<DayTotals, DbError> {
        let day = encode::business_day_to_sql(day);

        // The bill's own grand total, not the sum of its payments: an overpayment is change
        // given back, and change is not takings.
        let (gross, bills): (i64, i64) = self.tx.query_row(
            "SELECT COALESCE(SUM(b.grand_total), 0), COUNT(*)
               FROM orders o JOIN bills b ON b.order_id = o.id
              WHERE o.outlet_id = ?1 AND o.business_day = ?2
                AND o.state IN ('settled', 'voided')",
            rusqlite::params![outlet, day],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let (voids, voided_bills): (i64, i64) = self.tx.query_row(
            "SELECT COALESCE(SUM(b.grand_total), 0), COUNT(*)
               FROM orders o JOIN bills b ON b.order_id = o.id
              WHERE o.outlet_id = ?1 AND o.business_day = ?2 AND o.state = 'voided'",
            rusqlite::params![outlet, day],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let refunded: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM refunds
              WHERE outlet_id = ?1 AND business_day = ?2",
            rusqlite::params![outlet, day],
            |row| row.get(0),
        )?;

        let cancelled_orders: i64 = self.tx.query_row(
            "SELECT COUNT(*) FROM orders
              WHERE outlet_id = ?1 AND business_day = ?2 AND state = 'cancelled'",
            rusqlite::params![outlet, day],
            |row| row.get(0),
        )?;

        Ok(DayTotals {
            gross: encode::money_from_sql(gross),
            voids: encode::money_from_sql(voids),
            net: encode::money_from_sql(gross.saturating_sub(voids)),
            refunded: encode::money_from_sql(refunded),
            bills,
            voided_bills,
            cancelled_orders,
        })
    }
}
