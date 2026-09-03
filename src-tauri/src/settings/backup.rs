//! Backup, restore, and where the shop's data actually is.

use mb_auth::Permission;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::guard;
use crate::state::App;
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BackupView {
    /// Where they go, resolved — never the empty string the setting may hold.
    pub folder: String,
    pub second_folder: String,
    /// Where the shop's live data file is, so a support call can ask for one folder.
    pub database: String,
    /// The folder that holds the whole shop: its data file, its licence, its backups.
    pub shop_folder: String,
    pub backups: Vec<BackupRowView>,
    /// The sentence at the top, and it is the whole point of the screen.
    pub headline: String,
    /// `ok`, `warn` or `danger`.
    pub tone: String,
    /// True when a restore is already waiting for the next start.
    pub restore_waiting: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BackupRowView {
    pub path: String,
    pub name: String,
    /// Already formatted.
    pub taken_at: String,
    pub size: String,
    /// What the last verify of THIS file found, in words.
    pub verified: Option<String>,
    pub verified_ok: bool,
}

/// What a verify found, for the toast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct VerifyView {
    pub ok: bool,
    pub message: String,
    pub detail: String,
}

/// Where backups go when the shop has not said.
pub(crate) fn folder_for(app: &App, config: &super::ShopConfig) -> std::path::PathBuf {
    if !config.backup.folder.trim().is_empty() {
        return std::path::PathBuf::from(config.backup.folder.trim());
    }
    app.with_shop(|shop| {
        Ok(shop
            .path
            .parent()
            .map_or_else(
                || std::path::PathBuf::from("."),
                std::path::Path::to_path_buf,
            )
            .join("backups"))
    })
    .unwrap_or_else(|_| mb_db::locate::default_config_dir().join("backups"))
}

/// A file size, in the words a person reads.
#[allow(
    clippy::integer_division,
    reason = "tenths of a megabyte: the remainder is the decimal place, and it \
              is kept rather than discarded"
)]
fn megabytes(bytes: u64) -> String {
    let tenths = bytes.saturating_mul(10) / (1024 * 1024);
    format!("{}.{} MB", tenths / 10, tenths % 10)
}

pub fn status_on(app: &App) -> UiResult<BackupView> {
    guard::require(app, Permission::BackupRun)?;
    let config = app.shop_config();
    let folder = folder_for(app, &config);

    let backups = mb_db::backup::list(&folder).unwrap_or_default();
    let database = app
        .with_shop(|shop| Ok(shop.path.display().to_string()))
        .unwrap_or_else(|_| "no shop is open".to_owned());
    let shop_folder = app
        .with_shop(|shop| {
            Ok(shop
                .path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default())
        })
        .unwrap_or_default();

    // What a verify found, remembered.
    let verified = verify_marks(app);

    let rows: Vec<BackupRowView> = backups
        .iter()
        .rev()
        .map(|backup| {
            let key = backup.path.display().to_string();
            let mark = verified.iter().find(|(p, _)| *p == key);
            BackupRowView {
                name: backup
                    .path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                taken_at: words::when(mb_core::Timestamp::from_millis(backup.manifest.taken_at_ms)),
                size: megabytes(backup.manifest.bytes),
                verified: mark.map(|(_, words)| words.clone()),
                verified_ok: mark.is_some_and(|(_, w)| w.starts_with("Checked")),
                path: key,
            }
        })
        .collect();

    let (headline, tone) = match rows.first() {
        None => (
            "This shop has never been backed up. Everything it has is on one \
             disk."
                .to_owned(),
            "danger".to_owned(),
        ),
        Some(latest) if latest.verified.is_none() => (
            format!(
                "Last backed up {}. NOBODY HAS CHECKED IT — an unchecked backup \
                 is not a backup until the day you need it.",
                latest.taken_at
            ),
            "warn".to_owned(),
        ),
        Some(latest) if !latest.verified_ok => (
            format!(
                "The backup from {} DID NOT PASS its check. Take another one \
                 now and keep this file for support.",
                latest.taken_at
            ),
            "danger".to_owned(),
        ),
        Some(latest) => {
            let second = if config.backup.second_folder.trim().is_empty() {
                " There is only one copy, on this machine — a second folder on \
                 a pen drive or a network share is what survives the disk."
            } else {
                ""
            };
            (
                format!(
                    "Last backed up {}, and it was checked.{second}",
                    latest.taken_at
                ),
                if second.is_empty() { "ok" } else { "warn" }.to_owned(),
            )
        }
    };

    Ok(BackupView {
        folder: folder.display().to_string(),
        second_folder: config.backup.second_folder.clone(),
        database,
        shop_folder,
        backups: rows,
        headline,
        tone,
        restore_waiting: mb_db::backup::pending_restore(&mb_db::locate::default_config_dir())
            .map(|p| p.from.display().to_string()),
    })
}

