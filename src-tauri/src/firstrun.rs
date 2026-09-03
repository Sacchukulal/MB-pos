//! The first five minutes.

use serde::Serialize;
use ts_rs::TS;

use crate::state::App;
use crate::words::{UiError, UiResult};

/// Where a brand-new shop goes: beside the configuration, which is where start-up already looks
/// first.
fn default_shop_path() -> std::path::PathBuf {
    crate::config::AppConfig::directory().join("magicbill.db")
}

/// Where the first run has got to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct FirstRunView {
    /// True while the counter must not be shown.
    pub needed: bool,
    /// True once a database exists and is open.
    pub has_shop: bool,
    /// True once the shop has a name on it.
    pub has_details: bool,
    /// True once somebody has a PIN.
    pub has_pin: bool,
    /// True once the menu has an item — the wizard does not ask for what is already there.
    pub has_items: bool,
    /// True once the room has a table.
    pub has_tables: bool,
    /// Where the shop's data file is, once there is one.
    pub shop_path: String,
    /// Other databases already on this computer.
    pub found: Vec<String>,
    /// The default folder a new shop would go in, shown so nobody has to guess.
    pub default_folder: String,
}

/// What the first run knows, without needing a shop to be open.
pub fn look_on(app: &App) -> UiResult<FirstRunView> {
    let default_folder = default_shop_path().display().to_string();

    let Some(shop_path) = app
        .with_shop(|shop| Ok(shop.path.display().to_string()))
        .ok()
    else {
        return Ok(FirstRunView {
            needed: true,
            has_shop: false,
            has_details: false,
            has_pin: false,
            has_items: false,
            has_tables: false,
            shop_path: String::new(),
            // Every database already on this computer, described.
            found: mb_db::locate::search_usual_places(&[])
                .into_iter()
                .map(|found| {
                    format!(
                        "{} — {}",
                        found.path.display(),
                        if found.orders > 0 {
                            format!("{} bills, {} items", found.orders, found.items)
                        } else {
                            "empty".to_owned()
                        }
                    )
                })
                .collect(),
            default_folder,
        });
    };

    // A shop is open.
    let config = app.shop_config();
    let has_details = !config.store.name.trim().is_empty();
    // One read for the three facts the wizard skips steps on: a PIN, items, tables.
    let (has_pin, has_items, has_tables) = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let repos = mb_db::Repos::new(tx);
                    let people = repos.people().list_staff(crate::state::OUTLET)?;
                    let items = repos.menu().list_items(crate::state::OUTLET, false)?;
                    let tables = repos.floor().list_tables(crate::state::OUTLET)?;
                    Ok((
                        people.iter().any(|p| p.pin_hash.is_some()),
                        !items.is_empty(),
                        !tables.is_empty(),
                    ))
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .unwrap_or((false, false, false));

    Ok(FirstRunView {
        needed: !(has_details && has_pin),
        has_shop: true,
        has_details,
        has_pin,
        has_items,
        has_tables,
        shop_path,
        found: Vec::new(),
        default_folder,
    })
}

/// Make a shop. The thing that did not exist.
pub fn create_shop_on(app: &App, folder: String) -> UiResult<FirstRunView> {
    only_before_set_up(app)?;

    let config_dir = crate::config::AppConfig::directory();
    let path = if folder.trim().is_empty() {
        default_shop_path()
    } else {
        let folder = std::path::PathBuf::from(folder.trim());
        if folder.extension().is_some_and(|e| e == "db") {
            folder
        } else {
            folder.join("magicbill.db")
        }
    };

    match crate::startup::adopt(&config_dir, &path)? {
        crate::startup::Startup::Ready { db, path, .. } => {
            crate::log_info!("first run: a new shop was created at {}", path.display());
            app.open_shop(*db, path);
            look_on(app)
        }
        crate::startup::Startup::Failed { error } => Err(error),
        // `adopt` only ever returns Ready or Failed — it calls `open` directly.
        _ => Err(UiError::new(
            "shop.create",
            "The shop could not be opened after it was made. Look in Health for \
             what went wrong.",
        )),
    }
}

/// What a restore from the cloud answers with: the first-run view, and the sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CloudRestoreView {
    pub first_run: FirstRunView,
    /// "312 bills, 4 staff members, 96 other rows and 210 days of totals came down from the cloud."
    pub says: String,
}

