//! The first run: a folder the owner chose, the account they made at magicbill.in, and the
//! shop its licence names. Nothing else makes a shop.

use std::path::Path;
use std::sync::{Arc, Mutex};

use mb_db::{Db, DbConfig, Repos};
use mb_license::cloud::{DeviceLogin, Stub};
use mb_license::{Cloud, LicenceFile, Licensing};
use serde_json::{Value, json};

use crate::cloud::{Link, LinkError, OwnerLogin, Page, Session};
use crate::firstrun::{look_on, open_as_owner_on, sign_in_owner_on};
use crate::licence_tests::machine;
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

const PASSWORD: &str = "correct-horse";
const KEY: &str = "MB-STUB-0001";

/// The cloud as an owner meets it: one password that works, and whatever shops the account has.
#[derive(Debug)]
struct OwnerCloud {
    restaurants: Mutex<Value>,
}

impl OwnerCloud {
    fn with(restaurants: Value) -> Arc<OwnerCloud> {
        Arc::new(OwnerCloud {
            restaurants: Mutex::new(restaurants),
        })
    }
}

impl Link for OwnerCloud {
    fn password_login(&self, email: &str, password: &str) -> Result<OwnerLogin, LinkError> {
        if password != PASSWORD {
            return Err(LinkError::Refused(
                "That email and password do not match a Magic Bill account.".to_owned(),
            ));
        }
        Ok(OwnerLogin {
            access_token: "owner-token".to_owned(),
            name: "Meena".to_owned(),
            email: email.to_owned(),
        })
    }
    fn rpc(&self, name: &str, _: &Value, token: &str) -> Result<Value, LinkError> {
        assert_eq!(
            token, "owner-token",
            "the shops are asked for under the owner's login"
        );
        assert_eq!(name, "mb_my_restaurants");
        Ok(self.restaurants.lock().unwrap().clone())
    }
    // A brand-new shop has nothing in the cloud: every table comes back empty.
    fn rest(&self, _: &str, _: &str, _: usize, _: usize) -> Result<Page, LinkError> {
        Ok(Page {
            rows: Vec::new(),
            total: Some(0),
        })
    }
    fn refresh_session(&self, _: &str) -> Result<Session, LinkError> {
        Err(LinkError::Unreachable)
    }
    fn download(&self, _: &str, _: &Path) -> Result<String, LinkError> {
        Err(LinkError::Unreachable)
    }
}

fn owned(id: &str, name: &str) -> Value {
    json!({
        "id": id, "name": name, "short_code": "ABC123",
        "address": "14 Kamaraj Street, Chennai", "gstin": "33AAAAA0000A1Z5",
        "role": "owner", "staff": null, "permissions": ["reports.view"],
        "licence": { "status": "active", "plan": "starter", "plan_name": "Starter",
                     "features": [], "key": KEY, "bound": false, "bound_device": null }
    })
}

fn staff_at(id: &str, name: &str) -> Value {
    json!({
        "id": id, "name": name, "short_code": "XYZ789", "address": "", "gstin": "",
        "role": "staff", "staff": { "id": "staff_9", "name": "Meena" }, "permissions": [],
        "licence": { "status": "active", "plan": "starter", "plan_name": "Starter",
                     "features": [], "key": null, "bound": true, "bound_device": null }
    })
}

/// A counter with no shop, a licence office that knows one key, and the owner's cloud.
fn a_bare_counter(scratch: &Scratch, restaurants: Value) -> App {
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    let at = crate::flows::now();
    let stub = Arc::new(Stub::active(&machine(), crate::flows::today(at), at));
    let dir = scratch.dir().join("licence");
    let _ = std::fs::create_dir_all(&dir);
    app.use_licensing(Licensing::new(
        dir,
        machine(),
        stub as Arc<dyn Cloud>,
        "test",
    ));
    app.use_link(OwnerCloud::with(restaurants));
    app
}