/// The verify marks, kept as one setting keyed by file name.
const VERIFIED_KEY: &str = "backup.verified";

fn verify_marks(app: &App) -> Vec<(String, String)> {
    let raw = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    mb_db::Repos::new(tx)
                        .settings()
                        .get::<String>(crate::state::OUTLET, VERIFIED_KEY)
                })
                .map_err(|e| words::from_db(&e))
        })
        .unwrap_or(None)
        .unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

fn remember_verify(app: &App, path: &str, words_for_screen: &str) {
    let mut marks = verify_marks(app);
    marks.retain(|(p, _)| p != path);
    marks.push((path.to_owned(), words_for_screen.to_owned()));
    let Ok(text) = serde_json::to_string(&marks) else {
        return;
    };
    let written = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).settings().set(
                    crate::state::OUTLET,
                    VERIFIED_KEY,
                    &text,
                    crate::flows::now(),
                    None,
                )
            })
            .map_err(|e| words::from_db(&e))
    });
    if let Err(e) = written {
        log_warn!("the backup check could not be remembered: {e}");
    }
}

pub fn back_up_now_on(app: &App) -> UiResult<BackupView> {
    let who = guard::require(app, Permission::BackupRun)?;
    take_backup(app, &app.shop_config(), &who.name)?;
    status_on(app)
}

/// The schedule: a backup every `every_hours`, taken quietly, kept to `keep_count`.
pub fn watch(handle: &tauri::AppHandle) {
    use tauri::Manager as _;

    let handle = handle.clone();
    std::thread::Builder::new()
        .name("mb-backup".to_owned())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                let Some(app) = handle.try_state::<App>() else {
                    return;
                };
                if let Err(e) = take_if_due(&app) {
                    log_warn!("the scheduled backup failed: {}", e.message);
                }
            }
        })
        .ok();
}

/// A backup, if the newest one is older than the shop asked for. True when one was taken.
pub fn take_if_due(app: &App) -> UiResult<bool> {
    let config = app.shop_config();
    let hours = i64::from(config.backup.every_hours);
    if hours == 0 || !app.has_shop() {
        return Ok(false);
    }
    let folder = folder_for(app, &config);
    let newest = mb_db::backup::list(&folder)
        .unwrap_or_default()
        .iter()
        .map(|b| b.manifest.taken_at_ms)
        .max()
        .unwrap_or(0);
    let age = crate::flows::now().millis().saturating_sub(newest);
    if age < hours.saturating_mul(3_600_000) {
        return Ok(false);
    }
    take_backup(app, &config, "the schedule")?;
    Ok(true)
}

/// One backup, by a person or by the clock: taken, copied to the second folder, pruned.
fn take_backup(
    app: &App,
    config: &super::ShopConfig,
    who: &str,
) -> UiResult<mb_db::backup::Backup> {
    let folder = folder_for(app, config);

    let at = crate::flows::now();
    // **id-lint-ok: this is a FILE NAME, and the time in it is the point.**
    //
    // A shop looking in the backup folder reads these; a name that was only a
    // random tail would tell them nothing about which copy is which. The tail
    // is on the end because two backups in the same millisecond would otherwise
    // be one file name, and the second would land on top of the first — a shop
    // that pressed the button twice would have one copy where it thought it had
    // two.
    let name = format!("magicbill-{}-{}.db", at.millis(), crate::newid::tail_only());
    let target = folder.join(name);

    let backup = app.with_shop(|shop| {
        mb_db::backup::take(&shop.db, &target, env!("CARGO_PKG_VERSION"))
            .map_err(|e| words::from_db(&e))
    })?;

    // The second location, and it is the one that survives the disk.
    let second = config.backup.second_folder.trim().to_owned();
    if !second.is_empty() {
        match mb_db::backup::copy_to_second_location(&backup, std::path::Path::new(&second)) {
            Ok(path) => log_info!("a second copy went to {}", path.display()),
            Err(e) => log_warn!("the second copy could not be written to {second}: {e}"),
        }
    }

    // Keep only as many as the shop asked for.
    let keep = usize::try_from(config.backup.keep_count).unwrap_or(30);
    match mb_db::backup::prune(&folder, keep) {
        Ok(gone) if !gone.is_empty() => log_info!("{} old backup(s) removed", gone.len()),
        Ok(_) => {}
        Err(e) => log_warn!("old backups could not be tidied: {e}"),
    }

    log_info!("{who} took a backup to {}", backup.path.display());
    Ok(backup)
}