/// Bring my shop from the cloud: activate the licence here, read everything the cloud holds
/// under the counter's login into a fresh shop file, and only then open it. `move_here` moves
/// a licence that is bound to another computer — the one that died.
pub fn restore_from_cloud_on(
    app: &App,
    key: String,
    folder: String,
    move_here: bool,
) -> UiResult<CloudRestoreView> {
    only_before_set_up(app)?;
    if app.has_shop() {
        return Err(UiError::new(
            "shop.exists",
            "This computer already has a shop. A shop from the cloud can only be brought onto a \
             computer with none.",
        ));
    }
    let key = key.trim().to_owned();
    if key.is_empty() {
        return Err(UiError::new("licence.key", "Type the licence key first."));
    }

    // The licence first: it is what says which shop, and it hands the counter its login.
    let at = crate::flows::now();
    let outcome = app.with_licensing(|licensing| {
        if move_here {
            licensing.transfer(&key, at, crate::flows::today(at), mb_license::deadline::DEADLINE)
        } else {
            licensing.activate(&key, at, mb_license::deadline::DEADLINE)
        }
    });
    if let Err(e) = outcome {
        return Err(crate::words::from_licence(&e));
    }
    let Some(login) = app.device_login() else {
        return Err(UiError::new(
            "cloud.no_login",
            "The licence was activated but our server gave this counter no login. Try again in a \
             minute.",
        ));
    };

    let config_dir = crate::config::AppConfig::directory();
    let path = if folder.trim().is_empty() {
        default_shop_path()
    } else {
        let folder = std::path::PathBuf::from(folder.trim());
        if folder.extension().is_some_and(|e| e == "db") {
            folder
        } else {
            folder.join("magicbill.db")
        }
    };
    if path.exists() {
        return Err(UiError::new(
            "shop.exists",
            format!(
                "There is already a shop file at {}. Open that one, or choose another folder.",
                path.display()
            ),
        ));
    }

    // Written BEFORE it is opened, the same road a pen-drive restore takes.
    let db = mb_db::Db::open(&mb_db::DbConfig::new(path.clone())).map_err(|e| crate::words::from_db(&e))?;
    let report = match crate::sync::restore_into(app, &db, &login) {
        Ok(report) => report,
        Err(e) => {
            // Nothing half-restored is left behind to be mistaken for a shop.
            drop(db);
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
    };
    drop(db);

    match crate::startup::adopt(&config_dir, &path)? {
        crate::startup::Startup::Ready { db, path, .. } => {
            crate::log_info!("first run: the shop came down from the cloud to {}", path.display());
            app.open_shop(*db, path);
        }
        crate::startup::Startup::Failed { error } => return Err(error),
        _ => {
            return Err(UiError::new(
                "shop.create",
                "The shop could not be opened after it came down. Look in Health for what went wrong.",
            ));
        }
    }
    crate::licensing::after_licence_change(app);
    Ok(CloudRestoreView {
        first_run: look_on(app)?,
        says: report.sentence(),
    })
}

/// The data file inside a folder, when the folder holds exactly one shop.
fn data_file_in(folder: &std::path::Path) -> Option<std::path::PathBuf> {
    for name in ["magicbill.db", "shop.db"] {
        let path = folder.join(name);
        if mb_db::locate::inspect(&path).is_some() {
            return Some(path);
        }
    }
    let mut shops: Vec<std::path::PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "db"))
        .filter(|path| mb_db::locate::inspect(path).is_some())
        .collect();
    if shops.len() == 1 { shops.pop() } else { None }
}

/// Switch this counter to the shop in another folder: its data, its settings and its licence.
/// Whoever is signed in is signed out first, in the shop they were signed in to. Answers with
/// the folder now in use.
pub fn use_shop_folder_on(app: &App, folder: String) -> UiResult<String> {
    only_before_set_up(app)?;

    let folder = std::path::PathBuf::from(folder.trim());
    if !folder.is_dir() {
        return Err(UiError::new(
            "shop.folder",
            format!("There is no folder at {}.", folder.display()),
        ));
    }
    let Some(path) = data_file_in(&folder) else {
        return Err(UiError::new(
            "shop.folder",
            format!(
                "There is no Magic Bill data file in {}. Choose the folder that holds \
                 magicbill.db.",
                folder.display()
            ),
        ));
    };
    let already_open = app
        .with_shop(|shop| Ok(shop.path == path))
        .unwrap_or(false);
    if already_open {
        return Ok(folder.display().to_string());
    }

    let at = crate::flows::now();
    if let Some(who) = app.sessions().end() {
        app.record(&mb_auth::AuditEntry::new(
            at,
            crate::flows::today(at),
            Some(who.staff_id.clone()),
            mb_auth::audit::action::LOGOUT,
            "staff",
        ));
    }

    let config_dir = crate::config::AppConfig::directory();
    match crate::startup::adopt(&config_dir, &path)? {
        crate::startup::Startup::Ready { db, path, .. } => {
            crate::log_info!("the counter moved to the shop at {}", path.display());
            app.open_shop(*db, path);
        }
        crate::startup::Startup::Failed { error } => return Err(error),
        _ => {
            return Err(UiError::new(
                "shop.folder",
                "That shop could not be opened. Look in Health for what went wrong.",
            ));
        }
    }
    crate::licensing::after_licence_change(app);
    // The window hears who is at the counter now: nobody, or the stand-in on a shop with no PIN.
    let current = app.sessions().current();
    app.push(crate::state::Pushed::Session {
        who: current.as_ref().map(|s| s.actor.name.clone()),
        role: current.as_ref().and_then(|s| s.actor.role_name.clone()),
        stand_in: current.as_ref().is_some_and(|s| s.is_stand_in),
    });
    Ok(folder.display().to_string())
}

/// A set-up shop cannot be swapped from the first-run screen.
pub(crate) fn only_before_set_up(app: &App) -> UiResult<()> {
    if look_on(app)?.needed {
        return Ok(());
    }
    crate::guard::require(app, mb_auth::Permission::BackupRun)?;
    Ok(())
}

#[tauri::command]
pub fn first_run(app: tauri::State<'_, App>) -> UiResult<FirstRunView> {
    look_on(&app)
}

#[tauri::command]
pub fn create_shop(app: tauri::State<'_, App>, folder: String) -> UiResult<FirstRunView> {
    create_shop_on(&app, folder)
}

/// Adopt a shop already on this computer — a reinstall, or a drive letter that changed.
#[tauri::command]
pub fn use_existing_shop(app: tauri::State<'_, App>, path: String) -> UiResult<FirstRunView> {
    create_shop_on(&app, path)
}

#[tauri::command]
pub fn use_shop_folder(app: tauri::State<'_, App>, folder: String) -> UiResult<String> {
    use_shop_folder_on(&app, folder)
}

#[tauri::command]
pub fn restore_from_cloud(
    app: tauri::State<'_, App>,
    key: String,
    folder: String,
    move_here: bool,
) -> UiResult<CloudRestoreView> {
    restore_from_cloud_on(&app, key, folder, move_here)
}
