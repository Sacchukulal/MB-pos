//! **P28 driven end to end** — the real commands, against a real SQLite file.
//!
//! `mb_core::employment` proves the arithmetic and `mb-db` proves the rows.
//! What is proved here is the SEQUENCE, which is where this session can
//! actually hurt a shop:
//!
//! * approving a payroll run moves real money out of a real drawer, and the
//!   cash position has to still add up afterwards (T6);
//! * approving it twice would pay everybody twice (T7);
//! * an advance recovered twice would take a fortnight's wages off somebody
//!   (T8);
//! * a clock-out somebody edited without a trace is how hours get inflated
//!   (T9);
//! * and a night shift on the wrong day makes the payroll and the drawer
//!   disagree about the same evening (T13).
//!
//! None of those is visible from a unit test of a pure function.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_auth::{Actor, Permission, PermissionSet, RolePreset};
use mb_core::businessday::BusinessDay;
use mb_core::employment::HalfDays;
use mb_core::{Money, StaffId};
use mb_db::{Db, DbConfig};

use crate::ipc::{StaffEdit, audit_trail_on, save_staff_member_on};
use crate::employment::{
    EmployeeEdit, SalaryEdit, adjust_leave_on, approve_payroll_on, attendance_on, clock_in_on,
    compute_payroll_on, correct_attendance_on, decide_leave_on, give_advance_on,
    leave_on, payroll_on, people_on, request_leave_on, reverse_payroll_on, salary_on,
    save_employee_on, save_salary_on, staff_cost_on,
};
use crate::signin_tests::Scratch;
use crate::state::App;

// ---------------------------------------------------------------------------
// The fixture
// ---------------------------------------------------------------------------

fn a_shop(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    app
}

/// Sign somebody in with exactly these permissions — which is how every
/// refusal in this file is provoked: not by hiding a button, but by calling the
/// command with a person who genuinely may not.
fn signed_in_with(app: &App, id: &str, name: &str, permissions: PermissionSet) {
    app.sessions().begin(
        Actor {
            staff_id: StaffId::new(id),
            name: name.to_owned(),
            role_id: None,
            role_name: None,
            permissions,
            max_discount_bp: None,
            max_discount: None,
        },
        crate::flows::now(),
        false,
    );
}

/// The owner, signed in — **and on the staff list**, which is not a detail:
/// every row this session writes carries who did it, so an actor whose id is
/// not in `staff` is a foreign-key violation the moment they touch anything.
/// The first version of this fixture forgot, and twelve tests failed at once
/// with the same message.
fn as_owner(app: &App, id: &str, name: &str) {
    hire(app, id, name);
    signed_in_with(app, id, name, PermissionSet::everything());
}

fn only(permissions: &[Permission]) -> PermissionSet {
    permissions.iter().copied().collect()
}

/// Put somebody on the staff list the way the Staff screen does.
fn hire(app: &App, id: &str, name: &str) {
    save_staff_member_on(
        app,
        StaffEdit {
            id: id.to_owned(),
            name: name.to_owned(),
            code: None,
            role_id: Some(RolePreset::Cashier.id().to_owned()),
            status: "active".to_owned(),
        },
    )
    .expect("hired");
}

fn today() -> BusinessDay {
    crate::flows::today(crate::flows::now())
}

fn day_text(day: BusinessDay) -> String {
    let (y, m, d) = day.to_ymd();
    format!("{y:04}-{m:02}-{d:02}")
}

fn rupees(n: i64) -> Money {
    Money::from_paise(n * 100)
}

// ---------------------------------------------------------------------------
// T9 — a correction is watched
// ---------------------------------------------------------------------------

