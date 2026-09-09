//! Backup and restore. Backups live in a `backups` folder inside the shop folder, are taken
//! once a day by the clock or whenever somebody presses the button, are checked the moment
//! they are written, and the newest thirty are kept. A second folder gets a copy of each.

use mb_auth::Permission;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::guard;
use crate::state::App;
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

/// How old the newest backup may be before the clock takes another.
pub const EVERY_HOURS: i64 = 24;
/// How many backups are kept.
pub const KEEP: usize = 30;
/// The folder inside the shop folder.
const FOLDER: &str = "backups";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BackupView {
    /// The folder that holds the whole shop: its data file, its licence, its backups.
    pub shop_folder: String,
    /// Where the backups are, inside the shop folder.
    pub folder: String,
    /// The pen drive or network share every backup is copied to. Empty when there is none.
    pub second_folder: String,
    pub backups: Vec<BackupRowView>,
    /// The last backup in one line: "9 Sep, 3:04 pm, checked".
    pub last: String,
    /// `ok`, `warn` or `danger`.
    pub tone: String,
    /// The backup a restore is waiting to put in place on the next start, if one is.
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
    /// "Checked", "Failed", or "Not checked" for a backup an older build took.
    pub checked: String,
    /// True when the check passed, so a restore may use it.
    pub checked_ok: bool,
}

/// What a check found, for the toast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct VerifyView {
    pub ok: bool,
    pub message: String,
    pub detail: String,
}

