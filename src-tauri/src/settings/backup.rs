//! Backup and restore. A backup is taken by the schedule the owner set, or on the button,
//! checked the moment it is written, kept in the backup folder (the shop folder's `backups`
//! unless the owner moved it) and copied to every folder on the copies list: a pen drive,
//! Google Drive, OneDrive, a share. The newest `KEEP` are kept everywhere.

use std::path::{Path, PathBuf};

use mb_auth::Permission;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::guard;
use crate::state::App;
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

/// How many backups are kept, in the backup folder and in every copy folder.
pub const KEEP: usize = 30;
/// The folder inside the shop folder.
const FOLDER: &str = "backups";
/// The folder made inside a pen drive or a cloud folder.
const COPY_FOLDER: &str = "Magic Bill backups";
/// A press this soon after the last backup is the same backup.
const JUST_NOW_MS: i64 = 60_000;
const HOUR_MS: i64 = 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BackupView {
    /// The folder that holds the whole shop: its data file, its licence.
    pub shop_folder: String,
    /// Where the backups are kept.
    pub folder: String,
    /// True while that is the shop folder's own `backups`.
    pub folder_is_default: bool,
    /// Every folder that gets a copy.
    pub copies: Vec<CopyView>,
    /// Places found on this computer that are not on the list yet.
    pub suggested: Vec<CopyView>,
    /// `off`, `hourly`, `daily` or `day_close`.
    pub schedule: String,
    /// "23:00", the daily time as the clock box holds it.
    pub daily_at: String,
    /// The schedule in one line: "Every day at 11:00 pm".
    pub schedule_says: String,
    pub backups: Vec<BackupRowView>,
    /// The last backup in one line: "9 Sep, 3:04 pm, checked".
    pub last: String,
    /// `ok`, `warn` or `danger`.
    pub tone: String,
    /// The backup a restore is waiting to put in place on the next start, if one is.
    pub restore_waiting: Option<String>,
}

/// One folder that gets a copy, or could.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CopyView {
    pub path: String,
    /// "Google Drive", "OneDrive", "Dropbox", "Pen drive", "Network share" or "Folder".
    pub kind: String,
    /// The pen drive's label, or empty.
    pub name: String,
    /// False while the drive is out or the share is down.
    pub reachable: bool,
    /// When the newest copy there was taken, or empty.
    pub last: String,
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

/// The folder the shop lives in.
fn shop_folder(app: &App) -> PathBuf {
    app.with_shop(|shop| {
        Ok(shop
            .path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf))
    })
    .unwrap_or_else(|_| mb_db::locate::default_config_dir())
}

/// Where this shop's backups go: the owner's folder, or `backups` in the shop folder.
pub(crate) fn folder_for(app: &App) -> PathBuf {
    let chosen = app.shop_config().backup.folder.trim().to_owned();
    if chosen.is_empty() {
        shop_folder(app).join(FOLDER)
    } else {
        PathBuf::from(chosen)
    }
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

// The places a copy can go.

/// A place this computer has that a copy could go to.
struct Place {
    kind: &'static str,
    name: String,
    root: PathBuf,
}

/// Google Drive for desktop, OneDrive, Dropbox and every pen drive that is in right now.
fn places_on_this_computer() -> Vec<Place> {
    let mut out = Vec::new();
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);

    for key in ["OneDriveConsumer", "OneDrive", "OneDriveCommercial"] {
        if let Some(dir) = std::env::var_os(key).map(PathBuf::from)
            && dir.is_dir()
            && !out.iter().any(|p: &Place| p.root == dir)
        {
            out.push(Place {
                kind: "OneDrive",
                name: String::new(),
                root: dir,
            });
        }
    }

    for drive in mb_winprint::drives() {
        let root = PathBuf::from(&drive.root);
        match drive.kind {
            mb_winprint::DriveKind::Removable => out.push(Place {
                kind: "Pen drive",
                name: drive.label,
                root,
            }),
            mb_winprint::DriveKind::Fixed | mb_winprint::DriveKind::Network => {
                // Google Drive for desktop mounts as a drive letter with "My Drive" on it.
                let mine = root.join("My Drive");
                if mine.is_dir() {
                    out.push(Place {
                        kind: "Google Drive",
                        name: String::new(),
                        root: mine,
                    });
                }
            }
        }
    }

    if let Some(home) = home {
        for (kind, sub) in [
            ("Google Drive", "Google Drive"),
            ("Google Drive", "My Drive"),
            ("Dropbox", "Dropbox"),
        ] {
            let dir = home.join(sub);
            if dir.is_dir() && !out.iter().any(|p| p.root == dir) {
                out.push(Place {
                    kind,
                    name: String::new(),
                    root: dir,
                });
            }
        }
    }
    out
}