/// **T9.** A missed clock-out corrected by a manager writes an audit row with
/// a before AND an after — and a person can never correct their own.
///
/// This is the one control in attendance that matters. Hours somebody can edit
/// themselves, silently, are hours nobody can rely on.
#[test]
fn correcting_somebody_elses_hours_is_recorded_and_your_own_is_refused() {
    let scratch = Scratch::new("emp_correct");
    let app = a_shop(&scratch, "correct");
    hire(&app, "staff_ravi", "Ravi");
    hire(&app, "staff_boss", "Meena");

    // Ravi clocks in and never clocks out.
    signed_in_with(&app, "staff_ravi", "Ravi", only(&[]));
    clock_in_on(&app, None).expect("clocked in");

    // The manager fixes it.
    signed_in_with(
        &app,
        "staff_boss",
        "Meena",
        only(&[
            Permission::AttendanceCorrect,
            Permission::AttendanceMark,
            // To read the history back and check what the correction wrote.
            Permission::AuditView,
        ]),
    );
    let day = day_text(today());
    let view = attendance_on(&app, Some("staff_ravi".to_owned()), day.clone(), day.clone())
        .expect("read");
    let shift = view.shifts.first().expect("a shift");

    let after = correct_attendance_on(
        &app,
        shift.id.clone(),
        "09:00".to_owned(),
        "17:30".to_owned(),
        "Forgot to clock out".to_owned(),
    )
    .expect("corrected");

    let fixed = after.shifts.first().expect("still there");
    assert_eq!(fixed.started, "09:00");
    assert_eq!(fixed.ended, "17:30");
    assert_eq!(fixed.worked, "8h 30m");
    assert!(fixed.corrected, "the row must SAY it was corrected (D47)");

    // The audit row carries both sides.
    let history = audit_trail_on(&app, None, None, None).expect("history");
    let entry = history
        .entries
        .iter()
        .find(|r| r.what.contains("hours"))
        .expect("an audit row for the correction");
    assert!(
        entry.before.is_some(),
        "a correction with no BEFORE is not a correction anybody can check"
    );
    assert!(entry.after.is_some());

    // **And nobody corrects their own.** No permission can express this — the
    // rule is about WHOSE row it is — so the command enforces it.
    signed_in_with(
        &app,
        "staff_ravi",
        "Ravi",
        only(&[Permission::AttendanceCorrect]),
    );
    let own = correct_attendance_on(
        &app,
        shift.id.clone(),
        "08:00".to_owned(),
        "20:00".to_owned(),
        "I was here longer".to_owned(),
    );
    assert!(own.is_err(), "a person corrected their own hours");
}

// ---------------------------------------------------------------------------
// T13 — a night shift belongs to the day it started in
// ---------------------------------------------------------------------------

/// **T13, and it is D5 applied to a shift.**
///
/// A shift that starts at 22:00 and ends at 02:30 belongs to the day it
/// STARTED in — every hour of it. Re-deriving the day from the clock-out would
/// put half a night shift on tomorrow, and tomorrow's report would then
/// disagree with the drawer that was counted at the end of it.
#[test]
fn a_night_shift_belongs_to_the_day_it_started_in() {
    let scratch = Scratch::new("emp_night");
    let app = a_shop(&scratch, "night");
    hire(&app, "staff_ravi", "Ravi");
    hire(&app, "staff_boss", "Meena");

    signed_in_with(&app, "staff_ravi", "Ravi", only(&[]));
    clock_in_on(&app, None).expect("clocked in");

    signed_in_with(
        &app,
        "staff_boss",
        "Meena",
        only(&[Permission::AttendanceCorrect, Permission::AttendanceMark]),
    );
    let day = today();
    let text = day_text(day);
    let view =
        attendance_on(&app, Some("staff_ravi".to_owned()), text.clone(), text.clone())
            .expect("read");
    let shift = view.shifts.first().expect("a shift").id.clone();

    // 22:00 to 02:30 — the clock-out is BEFORE the clock-in on the face.
    let after = correct_attendance_on(
        &app,
        shift,
        "22:00".to_owned(),
        "02:30".to_owned(),
        "Night shift".to_owned(),
    )
    .expect("corrected");

    let fixed = after.shifts.first().expect("still there");
    assert_eq!(
        fixed.day, text,
        "the shift moved off the day it started in"
    );
    assert_eq!(
        fixed.worked, "4h 30m",
        "a wrapping shift came out as something other than four and a half hours"
    );

    // And it is NOT on tomorrow.
    let tomorrow = day_text(day.next());
    let next = attendance_on(
        &app,
        Some("staff_ravi".to_owned()),
        tomorrow.clone(),
        tomorrow,
    )
    .expect("read");
    assert!(
        next.shifts.is_empty(),
        "half the night shift landed on tomorrow"
    );
}

// ---------------------------------------------------------------------------
// T2 and T3 — leave
// ---------------------------------------------------------------------------

