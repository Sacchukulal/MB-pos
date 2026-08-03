//! Money that happens outside a bill: customers and their khata, expenses, and
//! the day close.
//!
//! # The column that is not here
//!
//! There is no customer balance column, and [`MoneyRepo::customer_balance`] is
//! a `SUM` every single time. v1 kept `credit_balance REAL` on the customers
//! table, beside the payments that make it — two sources of truth for what a
//! customer owes, one of them a floating-point number. A stored balance is a
//! balance that can disagree with its own ledger, and the day it does, nobody
//! can tell which one is right.

use mb_core::{BusinessDay, CustomerId, Money, StaffId, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Customer {
    pub id: CustomerId,
    pub name: String,
    pub phone: Option<String>,
    pub gstin: Option<String>,
    pub address: Option<String>,
    /// Scope 5.2. `None` means no limit, which is not a limit of zero.
    pub credit_limit: Option<Money>,
    pub is_active: bool,
}

/// A khata repayment. Audit A3: in v1 these were *never* sent to the cloud, so
/// the udhaar ledger could not be rebuilt from a backup at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KhataPayment {
    pub id: String,
    pub customer_id: CustomerId,
    pub amount: Money,
    /// The real mode (audit B12) — never the string "Full Settlement".
    pub mode: String,
    pub reference: Option<String>,
    pub received_at: Timestamp,
    pub received_by: Option<StaffId>,
    pub business_day: BusinessDay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expense {
    pub id: String,
    pub category_id: Option<String>,
    pub description: String,
    pub amount: Money,
    /// Whether it came out of the till, which decides the day close's expected
    /// cash.
    pub is_cash: bool,
    pub paid_at: Timestamp,
    pub paid_by: Option<StaffId>,
    pub business_day: BusinessDay,
}

/// Scope 10.8 and requirement 9 of the ten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayClose {
    pub id: String,
    pub business_day: BusinessDay,
    pub opening_float: Money,
    pub expected_cash: Money,
    pub counted_cash: Money,
    /// Stored, not derived, so the Z-report reprints identically years later
    /// even if a bill is voided afterwards.
    pub variance: Money,
    pub is_locked: bool,
    pub closed_at: Timestamp,
    pub closed_by: Option<StaffId>,
}

