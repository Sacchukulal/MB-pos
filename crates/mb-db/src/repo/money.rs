//! Money that happens outside a bill: customers and their credit, expenses, and
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

use mb_core::credit::{Ageing, Movement, MovementKind};
use mb_core::expense::Every;
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

/// A credit repayment. Audit A3: in v1 these were *never* sent to the cloud, so
/// the udhaar ledger could not be rebuilt from a backup at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditPayment {
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


/// An opening balance, a write-off, or a correction — scope 5.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditAdjustment {
    pub id: String,
    pub customer_id: CustomerId,
    /// Always positive. `increases` is the direction.
    pub amount: Money,
    pub increases: bool,
    pub reason: String,
    pub at: Timestamp,
    pub business_day: BusinessDay,
    pub made_by: Option<StaffId>,
}

/// One line of "who owes me money".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owing {
    pub customer: Customer,
    pub balance: Money,
    pub ageing: Ageing,
    pub last_movement: BusinessDay,
}



/// A category, and it is DATA (audit B14): v1's six were in the source, so a
/// shop that spent money on anything else recorded it as "Other" forever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpenseCategory {
    pub id: String,
    pub name: String,
    pub sort_order: i64,
    pub is_active: bool,
}

/// What moved in or out of the DRAWER that is not a sale and not an expense.
///
/// A cash expense deliberately has no row here — see the schema comment on
/// `cash_movements`. Two rows for one fact can disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashMovement {
    pub id: String,
    /// `float`, `top_up`, `payout` or `bank_drop`. The direction belongs to
    /// the kind; the amount is always positive.
    pub kind: String,
    pub amount: Money,
    pub reason: String,
    pub at: Timestamp,
    pub business_day: BusinessDay,
    pub moved_by: Option<StaffId>,
}

/// Rent, salary, the internet bill. **A template and a reminder — never an
/// automatic posting.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurring {
    pub id: String,
    pub category_id: Option<String>,
    pub description: String,
    pub amount: Money,
    pub mode: String,
    pub paid_to: Option<String>,
    pub every: Every,
    pub next_due: BusinessDay,
    pub is_active: bool,
}

/// The number a drawer is counted against — scope 10.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CashPosition {
    pub opening_float: Money,
    pub cash_sales: Money,
    pub top_ups: Money,
    pub cash_expenses: Money,
    pub payouts: Money,
    pub bank_drops: Money,
    /// **P26, D120.** Cash handed to suppliers at the door. Before this term
    /// existed the day close told a shop to expect money it had already paid the
    /// vegetable man.
    pub suppliers_paid: Money,
    /// **P29, scope 8.5 — how much of `cash_sales` is somebody's tip.**
    ///
    /// Not subtracted from anything: a cash tip really is in the drawer. It is
    /// split out so the day close can show it on its own line, because a
    /// takings figure that silently includes tips is a figure that will never
    /// agree with the sales report — and the gap between them is exactly the
    /// money that belongs to the staff.
    pub cash_tips: Money,
    /// **P29, scope 14.5 — cash a rider is carrying, and it is not here yet.**
    ///
    /// A delivery paid in cash is settled when the rider hands the food over,
    /// so the sale is real and the money is in somebody's pocket three
    /// kilometres away. Every other cash payment in this product is in the
    /// drawer the moment it is taken; these are not.
    ///
    /// Counting them is a drawer that is short all evening for a reason nobody
    /// can name — and the shortfall walks back in at nine o'clock, which makes
    /// it look like a theft that resolved itself.
    pub with_riders: Money,
    /// float + sales + top-ups − expenses − payouts − drops − suppliers paid
    /// − what riders are still carrying.
    pub expected: Money,
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expense {
    pub id: String,
    pub category_id: Option<String>,
    pub description: String,
    pub amount: Money,
    /// `cash`, `bank`, `upi` or `card`. **Cash decides the day close's
    /// expected drawer**; the other three are only telling a shop which
    /// statement to reconcile against, which is why one boolean was not
    /// enough (P16).
    pub mode: String,
    /// A NAME. Suppliers and their ledger are P26.
    pub paid_to: Option<String>,
    pub reference: Option<String>,
    /// Input credit (scope 2.9): the rate, and the tax inside what was paid.
    /// Both or neither — the schema says so too.
    pub gst_rate_bp: Option<i64>,
    pub gst_amount: Option<Money>,
    pub paid_at: Timestamp,
    pub paid_by: Option<StaffId>,
    pub business_day: BusinessDay,
    pub note: Option<String>,
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
    /// Why the drawer was out, when it was out by more than the shop's
    /// threshold. **The reason is the feature**: "₹340 short" tells an owner
    /// nothing, and "₹340 short — paid the vegetable man from the drawer" tells
    /// them everything. Audit B15.
    pub note: Option<String>,
    /// **D140 — which drawer this is.** `None` is the shop's roll-up, written
    /// when the last till closes, and it is the row that locks the day. A cash
    /// drawer is a box under one till, and "we were ₹340 short" is
    /// unanswerable until you know which box.
    pub terminal: Option<String>,
    /// Scope 9.8's boundary half: one till can count its drawer several times
    /// in a day. 0 on the shop's row.
    pub shift_no: i64,
}