/// **T3.** Two requests over the same days are refused, in a sentence.
#[test]
fn overlapping_leave_is_refused() {
    let scratch = Scratch::new("emp_overlap");
    let app = a_shop(&scratch, "overlap");
    hire(&app, "staff_ravi", "Ravi");
    as_owner(&app, "staff_boss", "Meena");

    let day = today();
    request_leave_on(
        &app,
        "staff_ravi".to_owned(),
        "lv_casual".to_owned(),
        day_text(day),
        day_text(day.next().next()),
        6,
        "Family".to_owned(),
    )
    .expect("asked");

    let clash = request_leave_on(
        &app,
        "staff_ravi".to_owned(),
        "lv_casual".to_owned(),
        day_text(day.next()),
        day_text(day.next().next().next()),
        6,
        "Also family".to_owned(),
    );
    let error = clash.expect_err("two lots of leave over the same days were allowed");
    assert!(
        !error.message.is_empty(),
        "the refusal has to be a sentence somebody can read"
    );

    // Somebody ELSE over the same days is fine — the clash is per person.
    hire(&app, "staff_priya", "Priya");
    request_leave_on(
        &app,
        "staff_priya".to_owned(),
        "lv_casual".to_owned(),
        day_text(day),
        day_text(day.next()),
        4,
        "Wedding".to_owned(),
    )
    .expect("a different person may be away too");
}

/// **T1 at the command level, and T2.**
///
/// Approving writes exactly one `taken` row, the balance is the sum of the
/// ledger, and **approving the same request twice is refused** — which is the
/// guard that stops a doubled deduction somebody finds in March.
#[test]
fn approving_leave_moves_the_balance_once_and_only_once() {
    let scratch = Scratch::new("emp_leave");
    let app = a_shop(&scratch, "leave");
    hire(&app, "staff_ravi", "Ravi");
    as_owner(&app, "staff_boss", "Meena");

    // Twelve days of casual leave, granted.
    adjust_leave_on(
        &app,
        "staff_ravi".to_owned(),
        "lv_casual".to_owned(),
        24,
        "Yearly entitlement".to_owned(),
        true,
    )
    .expect("granted");

    let before = leave_on(&app, Some("staff_ravi".to_owned())).expect("read");
    let casual = before
        .balances
        .iter()
        .find(|b| b.leave_type_id == "lv_casual")
        .expect("casual leave");
    assert_eq!(casual.left_says, "12 days");

    let day = today();
    let view = request_leave_on(
        &app,
        "staff_ravi".to_owned(),
        "lv_casual".to_owned(),
        day_text(day),
        day_text(day.next()),
        4,
        "Village".to_owned(),
    )
    .expect("asked");
    let request = view.requests.first().expect("a request").id.clone();

    let after = decide_leave_on(&app, request.clone(), true, String::new()).expect("approved");
    let casual = after
        .balances
        .iter()
        .find(|b| b.leave_type_id == "lv_casual")
        .expect("casual leave");
    assert_eq!(casual.left_says, "10 days", "two days should have come off");

    // **Approving it again is refused.** The request is no longer pending, and
    // even if it reached the ledger the partial unique index would refuse the
    // second `taken` row.
    let again = decide_leave_on(&app, request, true, String::new());
    assert!(again.is_err(), "the same leave was approved twice");

    let still = leave_on(&app, Some("staff_ravi".to_owned())).expect("read");
    let casual = still
        .balances
        .iter()
        .find(|b| b.leave_type_id == "lv_casual")
        .expect("casual leave");
    assert_eq!(casual.left_says, "10 days", "the balance moved twice");
}

/// A rejection needs a reason, and moves nothing.
#[test]
fn a_rejection_needs_a_reason_and_changes_no_balance() {
    let scratch = Scratch::new("emp_reject");
    let app = a_shop(&scratch, "reject");
    hire(&app, "staff_ravi", "Ravi");
    as_owner(&app, "staff_boss", "Meena");

    adjust_leave_on(
        &app,
        "staff_ravi".to_owned(),
        "lv_casual".to_owned(),
        24,
        "Yearly entitlement".to_owned(),
        true,
    )
    .expect("granted");

    let day = today();
    let view = request_leave_on(
        &app,
        "staff_ravi".to_owned(),
        "lv_casual".to_owned(),
        day_text(day),
        day_text(day),
        2,
        "Personal".to_owned(),
    )
    .expect("asked");
    let request = view.requests.first().expect("a request").id.clone();

    assert!(
        decide_leave_on(&app, request.clone(), false, String::new()).is_err(),
        "a rejection with no reason is one nobody can appeal"
    );

    let after = decide_leave_on(&app, request, false, "Too busy that week".to_owned())
        .expect("rejected");
    let casual = after
        .balances
        .iter()
        .find(|b| b.leave_type_id == "lv_casual")
        .expect("casual leave");
    assert_eq!(
        casual.left_says, "12 days",
        "a REJECTED request moved a balance"
    );
}

