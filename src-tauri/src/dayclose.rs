//! Closing the day.

use mb_auth::Permission;
use mb_auth::audit::{AuditEntry, action};
use mb_core::Money;
use mb_db::repo::money::{CashMovement, DayClose, Denomination};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// What the drawer holds.

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

/// The whole screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DayCloseView {
    pub day: String,
    /// The day in words, for the heading: "Today, Sunday 9 August".
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
    /// Whether this day is already closed and locked.
    pub is_closed: bool,
    /// "Closed at 11:14 pm by Ravi" — empty when it is not closed.
    pub closed_says: String,
    /// What carrying the float forward will do, in words.
    pub carry_says: String,
    /// Whether the person looking may close it, and may reopen it.
    pub may_close: bool,
    /// Which tills are in the shop's day, and which are not.
    pub tills_say: String,
    /// "Table 7 #12", "Parcel #13" — the orders nobody has finished.
    pub open_orders: Vec<String>,
    /// The same as one sentence, or empty.
    pub open_says: String,
}

// Reading the day.

/// The day close as it stands, with an optional count laid over it.
pub fn view_on(app: &App, counts: Option<Vec<CountArg>>) -> UiResult<DayCloseView> {
    let who = guard::require(app, Permission::ReportsView)?;
    let may_close = who.must(Permission::DayClose).is_ok();
    let day = today(now());
    let config = app.shop_config();

    app.with_shop(|shop| {
        // One trip. Everything this screen needs, including the closer's name — see
        // `closed_words` for what looking it up separately cost.
        let (
            position,
            totals,
            existing,
            stored_counts,
            closer,
            (how_many_tills, still_open),
            open_orders,
        ) = shop
            .db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let tables = repos.floor().list_tables(OUTLET)?;
                let open_orders: Vec<String> = repos
                    .orders()
                    .list_open(OUTLET)?
                    .iter()
                    .map(|order| crate::kitchen::place_and_token(order, &tables))
                    .collect();
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
                    // This till's own drawer, never the shop's: the person in front of this
                    // screen is counting the box under THIS till, and showing them the shop
                    // total would be a variance they cannot act on.
                    repos
                        .money()
                        .cash_position_of(OUTLET, day, Some(app.terminal_id()))?,
                    repos.corrections().day_totals(OUTLET, day)?,
                    existing,
                    stored,
                    closer.unwrap_or_else(|| "somebody".to_owned()),
                    (
                        repos.terminals().count(OUTLET)?,
                        repos.money().tills_still_open(OUTLET, day)?,
                    ),
                    open_orders,
                ))
            })
            .map_err(|e| words::from_db(&e))?;

        // What is on the screen: what the person is typing, or — if they have not typed
        // anything — what was counted last time this day was closed.
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

        Ok(DayCloseView {
            day: day.to_string(),
            day_says: format!("The day of {day}"),
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
                    "The drawer is out by more than {}, so this needs a reason \
                     before the day can be closed. It goes on the slip and into \
                     the history.",
                    threshold.to_plain_string()
                )
            } else {
                String::new()
            },
            reason: existing
                .as_ref()
                .and_then(|c| c.note.clone())
                .unwrap_or_default(),
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
            open_says: open_words(&open_orders),
            open_orders,
        })
    })
}

/// The orders nobody has finished, as one sentence — or nothing.
fn open_words(open: &[String]) -> String {
    if open.is_empty() {
        return String::new();
    }
    format!(
        "{} still open: {}. Settle or cancel them before closing the day.",
        words::count(
            i64::try_from(open.len()).unwrap_or(0),
            "order is",
            "orders are"
        ),
        open.join(", ")
    )
}

/// Which tills are in the shop's day — and it is silent in a one-till shop, because there is
/// nothing there worth saying.
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

/// "Closed 9 Aug, 11:14 pm by Ravi.".
fn closed_words(close: &DayClose, who: &str) -> String {
    format!("Closed {} by {who}.", words::when(close.closed_at))
}

// Closing it.

/// Close the day. The one write on this screen.
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
    if !preview.open_orders.is_empty() {
        return Err(UiError::new("day.open_orders", preview.open_says.clone()));
    }
    let reason = reason.trim().to_owned();
    if preview.needs_reason && reason.is_empty() {
        // The refusal IS the feature: the message is the sentence that explains the threshold,
        // not a generic "a field is required".
        return Err(UiError::new(
            "day.needs_reason",
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
        // A drawer close does not lock the day.
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

                // The day's totals go up to the cloud again, now with the day marked closed.
                let day_key = day.days_since_epoch().to_string();
                for table in mb_db::repo::wire::TOTALS_TABLES {
                    repos.outbox().enqueue(OUTLET, table, &day_key, mb_db::repo::Op::Upsert, at)?;
                }

                // Tomorrow's float, written today.
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

                // The last till to count closes the shop.
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
                    // Nobody will bump a finished order's ticket tomorrow.
                    repos.kitchen().close_finished(OUTLET)?;
                }

                // The same transaction as the thing it records.
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
        // A failed print must not un-close the day: the money is counted and recorded either
        // way, and the slip can be printed again.
        if let Err(e) = print_slip(app, &close, &counts) {
            crate::log_warn!("the closing slip could not be queued: {}", e.message);
        }
    }

    view_on(app, Some(counts))
}

/// Open a closed day again — the override, and it leaves a mark.
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
    // The slip prints the difference in the same words the screen showed, and in capitals
    // because it is the line a person looks for first.
    let variance = view.variance_says.to_uppercase();
    let carried = view.carry_says.clone();

    // Wherever a bill would go.
    let printer = crate::flows::default_printer(app)?;

    let document = mb_print::template::day_close_document(
        printer.paper,
        &mb_print::template::DayCloseContext {
            store: &store,
            day: &view.day,
            // The view already worked out who closed it, and reading it from there rather than
            // looking the name up again keeps this outside `with_shop` — which is what the hang
            // was.
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

    app.print(
        mb_print::queue::Job::new(
            mb_print::queue::JobKind::DayClose,
            &printer.id,
            document,
            close.business_day,
        )
        .because("closing slip".to_owned()),
    )
    .map(|_| ())
}

// The seats.

#[tauri::command]
pub fn day_close(app: tauri::State<'_, App>) -> UiResult<DayCloseView> {
    view_on(&app, None)
}

#[tauri::command]
pub fn count_cash(app: tauri::State<'_, App>, counts: Vec<CountArg>) -> UiResult<DayCloseView> {
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
    fn the_day_close_names_the_tills_that_have_not_counted_and_is_silent_with_one() {
        assert_eq!(tills_say(1, &[]), "");
        assert_eq!(tills_say(1, &["Counter 1".to_owned()]), "");

        let done = tills_say(2, &[]);
        assert!(done.contains("Every till"), "{done}");
        assert!(done.contains("closes the shop's day"), "{done}");

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
}
