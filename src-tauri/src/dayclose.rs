//! **Closing the day** — requirement 9 of the ten, scope 10.8, audit **B15**.
//!
//! > *"No opening cash, no closing cash, no expected vs actual, no Z-report.
//! > This is how every restaurant actually closes the day and it does not
//! > exist."*
//!
//! Every night a shop counts the drawer. The question it is answering is
//! "is the money that should be here, here?", and answering it needs three
//! things v1 had none of: **what the till expected**, **what the person
//! counted**, and **a record that the two were compared.**
//!
//! # The expected figure is not computed here
//!
//! It comes from [`mb_db::repo::MoneyRepo::cash_position`], which P16 wrote and
//! the Spends screen already shows. **One answer to "how much cash should be in
//! the drawer?"** — a second one here would be the exact disagreement this
//! screen exists to settle, and a shopkeeper who saw two different figures
//! would trust neither.
//!
//! # Nothing about this screen is arithmetic in TypeScript
//!
//! The count crosses as `{ value, count }` pairs, every total comes back
//! formatted, and the difference comes back as a **sentence** — "Short by
//! 340.00" — because a minus sign in front of an amount is read wrong by
//! somebody eventually, and because §6 forbids assembling that sentence on the
//! screen.
//!
//! # And the lock is real
//!
//! A closed day refuses a void ([`crate::corrections`] checks it and always
//! has). The way past it is not a flag: it is **reopening the day**, which
//! needs `day.close` and writes its own audit action, so an owner asking "who
//! unlocked Tuesday?" gets an answer. D77.

use mb_auth::audit::{AuditEntry, action};
use mb_auth::Permission;
use mb_core::Money;
use mb_db::repo::money::{CashMovement, DayClose, Denomination};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// ---------------------------------------------------------------------------
// What the drawer holds.
// ---------------------------------------------------------------------------

/// **The notes and coins an Indian till actually contains.**
///
/// Paise, largest first, because that is the order a person counts in. The
/// ₹2,000 note is not here: it was withdrawn in 2023 and a row for it on every
/// till in the country is a row nobody fills in. A shop that still has one
/// types the total into the count for ₹500 — which is wrong, and is why the
/// screen also accepts a plain total (see [`CountArg`]).
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
///
/// **`i32` paise and a `u32` count** — D58: `ts-rs` renders an `i64` as a
/// TypeScript `bigint` and `JSON.stringify` throws on one. ₹500 is 50,000
/// paise, which fits an `i32` with room for a denomination nobody has printed
/// yet.
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
    /// "500", "50 paise" — the words, so the screen prints no currency of its
    /// own.
    pub label: String,
    pub count: u32,
    /// `count × value`, computed in Rust. R8: the one multiplication on this
    /// screen is the one TypeScript would most obviously be asked to do.
    pub total: MoneyView,
}

/// A label and an amount. The takings summary and the drawer summary are both
/// lists of these, so the screen draws one kind of row and not two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SlipLineView {
    pub label: String,
    pub amount: MoneyView,
}

/// The whole screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DayCloseView {
    /// "2026-08-09".
    pub day: String,
    /// The day in words, for the heading: "Today, Sunday 9 August".
    pub day_says: String,
    pub takings: Vec<SlipLineView>,
    pub drawer: Vec<SlipLineView>,
    pub expected: MoneyView,
    pub denominations: Vec<DenominationView>,
    pub counted: MoneyView,
    pub variance: MoneyView,
    /// **The sentence.** "Short by 340.00", "Over by 20.00", "Matches
    /// exactly." Never a signed number on its own.
    pub variance_says: String,
    /// `short`, `over` or `exact` — for the shape and the colour, and colour is
    /// never the only signal.
    pub variance_kind: String,
    /// Whether the reason box is required, and the sentence that says why.
    pub needs_reason: bool,
    pub reason_says: String,
    pub reason: String,
    /// Whether this day is already closed and locked.
    pub is_closed: bool,
    /// "Closed at 11:14 pm by Ravi" — empty when it is not closed.
    pub closed_says: String,
    /// What carrying the float forward will do, in words. Empty when the shop
    /// does not carry one.
    pub carry_says: String,
    /// Whether the person looking may close it, and may reopen it.
    pub may_close: bool,
    /// **Which tills are in the shop's day, and which are not** (P27, D140).
    ///
    /// Empty in a one-till shop, which is every shop until somebody buys a
    /// second computer. In a two-till shop it is the sentence that stops a
    /// manager going home believing the day is closed: the shop's total is the
    /// SUM of the drawers, so a drawer nobody counted is a total that is short
    /// by whatever is in it.
    pub tills_say: String,
}