// ---------------------------------------------------------------------------
// T11 — self-service sees only its own
// ---------------------------------------------------------------------------

/// **T11.** A person sees their own attendance and their own leave, and is
/// refused anybody else's — server-side, not by a screen declining to draw it.
#[test]
fn self_service_cannot_read_somebody_elses_anything() {
    let scratch = Scratch::new("emp_self");
    let app = a_shop(&scratch, "self");
    hire(&app, "staff_ravi", "Ravi");
    hire(&app, "staff_priya", "Priya");

    // Ravi has no employment permissions at all — an ordinary cashier.
    signed_in_with(&app, "staff_ravi", "Ravi", only(&[Permission::BillCreate]));
    let day = day_text(today());

    attendance_on(&app, Some("staff_ravi".to_owned()), day.clone(), day.clone())
        .expect("their own hours are theirs");
    leave_on(&app, Some("staff_ravi".to_owned())).expect("their own leave is theirs");

    assert!(
        attendance_on(&app, Some("staff_priya".to_owned()), day.clone(), day).is_err(),
        "a cashier read somebody else's hours"
    );
    assert!(
        leave_on(&app, Some("staff_priya".to_owned())).is_err(),
        "a cashier read somebody else's leave"
    );
    assert!(
        salary_on(&app, "staff_priya".to_owned()).is_err(),
        "a cashier read somebody else's salary"
    );
}

// ---------------------------------------------------------------------------
// T10 — every command is checked server-side
// ---------------------------------------------------------------------------

/// **T10.** Each command, called directly by somebody who may not, is refused.
///
/// Not a sample — each one. A phone is a screen and never an authority (D9),
/// and the whole of scope 9.13 rests on this being true rather than intended.
#[test]
fn every_employment_command_refuses_somebody_without_the_permission() {
    let scratch = Scratch::new("emp_guard");
    let app = a_shop(&scratch, "guard");
    hire(&app, "staff_ravi", "Ravi");
    hire(&app, "staff_priya", "Priya");

    // A waiter: may take an order, and nothing else in this session.
    signed_in_with(&app, "staff_ravi", "Ravi", only(&[Permission::BillCreate]));
    let day = day_text(today());

    assert!(people_on(&app).is_err(), "employees");
    assert!(
        save_employee_on(
            &app,
            EmployeeEdit {
                id: "staff_priya".to_owned(),
                designation: "Cook".to_owned(),
                department: String::new(),
                address: String::new(),
                emergency_name: String::new(),
                emergency_phone: String::new(),
                id_proof: String::new(),
                employment_type: "full_time".to_owned(),
                left_on: String::new(),
            }
        )
        .is_err(),
        "save_employee"
    );
    assert!(
        correct_attendance_on(
            &app,
            "att_x".to_owned(),
            "09:00".to_owned(),
            "17:00".to_owned(),
            "because".to_owned()
        )
        .is_err(),
        "correct_attendance"
    );
    assert!(
        crate::employment::save_roster_on(
            &app,
            "staff_priya".to_owned(),
            day.clone(),
            "shp_morning".to_owned(),
            String::new()
        )
        .is_err(),
        "save_roster"
    );
    assert!(
        decide_leave_on(&app, "lvr_x".to_owned(), true, String::new()).is_err(),
        "decide_leave"
    );
    assert!(
        adjust_leave_on(
            &app,
            "staff_priya".to_owned(),
            "lv_casual".to_owned(),
            10,
            "why not".to_owned(),
            true
        )
        .is_err(),
        "adjust_leave"
    );
    assert!(salary_on(&app, "staff_priya".to_owned()).is_err(), "salary");
    assert!(
        save_salary_on(
            &app,
            SalaryEdit {
                staff_id: "staff_priya".to_owned(),
                effective_from: day.clone(),
                basis: "monthly".to_owned(),
                amount: "50000".to_owned(),
                components: Vec::new(),
            }
        )
        .is_err(),
        "save_salary"
    );
    assert!(
        give_advance_on(
            &app,
            "staff_priya".to_owned(),
            "5000".to_owned(),
            1,
            String::new()
        )
        .is_err(),
        "give_advance"
    );
    assert!(
        compute_payroll_on(&app, day.clone(), day.clone()).is_err(),
        "compute_payroll"
    );
    assert!(payroll_on(&app, "pay_x".to_owned()).is_err(), "payroll");
    assert!(
        crate::employment::payroll_list_on(&app).is_err(),
        "payroll_runs"
    );
    assert!(
        crate::employment::edit_payroll_line_on(
            &app,
            "pay_x".to_owned(),
            "staff_priya".to_owned(),
            "1".to_owned(),
            String::new()
        )
        .is_err(),
        "edit_payroll_line"
    );
    assert!(
        approve_payroll_on(&app, "pay_x".to_owned(), "cash".to_owned()).is_err(),
        "approve_payroll"
    );
    assert!(
        reverse_payroll_on(&app, "pay_x".to_owned(), "because".to_owned()).is_err(),
        "reverse_payroll"
    );
    assert!(
        staff_cost_on(&app, day.clone(), day).is_err(),
        "staff_cost"
    );
}

