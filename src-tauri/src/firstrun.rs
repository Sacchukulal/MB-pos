//! **The first five minutes** — P30.5.
//!
//! # What was wrong, and it was not a small thing
//!
//! Until P30.5 there was **no way to create a shop.** `startup::adopt` existed in
//! Rust, nothing exposed it, and nothing in the interface made a database. So a
//! genuinely fresh install opened onto the billing screen with no shop behind
//! it: every screen's first call failed, each failure raised a toast, the
//! toasts stacked three deep, and the owner was left on a till that could not
//! be set up and could not be escaped.
//!
//! Every session before this one tested against a seeded demo shop, so not one
//! of them ever saw it. The owner installed the build on a second computer on
//! 2026-08-16 and hit it in the first ten seconds.
//!
//! # The shape now
//!
//! **A shop that is not set up does not show the counter at all.** It shows one
//! thing at a time, in order, and hands over to billing when the compulsory
//! part is done:
//!
//! | | | |
//! |---|---|---|
//! | 1 | make the shop, or restore a backup | **compulsory** |
//! | 2 | the shop's name and address | **compulsory** — a bill without them is not one a customer can claim |
//! | 3 | a PIN for whoever is in charge | **compulsory** — audit C1 |
//! | 4 | the menu | skippable |
//! | 5 | a printer | skippable |
//! | 6 | tables | skippable |
//! | 7 | where backups go | skippable, and asked for again later |
//!
//! **Compulsory means the button is not there until it is done**, and skippable
//! means a plain "I will do this later" that does not nag. The three compulsory
//! ones are the three a shop cannot legally or safely trade without; everything
//! else is a shop's own business and can wait until it has taken some money.

use serde::Serialize;
use ts_rs::TS;

use crate::state::App;
use crate::words::{UiError, UiResult};

/// Where a brand-new shop goes: beside the configuration, which is where
/// start-up already looks first.
fn default_shop_path() -> std::path::PathBuf {
    crate::config::AppConfig::directory().join("magicbill.db")
}

/// Where the first run has got to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct FirstRunView {
    /// **True while the counter must not be shown.** The shell reads this and
    /// renders the set-up flow instead of the billing screen.
    pub needed: bool,
    /// True once a database exists and is open.
    pub has_shop: bool,
    /// True once the shop has a name on it.
    pub has_details: bool,
    /// True once somebody has a PIN.
    pub has_pin: bool,
    /// Where the shop's data file is, once there is one.
    pub shop_path: String,
    /// Other databases already on this computer — an owner reinstalling, or a
    /// drive letter that changed. Offering these is A5: v1 showed a first-run
    /// wizard with the shop three folders away.
    pub found: Vec<String>,
    /// The default folder a new shop would go in, shown so nobody has to guess.
    pub default_folder: String,
}

/// What the first run knows, without needing a shop to be open.
pub fn look_on(app: &App) -> UiResult<FirstRunView> {
    let default_folder = default_shop_path().display().to_string();

    let Some(shop_path) = app.with_shop(|shop| Ok(shop.path.display().to_string())).ok() else {
        return Ok(FirstRunView {
            needed: true,
            has_shop: false,
            has_details: false,
            has_pin: false,
            shop_path: String::new(),
            // Every database already on this computer, described. Not
            // `settings::backup::find_shops_on`: that one is behind the
            // `backup.run` permission, and on a first run there is nobody to
            // hold a permission yet.
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

    // A shop is open. The two other compulsory facts are read from it.
    let config = app.shop_config();
    let has_details = !config.store.name.trim().is_empty();
    let has_pin = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| mb_db::Repos::new(tx).people().list_staff(crate::state::OUTLET))
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

/// **Make a shop.** The thing that did not exist.
///
/// `folder` empty means the usual place beside the configuration, which is
/// what nearly everybody will use. A folder is accepted so that a shop can put
/// its data on a second drive — audit A5's real case, where the owner knows
/// exactly where they want it.
///
/// Creating one is idempotent in the way that matters: `Db::open` makes the
/// file if it is missing and runs the migrations if it is not, so pressing the
/// button twice opens the same shop rather than making two.
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
        // `adopt` only ever returns Ready or Failed — it calls `open`
        // directly. An unreachable arm is still an arm somebody could reach
        // after an edit, so it says something rather than panicking.
        _ => Err(UiError::new(
            "shop.create",
            "The shop could not be opened after it was made. Look in Health for \
             what went wrong.",
        )),
    }
}

/// **A set-up shop cannot be swapped from the first-run screen.**
///
/// These three commands are the only ones that work with no shop at all, so
/// they cannot ask for a permission — there is nobody to hold one. The moment
/// the shop IS set up that stops being true, and pointing a working till at a
/// different database becomes what it really is: a backup-level decision.
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

/// Adopt a shop already on this computer — a reinstall, or a drive letter that
/// changed.
#[tauri::command]
pub fn use_existing_shop(app: tauri::State<'_, App>, path: String) -> UiResult<FirstRunView> {
    create_shop_on(&app, path)
}