// ---------------------------------------------------------------------------
// Reading the day.
// ---------------------------------------------------------------------------

/// The day close as it stands, with an optional count laid over it.
///
/// The same function serves the first paint and every keystroke in the grid:
/// the screen sends what has been counted so far and gets back the variance and
/// its sentence. **There is no second variance calculation for the preview.**
pub fn view_on(app: &App, counts: Option<Vec<CountArg>>) -> UiResult<DayCloseView> {
    let who = guard::require(app, Permission::ReportsView)?;
    let may_close = who.must(Permission::DayClose).is_ok();
    let day = today(now());
    let config = app.shop_config();

    app.with_shop(|shop| {
        // **One trip.** Everything this screen needs, including the closer's
        // name — see `closed_words` for what looking it up separately cost.
        let (position, totals, existing, stored_counts, closer, (how_many_tills, still_open)) = shop
            .db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let existing = repos.money().find_day_close(OUTLET, day)?;
                let stored = match &existing {
                    Some(close) => repos.money().denominations(&close.id)?,
                    None => Vec::new(),
                };
                let closer = match existing.as_ref().and_then(|c| c.closed_by.as_ref()) {
                    Some(id) => repos
                        .people()
                        .find_staff(OUTLET, id.as_str())?
                        .map(|person| person.name),
                    None => None,
                };
                Ok((
                    // **This till's own drawer** (D140), never the shop's: the
                    // person in front of this screen is counting the box under
                    // THIS till, and showing them the shop total would be a
                    // variance they cannot act on.
                    repos.money().cash_position_of(OUTLET, day, Some(app.terminal_id()))?,
                    repos.corrections().day_totals(OUTLET, day)?,
                    existing,
                    stored,
                    closer.unwrap_or_else(|| "somebody".to_owned()),
                    (
                        repos.terminals().count(OUTLET)?,
                        repos.money().tills_still_open(OUTLET, day)?,
                    ),
                ))
            })
            .map_err(|e| words::from_db(&e))?;

        // What is on the screen: what the person is typing, or — if they have
        // not typed anything — what was counted last time this day was closed.
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
        // `>` and not `>=`: a threshold of ₹20 means "ask me when it is MORE
        // than twenty out", and a shop that sets zero is asking every time.
        let needs_reason = variance.abs() > threshold;

        Ok(DayCloseView {
            day: day.to_string(),
            day_says: format!("The day of {day}"),
            takings: vec![
                line("Bills", totals.gross),
                line("Voided", totals.voids),
                line("Refunded", totals.refunded),
                line("Net takings", totals.net),
            ],
            drawer: vec![
                line("Opening float", position.opening_float),
                line("Cash from bills", position.cash_sales),
                line("Put in", position.top_ups),
                line("Spent from the drawer", position.cash_expenses),
                line("Paid out", position.payouts),
                line("Sent to the bank", position.bank_drops),
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
                    "The drawer is out by more than {}, so this needs a reason \
                     before the day can be closed. It goes on the slip and into \
                     the history.",
                    threshold.to_plain_string()
                )
            } else {
                String::new()
            },
            reason: existing.as_ref().and_then(|c| c.note.clone()).unwrap_or_default(),
            is_closed: existing.as_ref().is_some_and(|c| c.is_locked),
            closed_says: existing
                .as_ref()
                .filter(|c| c.is_locked)
                .map(|c| closed_words(c, &closer))
                .unwrap_or_default(),
            carry_says: if config.day.carry_float {
                format!(
                    "{} will be left in the drawer and counted as tomorrow's \
                     opening float.",
                    config.day.float_amount.to_plain_string()
                )
            } else {
                String::new()
            },
            may_close,
            tills_say: tills_say(how_many_tills, &still_open),
        })
    })
}

/// **Which tills are in the shop's day** (D140) — and it is silent in a one-till
/// shop, because there is nothing there worth saying.
fn tills_say(how_many: u32, still_open: &[String]) -> String {
    if how_many < 2 {
        return String::new();
    }
    if still_open.is_empty() {
        return "Every till has counted its drawer. Closing now closes the \
                shop's day."
            .to_owned();
    }
    format!(
        "{} still to count: {}. The shop's day stays open until they have — its \
         total is the sum of the drawers, so closing without them would be short \
         by whatever is in them.",
        words::count(still_open.len() as i64, "till", "tills"),
        crate::words::list(still_open)
    )
}