// ---------------------------------------------------------------------------
// T4, T5, T6, T7, T8 — the payroll month
// ---------------------------------------------------------------------------

/// **T5.** A raise applies from its date and does not rewrite last month.
#[test]
fn a_raise_does_not_change_last_months_run() {
    let scratch = Scratch::new("emp_raise");
    let app = a_shop(&scratch, "raise");
    hire(&app, "staff_ravi", "Ravi");
    as_owner(&app, "staff_boss", "Meena");

    let start = today();
    save_salary_on(
        &app,
        SalaryEdit {
            staff_id: "staff_ravi".to_owned(),
            effective_from: day_text(start),
            basis: "monthly".to_owned(),
            amount: "18000".to_owned(),
            components: Vec::new(),
        },
    )
    .expect("salary set");

    // Last month's run, computed and approved.
    let first = compute_payroll_on(&app, day_text(start), day_text(start.next()))
        .expect("computed");
    let before = first.total.paise;
    approve_payroll_on(&app, first.id.clone(), "cash".to_owned()).expect("approved");

    // A raise, dated later.
    let later = start.next().next();
    save_salary_on(
        &app,
        SalaryEdit {
            staff_id: "staff_ravi".to_owned(),
            effective_from: day_text(later),
            basis: "monthly".to_owned(),
            amount: "22000".to_owned(),
            components: Vec::new(),
        },
    )
    .expect("raise");

    // **Last month is untouched.** Its lines were frozen when it was computed,
    // and reading it back gives the same figure a year later.
    let reread = payroll_on(&app, first.id).expect("read");
    assert_eq!(
        reread.total.paise, before,
        "a raise reached backwards into an approved run"
    );
}

