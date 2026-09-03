//! The business day: which days are still open, closing one, calling one a holiday, opening one
//! again — and the drawer count that sits beside it, optional and locking nothing.

use std::collections::{BTreeMap, BTreeSet};

use mb_auth::Permission;
use mb_auth::audit::{AuditEntry, action};
use mb_core::{BusinessDay, Money, StaffId, Timestamp};
use mb_db::repo::money::{CashMovement, DayClose, Denomination};
use mb_db::repo::{DayFigures, DayKind, DayRow};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

/// How far back the gate looks. A shop that has not closed a day in two months is asked about
/// the last sixty, not about the whole year.
const PENDING_WINDOW_DAYS: i32 = 60;
/// How many days the Days screen lists, today included.
const DAYS_LISTED: i32 = 14;

// The day.

/// One day the gate is asking about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PendingDayView {
    pub day: String,
    /// "Tuesday 2 September".
    pub day_says: String,
    pub bills: u32,
    pub net: MoneyView,
    pub cash: MoneyView,
    pub upi_and_card: MoneyView,
    pub expenses: MoneyView,
    /// "Table 7 #12", "Parcel #13" — what nobody finished that day.
    pub open_orders: Vec<String>,
    /// The same as one sentence, or empty. A day with this cannot be closed from the gate.
    pub open_says: String,
    /// Nothing happened on it, so it was probably a day the shop was shut.
    pub looks_like_holiday: bool,
    /// `close` or `holiday` — where the segment starts.
    pub suggested: String,
}

/// The gate: what is still open, and the one press that finishes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DayStateView {
    pub today: String,
    /// "Today, Wednesday 3 September".
    pub today_says: String,
    pub pending: Vec<PendingDayView>,
    /// "Tuesday was never closed.", "3 days were never closed." — or empty.
    pub pending_says: String,
    /// Whether the person looking may close a day.
    pub may_act: bool,
    /// Why they cannot, or empty.
    pub blocked_says: String,
    /// `open`, `closed` or `holiday`.
    pub today_state: String,
    /// "Closed 3 Sep, 11:14 pm by Ravi." — empty while today is open.
    pub today_closed_says: String,
    /// The primary button: "Close Tuesday", "Close 2 days and mark 1 holiday". Empty when
    /// nothing can be done yet.
    pub action_label: String,
    /// The way past the gate when a pending day still has open orders, or empty.
    pub escape_label: String,
}

/// One row of the Days screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DayRowView {
    pub day: String,
    pub day_says: String,
    /// `trading` or `holiday`.
    pub kind: String,
    pub is_locked: bool,
    pub bills: u32,
    pub net: MoneyView,
    /// "Closed 3 Sep, 11:14 pm by Ravi.", "Holiday, marked 1 Sep by Ravi.", "Never closed.",
    /// "Open."
    pub closed_says: String,
    /// The chip: `open`, `closed`, `holiday` or `pending`.
    pub state: String,
    /// Whether the Holiday switch may be pressed on this row.
    pub may_be_holiday: bool,
}

/// Reports › Days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DaysView {
    pub today: String,
    pub today_says: String,
    pub today_state: String,
    pub today_closed_says: String,
    pub may_act: bool,
    /// What closing today will leave in the drawer, in words — or empty.
    pub carry_says: String,
    /// Today and the thirteen days before it, newest first.
    pub days: Vec<DayRowView>,
    /// Holidays already marked for days that have not come yet.
    pub upcoming: Vec<DayRowView>,
    /// Whether a day that has not come yet may be marked a holiday.
    pub may_plan_holiday: bool,
}

/// `YYYY-MM-DD` from the screen, or the refusal.
fn parse_day(text: &str) -> UiResult<BusinessDay> {
    text.trim()
        .parse()
        .map_err(|_| UiError::new("day.bad", "That is not a day."))
}

/// Who closed it, by name — "somebody" when the row does not say.
fn name_of(repos: &mb_db::Repos<'_>, id: Option<&StaffId>) -> Result<String, mb_db::DbError> {
    Ok(match id {
        Some(id) => repos
            .people()
            .find_staff(OUTLET, id.as_str())?
            .map(|person| person.name),
        None => None,
    }
    .unwrap_or_else(|| "somebody".to_owned()))
}

/// A day, as it stands right now.
struct Look {
    day: BusinessDay,
    figures: DayFigures,
    open_orders: Vec<String>,
    row: Option<DayRow>,
}

fn look_at(
    repos: &mb_db::Repos<'_>,
    tables: &[mb_db::repo::floor::DiningTable],
    open: &[mb_core::AnyOrder],
    day: BusinessDay,
) -> Result<Look, mb_db::DbError> {
    Ok(Look {
        day,
        figures: repos.days().figures(OUTLET, day)?,
        open_orders: open
            .iter()
            .filter(|order| order.core().business_day == day)
            .map(|order| crate::kitchen::place_and_token(order, tables))
            .collect(),
        row: repos.days().find(OUTLET, day)?,
    })
}