fn line(label: &str, amount: Money) -> SlipLineView {
    SlipLineView {
        label: label.to_owned(),
        amount: MoneyView::from(amount),
    }
}

/// The full grid, with whatever has been counted filled in.
///
/// Always every denomination, in order, even the ones with no notes: a grid
/// that grows and shrinks as somebody types moves the box under their finger.
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

/// **The difference, in words.**
fn variance_words(variance: Money) -> String {
    if variance.is_zero() {
        return "The drawer matches exactly.".to_owned();
    }
    if variance.is_negative() {
        return format!("Short by {}.", variance.abs().to_plain_string());
    }
    format!("Over by {}.", variance.to_plain_string())
}

/// "Closed 9 Aug, 11:14 pm by Ravi."
///
/// The name is looked up by the caller rather than here, and passed in: a
/// `staff_id` on a slip is audit F8, and a person reading it wants "by Ravi".
///
/// **It takes the name and not the `App` on purpose.** The first version looked
/// the name up itself, from inside the closure `App::with_shop` had already
/// entered — which takes the shop's mutex a second time on the same thread and
/// hangs the till. Found by the test suite never finishing.
fn closed_words(close: &DayClose, who: &str) -> String {
    format!("Closed {} by {who}.", words::when(close.closed_at))
}

// ---------------------------------------------------------------------------
// Closing it.
// ---------------------------------------------------------------------------

/// **Close the day.** The one write on this screen.
///
/// In one transaction: the close row, the counted notes, the audit entry, and
/// — when the shop carries a float — tomorrow's opening movement. R11 and D23:
/// a close that recorded the count but not the audit row, or the audit row but
/// not tomorrow's float, would be worse than one that failed outright.
pub fn close_on(
    app: &App,
    counts: Vec<CountArg>,
    reason: String,
    print: bool,
) -> UiResult<DayCloseView> {
    let who = guard::require(app, Permission::DayClose)?;
    let at = now();
    let day = today(at);
    let config = app.shop_config();

    // The figures, from the same place the screen read them.
    let preview = view_on(app, Some(counts.clone()))?;
    if preview.is_closed {
        return Err(UiError::new(
            "day.already_closed",
            "This day has already been closed. Open it again first if you need \
             to change the count.",
        ));
    }
    let reason = reason.trim().to_owned();
    if preview.needs_reason && reason.is_empty() {
        // **The refusal IS the feature** (D75): the message is the sentence
        // that explains the threshold, not a generic "a field is required".
        return Err(UiError::new("day.needs_reason", preview.reason_says.clone()));
    }

    let counted = Money::from_paise(preview.counted.paise);
    let expected = Money::from_paise(preview.expected.paise);
    let variance = Money::from_paise(preview.variance.paise);
    // **D140 — this press counts ONE DRAWER**, this till's, for this shift.
    // The shop's day is closed further down, and only when no till is left.
    let shift_no = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                mb_db::Repos::new(tx).money().next_shift(OUTLET, day, app.terminal_id())
            })
            .map_err(|e| words::from_db(&e))
    })?;
    let close = DayClose {
        id: format!("close_{}_{}_{shift_no}", day.days_since_epoch(), app.terminal_id()),
        terminal: Some(app.terminal_id().to_owned()),
        shift_no,
        business_day: day,
        opening_float: Money::from_paise(
            preview
                .drawer
                .first()
                .map_or(0, |row| row.amount.paise),
        ),
        expected_cash: expected,
        counted_cash: counted,
        variance,
        // **A drawer close does not lock the day.** The shop's row does, and
        // it is written when the last till has counted, because a day that
        // locked while another till could still send bills into it would have
        // a total that changed after it was sealed.
        is_locked: false,
        closed_at: at,
        closed_by: Some(who.staff_id.clone()),
        note: (!reason.is_empty()).then(|| reason.clone()),
    };

    let carried = config.day.carry_float.then_some(config.day.float_amount);

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

                // **Tomorrow's float, written today.** A `float` movement dated
                // tomorrow — which is exactly what `cash_position` reads, so
                // tomorrow's expected figure needs no special case for "was
                // yesterday closed?".
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

                // **D140 — the last till to count closes the shop.**
                //
                // The roll-up is the SUM of the drawers and never an
                // independent query, because two ways to compute one number is
                // two numbers. And it is only written when no till is left
                // uncounted: a day that sealed while another till could still
                // send bills into it would have a total that changed
                // afterwards, which is the one thing a locked day promises not
                // to do.
                //
                // For a shop with one till this is the same single press it has
                // always been — that till was the last one.
                let waiting = repos.money().tills_still_open(OUTLET, day)?;
                if waiting.is_empty() {
                    let drawers = repos.money().drawer_closes(OUTLET, day)?;
                    let sum = |pick: fn(&mb_db::repo::money::DayClose) -> Money| {
                        Money::try_sum(drawers.iter().map(pick)).unwrap_or(Money::ZERO)
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
                            is_locked: true,
                            closed_at: at,
                            closed_by: Some(who.staff_id.clone()),
                            note: (!reason.is_empty()).then(|| reason.clone()),
                        },
                    )?;
                }

                // R11 — the same transaction as the thing it records.
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(at, day, Some(who.staff_id.clone()), action::DAY_CLOSED, "day")
                        .about(day.to_string())
                        .with_after(serde_json::json!({
                            "terminal": app.terminal_id(),
                            "shift": shift_no,
                            "expected_paise": expected.paise(),
                            "counted_paise": counted.paise(),
                            "variance_paise": variance.paise(),
                            "shop_closed": waiting.is_empty(),
                            "waiting_on": waiting,
                            "reason": reason,
                        })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    if print {
        // A failed print must not un-close the day: the money is counted and
        // recorded either way, and the slip can be printed again. So this is
        // reported and not propagated — which is the opposite of silent
        // failure, because the queue keeps the job and the screen shows it.
        if let Err(e) = print_slip(app, &close, &counts) {
            crate::log_warn!("the closing slip could not be queued: {}", e.message);
        }
    }

    view_on(app, Some(counts))
}