/// What a copy folder is, read off its path.
fn kind_of(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("\\\\") {
        return "Network share";
    }
    if lower.contains("my drive") || lower.contains("google drive") {
        return "Google Drive";
    }
    if lower.contains("onedrive") {
        return "OneDrive";
    }
    if lower.contains("dropbox") {
        return "Dropbox";
    }
    if mb_winprint::drives().iter().any(|d| {
        d.kind == mb_winprint::DriveKind::Removable
            && lower.starts_with(&d.root.to_ascii_lowercase())
    }) {
        return "Pen drive";
    }
    "Folder"
}

/// One copy folder as the screen shows it: reachable right now, and the newest copy in it.
fn copy_view(path: &str, kind: &'static str, name: &str) -> CopyView {
    let dir = Path::new(path);
    let reachable = dir.is_dir();
    let last = if reachable {
        mb_db::backup::list(dir)
            .unwrap_or_default()
            .iter()
            .map(|b| b.manifest.taken_at_ms)
            .max()
            .map(|ms| words::when(mb_core::Timestamp::from_millis(ms)))
            .unwrap_or_default()
    } else {
        String::new()
    };
    CopyView {
        path: path.to_owned(),
        kind: kind.to_owned(),
        name: name.to_owned(),
        reachable,
        last,
    }
}

/// The places not on the list yet, each offered as its own "Magic Bill backups" folder.
fn suggestions(copies: &[String]) -> Vec<CopyView> {
    places_on_this_computer()
        .into_iter()
        .filter(|place| {
            let root = place.root.to_string_lossy().to_ascii_lowercase();
            !copies
                .iter()
                .any(|have| have.to_ascii_lowercase().starts_with(&root))
        })
        .map(|place| {
            let path = place.root.join(COPY_FOLDER).display().to_string();
            CopyView {
                reachable: true,
                last: String::new(),
                ..copy_view(&path, place.kind, &place.name)
            }
        })
        .collect()
}

// The schedule.

/// The schedule in one line.
fn schedule_says(policy: &super::BackupPolicy) -> String {
    match policy.schedule.as_str() {
        "off" => "Only when you press Back up now".to_owned(),
        "hourly" => "Every hour".to_owned(),
        "day_close" => "When the day is closed".to_owned(),
        _ => format!("Every day at {}", words::clock(policy.daily_at_minutes)),
    }
}

/// The instant the daily backup was last due: today's time if it has passed, else yesterday's.
fn last_daily_due(policy: &super::BackupPolicy, now: mb_core::Timestamp) -> i64 {
    let offset = mb_core::UtcOffset::INDIA;
    let (days, seconds) = now.to_local_parts(offset);
    let at = policy.daily_at_minutes.saturating_mul(60);
    let days = if seconds >= at {
        days
    } else {
        days.saturating_sub(1)
    };
    mb_core::Timestamp::from_local_parts(days, at, offset).map_or(0, |t| t.millis())
}

/// Whether the schedule wants a backup now, given when the newest one was taken.
#[must_use]
pub fn is_due(policy: &super::BackupPolicy, newest_ms: i64, now: mb_core::Timestamp) -> bool {
    match policy.schedule.as_str() {
        "off" | "day_close" => false,
        "hourly" => now.millis().saturating_sub(newest_ms) >= HOUR_MS,
        _ => newest_ms < last_daily_due(policy, now),
    }
}

/// When the newest backup was taken, or nought.
fn newest_ms(folder: &Path) -> i64 {
    mb_db::backup::list(folder)
        .unwrap_or_default()
        .iter()
        .map(|b| b.manifest.taken_at_ms)
        .max()
        .unwrap_or(0)
}