/// The one question the gate asks: every day from the last locked day to yesterday that has no
/// locked row. Bounded, and a shop that has never closed anything only starts counting from
/// the first day anything happened.
fn pending_in(
    repos: &mb_db::Repos<'_>,
    today: BusinessDay,
) -> Result<Vec<BusinessDay>, mb_db::DbError> {
    let yesterday = today.previous();
    let start = match repos.days().last_locked_before(OUTLET, today)? {
        Some(last) => last.next(),
        None => match repos.days().first_activity(OUTLET)? {
            Some(first) => first,
            None => return Ok(Vec::new()),
        },
    };
    let bound = BusinessDay::from_days_since_epoch(
        today.days_since_epoch().saturating_sub(PENDING_WINDOW_DAYS),
    );
    let start = start.max(bound);
    if start > yesterday {
        return Ok(Vec::new());
    }
    let locked: BTreeSet<BusinessDay> = repos
        .days()
        .rows_between(OUTLET, start, yesterday)?
        .into_iter()
        .filter(|row| row.is_locked)
        .map(|row| row.day)
        .collect();
    let mut out = Vec::new();
    let mut day = start;
    while day <= yesterday {
        if !locked.contains(&day) {
            out.push(day);
        }
        day = day.next();
    }
    Ok(out)
}

/// The orders nobody finished, as one sentence — or nothing.
fn open_words(open: &[String]) -> String {
    if open.is_empty() {
        return String::new();
    }
    format!(
        "{} still open: {}. Settle or cancel them before this day can be closed.",
        words::count(
            i64::try_from(open.len()).unwrap_or(0),
            "order is",
            "orders are"
        ),
        words::list(open)
    )
}

/// `open`, `closed` or `holiday`, and the sentence that goes with it.
fn state_words(
    repos: &mb_db::Repos<'_>,
    row: Option<&DayRow>,
) -> Result<(String, String), mb_db::DbError> {
    let Some(row) = row.filter(|r| r.is_locked) else {
        return Ok(("open".to_owned(), String::new()));
    };
    let who = name_of(repos, row.closed_by.as_ref())?;
    let when = row.closed_at.map(words::when).unwrap_or_default();
    Ok(match row.kind {
        DayKind::Holiday => (
            "holiday".to_owned(),
            format!("Holiday, marked {when} by {who}."),
        ),
        DayKind::Trading => ("closed".to_owned(), format!("Closed {when} by {who}.")),
    })
}

fn count_of(bills: i64) -> u32 {
    u32::try_from(bills).unwrap_or(u32::MAX)
}