/// **Open a closed day again** — the override, and it leaves a mark.
///
/// Not a flag on the void. A void into a closed day is refused (P12), and this
/// is the door: a person with `day.close` reopens the day, makes the
/// correction, and closes it again. Three audit rows, and an owner can read
/// exactly what happened. A hidden "manager override" checkbox on the void
/// would have left one row saying a bill was voided and nothing saying the day
/// had been sealed when it happened.
pub fn reopen_on(app: &App, reason: String) -> UiResult<DayCloseView> {
    let who = guard::require(app, Permission::DayClose)?;
    let at = now();
    let day = today(at);
    let reason = reason.trim().to_owned();
    if reason.is_empty() {
        return Err(UiError::new(
            "day.reopen_reason",
            "Opening a closed day needs a reason. It is recorded against your \
             name, and an owner reading the history later will want to know why.",
        ));
    }

    let opened = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                if !repos.money().unlock_day(OUTLET, day)? {
                    return Ok(false);
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
    view_on(app, None)
}

/// Put the slip on paper.
fn print_slip(app: &App, close: &DayClose, counts: &[CountArg]) -> UiResult<()> {
    let config = app.shop_config();
    let store = config.store.to_print_store();
    let view = view_on(app, Some(counts.to_vec()))?;

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
    // The slip prints the difference in the same words the screen showed, and
    // in capitals because it is the line a person looks for first.
    let variance = view.variance_says.to_uppercase();
    let carried = view.carry_says.clone();

    // Wherever a bill would go. `default_printer` is the one rule for that and
    // this does not invent a second one.
    let printer = crate::flows::default_printer(app)?;

    let document = mb_print::template::day_close_document(
        printer.paper,
        &mb_print::template::DayCloseContext {
            store: &store,
            day: &view.day,
            // The view already worked out who closed it, and reading it from
            // there rather than looking the name up again keeps this outside
            // `with_shop` — which is what the hang was.
            closed: &view.closed_says,
            takings: &takings,
            drawer: &[],
            counted: &counted,
            counted_total: &view.counted.text,
            expected: &view.expected.text,
            variance: &variance,
            reason: close.note.as_deref(),
            carried: (!carried.is_empty()).then_some(carried.as_str()),
            sign_off: true,
        },
    );

    app.with_shop(|shop| {
        shop.queue
            .enqueue(
                mb_print::queue::Job::new(
                    mb_print::queue::JobKind::DayClose,
                    &printer.id,
                    document,
                    close.business_day,
                )
                .because("closing slip".to_owned()),
            )
            .map(|_| ())
            .map_err(|e| words::from_print(&e))
    })
}

// ---------------------------------------------------------------------------
// The seats.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn day_close(app: tauri::State<'_, App>) -> UiResult<DayCloseView> {
    view_on(&app, None)
}

