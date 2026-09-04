//! The first five minutes.
//!
//! A counter belongs to an account before it belongs to anybody else. The owner chooses the
//! folder their shop lives in, signs in with the email and password they made at magicbill.in,
//! and the licence on that account is what opens the shop: nothing here makes a shop out of
//! thin air, and nothing here picks a folder for them.

use serde::Serialize;
use serde_json::{Value, json};
use ts_rs::TS;

use crate::state::{App, OUTLET};
use crate::words::{UiError, UiResult};

/// The roles the cloud gives a person on a restaurant that make it theirs to run.
const OWNER_ROLES: [&str; 3] = ["owner", "co_owner", "admin"];

/// Somebody who owns a shop, as the staff list holds them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct OwnerRowView {
    pub id: String,
    pub name: String,
    pub has_pin: bool,
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
    /// The owner's row, once the shop has one: the PIN step sets that person's PIN rather than
    /// hiring a second owner.
    pub owner: Option<OwnerRowView>,
}

/// What the first run knows, without needing a shop to be open.
pub fn look_on(app: &App) -> UiResult<FirstRunView> {
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
            owner: None,
        });
    };

    // A shop is open.
    let config = app.shop_config();
    let has_details = !config.store.name.trim().is_empty();
    // One read for the facts the wizard skips steps on: a PIN, items, tables, and who owns it.
    let (has_pin, has_items, has_tables, owner) = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let repos = mb_db::Repos::new(tx);
                    let people = repos.people().list_staff(OUTLET)?;
                    let items = repos.menu().list_items(OUTLET, false)?;
                    let tables = repos.floor().list_tables(OUTLET)?;
                    let owner = people
                        .iter()
                        .find(|p| p.role_id.as_deref() == Some(mb_auth::RolePreset::Owner.id()))
                        .map(|p| OwnerRowView {
                            id: p.id.as_str().to_owned(),
                            name: p.name.clone(),
                            has_pin: p.pin_hash.is_some(),
                        });
                    Ok((
                        people.iter().any(|p| p.pin_hash.is_some()),
                        !items.is_empty(),
                        !tables.is_empty(),
                        owner,
                    ))
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .unwrap_or((false, false, false, None));

    Ok(FirstRunView {
        needed: !(has_details && has_pin),
        has_shop: true,
        has_details,
        has_pin,
        has_items,
        has_tables,
        shop_path,
        owner,
    })
}

// The owner, signed in.

/// A shop the signed-in account owns, as the cloud described it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerShop {
    pub id: String,
    pub name: String,
    pub address: String,
    pub gstin: String,
    pub short_code: String,
    /// active · trial · suspended · revoked · cancelled — the cloud's word.
    pub status: String,
    /// The licence key, which the cloud gives only to an owner.
    pub key: Option<String>,
}

/// The owner who signed in, until they open a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerSignIn {
    pub name: String,
    pub email: String,
    pub shops: Vec<OwnerShop>,
}

/// A shop the owner may open here. The key stays in Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct OwnerShopView {
    pub id: String,
    pub name: String,
    pub address: String,
    pub gstin: String,
    pub short_code: String,
    /// The licence's standing, in the cloud's word.
    pub licence: String,
}

/// What signing in answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct OwnerSignInView {
    pub name: String,
    pub email: String,
    /// One, usually. More than one and the owner picks.
    pub shops: Vec<OwnerShopView>,
}

/// What opening a shop answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct OwnerOpenedView {
    pub first_run: FirstRunView,
    /// The shop, so the details step starts filled in.
    pub shop: OwnerShopView,
    /// What came down from the cloud, when a shop already had a history there.
    pub came_down: Option<String>,
}