/// How many of each note and coin were counted.
///
/// A child table rather than a JSON column, because "we are always short of
/// tens" is a report an owner really asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Denomination {
    /// Paise, so a ₹500 note is 50000.
    pub value: Money,
    pub count: u32,
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
            "INSERT INTO customers (id, outlet_id, name, phone, phone_key, gstin, address,
                                    credit_limit, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?10, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT (id) DO UPDATE SET name         = excluded.name,
                                            phone        = excluded.phone,
                                            phone_key    = excluded.phone_key,
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
                // The derived copy, with ONE writer — this line (D56's rule
                // again). The UNIQUE INDEX on it is what makes "one phone is
                // one customer" structural rather than remembered.
                customer.phone.as_deref().and_then(mb_core::credit::phone_key),
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


    /// Who owns this phone number, if anybody — scope 5.4.
    ///
    /// **The lookup a duplicate must go through.** "That number is already
    /// Rekha's — open her?" is the only acceptable answer to a second row for
    /// one number, and it needs the existing customer, not a boolean.
    pub fn customer_by_phone(
        &self,
        outlet: &str,
        phone: &str,
    ) -> Result<Option<Customer>, DbError> {
        let Some(key) = mb_core::credit::phone_key(phone) else {
            return Ok(None);
        };
        let found = self
            .list_customers(outlet)?
            .into_iter()
            .find(|c| c.phone.as_deref().and_then(mb_core::credit::phone_key) == Some(key.clone()));
        Ok(found)
    }

    /// Everything that has ever moved this account — scope 5.1.
    ///
    /// Three sources, one shape:
    ///
    /// * **sales** are `payments` rows in credit mode on a SETTLED order. A
    ///   voided bill drops out here, which is why voiding one returns the
    ///   balance exactly to what it was without any reversing row: the sale
    ///   simply stops being a settled sale (D47 — a correction is a state).
    /// * **repayments** are `customer_payments`.
    /// * **adjustments** are `credit_adjustments`, with their reason.
    ///
    /// Ordered oldest first, because that is the order the ageing applies them
    /// in and a statement prints them in.
    pub fn credit_movements(&self, customer: &CustomerId) -> Result<Vec<Movement>, DbError> {
        let mut out = Vec::new();

        let mut sales = self.tx.prepare_cached(
            "SELECT o.business_day, p.amount, COALESCE(o.bill_number_formatted, o.id)
               FROM payments p JOIN orders o ON o.id = p.order_id
              WHERE p.customer_id = ?1 AND p.mode = 'credit' AND o.state = 'settled'",
        )?;
        for row in sales.query_map([customer.as_str()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
        })? {
            let (day, amount, note) = row?;
            out.push(Movement {
                day: encode::business_day_from_sql(day, "orders.business_day")?,
                kind: MovementKind::Sale,
                amount: encode::money_from_sql(amount),
                note,
            });
        }

        let mut repayments = self.tx.prepare_cached(
            "SELECT business_day, amount, COALESCE(reference, mode)
               FROM customer_payments WHERE customer_id = ?1",
        )?;
        for row in repayments.query_map([customer.as_str()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
        })? {
            let (day, amount, note) = row?;
            out.push(Movement {
                day: encode::business_day_from_sql(day, "customer_payments.business_day")?,
                kind: MovementKind::Repayment,
                amount: encode::money_from_sql(amount),
                note,
            });
        }

        let mut adjustments = self.tx.prepare_cached(
            "SELECT business_day, amount, increases, reason
               FROM credit_adjustments WHERE customer_id = ?1",
        )?;
        for row in adjustments.query_map([customer.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })? {
            let (day, amount, increases, reason) = row?;
            out.push(Movement {
                day: encode::business_day_from_sql(day, "credit_adjustments.business_day")?,
                kind: MovementKind::Adjustment {
                    increases: encode::bool_from_sql(increases, "credit_adjustments.increases")?,
                },
                amount: encode::money_from_sql(amount),
                note: reason,
            });
        }

        out.sort_by_key(|m| m.day.days_since_epoch());
        Ok(out)
    }

    /// An opening balance, a write-off, or a correction.
    ///
    /// The reason is required by the schema as well as by the type, because
    /// this is the one door in the credit account somebody could use to make
    /// money disappear.
    pub fn save_credit_adjustment(
        &self,
        outlet: &str,
        adjustment: &CreditAdjustment,
    ) -> Result<(), DbError> {
        if adjustment.reason.trim().is_empty() {
            return Err(DbError::invariant("an adjustment needs a reason"));
        }
        if !adjustment.amount.is_positive() {
            return Err(DbError::invariant(
                "an adjustment is a positive amount and a direction, never a negative amount",
            ));
        }
        self.tx.execute(
            "INSERT INTO credit_adjustments (id, outlet_id, customer_id, amount, increases,
                                             reason, at, business_day, made_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                adjustment.id,
                outlet,
                adjustment.customer_id.as_str(),
                encode::money_to_sql(adjustment.amount),
                encode::bool_to_sql(adjustment.increases),
                adjustment.reason.trim(),
                encode::timestamp_to_sql(adjustment.at),
                encode::business_day_to_sql(adjustment.business_day),
                adjustment.made_by.as_ref().map(StaffId::as_str),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "credit_adjustments",
            &adjustment.id,
            Op::Upsert,
            adjustment.at,
        )
    }

    /// **Who owes me money** — the screen an owner actually opens.
    ///
    /// One pass over every customer with a movement, so the list and the
    /// customer screen cannot disagree: both are built from
    /// [`MoneyRepo::credit_movements`].
    pub fn who_owes(&self, outlet: &str, today: BusinessDay) -> Result<Vec<Owing>, DbError> {
        let mut out = Vec::new();
        for customer in self.list_customers(outlet)? {
            let movements = self.credit_movements(&customer.id)?;
            if movements.is_empty() {
                continue;
            }
            let balance = mb_core::credit::balance(&movements)
                .map_err(|e| DbError::invariant(e.to_string()))?;
            let ageing = mb_core::credit::ageing(&movements, today)
                .map_err(|e| DbError::invariant(e.to_string()))?;
            let last = movements
                .last()
                .map_or(today, |m| m.day);
            out.push(Owing { customer, balance, ageing, last_movement: last });
        }
        // Oldest debt first: the point of the screen is what has been owed
        // longest, not who is alphabetically first.
        out.sort_by(|a, b| {
            b.ageing
                .oldest_days
                .unwrap_or(-1)
                .cmp(&a.ageing.oldest_days.unwrap_or(-1))
                .then(b.balance.paise().cmp(&a.balance.paise()))
        });
        Ok(out)
    }

    pub fn record_credit_payment(
        &self,
        outlet: &str,
        payment: &CreditPayment,
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

    pub fn list_credit_payments(
        &self,
        customer: &CustomerId,
    ) -> Result<Vec<CreditPayment>, DbError> {
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
            out.push(CreditPayment {
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
    /// Save or edit an expense — scope 10.6, audit B15 (v1 could do neither).
    pub fn save_expense(&self, outlet: &str, expense: &Expense) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO expenses (id, outlet_id, category_id, description, amount, mode,
                                   paid_to, reference, gst_rate_bp, gst_amount,
                                   paid_at, paid_by, business_day, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (id) DO UPDATE SET category_id  = excluded.category_id,
                                            description  = excluded.description,
                                            amount       = excluded.amount,
                                            mode         = excluded.mode,
                                            paid_to      = excluded.paid_to,
                                            reference    = excluded.reference,
                                            gst_rate_bp  = excluded.gst_rate_bp,
                                            gst_amount   = excluded.gst_amount,
                                            business_day = excluded.business_day,
                                            note         = excluded.note",
            rusqlite::params![
                expense.id,
                outlet,
                expense.category_id,
                expense.description,
                encode::money_to_sql(expense.amount),
                expense.mode,
                expense.paid_to,
                expense.reference,
                expense.gst_rate_bp,
                expense.gst_amount.map(encode::money_to_sql),
                encode::timestamp_to_sql(expense.paid_at),
                expense.paid_by.as_ref().map(StaffId::as_str),
                encode::business_day_to_sql(expense.business_day),
                expense.note,
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
        self.read_expenses(
            "SELECT id, category_id, description, amount, mode, paid_to, reference,
                    gst_rate_bp, gst_amount, paid_at, paid_by, business_day, note
               FROM expenses WHERE outlet_id = ?1 AND business_day = ?2 ORDER BY paid_at",
            rusqlite::params![outlet, encode::business_day_to_sql(day)],
        )
    }

    /// Everything between two days — what the month-against-month view reads.
    pub fn expenses_between(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<Expense>, DbError> {
        self.read_expenses(
            "SELECT id, category_id, description, amount, mode, paid_to, reference,
                    gst_rate_bp, gst_amount, paid_at, paid_by, business_day, note
               FROM expenses
              WHERE outlet_id = ?1 AND business_day BETWEEN ?2 AND ?3
              ORDER BY business_day, paid_at",
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to),
            ],
        )
    }

    fn read_expenses(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<Expense>, DbError> {
        let mut stmt = self.tx.prepare_cached(sql)?;
        let rows = stmt.query_map(params, |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, i64>(11)?,
                row.get::<_, Option<String>>(12)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (
                id,
                category_id,
                description,
                amount,
                mode,
                paid_to,
                reference,
                gst_rate_bp,
                gst_amount,
                paid_at,
                paid_by,
                day,
                note,
            ) = row?;
            out.push(Expense {
                id,
                category_id,
                description,
                amount: encode::money_from_sql(amount),
                mode,
                paid_to,
                reference,
                gst_rate_bp,
                gst_amount: gst_amount.map(encode::money_from_sql),
                paid_at: encode::timestamp_from_sql(paid_at),
                paid_by: paid_by.map(StaffId::new),
                business_day: encode::business_day_from_sql(day, "expenses.business_day")?,
                note,
            });
        }
        Ok(out)
    }

    /// **An expense really is deleted** — and it is the only money row in this
    /// product that is.
    ///
    /// A bill is voided rather than deleted because a customer has a copy of
    /// it and the number must never be reused. An expense has neither
    /// property: a ₹40 milk purchase typed twice is a typing mistake, not a
    /// transaction, and leaving it as a "voided expense" would mean a shop's
    /// expense list is full of things that did not happen. The audit row (R11)
    /// is what makes it accountable, and it carries the whole row as `before`.
    pub fn delete_expense(&self, outlet: &str, id: &str, at: Timestamp) -> Result<(), DbError> {
        let gone = self
            .tx
            .execute("DELETE FROM expenses WHERE outlet_id = ?1 AND id = ?2", rusqlite::params![outlet, id])?;
        if gone == 0 {
            return Err(DbError::invariant("that expense is not here any more"));
        }
        OutboxRepo::new(self.tx).enqueue(outlet, "expenses", id, Op::Delete, at)
    }

    // -- categories, which are DATA (audit B14) ------------------------------

    pub fn save_expense_category(
        &self,
        outlet: &str,
        category: &ExpenseCategory,
        at: Timestamp,
    ) -> Result<(), DbError> {
        if category.name.trim().is_empty() {
            return Err(DbError::invariant("a category needs a name"));
        }
        self.tx.execute(
            "INSERT INTO expense_categories (id, outlet_id, name, sort_order, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET name       = excluded.name,
                                            sort_order = excluded.sort_order,
                                            is_active  = excluded.is_active",
            rusqlite::params![
                category.id,
                outlet,
                category.name.trim(),
                category.sort_order,
                encode::bool_to_sql(category.is_active),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "expense_categories", &category.id, Op::Upsert, at)
    }

    pub fn list_expense_categories(&self, outlet: &str) -> Result<Vec<ExpenseCategory>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, sort_order, is_active FROM expense_categories
              WHERE outlet_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, sort_order, is_active) = row?;
            out.push(ExpenseCategory {
                id,
                name,
                sort_order,
                is_active: encode::bool_from_sql(is_active, "expense_categories.is_active")?,
            });
        }
        Ok(out)
    }

    /// A category with money against it cannot be deleted — the same rule and
    /// the same sentence shape as a table with bills against it (P14).
    pub fn delete_expense_category(
        &self,
        outlet: &str,
        id: &str,
        at: Timestamp,
    ) -> Result<(), DbError> {
        let used: i64 = self.tx.query_row(
            "SELECT count(*) FROM expenses WHERE category_id = ?1",
            [id],
            |r| r.get(0),
        )?;
        if used > 0 {
            return Err(DbError::invariant(format!(
                "{used} expense(s) are in this category. Hide it instead — that takes it off \
                 the list and keeps the history"
            )));
        }
        self.tx.execute(
            "DELETE FROM expense_categories WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, id],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "expense_categories", id, Op::Delete, at)
    }

    // -- the drawer ----------------------------------------------------------

    pub fn save_cash_movement(
        &self,
        outlet: &str,
        movement: &CashMovement,
    ) -> Result<(), DbError> {
        if movement.reason.trim().is_empty() {
            return Err(DbError::invariant("a cash movement needs a reason"));
        }
        if !movement.amount.is_positive() {
            return Err(DbError::invariant(
                "a cash movement is a positive amount and a kind, never a negative amount",
            ));
        }
        self.tx.execute(
            "INSERT INTO cash_movements (id, outlet_id, kind, amount, reason, at, business_day,
                                         moved_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET kind   = excluded.kind,
                                            amount = excluded.amount,
                                            reason = excluded.reason",
            rusqlite::params![
                movement.id,
                outlet,
                movement.kind,
                encode::money_to_sql(movement.amount),
                movement.reason.trim(),
                encode::timestamp_to_sql(movement.at),
                encode::business_day_to_sql(movement.business_day),
                movement.moved_by.as_ref().map(StaffId::as_str),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "cash_movements", &movement.id, Op::Upsert, movement.at)
    }

    pub fn list_cash_movements(
        &self,
        outlet: &str,
        day: BusinessDay,
    ) -> Result<Vec<CashMovement>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, kind, amount, reason, at, business_day, moved_by FROM cash_movements
              WHERE outlet_id = ?1 AND business_day = ?2 ORDER BY at",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![outlet, encode::business_day_to_sql(day)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (id, kind, amount, reason, at, day, moved_by) = row?;
            out.push(CashMovement {
                id,
                kind,
                amount: encode::money_from_sql(amount),
                reason,
                at: encode::timestamp_from_sql(at),
                business_day: encode::business_day_from_sql(day, "cash_movements.business_day")?,
                moved_by: moved_by.map(StaffId::new),
            });
        }
        Ok(out)
    }

    pub fn delete_cash_movement(
        &self,
        outlet: &str,
        id: &str,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "DELETE FROM cash_movements WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, id],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "cash_movements", id, Op::Delete, at)
    }

    // -- recurring templates -------------------------------------------------

    pub fn save_recurring(&self, outlet: &str, template: &Recurring, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO recurring_expenses (id, outlet_id, category_id, description, amount,
                                             mode, paid_to, every, next_due, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (id) DO UPDATE SET category_id = excluded.category_id,
                                            description = excluded.description,
                                            amount      = excluded.amount,
                                            mode        = excluded.mode,
                                            paid_to     = excluded.paid_to,
                                            every       = excluded.every,
                                            next_due    = excluded.next_due,
                                            is_active   = excluded.is_active",
            rusqlite::params![
                template.id,
                outlet,
                template.category_id,
                template.description,
                encode::money_to_sql(template.amount),
                template.mode,
                template.paid_to,
                match template.every {
                    Every::Week => "week",
                    Every::Month => "month",
                },
                encode::business_day_to_sql(template.next_due),
                encode::bool_to_sql(template.is_active),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "recurring_expenses", &template.id, Op::Upsert, at)
    }

    pub fn list_recurring(&self, outlet: &str) -> Result<Vec<Recurring>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, category_id, description, amount, mode, paid_to, every, next_due,
                    is_active
               FROM recurring_expenses WHERE outlet_id = ?1 ORDER BY next_due",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, category_id, description, amount, mode, paid_to, every, next_due, is_active) =
                row?;
            out.push(Recurring {
                id,
                category_id,
                description,
                amount: encode::money_from_sql(amount),
                mode,
                paid_to,
                every: if every == "week" { Every::Week } else { Every::Month },
                next_due: encode::business_day_from_sql(next_due, "recurring_expenses.next_due")?,
                is_active: encode::bool_from_sql(is_active, "recurring_expenses.is_active")?,
            });
        }
        Ok(out)
    }

    /// **The cash position** — scope 10.6, and the number a drawer is counted
    /// against.
    ///
    /// `float + cash taken on bills + top-ups − cash expenses − payouts − bank
    /// drops`, computed in one place because P18's day close reads this
    /// function rather than writing a second one that can disagree with it.
    ///
    /// **A cash expense is not double-counted**, and cannot be: it is an
    /// expense row and nothing else. The movements table deliberately holds no
    /// row for it (see the schema comment).
    /// **The whole shop's drawer.** Every till's box added together.
    pub fn cash_position(&self, outlet: &str, day: BusinessDay) -> Result<CashPosition, DbError> {
        self.cash_position_of(outlet, day, None)
    }

    /// **One till's drawer** — D140, and the reason it exists: a cash drawer is
    /// a box under one till, and "we were ₹340 short" is unanswerable until you
    /// know which box.
    ///
    /// **The per-till figures sum EXACTLY to the shop's**, and that is a
    /// property of this one function rather than of two that could drift: rows
    /// written before this shop had a second till carry no terminal, and every
    /// term below attributes those to the master. `None` here means the shop,
    /// and it is the same SQL with the filter removed.
    #[allow(
        clippy::too_many_lines,
        reason = "one query per term of one sum; splitting them would put the \
                  filter that makes the totals tie in six places"
    )]
    pub fn cash_position_of(
        &self,
        outlet: &str,
        day: BusinessDay,
        terminal: Option<&str>,
    ) -> Result<CashPosition, DbError> {
        let day_sql = encode::business_day_to_sql(day);
        // **The master owns every row written before this shop had a second
        // till**, which is what makes the per-drawer figures add up to the
        // shop's exactly rather than nearly.
        let master: Option<String> = self
            .tx
            .query_row(
                "SELECT id FROM terminals WHERE outlet_id = ?1 AND is_master = 1",
                [outlet],
                |row| row.get(0),
            )
            .ok();
        let master = master.as_deref();
        // `?3` is the till being asked about (NULL = the whole shop) and `?4`
        // is the master, which owns every row written before this shop had a
        // second till.
        let taken: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(p.amount + p.tip), 0)
               FROM payments p JOIN orders o ON o.id = p.order_id
              WHERE o.outlet_id = ?1 AND p.business_day = ?2 AND p.mode = 'cash'
                AND o.state = 'settled'
                AND (?3 IS NULL OR COALESCE(o.terminal_id, ?4) = ?3)",
            rusqlite::params![outlet, day_sql, terminal, master],
            |r| r.get(0),
        )?;
        // **P29, scope 8.5. How much of that was a tip.**
        //
        // Cash tips ARE in the drawer, so they are not subtracted from
        // anything — the same query, split out, so the day close can show them
        // on their own line. A drawer whose 'cash from bills' figure quietly
        // includes tips is a drawer that never quite matches the sales report,
        // and the difference is exactly the money that belongs to the staff.
        let tips: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(p.tip), 0)
               FROM payments p JOIN orders o ON o.id = p.order_id
              WHERE o.outlet_id = ?1 AND p.business_day = ?2 AND p.mode = 'cash'
                AND o.state = 'settled'
                AND (?3 IS NULL OR COALESCE(o.terminal_id, ?4) = ?3)",
            rusqlite::params![outlet, day_sql, terminal, master],
            |r| r.get(0),
        )?;
        let spent: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM expenses
              WHERE outlet_id = ?1 AND business_day = ?2 AND mode = 'cash'
                AND (?3 IS NULL OR COALESCE(terminal_id, ?4) = ?3)",
            rusqlite::params![outlet, day_sql, terminal, master],
            |r| r.get(0),
        )?;
        let moved = |kind: &str| -> Result<i64, DbError> {
            Ok(self.tx.query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM cash_movements
                  WHERE outlet_id = ?1 AND business_day = ?2 AND kind = ?5
                    AND (?3 IS NULL OR COALESCE(terminal_id, ?4) = ?3)",
                rusqlite::params![outlet, day_sql, terminal, master, kind],
                |r| r.get(0),
            )?)
        };
        let float = moved("float")?;
        let top_ups = moved("top_up")?;
        let payouts = moved("payout")?;
        let drops = moved("bank_drop")?;
        let paid_out: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM supplier_payments
              WHERE outlet_id = ?1 AND business_day = ?2 AND mode = 'cash'
                AND (?3 IS NULL OR COALESCE(terminal_id, ?4) = ?3)",
            rusqlite::params![outlet, day_sql, terminal, master],
            |r| r.get(0),
        )?;

        // **P29 — what the riders are still carrying.**
        //
        // Cash taken on delivery orders that are with a rider, minus what has
        // been handed back. Both halves are sums over rows; nothing is stored,
        // so this cannot drift away from the handbacks that made it (D120).
        //
        // **On the road counts, not just arrived.** Plenty of shops settle a
        // COD bill the moment they type it — the cash is with the rider from
        // the second the bike leaves, not from the second the food changes
        // hands — so an order that is `out` counts exactly like a `delivered`
        // one. A `failed` one never counts: nobody paid.
        //
        // A rider is required, so a shop that does not use this feature
        // assigns nobody, subtracts nothing, and its drawer is what it always
        // was.
        let collected: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(p.amount + p.tip), 0)
               FROM payments p JOIN orders o ON o.id = p.order_id
              WHERE o.outlet_id = ?1 AND p.business_day = ?2 AND p.mode = 'cash'
                AND o.state = 'settled'
                AND o.order_type = 'delivery'
                AND o.delivery_state IN ('out', 'delivered')
                AND o.delivery_rider IS NOT NULL
                AND (?3 IS NULL OR COALESCE(o.terminal_id, ?4) = ?3)",
            rusqlite::params![outlet, day_sql, terminal, master],
            |r| r.get(0),
        )?;
        let handed_back: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM rider_handbacks
              WHERE outlet_id = ?1 AND business_day = ?2",
            rusqlite::params![outlet, day_sql],
            |r| r.get(0),
        )?;
        // Never negative: a rider who hands back more than they collected has
        // made an arithmetic mistake somebody must look at, and it must not
        // quietly ADD to the expected drawer.
        let with_riders = (collected - handed_back).max(0);

        Ok(CashPosition {
            opening_float: encode::money_from_sql(float),
            cash_sales: encode::money_from_sql(taken),
            top_ups: encode::money_from_sql(top_ups),
            cash_expenses: encode::money_from_sql(spent),
            payouts: encode::money_from_sql(payouts),
            bank_drops: encode::money_from_sql(drops),
            suppliers_paid: encode::money_from_sql(paid_out),
            cash_tips: encode::money_from_sql(tips),
            with_riders: encode::money_from_sql(with_riders),
            expected: encode::money_from_sql(
                float + taken + top_ups - spent - payouts - drops - paid_out - with_riders,
            ),
        })
    }

    // -- the day close ------------------------------------------------------
    /// **Superseded by [`MoneyRepo::cash_position`], and kept as one line of
    /// delegation rather than a second answer.**
    ///
    /// P05 wrote this before cash movements existed, so it took the opening
    /// float as an argument and knew nothing about payouts or bank drops. P16
    /// gave the drawer its own table; a shop that pays out ₹300 and drops
    /// ₹1,000 at the bank would have been told to expect ₹1,300 it does not
    /// have.
    ///
    /// The float argument is now only used when the day has no float movement
    /// recorded — which is a shop that has not opened its drawer through the
    /// product yet.
    pub fn expected_cash(
        &self,
        outlet: &str,
        day: BusinessDay,
        opening_float: Money,
    ) -> Result<Money, DbError> {
        let position = self.cash_position(outlet, day)?;
        if position.opening_float.is_zero() {
            return position
                .expected
                .add(opening_float)
                .map_err(|e| DbError::invariant(e.to_string()));
        }
        Ok(position.expected)
    }

    /// **D140 — a close is one drawer, or it is the shop's roll-up.**
    ///
    /// `close.terminal` says which: `Some` is a till's own counted box for one
    /// shift, `None` is the shop's total, written when the last till closes and
    /// **the row that locks the day** (D77).
    ///
    /// Two conflict targets rather than one, because the shop row's terminal is
    /// NULL and SQLite treats NULLs as distinct — the same reason P25's recipes
    /// need three partial indexes.
    pub fn save_day_close(&self, outlet: &str, close: &DayClose) -> Result<(), DbError> {
        let conflict = if close.terminal.is_some() {
            "(outlet_id, business_day, terminal_id, shift_no) WHERE terminal_id IS NOT NULL"
        } else {
            "(outlet_id, business_day) WHERE terminal_id IS NULL"
        };
        self.tx.execute(
            &format!(
                "INSERT INTO day_closes (id, outlet_id, business_day, terminal_id, shift_no,
                                     opening_float, expected_cash,
                                     counted_cash, variance, is_locked, closed_at, closed_by,
                                     note)
             VALUES (?1, ?2, ?3, ?12, ?13, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT {conflict}
             DO UPDATE SET counted_cash = excluded.counted_cash,
                           variance     = excluded.variance,
                           is_locked    = excluded.is_locked,
                           closed_at    = excluded.closed_at,
                           closed_by    = excluded.closed_by,
                           note         = excluded.note"
            ),
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
                close.note,
                close.terminal,
                close.shift_no,
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

    /// Every drawer close for a day, in the order they were counted.
    ///
    /// **The shop's total is the SUM of these and never an independent query**
    /// — two ways to compute one number is two numbers.
    pub fn drawer_closes(
        &self,
        outlet: &str,
        day: BusinessDay,
    ) -> Result<Vec<DayClose>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT id, business_day, opening_float, expected_cash, counted_cash, variance,
                    is_locked, closed_at, closed_by, note, terminal_id, shift_no
               FROM day_closes
              WHERE outlet_id = ?1 AND business_day = ?2 AND terminal_id IS NOT NULL
              ORDER BY closed_at",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![outlet, encode::business_day_to_sql(day)],
            read_day_close,
        )?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// Which tills have not counted their drawer for this day yet.
    ///
    /// **The shop's day cannot close while this is not empty** (D140), and the
    /// Z-report names them rather than quietly totalling what it has.
    pub fn tills_still_open(
        &self,
        outlet: &str,
        day: BusinessDay,
    ) -> Result<Vec<String>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT t.name FROM terminals t
              WHERE t.outlet_id = ?1
                AND NOT EXISTS (SELECT 1 FROM day_closes d
                                 WHERE d.outlet_id = ?1 AND d.business_day = ?2
                                   AND d.terminal_id = t.id)
              ORDER BY t.created_at",
        )?;
        let mut cursor = stmt.query(rusqlite::params![
            outlet,
            encode::business_day_to_sql(day)
        ])?;
        let mut out = Vec::new();
        while let Some(row) = cursor.next()? {
            out.push(row.get::<_, String>(0)?);
        }
        Ok(out)
    }

    /// The next shift number for a till on a day — 1 on the first close.
    ///
    /// Scope 9.8's boundary half: one till can count its drawer several times
    /// in a day, and the till's day total is the sum of its shifts.
    pub fn next_shift(
        &self,
        outlet: &str,
        day: BusinessDay,
        terminal: &str,
    ) -> Result<i64, DbError> {
        let n: i64 = self.tx.query_row(
            "SELECT COALESCE(MAX(shift_no), 0) + 1 FROM day_closes
              WHERE outlet_id = ?1 AND business_day = ?2 AND terminal_id = ?3",
            rusqlite::params![outlet, encode::business_day_to_sql(day), terminal],
            |row| row.get(0),
        )?;
        Ok(n)
    }

    /// **The SHOP's close for a day** — the roll-up row, which is the one that
    /// locks the day (D77, D140). A till's own drawer close is
    /// [`MoneyRepo::drawer_closes`].
    pub fn find_day_close(
        &self,
        outlet: &str,
        day: BusinessDay,
    ) -> Result<Option<DayClose>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, business_day, opening_float, expected_cash, counted_cash, variance,
                    is_locked, closed_at, closed_by, note, terminal_id, shift_no
               FROM day_closes
              WHERE outlet_id = ?1 AND business_day = ?2 AND terminal_id IS NULL",
        )?;
        let mut rows = stmt.query_map(
            rusqlite::params![outlet, encode::business_day_to_sql(day)],
            read_day_close,
        )?;
        rows.next().transpose().map_err(DbError::from)
    }

    /// One till's own close for a day and shift.
    pub fn find_drawer_close(
        &self,
        outlet: &str,
        day: BusinessDay,
        terminal: &str,
        shift_no: i64,
    ) -> Result<Option<DayClose>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT id, business_day, opening_float, expected_cash, counted_cash, variance,
                    is_locked, closed_at, closed_by, note, terminal_id, shift_no
               FROM day_closes
              WHERE outlet_id = ?1 AND business_day = ?2 AND terminal_id = ?3
                AND shift_no = ?4",
        )?;
        let mut rows = stmt.query_map(
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(day),
                terminal,
                shift_no
            ],
            read_day_close,
        )?;
        rows.next().transpose().map_err(DbError::from)
    }

    /// **The counted notes and coins**, replacing whatever was there.
    ///
    /// A recount is a correction of the count, not a second count — two rows
    /// for the same denomination on the same day would make the mix report a
    /// lie. `DELETE` then insert, in the caller's transaction, so a half-written
    /// count cannot exist.
    pub fn save_denominations(
        &self,
        close_id: &str,
        counted: &[Denomination],
    ) -> Result<(), DbError> {
        self.tx.execute(
            "DELETE FROM day_close_denominations WHERE day_close_id = ?1",
            rusqlite::params![close_id],
        )?;
        let mut stmt = self.tx.prepare_cached(
            "INSERT INTO day_close_denominations (day_close_id, denomination, count)
             VALUES (?1, ?2, ?3)",
        )?;
        for note in counted {
            // A zero count is not stored: "no five-hundreds" and "we did not
            // record five-hundreds" are the same thing to a shop, and a table
            // full of zeroes makes the mix report harder to read for nothing.
            if note.count == 0 {
                continue;
            }
            stmt.execute(rusqlite::params![
                close_id,
                encode::money_to_sql(note.value),
                i64::from(note.count),
            ])?;
        }
        Ok(())
    }

    pub fn denominations(&self, close_id: &str) -> Result<Vec<Denomination>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT denomination, count FROM day_close_denominations
              WHERE day_close_id = ?1 ORDER BY denomination DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![close_id], |row| {
            Ok(Denomination {
                value: encode::money_from_sql(row.get(0)?),
                count: u32::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// **Unlock a day that was closed** — see the audit row the caller writes.
    ///
    /// Not a delete. The close row stays, with its count and its reason, and
    /// only the lock comes off: a day that was closed at 11 pm, reopened at
    /// 11:20 and closed again is three facts, and deleting the first one loses
    /// two of them.
    pub fn unlock_day(&self, outlet: &str, day: BusinessDay) -> Result<bool, DbError> {
        let changed = self.tx.execute(
            // The SHOP's row is the only locked one (D140), so this touches it
            // and leaves every drawer's count exactly as it was counted —
            // reopening a day must not look like somebody recounted a box.
            "UPDATE day_closes SET is_locked = 0
              WHERE outlet_id = ?1 AND business_day = ?2 AND terminal_id IS NULL
                AND is_locked = 1",
            rusqlite::params![outlet, encode::business_day_to_sql(day)],
        )?;
        Ok(changed > 0)
    }
}

/// One `day_closes` row, read back.
///
/// One reader for the shop's roll-up, a till's drawer and the list — three
/// copies of this would be three chances for the terminal column to be dropped
/// from one of them.
fn read_day_close(row: &rusqlite::Row<'_>) -> rusqlite::Result<DayClose> {
    Ok(DayClose {
        id: row.get(0)?,
        business_day: BusinessDay::from_days_since_epoch(
            i32::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
        ),
        opening_float: encode::money_from_sql(row.get(2)?),
        expected_cash: encode::money_from_sql(row.get(3)?),
        counted_cash: encode::money_from_sql(row.get(4)?),
        variance: encode::money_from_sql(row.get(5)?),
        is_locked: row.get::<_, i64>(6)? == 1,
        closed_at: encode::timestamp_from_sql(row.get(7)?),
        closed_by: row.get::<_, Option<String>>(8)?.map(StaffId::new),
        note: row.get(9)?,
        terminal: row.get(10)?,
        shift_no: row.get(11)?,
    })
}