/// The gate. `holidays` is what the person has switched to Holiday so far, so the button's
/// words follow their choice; `None` means the suggestions stand.
pub fn day_state_on(app: &App, holidays: Option<Vec<String>>) -> UiResult<DayStateView> {
    let who = guard::require_signed_in(app)?;
    let may_act = who.must(Permission::DayClose).is_ok();
    let today = today(now());
    let chosen: Option<BTreeSet<BusinessDay>> = holidays.map(|list| {
        list.iter()
            .filter_map(|text| text.trim().parse().ok())
            .collect()
    });

    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let tables = repos.floor().list_tables(OUTLET)?;
                let open = repos.orders().list_open(OUTLET)?;

                let mut pending = Vec::new();
                let (mut closes, mut holidays, mut blocked) = (Vec::new(), Vec::new(), 0_usize);
                for day in pending_in(&repos, today)? {
                    let look = look_at(&repos, &tables, &open, day)?;
                    let looks_like_holiday = look.figures.is_empty() && look.open_orders.is_empty();
                    let holiday = match &chosen {
                        Some(set) => looks_like_holiday && set.contains(&day),
                        None => looks_like_holiday,
                    };
                    if holiday {
                        holidays.push(day);
                    } else if look.open_orders.is_empty() {
                        closes.push(day);
                    } else {
                        blocked += 1;
                    }
                    pending.push(PendingDayView {
                        day: day.to_string(),
                        day_says: words::day_with_weekday(day, today),
                        bills: count_of(look.figures.bills),
                        net: MoneyView::from(look.figures.net),
                        cash: MoneyView::from(look.figures.cash),
                        upi_and_card: MoneyView::from(look.figures.upi_and_card),
                        expenses: MoneyView::from(look.figures.expenses),
                        open_says: open_words(&look.open_orders),
                        open_orders: look.open_orders,
                        looks_like_holiday,
                        suggested: if holiday { "holiday" } else { "close" }.to_owned(),
                    });
                }

                let (today_state, today_closed_says) =
                    state_words(&repos, repos.days().find(OUTLET, today)?.as_ref())?;
                Ok(DayStateView {
                    today: today.to_string(),
                    today_says: format!("Today, {}", words::day_with_weekday(today, today)),
                    pending_says: pending_words(&pending),
                    blocked_says: if may_act || pending.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "Closing a day needs permission, and {} does not have it. Ask \
                             somebody who can, or sign out.",
                            who.name
                        )
                    },
                    action_label: action_words(&closes, &holidays),
                    escape_label: if blocked > 0 {
                        "Finish the open orders first".to_owned()
                    } else {
                        String::new()
                    },
                    pending,
                    may_act,
                    today_state,
                    today_closed_says,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// "Tuesday was never closed." / "3 days were never closed."
fn pending_words(pending: &[PendingDayView]) -> String {
    match pending {
        [] => String::new(),
        [one] => format!(
            "{} was never closed.",
            one.day_says.split(' ').next().unwrap_or_default()
        ),
        many => format!("{} days were never closed.", many.len()),
    }
}

/// The primary button's words, from what the press will do.
fn action_words(closes: &[BusinessDay], holidays: &[BusinessDay]) -> String {
    match (closes, holidays) {
        ([], []) => String::new(),
        ([one], []) => format!("Close {}", words::weekday(*one)),
        (many, []) => format!("Close {} days", many.len()),
        ([], [one]) => format!("Mark {} a holiday", words::weekday(*one)),
        ([], many) => format!("Mark {} holidays", many.len()),
        (c, h) => format!(
            "Close {} and mark {}",
            words::count(i64::try_from(c.len()).unwrap_or(0), "day", "days"),
            words::count(i64::try_from(h.len()).unwrap_or(0), "holiday", "holidays")
        ),
    }
}

/// Reports › Days.
pub fn days_on(app: &App) -> UiResult<DaysView> {
    let who = guard::require_any(app, &[Permission::ReportsView, Permission::DayClose])?;
    let may_act = who.must(Permission::DayClose).is_ok();
    let today = today(now());
    let config = app.shop_config();
    let from = BusinessDay::from_days_since_epoch(
        today
            .days_since_epoch()
            .saturating_sub(DAYS_LISTED.saturating_sub(1)),
    );

    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let tables = repos.floor().list_tables(OUTLET)?;
                let open = repos.orders().list_open(OUTLET)?;

                let mut days = Vec::new();
                let mut day = today;
                while day >= from {
                    let look = look_at(&repos, &tables, &open, day)?;
                    days.push(row_view(&repos, &look, today, may_act)?);
                    day = day.previous();
                }
                let mut upcoming = Vec::new();
                for row in repos.days().locked_after(OUTLET, today)? {
                    let look = Look {
                        day: row.day,
                        figures: DayFigures::default(),
                        open_orders: Vec::new(),
                        row: Some(row),
                    };
                    upcoming.push(row_view(&repos, &look, today, may_act)?);
                }

                let (today_state, today_closed_says) =
                    state_words(&repos, repos.days().find(OUTLET, today)?.as_ref())?;
                Ok(DaysView {
                    today: today.to_string(),
                    today_says: format!("Today, {}", words::day_with_weekday(today, today)),
                    today_state,
                    today_closed_says,
                    may_act,
                    carry_says: if config.day.carry_float && config.day.float_amount.is_positive() {
                        format!(
                            "{} will be left in the drawer and counted as tomorrow's opening \
                             float.",
                            config.day.float_amount.to_plain_string()
                        )
                    } else {
                        String::new()
                    },
                    days,
                    upcoming,
                    may_plan_holiday: may_act,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

fn row_view(
    repos: &mb_db::Repos<'_>,
    look: &Look,
    today: BusinessDay,
    may_act: bool,
) -> Result<DayRowView, mb_db::DbError> {
    let locked = look.row.as_ref().filter(|r| r.is_locked);
    let (state, closed_says) = match locked {
        Some(_) => state_words(repos, look.row.as_ref())?,
        None if look.day == today => ("open".to_owned(), "Open.".to_owned()),
        None if look.day > today => ("open".to_owned(), String::new()),
        None => ("pending".to_owned(), "Never closed.".to_owned()),
    };
    // The frozen figures once it is locked; the live ones until then.
    let (bills, net) = match locked {
        Some(row) => (row.bills, row.net),
        None => (look.figures.bills, look.figures.net),
    };
    Ok(DayRowView {
        day: look.day.to_string(),
        day_says: words::day_with_weekday(look.day, today),
        kind: look
            .row
            .as_ref()
            .map_or(DayKind::Trading, |r| r.kind)
            .as_str()
            .to_owned(),
        is_locked: locked.is_some(),
        bills: count_of(bills),
        net: MoneyView::from(net),
        closed_says,
        state,
        may_be_holiday: may_act && look.figures.is_empty() && look.open_orders.is_empty(),
    })
}

// The lock.

/// When a day was locked, or `None` while it is open.
pub fn locked_since(app: &App, day: BusinessDay) -> UiResult<Option<Timestamp>> {
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| mb_db::Repos::new(tx).days().locked_at(OUTLET, day))
            .map_err(|e| words::from_db(&e))
    })
}

/// The refusal a locked day gives, wherever money would have moved in it. `then` is what the
/// person was trying to do: "keep billing", "void this bill".
#[must_use]
pub fn locked_refusal(code: &str, since: Timestamp, then: &str) -> UiError {
    UiError::new(
        code,
        format!(
            "That day was closed at {}. Open it again under Reports › Days to {then}.",
            words::when(since)
        ),
    )
}

// Closing, holidays, opening again.

/// Close one day: freeze its figures, lock it, carry the float, sweep the kitchen, freeze the
/// stock. No count required.
fn close_one(app: &App, who: &mb_auth::Actor, at: Timestamp, day: BusinessDay) -> UiResult<()> {
    let today = today(at);
    if day > today {
        return Err(UiError::new(
            "day.future",
            format!(
                "{} has not happened yet.",
                words::day_with_weekday(day, today)
            ),
        ));
    }
    let config = app.shop_config();
    let carried = config.day.carry_float.then_some(config.day.float_amount);
    let day_key = day.days_since_epoch().to_string();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| -> Result<Result<(), UiError>, mb_db::DbError> {
                let repos = mb_db::Repos::new(tx);
                if repos.days().locked_at(OUTLET, day)?.is_some() {
                    return Ok(Err(UiError::new(
                        "day.already_closed",
                        format!("{} is already closed.", words::day_with_weekday(day, today)),
                    )));
                }
                let tables = repos.floor().list_tables(OUTLET)?;
                let open = repos.orders().list_open(OUTLET)?;
                let look = look_at(&repos, &tables, &open, day)?;
                if !look.open_orders.is_empty() {
                    return Ok(Err(UiError::new(
                        "day.open_orders",
                        open_words(&look.open_orders),
                    )));
                }

                repos.days().lock(
                    OUTLET,
                    &DayRow {
                        day,
                        kind: DayKind::Trading,
                        is_locked: true,
                        closed_at: Some(at),
                        closed_by: Some(who.staff_id.clone()),
                        reopened_at: None,
                        reopened_by: None,
                        note: None,
                        bills: look.figures.bills,
                        net: look.figures.net,
                        cash_taken: look.figures.cash,
                    },
                )?;

                // The day's totals go up to the cloud again, now marked closed.
                for table in mb_db::repo::wire::TOTALS_TABLES {
                    repos
                        .outbox()
                        .enqueue(OUTLET, table, &day_key, mb_db::repo::Op::Upsert, at)?;
                }

                // Tomorrow's float, written today. One row per day, so opening and closing
                // again does not put two floats in the drawer.
                if let Some(amount) = carried.filter(|m| m.is_positive()) {
                    repos.money().save_cash_movement(
                        OUTLET,
                        &CashMovement {
                            id: format!("float_{}", day.next().days_since_epoch()),
                            kind: "float".to_owned(),
                            amount,
                            reason: format!("Left in the drawer when {day} was closed"),
                            at,
                            business_day: day.next(),
                            moved_by: Some(who.staff_id.clone()),
                        },
                    )?;
                }

                // Nobody will bump a finished order's ticket tomorrow.
                repos.kitchen().close_finished(OUTLET)?;
                // What was on the shelf when the day ended.
                repos.stock().close_day(OUTLET, day)?;

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::DAY_CLOSED,
                        "day",
                    )
                    .about(day.to_string())
                    .with_after(serde_json::json!({
                        "bills": look.figures.bills,
                        "net_paise": look.figures.net.paise(),
                        "cash_paise": look.figures.cash.paise(),
                    })),
                )?;
                Ok(Ok(()))
            })
            .map_err(|e| words::from_db(&e))
    })?
}