impl OwnerShop {
    fn view(&self) -> OwnerShopView {
        OwnerShopView {
            id: self.id.clone(),
            name: self.name.clone(),
            address: self.address.clone(),
            gstin: self.gstin.clone(),
            short_code: self.short_code.clone(),
            licence: self.status.clone(),
        }
    }
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// The shops in a `mb_my_restaurants` answer, and whether any of them is the caller's as staff
/// rather than as owner.
fn shops_in(answer: &Value) -> (Vec<OwnerShop>, bool) {
    let mut owned = Vec::new();
    let mut staff_at_one = false;
    for row in answer.as_array().into_iter().flatten() {
        let role = text(row, "role");
        if !OWNER_ROLES.contains(&role.as_str()) {
            staff_at_one = true;
            continue;
        }
        let licence = row.get("licence").cloned().unwrap_or(Value::Null);
        owned.push(OwnerShop {
            id: text(row, "id"),
            name: text(row, "name"),
            address: text(row, "address"),
            gstin: text(row, "gstin"),
            short_code: text(row, "short_code"),
            status: text(&licence, "status"),
            key: licence
                .get("key")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(str::to_owned),
        });
    }
    (owned, staff_at_one)
}

/// The owner, by the email and password of their Magic Bill account. Answers with the shops
/// that account owns and holds a licence for; the counter cannot open anything else.
pub fn sign_in_owner_on(app: &App, email: String, password: String) -> UiResult<OwnerSignInView> {
    only_before_set_up(app)?;
    if app.has_shop() {
        return Err(UiError::new(
            "shop.exists",
            "This computer already has a shop open. Settings › Backup is where another folder \
             is chosen.",
        ));
    }
    let email = email.trim().to_owned();
    if email.is_empty() || password.is_empty() {
        return Err(UiError::new(
            "owner.blank",
            "Type the email and the password of your Magic Bill account.",
        ));
    }

    let link = app.link();
    let login = link
        .password_login(&email, &password)
        .map_err(|e| crate::words::from_link(&e))?;
    let answer = link
        .rpc("mb_my_restaurants", &json!({}), &login.access_token)
        .map_err(|e| crate::words::from_link(&e))?;
    let (shops, staff_at_one) = shops_in(&answer);
    let licensed: Vec<OwnerShop> = shops.into_iter().filter(|s| s.key.is_some()).collect();
    if licensed.is_empty() {
        return Err(UiError::new(
            "owner.no_licence",
            if staff_at_one {
                "This account works at a shop but does not own one. The owner signs the counter \
                 in; you sign in on the phone."
            } else {
                "This account has no Magic Bill licence yet. Start your free trial or buy a plan \
                 at magicbill.in, then sign in here again."
            },
        ));
    }

    let view = OwnerSignInView {
        name: login.name.clone(),
        email: login.email.clone(),
        shops: licensed.iter().map(OwnerShop::view).collect(),
    };
    app.with_owner_sign_in(|held| {
        *held = Some(OwnerSignIn {
            name: login.name,
            email: login.email,
            shops: licensed,
        });
    });
    crate::log_info!(
        "first run: {} signed in with {} shop(s)",
        view.email,
        view.shops.len()
    );
    Ok(view)
}

/// What a chosen folder already holds.
enum Holds {
    /// No data file: the shop starts here.
    Nothing,
    /// A data file whose licence names this very shop, or one that was never licensed.
    Theirs(std::path::PathBuf),
    /// A data file that belongs to another shop.
    Another { name: String },
}

/// Whose shop a folder holds, read from the licence beside the data — never by opening it.
fn folder_holds(folder: &std::path::Path, shop: &OwnerShop) -> Holds {
    let Some(path) = data_file_in(folder) else {
        return Holds::Nothing;
    };
    let file = mb_license::LicenceFile::load(folder);
    let device_says = file.device.as_ref().map(|d| d.restaurant_id.clone());
    let snapshot: Value = file
        .snapshot
        .as_ref()
        .and_then(|s| serde_json::from_str(&s.payload).ok())
        .unwrap_or(Value::Null);
    let licence = snapshot.get("licence").cloned().unwrap_or(Value::Null);
    let key_says = licence
        .get("key")
        .and_then(Value::as_str)
        .filter(|k| !k.is_empty())
        .map(str::to_owned);

    let unclaimed = device_says.is_none() && key_says.is_none();
    let same_shop = device_says.as_deref() == Some(shop.id.as_str());
    let same_key = key_says.is_some() && key_says == shop.key;
    if unclaimed || same_shop || same_key {
        return Holds::Theirs(path);
    }
    let name = text(&licence, "shop_name");
    Holds::Another {
        name: if name.is_empty() {
            "another shop".to_owned()
        } else {
            name
        },
    }
}

/// Activate the licence here, or move it here from the computer that is gone.
fn take_licence(app: &App, key: &str, move_here: bool) -> UiResult<()> {
    let at = crate::flows::now();
    let outcome = app.with_licensing(|licensing| {
        if move_here {
            licensing.transfer(
                key,
                at,
                crate::flows::today(at),
                mb_license::deadline::DEADLINE,
            )
        } else {
            licensing.activate(key, at, mb_license::deadline::DEADLINE)
        }
    });
    outcome.map_err(|e| {
        let said = crate::words::from_licence(&e);
        // Its own code, because the screen answers it with a checkbox rather than a sentence.
        if matches!(
            e,
            mb_license::LicenceError::Cloud(mb_license::CloudError::BoundElsewhere { .. })
        ) {
            UiError::new("licence.bound_elsewhere", said.message)
        } else {
            said
        }
    })
}

/// The owner's own row in the staff list, made if the shop has none. Named after the account;
/// the PIN step lets them correct the name and gives them the PIN.
fn ensure_owner_row(app: &App, name: &str) -> UiResult<()> {
    let at = crate::flows::now();
    let name = if name.trim().is_empty() {
        "Owner"
    } else {
        name.trim()
    };
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let owner_role = mb_auth::RolePreset::Owner.id();
                let has_one = repos
                    .people()
                    .list_staff(OUTLET)?
                    .iter()
                    .any(|p| p.role_id.as_deref() == Some(owner_role));
                if has_one {
                    return Ok(());
                }
                let member = mb_db::repo::people::StaffMember {
                    id: mb_core::StaffId::new(crate::newid::fresh_at("staff", at)),
                    name: name.to_owned(),
                    role_id: Some(owner_role.to_owned()),
                    role_name: None,
                    pin_hash: None,
                    status: mb_db::repo::people::StaffStatus::Active,
                    permissions: mb_auth::PermissionSet::new(),
                    max_discount_bp: None,
                    max_discount: None,
                };
                repos.people().save_staff(OUTLET, &member, at)
            })
            .map_err(|e| crate::words::from_db(&e))
    })
}

