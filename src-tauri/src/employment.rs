//! **Shifts, attendance, leave, salary and payroll** — P28.
//!
//! P11 built IDENTITY: who this is and what they may do. This is EMPLOYMENT:
//! what they are paid, when they worked, when they were away, and how an owner
//! manages all of it without standing at the counter.
//!
//! # Where the pieces live
//!
//! | | |
//! |---|---|
//! | the arithmetic | [`mb_core::employment`] — pure, no clock, no database |
//! | the rows | [`mb_db::repo::employment`] |
//! | this file | the commands, the permission boundary, and the words |
//!
//! Nothing here does a sum that `mb-core` could do, and nothing in `mb-core`
//! knows what a transaction is. That split is what makes the payroll
//! arithmetic — the part an owner will argue with — provable.
//!
//! # How payroll money reaches the drawer, and why it is TWO rows
//!
//! This is the part that is easy to get wrong, and getting it wrong makes the
//! cash position disagree with itself.
//!
//! A salary paid in cash is an **expense**, not a cash movement — P16 settled
//! that: the cash position already subtracts cash expenses, so writing both
//! would count the money twice (the schema note on `cash_movements` says so in
//! as many words).
//!
//! An **advance** is different. It is not a cost when it is handed over — it is
//! money the shop expects back — so it is a `payout` cash movement and no
//! expense at all. The drawer is genuinely down that much from that day, which
//! is the honest thing for it to show.
//!
//! Then payroll approval has to reconcile the two:
//!
//! ```text
//!   10th   advance   payout −2,000            drawer −2,000
//!    1st   payroll   expense −18,000 (gross)  drawer −18,000
//!                    top_up  +2,000           drawer +2,000
//!                                             ────────────
//!                                    cash actually paid −18,000  ✓
//!                                    salary cost         18,000  ✓
//! ```
//!
//! The `top_up` is not an invention to make a number work: at the moment the
//! advance is recovered it **stops being money owed to the shop** and becomes
//! part of the salary that was already counted. Without it the drawer would be
//! short by every advance ever given, for ever.
//!
//! **The honest limit:** on a run paid `bank`, that `top_up` credits the DRAWER
//! rather than the bank. That is right when the advance was cash — which it
//! always is, because an advance is notes handed over — and it keeps the one
//! figure this product actually counts exactly correct. A shop that pays
//! salaries by bank and gives advances by bank is not a shop this version
//! serves; it is written down here rather than discovered.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use mb_auth::Permission;
use mb_auth::audit::action;
use mb_core::businessday::BusinessDay;
use mb_core::employment::{
    self, Basis, Component, ComponentKind, HalfDays, LeaveKind, Structure, Worked,
};
use mb_core::money::Money;
use mb_auth::AuditEntry;
use mb_db::repo::employment::{
    Attendance, LeaveRequest, PayrollLine, PayrollRun, RequestState, RosterDay, RunState,
};
use mb_db::repo::money::{CashMovement, Expense};

use crate::flows::{now, today};
use crate::guard;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

/// The expense category a payroll run posts against. P16 seeds it.
const SALARY_CATEGORY: &str = "exc_salary";

// ===========================================================================
// The view models
// ===========================================================================

// **MoneyView is ipc.rs's, not a second one.** P28 declared its own for
// half an hour and it would have quietly overwritten the canonical file — the
// two shapes happened to match, so nothing would have complained, and the
// doc-comment explaining why money crosses the wire as a pair would have gone.
// One shape for money on the wire, one file, one explanation.
use crate::ipc::MoneyView;

fn money(m: Money) -> MoneyView {
    MoneyView::from(m)
}

/// One person, on the employment side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct EmployeeView {
    pub id: String,
    pub name: String,
    pub designation: Option<String>,
    pub department: Option<String>,
    pub phone: Option<String>,
    pub employment_type: String,
    /// `active`, `suspended` or `left`.
    pub status: String,
    /// "Joined 12 March 2025", or empty.
    pub joined: String,
    /// Set when they have left. The record stays for ever (scope 9.15).
    pub left: String,
    /// What they are on right now, in words: "₹18,000 a month".
    pub salary_says: String,
    /// True while they are clocked in — the screen shows a dot.
    pub is_in: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct EmployeeEdit {
    pub id: String,
    pub designation: String,
    pub department: String,
    pub address: String,
    pub emergency_name: String,
    pub emergency_phone: String,
    pub id_proof: String,
    pub employment_type: String,
    /// Typed by a person, parsed in Rust (D39). Empty is "still working".
    pub left_on: String,
}

/// One shift on the attendance screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ShiftView {
    pub id: String,
    pub staff_id: String,
    pub staff_name: String,
    pub day: String,
    pub started: String,
    /// Empty while they are still in.
    pub ended: String,
    /// "7h 30m", or "still in".
    pub worked: String,
    /// From [`mb_core::employment::DayVerdict`], in words: "Late by 35 minutes".
    pub verdict: String,
    /// `ok`, `warn` or `danger` — for the badge. The sentence says it too (§2).
    pub tone: String,
    /// True when a manager changed it. The screen marks it (D47).
    pub corrected: bool,
    pub correction_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AttendanceView {
    pub from: String,
    pub to: String,
    pub shifts: Vec<ShiftView>,
    /// **Still clocked in from a day before today.** The one an owner has to
    /// deal with, so it is its own list rather than a row in the middle.
    pub missed: Vec<ShiftView>,
    pub says: String,
    pub may_correct: bool,
    /// **The shifts this shop runs** — P31.
    ///
    /// Already read here to work out whether somebody was late; sent on so the
    /// roster can be SET as well as judged against. Without it `save_roster`
    /// would need a screen where a person typed a pattern id, which is not a
    /// thing anybody outside this repository knows.
    pub patterns: Vec<PatternView>,
}

