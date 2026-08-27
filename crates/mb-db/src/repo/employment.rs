//! Shifts, attendance, leave, salary and payroll, on disk.

use mb_core::Timestamp;
use mb_core::businessday::BusinessDay;
use mb_core::employment::{
    Advance, Basis, Component, ComponentKind, HalfDays, LeaveEntry, LeaveKind, Structure,
};
use mb_core::money::Money;
use rusqlite::{Transaction, params};
use std::collections::BTreeMap;

use crate::encode;
use crate::error::DbError;

#[derive(Debug)]
pub struct EmploymentRepo<'a> {
    tx: &'a Transaction<'a>,
}

// The rows a screen reads.

/// One person, on the employment side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Employee {
    pub id: String,
    pub name: String,
    pub designation: Option<String>,
    pub department: Option<String>,
    pub phone: Option<String>,
    pub employment_type: String,
    pub status: String,
    pub joined_on: Option<BusinessDay>,
    /// Set when they have left.
    pub left_on: Option<BusinessDay>,
}

/// A stored day, when there is one.
fn day_opt(days: Option<i64>) -> Option<BusinessDay> {
    days.and_then(|d| i32::try_from(d).ok())
        .map(BusinessDay::from_days_since_epoch)
}

/// One of the shapes a shop's day comes in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShiftPattern {
    pub id: String,
    pub name: String,
    /// Minutes from midnight. `end` below `start` means it wraps past midnight — a night shift
    /// — and `mb_core::employment::minutes_between` is the only thing that may do arithmetic on
    /// the pair.
    pub start_minute: i64,
    pub end_minute: i64,
    pub break_minutes: i64,
    pub sort_order: i64,
    pub is_active: bool,
}

/// Who is expected on a day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterDay {
    pub id: String,
    pub staff_id: String,
    pub day: BusinessDay,
    pub pattern_id: Option<String>,
    pub note: Option<String>,
}