pub fn status_on(app: &App) -> UiResult<BackupView> {
    guard::require(app, Permission::BackupRun)?;
    adopt_old_second_folder(app);
    let config = app.shop_config();
    let folder = folder_for(app);

    let backups = mb_db::backup::list(&folder).unwrap_or_default();
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

    let copies = config.backup.copy_folders();
    Ok(BackupView {
        shop_folder: shop_folder(app).display().to_string(),
        folder: folder.display().to_string(),
        folder_is_default: config.backup.folder.trim().is_empty(),
        copies: copies
            .iter()
            .map(|path| copy_view(path, kind_of(path), ""))
            .collect(),
        suggested: suggestions(&copies),
        schedule: config.backup.schedule.clone(),
        daily_at: super::ipc::clock_wire(config.backup.daily_at_minutes),
        schedule_says: schedule_says(&config.backup),
        backups: rows,
        last,
        tone: tone.to_owned(),
        restore_waiting: mb_db::backup::pending_restore(&mb_db::locate::default_config_dir())
            .map(|p| p.from.display().to_string()),
    })
}

/// The setting an older build kept its one second folder in.
const OLD_SECOND_FOLDER: &str = "backup.second_folder";

/// A second folder set by an older build joins the copies list, once.
fn adopt_old_second_folder(app: &App) {
    let old = raw_setting(app, OLD_SECOND_FOLDER).trim().to_owned();
    if old.is_empty() {
        return;
    }
    let mut copies = app.shop_config().backup.copy_folders();
    if !copies.iter().any(|c| c.eq_ignore_ascii_case(&old)) {
        copies.push(old.clone());
        if let Err(e) = save_copies(app, &copies) {
            log_warn!(
                "the old second folder could not join the copies list: {}",
                e.message
            );
            return;
        }
    }
    write_raw_setting(app, OLD_SECOND_FOLDER, "");
    log_info!("the second backup folder {old} is now on the copies list");
}

/// A setting read straight from the shop, outside the catalog.
fn raw_setting(app: &App, key: &str) -> String {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .settings()
                    .get::<String>(crate::state::OUTLET, key)
            })
            .map_err(|e| words::from_db(&e))
    })
    .unwrap_or(None)
    .unwrap_or_default()
}

fn write_raw_setting(app: &App, key: &str, text: &str) {
    let written = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).settings().set(
                    crate::state::OUTLET,
                    key,
                    &text.to_owned(),
                    crate::flows::now(),
                    None,
                )
            })
            .map_err(|e| words::from_db(&e))
    });
    if let Err(e) = written {
        log_warn!("the setting {key} could not be written: {e}");
    }
}

/// The check marks, kept as one setting keyed by file name.
const VERIFIED_KEY: &str = "backup.verified";

fn verify_marks(app: &App) -> Vec<(String, String)> {
    serde_json::from_str(&raw_setting(app, VERIFIED_KEY)).unwrap_or_default()
}

fn remember_verify(app: &App, path: &str, mark: &str) {
    let mut marks = verify_marks(app);
    marks.retain(|(p, _)| p != path);
    marks.push((path.to_owned(), mark.to_owned()));
    let Ok(text) = serde_json::to_string(&marks) else {
        return;
    };
    write_raw_setting(app, VERIFIED_KEY, &text);
}

/// The button. A press inside a minute of the last backup is that backup: the list does not
/// grow a row for every click.
pub fn back_up_now_on(app: &App) -> UiResult<BackupView> {
    let who = guard::require(app, Permission::BackupRun)?;
    let age = crate::flows::now()
        .millis()
        .saturating_sub(newest_ms(&folder_for(app)));
    if age < JUST_NOW_MS {
        log_info!(
            "{} pressed Back up now inside a minute of the last one",
            who.name
        );
        return status_on(app);
    }
    let (_, report) = take_backup(app, &app.shop_config(), &who.name)?;
    if !report.ok {
        return Err(UiError::new("backup.failed", report.message).with_detail(report.detail));
    }
    status_on(app)
}

/// The clock: looks once a minute at whether the owner's schedule wants a backup.
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

/// A backup, if the schedule wants one. True when one was taken.
pub fn take_if_due(app: &App) -> UiResult<bool> {
    if !app.has_shop() {
        return Ok(false);
    }
    let config = app.shop_config();
    if !is_due(
        &config.backup,
        newest_ms(&folder_for(app)),
        crate::flows::now(),
    ) {
        return Ok(false);
    }
    take_backup(app, &config, "the schedule")?;
    Ok(true)
}