/// Close one day from the Days screen.
pub fn close_day_on(app: &App, day: String) -> UiResult<DaysView> {
    let who = guard::require(app, Permission::DayClose)?;
    close_one(app, &who, now(), parse_day(&day)?)?;
    days_on(app)
}

/// Mark days as holidays, or take the mark off. A day with a bill or an expense on it was not
/// a holiday, and the refusal says so.
pub fn set_holiday_on(app: &App, days: Vec<String>, on: bool) -> UiResult<DaysView> {
    let who = guard::require(app, Permission::DayClose)?;
    let at = now();
    let today = today(at);
    let mut wanted = Vec::new();
    for text in &days {
        wanted.push(parse_day(text)?);
    }
    if wanted.is_empty() {
        return Err(UiError::new("day.none", "Pick a day first."));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| -> Result<Result<(), UiError>, mb_db::DbError> {
                let repos = mb_db::Repos::new(tx);
                let tables = repos.floor().list_tables(OUTLET)?;
                let open = repos.orders().list_open(OUTLET)?;
                for day in &wanted {
                    let day = *day;
                    let says = words::day_with_weekday(day, today);
                    if on {
                        let look = look_at(&repos, &tables, &open, day)?;
                        if !look.figures.is_empty() {
                            return Ok(Err(UiError::new(
                                "day.not_empty",
                                format!(
                                    "{says} has {} on it, so it was not a holiday.",
                                    if look.figures.bills > 0 {
                                        words::count(look.figures.bills, "bill", "bills")
                                    } else {
                                        "an expense".to_owned()
                                    }
                                ),
                            )));
                        }
                        if !look.open_orders.is_empty() {
                            return Ok(Err(UiError::new(
                                "day.open_orders",
                                open_words(&look.open_orders),
                            )));
                        }
                        repos.days().lock(
                            OUTLET,
                            &DayRow {
                                day,
                                kind: DayKind::Holiday,
                                is_locked: true,
                                closed_at: Some(at),
                                closed_by: Some(who.staff_id.clone()),
                                reopened_at: None,
                                reopened_by: None,
                                note: None,
                                bills: 0,
                                net: Money::ZERO,
                                cash_taken: Money::ZERO,
                            },
                        )?;
                    } else {
                        let is_holiday = repos
                            .days()
                            .find(OUTLET, day)?
                            .is_some_and(|row| row.is_locked && row.kind == DayKind::Holiday);
                        if !is_holiday {
                            return Ok(Err(UiError::new(
                                "day.not_holiday",
                                format!("{says} is not marked a holiday."),
                            )));
                        }
                        repos
                            .days()
                            .unlock(OUTLET, day, at, Some(&who.staff_id), None)?;
                    }
                    for table in mb_db::repo::wire::TOTALS_TABLES {
                        repos.outbox().enqueue(
                            OUTLET,
                            table,
                            &day.days_since_epoch().to_string(),
                            mb_db::repo::Op::Upsert,
                            at,
                        )?;
                    }
                    repos.audit().append(
                        OUTLET,
                        &AuditEntry::new(
                            at,
                            day,
                            Some(who.staff_id.clone()),
                            action::DAY_HOLIDAY,
                            "day",
                        )
                        .about(day.to_string())
                        .with_after(serde_json::json!({ "is_holiday": on })),
                    )?;
                }
                Ok(Ok(()))
            })
            .map_err(|e| words::from_db(&e))
    })??;
    days_on(app)
}

