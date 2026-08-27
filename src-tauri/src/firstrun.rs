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
    let has_pin = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    mb_db::Repos::new(tx)
                        .people()
                        .list_staff(crate::state::OUTLET)
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .map(|people| people.iter().any(|p| p.pin_hash.is_some()))
        .unwrap_or(false);

    Ok(FirstRunView {
        needed: !(has_details && has_pin),
        has_shop: true,
        has_details,
        has_pin,
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