fn config_dir(scratch: &Scratch) -> std::path::PathBuf {
    let dir = scratch.dir().join("config");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn shop_folder(scratch: &Scratch, name: &str) -> std::path::PathBuf {
    let dir = scratch.dir().join(name);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn owners_in(app: &App) -> Vec<String> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Ok(Repos::new(tx)
                    .people()
                    .list_staff(OUTLET)?
                    .into_iter()
                    .filter(|p| p.role_id.as_deref() == Some("role_owner"))
                    .map(|p| p.name)
                    .collect())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a shop")
}

#[test]
fn signing_in_lists_the_shops_the_account_owns_and_none_it_only_works_at() {
    let scratch = Scratch::new("firstrun_signin");
    let app = a_bare_counter(
        &scratch,
        json!([
            owned("rest_anand", "Anand Bhavan"),
            staff_at("rest_other", "Saravana")
        ]),
    );

    let wrong = sign_in_owner_on(&app, "meena@example.in".to_owned(), "nope".to_owned())
        .expect_err("a wrong password signs nobody in");
    assert_eq!(wrong.code, "cloud.refused");

    let view = sign_in_owner_on(&app, " meena@example.in ".to_owned(), PASSWORD.to_owned())
        .expect("signed in");
    assert_eq!(view.name, "Meena");
    assert_eq!(view.email, "meena@example.in");
    assert_eq!(
        view.shops.len(),
        1,
        "only the shop she owns: {:?}",
        view.shops
    );
    assert_eq!(view.shops[0].name, "Anand Bhavan");
    assert_eq!(view.shops[0].licence, "active");
    assert_eq!(view.shops[0].gstin, "33AAAAA0000A1Z5");
}

#[test]
fn an_account_with_no_licence_is_sent_to_the_website_and_gets_no_shop() {
    let scratch = Scratch::new("firstrun_unlicensed");
    let app = a_bare_counter(&scratch, json!([]));
    let refused = sign_in_owner_on(&app, "new@example.in".to_owned(), PASSWORD.to_owned())
        .expect_err("no licence, no shop");
    assert_eq!(refused.code, "owner.no_licence");
    assert!(
        refused.message.contains("magicbill.in"),
        "{}",
        refused.message
    );

    // Staff at somebody else's shop is not an owner either.
    let app = a_bare_counter(&scratch, json!([staff_at("rest_other", "Saravana")]));
    let refused = sign_in_owner_on(&app, "meena@example.in".to_owned(), PASSWORD.to_owned())
        .expect_err("staff do not open counters");
    assert_eq!(refused.code, "owner.no_licence");
    assert!(
        refused.message.contains("does not own one"),
        "{}",
        refused.message
    );
    assert!(!app.has_shop());
}

#[test]
fn an_empty_folder_becomes_the_shop_and_the_owner_is_named_after_the_account() {
    let scratch = Scratch::new("firstrun_new");
    let app = a_bare_counter(&scratch, json!([owned("rest_anand", "Anand Bhavan")]));
    let config = config_dir(&scratch);
    let folder = shop_folder(&scratch, "Anand Bhavan");

    // Nothing opens before the owner has signed in, and nothing opens with no folder.
    let early = open_as_owner_on(&app, &config, "rest_anand", folder.to_str().unwrap(), false)
        .expect_err("not signed in");
    assert_eq!(early.code, "owner.sign_in");
    sign_in_owner_on(&app, "meena@example.in".to_owned(), PASSWORD.to_owned()).expect("signed in");
    let blank = open_as_owner_on(&app, &config, "rest_anand", "  ", false)
        .expect_err("no folder was chosen");
    assert_eq!(blank.code, "shop.folder");
    assert!(!app.has_shop(), "a refusal opens nothing");

    let opened = open_as_owner_on(&app, &config, "rest_anand", folder.to_str().unwrap(), false)
        .expect("the shop opens");
    assert!(app.has_shop());
    assert!(
        folder.join("magicbill.db").is_file(),
        "the data is in the chosen folder"
    );
    assert!(
        LicenceFile::path(&folder).is_file(),
        "the licence sits beside the data, so one folder holds the whole shop"
    );
    assert_eq!(
        mb_db::locate::read_config(&config)
            .expect("readable")
            .as_deref(),
        Some(folder.join("magicbill.db").as_path()),
        "start-up will find it again"
    );
    // The details step starts filled in from the account's shop.
    assert_eq!(opened.shop.name, "Anand Bhavan");
    assert_eq!(opened.shop.address, "14 Kamaraj Street, Chennai");
    assert_eq!(
        opened.came_down, None,
        "a new shop has no history to bring down"
    );
    // The owner is a row already; the PIN step gives that row its PIN rather than hiring twice.
    let owner = opened.first_run.owner.as_ref().expect("the owner's row");
    assert_eq!(owner.name, "Meena");
    assert!(!owner.has_pin);
    assert!(
        opened.first_run.needed,
        "a PIN and a name are still to come"
    );
    assert_eq!(owners_in(&app), vec!["Meena".to_owned()]);
    assert!(
        app.device_login().is_some(),
        "activation handed the counter its own login"
    );
}

#[test]
fn a_folder_holding_another_shops_data_is_refused_and_not_touched() {
    let scratch = Scratch::new("firstrun_foreign");
    let app = a_bare_counter(&scratch, json!([owned("rest_anand", "Anand Bhavan")]));
    let config = config_dir(&scratch);
    let folder = shop_folder(&scratch, "somebody-elses");
    // Another shop's data and its licence, as a counter would leave them.
    let path = folder.join("magicbill.db");
    drop(Db::open(&DbConfig::new(path.clone())).expect("their shop"));
    let theirs = LicenceFile {
        device: Some(DeviceLogin {
            device_id: "dev_1".to_owned(),
            restaurant_id: "rest_saravana".to_owned(),
            access_token: "a".to_owned(),
            refresh_token: "r".to_owned(),
            expires_at: mb_core::Timestamp::EPOCH,
        }),
        ..LicenceFile::default()
    };
    theirs.save(&folder).expect("their licence");
    let before = std::fs::metadata(&path).expect("the file").len();

    sign_in_owner_on(&app, "meena@example.in".to_owned(), PASSWORD.to_owned()).expect("signed in");
    let refused = open_as_owner_on(&app, &config, "rest_anand", folder.to_str().unwrap(), false)
        .expect_err("not her shop");
    assert_eq!(refused.code, "shop.foreign");
    assert!(
        refused.message.contains("Choose another folder"),
        "{}",
        refused.message
    );

    assert!(!app.has_shop(), "nothing opened");
    assert_eq!(std::fs::metadata(&path).expect("still there").len(), before);
    assert_eq!(
        LicenceFile::load(&folder),
        theirs,
        "their licence was not rewritten"
    );
    assert!(
        mb_db::locate::read_config(&config)
            .expect("readable")
            .is_none(),
        "start-up was not pointed at it"
    );
}

#[test]
fn a_folder_holding_this_shops_data_is_opened_as_it_is() {
    let scratch = Scratch::new("firstrun_reinstall");
    let app = a_bare_counter(&scratch, json!([owned("rest_anand", "Anand Bhavan")]));
    let config = config_dir(&scratch);
    let folder = shop_folder(&scratch, "Anand Bhavan");
    // The shop as the old installation left it: an owner called Sachin, and a licence naming
    // this restaurant.
    let path = folder.join("magicbill.db");
    // Opened once by a counter, which is what seeds the roles an owner row hangs off.
    let old = App::new(crate::config::AppConfig::default()).expect("the font loads");
    old.open_shop(
        Db::open(&DbConfig::new(path.clone())).expect("their shop"),
        path.clone(),
    );
    drop(old);
    let db = Db::open(&DbConfig::new(path.clone())).expect("their shop");
    let at = crate::flows::now();
    db.transaction(|tx| {
        Repos::new(tx).people().save_staff(
            OUTLET,
            &mb_db::repo::people::StaffMember {
                id: mb_core::StaffId::new("staff_sachin"),
                name: "Sachin".to_owned(),
                role_id: Some("role_owner".to_owned()),
                role_name: None,
                pin_hash: None,
                status: mb_db::repo::people::StaffStatus::Active,
                permissions: mb_auth::PermissionSet::new(),
                max_discount_bp: None,
                max_discount: None,
            },
            at,
        )
    })
    .expect("an owner");
    drop(db);
    LicenceFile {
        device: Some(DeviceLogin {
            device_id: "dev_old".to_owned(),
            restaurant_id: "rest_anand".to_owned(),
            access_token: "a".to_owned(),
            refresh_token: "r".to_owned(),
            expires_at: mb_core::Timestamp::EPOCH,
        }),
        ..LicenceFile::default()
    }
    .save(&folder)
    .expect("the licence");

    sign_in_owner_on(&app, "meena@example.in".to_owned(), PASSWORD.to_owned()).expect("signed in");
    let opened = open_as_owner_on(&app, &config, "rest_anand", folder.to_str().unwrap(), false)
        .expect("her own shop opens");
    assert!(app.has_shop());
    assert_eq!(
        owners_in(&app),
        vec!["Sachin".to_owned()],
        "the owner row that was there is kept, and no second one is made"
    );
    assert_eq!(
        opened.first_run.owner.as_ref().map(|o| o.name.as_str()),
        Some("Sachin")
    );
    assert_eq!(
        look_on(&app).expect("view").shop_path,
        path.display().to_string()
    );
}