/// Open a locked day again — the override, and it leaves a mark.
pub fn reopen_day_on(app: &App, day: String, reason: String) -> UiResult<DaysView> {
    let who = guard::require(app, Permission::DayClose)?;
    let at = now();
    let day = parse_day(&day)?;
    let reason = reason.trim().to_owned();
    if reason.is_empty() {
        return Err(UiError::new(
            "day.reopen_reason",
            "Opening a closed day needs a reason. It is recorded against your name, and an \
             owner reading the history later will want to know why.",
        ));
    }

    let opened = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                if !repos
                    .days()
                    .unlock(OUTLET, day, at, Some(&who.staff_id), Some(&reason))?
                {
                    return Ok(false);
                }
                for table in mb_db::repo::wire::TOTALS_TABLES {
                    repos.outbox().enqueue(
                        OUTLET,
                        table,
                        &day.days_since_epoch().to_string(),
                        mb_db::repo::Op::Upsert,
                        at,
                    )?;
                }
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::DAY_REOPENED,
                        "day",
                    )
                    .about(day.to_string())
                    .with_after(serde_json::json!({ "reason": reason })),
                )?;
                Ok(true)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    if !opened {
        return Err(UiError::new(
            "day.not_closed",
            "That day is not closed, so there is nothing to open.",
        ));
    }
    days_on(app)
}

/// The gate's one press: every pending day is closed, or marked a holiday where the person
/// switched it to one. A day with open orders is left for later.
pub fn close_pending_on(app: &App, holidays: Vec<String>) -> UiResult<DayStateView> {
    let who = guard::require(app, Permission::DayClose)?;
    let at = now();
    let state = day_state_on(app, Some(holidays))?;
    let mut mark = Vec::new();
    for row in &state.pending {
        if row.suggested == "holiday" {
            mark.push(row.day.clone());
        } else if row.open_says.is_empty() {
            close_one(app, &who, at, parse_day(&row.day)?)?;
        }
    }
    if !mark.is_empty() {
        set_holiday_on(app, mark, true)?;
    }
    day_state_on(app, None)
}

// The drawer.

/// The notes and coins an Indian till actually contains.
const DENOMINATIONS: &[(i32, &str)] = &[
    (50_000, "500"),
    (20_000, "200"),
    (10_000, "100"),
    (5_000, "50"),
    (2_000, "20"),
    (1_000, "10"),
    (500, "5"),
    (200, "2"),
    (100, "1"),
];

/// One row of the count, as the screen sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CountArg {
    pub value: i32,
    pub count: u32,
}

/// One row of the count, as the screen draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DenominationView {
    pub value: i32,
    /// "500", "50 paise" — the words, so the screen prints no currency of its own.
    pub label: String,
    pub count: u32,
    /// `count × value`, computed in Rust.
    pub total: MoneyView,
}

/// A label and an amount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SlipLineView {
    pub label: String,
    pub amount: MoneyView,
}

/// This till's drawer, today, with whatever has been counted laid over it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DrawerView {
    pub day: String,
    pub day_says: String,
    pub takings: Vec<SlipLineView>,
    pub drawer: Vec<SlipLineView>,
    pub expected: MoneyView,
    pub denominations: Vec<DenominationView>,
    pub counted: MoneyView,
    pub variance: MoneyView,
    /// The sentence. "Short by 340.00", "Over by 20.00", "Matches exactly." Never a signed
    /// number on its own.
    pub variance_says: String,
    /// `short`, `over` or `exact` — for the shape and the colour, and colour is never the only
    /// signal.
    pub variance_kind: String,
    /// Whether the reason box is required, and the sentence that says why.
    pub needs_reason: bool,
    pub reason_says: String,
    pub reason: String,
    /// "Counted 3 Sep, 11:14 pm by Ravi." — the last count on this till today, or empty.
    pub counted_says: String,
    /// Whether the person looking may write a count.
    pub may_count: bool,
    /// Which tills are in the shop's day, and which are not.
    pub tills_say: String,
}