#[derive(Debug)]
pub struct MoneyRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> MoneyRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        MoneyRepo { tx }
    }

    // -- customers ----------------------------------------------------------

    pub fn save_customer(
        &self,
        outlet: &str,
        customer: &Customer,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO customers (id, outlet_id, name, phone, gstin, address, credit_limit,
                                    is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT (id) DO UPDATE SET name         = excluded.name,
                                            phone        = excluded.phone,
                                            gstin        = excluded.gstin,
                                            address      = excluded.address,
                                            credit_limit = excluded.credit_limit,
                                            is_active    = excluded.is_active,
                                            updated_at   = excluded.updated_at",
            rusqlite::params![
                customer.id.as_str(),
                outlet,
                customer.name,
                customer.phone,
                customer.gstin,
                customer.address,
                customer.credit_limit.map(encode::money_to_sql),
                encode::bool_to_sql(customer.is_active),
                encode::timestamp_to_sql(at),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "customers", customer.id.as_str(), Op::Upsert, at)
    }

    pub fn list_customers(&self, outlet: &str) -> Result<Vec<Customer>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, phone, gstin, address, credit_limit, is_active
               FROM customers WHERE outlet_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, phone, gstin, address, credit_limit, is_active) = row?;
            out.push(Customer {
                id: CustomerId::new(id),
                name,
                phone,
                gstin,
                address,
                credit_limit: credit_limit.map(encode::money_from_sql),
                is_active: encode::bool_from_sql(is_active, "customers.is_active")?,
            });
        }
        Ok(out)
    }

    /// What the customer owes, right now, from the ledger.
    ///
    /// Credit taken on bills, less repayments. **Computed, never stored** — see
    /// the module header for why v1's `credit_balance REAL` was the wrong
    /// shape twice over.
    pub fn customer_balance(&self, id: &CustomerId) -> Result<Money, DbError> {
        let taken: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(p.amount), 0)
               FROM payments p JOIN orders o ON o.id = p.order_id
              WHERE p.customer_id = ?1 AND p.mode = 'credit' AND o.state = 'settled'",
            [id.as_str()],
            |r| r.get(0),
        )?;
        let repaid: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM customer_payments WHERE customer_id = ?1",
            [id.as_str()],
            |r| r.get(0),
        )?;
        Ok(encode::money_from_sql(taken - repaid))
    }

    pub fn record_khata_payment(
        &self,
        outlet: &str,
        payment: &KhataPayment,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO customer_payments (id, outlet_id, customer_id, amount, mode, reference,
                                            received_at, received_by, business_day)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                payment.id,
                outlet,
                payment.customer_id.as_str(),
                encode::money_to_sql(payment.amount),
                payment.mode,
                payment.reference,
                encode::timestamp_to_sql(payment.received_at),
                payment.received_by.as_ref().map(StaffId::as_str),
                encode::business_day_to_sql(payment.business_day),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "customer_payments",
            &payment.id,
            Op::Upsert,
            payment.received_at,
        )
    }

    pub fn list_khata_payments(
        &self,
        customer: &CustomerId,
    ) -> Result<Vec<KhataPayment>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, amount, mode, reference, received_at, received_by, business_day
               FROM customer_payments WHERE customer_id = ?1 ORDER BY received_at",
        )?;
        let rows = stmt.query_map([customer.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, amount, mode, reference, received_at, received_by, day) = row?;
            out.push(KhataPayment {
                id,
                customer_id: customer.clone(),
                amount: encode::money_from_sql(amount),
                mode,
                reference,
                received_at: encode::timestamp_from_sql(received_at),
                received_by: received_by.map(StaffId::new),
                business_day: encode::business_day_from_sql(
                    day,
                    "customer_payments.business_day",
                )?,
            });
        }
        Ok(out)
    }

    // -- expenses -----------------------------------------------------------

    /// Audit A2: v1's counter never sent expenses, so the owner's phone showed
    /// ₹0 and an inflated net profit forever. Here it is the same table, the
    /// same outbox and the same treatment as a bill.
    pub fn save_expense(&self, outlet: &str, expense: &Expense) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO expenses (id, outlet_id, category_id, description, amount, is_cash,
                                   paid_at, paid_by, business_day)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (id) DO UPDATE SET category_id = excluded.category_id,
                                            description = excluded.description,
                                            amount      = excluded.amount,
                                            is_cash     = excluded.is_cash",
            rusqlite::params![
                expense.id,
                outlet,
                expense.category_id,
                expense.description,
                encode::money_to_sql(expense.amount),
                encode::bool_to_sql(expense.is_cash),
                encode::timestamp_to_sql(expense.paid_at),
                expense.paid_by.as_ref().map(StaffId::as_str),
                encode::business_day_to_sql(expense.business_day),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "expenses",
            &expense.id,
            Op::Upsert,
            expense.paid_at,
        )
    }

    pub fn list_expenses(&self, outlet: &str, day: BusinessDay) -> Result<Vec<Expense>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, category_id, description, amount, is_cash, paid_at, paid_by, business_day
               FROM expenses WHERE outlet_id = ?1 AND business_day = ?2 ORDER BY paid_at",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![outlet, encode::business_day_to_sql(day)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (id, category_id, description, amount, is_cash, paid_at, paid_by, day) = row?;
            out.push(Expense {
                id,
                category_id,
                description,
                amount: encode::money_from_sql(amount),
                is_cash: encode::bool_from_sql(is_cash, "expenses.is_cash")?,
                paid_at: encode::timestamp_from_sql(paid_at),
                paid_by: paid_by.map(StaffId::new),
                business_day: encode::business_day_from_sql(day, "expenses.business_day")?,
            });
        }
        Ok(out)
    }

    // -- the day close ------------------------------------------------------

    /// Expected cash in the drawer: the opening float, plus cash taken on
    /// bills, less cash expenses. What the Z-report compares against a count.
    pub fn expected_cash(
        &self,
        outlet: &str,
        day: BusinessDay,
        opening_float: Money,
    ) -> Result<Money, DbError> {
        let day_sql = encode::business_day_to_sql(day);
        let taken: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(p.amount + p.tip), 0)
               FROM payments p JOIN orders o ON o.id = p.order_id
              WHERE o.outlet_id = ?1 AND p.business_day = ?2 AND p.mode = 'cash'
                AND o.state = 'settled'",
            rusqlite::params![outlet, day_sql],
            |r| r.get(0),
        )?;
        let paid_out: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM expenses
              WHERE outlet_id = ?1 AND business_day = ?2 AND is_cash = 1",
            rusqlite::params![outlet, day_sql],
            |r| r.get(0),
        )?;
        Ok(encode::money_from_sql(
            opening_float.paise() + taken - paid_out,
        ))
    }

    pub fn save_day_close(&self, outlet: &str, close: &DayClose) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO day_closes (id, outlet_id, business_day, opening_float, expected_cash,
                                     counted_cash, variance, is_locked, closed_at, closed_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (outlet_id, business_day)
             DO UPDATE SET counted_cash = excluded.counted_cash,
                           variance     = excluded.variance,
                           is_locked    = excluded.is_locked",
            rusqlite::params![
                close.id,
                outlet,
                encode::business_day_to_sql(close.business_day),
                encode::money_to_sql(close.opening_float),
                encode::money_to_sql(close.expected_cash),
                encode::money_to_sql(close.counted_cash),
                encode::money_to_sql(close.variance),
                encode::bool_to_sql(close.is_locked),
                encode::timestamp_to_sql(close.closed_at),
                close.closed_by.as_ref().map(StaffId::as_str),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "day_closes",
            &close.id,
            Op::Upsert,
            close.closed_at,
        )
    }

    pub fn find_day_close(
        &self,
        outlet: &str,
        day: BusinessDay,
    ) -> Result<Option<DayClose>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, business_day, opening_float, expected_cash, counted_cash, variance,
                    is_locked, closed_at, closed_by
               FROM day_closes WHERE outlet_id = ?1 AND business_day = ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![
            outlet,
            encode::business_day_to_sql(day)
        ])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(DayClose {
            id: row.get(0)?,
            business_day: encode::business_day_from_sql(row.get(1)?, "day_closes.business_day")?,
            opening_float: encode::money_from_sql(row.get(2)?),
            expected_cash: encode::money_from_sql(row.get(3)?),
            counted_cash: encode::money_from_sql(row.get(4)?),
            variance: encode::money_from_sql(row.get(5)?),
            is_locked: encode::bool_from_sql(row.get(6)?, "day_closes.is_locked")?,
            closed_at: encode::timestamp_from_sql(row.get(7)?),
            closed_by: row.get::<_, Option<String>>(8)?.map(StaffId::new),
        }))
    }
}