/// The day was closed: the schedule that waits for that takes its backup now.
pub fn after_day_close(app: &App) {
    let config = app.shop_config();
    if config.backup.schedule != "day_close" {
        return;
    }
    if let Err(e) = take_backup(app, &config, "the day close") {
        log_warn!("the day-close backup failed: {}", e.message);
    }
}

/// One backup, by a person or by the clock: taken, checked, copied to every copy folder, and
/// the old ones pruned everywhere. The check's outcome comes back with it.
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
    // be one file name, and the second would land on top of the first.
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

    for copy in config.backup.copy_folders() {
        copy_backup_to(&backup, Path::new(&copy));
    }
    prune(&folder);

    log_info!("{who} took a backup to {}", backup.path.display());
    Ok((backup, report))
}

/// One copy into one folder, and that folder tidied. A drive that is out is a log line, never
/// a failure: the backup itself is safe.
fn copy_backup_to(backup: &mb_db::backup::Backup, dir: &Path) {
    match mb_db::backup::copy_to_second_location(backup, dir) {
        Ok(path) => {
            log_info!("a copy went to {}", path.display());
            prune(dir);
        }
        Err(e) => log_warn!("the copy could not be written to {}: {e}", dir.display()),
    }
}

/// Keep the newest `KEEP` in a folder.
fn prune(dir: &Path) {
    match mb_db::backup::prune(dir, KEEP) {
        Ok(gone) if !gone.is_empty() => {
            log_info!(
                "{} old backup(s) removed from {}",
                gone.len(),
                dir.display()
            );
        }
        Ok(_) => {}
        Err(e) => log_warn!("old backups in {} could not be tidied: {e}", dir.display()),
    }
}