/// The drawer as it stands, with an optional count laid over it.
pub fn drawer_on(app: &App, counts: Option<Vec<CountArg>>) -> UiResult<DrawerView> {
    let who = guard::require_any(app, &[Permission::ReportsView, Permission::DayClose])?;
    let may_count = who.must(Permission::DayClose).is_ok();
    let day = today(now());
    let config = app.shop_config();

    app.with_shop(|shop| {
        let (position, totals, last, stored_counts, counter, how_many_tills, still_open) = shop
            .db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                // The last count on THIS till: the person in front of this screen is counting
                // the box under it, and the shop total would be a variance they cannot act on.
                let shift = repos
                    .money()
                    .next_shift(OUTLET, day, app.terminal_id())?
                    .saturating_sub(1);
                let last = if shift > 0 {
                    repos
                        .money()
                        .find_drawer_close(OUTLET, day, app.terminal_id(), shift)?
                } else {
                    None
                };
                let stored = match &last {
                    Some(close) => repos.money().denominations(&close.id)?,
                    None => Vec::new(),
                };
                let counter = match &last {
                    Some(close) => name_of(&repos, close.closed_by.as_ref())?,
                    None => String::new(),
                };
                Ok((
                    repos
                        .money()
                        .cash_position_of(OUTLET, day, Some(app.terminal_id()))?,
                    repos.corrections().day_totals(OUTLET, day)?,
                    last,
                    stored,
                    counter,
                    repos.terminals().count(OUTLET)?,
                    repos.money().tills_still_open(OUTLET, day)?,
                ))
            })
            .map_err(|e| words::from_db(&e))?;

        // What is on the screen: what the person is typing, or — if they have not typed
        // anything — what was counted last time.
        let counted_rows: Vec<CountArg> = match counts {
            Some(rows) => rows,
            None => stored_counts
                .iter()
                .map(|d| CountArg {
                    value: i32::try_from(d.value.paise()).unwrap_or(0),
                    count: d.count,
                })
                .collect(),
        };

        let denominations = grid(&counted_rows);
        let counted = Money::from_paise(
            denominations
                .iter()
                .map(|row| row.total.paise)
                .fold(0_i64, i64::saturating_add),
        );
        let expected = position.expected;
        let variance = counted.sub(expected).unwrap_or(Money::ZERO);
        let threshold = config.day.variance_reason_above;
        // `>` and not `>=`: a threshold of ₹20 means "ask me when it is MORE than twenty out",
        // and a shop that sets zero is asking every time.
        let needs_reason = variance.abs() > threshold;

        Ok(DrawerView {
            day: day.to_string(),
            day_says: format!("Today, {}", words::day_with_weekday(day, day)),
            takings: vec![
                line("Bills", totals.gross),
                line("Voided", totals.voids),
                line("Refunded", totals.refunded),
                line("Net takings", totals.net),
            ],
            // Every line that makes the expected figure, and nothing that does not.
            drawer: vec![
                line("Opening float", position.opening_float),
                // Tips are split out of the takings line rather than added to it — a cash tip
                // really is in the drawer, but a 'cash from bills' figure that quietly includes
                // it will never agree with the sales report, and the gap is exactly the staff's
                // money.
                line(
                    "Cash from bills",
                    position
                        .cash_sales
                        .sub(position.cash_tips)
                        .unwrap_or(position.cash_sales),
                ),
                line("Tips in the drawer", position.cash_tips),
                line("Put in", position.top_ups),
                line("Spent from the drawer", position.cash_expenses),
                line("Paid out", position.payouts),
                line("Sent to the bank", position.bank_drops),
                line("Paid to suppliers", position.suppliers_paid),
                line("Still with the riders", position.with_riders),
            ],
            expected: MoneyView::from(expected),
            denominations,
            counted: MoneyView::from(counted),
            variance: MoneyView::from(variance),
            variance_says: variance_words(variance),
            variance_kind: if variance.is_negative() {
                "short".to_owned()
            } else if variance.is_positive() {
                "over".to_owned()
            } else {
                "exact".to_owned()
            },
            needs_reason,
            reason_says: if needs_reason {
                format!(
                    "The drawer is out by more than {}, so this needs a reason before the \
                     count is written. It goes on the slip and into the history.",
                    threshold.to_plain_string()
                )
            } else {
                String::new()
            },
            reason: last
                .as_ref()
                .and_then(|c| c.note.clone())
                .unwrap_or_default(),
            counted_says: last
                .as_ref()
                .map(|c| format!("Counted {} by {counter}.", words::when(c.closed_at)))
                .unwrap_or_default(),
            may_count,
            tills_say: tills_say(how_many_tills, &still_open),
        })
    })
}

/// Which tills are in the shop's day — and it is silent in a one-till shop, because there is
/// nothing there worth saying.
fn tills_say(how_many: u32, still_open: &[String]) -> String {
    if how_many < 2 {
        return String::new();
    }
    if still_open.is_empty() {
        return "Every till has counted its drawer.".to_owned();
    }
    format!(
        "{} still to count: {}. The shop's figure is the sum of the drawers, so it is short \
         by whatever is in them until they have.",
        words::count(
            i64::try_from(still_open.len()).unwrap_or(0),
            "till",
            "tills"
        ),
        words::list(still_open)
    )
}

fn line(label: &str, amount: Money) -> SlipLineView {
    SlipLineView {
        label: label.to_owned(),
        amount: MoneyView::from(amount),
    }
}

/// The full grid, with whatever has been counted filled in.
fn grid(counts: &[CountArg]) -> Vec<DenominationView> {
    DENOMINATIONS
        .iter()
        .map(|(value, label)| {
            let count = counts
                .iter()
                .find(|c| c.value == *value)
                .map_or(0, |c| c.count);
            DenominationView {
                value: *value,
                label: (*label).to_owned(),
                count,
                total: MoneyView::from(Money::from_paise(
                    i64::from(*value).saturating_mul(i64::from(count)),
                )),
            }
        })
        .collect()
}

/// The difference, in words.
pub(crate) fn variance_words(variance: Money) -> String {
    if variance.is_zero() {
        return "The drawer matches exactly.".to_owned();
    }
    if variance.is_negative() {
        return format!("Short by {}.", variance.abs().to_plain_string());
    }
    format!("Over by {}.", variance.to_plain_string())
}