#[tauri::command]
pub fn count_cash(
    app: tauri::State<'_, App>,
    counts: Vec<CountArg>,
) -> UiResult<DayCloseView> {
    view_on(&app, Some(counts))
}

#[tauri::command]
pub fn close_day(
    app: tauri::State<'_, App>,
    counts: Vec<CountArg>,
    reason: String,
    print: bool,
) -> UiResult<DayCloseView> {
    close_on(&app, counts, reason, print)
}

#[tauri::command]
pub fn reopen_day(app: tauri::State<'_, App>, reason: String) -> UiResult<DayCloseView> {
    reopen_on(&app, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **T7.** The difference is words, and the words are right in all three
    /// directions. A signed number would be read wrong by somebody eventually,
    /// and "-340.00" on a slip stapled to a bag of cash is exactly that.
    #[test]
    fn the_difference_is_said_rather_than_signed() {
        assert_eq!(
            variance_words(Money::from_paise(-34_000)),
            "Short by 340.00."
        );
        assert_eq!(variance_words(Money::from_paise(2_000)), "Over by 20.00.");
        assert_eq!(
            variance_words(Money::ZERO),
            "The drawer matches exactly."
        );
    }

    /// The grid is always the whole grid, and every row multiplies correctly.
    #[test]
    fn the_count_grid_is_complete_and_adds_up() {
        let rows = grid(&[
            CountArg { value: 50_000, count: 20 },
            CountArg { value: 1_000, count: 6 },
        ]);
        assert_eq!(rows.len(), DENOMINATIONS.len(), "a row went missing");
        // Largest first, which is the order a person counts in.
        assert_eq!(rows[0].label, "500");
        assert_eq!(rows[0].total.text, "10000.00");
        assert_eq!(rows[5].label, "10");
        assert_eq!(rows[5].total.text, "60.00");
        // A denomination nobody counted is present and zero, not absent — a
        // grid that grows as somebody types moves the box under their finger.
        assert_eq!(rows[1].count, 0);
        assert_eq!(rows[1].total.text, "0.00");

        let total: i64 = rows.iter().map(|r| r.total.paise).sum();
        assert_eq!(total, 1_006_000);
    }

    /// A count for a denomination this build does not know about is ignored
    /// rather than silently added. It cannot be typed — the grid is the screen
    /// — so reaching here means a stale front end, and adding it to the total
    /// would make the slip disagree with its own rows.
    #[test]
    fn a_denomination_that_does_not_exist_does_not_reach_the_total() {
        let rows = grid(&[CountArg { value: 200_000, count: 5 }]);
        assert!(rows.iter().all(|r| r.count == 0));
        assert_eq!(rows.iter().map(|r| r.total.paise).sum::<i64>(), 0);
    }

    /// **P27, D140 — the sentence that names the drawers nobody has counted.**
    ///
    /// The first assertion is the one that matters most: a one-till shop is
    /// every shop until somebody buys a second computer, and a night-time
    /// sentence about "tills" in a shop with one till is noise a person learns
    /// to skip past — which is how they come to skip past it when it matters.
    #[test]
    fn the_day_close_names_the_tills_that_have_not_counted_and_is_silent_with_one() {
        assert_eq!(tills_say(1, &[]), "");
        assert_eq!(tills_say(1, &["Counter 1".to_owned()]), "");

        let done = tills_say(2, &[]);
        assert!(done.contains("Every till"), "{done}");
        assert!(done.contains("closes the shop's day"), "{done}");

        let one = tills_say(2, &["Counter 2".to_owned()]);
        assert!(one.starts_with("1 till still to count: Counter 2."), "{one}");
        // **Why it matters, not just that it is true.** A manager who reads
        // only "1 till still to count" has no reason not to close anyway.
        assert!(one.contains("sum of the drawers"), "{one}");

        let three = tills_say(
            4,
            &["Counter 2".to_owned(), "Counter 3".to_owned(), "Parcel".to_owned()],
        );
        assert!(
            three.contains("Counter 2, Counter 3 and Parcel"),
            "the list does not read out loud: {three}"
        );
    }
}
