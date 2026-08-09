//! The settings screen's commands — **and the screen is the catalogue.**
//!
//! There is no `StoreSettingsView`, no `TaxSettingsView` and no
//! `ReceiptSettingsView`. [`catalog::CATALOG`] already knows every setting's
//! label, help, limits and choices, so one view model carries all of them and
//! **one React component renders every section**. Adding a setting is a line in
//! `catalog.rs` and nothing else, ever — which is the same promise D21 makes
//! about a theme and D39 makes about a view model.
//!
//! # Everything crosses as a string
//!
//! A tick, a number, an amount and a choice all arrive as `value: String`, and
//! Rust parses each one against its own [`Kind`]. Three reasons, and the third
//! is the one that decides it:
//!
//! * `ts-rs` renders an `i64` as a TypeScript `bigint` and `invoke`'s
//!   `JSON.stringify` throws on one (**D58**);
//! * money must not become a float on the way in (**R2**), and `"15.50"` typed
//!   into a box is text until Rust says otherwise;
//! * a form is text. Every one of these is an `<input>`'s value, and inventing
//!   a union type to carry four shapes of the same box would put the parsing
//!   decision in TypeScript, where R8 says it may not live.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::Money;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::catalog::{self, Entry, Group};
use super::value::{Kind, Value};
use super::{Changed, ShopConfig};
use crate::guard;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

// ---------------------------------------------------------------------------
// What the screen is handed.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub groups: Vec<GroupView>,
    /// **False when there is no shop open**, and the screen says so instead of
    /// drawing a form.
    ///
    /// Found by running it: the configuration lives in `App` and starts as the
    /// defaults, so on a machine whose database would not open — a first run, a
    /// failed migration, a restore waiting for a restart — every setting drew
    /// perfectly and Save was the first thing that said anything. That is the
    /// exact situation audit **A5** is about, and it is the worst possible one
    /// in which to show somebody a form.
    pub has_shop: bool,
    /// Why, when there is no shop. Already in the cashier's words.
    pub trouble: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub code: String,
    pub label: String,
    /// **False greys the whole section out** — a courtesy. `save_settings`
    /// re-checks it per setting, which is the control (D45).
    pub can_edit: bool,
    pub settings: Vec<SettingView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SettingView {
    pub key: String,
    /// The sub-heading this setting sits under. The screen draws it when it
    /// changes, which is what turns thirty-nine settings into five short lists.
    pub topic: String,
    pub label: String,
    pub help: String,
    /// `tick`, `number`, `amount`, `words` or `choice` — what kind of control
    /// to draw. Words rather than a Rust type name, because the screen reads it.
    pub control: String,
    /// The current value, always as text. See the module note.
    pub value: String,
    pub choices: Vec<ChoiceView>,
    /// For a number: the range, and the unit to print beside the box.
    pub min: Option<i32>,
    pub max: Option<i32>,
    pub unit: String,
    pub max_len: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ChoiceView {
    pub value: String,
    pub label: String,
}

/// One box, on its way back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SettingEdit {
    pub key: String,
    pub value: String,
}

/// What a save did, in words, for the toast and for the screen's summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SavedView {
    pub changed: Vec<ChangeView>,
    pub settings: SettingsView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ChangeView {
    pub label: String,
    pub before: String,
    pub after: String,
}