/// Write this till's count for today. It locks nothing: the day is closed under Days.
pub fn count_drawer_on(
    app: &App,
    counts: Vec<CountArg>,
    reason: String,
    print: bool,
) -> UiResult<DrawerView> {
    let who = guard::require(app, Permission::DayClose)?;
    let at = now();
    let day = today(at);

    // The figures, from the same place the screen read them.
    let preview = drawer_on(app, Some(counts.clone()))?;
    let reason = reason.trim().to_owned();
    if preview.needs_reason && reason.is_empty() {
        // The refusal IS the feature: the message is the sentence that explains the threshold,
        // not a generic "a field is required".
        return Err(UiError::new(
            "drawer.needs_reason",
            preview.reason_says.clone(),
        ));
    }

    let counted = Money::from_paise(preview.counted.paise);
    let expected = Money::from_paise(preview.expected.paise);
    let variance = Money::from_paise(preview.variance.paise);
    // This press counts ONE DRAWER, this till's, for this shift.
    let shift_no = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                mb_db::Repos::new(tx)
                    .money()
                    .next_shift(OUTLET, day, app.terminal_id())
            })
            .map_err(|e| words::from_db(&e))
    })?;
    let close = DayClose {
        id: format!(
            "close_{}_{}_{shift_no}",
            day.days_since_epoch(),
            app.terminal_id()
        ),
        terminal: Some(app.terminal_id().to_owned()),
        shift_no,
        business_day: day,
        opening_float: Money::from_paise(preview.drawer.first().map_or(0, |row| row.amount.paise)),
        expected_cash: expected,
        counted_cash: counted,
        variance,
        // A drawer count never locks anything; the day does that for itself.
        is_locked: false,
        closed_at: at,
        closed_by: Some(who.staff_id.clone()),
        note: (!reason.is_empty()).then(|| reason.clone()),
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.money().save_day_close(OUTLET, &close)?;
                repos.money().save_denominations(
                    &close.id,
                    &counts
                        .iter()
                        .map(|c| Denomination {
                            value: Money::from_paise(i64::from(c.value)),
                            count: c.count,
                        })
                        .collect::<Vec<_>>(),
                )?;

                // The last till to count writes the shop's figure: the sum of each till's
                // LATEST count, so a till that counted twice is in it once.
                let waiting = repos.money().tills_still_open(OUTLET, day)?;
                if waiting.is_empty() {
                    let mut latest: BTreeMap<String, DayClose> = BTreeMap::new();
                    for drawer in repos.money().drawer_closes(OUTLET, day)? {
                        if let Some(till) = drawer.terminal.clone() {
                            latest.insert(till, drawer);
                        }
                    }
                    let sum = |pick: fn(&DayClose) -> Money| {
                        Money::try_sum(latest.values().map(pick)).unwrap_or(Money::ZERO)
                    };
                    repos.money().save_day_close(
                        OUTLET,
                        &DayClose {
                            id: format!("close_{}", day.days_since_epoch()),
                            terminal: None,
                            shift_no: 0,
                            business_day: day,
                            opening_float: sum(|c| c.opening_float),
                            expected_cash: sum(|c| c.expected_cash),
                            counted_cash: sum(|c| c.counted_cash),
                            variance: sum(|c| c.variance),
                            is_locked: false,
                            closed_at: at,
                            closed_by: Some(who.staff_id.clone()),
                            note: (!reason.is_empty()).then(|| reason.clone()),
                        },
                    )?;
                }

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::DRAWER_COUNTED,
                        "drawer",
                    )
                    .about(day.to_string())
                    .with_after(serde_json::json!({
                        "terminal": app.terminal_id(),
                        "shift": shift_no,
                        "expected_paise": expected.paise(),
                        "counted_paise": counted.paise(),
                        "variance_paise": variance.paise(),
                        "waiting_on": waiting,
                        "reason": reason,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    if print {
        // A failed print must not lose the count: it is recorded either way, and the slip can
        // be printed again.
        if let Err(e) = print_slip(app, &close, &counts) {
            crate::log_warn!("the counting slip could not be queued: {}", e.message);
        }
    }

    drawer_on(app, Some(counts))
}

/// Put the slip on paper.
fn print_slip(app: &App, close: &DayClose, counts: &[CountArg]) -> UiResult<()> {
    let config = app.shop_config();
    let store = config.store.to_print_store();
    let view = drawer_on(app, Some(counts.to_vec()))?;

    let takings: Vec<mb_print::template::SlipLine> = view
        .takings
        .iter()
        .chain(view.drawer.iter())
        .map(|row| mb_print::template::SlipLine {
            label: row.label.clone(),
            amount: row.amount.text.clone(),
        })
        .collect();
    let counted: Vec<mb_print::template::CountedNote> = view
        .denominations
        .iter()
        .filter(|row| row.count > 0)
        .map(|row| mb_print::template::CountedNote {
            label: row.label.clone(),
            count: row.count,
            total: row.total.text.clone(),
        })
        .collect();
    // The slip prints the difference in the same words the screen showed, and in capitals
    // because it is the line a person looks for first.
    let variance = view.variance_says.to_uppercase();

    // Wherever a bill would go.
    let printer = crate::flows::default_printer(app)?;

    let document = mb_print::template::day_close_document(
        printer.paper,
        &mb_print::template::DayCloseContext {
            store: &store,
            day: &view.day,
            closed: &view.counted_says,
            takings: &takings,
            drawer: &[],
            counted: &counted,
            counted_total: &view.counted.text,
            expected: &view.expected.text,
            variance: &variance,
            reason: close.note.as_deref(),
            carried: None,
            sign_off: true,
        },
    );

    app.print(
        mb_print::queue::Job::new(
            mb_print::queue::JobKind::DayClose,
            &printer.id,
            document,
            close.business_day,
        )
        .because("counting slip".to_owned()),
    )
    .map(|_| ())
}

// The seats.

#[tauri::command]
pub fn day_state(
    app: tauri::State<'_, App>,
    holidays: Option<Vec<String>>,
) -> UiResult<DayStateView> {
    day_state_on(&app, holidays)
}

#[tauri::command]
pub fn close_pending(app: tauri::State<'_, App>, holidays: Vec<String>) -> UiResult<DayStateView> {
    close_pending_on(&app, holidays)
}

#[tauri::command]
pub fn days(app: tauri::State<'_, App>) -> UiResult<DaysView> {
    days_on(&app)
}

#[tauri::command]
pub fn close_day(app: tauri::State<'_, App>, day: String) -> UiResult<DaysView> {
    close_day_on(&app, day)
}

#[tauri::command]
pub fn mark_holiday(app: tauri::State<'_, App>, days: Vec<String>) -> UiResult<DaysView> {
    set_holiday_on(&app, days, true)
}

#[tauri::command]
pub fn unmark_holiday(app: tauri::State<'_, App>, days: Vec<String>) -> UiResult<DaysView> {
    set_holiday_on(&app, days, false)
}

#[tauri::command]
pub fn reopen_day(app: tauri::State<'_, App>, day: String, reason: String) -> UiResult<DaysView> {
    reopen_day_on(&app, day, reason)
}

#[tauri::command]
pub fn count_cash(
    app: tauri::State<'_, App>,
    counts: Option<Vec<CountArg>>,
) -> UiResult<DrawerView> {
    drawer_on(&app, counts)
}

#[tauri::command]
pub fn count_drawer(
    app: tauri::State<'_, App>,
    counts: Vec<CountArg>,
    reason: String,
    print: bool,
) -> UiResult<DrawerView> {
    count_drawer_on(&app, counts, reason, print)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The difference is words, and the words are right in all three directions.
    #[test]
    fn the_difference_is_said_rather_than_signed() {
        assert_eq!(
            variance_words(Money::from_paise(-34_000)),
            "Short by 340.00."
        );
        assert_eq!(variance_words(Money::from_paise(2_000)), "Over by 20.00.");
        assert_eq!(variance_words(Money::ZERO), "The drawer matches exactly.");
    }

    /// The grid is always the whole grid, and every row multiplies correctly.
    #[test]
    fn the_count_grid_is_complete_and_adds_up() {
        let rows = grid(&[
            CountArg {
                value: 50_000,
                count: 20,
            },
            CountArg {
                value: 1_000,
                count: 6,
            },
        ]);
        assert_eq!(rows.len(), DENOMINATIONS.len(), "a row went missing");
        // Largest first, which is the order a person counts in.
        assert_eq!(rows[0].label, "500");
        assert_eq!(rows[0].total.text, "10000.00");
        assert_eq!(rows[5].label, "10");
        assert_eq!(rows[5].total.text, "60.00");
        // A denomination nobody counted is present and zero, not absent — a grid that grows as
        // somebody types moves the box under their finger.
        assert_eq!(rows[1].count, 0);
        assert_eq!(rows[1].total.text, "0.00");

        let total: i64 = rows.iter().map(|r| r.total.paise).sum();
        assert_eq!(total, 1_006_000);
    }

    /// A count for a denomination this build does not know about is ignored rather than
    /// silently added.
    #[test]
    fn a_denomination_that_does_not_exist_does_not_reach_the_total() {
        let rows = grid(&[CountArg {
            value: 200_000,
            count: 5,
        }]);
        assert!(rows.iter().all(|r| r.count == 0));
        assert_eq!(rows.iter().map(|r| r.total.paise).sum::<i64>(), 0);
    }

    #[test]
    fn the_drawer_names_the_tills_that_have_not_counted_and_is_silent_with_one() {
        assert_eq!(tills_say(1, &[]), "");
        assert_eq!(tills_say(1, &["Counter 1".to_owned()]), "");
        assert!(tills_say(2, &[]).contains("Every till"));

        let one = tills_say(2, &["Counter 2".to_owned()]);
        assert!(
            one.starts_with("1 till still to count: Counter 2."),
            "{one}"
        );
        // Why it matters, not just that it is true.
        assert!(one.contains("sum of the drawers"), "{one}");

        let three = tills_say(
            4,
            &[
                "Counter 2".to_owned(),
                "Counter 3".to_owned(),
                "Parcel".to_owned(),
            ],
        );
        assert!(
            three.contains("Counter 2, Counter 3 and Parcel"),
            "the list does not read out loud: {three}"
        );
    }

    /// The button says exactly what the press will do.
    #[test]
    fn the_button_is_labelled_by_what_it_will_do() {
        let tue = BusinessDay::from_ymd(2026, 9, 1);
        let mon = tue.previous();
        let sun = mon.previous();
        assert_eq!(action_words(&[], &[]), "");
        assert_eq!(action_words(&[tue], &[]), "Close Tuesday");
        assert_eq!(action_words(&[mon, tue], &[]), "Close 2 days");
        assert_eq!(action_words(&[], &[sun]), "Mark Sunday a holiday");
        assert_eq!(action_words(&[], &[sun, mon]), "Mark 2 holidays");
        assert_eq!(
            action_words(&[mon, tue], &[sun]),
            "Close 2 days and mark 1 holiday"
        );
        assert_eq!(
            action_words(&[tue], &[sun, mon]),
            "Close 1 day and mark 2 holidays"
        );
    }

    /// The refusal says when, and where to go.
    #[test]
    fn a_locked_day_says_when_it_closed_and_where_to_open_it() {
        let refused = locked_refusal("bill.day_closed", Timestamp::from_millis(0), "keep billing");
        assert_eq!(refused.code, "bill.day_closed");
        assert!(
            refused
                .message
                .ends_with("Open it again under Reports › Days to keep billing."),
            "{}",
            refused.message
        );
        assert!(refused.message.starts_with("That day was closed at "));
    }
}