/// One shift somebody actually worked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attendance {
    pub id: String,
    pub staff_id: String,
    /// The day it STARTED in.
    pub day: BusinessDay,
    pub terminal_id: Option<String>,
    pub shift_no: i64,
    pub pattern_id: Option<String>,
    pub started_at: Timestamp,
    /// `None` is still clocked in.
    pub ended_at: Option<Timestamp>,
    pub corrected_at: Option<Timestamp>,
    pub corrected_by: Option<String>,
    pub correction_reason: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaveType {
    pub id: String,
    pub name: String,
    pub annual_half_days: Option<HalfDays>,
    /// The only column payroll reads.
    pub is_paid: bool,
    pub sort_order: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestState {
    Pending,
    Approved,
    Rejected,
    Cancelled,
}

impl RequestState {
    #[must_use]
    pub const fn as_sql(self) -> &'static str {
        match self {
            RequestState::Pending => "pending",
            RequestState::Approved => "approved",
            RequestState::Rejected => "rejected",
            RequestState::Cancelled => "cancelled",
        }
    }

    fn from_sql(text: &str) -> Result<Self, DbError> {
        match text {
            "pending" => Ok(RequestState::Pending),
            "approved" => Ok(RequestState::Approved),
            "rejected" => Ok(RequestState::Rejected),
            "cancelled" => Ok(RequestState::Cancelled),
            other => Err(DbError::invariant(format!(
                "leave_requests.state holds {other:?}, which is not a state this program knows"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaveRequest {
    pub id: String,
    pub staff_id: String,
    pub leave_type_id: String,
    pub from_day: BusinessDay,
    pub to_day: BusinessDay,
    pub half_days: HalfDays,
    pub reason: String,
    pub state: RequestState,
    pub requested_at: Timestamp,
    pub requested_by: Option<String>,
    pub decided_at: Option<Timestamp>,
    pub decided_by: Option<String>,
    pub decision_note: Option<String>,
}

/// One line of one payroll run, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayrollLine {
    pub id: String,
    pub staff_id: String,
    pub basis: Basis,
    pub basis_amount: Money,
    pub days_worked: HalfDays,
    pub minutes_worked: i64,
    pub unpaid: HalfDays,
    pub earned: Money,
    pub allowances: Money,
    pub deductions: Money,
    pub unpaid_leave_deduction: Money,
    pub advance_recovered: Money,
    pub net: Money,
    pub edited: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Draft,
    Approved,
    Reversed,
}

impl RunState {
    #[must_use]
    pub const fn as_sql(self) -> &'static str {
        match self {
            RunState::Draft => "draft",
            RunState::Approved => "approved",
            RunState::Reversed => "reversed",
        }
    }

    fn from_sql(text: &str) -> Result<Self, DbError> {
        match text {
            "draft" => Ok(RunState::Draft),
            "approved" => Ok(RunState::Approved),
            "reversed" => Ok(RunState::Reversed),
            other => Err(DbError::invariant(format!(
                "payroll_runs.state holds {other:?}, which is not a state this program knows"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayrollRun {
    pub id: String,
    pub from_day: BusinessDay,
    pub to_day: BusinessDay,
    pub state: RunState,
    pub computed_at: Timestamp,
    pub computed_by: Option<String>,
    pub approved_at: Option<Timestamp>,
    pub approved_by: Option<String>,
    pub cash_movement_id: Option<String>,
    pub expense_id: Option<String>,
    pub paid_by: String,
    pub reversed_at: Option<Timestamp>,
    pub reversal_reason: Option<String>,
    pub note: Option<String>,
}

fn basis_to_sql(b: Basis) -> &'static str {
    match b {
        Basis::Monthly => "monthly",
        Basis::Daily => "daily",
        Basis::Hourly => "hourly",
    }
}

fn basis_from_sql(text: &str) -> Result<Basis, DbError> {
    match text {
        "monthly" => Ok(Basis::Monthly),
        "daily" => Ok(Basis::Daily),
        "hourly" => Ok(Basis::Hourly),
        other => Err(DbError::invariant(format!(
            "a salary basis of {other:?} is not one this program knows"
        ))),
    }
}

fn leave_kind_to_sql(k: LeaveKind) -> &'static str {
    match k {
        LeaveKind::Accrued => "accrued",
        LeaveKind::Taken => "taken",
        LeaveKind::Adjusted => "adjusted",
        LeaveKind::Lapsed => "lapsed",
    }
}

fn leave_kind_from_sql(text: &str) -> Result<LeaveKind, DbError> {
    match text {
        "accrued" => Ok(LeaveKind::Accrued),
        "taken" => Ok(LeaveKind::Taken),
        "adjusted" => Ok(LeaveKind::Adjusted),
        "lapsed" => Ok(LeaveKind::Lapsed),
        other => Err(DbError::invariant(format!(
            "a leave ledger row of kind {other:?} is not one this program knows"
        ))),
    }
}

impl<'a> EmploymentRepo<'a> {
    #[must_use]
    pub fn new(tx: &'a Transaction<'a>) -> Self {
        EmploymentRepo { tx }
    }

    // The employment record.

    /// Everybody, with the employment side filled in.
    pub fn list_employees(&self, outlet: &str) -> Result<Vec<Employee>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, designation, department, phone, employment_type,
                    status, joined_on, left_on
               FROM staff WHERE outlet_id = ?1 ORDER BY status, name",
        )?;
        let mut rows = stmt.query(params![outlet])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Employee {
                id: row.get(0)?,
                name: row.get(1)?,
                designation: row.get(2)?,
                department: row.get(3)?,
                phone: row.get(4)?,
                employment_type: row.get(5)?,
                status: row.get(6)?,
                joined_on: day_opt(row.get(7)?),
                left_on: day_opt(row.get(8)?),
            });
        }
        Ok(out)
    }

    /// Name by id, for a screen that shows whose row this is.
    pub fn names(&self, outlet: &str) -> Result<BTreeMap<String, String>, DbError> {
        let mut stmt = self
            .tx
            .prepare_cached("SELECT id, name FROM staff WHERE outlet_id = ?1")?;
        let mut rows = stmt.query(params![outlet])?;
        let mut out = BTreeMap::new();
        while let Some(row) = rows.next()? {
            out.insert(row.get::<_, String>(0)?, row.get::<_, String>(1)?);
        }
        Ok(out)
    }

    /// Everybody a payroll run should consider: anybody who had not already left before the
    /// period started.
    pub fn employed_on_or_after(
        &self,
        outlet: &str,
        from: BusinessDay,
    ) -> Result<Vec<(String, String)>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name FROM staff
              WHERE outlet_id = ?1 AND (status <> 'left' OR left_on >= ?2)
              ORDER BY name",
        )?;
        let mut rows = stmt.query(params![outlet, encode::business_day_to_sql(from)])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
        }
        Ok(out)
    }

    /// The employment side of one person's record.
    #[allow(
        clippy::too_many_arguments,
        reason = "an employment record IS this many facts"
    )]
    pub fn save_employment(
        &self,
        outlet: &str,
        id: &str,
        designation: Option<&str>,
        department: Option<&str>,
        address: Option<&str>,
        emergency_name: Option<&str>,
        emergency_phone: Option<&str>,
        id_proof: Option<&str>,
        employment_type: &str,
        left_on: Option<BusinessDay>,
        at: Timestamp,
    ) -> Result<(), DbError> {
        let changed = self.tx.execute(
            "UPDATE staff
                SET designation     = ?3,
                    department      = ?4,
                    address         = ?5,
                    emergency_name  = ?6,
                    emergency_phone = ?7,
                    id_proof        = ?8,
                    employment_type = ?9,
                    left_on         = ?10,
                    status          = CASE
                                        WHEN ?10 IS NOT NULL THEN 'left'
                                        WHEN status = 'left'  THEN 'active'
                                        ELSE status
                                      END,
                    updated_at      = ?11
              WHERE outlet_id = ?1 AND id = ?2",
            params![
                outlet,
                id,
                designation,
                department,
                address,
                emergency_name,
                emergency_phone,
                id_proof,
                employment_type,
                left_on.map(encode::business_day_to_sql),
                encode::timestamp_to_sql(at),
            ],
        )?;
        if changed == 0 {
            return Err(DbError::invariant(
                "there is nobody on this counter with that id".to_owned(),
            ));
        }
        Ok(())
    }

    /// What the shop took over a window, for the staff-cost percentage.
    pub fn revenue_between(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Money, DbError> {
        let total: i64 = self.tx.query_row(
            // Joined to `orders`, because the outlet, the business day and the state all live
            // there — and `state = 'settled'` is what excludes a void.
            "SELECT coalesce(SUM(b.grand_total), 0)
               FROM bills b JOIN orders o ON o.id = b.order_id
              WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                AND o.state = 'settled'",
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            |row| row.get(0),
        )?;
        Ok(encode::money_from_sql(total))
    }

    /// Take back the drawer row a reversed payroll run wrote.
    pub fn delete_cash_movement(&self, outlet: &str, id: &str) -> Result<(), DbError> {
        self.tx.execute(
            "DELETE FROM cash_movements WHERE outlet_id = ?1 AND id = ?2",
            params![outlet, id],
        )?;
        Ok(())
    }

    // Shift patterns.

    pub fn save_pattern(&self, outlet: &str, p: &ShiftPattern) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO shift_patterns
                 (id, outlet_id, name, start_minute, end_minute, break_minutes,
                  sort_order, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET name          = excluded.name,
                                            start_minute  = excluded.start_minute,
                                            end_minute    = excluded.end_minute,
                                            break_minutes = excluded.break_minutes,
                                            sort_order    = excluded.sort_order,
                                            is_active     = excluded.is_active",
            params![
                p.id,
                outlet,
                p.name,
                p.start_minute,
                p.end_minute,
                p.break_minutes,
                p.sort_order,
                encode::bool_to_sql(p.is_active),
            ],
        )?;
        Ok(())
    }

    pub fn list_patterns(&self, outlet: &str) -> Result<Vec<ShiftPattern>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, start_minute, end_minute, break_minutes, sort_order, is_active
               FROM shift_patterns WHERE outlet_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map(params![outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, start_minute, end_minute, break_minutes, sort_order, active) = row?;
            out.push(ShiftPattern {
                id,
                name,
                start_minute,
                end_minute,
                break_minutes,
                sort_order,
                is_active: encode::bool_from_sql(active, "shift_patterns.is_active")?,
            });
        }
        Ok(out)
    }

    // The roster.

    pub fn save_roster_day(
        &self,
        outlet: &str,
        day: &RosterDay,
        at: Timestamp,
        by: Option<&str>,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO roster
                 (id, outlet_id, staff_id, business_day, pattern_id, note, created_at, created_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (outlet_id, staff_id, business_day)
                 DO UPDATE SET pattern_id = excluded.pattern_id,
                               note       = excluded.note",
            params![
                day.id,
                outlet,
                day.staff_id,
                encode::business_day_to_sql(day.day),
                day.pattern_id,
                day.note,
                encode::timestamp_to_sql(at),
                by,
            ],
        )?;
        Ok(())
    }

    /// The roster over a window, for a screen or for a payroll run.
    pub fn roster_between(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<RosterDay>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, staff_id, business_day, pattern_id, note
               FROM roster
              WHERE outlet_id = ?1 AND business_day BETWEEN ?2 AND ?3
              ORDER BY business_day, staff_id",
        )?;
        let rows = stmt.query_map(
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (id, staff_id, day, pattern_id, note) = row?;
            out.push(RosterDay {
                id,
                staff_id,
                day: encode::business_day_from_sql(day, "roster.business_day")?,
                pattern_id,
                note,
            });
        }
        Ok(out)
    }

    /// The row somebody is standing in front of, if they are clocked in.
    pub fn open_shift(&self, outlet: &str, staff_id: &str) -> Result<Option<Attendance>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, staff_id, business_day, terminal_id, shift_no, pattern_id,
                    started_at, ended_at, corrected_at, corrected_by, correction_reason, note
               FROM attendance
              WHERE outlet_id = ?1 AND staff_id = ?2 AND ended_at IS NULL
              ORDER BY started_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![outlet, staff_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(attendance_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn save_attendance(&self, outlet: &str, a: &Attendance) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO attendance
                 (id, outlet_id, staff_id, business_day, terminal_id, shift_no, pattern_id,
                  started_at, ended_at, corrected_at, corrected_by, correction_reason, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT (id) DO UPDATE SET
                 -- **started_at is in this list, and the first version left it
                 -- out.** Clocking out only ever changes the end, so it looked
                 -- complete; a CORRECTION changes both, and leaving the start
                 -- behind meant the new end could land before the old start —
                 -- which the CHECK then refused, with a message about a
                 -- constraint rather than about what a person had done.
                 started_at        = excluded.started_at,
                 ended_at          = excluded.ended_at,
                 corrected_at      = excluded.corrected_at,
                 corrected_by      = excluded.corrected_by,
                 correction_reason = excluded.correction_reason,
                 note              = excluded.note",
            params![
                a.id,
                outlet,
                a.staff_id,
                encode::business_day_to_sql(a.day),
                a.terminal_id,
                a.shift_no,
                a.pattern_id,
                encode::timestamp_to_sql(a.started_at),
                a.ended_at.map(encode::timestamp_to_sql),
                a.corrected_at.map(encode::timestamp_to_sql),
                a.corrected_by,
                a.correction_reason,
                a.note,
            ],
        )?;
        Ok(())
    }

    pub fn attendance_between(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<Attendance>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, staff_id, business_day, terminal_id, shift_no, pattern_id,
                    started_at, ended_at, corrected_at, corrected_by, correction_reason, note
               FROM attendance
              WHERE outlet_id = ?1 AND business_day BETWEEN ?2 AND ?3
              ORDER BY business_day, started_at",
        )?;
        let mut rows = stmt.query(params![
            outlet,
            encode::business_day_to_sql(from),
            encode::business_day_to_sql(to)
        ])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(attendance_from_row(row)?);
        }
        Ok(out)
    }

    /// Who is still clocked in from a day before this one.
    pub fn missed_clock_outs(
        &self,
        outlet: &str,
        before: BusinessDay,
    ) -> Result<Vec<Attendance>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, staff_id, business_day, terminal_id, shift_no, pattern_id,
                    started_at, ended_at, corrected_at, corrected_by, correction_reason, note
               FROM attendance
              WHERE outlet_id = ?1 AND ended_at IS NULL AND business_day < ?2
              ORDER BY business_day, started_at",
        )?;
        let mut rows = stmt.query(params![outlet, encode::business_day_to_sql(before)])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(attendance_from_row(row)?);
        }
        Ok(out)
    }

    pub fn save_leave_type(&self, outlet: &str, t: &LeaveType) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO leave_types
                 (id, outlet_id, name, annual_half_days, is_paid, sort_order, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET name             = excluded.name,
                                            annual_half_days = excluded.annual_half_days,
                                            is_paid          = excluded.is_paid,
                                            sort_order       = excluded.sort_order,
                                            is_active        = excluded.is_active",
            params![
                t.id,
                outlet,
                t.name,
                t.annual_half_days.map(|h| i64::from(h.halves())),
                encode::bool_to_sql(t.is_paid),
                t.sort_order,
                encode::bool_to_sql(t.is_active),
            ],
        )?;
        Ok(())
    }

    pub fn list_leave_types(&self, outlet: &str) -> Result<Vec<LeaveType>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, annual_half_days, is_paid, sort_order, is_active
               FROM leave_types WHERE outlet_id = ?1 ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map(params![outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, annual, paid, sort_order, active) = row?;
            out.push(LeaveType {
                id,
                name,
                annual_half_days: annual.map(|h| HalfDays::new(i32::try_from(h).unwrap_or(0))),
                is_paid: encode::bool_from_sql(paid, "leave_types.is_paid")?,
                sort_order,
                is_active: encode::bool_from_sql(active, "leave_types.is_active")?,
            });
        }
        Ok(out)
    }

    pub fn save_leave_request(&self, outlet: &str, r: &LeaveRequest) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO leave_requests
                 (id, outlet_id, staff_id, leave_type_id, from_day, to_day, half_days,
                  reason, state, requested_at, requested_by, decided_at, decided_by,
                  decision_note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (id) DO UPDATE SET state         = excluded.state,
                                            decided_at    = excluded.decided_at,
                                            decided_by    = excluded.decided_by,
                                            decision_note = excluded.decision_note",
            params![
                r.id,
                outlet,
                r.staff_id,
                r.leave_type_id,
                encode::business_day_to_sql(r.from_day),
                encode::business_day_to_sql(r.to_day),
                i64::from(r.half_days.halves()),
                r.reason,
                r.state.as_sql(),
                encode::timestamp_to_sql(r.requested_at),
                r.requested_by,
                r.decided_at.map(encode::timestamp_to_sql),
                r.decided_by,
                r.decision_note,
            ],
        )?;
        Ok(())
    }

    /// One person's requests, newest first.
    pub fn requests_for(&self, outlet: &str, staff_id: &str) -> Result<Vec<LeaveRequest>, DbError> {
        self.requests_where(
            "outlet_id = ?1 AND staff_id = ?2 ORDER BY from_day DESC",
            params![outlet, staff_id],
        )
    }

    /// Everything still waiting on a decision.
    pub fn pending_requests(&self, outlet: &str) -> Result<Vec<LeaveRequest>, DbError> {
        self.requests_where(
            "outlet_id = ?1 AND state = 'pending' ORDER BY from_day",
            params![outlet],
        )
    }

    /// The calendar. Who is approved to be away in this window.
    pub fn approved_between(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<LeaveRequest>, DbError> {
        self.requests_where(
            "outlet_id = ?1 AND state = 'approved' AND from_day <= ?3 AND to_day >= ?2
             ORDER BY from_day",
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
        )
    }

    fn requests_where(
        &self,
        clause: &str,
        args: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<LeaveRequest>, DbError> {
        let sql = format!(
            "SELECT id, staff_id, leave_type_id, from_day, to_day, half_days, reason,
                    state, requested_at, requested_by, decided_at, decided_by, decision_note
               FROM leave_requests WHERE {clause}"
        );
        let mut stmt = self.tx.prepare(&sql)?;
        let mut rows = stmt.query(args)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(LeaveRequest {
                id: row.get(0)?,
                staff_id: row.get(1)?,
                leave_type_id: row.get(2)?,
                from_day: encode::business_day_from_sql(row.get(3)?, "leave_requests.from_day")?,
                to_day: encode::business_day_from_sql(row.get(4)?, "leave_requests.to_day")?,
                half_days: HalfDays::new(i32::try_from(row.get::<_, i64>(5)?).unwrap_or(0)),
                reason: row.get(6)?,
                state: RequestState::from_sql(&row.get::<_, String>(7)?)?,
                requested_at: encode::timestamp_from_sql(row.get(8)?),
                requested_by: row.get(9)?,
                decided_at: row
                    .get::<_, Option<i64>>(10)?
                    .map(encode::timestamp_from_sql),
                decided_by: row.get(11)?,
                decision_note: row.get(12)?,
            });
        }
        Ok(out)
    }

    /// Write one row of the leave ledger.
    #[allow(clippy::too_many_arguments, reason = "a ledger row IS this many facts")]
    pub fn post_leave(
        &self,
        outlet: &str,
        id: &str,
        staff_id: &str,
        leave_type_id: &str,
        kind: LeaveKind,
        half_days: HalfDays,
        request_id: Option<&str>,
        reason: Option<&str>,
        at: Timestamp,
        day: BusinessDay,
        by: Option<&str>,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO leave_ledger
                 (id, outlet_id, staff_id, leave_type_id, kind, half_days, request_id,
                  reason, at, business_day, made_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                outlet,
                staff_id,
                leave_type_id,
                leave_kind_to_sql(kind),
                i64::from(half_days.halves()),
                request_id,
                reason,
                encode::timestamp_to_sql(at),
                encode::business_day_to_sql(day),
                by,
            ],
        )?;
        Ok(())
    }

    /// One person's ledger in one leave type, for `mb_core::employment::leave_balance`.
    pub fn leave_ledger(
        &self,
        outlet: &str,
        staff_id: &str,
        leave_type_id: &str,
    ) -> Result<Vec<LeaveEntry>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT business_day, kind, half_days, coalesce(reason, '')
               FROM leave_ledger
              WHERE outlet_id = ?1 AND staff_id = ?2 AND leave_type_id = ?3
              ORDER BY business_day, at",
        )?;
        let mut rows = stmt.query(params![outlet, staff_id, leave_type_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(LeaveEntry {
                day: encode::business_day_from_sql(row.get(0)?, "leave_ledger.business_day")?,
                kind: leave_kind_from_sql(&row.get::<_, String>(1)?)?,
                half_days: HalfDays::new(i32::try_from(row.get::<_, i64>(2)?).unwrap_or(0)),
                note: row.get(3)?,
            });
        }
        Ok(out)
    }

    pub fn save_structure(
        &self,
        outlet: &str,
        id: &str,
        staff_id: &str,
        s: &Structure,
        at: Timestamp,
        by: Option<&str>,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO salary_structures
                 (id, outlet_id, staff_id, effective_from, basis, amount, created_at,
                  created_by, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)
             ON CONFLICT (outlet_id, staff_id, effective_from)
                 DO UPDATE SET basis = excluded.basis, amount = excluded.amount",
            params![
                id,
                outlet,
                staff_id,
                encode::business_day_to_sql(s.effective_from),
                basis_to_sql(s.basis),
                encode::money_to_sql(s.amount),
                encode::timestamp_to_sql(at),
                by,
            ],
        )?;

        // The components belong to the structure and are rewritten with it — an edit that left
        // the old rows behind would double somebody's food allowance, which is the kind of bug
        // that pays out for months.
        self.tx.execute(
            "DELETE FROM salary_components WHERE structure_id = ?1",
            params![id],
        )?;
        for (n, c) in s.components.iter().enumerate() {
            self.tx.execute(
                "INSERT INTO salary_components (id, structure_id, kind, name, amount, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    format!("{id}_c{n}"),
                    id,
                    match c.kind {
                        ComponentKind::Allowance => "allowance",
                        ComponentKind::Deduction => "deduction",
                    },
                    c.name,
                    encode::money_to_sql(c.amount),
                    i64::try_from(n).unwrap_or(0),
                ],
            )?;
        }
        Ok(())
    }

    /// One person's whole salary history, for `mb_core::employment::structure_on`.
    pub fn structures_for(&self, outlet: &str, staff_id: &str) -> Result<Vec<Structure>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, effective_from, basis, amount
               FROM salary_structures
              WHERE outlet_id = ?1 AND staff_id = ?2
              ORDER BY effective_from",
        )?;
        let rows = stmt.query_map(params![outlet, staff_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, from, basis, amount) = row?;
            out.push(Structure {
                effective_from: encode::business_day_from_sql(
                    from,
                    "salary_structures.effective_from",
                )?,
                basis: basis_from_sql(&basis)?,
                amount: encode::money_from_sql(amount),
                components: self.components_of(&id)?,
            });
        }
        Ok(out)
    }

    fn components_of(&self, structure_id: &str) -> Result<Vec<Component>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT kind, name, amount FROM salary_components
              WHERE structure_id = ?1 ORDER BY sort_order",
        )?;
        let rows = stmt.query_map(params![structure_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, name, amount) = row?;
            out.push(Component {
                name,
                kind: match kind.as_str() {
                    "allowance" => ComponentKind::Allowance,
                    "deduction" => ComponentKind::Deduction,
                    other => {
                        return Err(DbError::invariant(format!(
                            "a salary component of kind {other:?} is not one this program knows"
                        )));
                    }
                },
                amount: encode::money_from_sql(amount),
            });
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments, reason = "an advance IS this many facts")]
    pub fn save_advance(
        &self,
        outlet: &str,
        id: &str,
        staff_id: &str,
        amount: Money,
        instalments: i32,
        reason: Option<&str>,
        at: Timestamp,
        day: BusinessDay,
        by: Option<&str>,
        cash_movement_id: Option<&str>,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO salary_advances
                 (id, outlet_id, staff_id, amount, instalments, reason, given_at,
                  business_day, given_by, cash_movement_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                outlet,
                staff_id,
                encode::money_to_sql(amount),
                i64::from(instalments),
                reason,
                encode::timestamp_to_sql(at),
                encode::business_day_to_sql(day),
                by,
                cash_movement_id,
            ],
        )?;
        Ok(())
    }

    /// One person's advances, with what has already come back off each — which is a `SUM` over
    /// `advance_recoveries` and never a stored column.
    pub fn advances_for(&self, outlet: &str, staff_id: &str) -> Result<Vec<Advance>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT a.id, a.business_day, a.amount, a.instalments,
                    coalesce((SELECT SUM(r.amount) FROM advance_recoveries r
                               WHERE r.advance_id = a.id), 0)
               FROM salary_advances a
              WHERE a.outlet_id = ?1 AND a.staff_id = ?2
              ORDER BY a.business_day, a.id",
        )?;
        let mut rows = stmt.query(params![outlet, staff_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Advance {
                id: row.get(0)?,
                day: encode::business_day_from_sql(row.get(1)?, "salary_advances.business_day")?,
                amount: encode::money_from_sql(row.get(2)?),
                instalments: i32::try_from(row.get::<_, i64>(3)?).unwrap_or(1),
                recovered: encode::money_from_sql(row.get(4)?),
            });
        }
        Ok(out)
    }

    pub fn save_recovery(
        &self,
        id: &str,
        advance_id: &str,
        run_id: &str,
        amount: Money,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO advance_recoveries (id, advance_id, run_id, amount)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, advance_id, run_id, encode::money_to_sql(amount)],
        )?;
        Ok(())
    }

    /// Undo a run's recoveries.
    pub fn clear_recoveries(&self, run_id: &str) -> Result<(), DbError> {
        self.tx.execute(
            "DELETE FROM advance_recoveries WHERE run_id = ?1",
            params![run_id],
        )?;
        Ok(())
    }

    pub fn save_run(&self, outlet: &str, r: &PayrollRun) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO payroll_runs
                 (id, outlet_id, from_day, to_day, state, computed_at, computed_by,
                  approved_at, approved_by, cash_movement_id, expense_id, paid_by,
                  reversed_at, reversed_by, reversal_reason, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?9, ?14, ?15)
             ON CONFLICT (id) DO UPDATE SET
                 state            = excluded.state,
                 approved_at      = excluded.approved_at,
                 approved_by      = excluded.approved_by,
                 cash_movement_id = excluded.cash_movement_id,
                 expense_id       = excluded.expense_id,
                 paid_by          = excluded.paid_by,
                 reversed_at      = excluded.reversed_at,
                 reversed_by      = excluded.reversed_by,
                 reversal_reason  = excluded.reversal_reason,
                 note             = excluded.note",
            params![
                r.id,
                outlet,
                encode::business_day_to_sql(r.from_day),
                encode::business_day_to_sql(r.to_day),
                r.state.as_sql(),
                encode::timestamp_to_sql(r.computed_at),
                r.computed_by,
                r.approved_at.map(encode::timestamp_to_sql),
                r.approved_by,
                r.cash_movement_id,
                r.expense_id,
                r.paid_by,
                r.reversed_at.map(encode::timestamp_to_sql),
                r.reversal_reason,
                r.note,
            ],
        )?;
        Ok(())
    }

    pub fn run(&self, outlet: &str, id: &str) -> Result<Option<PayrollRun>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, from_day, to_day, state, computed_at, computed_by, approved_at,
                    approved_by, cash_movement_id, expense_id, paid_by, reversed_at,
                    reversal_reason, note
               FROM payroll_runs WHERE outlet_id = ?1 AND id = ?2",
        )?;
        let mut rows = stmt.query(params![outlet, id])?;
        match rows.next()? {
            Some(row) => Ok(Some(run_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_runs(&self, outlet: &str) -> Result<Vec<PayrollRun>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, from_day, to_day, state, computed_at, computed_by, approved_at,
                    approved_by, cash_movement_id, expense_id, paid_by, reversed_at,
                    reversal_reason, note
               FROM payroll_runs WHERE outlet_id = ?1 ORDER BY from_day DESC",
        )?;
        let mut rows = stmt.query(params![outlet])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(run_from_row(row)?);
        }
        Ok(out)
    }

    pub fn replace_lines(&self, run_id: &str, lines: &[PayrollLine]) -> Result<(), DbError> {
        self.tx.execute(
            "DELETE FROM payroll_lines WHERE run_id = ?1",
            params![run_id],
        )?;
        for line in lines {
            self.tx.execute(
                "INSERT INTO payroll_lines
                     (id, run_id, staff_id, basis, basis_amount, days_worked_half,
                      minutes_worked, unpaid_half_days, earned, allowances, deductions,
                      unpaid_leave_deduction, advance_recovered, net, edited, note)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    line.id,
                    run_id,
                    line.staff_id,
                    basis_to_sql(line.basis),
                    encode::money_to_sql(line.basis_amount),
                    i64::from(line.days_worked.halves()),
                    line.minutes_worked,
                    i64::from(line.unpaid.halves()),
                    encode::money_to_sql(line.earned),
                    encode::money_to_sql(line.allowances),
                    encode::money_to_sql(line.deductions),
                    encode::money_to_sql(line.unpaid_leave_deduction),
                    encode::money_to_sql(line.advance_recovered),
                    encode::money_to_sql(line.net),
                    encode::bool_to_sql(line.edited),
                    line.note,
                ],
            )?;
        }
        Ok(())
    }

    pub fn lines_of(&self, run_id: &str) -> Result<Vec<PayrollLine>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT l.id, l.staff_id, l.basis, l.basis_amount, l.days_worked_half,
                    l.minutes_worked, l.unpaid_half_days, l.earned, l.allowances,
                    l.deductions, l.unpaid_leave_deduction, l.advance_recovered, l.net,
                    l.edited, l.note
               FROM payroll_lines l JOIN staff s ON s.id = l.staff_id
              WHERE l.run_id = ?1 ORDER BY s.name",
        )?;
        let mut rows = stmt.query(params![run_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(PayrollLine {
                id: row.get(0)?,
                staff_id: row.get(1)?,
                basis: basis_from_sql(&row.get::<_, String>(2)?)?,
                basis_amount: encode::money_from_sql(row.get(3)?),
                days_worked: HalfDays::new(i32::try_from(row.get::<_, i64>(4)?).unwrap_or(0)),
                minutes_worked: row.get(5)?,
                unpaid: HalfDays::new(i32::try_from(row.get::<_, i64>(6)?).unwrap_or(0)),
                earned: encode::money_from_sql(row.get(7)?),
                allowances: encode::money_from_sql(row.get(8)?),
                deductions: encode::money_from_sql(row.get(9)?),
                unpaid_leave_deduction: encode::money_from_sql(row.get(10)?),
                advance_recovered: encode::money_from_sql(row.get(11)?),
                net: encode::money_from_sql(row.get(12)?),
                edited: encode::bool_from_sql(row.get(13)?, "payroll_lines.edited")?,
                note: row.get(14)?,
            });
        }
        Ok(out)
    }

    /// What a shop paid its people over a period, for the staff-cost figure.
    pub fn staff_cost_between(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Money, DbError> {
        let total: i64 = self.tx.query_row(
            "SELECT coalesce(SUM(l.net), 0)
               FROM payroll_lines l JOIN payroll_runs r ON r.id = l.run_id
              WHERE r.outlet_id = ?1 AND r.state = 'approved'
                AND r.from_day >= ?2 AND r.to_day <= ?3",
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            |row| row.get(0),
        )?;
        Ok(encode::money_from_sql(total))
    }
}