/// One of the shop's shifts, for the roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PatternView {
    pub id: String,
    /// "Morning — 7:00 to 15:00". Written here, because turning 420 minutes
    /// past midnight into a time somebody reads is a conversion and every
    /// conversion in this product happens once, in Rust (D39).
    pub says: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct LeaveBalanceView {
    pub leave_type_id: String,
    pub leave_type: String,
    pub is_paid: bool,
    /// Half-days, as the number — for a screen that wants to compare.
    pub left_halves: i32,
    /// The same thing in words: "7½ days". Written in Rust so the screen, the
    /// payslip and a refusal cannot disagree (§6).
    pub left_says: String,
    pub accrued_says: String,
    pub taken_says: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct LeaveRequestView {
    pub id: String,
    pub staff_id: String,
    pub staff_name: String,
    pub leave_type: String,
    pub from: String,
    pub to: String,
    pub days_says: String,
    pub reason: String,
    pub state: String,
    pub decided_by: Option<String>,
    pub decision_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct LeaveView {
    /// Whose balances these are. Empty on the shop-wide view.
    pub staff_id: String,
    pub balances: Vec<LeaveBalanceView>,
    pub requests: Vec<LeaveRequestView>,
    /// Waiting on somebody. Empty for a person looking at their own.
    pub pending: Vec<LeaveRequestView>,
    pub may_approve: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AdvanceView {
    pub id: String,
    pub given: String,
    pub amount: MoneyView,
    pub recovered: MoneyView,
    pub outstanding: MoneyView,
    pub instalments: i32,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SalaryView {
    pub staff_id: String,
    pub staff_name: String,
    /// Every structure, oldest first. **The history stays** — a raise is a new
    /// row, never an edit, which is what lets last month recompute the same.
    pub structures: Vec<StructureView>,
    pub advances: Vec<AdvanceView>,
    pub outstanding: MoneyView,
    pub may_manage: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct StructureView {
    pub effective_from: String,
    pub basis: String,
    pub amount: MoneyView,
    pub says: String,
    pub components: Vec<ComponentView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ComponentView {
    pub name: String,
    pub kind: String,
    pub amount: MoneyView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PayLineView {
    pub id: String,
    pub staff_id: String,
    pub staff_name: String,
    pub basis: String,
    pub basis_amount: MoneyView,
    pub days_says: String,
    pub unpaid_says: String,
    pub earned: MoneyView,
    pub allowances: MoneyView,
    pub deductions: MoneyView,
    pub unpaid_leave_deduction: MoneyView,
    pub advance_recovered: MoneyView,
    pub net: MoneyView,
    /// Somebody changed a figure by hand. The screen shows it, so a reviewer
    /// knows which lines are the computer's.
    pub edited: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PayrollView {
    pub id: String,
    pub from: String,
    pub to: String,
    pub state: String,
    pub lines: Vec<PayLineView>,
    pub total: MoneyView,
    pub says: String,
    pub may_manage: bool,
    pub paid_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PayrollListView {
    pub runs: Vec<PayrollRunView>,
    pub may_manage: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PayrollRunView {
    pub id: String,
    pub from: String,
    pub to: String,
    pub state: String,
    pub total: MoneyView,
    pub people: u32,
}

/// Scope 9.16 — **the second of the two numbers that decide whether a
/// restaurant makes money.** P25 gave the first (food cost); this is the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct StaffCostView {
    pub from: String,
    pub to: String,
    pub wages: MoneyView,
    pub revenue: MoneyView,
    /// "23.4%", or a sentence saying why there is no percentage.
    pub says: String,
}

// ===========================================================================
// Words
// ===========================================================================

fn day_words(day: BusinessDay) -> String {
    let (y, m, d) = day.to_ymd();
    format!("{y:04}-{m:02}-{d:02}")
}

fn parse_day(text: &str, field: &'static str) -> UiResult<BusinessDay> {
    let parts: Vec<&str> = text.trim().split('-').collect();
    let bad = || {
        UiError::new(
            field,
            "Type the date as 2026-08-16 — the year, the month, then the day.",
        )
    };
    if parts.len() != 3 {
        return Err(bad());
    }
    let year: i32 = parts[0].parse().map_err(|_| bad())?;
    let month: u32 = parts[1].parse().map_err(|_| bad())?;
    let dom: u32 = parts[2].parse().map_err(|_| bad())?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&dom) {
        return Err(bad());
    }
    Ok(BusinessDay::from_ymd(year, month, dom))
}

#[allow(
    clippy::integer_division,
    reason = "minutes into hours and minutes IS a flooring division, and the 
              remainder is used on the next line — nothing is discarded"
)]
fn minutes_words(minutes: i64) -> String {
    if minutes <= 0 {
        return "none".to_owned();
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    match (hours, rest) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// "₹18,000.00 a month".
///
/// **No rupee sign of its own** — `Money::to_indian_string` already carries
/// one, and the first version added a second. Found by opening the Salary
/// screen and reading "₹₹650.00 a day": the kind of thing no test would ever
/// notice and every shopkeeper would (D55).
fn basis_words(basis: Basis, amount: Money) -> String {
    let sum = amount.to_indian_string();
    match basis {
        Basis::Monthly => format!("{sum} a month"),
        Basis::Daily => format!("{sum} a day"),
        Basis::Hourly => format!("{sum} an hour"),
    }
}

fn basis_tag(basis: Basis) -> &'static str {
    match basis {
        Basis::Monthly => "monthly",
        Basis::Daily => "daily",
        Basis::Hourly => "hourly",
    }
}

fn basis_from_tag(tag: &str) -> UiResult<Basis> {
    match tag {
        "monthly" => Ok(Basis::Monthly),
        "daily" => Ok(Basis::Daily),
        "hourly" => Ok(Basis::Hourly),
        _ => Err(UiError::new(
            "salary.basis",
            "Pay by the month, by the day or by the hour.",
        )),
    }
}

/// A verdict, in the words a shop reads (§6) — and its tone, which is the
/// second signal (§2 rule 2: colour is never the only one).
fn verdict_words(verdict: employment::DayVerdict) -> (String, &'static str) {
    use employment::DayVerdict as V;
    match verdict {
        V::Present => ("On time".to_owned(), "ok"),
        V::Late { by_minutes } => (
            format!("Late by {}", minutes_words(by_minutes)),
            "warn",
        ),
        V::LeftEarly { by_minutes } => (
            format!("Left {} early", minutes_words(by_minutes)),
            "warn",
        ),
        V::Absent => ("Did not come".to_owned(), "danger"),
        V::Away => ("Away".to_owned(), "neutral"),
        V::Unrostered => ("Worked — not rostered".to_owned(), "neutral"),
    }
}

// ===========================================================================
// Reading
// ===========================================================================

/// The People tab, with the employment side filled in.
pub fn people_on(app: &App) -> UiResult<Vec<EmployeeView>> {
    guard::require(app, Permission::StaffManage)?;
    let may_see_pay = guard::require(app, Permission::SalaryView).is_ok();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut out = Vec::new();

                for person in repos.employment().list_employees(OUTLET)? {
                    // **The salary is behind its own permission**, so a manager
                    // who may edit staff does not thereby learn what everybody
                    // earns. Hidden HERE and not in React, or it would have
                    // been sent and merely not drawn.
                    let salary_says = if may_see_pay {
                        let structures =
                            repos.employment().structures_for(OUTLET, &person.id)?;
                        employment::structure_on(&structures, today(now()))
                            .map(|s| basis_words(s.basis, s.amount))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };

                    out.push(EmployeeView {
                        is_in: repos
                            .employment()
                            .open_shift(OUTLET, &person.id)?
                            .is_some(),
                        id: person.id.clone(),
                        name: person.name.clone(),
                        designation: person.designation.clone(),
                        department: person.department.clone(),
                        phone: person.phone.clone(),
                        employment_type: person.employment_type.clone(),
                        status: person.status.clone(),
                        joined: person.joined_on.map(day_words).unwrap_or_default(),
                        left: person.left_on.map(day_words).unwrap_or_default(),
                        salary_says,
                    });
                }
                Ok(out)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// Attendance over a window. `staff_id` empty is the whole shop.
pub fn attendance_on(
    app: &App,
    staff_id: Option<String>,
    from: String,
    to: String,
) -> UiResult<AttendanceView> {
    // **Reading your OWN attendance needs nothing but being signed in** (scope
    // 9.14). Reading anybody else's needs `attendance.mark`. That single line
    // is the whole of self-service's read side, and it is here rather than in
    // React because a screen that merely does not draw a row has still been
    // sent it.
    let who = guard::require_any(
        app,
        &[Permission::AttendanceMark, Permission::AttendanceCorrect],
    );
    let signed_in = app
        .sessions()
        .current()
        .ok_or_else(|| UiError::new("auth.locked", "The screen is locked. Sign in to carry on."))?;

    let asked_for = staff_id.clone().unwrap_or_default();
    let only_own = who.is_err();
    if only_own && asked_for != signed_in.actor.staff_id.as_str() {
        return Err(UiError::new(
            "attendance.not_yours",
            "You can see your own hours. Somebody else's needs permission.",
        ));
    }

    let from_day = parse_day(&from, "attendance.from")?;
    let to_day = parse_day(&to, "attendance.to")?;
    let today_day = today(now());
    let may_correct = guard::require(app, Permission::AttendanceCorrect).is_ok();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let names = staff_names(&repos)?;
                let patterns = repos.employment().list_patterns(OUTLET)?;
                let roster = repos.employment().roster_between(OUTLET, from_day, to_day)?;
                let approved = repos.employment().approved_between(OUTLET, from_day, to_day)?;
                let grace = grace_minutes(&repos)?;

                let rows = repos
                    .employment()
                    .attendance_between(OUTLET, from_day, to_day)?;

                let mut shifts = Vec::new();
                for a in rows {
                    if !asked_for.is_empty() && a.staff_id != asked_for {
                        continue;
                    }
                    shifts.push(shift_view(
                        &a, &names, &patterns, &roster, &approved, grace,
                    ));
                }

                let mut missed = Vec::new();
                for a in repos.employment().missed_clock_outs(OUTLET, today_day)? {
                    if !asked_for.is_empty() && a.staff_id != asked_for {
                        continue;
                    }
                    missed.push(shift_view(
                        &a, &names, &patterns, &roster, &approved, grace,
                    ));
                }

                let says = if missed.is_empty() {
                    words::count(i64::try_from(shifts.len()).unwrap_or(i64::MAX), "shift", "shifts")
                } else {
                    format!(
                        "{} shifts, and {} nobody clocked out of — those hours cannot be \
                         worked out until somebody says when they left.",
                        shifts.len(),
                        missed.len()
                    )
                };

                Ok(AttendanceView {
                    from: day_words(from_day),
                    to: day_words(to_day),
                    shifts,
                    missed,
                    says,
                    may_correct,
                    patterns: patterns
                        .iter()
                        .filter(|p| p.is_active)
                        .map(|p| PatternView {
                            id: p.id.clone(),
                            says: format!(
                                "{} — {} to {}",
                                p.name,
                                clock(p.start_minute),
                                clock(p.end_minute)
                            ),
                        })
                        .collect(),
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

fn shift_view(
    a: &Attendance,
    names: &std::collections::BTreeMap<String, String>,
    patterns: &[mb_db::repo::employment::ShiftPattern],
    roster: &[RosterDay],
    approved: &[LeaveRequest],
    grace: i64,
) -> ShiftView {
    let expected = roster
        .iter()
        .find(|r| r.staff_id == a.staff_id && r.day == a.day)
        .and_then(|r| r.pattern_id.as_ref())
        .and_then(|id| patterns.iter().find(|p| &p.id == id))
        .map(|p| (p.start_minute, p.end_minute));

    let on_leave = approved
        .iter()
        .any(|r| r.staff_id == a.staff_id && r.from_day <= a.day && r.to_day >= a.day);

    let start_minute = minute_of(a.started_at);
    let end_minute = a.ended_at.map(minute_of);
    let (verdict, tone) = verdict_words(employment::judge_day(
        expected,
        Some(start_minute),
        end_minute,
        on_leave,
        grace,
    ));

    let worked = match end_minute {
        Some(end) => minutes_words(employment::minutes_between(start_minute, end)),
        None => "still in".to_owned(),
    };

    ShiftView {
        id: a.id.clone(),
        staff_id: a.staff_id.clone(),
        staff_name: names.get(&a.staff_id).cloned().unwrap_or_default(),
        day: day_words(a.day),
        started: clock_words(a.started_at),
        ended: a.ended_at.map(clock_words).unwrap_or_default(),
        worked,
        verdict,
        tone: tone.to_owned(),
        corrected: a.corrected_at.is_some(),
        correction_reason: a.correction_reason.clone(),
    }
}

/// Minutes past local midnight, for the roster comparison.
#[allow(
    clippy::integer_division,
    reason = "seconds into minutes: the remainder is a fraction of a minute and 
              a roster is not kept to the second"
)]
fn minute_of(at: mb_core::Timestamp) -> i64 {
    let (_, seconds) = at.to_local_parts(mb_core::UtcOffset::INDIA);
    i64::from(seconds / 60)
}

#[allow(
    clippy::integer_division,
    reason = "a minute count into a clock face: both halves are used"
)]
fn clock_words(at: mb_core::Timestamp) -> String {
    let minute = minute_of(at);
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// Minutes past midnight, as a clock face. P31 — a shift pattern's start and
/// end, so the roster can say "Morning — 07:00 to 15:00" rather than "420".
#[allow(
    clippy::integer_division,
    reason = "a minute count into a clock face: both halves are used"
)]
fn clock(minute_of_day: i64) -> String {
    // Past midnight wraps, which is what a night shift is.
    let m = minute_of_day.rem_euclid(24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

fn staff_names(
    repos: &mb_db::Repos<'_>,
) -> Result<std::collections::BTreeMap<String, String>, mb_db::DbError> {
    repos.employment().names(OUTLET)
}

/// How late is late. A shop's own number, defaulting to ten minutes — which is
/// forgiving on purpose: a grace period of zero turns every bus into a
/// disciplinary matter and the report into noise nobody reads.
fn grace_minutes(repos: &mb_db::Repos<'_>) -> Result<i64, mb_db::DbError> {
    Ok(repos
        .settings()
        .get::<i64>(OUTLET, "attendance.grace_minutes")?
        .unwrap_or(10))
}

/// The leave screen. `staff_id` empty means the signed-in person.
pub fn leave_on(app: &App, staff_id: Option<String>) -> UiResult<LeaveView> {
    let signed_in = app
        .sessions()
        .current()
        .ok_or_else(|| UiError::new("auth.locked", "The screen is locked. Sign in to carry on."))?;
    let may_approve = guard::require(app, Permission::LeaveApprove).is_ok();

    // Self-service (9.14): your own is yours; anybody else's needs the
    // permission. Enforced HERE, server-side — T11. A screen that merely does
    // not draw a row has still been sent it.
    let asked_for = staff_id
        .clone()
        .unwrap_or_else(|| signed_in.actor.staff_id.as_str().to_owned());
    if !may_approve && asked_for != signed_in.actor.staff_id.as_str() {
        return Err(UiError::new(
            "leave.not_yours",
            "You can see your own leave. Somebody else's needs permission.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let names = staff_names(&repos)?;
                let types = repos.employment().list_leave_types(OUTLET)?;

                let mut balances = Vec::new();
                for t in &types {
                    let ledger = repos.employment().leave_ledger(OUTLET, &asked_for, &t.id)?;
                    // **The balance is the sum of the ledger** — computed by
                    // mb-core, from the rows, every time. Nothing is stored.
                    let b = employment::leave_balance(&ledger);
                    balances.push(LeaveBalanceView {
                        leave_type_id: t.id.clone(),
                        leave_type: t.name.clone(),
                        is_paid: t.is_paid,
                        left_halves: b.left.halves(),
                        left_says: b.left.say(),
                        accrued_says: b.accrued.say(),
                        taken_says: (HalfDays::ZERO - b.taken).say(),
                    });
                }

                let view = |r: &LeaveRequest| LeaveRequestView {
                    id: r.id.clone(),
                    staff_id: r.staff_id.clone(),
                    staff_name: names.get(&r.staff_id).cloned().unwrap_or_default(),
                    leave_type: types
                        .iter()
                        .find(|t| t.id == r.leave_type_id)
                        .map(|t| t.name.clone())
                        .unwrap_or_default(),
                    from: day_words(r.from_day),
                    to: day_words(r.to_day),
                    days_says: r.half_days.say(),
                    reason: r.reason.clone(),
                    state: r.state.as_sql().to_owned(),
                    decided_by: r.decided_by.as_ref().and_then(|id| names.get(id).cloned()),
                    decision_note: r.decision_note.clone(),
                };

                let requests = repos
                    .employment()
                    .requests_for(OUTLET, &asked_for)?
                    .iter()
                    .map(view)
                    .collect();

                let pending = if may_approve {
                    repos
                        .employment()
                        .pending_requests(OUTLET)?
                        .iter()
                        .map(view)
                        .collect()
                } else {
                    Vec::new()
                };

                Ok(LeaveView {
                    staff_id: asked_for.clone(),
                    balances,
                    requests,
                    pending,
                    may_approve,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// One person's salary history and advances.
pub fn salary_on(app: &App, staff_id: String) -> UiResult<SalaryView> {
    guard::require(app, Permission::SalaryView)?;
    let may_manage = guard::require(app, Permission::SalaryManage).is_ok();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let names = staff_names(&repos)?;
                let structures = repos.employment().structures_for(OUTLET, &staff_id)?;
                let advances = repos.employment().advances_for(OUTLET, &staff_id)?;

                let mut outstanding = Money::ZERO;
                let mut rows = Vec::new();
                for a in &advances {
                    let left = a
                        .outstanding()
                        .map_err(|e| mb_db::DbError::invariant(format!("an advance: {e}")))?;
                    outstanding = outstanding
                        .add(left)
                        .map_err(|e| mb_db::DbError::invariant(format!("the advances: {e}")))?;
                    rows.push(AdvanceView {
                        id: a.id.clone(),
                        given: day_words(a.day),
                        amount: money(a.amount),
                        recovered: money(a.recovered),
                        outstanding: money(left),
                        instalments: a.instalments,
                        reason: None,
                    });
                }

                Ok(SalaryView {
                    staff_name: names.get(&staff_id).cloned().unwrap_or_default(),
                    staff_id: staff_id.clone(),
                    structures: structures
                        .iter()
                        .map(|s| StructureView {
                            effective_from: day_words(s.effective_from),
                            basis: basis_tag(s.basis).to_owned(),
                            amount: money(s.amount),
                            says: basis_words(s.basis, s.amount),
                            components: s
                                .components
                                .iter()
                                .map(|c| ComponentView {
                                    name: c.name.clone(),
                                    kind: match c.kind {
                                        ComponentKind::Allowance => "allowance".to_owned(),
                                        ComponentKind::Deduction => "deduction".to_owned(),
                                    },
                                    amount: money(c.amount),
                                })
                                .collect(),
                        })
                        .collect(),
                    advances: rows,
                    outstanding: money(outstanding),
                    may_manage,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

// ===========================================================================
// Clocking on and off
// ===========================================================================

/// **Clock in.** Needs nothing but being signed in — it IS the PIN.
///
/// Deliberately weak, and it is the counterpart to `attendance.correct` being
/// strong: recording that you turned up should cost nothing, and changing the
/// record afterwards should cost a permission and an audit row.
pub fn clock_in_on(app: &App, terminal_id: Option<String>) -> UiResult<AttendanceView> {
    let session = app
        .sessions()
        .current()
        .ok_or_else(|| UiError::new("auth.locked", "The screen is locked. Sign in to carry on."))?;
    let staff_id = session.actor.staff_id.as_str().to_owned();
    let at = now();
    // **D5.** The day it STARTS in, stamped, never re-derived. A night shift
    // belongs to this day whole — every hour of it, on the payroll and on the
    // handover.
    let day = today(at);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                if let Some(open) = repos.employment().open_shift(OUTLET, &staff_id)? {
                    return Err(mb_db::DbError::invariant(format!(
                        "already clocked in at {}",
                        clock_words(open.started_at)
                    )));
                }

                let pattern = repos
                    .employment()
                    .roster_between(OUTLET, day, day)?
                    .into_iter()
                    .find(|r| r.staff_id == staff_id)
                    .and_then(|r| r.pattern_id);

                repos.employment().save_attendance(
                    OUTLET,
                    &Attendance {
                        id: crate::newid::fresh_at("att", at),
                        staff_id: staff_id.clone(),
                        day,
                        terminal_id: terminal_id.clone(),
                        shift_no: 0,
                        pattern_id: pattern,
                        started_at: at,
                        ended_at: None,
                        corrected_at: None,
                        corrected_by: None,
                        correction_reason: None,
                        note: None,
                    },
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(session.actor.staff_id.clone()),
                        action::CLOCKED_IN,
                        "attendance",
                    )
                    .about(staff_id.clone()),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    attendance_on(app, Some(staff_id), day_words(day), day_words(day))
}

/// **Clock out**, closing whichever row is open — which may be yesterday's.
pub fn clock_out_on(app: &App) -> UiResult<AttendanceView> {
    let session = app
        .sessions()
        .current()
        .ok_or_else(|| UiError::new("auth.locked", "The screen is locked. Sign in to carry on."))?;
    let staff_id = session.actor.staff_id.as_str().to_owned();
    let at = now();
    let day = today(at);

    let shift_day = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mut open) = repos.employment().open_shift(OUTLET, &staff_id)? else {
                    return Err(mb_db::DbError::invariant("not clocked in".to_owned()));
                };
                let shift_day = open.day;
                open.ended_at = Some(at);
                repos.employment().save_attendance(OUTLET, &open)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        // The audit row belongs to TODAY; the shift belongs to
                        // the day it started in. Two different facts, and a
                        // night shift is exactly where they differ (D5).
                        day,
                        Some(session.actor.staff_id.clone()),
                        action::CLOCKED_OUT,
                        "attendance",
                    )
                    .about(open.id.clone())
                    .with_after(serde_json::json!({
                        "shift_day": day_words(open.day),
                        "minutes": employment::minutes_between(
                            minute_of(open.started_at),
                            minute_of(at),
                        ),
                    })),
                )?;
                Ok(shift_day)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    attendance_on(
        app,
        Some(staff_id),
        day_words(shift_day),
        day_words(day),
    )
}

/// **Correct a clock-in or a clock-out.** The one control in attendance that
/// matters — see `Permission::AttendanceCorrect`.
pub fn correct_attendance_on(
    app: &App,
    id: String,
    started: String,
    ended: String,
    reason: String,
) -> UiResult<AttendanceView> {
    let who = guard::require(app, Permission::AttendanceCorrect)?;
    let at = now();
    let day = today(at);

    if reason.trim().is_empty() {
        return Err(UiError::new(
            "attendance.reason",
            "Say why the hours are being changed. A correction nobody can \
             explain is indistinguishable from a mistake.",
        ));
    }

    let start_minute = parse_clock(&started, "attendance.started")?;
    let end_minute = if ended.trim().is_empty() {
        None
    } else {
        Some(parse_clock(&ended, "attendance.ended")?)
    };

    let (shift_day, staff_id) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mut row) = find_shift(&repos, &id, day)? else {
                    return Err(mb_db::DbError::invariant(
                        "that shift is not on this counter".to_owned(),
                    ));
                };

                // **Never your own.** The whole point of the permission: hours
                // a person can edit themselves are hours nobody can rely on.
                if row.staff_id == who.staff_id.as_str() {
                    return Err(mb_db::DbError::invariant(
                        "a person may not correct their own hours".to_owned(),
                    ));
                }

                let before = serde_json::json!({
                    "started": clock_words(row.started_at),
                    "ended": row.ended_at.map(clock_words),
                });

                row.started_at = stamp_on(row.day, start_minute);
                row.ended_at = end_minute.map(|m| {
                    // An end BEFORE the start is a night shift: it is on the
                    // next calendar day, and the stamp has to say so or the
                    // hours come out negative.
                    let end_day = if m < start_minute { row.day.next() } else { row.day };
                    stamp_on(end_day, m)
                });
                row.corrected_at = Some(at);
                row.corrected_by = Some(who.staff_id.as_str().to_owned());
                row.correction_reason = Some(reason.trim().to_owned());

                let after = serde_json::json!({
                    "started": clock_words(row.started_at),
                    "ended": row.ended_at.map(clock_words),
                    "reason": reason.trim(),
                });

                let staff_id = row.staff_id.clone();
                let shift_day = row.day;
                repos.employment().save_attendance(OUTLET, &row)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::ATTENDANCE_CORRECTED,
                        "attendance",
                    )
                    .about(id.clone())
                    .changed(before, after),
                )?;
                Ok((shift_day, staff_id))
            })
            .map_err(|e| words::from_db(&e))
    })?;

    attendance_on(
        app,
        Some(staff_id),
        day_words(shift_day),
        day_words(shift_day),
    )
}

/// A shift by id, wherever it is — including one that is still open from a
/// later day, which `attendance_between` would miss.
fn find_shift(
    repos: &mb_db::Repos<'_>,
    id: &str,
    upto: BusinessDay,
) -> Result<Option<Attendance>, mb_db::DbError> {
    let mut found = repos
        .employment()
        .attendance_between(OUTLET, BusinessDay::from_days_since_epoch(0), upto.next())?
        .into_iter()
        .find(|a| a.id == id);
    if found.is_none() {
        found = repos
            .employment()
            .missed_clock_outs(OUTLET, upto.next())?
            .into_iter()
            .find(|a| a.id == id);
    }
    Ok(found)
}

fn parse_clock(text: &str, field: &'static str) -> UiResult<i64> {
    let bad = || UiError::new(field, "Type the time as 09:30 — the hour, then the minutes.");
    let (h, m) = text.trim().split_once(':').ok_or_else(bad)?;
    let hour: i64 = h.trim().parse().map_err(|_| bad())?;
    let minute: i64 = m.trim().parse().map_err(|_| bad())?;
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) {
        return Err(bad());
    }
    Ok(hour * 60 + minute)
}

/// A stamp at a minute of a business day, in the shop's own time.
fn stamp_on(day: BusinessDay, minute: i64) -> mb_core::Timestamp {
    let seconds = u32::try_from(minute * 60).unwrap_or(0);
    mb_core::Timestamp::from_local_parts(
        day.days_since_epoch(),
        seconds,
        mb_core::UtcOffset::INDIA,
    )
    .unwrap_or_else(|_| now())
}

// ===========================================================================
// Leave
// ===========================================================================

/// Ask for leave. A person may ask for their own; a manager may enter one on
/// somebody's behalf, and then the audit row says who did which.
pub fn request_leave_on(
    app: &App,
    staff_id: String,
    leave_type_id: String,
    from: String,
    to: String,
    half_days: i32,
    reason: String,
) -> UiResult<LeaveView> {
    let session = app
        .sessions()
        .current()
        .ok_or_else(|| UiError::new("auth.locked", "The screen is locked. Sign in to carry on."))?;
    let may_approve = guard::require(app, Permission::LeaveApprove).is_ok();

    let asking_for = if staff_id.trim().is_empty() {
        session.actor.staff_id.as_str().to_owned()
    } else {
        staff_id
    };
    if !may_approve && asking_for != session.actor.staff_id.as_str() {
        return Err(UiError::new(
            "leave.not_yours",
            "You can ask for your own leave. Asking on somebody else's behalf \
             needs permission.",
        ));
    }
    if reason.trim().is_empty() {
        return Err(UiError::new("leave.reason", "Say why the leave is needed."));
    }
    if half_days <= 0 {
        return Err(UiError::new(
            "leave.days",
            "Leave is at least half a day.",
        ));
    }

    let from_day = parse_day(&from, "leave.from")?;
    let to_day = parse_day(&to, "leave.to")?;
    if to_day < from_day {
        return Err(UiError::new(
            "leave.to",
            "The last day cannot be before the first one.",
        ));
    }

    let at = now();
    let day = today(at);
    let id = crate::newid::fresh_at("lvr", at);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);

                // **T3 — no two requests over the same days.** Checked against
                // what is PENDING as well as what is approved: two overlapping
                // requests waiting for a decision is the same mess arriving a
                // day later.
                let mut existing =
                    repos.employment().approved_between(OUTLET, from_day, to_day)?;
                existing.extend(repos.employment().pending_requests(OUTLET)?);
                if let Some(clash) = existing.iter().find(|r| {
                    r.staff_id == asking_for
                        && employment::overlaps((from_day, to_day), (r.from_day, r.to_day))
                }) {
                    return Err(mb_db::DbError::invariant(format!(
                        "there is already leave from {} to {}",
                        day_words(clash.from_day),
                        day_words(clash.to_day)
                    )));
                }

                repos.employment().save_leave_request(
                    OUTLET,
                    &LeaveRequest {
                        id: id.clone(),
                        staff_id: asking_for.clone(),
                        leave_type_id: leave_type_id.clone(),
                        from_day,
                        to_day,
                        half_days: HalfDays::new(half_days),
                        reason: reason.trim().to_owned(),
                        state: RequestState::Pending,
                        requested_at: at,
                        requested_by: Some(session.actor.staff_id.as_str().to_owned()),
                        decided_at: None,
                        decided_by: None,
                        decision_note: None,
                    },
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(session.actor.staff_id.clone()),
                        action::LEAVE_REQUESTED,
                        "leave_request",
                    )
                    .about(id.clone())
                    .with_after(serde_json::json!({
                        "for": asking_for,
                        "from": day_words(from_day),
                        "to": day_words(to_day),
                        "half_days": half_days,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    leave_on(app, Some(asking_for))
}

/// **Approve or reject.** Approving writes the one `taken` row; rejecting
/// writes none, so a rejected request cannot move a balance even by accident.
pub fn decide_leave_on(
    app: &App,
    id: String,
    approve: bool,
    note: String,
) -> UiResult<LeaveView> {
    let who = guard::require(app, Permission::LeaveApprove)?;
    let at = now();
    let day = today(at);

    if !approve && note.trim().is_empty() {
        return Err(UiError::new(
            "leave.note",
            "Say why it is refused. A rejection without a reason is one nobody \
             can appeal.",
        ));
    }

    let staff_id = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mut request) = repos
                    .employment()
                    .pending_requests(OUTLET)?
                    .into_iter()
                    .find(|r| r.id == id)
                else {
                    // Either it does not exist or somebody has already decided
                    // it. Both are the same refusal from where the person is
                    // standing, and saying which would leak whose it was.
                    return Err(mb_db::DbError::invariant(
                        "that request is not waiting for a decision".to_owned(),
                    ));
                };

                request.state = if approve {
                    RequestState::Approved
                } else {
                    RequestState::Rejected
                };
                request.decided_at = Some(at);
                request.decided_by = Some(who.staff_id.as_str().to_owned());
                request.decision_note = Some(note.trim().to_owned()).filter(|n| !n.is_empty());
                repos.employment().save_leave_request(OUTLET, &request)?;

                if approve {
                    // **The ONE `taken` row.** A partial unique index on
                    // `request_id` means a second approval is a constraint
                    // violation rather than a doubled deduction that somebody
                    // finds in March and cannot explain.
                    repos.employment().post_leave(
                        OUTLET,
                        &crate::newid::fresh_at("lvl", at),
                        &request.staff_id,
                        &request.leave_type_id,
                        LeaveKind::Taken,
                        HalfDays::ZERO - request.half_days,
                        Some(&request.id),
                        None,
                        at,
                        day,
                        Some(who.staff_id.as_str()),
                    )?;
                }

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::LEAVE_DECIDED,
                        "leave_request",
                    )
                    .about(id.clone())
                    .with_after(serde_json::json!({
                        "approved": approve,
                        "note": note.trim(),
                    })),
                )?;
                Ok(request.staff_id)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    leave_on(app, Some(staff_id))
}

/// Grant an entitlement, or correct a balance by hand.
///
/// The freehand row, and therefore the watched one: it carries a required
/// reason for the same reason `stock.adjust` does.
pub fn adjust_leave_on(
    app: &App,
    staff_id: String,
    leave_type_id: String,
    half_days: i32,
    reason: String,
    accrual: bool,
) -> UiResult<LeaveView> {
    let who = guard::require(app, Permission::LeaveApprove)?;
    let at = now();
    let day = today(at);

    if half_days == 0 {
        return Err(UiError::new("leave.days", "Nothing to add or take away."));
    }
    if reason.trim().is_empty() {
        return Err(UiError::new(
            "leave.reason",
            "Say why the balance is being changed.",
        ));
    }
    if accrual && half_days < 0 {
        return Err(UiError::new(
            "leave.days",
            "An entitlement adds days. To take days away, adjust instead.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.employment().post_leave(
                    OUTLET,
                    &crate::newid::fresh_at("lvl", at),
                    &staff_id,
                    &leave_type_id,
                    if accrual {
                        LeaveKind::Accrued
                    } else {
                        LeaveKind::Adjusted
                    },
                    HalfDays::new(half_days),
                    None,
                    Some(reason.trim()),
                    at,
                    day,
                    Some(who.staff_id.as_str()),
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::LEAVE_ADJUSTED,
                        "leave_ledger",
                    )
                    .about(staff_id.clone())
                    .with_after(serde_json::json!({
                        "type": leave_type_id,
                        "half_days": half_days,
                        "reason": reason.trim(),
                        "accrual": accrual,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    leave_on(app, Some(staff_id))
}

// ===========================================================================
// The employment record
// ===========================================================================

/// Scope 9.15. **Nobody is ever deleted** — `left` with a date is the only
/// ending there is, because this person's name is on last year's bills.
pub fn save_employee_on(app: &App, edit: EmployeeEdit) -> UiResult<Vec<EmployeeView>> {
    let who = guard::require(app, Permission::StaffManage)?;
    let at = now();
    let day = today(at);

    if !matches!(
        edit.employment_type.as_str(),
        "full_time" | "part_time" | "casual"
    ) {
        return Err(UiError::new(
            "staff.type",
            "Full-time, part-time or casual.",
        ));
    }

    let left_on = if edit.left_on.trim().is_empty() {
        None
    } else {
        Some(parse_day(&edit.left_on, "staff.left_on")?)
    };

    // **The number somebody rings when there is an accident**, so it is worth
    // the same rule as every other phone in the product. It was stored exactly
    // as typed, which meant it could be a name (owner, 2026-08-22). See
    // `mb_core::phone`.
    let emergency_phone = mb_core::Phone::parse_optional(&edit.emergency_phone)
        .map_err(|e| UiError::new("staff.emergency_phone", e.to_string()))?
        .map(|p| p.as_str().to_owned());

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                // The status and the leaving date travel together — the schema
                // refuses a date on somebody still working, and this is what
                // sets the status so the pair is always consistent.
                let status = if left_on.is_some() { "left" } else { "active" };
                let repos = mb_db::Repos::new(tx);
                repos.employment().save_employment(
                    OUTLET,
                    &edit.id,
                    blank_to_none(&edit.designation).as_deref(),
                    blank_to_none(&edit.department).as_deref(),
                    blank_to_none(&edit.address).as_deref(),
                    blank_to_none(&edit.emergency_name).as_deref(),
                    emergency_phone.as_deref(),
                    blank_to_none(&edit.id_proof).as_deref(),
                    &edit.employment_type,
                    left_on,
                    at,
                )?;

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::STAFF_SAVED,
                        "staff",
                    )
                    .about(edit.id.clone())
                    .with_after(serde_json::json!({
                        "designation": edit.designation,
                        "department": edit.department,
                        "employment_type": edit.employment_type,
                        "left_on": edit.left_on,
                        "status": status,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    people_on(app)
}

fn blank_to_none(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

// ===========================================================================
// The roster
// ===========================================================================

/// Say who is expected on a day. `pattern_id` empty is a rostered day OFF,
/// which is a different fact from having no row at all.
pub fn save_roster_on(
    app: &App,
    staff_id: String,
    day: String,
    pattern_id: String,
    note: String,
) -> UiResult<AttendanceView> {
    let who = guard::require(app, Permission::AttendanceMark)?;
    let at = now();
    let on = parse_day(&day, "roster.day")?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.employment().save_roster_day(
                    OUTLET,
                    &RosterDay {
                        id: format!("ros_{}_{}", staff_id, on.days_since_epoch()),
                        staff_id: staff_id.clone(),
                        day: on,
                        pattern_id: blank_to_none(&pattern_id),
                        note: blank_to_none(&note),
                    },
                    at,
                    Some(who.staff_id.as_str()),
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        today(at),
                        Some(who.staff_id.clone()),
                        action::ROSTER_CHANGED,
                        "roster",
                    )
                    .about(staff_id.clone())
                    .with_after(serde_json::json!({
                        "day": day_words(on),
                        "pattern": pattern_id,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    attendance_on(app, Some(staff_id), day_words(on), day_words(on))
}

// ===========================================================================
// Salary and advances
// ===========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SalaryEdit {
    pub staff_id: String,
    /// The date the new figure starts from. **A raise is a NEW ROW**, never an
    /// edit — that is what lets last month recompute to what it printed.
    pub effective_from: String,
    pub basis: String,
    /// Typed by a person, parsed in Rust (D39).
    pub amount: String,
    pub components: Vec<ComponentEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ComponentEdit {
    pub name: String,
    pub kind: String,
    pub amount: String,
}

pub fn save_salary_on(app: &App, edit: SalaryEdit) -> UiResult<SalaryView> {
    let who = guard::require(app, Permission::SalaryManage)?;
    let at = now();
    let day = today(at);

    let basis = basis_from_tag(&edit.basis)?;
    let amount = crate::menu::parse_money_public(&edit.amount)?;
    if amount.is_negative() {
        return Err(UiError::new("salary.amount", "A salary is not less than nothing."));
    }
    let from = parse_day(&edit.effective_from, "salary.effective_from")?;

    let mut components = Vec::new();
    for c in &edit.components {
        if c.name.trim().is_empty() {
            continue;
        }
        let value = crate::menu::parse_money_public(&c.amount)?;
        if !value.is_positive() {
            return Err(UiError::new(
                "salary.component",
                format!("{} has to be more than nothing.", c.name.trim()),
            ));
        }
        components.push(Component {
            name: c.name.trim().to_owned(),
            kind: match c.kind.as_str() {
                "deduction" => ComponentKind::Deduction,
                _ => ComponentKind::Allowance,
            },
            amount: value,
        });
    }

    let structure = Structure {
        effective_from: from,
        basis,
        amount,
        components,
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let id = format!("sal_{}_{}", edit.staff_id, from.days_since_epoch());
                repos.employment().save_structure(
                    OUTLET,
                    &id,
                    &edit.staff_id,
                    &structure,
                    at,
                    Some(who.staff_id.as_str()),
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::SALARY_SET,
                        "salary_structure",
                    )
                    .about(id)
                    .with_after(serde_json::json!({
                        "staff": edit.staff_id,
                        "from": day_words(from),
                        "basis": edit.basis,
                        "amount_paise": amount.paise(),
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    salary_on(app, edit.staff_id)
}

/// **Give an advance.** Money out of the drawer today, recovered from the next
/// run — see the module note on why this is a `payout` and not an expense.
pub fn give_advance_on(
    app: &App,
    staff_id: String,
    amount: String,
    instalments: i32,
    reason: String,
) -> UiResult<SalaryView> {
    let who = guard::require(app, Permission::SalaryManage)?;
    let at = now();
    let day = today(at);

    let value = crate::menu::parse_money_public(&amount)?;
    if !value.is_positive() {
        return Err(UiError::new(
            "advance.amount",
            "An advance is more than nothing.",
        ));
    }
    let instalments = instalments.max(1);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let name = repos
                    .employment()
                    .names(OUTLET)?
                    .get(&staff_id)
                    .cloned()
                    .unwrap_or_default();

                // **The drawer, today.** An advance that only appears at month
                // end is a drawer that is short all month and nobody knows why.
                let movement_id = crate::newid::fresh_at("cm_adv", at);
                repos.money().save_cash_movement(
                    OUTLET,
                    &CashMovement {
                        id: movement_id.clone(),
                        kind: "payout".to_owned(),
                        amount: value,
                        reason: format!("Salary advance — {name}"),
                        at,
                        business_day: day,
                        moved_by: Some(who.staff_id.clone()),
                    },
                )?;

                let id = crate::newid::fresh_at("adv", at);
                repos.employment().save_advance(
                    OUTLET,
                    &id,
                    &staff_id,
                    value,
                    instalments,
                    blank_to_none(&reason).as_deref(),
                    at,
                    day,
                    Some(who.staff_id.as_str()),
                    Some(&movement_id),
                )?;

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::ADVANCE_GIVEN,
                        "salary_advance",
                    )
                    .about(id)
                    .with_after(serde_json::json!({
                        "staff": staff_id,
                        "amount_paise": value.paise(),
                        "instalments": instalments,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    salary_on(app, staff_id)
}

// ===========================================================================
// Payroll
// ===========================================================================

/// **Work out a run.** Computes, stores as a DRAFT, and changes no money.
///
/// Recomputing an existing draft is allowed and is the ordinary thing: an owner
/// fixes somebody's attendance and asks for the figures again. The recoveries
/// are cleared first, or the second computation would recover the same advance
/// twice.
pub fn compute_payroll_on(app: &App, from: String, to: String) -> UiResult<PayrollView> {
    let who = guard::require(app, Permission::SalaryManage)?;
    let at = now();
    let today_day = today(at);

    let from_day = parse_day(&from, "payroll.from")?;
    let to_day = parse_day(&to, "payroll.to")?;
    if to_day < from_day {
        return Err(UiError::new(
            "payroll.to",
            "The last day cannot be before the first one.",
        ));
    }

    // Calendar days, inclusive — **D148**. The denominator for a monthly
    // salary's unpaid-leave deduction, and the shop's period decides it rather
    // than a fixed 30 that would be wrong eight months a year.
    let period_days = from_day.days_until(to_day) + 1;

    let run_id = format!("pay_{}_{}", from_day.days_since_epoch(), to_day.days_since_epoch());

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);

                if repos
                    .employment()
                    .run(OUTLET, &run_id)?
                    .is_some_and(|existing| existing.state != RunState::Draft)
                {
                    return Err(mb_db::DbError::invariant(
                        "that period has already been approved — reverse it first".to_owned(),
                    ));
                }
                repos.employment().clear_recoveries(&run_id)?;

                let attendance = repos
                    .employment()
                    .attendance_between(OUTLET, from_day, to_day)?;
                let approved = repos.employment().approved_between(OUTLET, from_day, to_day)?;
                let types = repos.employment().list_leave_types(OUTLET)?;

                let mut lines = Vec::new();
                let mut recoveries = Vec::new();

                let people = repos.employment().employed_on_or_after(OUTLET, from_day)?;

                for (staff_id, _name) in people {
                    let structures = repos.employment().structures_for(OUTLET, &staff_id)?;
                    // The structure that applied on the LAST day of the period.
                    // A raise dated inside the period therefore applies to the
                    // whole of it, which is what a shop means by "from this
                    // month" — and T5 proves the previous month is untouched.
                    let Some(structure) = employment::structure_on(&structures, to_day) else {
                        continue;
                    };

                    let mut days = HalfDays::ZERO;
                    let mut minutes = 0_i64;
                    for a in attendance.iter().filter(|a| a.staff_id == staff_id) {
                        let Some(ended) = a.ended_at else {
                            // Still open. Its hours cannot be worked out, and
                            // guessing would be inventing pay.
                            continue;
                        };
                        days = days + HalfDays::from_days(1);
                        minutes += employment::minutes_between(
                            minute_of(a.started_at),
                            minute_of(ended),
                        );
                    }

                    let unpaid = approved
                        .iter()
                        .filter(|r| {
                            r.staff_id == staff_id
                                && types
                                    .iter()
                                    .find(|t| t.id == r.leave_type_id)
                                    .is_some_and(|t| !t.is_paid)
                        })
                        .map(|r| r.half_days)
                        .fold(HalfDays::ZERO, |a, b| a + b);

                    let worked = Worked {
                        days,
                        minutes,
                        unpaid,
                        period_days,
                    };

                    // What could be recovered is what the line is worth BEFORE
                    // any recovery — computed once with nothing recovered, so
                    // the cap is the real one.
                    let dry = employment::pay_line(structure, &worked, Money::ZERO)
                        .map_err(|e| mb_db::DbError::invariant(format!("payroll: {e}")))?;

                    let advances = repos.employment().advances_for(OUTLET, &staff_id)?;
                    let taking = employment::recover_advances(&advances, dry.net)
                        .map_err(|e| mb_db::DbError::invariant(format!("advances: {e}")))?;
                    let recovered = Money::try_sum(taking.iter().map(|r| r.amount))
                        .map_err(|e| mb_db::DbError::invariant(format!("advances: {e}")))?;

                    let line = employment::pay_line(structure, &worked, recovered)
                        .map_err(|e| mb_db::DbError::invariant(format!("payroll: {e}")))?;

                    for r in taking {
                        recoveries.push((staff_id.clone(), r));
                    }

                    lines.push(PayrollLine {
                        id: format!("pln_{run_id}_{staff_id}"),
                        staff_id: staff_id.clone(),
                        basis: line.basis,
                        basis_amount: line.basis_amount,
                        days_worked: line.days_worked,
                        minutes_worked: line.minutes_worked,
                        unpaid: line.unpaid,
                        earned: line.earned,
                        allowances: line.allowances,
                        deductions: line.deductions,
                        unpaid_leave_deduction: line.unpaid_leave_deduction,
                        advance_recovered: line.advance_recovered,
                        net: line.net,
                        edited: false,
                        note: None,
                    });
                }

                repos.employment().save_run(
                    OUTLET,
                    &PayrollRun {
                        id: run_id.clone(),
                        from_day,
                        to_day,
                        state: RunState::Draft,
                        computed_at: at,
                        computed_by: Some(who.staff_id.as_str().to_owned()),
                        approved_at: None,
                        approved_by: None,
                        cash_movement_id: None,
                        expense_id: None,
                        paid_by: "cash".to_owned(),
                        reversed_at: None,
                        reversal_reason: None,
                        note: None,
                    },
                )?;
                repos.employment().replace_lines(&run_id, &lines)?;

                for (_staff, r) in &recoveries {
                    repos.employment().save_recovery(
                        &format!("rec_{run_id}_{}", r.advance_id),
                        &r.advance_id,
                        &run_id,
                        r.amount,
                    )?;
                }

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        today_day,
                        Some(who.staff_id.clone()),
                        action::PAYROLL_COMPUTED,
                        "payroll_run",
                    )
                    .about(run_id.clone())
                    .with_after(serde_json::json!({
                        "from": day_words(from_day),
                        "to": day_words(to_day),
                        "people": lines.len(),
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    payroll_on(app, run_id)
}

/// One run, with its lines.
pub fn payroll_on(app: &App, run_id: String) -> UiResult<PayrollView> {
    guard::require(app, Permission::SalaryView)?;
    let may_manage = guard::require(app, Permission::SalaryManage).is_ok();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let names = staff_names(&repos)?;
                let Some(run) = repos.employment().run(OUTLET, &run_id)? else {
                    return Err(mb_db::DbError::invariant(
                        "there is no payroll run with that name".to_owned(),
                    ));
                };
                let lines = repos.employment().lines_of(&run_id)?;
                let total = Money::try_sum(lines.iter().map(|l| l.net))
                    .map_err(|e| mb_db::DbError::invariant(format!("payroll: {e}")))?;

                let says = match run.state {
                    RunState::Draft => format!(
                        "A draft. Check every line — nothing has left the drawer yet. \
                         {} people, {} in total.",
                        lines.len(),
                        total.to_indian_string()
                    ),
                    RunState::Approved => format!(
                        "Approved. {} went out as one salary expense, and the cash \
                         position already knows.",
                        total.to_indian_string()
                    ),
                    RunState::Reversed => "Reversed. The expense and the drawer row were \
                                           taken back, and every advance owes what it owed."
                        .to_owned(),
                };

                Ok(PayrollView {
                    id: run.id.clone(),
                    from: day_words(run.from_day),
                    to: day_words(run.to_day),
                    state: run.state.as_sql().to_owned(),
                    paid_by: run.paid_by.clone(),
                    lines: lines
                        .iter()
                        .map(|l| PayLineView {
                            id: l.id.clone(),
                            staff_id: l.staff_id.clone(),
                            staff_name: names.get(&l.staff_id).cloned().unwrap_or_default(),
                            basis: basis_tag(l.basis).to_owned(),
                            basis_amount: money(l.basis_amount),
                            days_says: l.days_worked.say(),
                            unpaid_says: l.unpaid.say(),
                            earned: money(l.earned),
                            allowances: money(l.allowances),
                            deductions: money(l.deductions),
                            unpaid_leave_deduction: money(l.unpaid_leave_deduction),
                            advance_recovered: money(l.advance_recovered),
                            net: money(l.net),
                            edited: l.edited,
                        })
                        .collect(),
                    total: money(total),
                    says,
                    may_manage,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}


/// **Put one person's payslip on paper** — scope 9.14's third part, P30.
///
/// P28 built the payroll and named this as not done. It is the piece the
/// person being paid actually holds, and a shop that cannot hand one over
/// settles every argument about pay by memory.
///
/// Printed like every other document, through P07's queue — so a printer that
/// is off means a slip that prints when it comes back, exactly as a bill does.
/// **`SalaryManage`, not `SalaryView`**: reading what somebody earns and
/// handing them the paper are the same authority as approving the run that
/// produced it, and this is the one that ends up in somebody's pocket.
pub fn print_payslip_on(app: &App, run_id: String, staff_id: String) -> UiResult<String> {
    guard::require(app, Permission::SalaryManage)?;
    let at = now();
    let printer = crate::flows::default_printer(app)?;
    let config = app.shop_config();

    let run = payroll_on(app, run_id.clone())?;
    let line = run
        .lines
        .iter()
        .find(|l| l.staff_id == staff_id)
        .ok_or_else(|| {
            UiError::new(
                "payroll.no_line",
                "That person is not on this payroll run.",
            )
        })?;

    let people = people_on(app)?;
    let person = people.iter().find(|p| p.id == staff_id);
    let designation = person
        .and_then(|p| p.designation.clone())
        .filter(|d| !d.trim().is_empty());

    // Every figure, in the order it was worked out. A slip that showed only
    // the net would be a number somebody has to trust.
    let mut lines = vec![mb_print::template::PaySlipLine {
        label: "Earned".to_owned(),
        amount: line.earned.text.clone(),
        takes_away: false,
    }];
    if line.allowances.paise > 0 {
        lines.push(mb_print::template::PaySlipLine {
            label: "Allowances".to_owned(),
            amount: line.allowances.text.clone(),
            takes_away: false,
        });
    }
    if line.deductions.paise > 0 {
        lines.push(mb_print::template::PaySlipLine {
            label: "Deductions".to_owned(),
            amount: line.deductions.text.clone(),
            takes_away: true,
        });
    }
    if line.unpaid_leave_deduction.paise > 0 {
        lines.push(mb_print::template::PaySlipLine {
            label: "Unpaid leave".to_owned(),
            amount: line.unpaid_leave_deduction.text.clone(),
            takes_away: true,
        });
    }
    if line.advance_recovered.paise > 0 {
        lines.push(mb_print::template::PaySlipLine {
            label: "Advance recovered".to_owned(),
            amount: line.advance_recovered.text.clone(),
            takes_away: true,
        });
    }

    let period = format!("{} to {}", run.from, run.to);
    let worked = if line.unpaid_says.trim().is_empty() {
        format!("{} worked", line.days_says)
    } else {
        format!("{} worked, {} unpaid", line.days_says, line.unpaid_says)
    };
    let paid_by = if run.paid_by.trim().is_empty() {
        "Cash".to_owned()
    } else {
        run.paid_by.clone()
    };

    let document = mb_print::template::payslip_document(
        printer.paper,
        &mb_print::template::PayslipContext {
            shop: &config.store.name,
            person: &line.staff_name,
            designation: designation.as_deref(),
            period: &period,
            basis_says: &basis_words(
                // The tag on the view, back into the enum it came from. A
                // basis this build has never heard of would be a payroll row
                // written by a newer version, and "monthly" is the honest
                // fallback for a slip somebody is holding.
                basis_from_tag(&line.basis).unwrap_or(Basis::Monthly),
                Money::from_paise(line.basis_amount.paise),
            ),
            worked_says: &worked,
            lines: &lines,
            net: &line.net.text,
            paid_by: &paid_by,
            edited: line.edited,
        },
    );

    app.print(mb_print::queue::Job::new(
                    mb_print::queue::JobKind::DayClose,
                    &printer.id,
                    document,
                    today(at),
                )
                .because(format!("payslip for {}", line.staff_name)),)
}

pub fn payroll_list_on(app: &App) -> UiResult<PayrollListView> {
    guard::require(app, Permission::SalaryView)?;
    let may_manage = guard::require(app, Permission::SalaryManage).is_ok();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut runs = Vec::new();
                for run in repos.employment().list_runs(OUTLET)? {
                    let lines = repos.employment().lines_of(&run.id)?;
                    let total = Money::try_sum(lines.iter().map(|l| l.net))
                        .map_err(|e| mb_db::DbError::invariant(format!("payroll: {e}")))?;
                    runs.push(PayrollRunView {
                        id: run.id.clone(),
                        from: day_words(run.from_day),
                        to: day_words(run.to_day),
                        state: run.state.as_sql().to_owned(),
                        total: money(total),
                        people: u32::try_from(lines.len()).unwrap_or(0),
                    });
                }
                Ok(PayrollListView { runs, may_manage })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// Change one figure by hand before approving.
///
/// The first thing an owner does with a payroll figure is disagree with one
/// line of it. The line is marked `edited` so a reviewer knows which numbers
/// are the computer's and which are somebody's.
pub fn edit_payroll_line_on(
    app: &App,
    run_id: String,
    staff_id: String,
    net: String,
    note: String,
) -> UiResult<PayrollView> {
    let who = guard::require(app, Permission::SalaryManage)?;
    let at = now();
    let value = crate::menu::parse_money_public(&net)?;
    if value.is_negative() {
        return Err(UiError::new("payroll.net", "A payslip is not less than nothing."));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(run) = repos.employment().run(OUTLET, &run_id)? else {
                    return Err(mb_db::DbError::invariant("no such run".to_owned()));
                };
                if run.state != RunState::Draft {
                    return Err(mb_db::DbError::invariant(
                        "an approved run cannot be edited — reverse it first".to_owned(),
                    ));
                }
                let mut lines = repos.employment().lines_of(&run_id)?;
                let Some(line) = lines.iter_mut().find(|l| l.staff_id == staff_id) else {
                    return Err(mb_db::DbError::invariant(
                        "that person is not in this run".to_owned(),
                    ));
                };
                let before = line.net;
                line.net = value;
                line.edited = true;
                line.note = blank_to_none(&note);
                repos.employment().replace_lines(&run_id, &lines)?;

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        today(at),
                        Some(who.staff_id.clone()),
                        action::PAYROLL_COMPUTED,
                        "payroll_line",
                    )
                    .about(format!("{run_id}/{staff_id}"))
                    .changed(
                        serde_json::json!({ "net_paise": before.paise() }),
                        serde_json::json!({ "net_paise": value.paise(), "note": note.trim() }),
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    payroll_on(app, run_id)
}

/// **Approve a run.** This is where money leaves the shop.
///
/// One expense for the gross, one compensating `top_up` for whatever came off
/// advances, and the audit row — all in ONE transaction (D82). See the module
/// note for why it is those two rows and not one or three.
pub fn approve_payroll_on(app: &App, run_id: String, paid_by: String) -> UiResult<PayrollView> {
    let who = guard::require(app, Permission::SalaryManage)?;
    let at = now();
    let day = today(at);

    if !matches!(paid_by.as_str(), "cash" | "bank") {
        return Err(UiError::new("payroll.paid_by", "Pay it in cash or by bank."));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mut run) = repos.employment().run(OUTLET, &run_id)? else {
                    return Err(mb_db::DbError::invariant("no such run".to_owned()));
                };
                // **T7 — approving twice is refused**, and it is refused here
                // rather than tolerated, because the second approval would post
                // a second expense for the same month.
                if run.state != RunState::Draft {
                    return Err(mb_db::DbError::invariant(
                        "that run has already been approved".to_owned(),
                    ));
                }

                let lines = repos.employment().lines_of(&run_id)?;
                if lines.is_empty() {
                    return Err(mb_db::DbError::invariant(
                        "there is nobody in that run".to_owned(),
                    ));
                }

                // **The COST is the gross**: what each person earned, before
                // an advance they already took is deducted from what they are
                // handed. Netting it would understate the shop's wage bill by
                // every advance ever given.
                let gross = Money::try_sum(
                    lines
                        .iter()
                        .map(|l| l.net.add(l.advance_recovered).unwrap_or(l.net)),
                )
                .map_err(|e| mb_db::DbError::invariant(format!("payroll: {e}")))?;
                let recovered = Money::try_sum(lines.iter().map(|l| l.advance_recovered))
                    .map_err(|e| mb_db::DbError::invariant(format!("payroll: {e}")))?;

                let expense_id = crate::newid::fresh_at("exp_pay", at);
                repos.money().save_expense(
                    OUTLET,
                    &Expense {
                        id: expense_id.clone(),
                        category_id: Some(SALARY_CATEGORY.to_owned()),
                        description: format!(
                            "Salary {} to {}",
                            day_words(run.from_day),
                            day_words(run.to_day)
                        ),
                        amount: gross,
                        mode: paid_by.clone(),
                        paid_to: None,
                        reference: Some(run_id.clone()),
                        gst_rate_bp: None,
                        gst_amount: None,
                        paid_at: at,
                        paid_by: Some(who.staff_id.clone()),
                        business_day: day,
                        note: None,
                    },
                )?;

                // The compensating drawer row. Only when something was actually
                // recovered — a run with no advances writes one row, not two.
                let movement_id = if recovered.is_positive() {
                    let id = crate::newid::fresh_at("cm_pay", at);
                    repos.money().save_cash_movement(
                        OUTLET,
                        &CashMovement {
                            id: id.clone(),
                            kind: "top_up".to_owned(),
                            amount: recovered,
                            reason: format!(
                                "Advances recovered in salary {} to {}",
                                day_words(run.from_day),
                                day_words(run.to_day)
                            ),
                            at,
                            business_day: day,
                            moved_by: Some(who.staff_id.clone()),
                        },
                    )?;
                    Some(id)
                } else {
                    None
                };

                run.state = RunState::Approved;
                run.approved_at = Some(at);
                run.approved_by = Some(who.staff_id.as_str().to_owned());
                run.expense_id = Some(expense_id.clone());
                run.cash_movement_id = movement_id;
                run.paid_by = paid_by.clone();
                repos.employment().save_run(OUTLET, &run)?;

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::PAYROLL_APPROVED,
                        "payroll_run",
                    )
                    .about(run_id.clone())
                    .with_after(serde_json::json!({
                        "gross_paise": gross.paise(),
                        "recovered_paise": recovered.paise(),
                        "paid_by": paid_by,
                        "expense": expense_id,
                        "people": lines.len(),
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    payroll_on(app, run_id)
}

/// **Reverse an approved run.** A correction is a STATE, not a delete (D47).
///
/// The expense and the drawer row are taken back, and every advance goes back
/// to owing what it owed — because otherwise reversing a run would quietly
/// forgive the money that came off them.
pub fn reverse_payroll_on(app: &App, run_id: String, reason: String) -> UiResult<PayrollView> {
    let who = guard::require(app, Permission::SalaryManage)?;
    let at = now();
    let day = today(at);

    if reason.trim().is_empty() {
        return Err(UiError::new(
            "payroll.reason",
            "Say why the run is being reversed.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mut run) = repos.employment().run(OUTLET, &run_id)? else {
                    return Err(mb_db::DbError::invariant("no such run".to_owned()));
                };
                if run.state != RunState::Approved {
                    return Err(mb_db::DbError::invariant(
                        "only an approved run can be reversed".to_owned(),
                    ));
                }

                // **The run lets go of the rows BEFORE they are deleted.**
                //  is a foreign key, so deleting the
                // expense while the run still points at it is a constraint
                // violation — which is what the first version did, and the
                // message a shop would have seen was about a foreign key
                // rather than about anything they had done.
                let expense = run.expense_id.take();
                let movement = run.cash_movement_id.take();
                run.state = RunState::Reversed;
                run.reversed_at = Some(at);
                run.reversal_reason = Some(reason.trim().to_owned());
                repos.employment().save_run(OUTLET, &run)?;

                if let Some(id) = &expense {
                    repos.money().delete_expense(OUTLET, id, at)?;
                }
                if let Some(id) = &movement {
                    repos.employment().delete_cash_movement(OUTLET, id)?;
                }
                // And every advance goes back to owing what it owed —
                // otherwise reversing a run would quietly forgive money.
                repos.employment().clear_recoveries(&run_id)?;

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::PAYROLL_REVERSED,
                        "payroll_run",
                    )
                    .about(run_id.clone())
                    .with_after(serde_json::json!({ "reason": reason.trim() })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    payroll_on(app, run_id)
}

// ===========================================================================
// Staff cost (scope 9.16)
// ===========================================================================

/// **The second of the two numbers that decide whether a restaurant makes
/// money.** P25 gave the first — food cost. This is the other one.
#[allow(
    clippy::integer_division,
    reason = "a percentage in basis points, kept in integers on purpose (D2): 
              the division IS the operation and the remainder is the decimals"
)]
pub fn staff_cost_on(app: &App, from: String, to: String) -> UiResult<StaffCostView> {
    guard::require(app, Permission::SalaryView)?;
    let from_day = parse_day(&from, "cost.from")?;
    let to_day = parse_day(&to, "cost.to")?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let wages = repos
                    .employment()
                    .staff_cost_between(OUTLET, from_day, to_day)?;

                // Revenue over the same window, from the bills themselves — not
                // a second sum kept anywhere, which is G1's whole lesson.
                let revenue = repos.employment().revenue_between(OUTLET, from_day, to_day)?;

                let says = if revenue.is_zero() {
                    "No sales in this period, so there is no percentage to give."
                        .to_owned()
                } else {
                    // Basis points, so the division stays in integers.
                    let bp = i64::from(
                        i32::try_from(
                            i128::from(wages.paise()) * 10_000 / i128::from(revenue.paise()),
                        )
                        .unwrap_or(0),
                    );
                    format!(
                        "Staff cost is {}.{}% of what the shop took.",
                        bp / 100,
                        format_args!("{:02}", bp % 100)
                    )
                };

                Ok(StaffCostView {
                    from: day_words(from_day),
                    to: day_words(to_day),
                    wages: money(wages),
                    revenue: money(revenue),
                    says,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

// ===========================================================================
// The commands
//
// **Thin, every one of them.** The permission check, the words and the
// arithmetic are all above; these exist so Tauri has something to call, and so
// that `ipc.rs` has one list of what this session added.
//
// P28's other half — scope 9.13, the owner managing this from a phone — is the
// SERVICE these wrap, not the wrappers. `docs/LAN_PROTOCOL.md` names the same
// command set over the LAN, and every one of them checks its permission
// server-side (T10), because a phone is a screen and never an authority (D9).
// ===========================================================================

#[tauri::command]
pub fn employees(app: tauri::State<'_, App>) -> UiResult<Vec<EmployeeView>> {
    people_on(&app)
}

#[tauri::command]
pub fn save_employee(
    app: tauri::State<'_, App>,
    edit: EmployeeEdit,
) -> UiResult<Vec<EmployeeView>> {
    save_employee_on(&app, edit)
}

#[tauri::command]
pub fn attendance(
    app: tauri::State<'_, App>,
    staff_id: Option<String>,
    from: String,
    to: String,
) -> UiResult<AttendanceView> {
    attendance_on(&app, staff_id, from, to)
}

#[tauri::command]
pub fn clock_in(
    app: tauri::State<'_, App>,
    terminal_id: Option<String>,
) -> UiResult<AttendanceView> {
    clock_in_on(&app, terminal_id)
}

#[tauri::command]
pub fn clock_out(app: tauri::State<'_, App>) -> UiResult<AttendanceView> {
    clock_out_on(&app)
}

#[tauri::command]
pub fn correct_attendance(
    app: tauri::State<'_, App>,
    id: String,
    started: String,
    ended: String,
    reason: String,
) -> UiResult<AttendanceView> {
    correct_attendance_on(&app, id, started, ended, reason)
}

#[tauri::command]
pub fn save_roster(
    app: tauri::State<'_, App>,
    staff_id: String,
    day: String,
    pattern_id: String,
    note: String,
) -> UiResult<AttendanceView> {
    save_roster_on(&app, staff_id, day, pattern_id, note)
}

#[tauri::command]
pub fn leave(app: tauri::State<'_, App>, staff_id: Option<String>) -> UiResult<LeaveView> {
    leave_on(&app, staff_id)
}

#[tauri::command]
pub fn request_leave(
    app: tauri::State<'_, App>,
    staff_id: String,
    leave_type_id: String,
    from: String,
    to: String,
    half_days: i32,
    reason: String,
) -> UiResult<LeaveView> {
    request_leave_on(&app, staff_id, leave_type_id, from, to, half_days, reason)
}

#[tauri::command]
pub fn decide_leave(
    app: tauri::State<'_, App>,
    id: String,
    approve: bool,
    note: String,
) -> UiResult<LeaveView> {
    decide_leave_on(&app, id, approve, note)
}

#[tauri::command]
pub fn adjust_leave(
    app: tauri::State<'_, App>,
    staff_id: String,
    leave_type_id: String,
    half_days: i32,
    reason: String,
    accrual: bool,
) -> UiResult<LeaveView> {
    adjust_leave_on(&app, staff_id, leave_type_id, half_days, reason, accrual)
}

#[tauri::command]
pub fn salary(app: tauri::State<'_, App>, staff_id: String) -> UiResult<SalaryView> {
    salary_on(&app, staff_id)
}

#[tauri::command]
pub fn save_salary(app: tauri::State<'_, App>, edit: SalaryEdit) -> UiResult<SalaryView> {
    save_salary_on(&app, edit)
}

#[tauri::command]
pub fn give_advance(
    app: tauri::State<'_, App>,
    staff_id: String,
    amount: String,
    instalments: i32,
    reason: String,
) -> UiResult<SalaryView> {
    give_advance_on(&app, staff_id, amount, instalments, reason)
}

#[tauri::command]
pub fn payroll_runs(app: tauri::State<'_, App>) -> UiResult<PayrollListView> {
    payroll_list_on(&app)
}

#[tauri::command]
pub fn payroll(app: tauri::State<'_, App>, run_id: String) -> UiResult<PayrollView> {
    payroll_on(&app, run_id)
}

#[tauri::command]
pub fn compute_payroll(
    app: tauri::State<'_, App>,
    from: String,
    to: String,
) -> UiResult<PayrollView> {
    compute_payroll_on(&app, from, to)
}

#[tauri::command]
pub fn edit_payroll_line(
    app: tauri::State<'_, App>,
    run_id: String,
    staff_id: String,
    net: String,
    note: String,
) -> UiResult<PayrollView> {
    edit_payroll_line_on(&app, run_id, staff_id, net, note)
}

#[tauri::command]
pub fn approve_payroll(
    app: tauri::State<'_, App>,
    run_id: String,
    paid_by: String,
) -> UiResult<PayrollView> {
    approve_payroll_on(&app, run_id, paid_by)
}

#[tauri::command]
pub fn reverse_payroll(
    app: tauri::State<'_, App>,
    run_id: String,
    reason: String,
) -> UiResult<PayrollView> {
    reverse_payroll_on(&app, run_id, reason)
}

/// P30 — scope 9.14's third part.
#[tauri::command]
pub fn print_payslip(
    app: tauri::State<'_, App>,
    run_id: String,
    staff_id: String,
) -> UiResult<String> {
    print_payslip_on(&app, run_id, staff_id)
}

#[tauri::command]
pub fn staff_cost(
    app: tauri::State<'_, App>,
    from: String,
    to: String,
) -> UiResult<StaffCostView> {
    staff_cost_on(&app, from, to)
}
