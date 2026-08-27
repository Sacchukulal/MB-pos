//! The settings screen's commands — and the screen is the catalogue.

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

// What the screen is handed.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub groups: Vec<GroupView>,
    /// False when there is no shop open, and the screen says so instead of drawing a form.
    pub has_shop: bool,
    /// Why, when there is no shop.
    pub trouble: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub code: String,
    pub label: String,
    /// False greys the whole section out — a courtesy.
    pub can_edit: bool,
    pub settings: Vec<SettingView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SettingView {
    pub key: String,
    /// The sub-heading this setting sits under.
    pub topic: String,
    /// The line this setting shares with the next, or empty when it has a line of its own.
    pub row: String,
    /// The word this control wears inside a shared line.
    pub short: String,
    pub label: String,
    pub help: String,
    /// `tick`, `number`, `amount`, `phone`, `words` or `choice` — what control to draw.
    pub control: String,
    /// The current value, always as text.
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

/// Which permission a section is behind.
#[must_use]
pub const fn permission_for(group: Group) -> Permission {
    match group {
        // Appearance and billing behaviour are "how this shop works", which is the same
        // authority as the shop's own details.
        Group::Store | Group::Billing => Permission::SettingsStore,
        // A tax rate is what the shop owes the government, and the day start decides which day
        // every rupee lands in.
        Group::Tax | Group::Day => Permission::SettingsTax,
        // What comes out of the printer, and what it comes out of.
        Group::Receipt | Group::Kitchen | Group::Printers => Permission::SettingsPrinter,
        // A bill number is what a GST return is a list of, so moving one is the same authority
        // as changing a tax rate.
        Group::Numbering => Permission::SettingsTax,
        // The threshold at which a count variance needs explaining is a stock-book decision, so
        // it is behind the permission that lets somebody move a stock figure at all — not
        // `settings.store`.
        Group::Stock => Permission::StockAdjust,
        // The look and the language are "how this shop presents itself".
        Group::Appearance => Permission::SettingsStore,
        Group::Backup => Permission::BackupRun,
        // A scanner, a scale, a customer display and a label printer are things somebody plugs
        // in — the same job, and usually the same person, as setting up a printer.
        Group::Devices => Permission::SettingsPrinter,
    }
}

fn control_for(kind: Kind) -> &'static str {
    match kind {
        Kind::Bool => "tick",
        Kind::Int { .. } => "number",
        Kind::Money { .. } => "amount",
        // A phone is not "words", and calling it words is why the shop's own number was the one
        // phone box in the product you could type a name into.
        Kind::Text {
            shape: super::value::Shape::Phone,
            ..
        } => "phone",
        Kind::Text { .. } => "words",
        Kind::Choice(_) => "choice",
    }
}

/// A value, as the box shows it.
fn on_the_wire(value: &Value) -> String {
    match value {
        Value::Bool(true) => "1".to_owned(),
        Value::Bool(false) => "0".to_owned(),
        Value::Int(n) => n.to_string(),
        Value::Money(m) => m.to_plain_string(),
        Value::Text(t) => t.clone(),
    }
}