/// Where this shop's backups go.
pub(crate) fn folder_for(app: &App) -> std::path::PathBuf {
    app.with_shop(|shop| {
        Ok(shop
            .path
            .parent()
            .map_or_else(
                || std::path::PathBuf::from("."),
                std::path::Path::to_path_buf,
            )
            .join(FOLDER))
    })
    .unwrap_or_else(|_| mb_db::locate::default_config_dir().join(FOLDER))
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

/// The mark a passed check leaves.
const CHECKED: &str = "Checked";
/// The mark a failed check leaves.
const FAILED: &str = "Failed";

pub fn status_on(app: &App) -> UiResult<BackupView> {
    guard::require(app, Permission::BackupRun)?;
    let config = app.shop_config();
    let folder = folder_for(app);

    let backups = mb_db::backup::list(&folder).unwrap_or_default();
    let shop_folder = app
        .with_shop(|shop| {
            Ok(shop
                .path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default())
        })
        .unwrap_or_default();

    let marks = verify_marks(app);
    let rows: Vec<BackupRowView> = backups
        .iter()
        .rev()
        .map(|backup| {
            let key = backup.path.display().to_string();
            let mark = marks
                .iter()
                .find(|(p, _)| *p == key)
                .map(|(_, m)| m.as_str());
            BackupRowView {
                name: backup
                    .path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                taken_at: words::when(mb_core::Timestamp::from_millis(backup.manifest.taken_at_ms)),
                size: megabytes(backup.manifest.bytes),
                checked: mark.unwrap_or("Not checked").to_owned(),
                checked_ok: mark == Some(CHECKED),
                path: key,
            }
        })
        .collect();

    let (last, tone) = match rows.first() {
        None => ("No backup yet".to_owned(), "danger"),
        Some(latest) if latest.checked_ok => (format!("{}, checked", latest.taken_at), "ok"),
        Some(latest) if latest.checked == FAILED => {
            (format!("{}, failed its check", latest.taken_at), "danger")
        }
        Some(latest) => (format!("{}, not checked", latest.taken_at), "warn"),
    };

    Ok(BackupView {
        shop_folder,
        folder: folder.display().to_string(),
        second_folder: config.backup.second_folder.trim().to_owned(),
        backups: rows,
        last,
        tone: tone.to_owned(),
        restore_waiting: mb_db::backup::pending_restore(&mb_db::locate::default_config_dir())
            .map(|p| p.from.display().to_string()),
    })
}

/// The check marks, kept as one setting keyed by file name.
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

fn remember_verify(app: &App, path: &str, mark: &str) {
    let mut marks = verify_marks(app);
    marks.retain(|(p, _)| p != path);
    marks.push((path.to_owned(), mark.to_owned()));
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
    let (_, report) = take_backup(app, &app.shop_config(), &who.name)?;
    if !report.ok {
        return Err(UiError::new("backup.failed", report.message).with_detail(report.detail));
    }
    status_on(app)
}

/// The schedule: a backup every `EVERY_HOURS`, taken quietly, kept to `KEEP`.
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

/// A backup, if the newest one is a day old. True when one was taken.
pub fn take_if_due(app: &App) -> UiResult<bool> {
    if !app.has_shop() {
        return Ok(false);
    }
    let folder = folder_for(app);
    let newest = mb_db::backup::list(&folder)
        .unwrap_or_default()
        .iter()
        .map(|b| b.manifest.taken_at_ms)
        .max()
        .unwrap_or(0);
    let age = crate::flows::now().millis().saturating_sub(newest);
    if age < EVERY_HOURS.saturating_mul(3_600_000) {
        return Ok(false);
    }
    take_backup(app, &app.shop_config(), "the schedule")?;
    Ok(true)
}

/// One backup, by a person or by the clock: taken, checked, copied to the second folder,
/// and the old ones pruned. The check's outcome comes back with it.
fn take_backup(
    app: &App,
    config: &super::ShopConfig,
    who: &str,
) -> UiResult<(mb_db::backup::Backup, VerifyView)> {
    let folder = folder_for(app);

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

    // Checked before anything is copied: a copy of a bad file is two bad files.
    let report = check(app, &backup.path.display().to_string())?;
    if !report.ok {
        log_warn!(
            "the backup {} failed its check: {}",
            backup.path.display(),
            report.detail
        );
        return Ok((backup, report));
    }

    // The second location, and it is the one that survives the disk.
    let second = config.backup.second_folder.trim().to_owned();
    if !second.is_empty() {
        match mb_db::backup::copy_to_second_location(&backup, std::path::Path::new(&second)) {
            Ok(path) => log_info!("a second copy went to {}", path.display()),
            Err(e) => log_warn!("the second copy could not be written to {second}: {e}"),
        }
    }

    match mb_db::backup::prune(&folder, KEEP) {
        Ok(gone) if !gone.is_empty() => log_info!("{} old backup(s) removed", gone.len()),
        Ok(_) => {}
        Err(e) => log_warn!("old backups could not be tidied: {e}"),
    }

    log_info!("{who} took a backup to {}", backup.path.display());
    Ok((backup, report))
}

/// Check one backup file and remember what was found.
fn check(app: &App, path: &str) -> UiResult<VerifyView> {
    let report =
        mb_db::backup::verify(std::path::Path::new(path)).map_err(|e| words::from_db(&e))?;

    let (ok, message) = if report.is_ok() && report.count_mismatches.is_empty() {
        (true, "This backup is good.".to_owned())
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
            format!("This backup failed its check: {}.", wrong.join(", ")),
        )
    };

    remember_verify(app, path, if ok { CHECKED } else { FAILED });

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

pub fn verify_on(app: &App, path: String) -> UiResult<VerifyView> {
    guard::require(app, Permission::BackupRun)?;
    check(app, &path)
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
    // Only a backup that passed its check goes in.
    let checked = verify_marks(app)
        .into_iter()
        .any(|(p, mark)| p == path && mark == CHECKED);
    if !checked {
        return Err(UiError::new(
            "backup.unchecked",
            "Check this backup first. Only a backup that passed its check can be restored.",
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

/// Set, or clear, the folder every backup is copied to. It must exist and take a file.
pub fn set_second_folder_on(app: &App, folder: Option<String>) -> UiResult<BackupView> {
    guard::require(app, Permission::BackupRun)?;
    let folder = folder.map(|f| f.trim().to_owned()).unwrap_or_default();
    if !folder.is_empty() {
        let path = std::path::Path::new(&folder);
        if !path.is_dir() {
            return Err(UiError::new(
                "backup.second_folder",
                format!("There is no folder at {folder}."),
            ));
        }
        let probe = path.join("magicbill-write-test.tmp");
        if let Err(e) = std::fs::write(&probe, b"ok") {
            return Err(UiError::new(
                "backup.second_folder",
                format!("Nothing can be written to {folder}: {e}."),
            ));
        }
        let _ = std::fs::remove_file(&probe);
    }
    super::ipc::save_on(
        app,
        vec![super::ipc::SettingEdit {
            key: "backup.second_folder".to_owned(),
            value: folder,
        }],
    )?;
    status_on(app)
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
pub fn set_second_backup_folder(
    app: tauri::State<'_, App>,
    folder: Option<String>,
) -> UiResult<BackupView> {
    set_second_folder_on(&app, folder)
}