/// **T4, T6, T7 and T8 in one sequence — a real payroll month.**
///
/// This is the test that would catch the expensive mistakes, and it walks the
/// month a shop actually has: a salary, an advance in the middle of it, a run,
/// an approval, the drawer, and then the two things that must be refused.
#[test]
fn a_payroll_month_with_an_advance_adds_up_and_reconciles_with_the_drawer() {
    let scratch = Scratch::new("emp_payroll");
    let app = a_shop(&scratch, "payroll");
    hire(&app, "staff_ravi", "Ravi");
    as_owner(&app, "staff_boss", "Meena");

    let from = today();
    let to = from.next().next();

    save_salary_on(
        &app,
        SalaryEdit {
            staff_id: "staff_ravi".to_owned(),
            effective_from: day_text(from),
            basis: "monthly".to_owned(),
            amount: "18000".to_owned(),
            components: Vec::new(),
        },
    )
    .expect("salary set");

    // **T8, first half — the advance shows in the drawer the day it is given.**
    let drawer_before = crate::expenses::expenses_on(&app)
        .expect("the drawer")
        .cash
        .expected
        .paise;
    give_advance_on(
        &app,
        "staff_ravi".to_owned(),
        "2000".to_owned(),
        1,
        "Festival".to_owned(),
    )
    .expect("advance given");
    let drawer_after = crate::expenses::expenses_on(&app)
        .expect("the drawer")
        .cash
        .expected
        .paise;
    assert_eq!(
        drawer_before - drawer_after,
        rupees(2_000).paise(),
        "an advance did not come out of the drawer on the day it was given"
    );

    // The run.
    let run = compute_payroll_on(&app, day_text(from), day_text(to)).expect("computed");
    let line = run.lines.first().expect("one line");

    // **T4 — the arithmetic, by hand.** ₹18,000 monthly, no unpaid days, and
    // ₹2,000 of advance recovered: the net is ₹16,000 and the shop's COST is
    // still ₹18,000.
    assert_eq!(line.earned.paise, rupees(18_000).paise());
    assert_eq!(line.advance_recovered.paise, rupees(2_000).paise());
    assert_eq!(line.net.paise, rupees(16_000).paise());
    assert_eq!(run.state, "draft", "computing must not move any money");

    // **T6 — approving posts ONE expense, and the drawer reconciles.**
    let approved = approve_payroll_on(&app, run.id.clone(), "cash".to_owned())
        .expect("approved");
    assert_eq!(approved.state, "approved");

    let after = crate::expenses::expenses_on(&app).expect("the drawer");
    let salary_rows: Vec<_> = after
        .rows
        .iter()
        .filter(|r| r.description.starts_with("Salary "))
        .collect();
    assert_eq!(salary_rows.len(), 1, "a run must post exactly ONE expense");
    assert_eq!(
        salary_rows[0].amount.paise,
        rupees(18_000).paise(),
        "the salary EXPENSE is the gross — netting it would understate the \
         wage bill by every advance ever given"
    );

    // The drawer: ₹2,000 out for the advance, ₹18,000 out as the expense, and
    // ₹2,000 back as the compensating top-up. Eighteen thousand, once.
    let drawer_end = after.cash.expected.paise;
    assert_eq!(
        drawer_before - drawer_end,
        rupees(18_000).paise(),
        "the drawer disagrees with what was actually handed over"
    );

    // **P30 — the payslip, which P28 named as not done** (scope 9.14).
    //
    // The paper is what the person being paid holds, and a shop that cannot
    // hand one over settles every argument about pay by memory.
    let job = crate::employment::print_payslip_on(&app, run.id.clone(), "staff_ravi".to_owned())
        .expect("a payslip goes to the printer");
    assert!(!job.is_empty(), "the slip was queued like any other document");
    assert!(
        crate::employment::print_payslip_on(&app, run.id.clone(), "staff_nobody".to_owned())
            .is_err(),
        "a payslip for somebody who is not on the run is a refusal, not a blank slip"
    );

    // **T7 — approving twice is refused.**
    assert!(
        approve_payroll_on(&app, run.id.clone(), "cash".to_owned()).is_err(),
        "a run was approved twice, and everybody was paid twice"
    );

    // **T8, second half — the advance is fully recovered.**
    let salary = salary_on(&app, "staff_ravi".to_owned()).expect("read");
    assert_eq!(
        salary.outstanding.paise, 0,
        "the advance was not recovered from the run"
    );

    // **And reversing puts everything back**, including what the advance owed —
    // otherwise reversing a run would quietly forgive money (D47).
    let reversed = reverse_payroll_on(&app, run.id, "Wrong period".to_owned())
        .expect("reversed");
    assert_eq!(reversed.state, "reversed");
    let salary = salary_on(&app, "staff_ravi".to_owned()).expect("read");
    assert_eq!(
        salary.outstanding.paise,
        rupees(2_000).paise(),
        "reversing a run forgave an advance"
    );
    let drawer_final = crate::expenses::expenses_on(&app)
        .expect("the drawer")
        .cash
        .expected
        .paise;
    assert_eq!(
        drawer_before - drawer_final,
        rupees(2_000).paise(),
        "reversing left the salary expense behind"
    );
}

/// **T8's instalments.** An advance agreed over three months comes back in
/// three, not one — and the outstanding balance is a sum of the recoveries,
/// never a stored column.
#[test]
fn an_advance_in_instalments_comes_back_in_instalments() {
    let scratch = Scratch::new("emp_instal");
    let app = a_shop(&scratch, "instal");
    hire(&app, "staff_ravi", "Ravi");
    as_owner(&app, "staff_boss", "Meena");

    let from = today();
    save_salary_on(
        &app,
        SalaryEdit {
            staff_id: "staff_ravi".to_owned(),
            effective_from: day_text(from),
            basis: "monthly".to_owned(),
            amount: "18000".to_owned(),
            components: Vec::new(),
        },
    )
    .expect("salary set");

    give_advance_on(
        &app,
        "staff_ravi".to_owned(),
        "6000".to_owned(),
        3,
        "School fees".to_owned(),
    )
    .expect("advance given");

    let run = compute_payroll_on(&app, day_text(from), day_text(from.next()))
        .expect("computed");
    let line = run.lines.first().expect("one line");
    assert_eq!(
        line.advance_recovered.paise,
        rupees(2_000).paise(),
        "the whole advance came back in one month despite being agreed over three"
    );
    assert_eq!(line.net.paise, rupees(16_000).paise());

    approve_payroll_on(&app, run.id, "cash".to_owned()).expect("approved");
    let salary = salary_on(&app, "staff_ravi".to_owned()).expect("read");
    assert_eq!(
        salary.outstanding.paise,
        rupees(4_000).paise(),
        "two instalments should still be owed"
    );
}