/// Check one backup file and remember what was found.
fn check(app: &App, path: &str) -> UiResult<VerifyView> {
    let report = mb_db::backup::verify(Path::new(path)).map_err(|e| words::from_db(&e))?;

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

    if !Path::new(&path).exists() {
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

    mb_db::backup::request_restore(&mb_db::locate::default_config_dir(), Path::new(&path))
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

// The owner's choices.

/// A folder that exists, or can be made, and takes a file.
fn usable_folder(folder: &str) -> UiResult<String> {
    let folder = folder.trim().to_owned();
    if folder.is_empty() {
        return Err(UiError::new("backup.folder", "Choose a folder."));
    }
    let path = Path::new(&folder);
    if let Err(e) = std::fs::create_dir_all(path) {
        return Err(UiError::new(
            "backup.folder",
            format!("The folder {folder} cannot be made: {e}."),
        ));
    }
    let probe = path.join("magicbill-write-test.tmp");
    if let Err(e) = std::fs::write(&probe, b"ok") {
        return Err(UiError::new(
            "backup.folder",
            format!("Nothing can be written to {folder}: {e}."),
        ));
    }
    let _ = std::fs::remove_file(&probe);
    Ok(folder)
}

fn save_setting(app: &App, key: &str, value: String) -> UiResult<()> {
    super::ipc::save_on(
        app,
        vec![super::ipc::SettingEdit {
            key: key.to_owned(),
            value,
        }],
    )
    .map(|_| ())
}

fn save_copies(app: &App, copies: &[String]) -> UiResult<()> {
    save_setting(
        app,
        "backup.copies",
        super::BackupPolicy::join_copies(copies),
    )
}

/// Where the backups are kept from now on. `None` is the shop folder's own `backups`. The
/// backups already taken are carried over so the list does not start empty.
pub fn set_folder_on(app: &App, folder: Option<String>) -> UiResult<BackupView> {
    guard::require(app, Permission::BackupRun)?;
    let was = folder_for(app);
    let folder = match folder {
        Some(f) if !f.trim().is_empty() => usable_folder(&f)?,
        _ => String::new(),
    };
    save_setting(app, "backup.folder", folder)?;
    let now = folder_for(app);
    if now != was {
        for backup in mb_db::backup::list(&was).unwrap_or_default() {
            if let Err(e) = mb_db::backup::copy_to_second_location(&backup, &now) {
                log_warn!("{} could not be carried over: {e}", backup.path.display());
            }
        }
        log_info!("backups now go to {}", now.display());
    }
    status_on(app)
}

/// One more folder that gets every backup. The newest backup goes there at once.
pub fn add_copy_on(app: &App, folder: String) -> UiResult<BackupView> {
    guard::require(app, Permission::BackupRun)?;
    let folder = usable_folder(&folder)?;
    if Path::new(&folder) == folder_for(app) {
        return Err(UiError::new(
            "backup.copy_is_folder",
            "That is where the backups already are.",
        ));
    }
    let mut copies = app.shop_config().backup.copy_folders();
    if copies.iter().any(|c| c.eq_ignore_ascii_case(&folder)) {
        return status_on(app);
    }
    copies.push(folder.clone());
    save_copies(app, &copies)?;

    if let Some(newest) = mb_db::backup::list(&folder_for(app))
        .unwrap_or_default()
        .into_iter()
        .max_by_key(|b| b.manifest.taken_at_ms)
    {
        copy_backup_to(&newest, Path::new(&folder));
    }
    log_info!("backups are copied to {folder} from now on");
    status_on(app)
}

pub fn remove_copy_on(app: &App, folder: String) -> UiResult<BackupView> {
    guard::require(app, Permission::BackupRun)?;
    let copies: Vec<String> = app
        .shop_config()
        .backup
        .copy_folders()
        .into_iter()
        .filter(|c| !c.eq_ignore_ascii_case(folder.trim()))
        .collect();
    save_copies(app, &copies)?;
    log_info!("backups are no longer copied to {folder}");
    status_on(app)
}

/// When backups are taken by themselves, and for the daily one, at what time ("23:00").
pub fn set_schedule_on(app: &App, schedule: String, daily_at: String) -> UiResult<BackupView> {
    guard::require(app, Permission::BackupRun)?;
    if !super::catalog::BACKUP_SCHEDULES
        .iter()
        .any(|c| c.value == schedule)
    {
        return Err(UiError::new("backup.schedule", "That is not a schedule."));
    }
    super::ipc::save_on(
        app,
        vec![
            super::ipc::SettingEdit {
                key: "backup.schedule".to_owned(),
                value: schedule,
            },
            super::ipc::SettingEdit {
                key: "backup.daily_at_minutes".to_owned(),
                value: daily_at,
            },
        ],
    )?;
    status_on(app)
}

/// A fresh backup, saved into a folder the owner picked just now: an export.
pub fn save_to_on(app: &App, folder: String) -> UiResult<BackupView> {
    let who = guard::require(app, Permission::BackupRun)?;
    let folder = usable_folder(&folder)?;
    let (backup, report) = take_backup(app, &app.shop_config(), &who.name)?;
    if !report.ok {
        return Err(UiError::new("backup.failed", report.message).with_detail(report.detail));
    }
    let path = mb_db::backup::copy_to_second_location(&backup, Path::new(&folder))
        .map_err(|e| words::from_db(&e))?;
    log_info!("{} saved a backup to {}", who.name, path.display());
    status_on(app)
}

// The seats. The ones that touch a drive or a share are `async`: a pen drive that is out or a
// share that is down answers slowly, and Tauri runs an async command off the thread that
// paints.

#[tauri::command]
pub async fn backup_status(app: tauri::State<'_, App>) -> UiResult<BackupView> {
    status_on(&app)
}

#[tauri::command]
pub async fn back_up_now(app: tauri::State<'_, App>) -> UiResult<BackupView> {
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
pub async fn set_backup_folder(
    app: tauri::State<'_, App>,
    folder: Option<String>,
) -> UiResult<BackupView> {
    set_folder_on(&app, folder)
}

#[tauri::command]
pub async fn add_backup_copy(app: tauri::State<'_, App>, folder: String) -> UiResult<BackupView> {
    add_copy_on(&app, folder)
}

#[tauri::command]
pub async fn remove_backup_copy(
    app: tauri::State<'_, App>,
    folder: String,
) -> UiResult<BackupView> {
    remove_copy_on(&app, folder)
}

#[tauri::command]
pub fn set_backup_schedule(
    app: tauri::State<'_, App>,
    schedule: String,
    daily_at: String,
) -> UiResult<BackupView> {
    set_schedule_on(&app, schedule, daily_at)
}

#[tauri::command]
pub async fn save_backup_to(app: tauri::State<'_, App>, folder: String) -> UiResult<BackupView> {
    save_to_on(&app, folder)
}