/// Open the shop the signed-in owner chose, in the folder they chose. `config_dir` is where the
/// pointer to the folder is written — the app's own, or a test's.
pub fn open_as_owner_on(
    app: &App,
    config_dir: &std::path::Path,
    restaurant_id: &str,
    folder: &str,
    move_here: bool,
) -> UiResult<OwnerOpenedView> {
    only_before_set_up(app)?;
    if app.has_shop() {
        return Err(UiError::new(
            "shop.exists",
            "This computer already has a shop open. Settings › Backup is where another folder \
             is chosen.",
        ));
    }
    let folder = std::path::PathBuf::from(folder.trim());
    if folder.as_os_str().is_empty() {
        return Err(UiError::new("shop.folder", "Choose the folder first."));
    }
    if !folder.is_dir() {
        return Err(UiError::new(
            "shop.folder",
            format!("There is no folder at {}.", folder.display()),
        ));
    }
    let Some((owner_name, shop)) = app.with_owner_sign_in(|held| {
        held.as_ref().and_then(|signed| {
            signed
                .shops
                .iter()
                .find(|s| s.id == restaurant_id)
                .map(|s| (signed.name.clone(), s.clone()))
        })
    }) else {
        return Err(UiError::new("owner.sign_in", "Sign in first."));
    };
    let Some(key) = shop.key.clone() else {
        return Err(UiError::new(
            "owner.no_licence",
            format!(
                "{} has no licence. Start one at magicbill.in first.",
                shop.name
            ),
        ));
    };

    let holds = folder_holds(&folder, &shop);
    if let Holds::Another { name } = &holds {
        return Err(UiError::new(
            "shop.foreign",
            format!(
                "The folder {} holds {name}\u{2019}s data, not {}\u{2019}s. Nothing there has \
                 been touched. Choose another folder.",
                folder.display(),
                shop.name
            ),
        ));
    }

    // The licence first: it says which shop this is and hands the counter its login. Nothing
    // is written to the folder until it has answered.
    take_licence(app, &key, move_here)?;

    let mut came_down = None;
    let path = match holds {
        Holds::Theirs(path) => path,
        Holds::Nothing => {
            let Some(login) = app.device_login() else {
                return Err(UiError::new(
                    "cloud.no_login",
                    "The licence was activated but our server gave this counter no login. Try \
                     again in a minute.",
                ));
            };
            let path = folder.join("magicbill.db");
            // Written BEFORE it is opened, the same road a pen-drive restore takes. A shop with
            // a history in the cloud comes down whole; a new one comes down empty.
            let db = mb_db::Db::open(&mb_db::DbConfig::new(path.clone()))
                .map_err(|e| crate::words::from_db(&e))?;
            match crate::sync::restore_into(app, &db, &login) {
                Ok(report) => {
                    if report.bills > 0 || report.staff > 0 {
                        came_down = Some(report.sentence());
                    }
                }
                Err(e) => {
                    // Nothing half-restored is left behind to be mistaken for a shop.
                    drop(db);
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
            }
            drop(db);
            path
        }
        Holds::Another { .. } => unreachable!("refused above"),
    };

    match crate::startup::adopt(config_dir, &path)? {
        crate::startup::Startup::Ready { db, path, .. } => {
            crate::log_info!("first run: {} opened {}", shop.name, path.display());
            app.open_shop(*db, path);
        }
        crate::startup::Startup::Failed { error } => return Err(error),
        // `adopt` only ever returns Ready or Failed — it calls `open` directly.
        _ => {
            return Err(UiError::new(
                "shop.create",
                "The shop could not be opened. Look in Health for what went wrong.",
            ));
        }
    }
    crate::licensing::after_licence_change(app);
    ensure_owner_row(app, &owner_name)?;
    app.with_owner_sign_in(|held| *held = None);

    Ok(OwnerOpenedView {
        first_run: look_on(app)?,
        shop: shop.view(),
        came_down,
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
    let already_open = app.with_shop(|shop| Ok(shop.path == path)).unwrap_or(false);
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
pub fn sign_in_owner(
    app: tauri::State<'_, App>,
    email: String,
    password: String,
) -> UiResult<OwnerSignInView> {
    sign_in_owner_on(&app, email, password)
}

#[tauri::command]
pub fn open_as_owner(
    app: tauri::State<'_, App>,
    restaurant_id: String,
    folder: String,
    move_here: bool,
) -> UiResult<OwnerOpenedView> {
    open_as_owner_on(
        &app,
        &crate::config::AppConfig::directory(),
        &restaurant_id,
        &folder,
        move_here,
    )
}

#[tauri::command]
pub fn use_shop_folder(app: tauri::State<'_, App>, folder: String) -> UiResult<String> {
    use_shop_folder_on(&app, folder)
}