/// **Unpaid leave is deducted from a monthly salary, once.**
///
/// The period is three days long, so one unpaid day is exactly a third of the
/// month — which makes the arithmetic checkable by hand, which is the whole
/// standard this session is held to.
#[test]
fn an_unpaid_day_comes_off_a_monthly_salary() {
    let scratch = Scratch::new("emp_unpaid");
    let app = a_shop(&scratch, "unpaid");
    hire(&app, "staff_ravi", "Ravi");
    as_owner(&app, "staff_boss", "Meena");

    let from = today();
    let to = from.next().next(); // three days, inclusive

    save_salary_on(
        &app,
        SalaryEdit {
            staff_id: "staff_ravi".to_owned(),
            effective_from: day_text(from),
            basis: "monthly".to_owned(),
            amount: "3000".to_owned(),
            components: Vec::new(),
        },
    )
    .expect("salary set");

    // One day of UNPAID leave, approved.
    let asked = request_leave_on(
        &app,
        "staff_ravi".to_owned(),
        "lv_unpaid".to_owned(),
        day_text(from.next()),
        day_text(from.next()),
        2,
        "Personal".to_owned(),
    )
    .expect("asked");
    let request = asked.requests.first().expect("a request").id.clone();
    decide_leave_on(&app, request, true, String::new()).expect("approved");

    let run = compute_payroll_on(&app, day_text(from), day_text(to)).expect("computed");
    let line = run.lines.first().expect("one line");
    assert_eq!(
        line.unpaid_leave_deduction.paise,
        rupees(1_000).paise(),
        "one unpaid day of a three-day period at ₹3,000 is ₹1,000"
    );
    assert_eq!(line.net.paise, rupees(2_000).paise());

    // **A PAID day is not deducted.** Same shape, different leave type.
    hire(&app, "staff_priya", "Priya");
    save_salary_on(
        &app,
        SalaryEdit {
            staff_id: "staff_priya".to_owned(),
            effective_from: day_text(from),
            basis: "monthly".to_owned(),
            amount: "3000".to_owned(),
            components: Vec::new(),
        },
    )
    .expect("salary set");
    let asked = request_leave_on(
        &app,
        "staff_priya".to_owned(),
        "lv_casual".to_owned(),
        day_text(from.next()),
        day_text(from.next()),
        2,
        "Family".to_owned(),
    )
    .expect("asked");
    let request = asked
        .requests
        .first()
        .expect("a request")
        .id
        .clone();
    decide_leave_on(&app, request, true, String::new()).expect("approved");

    let run = compute_payroll_on(&app, day_text(from), day_text(to)).expect("computed");
    let priya = run
        .lines
        .iter()
        .find(|l| l.staff_id == "staff_priya")
        .expect("her line");
    assert_eq!(
        priya.unpaid_leave_deduction.paise, 0,
        "a PAID day of leave was deducted"
    );
    assert_eq!(priya.net.paise, rupees(3_000).paise());
}

// ---------------------------------------------------------------------------
// Scope 9.15 — nobody is ever deleted
// ---------------------------------------------------------------------------