pub fn verify_on(app: &App, path: String) -> UiResult<VerifyView> {
    guard::require(app, Permission::BackupRun)?;
    let report =
        mb_db::backup::verify(std::path::Path::new(&path)).map_err(|e| words::from_db(&e))?;

    let (ok, message) = if report.is_ok() && report.count_mismatches.is_empty() {
        (true, "Checked, and this backup is sound.".to_owned())
    } else {
        let mut wrong = Vec::new();
        if !report.integrity_ok {
            wrong.push("the file itself is damaged");
        }
        if !report.foreign_keys_ok {
            wrong.push("rows point at things that are not there");
        }
        if !report.checksum_ok {
            wrong.push("it does not match what was written");
        }
        if !report.count_mismatches.is_empty() {
            wrong.push("some tables have the wrong number of rows");
        }
        (
            false,
            format!("THIS BACKUP DID NOT PASS: {}.", wrong.join(", ")),
        )
    };

    remember_verify(app, &path, if ok { "Checked" } else { "Did not pass" });

    Ok(VerifyView {
        ok,
        message,
        detail: format!(
            "integrity {} · foreign keys {} · checksum {} · {} table(s) counted wrong",
            yes_no(report.integrity_ok),
            yes_no(report.foreign_keys_ok),
            yes_no(report.checksum_ok),
            report.count_mismatches.len()
        ),
    })
}

const fn yes_no(ok: bool) -> &'static str {
    if ok { "ok" } else { "NOT ok" }
}

/// This does not restore.
pub fn request_restore_on(app: &App, path: String) -> UiResult<BackupView> {
    let who = guard::require(app, Permission::BackupRun)?;

    if !std::path::Path::new(&path).exists() {
        return Err(UiError::new(
            "backup.missing",
            "That backup file is not there any more. Choose another one.",
        ));
    }
    // Refuse to restore something that has not been checked.
    let checked = verify_marks(app)
        .into_iter()
        .any(|(p, w)| p == path && w.starts_with("Checked"));
    if !checked {
        return Err(UiError::new(
            "backup.unchecked",
            "Check this backup first. Restoring a file nobody has checked is \
             how a bad backup replaces a good shop.",
        ));
    }

    mb_db::backup::request_restore(
        &mb_db::locate::default_config_dir(),
        std::path::Path::new(&path),
    )
    .map_err(|e| words::from_db(&e))?;

    log_info!("{} asked to restore {path} on the next start", who.name);
    status_on(app)
}

pub fn cancel_restore_on(app: &App) -> UiResult<BackupView> {
    guard::require(app, Permission::BackupRun)?;
    mb_db::backup::clear_pending_restore(&mb_db::locate::default_config_dir())
        .map_err(|e| words::from_db(&e))?;
    status_on(app)
}

pub fn find_shops_on(app: &App) -> UiResult<Vec<String>> {
    guard::require(app, Permission::BackupRun)?;
    Ok(mb_db::locate::search_usual_places(&[])
        .into_iter()
        .map(|found| {
            format!(
                "{} — {}",
                found.path.display(),
                if found.orders > 0 {
                    format!("{} bills, {} items", found.orders, found.items)
                } else {
                    format!("{} items, no bills", found.items)
                }
            )
        })
        .collect())
}

// The seats.

#[tauri::command]
pub fn backup_status(app: tauri::State<'_, App>) -> UiResult<BackupView> {
    status_on(&app)
}

#[tauri::command]
pub fn back_up_now(app: tauri::State<'_, App>) -> UiResult<BackupView> {
    back_up_now_on(&app)
}

#[tauri::command]
pub fn verify_backup(app: tauri::State<'_, App>, path: String) -> UiResult<VerifyView> {
    verify_on(&app, path)
}

#[tauri::command]
pub fn request_restore(app: tauri::State<'_, App>, path: String) -> UiResult<BackupView> {
    request_restore_on(&app, path)
}

#[tauri::command]
pub fn cancel_restore(app: tauri::State<'_, App>) -> UiResult<BackupView> {
    cancel_restore_on(&app)
}

#[tauri::command]
pub fn find_shops(app: tauri::State<'_, App>) -> UiResult<Vec<String>> {
    find_shops_on(&app)
}