fn attendance_from_row(row: &rusqlite::Row<'_>) -> Result<Attendance, DbError> {
    Ok(Attendance {
        id: row.get(0)?,
        staff_id: row.get(1)?,
        day: encode::business_day_from_sql(row.get(2)?, "attendance.business_day")?,
        terminal_id: row.get(3)?,
        shift_no: row.get(4)?,
        pattern_id: row.get(5)?,
        started_at: encode::timestamp_from_sql(row.get(6)?),
        ended_at: row
            .get::<_, Option<i64>>(7)?
            .map(encode::timestamp_from_sql),
        corrected_at: row
            .get::<_, Option<i64>>(8)?
            .map(encode::timestamp_from_sql),
        corrected_by: row.get(9)?,
        correction_reason: row.get(10)?,
        note: row.get(11)?,
    })
}

fn run_from_row(row: &rusqlite::Row<'_>) -> Result<PayrollRun, DbError> {
    Ok(PayrollRun {
        id: row.get(0)?,
        from_day: encode::business_day_from_sql(row.get(1)?, "payroll_runs.from_day")?,
        to_day: encode::business_day_from_sql(row.get(2)?, "payroll_runs.to_day")?,
        state: RunState::from_sql(&row.get::<_, String>(3)?)?,
        computed_at: encode::timestamp_from_sql(row.get(4)?),
        computed_by: row.get(5)?,
        approved_at: row
            .get::<_, Option<i64>>(6)?
            .map(encode::timestamp_from_sql),
        approved_by: row.get(7)?,
        cash_movement_id: row.get(8)?,
        expense_id: row.get(9)?,
        paid_by: row.get(10)?,
        reversed_at: row
            .get::<_, Option<i64>>(11)?
            .map(encode::timestamp_from_sql),
        reversal_reason: row.get(12)?,
        note: row.get(13)?,
    })
}
