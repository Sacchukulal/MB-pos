//! Current-day full returns of closed historical sales. The original bill is never rewritten.

use mb_core::{BusinessDay, Money, OrderId, StaffId, Timestamp};
use rusqlite::{OptionalExtension as _, Transaction};

use crate::{DbError, encode};
use super::{DaysRepo, Op, OutboxRepo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullReturn {
    pub id: String,
    pub order_id: OrderId,
    pub business_day: BusinessDay,
    pub terminal_id: String,
    pub at: Timestamp,
    pub by: StaffId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnRow {
    pub id: String,
    pub order_id: OrderId,
    pub business_day: BusinessDay,
    pub at: Timestamp,
    pub by: StaffId,
    pub reason: String,
    pub total: Money,
}

#[derive(Debug)]
pub struct ReturnsRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> ReturnsRepo<'a> {
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self { Self { tx } }

    pub fn for_order(&self, order_id: &OrderId) -> Result<Option<ReturnRow>, DbError> {
        self.tx.query_row(
            "SELECT id, business_day, returned_at, returned_by, reason, grand_total
             FROM bill_returns WHERE order_id = ?1", [order_id.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, i64>(5)?))
            }).optional()?.map(|(id, day, at, by, reason, total)| Ok(ReturnRow {
                id, order_id: order_id.clone(), business_day: encode::business_day_from_sql(day, "bill_returns.business_day")?,
                at: Timestamp::from_millis(at), by: StaffId::new(by), reason, total: Money::from_paise(total),
            })).transpose()
    }

    pub fn contains(&self, order_id: &OrderId) -> Result<bool, DbError> {
        Ok(self.tx.query_row("SELECT EXISTS(SELECT 1 FROM bill_returns WHERE order_id = ?1)",
            [order_id.as_str()], |r| r.get(0))?)
    }

    /// Must share the caller's transaction with its refund rows and audit event. `false`
    /// means the same return was already committed: the caller must not refund it again.
    /// Food already prepared is not added back into stock merely because money is returned.
    pub fn record_full(&self, outlet: &str, request: &FullReturn) -> Result<bool, DbError> {
        if request.id.trim().is_empty() || request.reason.trim().is_empty() {
            return Err(DbError::invariant("A return needs a reference and a reason."));
        }
        if let Some(previous) = self.for_order(&request.order_id)? {
            if previous.id == request.id && previous.by == request.by && previous.reason == request.reason {
                return Ok(false);
            }
            return Err(DbError::invariant("This bill already has a full return. Open its history to see the refund."));
        }
        let day: i64 = self.tx.query_row(
            "SELECT business_day FROM orders WHERE id = ?1 AND outlet_id = ?2 AND state = 'settled'",
            rusqlite::params![request.order_id.as_str(), outlet], |r| r.get(0)).optional()?
            .ok_or_else(|| DbError::invariant("Only a paid historical bill can be returned."))?;
        let original_day = encode::business_day_from_sql(day, "orders.business_day")?;
        if original_day >= request.business_day || !DaysRepo::new(self.tx).is_locked(outlet, original_day)? {
            return Err(DbError::invariant("Use the normal bill correction for an open day. This return is for a closed earlier day."));
        }
        if DaysRepo::new(self.tx).is_locked(outlet, request.business_day)? {
            return Err(DbError::invariant("Today's business day is closed. Open today's day before returning money."));
        }
        let editing: bool = self.tx.query_row("SELECT EXISTS(SELECT 1 FROM order_edits WHERE order_id = ?1)",
            [request.order_id.as_str()], |r| r.get(0))?;
        if editing { return Err(DbError::invariant("Finish or discard this bill's correction before returning it.")); }
        let saved = self.tx.execute(
            "INSERT INTO bill_returns (id, outlet_id, order_id, terminal_id, business_day,
                original_business_day, returned_at, returned_by, reason, bill_number, order_type,
                table_id, grand_total, total_charges, total_discount, total_taxable, total_cgst, total_sgst, total_igst, total_vat)
             SELECT ?1, o.outlet_id, o.id, ?2, ?3, o.business_day, ?4, ?5, ?6,
                o.bill_number_formatted, o.order_type, o.table_id, b.grand_total, b.total_charges, b.total_discount,
                b.total_taxable, b.total_cgst, b.total_sgst, b.total_igst, b.total_vat
             FROM orders o JOIN bills b ON b.order_id = o.id WHERE o.id = ?7",
            rusqlite::params![request.id, request.terminal_id, request.business_day.days_since_epoch(),
                request.at.millis(), request.by.as_str(), request.reason, request.order_id.as_str()])?;
        if saved != 1 { return Err(DbError::invariant("The original bill's stored totals are missing.")); }
        self.tx.execute(
            "INSERT INTO bill_return_lines (id, return_id, seq, item_id, name, category_id, hsn, qty,
                gross_including_tax, line_discount, bill_discount_share, taxable, cgst, sgst, igst, vat, rate_bp, tax_kind)
             SELECT ?1 || '_line_' || l.seq, ?1, l.seq, l.item_id, l.name, l.category_id, l.hsn, l.qty,
                b.gross_including_tax, b.line_discount, b.bill_discount_share, b.taxable, b.cgst, b.sgst,
                b.igst, b.vat, b.rate_bp, b.tax_kind FROM order_lines l JOIN bill_lines b ON b.order_line_id = l.id
             WHERE l.order_id = ?2", rusqlite::params![request.id, request.order_id.as_str()])?;
        let outbox = OutboxRepo::new(self.tx);
        let mut credit = self.tx.prepare_cached("SELECT customer_id, SUM(amount) FROM payments WHERE order_id = ?1 AND mode = 'credit' AND customer_id IS NOT NULL GROUP BY customer_id")?;
        for customer in credit.query_map([request.order_id.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
            let (customer, amount) = customer?;
            if amount <= 0 { continue; }
            super::money::MoneyRepo::new(self.tx).save_credit_adjustment(outlet, &super::money::CreditAdjustment {
                id: format!("{}_credit_{customer}", request.id), customer_id: mb_core::CustomerId::new(customer),
                amount: Money::from_paise(amount), increases: false,
                reason: format!("Return of bill {}: {}", request.order_id.as_str(), request.reason),
                at: request.at, business_day: request.business_day, made_by: Some(request.by.clone()),
            })?;
        }
        outbox.enqueue(outlet, "bill_returns", &request.id, Op::Upsert, request.at)?;
        for table in super::wire::TOTALS_TABLES {
            outbox.enqueue(outlet, table, &request.business_day.days_since_epoch().to_string(), Op::Upsert, request.at)?;
        }
        let archive = crate::archive::ArchiveRepo::new(self.tx);
        // The original file carries the linked return for a complete bill history. Today's
        // header must be rebuilt too: it now includes the signed sale/tax adjustment.
        archive.mark_dirty(outlet, original_day, request.at)?;
        archive.mark_dirty(outlet, request.business_day, request.at)?;
        Ok(true)
    }

    pub fn total_for_day(&self, outlet: &str, day: BusinessDay) -> Result<Money, DbError> {
        let amount = self.tx.query_row("SELECT COALESCE(SUM(grand_total), 0) FROM bill_returns WHERE outlet_id = ?1 AND business_day = ?2",
            rusqlite::params![outlet, day.days_since_epoch()], |r| r.get(0))?;
        Ok(Money::from_paise(amount))
    }
}