/// The same journey back.
fn off_the_wire(entry: &Entry, raw: &str) -> UiResult<Value> {
    let value = match entry.kind {
        Kind::Bool => Value::Bool(raw == "1" || raw.eq_ignore_ascii_case("true")),
        Kind::Int { unit, .. } => Value::Int(raw.trim().parse::<i64>().map_err(|_| {
            UiError::new(
                "settings.invalid",
                format!(
                    "\"{raw}\" is not a number. \"{}\" wants {unit}.",
                    entry.label
                ),
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
        // A phone is normalised before it is checked.
        Kind::Text {
            shape: super::value::Shape::Phone,
            ..
        } => Value::Text(
            mb_core::Phone::parse_optional(raw)
                .map_err(|e| {
                    UiError::new(
                        "settings.invalid",
                        format!("\"{raw}\" is not a phone number — {e}."),
                    )
                    .with_detail(format!("setting {}", entry.key))
                })?
                .map_or_else(String::new, |p| p.as_str().to_owned()),
        ),
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
                    row: catalog::row_for(entry).to_owned(),
                    short: catalog::short_for(entry).to_owned(),
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
                    // `i32`, not `i64`.
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

/// The keys that match, for the screen to jump to.
pub fn search_on(app: &App, text: String) -> UiResult<Vec<String>> {
    guard::require_any(app, guard::SETTINGS_PERMISSIONS)?;
    Ok(super::search(&text)
        .into_iter()
        .map(|entry| entry.key.to_owned())
        .collect())
}

// The live preview.

/// The sample bill or ticket, laid out with the settings as they are on screen right now, saved
/// or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    pub doc: crate::preview::PreviewDoc,
    pub paper: String,
    /// The face the printer will use, named so a browser can use it too.
    pub font: String,
    /// Settings that could not be used yet, by name.
    pub not_usable_yet: Vec<String>,
}

/// Which paper the preview is drawn on — the shop's own default printer's.
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
    // The preview takes the printer's own engine, face and paper.
    let around = super::sample::around_for(app, paper, group.as_str());
    let doc = match Group::from_code(&group) {
        Some(Group::Kitchen) => super::sample::kitchen_preview(&wanted, &around),
        // Everything else previews the BILL, and on purpose: a shop changing its name or its
        // GST number wants to see where that lands on paper just as much as one changing a
        // separator.
        _ => super::sample::bill_preview(&wanted, &around),
    }
    .map_err(|e| words::from_print(&e))?;

    // The face this preview is of.
    let key = match Group::from_code(&group) {
        Some(Group::Kitchen) => wanted.kitchen.font.as_str(),
        _ => wanted.receipt.font.as_str(),
    };
    let family = mb_print::font::family(key);

    Ok(PreviewView {
        doc,
        paper: paper_label,
        // The built-in face is the only one with no Windows name, and the screen has its own
        // monospace stack for it — so it is named as nothing rather than as a family a browser
        // would fail to find.
        font: family
            .filter(|f| f.file.is_some())
            .map_or_else(String::new, |f| windows_family(f.label).to_owned()),
        not_usable_yet,
    })
}

/// The Windows family name inside a label.
fn windows_family(label: &str) -> &str {
    label.split(" — ").next().unwrap_or(label).trim()
}

/// Save what changed, and only what changed.
pub fn save_on(app: &App, edits: Vec<SettingEdit>) -> UiResult<SavedView> {
    let who = guard::require_any(app, guard::SETTINGS_PERMISSIONS)?;

    let before = app.shop_config();
    let mut wanted = before.clone();

    // 1 and 2.
    for edit in &edits {
        let Some(entry) = catalog::find(&edit.key) else {
            // Not a coercion and not a silent skip: a screen asking for a setting this build
            // does not have is a bug in one of the two, and saying so is how it gets found.
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
                        .changed(
                            json_of(&changed, |c| &c.before),
                            json_of(&changed, |c| &c.after),
                        ),
                    )?;
                }
                Ok(changed)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // Only after the commit.
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

/// The before or the after side of a change list, as one object keyed by setting.
fn json_of(changed: &[Changed], side: fn(&Changed) -> &String) -> serde_json::Value {
    serde_json::Value::Object(
        changed
            .iter()
            .map(|c| {
                (
                    (*c.key).to_owned(),
                    serde_json::Value::String(side(c).clone()),
                )
            })
            .collect(),
    )
}

/// Put one section back to how it shipped.
pub fn defaults_for_on(app: &App, group: String) -> UiResult<Vec<SettingEdit>> {
    guard::require_any(app, guard::SETTINGS_PERMISSIONS)?;
    let Some(group) = Group::from_code(&group) else {
        return Err(
            UiError::new("settings.unknown", "There is no such settings section.")
                .with_detail(group),
        );
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

/// Re-read from disk, for the moments the configuration may have changed under us — after a
/// restore, and when a screen wants to be sure.
pub fn reload_on(app: &App) -> UiResult<SettingsView> {
    app.reload_shop_config();
    all_on(app)
}

// The whole configuration, out and in — so a dealer sets up the second shop in a minute instead
// of an afternoon.

/// What an import WOULD do, before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ConfigPlanView {
    pub changes: Vec<ChangeView>,
    /// Keys this build has never heard of.
    pub unknown: Vec<String>,
    /// Why it cannot be used.
    pub problems: Vec<String>,
    pub usable: bool,
}

/// Where the file goes.
fn config_file(app: &App) -> std::path::PathBuf {
    app.with_shop(|shop| {
        Ok(shop
            .path
            .parent()
            .map_or_else(
                || std::path::PathBuf::from("."),
                std::path::Path::to_path_buf,
            )
            .join("magic-bill-settings.json"))
    })
    .unwrap_or_else(|_| mb_db::locate::default_config_dir().join("magic-bill-settings.json"))
}

/// Export. Every setting as `key: value`, sorted, with a version line.
pub fn export_on(app: &App) -> UiResult<String> {
    guard::require(app, Permission::SettingsStore)?;
    let map = super::to_map(&app.shop_config());
    let text = serde_json::to_string_pretty(&map).map_err(|e| {
        UiError::new(
            "settings.export",
            "This shop's settings could not be written out.",
        )
        .with_detail(e.to_string())
    })?;

    let path = config_file(app);
    std::fs::write(&path, &text).map_err(|e| {
        UiError::new(
            "settings.export",
            format!("The settings could not be written to {}.", path.display()),
        )
        .with_detail(e.to_string())
    })?;
    log_info!("the settings were written to {}", path.display());
    Ok(path.display().to_string())
}

fn read_config_file(text: &str) -> UiResult<std::collections::BTreeMap<String, serde_json::Value>> {
    serde_json::from_str(text).map_err(|e| {
        UiError::new(
            "settings.import",
            "That file is not a Magic Bill settings file.",
        )
        .with_detail(e.to_string())
    })
}

/// The dry run is the feature.
pub fn plan_config_import_on(app: &App, text: String) -> UiResult<ConfigPlanView> {
    guard::require_any(app, guard::SETTINGS_PERMISSIONS)?;
    let file = read_config_file(&text)?;
    let (_, plan) = super::plan_import(&app.shop_config(), &file);
    Ok(ConfigPlanView {
        changes: plan.changes.iter().map(ChangeView::from).collect(),
        unknown: plan.unknown.clone(),
        problems: plan.problems.clone(),
        usable: plan.is_usable(),
    })
}

/// An import writes tax rates and printer setup, so it needs ALL FOUR permissions, not the one
/// that happens to cover the first key in the file.
pub fn run_config_import_on(app: &App, text: String) -> UiResult<SavedView> {
    for need in guard::SETTINGS_PERMISSIONS {
        guard::require(app, *need)?;
    }
    let file = read_config_file(&text)?;
    let before = app.shop_config();
    let (wanted, plan) = super::plan_import(&before, &file);
    if !plan.is_usable() {
        return Err(UiError::new(
            "settings.import",
            format!(
                "This file cannot be used, so nothing has been changed: {}",
                plan.problems.join(" ")
            ),
        ));
    }

    // Straight through `save_on`, so an import obeys exactly the rules a person typing obeys.
    let edits = catalog::CATALOG
        .iter()
        .map(|entry| SettingEdit {
            key: entry.key.to_owned(),
            value: on_the_wire(&(entry.read)(&wanted)),
        })
        .collect();
    save_on(app, edits)
}

// The seats.

#[tauri::command]
pub fn export_settings(app: tauri::State<'_, App>) -> UiResult<String> {
    export_on(&app)
}

#[tauri::command]
pub fn plan_settings_import(app: tauri::State<'_, App>, text: String) -> UiResult<ConfigPlanView> {
    plan_config_import_on(&app, text)
}

#[tauri::command]
pub fn run_settings_import(app: tauri::State<'_, App>, text: String) -> UiResult<SavedView> {
    run_config_import_on(&app, text)
}

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
pub fn save_settings(app: tauri::State<'_, App>, edits: Vec<SettingEdit>) -> UiResult<SavedView> {
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

/// A shop that will not answer is worth one line in the log rather than a silent empty screen.
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