/// **T12's half that this session owns.** Somebody who has left keeps their
/// record, and the record says when.
#[test]
fn somebody_who_leaves_keeps_their_record() {
    let scratch = Scratch::new("emp_left");
    let app = a_shop(&scratch, "left");
    hire(&app, "staff_ravi", "Ravi");
    as_owner(&app, "staff_boss", "Meena");

    let day = today();
    save_employee_on(
        &app,
        EmployeeEdit {
            id: "staff_ravi".to_owned(),
            designation: "Cook".to_owned(),
            department: "Kitchen".to_owned(),
            address: String::new(),
            emergency_name: "Lakshmi".to_owned(),
            emergency_phone: "9845000000".to_owned(),
            id_proof: "Aadhaar ...4321".to_owned(),
            employment_type: "full_time".to_owned(),
            left_on: day_text(day),
        },
    )
    .expect("saved");

    let people = people_on(&app).expect("read");
    let ravi = people
        .iter()
        .find(|p| p.id == "staff_ravi")
        .expect("still on the list — nobody is ever deleted");
    assert_eq!(ravi.status, "left");
    assert_eq!(ravi.left, day_text(day));
    assert_eq!(ravi.designation.as_deref(), Some("Cook"));

    // And coming back sets them active again without losing anything.
    save_employee_on(
        &app,
        EmployeeEdit {
            id: "staff_ravi".to_owned(),
            designation: "Cook".to_owned(),
            department: "Kitchen".to_owned(),
            address: String::new(),
            emergency_name: "Lakshmi".to_owned(),
            emergency_phone: "9845000000".to_owned(),
            id_proof: "Aadhaar ...4321".to_owned(),
            employment_type: "full_time".to_owned(),
            left_on: String::new(),
        },
    )
    .expect("saved");
    let people = people_on(&app).expect("read");
    let ravi = people
        .iter()
        .find(|p| p.id == "staff_ravi")
        .expect("there");
    assert_eq!(ravi.status, "active");
    assert!(ravi.left.is_empty());
}

// ---------------------------------------------------------------------------
// Scope 9.16 — the staff cost
// ---------------------------------------------------------------------------

/// The second of the two numbers that decide whether a restaurant makes money.
/// A DRAFT run must not count — a proposal is not a cost.
#[test]
fn staff_cost_counts_approved_runs_and_not_drafts() {
    let scratch = Scratch::new("emp_cost");
    let app = a_shop(&scratch, "cost");
    hire(&app, "staff_ravi", "Ravi");
    as_owner(&app, "staff_boss", "Meena");

    let from = today();
    let to = from.next();
    save_salary_on(
        &app,
        SalaryEdit {
            staff_id: "staff_ravi".to_owned(),
            effective_from: day_text(from),
            basis: "monthly".to_owned(),
            amount: "18000".to_owned(),
            components: Vec::new(),
        },
    )
    .expect("salary set");

    let run = compute_payroll_on(&app, day_text(from), day_text(to)).expect("computed");
    let draft = staff_cost_on(&app, day_text(from), day_text(to)).expect("cost");
    assert_eq!(
        draft.wages.paise, 0,
        "a draft run was counted as money the shop had spent"
    );

    approve_payroll_on(&app, run.id, "cash".to_owned()).expect("approved");
    let real = staff_cost_on(&app, day_text(from), day_text(to)).expect("cost");
    assert_eq!(real.wages.paise, rupees(18_000).paise());
    assert!(
        real.says.contains("no percentage") || real.says.contains('%'),
        "the sentence has to say something: {}",
        real.says
    );
}

/// A leave balance can go negative, and the words say so rather than hiding it
/// behind a floor of zero — a shop grants more than somebody had and recovers
/// it later, and software that lied about that would be worse than a notebook.
#[test]
fn a_leave_balance_that_has_gone_negative_says_so() {
    let scratch = Scratch::new("emp_negative");
    let app = a_shop(&scratch, "negative");
    hire(&app, "staff_ravi", "Ravi");
    as_owner(&app, "staff_boss", "Meena");

    adjust_leave_on(
        &app,
        "staff_ravi".to_owned(),
        "lv_casual".to_owned(),
        2,
        "One day only".to_owned(),
        true,
    )
    .expect("granted");

    let day = today();
    let asked = request_leave_on(
        &app,
        "staff_ravi".to_owned(),
        "lv_casual".to_owned(),
        day_text(day),
        day_text(day.next()),
        6,
        "Emergency".to_owned(),
    )
    .expect("asked");
    let request = asked.requests.first().expect("a request").id.clone();
    decide_leave_on(&app, request, true, String::new()).expect("approved");

    let after = leave_on(&app, Some("staff_ravi".to_owned())).expect("read");
    let casual = after
        .balances
        .iter()
        .find(|b| b.leave_type_id == "lv_casual")
        .expect("casual");
    assert_eq!(casual.left_halves, -4);
    assert!(
        casual.left_says.starts_with('−'),
        "a negative balance must read as one: {}",
        casual.left_says
    );
    assert_eq!(HalfDays::new(-4).say(), casual.left_says);
}
