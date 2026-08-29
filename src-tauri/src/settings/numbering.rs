//! The bill number and the token number.

use mb_auth::Permission;
use mb_db::CounterKind;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::guard;
use crate::log_info;
use crate::state::{App, OUTLET};
use crate::terminals::TERMINAL;
use crate::words::{self, UiError, UiResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct NumberingView {
    pub counters: Vec<CounterView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CounterView {
    /// `bill` or `token`.
    pub kind: String,
    /// "Bill number" / "Token number".
    pub label: String,
    pub help: String,
    pub prefix: String,
    /// 0 means no padding: bill 7 prints as `7`, not `0007`.
    pub pad_width: u32,
    pub reset_daily: bool,
    pub start: u32,
    /// What the NEXT one will be, already formatted.
    pub next: String,
    pub next_value: u32,
    /// What has been handed out, or `None` when nothing has.
    pub issued: Option<u32>,
    /// The whole sentence, built here.
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CounterEdit {
    pub kind: String,
    pub prefix: String,
    pub pad_width: u32,
    pub reset_daily: bool,
    pub start: u32,
    /// The next number to hand out.
    pub next_value: u32,
}

fn kind_of(text: &str) -> UiResult<CounterKind> {
    match text {
        "token" => Ok(CounterKind::Token),
        "bill" => Ok(CounterKind::Bill),
        "kot" => Ok(CounterKind::Kot),
        other => Err(UiError::new(
            "counter.unknown",
            "There are three counters here: the bill number, the token number and the \
             kitchen ticket number.",
        )
        .with_detail(other.to_owned())),
    }
}

fn view_of(counter: &mb_db::numbering::Counter) -> CounterView {
    let next = counter
        .last_issued
        .map_or(counter.start, |issued| issued.saturating_add(1));
    let width = usize::try_from(counter.pad_width.max(0)).unwrap_or(0);
    // Three series, three names. The kitchen ticket's was shown as a second "Token number",
    // and two sections with one name and different figures read as a broken counter.
    let (kind, label, help) = match counter.kind {
        CounterKind::Bill => (
            "bill",
            "Bill number",
            "The number on the bill, and the number your GST return lists. It must never go \
             backwards.",
        ),
        CounterKind::Token => (
            "token",
            "Token number",
            "The number the customer waits for. Most shops start it again every day.",
        ),
        CounterKind::Kot => (
            "kot",
            "Kitchen ticket number",
            "The number on each kitchen ticket, so the kitchen can say which one it means.",
        ),
    };
    CounterView {
        kind: kind.to_owned(),
        label: label.to_owned(),
        help: help.to_owned(),
        prefix: counter.prefix.clone(),
        pad_width: u32::try_from(counter.pad_width).unwrap_or(0),
        reset_daily: counter.reset_daily,
        start: u32::try_from(counter.start).unwrap_or(1),
        // Built here, the way the claim builds it (`numbering::claim`), so what the screen
        // shows and what prints cannot disagree.
        next: format!("{}{:0width$}", counter.prefix, next),
        next_value: u32::try_from(next).unwrap_or(1),
        issued: counter.last_issued.and_then(|n| u32::try_from(n).ok()),
        summary: match counter.last_issued {
            None => format!(
                "The next one will be {}{next:0width$} — nothing has been issued yet.",
                counter.prefix
            ),
            Some(1) => format!(
                "The next one will be {}{next:0width$} — one has been issued.",
                counter.prefix
            ),
            Some(issued) => format!(
                "The next one will be {}{next:0width$} — {issued} have been issued.",
                counter.prefix
            ),
        },
    }
}

pub fn numbering_on(app: &App) -> UiResult<NumberingView> {
    guard::require(app, Permission::SettingsTax)?;
    app.with_shop(|shop| {
        let counters = shop
            .db
            .transaction(|tx| mb_db::numbering::counters(tx, OUTLET, TERMINAL))
            .map_err(|e| words::from_db(&e))?;
        Ok(NumberingView {
            counters: counters.iter().map(view_of).collect(),
        })
    })
}

/// The most dangerous number this product lets anybody type.
const MAX_PAD: u32 = 8;

pub fn save_counter_on(app: &App, edit: CounterEdit) -> UiResult<NumberingView> {
    let who = guard::require(app, Permission::SettingsTax)?;
    let kind = kind_of(&edit.kind)?;

    if edit.pad_width > MAX_PAD {
        return Err(UiError::new(
            "counter.pad",
            format!(
                "Padding is at most {MAX_PAD} digits — {} would print a bill \
                 number wider than the paper.",
                edit.pad_width
            ),
        ));
    }
    if edit.prefix.chars().count() > 8 {
        return Err(UiError::new(
            "counter.prefix",
            "A prefix is at most eight characters, or it crowds the number off \
             a narrow bill.",
        ));
    }
    if edit.start == 0 {
        return Err(UiError::new(
            "counter.start",
            "A series starts at 1 or more. There is no bill number zero.",
        ));
    }
    if edit.next_value == 0 {
        return Err(UiError::new(
            "counter.next",
            "The next number is 1 or more. There is no bill number zero.",
        ));
    }

    let at = crate::flows::now();
    let day = crate::flows::today(at);

    // The refusal is returned as a VALUE, not as a database error.
    let refusal = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let before = mb_db::numbering::counters(tx, OUTLET, TERMINAL)?
                    .into_iter()
                    .find(|c| c.kind == kind);
                let issued = before.as_ref().and_then(|c| c.last_issued);

                // THE RULE, and it is the whole reason this screen is guarded.
                if let Some(issued) = issued
                    && i64::from(edit.next_value) <= issued
                {
                    return Ok(Some(format!(
                        "{}{issued} has already been printed and given to a \
                         customer. Setting the next one to {} would produce two \
                         bills with the same number, and your GST return is a \
                         list of bill numbers — it would be rejected. The next \
                         number can be {} or higher.",
                        if kind == CounterKind::Bill {
                            "Bill number "
                        } else {
                            "Token "
                        },
                        edit.next_value,
                        issued.saturating_add(1)
                    )));
                }

                mb_db::numbering::set_format(
                    tx,
                    OUTLET,
                    TERMINAL,
                    kind,
                    &mb_db::numbering::Format {
                        prefix: edit.prefix.clone(),
                        pad_width: i64::from(edit.pad_width),
                        reset_daily: edit.reset_daily,
                        start: i64::from(edit.start),
                    },
                )?;
                mb_db::numbering::set_next(
                    tx,
                    OUTLET,
                    TERMINAL,
                    kind,
                    u64::from(edit.next_value),
                )?;

                mb_db::Repos::new(tx).audit().append(
                    OUTLET,
                    &mb_auth::AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        mb_auth::audit::action::COUNTER_CHANGED,
                        "counters",
                    )
                    .about(edit.kind.clone())
                    .changed(
                        serde_json::json!({
                            "Next number": issued
                                .map_or_else(|| "nothing yet".to_owned(), |n| (n + 1).to_string()),
                            "Prefix": before.as_ref().map_or_else(String::new, |c| c.prefix.clone()),
                        }),
                        serde_json::json!({
                            "Next number": edit.next_value.to_string(),
                            "Prefix": edit.prefix.clone(),
                        }),
                    ),
                )?;
                Ok(None)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    if let Some(refusal) = refusal {
        return Err(UiError::new("counter.backwards", refusal));
    }

    log_info!("{} changed the {} counter", who.name, edit.kind);
    numbering_on(app)
}

#[tauri::command]
pub fn numbering(app: tauri::State<'_, App>) -> UiResult<NumberingView> {
    numbering_on(&app)
}

#[tauri::command]
pub fn save_counter(app: tauri::State<'_, App>, edit: CounterEdit) -> UiResult<NumberingView> {
    save_counter_on(&app, edit)
}