impl From<&Changed> for ChangeView {
    fn from(changed: &Changed) -> Self {
        ChangeView {
            label: changed.label.to_owned(),
            before: changed.before.clone(),
            after: changed.after.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Reading.
// ---------------------------------------------------------------------------

/// Which permission a section is behind.
///
/// **One place, and the save path uses the same function**, so a section that
/// looks read-only cannot be written and a section that looks editable cannot
/// be refused for a reason the screen never mentioned.
#[must_use]
pub const fn permission_for(group: Group) -> Permission {
    match group {
        // Appearance and billing behaviour are "how this shop works", which is
        // the same authority as the shop's own details.
        Group::Store | Group::Billing => Permission::SettingsStore,
        // A tax rate is what the shop owes the government, and the day start
        // decides which day every rupee lands in. Both are `settings.tax`.
        Group::Tax | Group::Day => Permission::SettingsTax,
        // What comes out of the printer.
        Group::Receipt | Group::Kitchen => Permission::SettingsPrinter,
        Group::Backup => Permission::BackupRun,
    }
}

fn control_for(kind: Kind) -> &'static str {
    match kind {
        Kind::Bool => "tick",
        Kind::Int { .. } => "number",
        Kind::Money { .. } => "amount",
        Kind::Text { .. } => "words",
        Kind::Choice(_) => "choice",
    }
}

/// A value, as the box shows it.
///
/// **Money becomes rupees here** and nowhere else — `Money::to_plain_string` is
/// the only formatter in the product (R2), so "1500 paise" never reaches a
/// screen as either 1500 or 15.
fn on_the_wire(value: &Value) -> String {
    match value {
        Value::Bool(true) => "1".to_owned(),
        Value::Bool(false) => "0".to_owned(),
        Value::Int(n) => n.to_string(),
        Value::Money(m) => m.to_plain_string(),
        Value::Text(t) => t.clone(),
    }
}

/// The same journey back. **The only parser**, and it refuses rather than
/// coercing: `"abc"` in a number box is a sentence, not a zero.
fn off_the_wire(entry: &Entry, raw: &str) -> UiResult<Value> {
    let value = match entry.kind {
        Kind::Bool => Value::Bool(raw == "1" || raw.eq_ignore_ascii_case("true")),
        Kind::Int { unit, .. } => Value::Int(raw.trim().parse::<i64>().map_err(|_| {
            UiError::new(
                "settings.invalid",
                format!("\"{raw}\" is not a number. \"{}\" wants {unit}.", entry.label),
            )
            .with_detail(format!("setting {}", entry.key))
        })?),
        Kind::Money { .. } => Value::Money(Money::parse(raw.trim()).map_err(|e| {
            UiError::new(
                "settings.invalid",
                format!("\"{raw}\" is not an amount. Try 15 or 15.50."),
            )
            .with_detail(format!("setting {} — {e}", entry.key))
        })?),
        Kind::Text { .. } | Kind::Choice(_) => Value::Text(raw.to_owned()),
    };
    super::check(entry, &value).map_err(UiError::from)?;
    Ok(value)
}

fn view_of(app: &App, config: &ShopConfig, allowed: &[Permission]) -> SettingsView {
    let trouble = app.with_shop(|_| Ok(())).err().map(|e| e.message);
    let groups = Group::ALL
        .iter()
        .map(|group| GroupView {
            code: group.code().to_owned(),
            label: group.label().to_owned(),
            can_edit: allowed.contains(&permission_for(*group)),
            settings: catalog::CATALOG
                .iter()
                .filter(|entry| entry.group == *group)
                .map(|entry| SettingView {
                    key: entry.key.to_owned(),
                    topic: catalog::topic_for(entry).to_owned(),
                    label: entry.label.to_owned(),
                    help: entry.help.to_owned(),
                    control: control_for(entry.kind).to_owned(),
                    value: on_the_wire(&(entry.read)(config)),
                    choices: match entry.kind {
                        Kind::Choice(options) => options
                            .iter()
                            .map(|o| ChoiceView {
                                value: o.value.to_owned(),
                                label: o.label.to_owned(),
                            })
                            .collect(),
                        _ => Vec::new(),
                    },
                    // **`i32`, not `i64`** — D58: an `i64` crosses as a
                    // `bigint` and `JSON.stringify` throws on one. Every limit
                    // in the catalogue is a screen-sized number.
                    min: match entry.kind {
                        Kind::Int { min, .. } => i32::try_from(min).ok(),
                        _ => None,
                    },
                    max: match entry.kind {
                        Kind::Int { max, .. } => i32::try_from(max).ok(),
                        _ => None,
                    },
                    unit: match entry.kind {
                        Kind::Int { unit, .. } => unit.to_owned(),
                        _ => String::new(),
                    },
                    max_len: match entry.kind {
                        Kind::Text { max_len, .. } => u32::try_from(max_len).unwrap_or(u32::MAX),
                        _ => 0,
                    },
                })
                .collect(),
        })
        .collect();
    SettingsView {
        groups,
        has_shop: trouble.is_none(),
        trouble,
    }
}

/// Everything a person may look at, with what they may change marked.
pub fn all_on(app: &App) -> UiResult<SettingsView> {
    let who = guard::require_any(app, guard::SETTINGS_PERMISSIONS)?;
    let allowed: Vec<Permission> = guard::SETTINGS_PERMISSIONS
        .iter()
        .copied()
        .filter(|p| who.must(*p).is_ok())
        .collect();
    Ok(view_of(app, &app.shop_config(), &allowed))
}

/// **T9.** The keys that match, for the screen to jump to.
///
/// The matching lives in Rust rather than in a filter over the view, because
/// the synonym list is part of the rule — "roundoff" finding "Round off the
/// total" is a decision somebody made in `catalog.rs`, and a second copy of
/// that matching in TypeScript is a second answer waiting to disagree.
pub fn search_on(app: &App, text: String) -> UiResult<Vec<String>> {
    guard::require_any(app, guard::SETTINGS_PERMISSIONS)?;
    Ok(super::search(&text)
        .into_iter()
        .map(|entry| entry.key.to_owned())
        .collect())
}

// ---------------------------------------------------------------------------
// The live preview — audit D1, and the reason the design section is usable.
// ---------------------------------------------------------------------------

/// The sample bill or ticket, laid out with the settings **as they are on
/// screen right now**, saved or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    pub doc: crate::preview::PreviewDoc,
    /// Which paper this is, in the owner's words: "80 mm (3 inch)".
    pub paper: String,
    /// **Settings that could not be used yet, by name.**
    ///
    /// The preview redraws on every keystroke, and half-typed is a normal
    /// state: a logo width of `4` on its way to `40` is below the minimum for
    /// one keypress. Blanking the preview would punish typing and erroring on
    /// every character would be noise — so the last usable value is drawn and
    /// this says what was skipped. Nothing is silent (R3) and nothing blocks.
    pub not_usable_yet: Vec<String>,
}

/// Which paper the preview is drawn on — **the shop's own default printer's**.
///
/// Not always 80 mm: a 58 mm shop tuning its receipt against a 48-column
/// preview is tuning the wrong receipt, and `narrow_items` makes those two
/// genuinely different documents.
fn preview_paper(app: &App) -> (mb_print::paper::Paper, String) {
    use mb_print::paper::{Paper, PaperKind};
    let mm = crate::flows::default_printer(app)
        .map(|printer| match printer.paper.kind {
            PaperKind::Mm58 => 58,
            PaperKind::Mm100 => 100,
            _ => 80,
        })
        .unwrap_or(80);
    let kind = match mm {
        58 => PaperKind::Mm58,
        100 => PaperKind::Mm100,
        _ => PaperKind::Mm80,
    };
    let inches = match mm {
        58 => "2 inch",
        100 => "4 inch",
        _ => "3 inch",
    };
    (Paper::new(kind), format!("{mm} mm ({inches})"))
}

pub fn preview_on(app: &App, group: String, edits: Vec<SettingEdit>) -> UiResult<PreviewView> {
    guard::require_any(app, guard::SETTINGS_PERMISSIONS)?;

    let mut wanted = app.shop_config();
    let mut not_usable_yet = Vec::new();
    for edit in &edits {
        let Some(entry) = catalog::find(&edit.key) else {
            continue;
        };
        match off_the_wire(entry, &edit.value)
            .and_then(|value| (entry.write)(&mut wanted, &value).map_err(UiError::from))
        {
            Ok(()) => {}
            Err(_) => not_usable_yet.push(entry.label.to_owned()),
        }
    }

    let (paper, paper_label) = preview_paper(app);
    let doc = match Group::from_code(&group) {
        Some(Group::Kitchen) => super::sample::kitchen_preview(&wanted, paper),
        // Everything else previews the BILL, and on purpose: a shop changing
        // its name or its GST number wants to see where that lands on paper
        // just as much as one changing a separator.
        _ => super::sample::bill_preview(&wanted, paper),
    }
    .map_err(|e| words::from_print(&e))?;

    Ok(PreviewView {
        doc,
        paper: paper_label,
        not_usable_yet,
    })
}

// ---------------------------------------------------------------------------
// Writing.
// ---------------------------------------------------------------------------

/// **Save what changed, and only what changed.**
///
/// The order is load-bearing:
///
/// 1. every edit is parsed and validated — a form with one bad box writes
///    nothing, which is the same rule P13's import obeys and for the same
///    reason;
/// 2. every edit's *section* permission is checked, so a printer-only person
///    cannot post a tax rate through a screen that greyed it out;
/// 3. the cross-field rules run (the GSTIN against the state), because they
///    cannot be checked one box at a time;
/// 4. one transaction: the rows, the store profile if it moved, and the audit
///    row — **R11**, in the same commit as the thing it records.
pub fn save_on(app: &App, edits: Vec<SettingEdit>) -> UiResult<SavedView> {
    let who = guard::require_any(app, guard::SETTINGS_PERMISSIONS)?;

    let before = app.shop_config();
    let mut wanted = before.clone();

    // 1 and 2.
    for edit in &edits {
        let Some(entry) = catalog::find(&edit.key) else {
            // Not a coercion and not a silent skip: a screen asking for a
            // setting this build does not have is a bug in one of the two, and
            // saying so is how it gets found.
            return Err(UiError::new(
                "settings.unknown",
                "This screen asked to change something Magic Bill does not \
                 have. Restart and try again.",
            )
            .with_detail(format!("setting {}", edit.key)));
        };
        guard::require(app, permission_for(entry.group))?;
        let value = off_the_wire(entry, &edit.value)?;
        (entry.write)(&mut wanted, &value).map_err(|e| UiError::from(e.about(entry.key)))?;
    }

    // 3.
    catalog::check_gstin_against_state(&wanted).map_err(UiError::from)?;

    // 4.
    let at = crate::flows::now();
    let day = crate::flows::today(at);
    let changed = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let changed = super::save_changes(
                    &repos,
                    OUTLET,
                    &before,
                    &wanted,
                    at,
                    Some(who.staff_id.as_str()),
                )?;
                if !changed.is_empty() {
                    repos.audit().append(
                        OUTLET,
                        &AuditEntry::new(
                            at,
                            day,
                            Some(who.staff_id.clone()),
                            action::SETTING_CHANGED,
                            "settings",
                        )
                        .changed(json_of(&changed, |c| &c.before), json_of(&changed, |c| &c.after)),
                    )?;
                }
                Ok(changed)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // **Only after the commit.** A configuration published from an uncommitted
    // transaction is a counter printing settings the disk does not have.
    app.publish_shop_config(wanted);

    if changed.is_empty() {
        log_info!("{} pressed Save and nothing had changed", who.name);
    } else {
        log_info!("{} changed {} setting(s)", who.name, changed.len());
    }

    let allowed: Vec<Permission> = guard::SETTINGS_PERMISSIONS
        .iter()
        .copied()
        .filter(|p| who.must(*p).is_ok())
        .collect();
    Ok(SavedView {
        changed: changed.iter().map(ChangeView::from).collect(),
        settings: view_of(app, &app.shop_config(), &allowed),
    })
}

/// The before or the after side of a change list, as one object keyed by
/// setting. Keyed by the setting's KEY rather than its label, because the
/// label is words and words get reworded.
fn json_of(changed: &[Changed], side: fn(&Changed) -> &String) -> serde_json::Value {
    serde_json::Value::Object(
        changed
            .iter()
            .map(|c| ((*c.key).to_owned(), serde_json::Value::String(side(c).clone())))
            .collect(),
    )
}

/// Put one section back to how it shipped.
///
/// **It does not save.** It hands the screen the values it would set, and the
/// screen shows them as unsaved edits — so "reset" is something a person can
/// look at and cancel, rather than something that has already happened. Same
/// shape as P13's import dry run.
pub fn defaults_for_on(app: &App, group: String) -> UiResult<Vec<SettingEdit>> {
    guard::require_any(app, guard::SETTINGS_PERMISSIONS)?;
    let Some(group) = Group::from_code(&group) else {
        return Err(UiError::new(
            "settings.unknown",
            "There is no such settings section.",
        )
        .with_detail(group));
    };
    guard::require(app, permission_for(group))?;

    let defaults = ShopConfig::default();
    Ok(catalog::CATALOG
        .iter()
        .filter(|entry| entry.group == group)
        .map(|entry| SettingEdit {
            key: entry.key.to_owned(),
            value: on_the_wire(&(entry.read)(&defaults)),
        })
        .collect())
}

/// Re-read from disk, for the moments the configuration may have changed under
/// us — after a restore, and when a screen wants to be sure.
pub fn reload_on(app: &App) -> UiResult<SettingsView> {
    app.reload_shop_config();
    all_on(app)
}

// ---------------------------------------------------------------------------
// The seats (D46).
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn settings_all(app: tauri::State<'_, App>) -> UiResult<SettingsView> {
    all_on(&app)
}

#[tauri::command]
pub fn search_settings(app: tauri::State<'_, App>, text: String) -> UiResult<Vec<String>> {
    search_on(&app, text)
}

#[tauri::command]
pub fn preview_settings(
    app: tauri::State<'_, App>,
    group: String,
    edits: Vec<SettingEdit>,
) -> UiResult<PreviewView> {
    preview_on(&app, group, edits)
}

#[tauri::command]
pub fn save_settings(
    app: tauri::State<'_, App>,
    edits: Vec<SettingEdit>,
) -> UiResult<SavedView> {
    save_on(&app, edits)
}

#[tauri::command]
pub fn settings_defaults_for(
    app: tauri::State<'_, App>,
    group: String,
) -> UiResult<Vec<SettingEdit>> {
    defaults_for_on(&app, group)
}

#[tauri::command]
pub fn reload_settings(app: tauri::State<'_, App>) -> UiResult<SettingsView> {
    reload_on(&app)
}

/// A shop that will not answer is worth one line in the log rather than a
/// silent empty screen.
pub fn warn_if_unreadable(app: &App) {
    if let Err(e) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| super::load(&mb_db::Repos::new(tx), OUTLET))
            .map_err(|e| words::from_db(&e))
    }) && e.code != "shop.none"
    {
        log_warn!("this shop's settings will not read: {e}");
    }
}
