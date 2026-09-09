//! The licence, on the counter.

use std::sync::Arc;
use std::time::Duration;

use mb_auth::Permission;
use mb_auth::audit::{AuditEntry, action};
use mb_license::{Cloud, Feature, Licensing, MachineId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::state::{App, Pushed};
use crate::words::{self, UiError, UiResult};
use crate::{guard, log_info, log_warn};

/// The one outlet this counter is.
const OUTLET: &str = "outlet_default";

/// The routine check: once a day.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 3600);
/// A check that could not reach the cloud is tried again after this.
pub const RETRY_AFTER: Duration = Duration::from_secs(3600);
/// Opening the Account screen checks again when the last check is older than this.
pub const STALE_ON_OPEN: Duration = Duration::from_secs(6 * 3600);

/// Build the licensing subsystem, and the client it and the sender share.
#[cfg(not(test))]
#[must_use]
pub fn start() -> (Licensing, Arc<crate::cloud::Http>) {
    let dir = crate::config::AppConfig::directory();
    let machine = MachineId::of(&dir);
    let http = crate::cloud::Http::new();
    let cloud: Arc<dyn Cloud> = Arc::clone(&http) as Arc<dyn Cloud>;
    (
        Licensing::new(dir, machine, cloud, env!("CARGO_PKG_VERSION")),
        http,
    )
}

/// The same thing on a scratch folder, so a test never reads — or writes — the licence
/// belonging to whoever is running it. The cloud is a stub with nothing on it.
#[cfg(test)]
#[must_use]
pub fn for_tests() -> Licensing {
    let mut licensing = for_tests_blank();
    // A test shop has a running plan unless a test says otherwise: the door is open.
    let _ = licensing.activate("MB-STUB-0001", now(), mb_license::deadline::DEADLINE);
    licensing
}

/// The same, with no licence on it at all: a first run.
#[cfg(test)]
#[must_use]
pub fn for_tests_blank() -> Licensing {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NTH: AtomicU32 = AtomicU32::new(0);
    // A folder of its own per app, so tests that run at once never share a licence file.
    let dir = std::env::temp_dir().join(format!(
        "mb-licence-test-{}-{}",
        std::process::id(),
        NTH.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(mb_license::LicenceFile::path(&dir));
    let machine = MachineId::for_tests("test-machine-0001");
    let cloud: Arc<dyn Cloud> = Arc::new(mb_license::cloud::Stub::active(
        &machine,
        today(now()),
        now(),
    ));
    Licensing::new(dir, machine, cloud, "test")
}

// The gate.

/// Refuse a feature this shop is not entitled to.
pub fn gate(app: &App, feature: Feature) -> UiResult<()> {
    let entitlement = app.entitlement();
    match entitlement.may(feature) {
        Ok(()) => Ok(()),
        Err(refusal) => Err(words::licence_refusal(&refusal, &entitlement, today(now()))),
    }
}

#[cfg(test)]
pub const GATED: &[(&str, Feature)] = &[
    // The reports screen and its exports.
    ("report_list", Feature::Reports),
    ("report", Feature::Reports),
    ("report_csv", Feature::Reports),
    ("report_pdf", Feature::Reports),
    ("dashboard", Feature::Reports),
    ("open_pairing", Feature::MobileOrdering),
    ("allow_device", Feature::MobileOrdering),
    // The stock book — the SCREENS, and only the screens.
    ("inventory", Feature::Inventory),
    ("recipe", Feature::Inventory),
    ("save_material", Feature::Inventory),
    ("save_recipe", Feature::Inventory),
    ("delete_recipe", Feature::Inventory),
    ("record_stock_movement", Feature::Inventory),
    ("rebuild_stock_balances", Feature::Inventory),
    ("resolve_stock_problem", Feature::Inventory),
    ("stock_variance", Feature::Inventory),
    ("buy_list_text", Feature::Inventory),
];

// What the screen sees.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct LicenceView {
    /// `fine` · `grace` · `expired` · `suspended` · `revoked` · `cancelled` · `never-activated`
    /// · `trial-ended` · `needs-checking` · `bound-elsewhere` · `emergency`.
    pub standing: String,
    /// The chip: "Active", "Grace period", "Not activated".
    pub chip: String,
    /// `ok`, `warn` or `danger` — and the words say it too.
    pub tone: String,
    /// The sentence. Empty when there is nothing to say.
    pub headline: String,
    /// True while this computer holds a licence.
    pub has_licence: bool,
    /// True when the licence is on another computer now.
    pub bound_elsewhere: bool,
    /// The name on the account that holds the licence. Blank until the cloud has said.
    pub owner_name: String,
    /// That account's mobile, ten digits, or blank.
    pub owner_phone: String,
    pub shop_name: String,
    pub plan_name: String,
    /// "Renews on" · "Ends on" · "Ended on" · "Trial ends on". Empty when there is no date.
    pub date_label: String,
    /// "12 September", not a date field.
    pub date: String,
    /// The key, only for somebody who may change the licence; blank otherwise.
    pub key: String,
    /// What staff type on a phone to reach this shop. Empty until the cloud says.
    pub restaurant_code: String,
    pub phones_allowed: u32,
    pub tills_allowed: u32,
    /// What the plan includes, in the shop's words.
    pub included: Vec<String>,
    /// "checked 4 minutes ago", or "never".
    pub checked: String,
    pub still_held: String,
    /// The clock went backwards.
    pub clock_note: String,
    /// Whether this person may press anything on this screen.
    pub may_manage: bool,
    /// The cloud copy, in one sentence.
    pub cloud_copy: String,
    /// `ok`, `warn` or `danger`, for the sentence above.
    pub cloud_tone: String,
}

/// `ok`, `warn` or `danger`.
#[must_use]
pub const fn tone_for(standing: mb_license::Standing) -> &'static str {
    match standing {
        mb_license::Standing::Fine => "ok",
        mb_license::Standing::InGrace { .. }
        | mb_license::Standing::Ending { .. }
        | mb_license::Standing::NeverActivated
        | mb_license::Standing::NeedsChecking
        | mb_license::Standing::Emergency { .. }
        | mb_license::Standing::Cancelled
        | mb_license::Standing::TrialEnded => "warn",
        mb_license::Standing::Expired
        | mb_license::Standing::Suspended
        | mb_license::Standing::Revoked
        | mb_license::Standing::BoundElsewhere => "danger",
    }
}

/// The cloud copy, as one sentence and a tone.
#[must_use]
#[allow(
    clippy::integer_division,
    reason = "an age in whole hours for a sentence, not money"
)]
pub fn cloud_copy_says(app: &App, at: mb_core::Timestamp) -> (String, &'static str) {
    let status = app.sync_status();
    if app.device_login().is_none() {
        return if app.with_licence(|l| l.key().is_some()) {
            (
                "The cloud copy is waiting for this counter's login, which the next licence check issues."
                    .to_owned(),
                "warn",
            )
        } else {
            (
                "Bills are copied to the cloud once the licence key is entered.".to_owned(),
                "warn",
            )
        };
    }
    if let Some(why) = status.stopped {
        return (why, "danger");
    }
    let waiting = app
        .shop_db()
        .and_then(|db| {
            db.read_transaction(|tx| mb_db::Repos::new(tx).outbox().pending_count())
                .ok()
        })
        .unwrap_or(0);
    if let Some(behind) = status.behind_by(at) {
        let hours = i64::try_from(behind.as_secs() / 3600).unwrap_or(0);
        return (
            format!(
                "The cloud copy is {} behind — {} waiting. {}",
                words::count(hours.max(1), "hour", "hours"),
                words::count(waiting, "row", "rows"),
                status
                    .last_error
                    .unwrap_or_else(|| "We could not reach our server.".to_owned())
            ),
            "warn",
        );
    }
    let last = status
        .last_push_at
        .map(|ms| words::when(mb_core::Timestamp::from_millis(ms)))
        .unwrap_or_else(|| "never".to_owned());
    let refusal = status
        .last_refusal
        .map(|r| format!(" The cloud refused one row: {r}."))
        .unwrap_or_default();
    let queue = if waiting == 0 {
        "Nothing waiting".to_owned()
    } else {
        format!("{} waiting", words::count(waiting, "row", "rows"))
    };
    (
        format!("Last copied to the cloud: {last}. {queue}.{refusal}"),
        if refusal.is_empty() { "ok" } else { "warn" },
    )
}

/// What the date row is called, by what the licence is doing. `None` when there is no date to
/// show.
#[must_use]
pub fn date_label_for(
    standing: mb_license::Standing,
    status: Option<mb_license::Status>,
) -> Option<&'static str> {
    Some(match standing {
        mb_license::Standing::Fine if status == Some(mb_license::Status::Trial) => "Trial ends on",
        mb_license::Standing::Fine => "Renews on",
        mb_license::Standing::InGrace { .. } => "Payment was due on",
        mb_license::Standing::Ending { .. } => "Ends on",
        mb_license::Standing::Cancelled => "Ended on",
        mb_license::Standing::Expired => "Ran out on",
        mb_license::Standing::TrialEnded => "Trial ended on",
        mb_license::Standing::Suspended
        | mb_license::Standing::Revoked
        | mb_license::Standing::NeverActivated
        | mb_license::Standing::NeedsChecking
        | mb_license::Standing::BoundElsewhere
        | mb_license::Standing::Emergency { .. } => return None,
    })
}

/// A mobile as the counter shows it: the ten digits, or whatever the cloud sent when it is
/// not an Indian mobile.
fn local_phone(cloud_says: &str) -> String {
    mb_core::Phone::parse_optional(cloud_says)
        .ok()
        .flatten()
        .map_or_else(|| cloud_says.trim().to_owned(), |p| p.as_str().to_owned())
}

/// Build the view. Never fails.
pub fn view_on(app: &App) -> LicenceView {
    let at = now();
    let day = today(at);
    let entitlement = app.entitlement();
    let standing = entitlement.standing;
    let may_manage = guard::require(app, Permission::LicenceManage).is_ok();

    let (still_held, clock_note, restaurant_code, key, owner_name, owner_phone, status) = app
        .with_licence(|licensing| {
            let held = licensing
                .file()
                .pending_release
                .as_ref()
                .map(|_| {
                    "This computer has stopped using the licence, but we could not tell our \
                     server. The licence is still held. We will keep trying, and you can also \
                     release it from magicbill.in."
                        .to_owned()
                })
                .unwrap_or_default();
            let clock = match licensing.clock_says(at) {
                mb_license::ClockSays::Fine => String::new(),
                mb_license::ClockSays::WentBackwards { .. } => {
                    "This computer's clock is behind. Nothing is blocked, but the licence will \
                     need checking online soon. Check the date and time."
                        .to_owned()
                }
            };
            let snapshot = licensing.snapshot();
            let licence = snapshot.as_ref().map(|s| &s.licence);
            (
                held,
                clock,
                licence
                    .and_then(|l| l.short_code.clone())
                    .unwrap_or_default(),
                if may_manage {
                    licensing.key().unwrap_or_default()
                } else {
                    String::new()
                },
                licence.map(|l| l.owner_name.clone()).unwrap_or_default(),
                licence
                    .map(|l| local_phone(&l.owner_phone))
                    .unwrap_or_default(),
                licence.map(|l| l.status),
            )
        });

    let date_label = date_label_for(standing, status).unwrap_or_default();
    let date = if date_label.is_empty() {
        String::new()
    } else {
        entitlement
            .renews_on
            .map(|on| words::day(on, day))
            .unwrap_or_default()
    };
    let checked = if entitlement.last_checked == mb_core::Timestamp::EPOCH {
        "never".to_owned()
    } else {
        words::when(entitlement.last_checked)
    };
    let (cloud_copy, cloud_tone) = cloud_copy_says(app, at);

    LicenceView {
        standing: standing.code().to_owned(),
        chip: standing.chip().to_owned(),
        tone: tone_for(standing).to_owned(),
        headline: words::licence_banner(&entitlement, day).unwrap_or_default(),
        has_licence: !matches!(standing, mb_license::Standing::NeverActivated),
        bound_elsewhere: matches!(standing, mb_license::Standing::BoundElsewhere),
        owner_name,
        owner_phone,
        shop_name: entitlement.shop_name.clone().unwrap_or_default(),
        plan_name: entitlement.plan_name.clone(),
        date_label: date_label.to_owned(),
        date,
        key,
        restaurant_code,
        phones_allowed: entitlement.limits.devices,
        tills_allowed: entitlement.limits.terminals,
        included: entitlement
            .features()
            .known()
            .iter()
            .map(|f| f.in_words().to_owned())
            .collect(),
        checked,
        still_held,
        clock_note,
        may_manage,
        cloud_copy,
        cloud_tone: cloud_tone.to_owned(),
    }
}
fn note(app: &App, what: mb_auth::audit::AuditAction, detail: &str) {
    let at = now();
    let who = app.sessions().current().map(|s| s.actor.staff_id.clone());
    let outcome = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).audit().append(
                    OUTLET,
                    &AuditEntry::new(at, today(at), who.clone(), what, "licence")
                        .about(detail.to_owned()),
                )
            })
            .map_err(|e| words::from_db(&e))
    });
    if let Err(e) = outcome {
        log_warn!("the licensing note '{what}' could not be filed: {e}");
    }
}

/// What every change of licence leads to: the sender may go again, the window hears the
/// banner, and the release shelf is read.
pub fn after_licence_change(app: &App) {
    app.update_sync(|s| {
        s.stopped = None;
        s.failures = 0;
        s.next_try_at = None;
    });
    app.sender_wakeup().wake();
    tell_the_window(app);
    crate::updates::check_now(app);
}

/// The window hears the banner and its tone.
fn tell_the_window(app: &App) {
    let at = now();
    let entitlement = app.entitlement();
    app.push(Pushed::Licence {
        says: words::licence_banner(&entitlement, today(at)).unwrap_or_default(),
        tone: tone_for(entitlement.standing).to_owned(),
    });
}

/// The standing again from the copy on disk — a date may have passed — and the window told.
pub fn re_decide_and_tell(app: &App) {
    let before = app.entitlement().standing;
    app.re_decide();
    if app.entitlement().standing != before {
        tell_the_window(app);
    }
}

pub fn account_on(app: &App) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    // Opening the screen is a reason to check, when the last check is old — off this thread,
    // so the screen draws now.
    let last = app.entitlement().last_checked;
    let age = now().millis().saturating_sub(last.millis());
    if app.with_licence(|l| l.key().is_some())
        && age > i64::try_from(STALE_ON_OPEN.as_millis()).unwrap_or(i64::MAX)
    {
        app.refresher_wakeup().wake();
    }
    Ok(view_on(app))
}

/// Which licence the counter should run on from now: a shop of the account that signed in
/// with `licence_shops`, or a key from the dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(tag = "by", rename_all = "camelCase")]
pub enum LicenceDoor {
    Shop {
        #[serde(rename = "restaurantId")]
        restaurant_id: String,
    },
    Key {
        key: String,
    },
}

/// The shops an account owns, for the owner to pick from before `change_licence`.
pub fn licence_shops_on(
    app: &App,
    email: String,
    password: String,
) -> UiResult<crate::firstrun::OwnerSignInView> {
    guard::require(app, Permission::LicenceManage)?;
    crate::firstrun::owner_shops_by_password(app, email, password)
}

/// Run this shop on another licence: the new one is taken, the old one released, the owner's
/// row renamed after the new licence's owner and given the PIN they typed, and they are signed
/// in with it. The menu, the tables, the bills and the staff stay as they are.
pub fn change_licence_on(
    app: &App,
    door: LicenceDoor,
    move_here: bool,
    new_pin: String,
) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    let at = now();
    // The PIN's shape first, so a bad PIN costs no round trip to the cloud.
    let Some(hashed) = crate::ipc::hashed_pin(Some(&new_pin))? else {
        return Err(UiError::new("auth.pin_shape", "Type the new PIN."));
    };
    let (key, named) = match door {
        LicenceDoor::Key { key } => {
            let key = key.trim().to_uppercase();
            if key.is_empty() {
                return Err(UiError::new(
                    "licence.blank",
                    "Paste the licence key from your magicbill.in dashboard, or sign in.",
                ));
            }
            (key, String::new())
        }
        LicenceDoor::Shop { restaurant_id } => {
            let Some(shop) = app.with_owner_sign_in(|held| {
                held.as_ref().and_then(|s| {
                    s.shops
                        .iter()
                        .find(|shop| shop.id == restaurant_id)
                        .cloned()
                })
            }) else {
                return Err(UiError::new("owner.sign_in", "Sign in first."));
            };
            let Some(key) = shop.key else {
                return Err(UiError::new(
                    "owner.no_licence",
                    format!(
                        "{} has no licence. Start one at magicbill.in first.",
                        shop.name
                    ),
                ));
            };
            (key, shop.owner_name)
        }
    };
    if app.with_licence(|l| l.key()).as_deref() == Some(key.as_str()) {
        return Err(UiError::new(
            "licence.same",
            "This computer already runs on that licence.",
        ));
    }

    let outcome = app.with_licensing(|licensing| {
        licensing.switch_to(
            &key,
            move_here,
            at,
            today(at),
            mb_license::deadline::DEADLINE,
        )
    });
    if let Err(e) = outcome {
        note(
            app,
            action::LICENCE_REFUSED,
            &format!("change: {}", e.code()),
        );
        return Err(crate::firstrun::licence_said(&e));
    }
    note(app, action::LICENCE_ACTIVATED, &key);
    // The cloud copy starts again under the new licence: the old cursor names the old shop.
    app.update_sync(|s| *s = crate::sync::SyncFile::default());
    after_licence_change(app);

    // The owner's row is whoever holds the licence now.
    let owner_name = app.with_licence(|l| {
        l.snapshot()
            .map(|s| s.licence.owner_name)
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(named)
    });
    let owner = crate::firstrun::owner_row_named(app, &owner_name, true)?;
    crate::ipc::write_pin(app, owner.id.as_str(), Some(&hashed), &owner.id, at)?;
    log_info!("{} took over this counter on another licence", owner.name);
    crate::ipc::admit(app, &owner, at)?;
    app.with_owner_sign_in(|held| *held = None);
    Ok(view_on(app))
}

/// Stop using the licence on this computer, so another computer can take it. The shop's data
/// stays, and billing carries on.
pub fn sign_out_on(app: &App) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    let at = now();
    let released = app
        .with_licensing(|licensing| licensing.deactivate(at, mb_license::deadline::DEADLINE))
        .map_err(|e| words::from_licence(&e))?;
    note(
        app,
        action::LICENCE_DEACTIVATED,
        if released {
            "released"
        } else {
            "queued — the server still holds the binding"
        },
    );
    app.update_sync(|s| *s = crate::sync::SyncFile::default());
    after_licence_change(app);
    Ok(view_on(app))
}

/// Bring the licence this computer holds back from the computer that took it.
pub fn bring_here_on(app: &App) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    let at = now();
    let Some(key) = app.with_licence(|l| l.key()) else {
        return Err(UiError::new(
            "licence.none",
            "This computer has no licence to bring back. Use Change licence.",
        ));
    };
    let outcome = app.with_licensing(|licensing| {
        licensing.transfer(&key, at, today(at), mb_license::deadline::DEADLINE)
    });
    match outcome {
        Ok(()) => {
            note(app, action::LICENCE_TRANSFERRED, &key);
            after_licence_change(app);
            Ok(view_on(app))
        }
        Err(e) => {
            note(
                app,
                action::LICENCE_REFUSED,
                &format!("transfer: {}", e.code()),
            );
            Err(words::from_licence(&e))
        }
    }
}

pub fn use_emergency_code_on(app: &App, code: String) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    let at = now();
    let outcome = app.with_licensing(|licensing| licensing.use_emergency_code(code.trim(), at));
    match outcome {
        Ok(until) => {
            note(app, action::LICENCE_EMERGENCY, &words::when(until));
            Ok(view_on(app))
        }
        Err(e) => {
            note(
                app,
                action::LICENCE_REFUSED,
                &format!("emergency: {}", e.code()),
            );
            Err(words::from_licence(&e))
        }
    }
}

/// Ask the cloud now, because somebody pressed the button. An answer the cloud did not give
/// is an error, never a "checked".
pub fn refresh_on(app: &App) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    if app.with_licence(|l| l.key().is_none()) {
        return Err(UiError::new(
            "licence.none",
            "This computer has no licence to check.",
        ));
    }
    if !refresh_now(app, mb_license::deadline::DEADLINE) {
        return Err(UiError::new(
            "cloud.unreachable",
            "We could not reach our server. Check the internet connection and try again. \
             Billing is not affected.",
        ));
    }
    Ok(view_on(app))
}

/// The check itself: the one metered call. True when the cloud answered.
pub fn refresh_now(app: &App, limit: Duration) -> bool {
    if app.with_licence(|l| l.key().is_none()) {
        return false;
    }
    let at = now();
    match app.with_licensing(|licensing| licensing.refresh(at, limit)) {
        Ok(()) => {
            after_licence_change(app);
            // Notices posted since the last pull ride the next push; when nothing is waiting to
            // go up, fetch them now so the bell is right today rather than tomorrow.
            let unread = app.with_licence(|l| l.extras().map_or(0, |e| e.unread_notices));
            if unread > 0 && app.has_shop() {
                match crate::sync::pull_once(app) {
                    Ok(_) => {}
                    Err(e) => log_warn!("the notices could not be fetched after the check: {e}"),
                }
            }
            true
        }
        Err(e) => {
            log_warn!("the licence could not be checked: {e}");
            // The standing may have moved (needs-checking, grace) without a fresh snapshot.
            app.re_decide();
            false
        }
    }
}

/// The daily check: at start-up with the short deadline, then every day, or sooner when the
/// cloud says the licence changed. Never on the thread that paints.
pub fn start_refresher(handle: &tauri::AppHandle) {
    use tauri::Manager as _;
    let handle = handle.clone();
    let spawned = std::thread::Builder::new()
        .name("mb-licence".to_owned())
        .spawn(move || {
            let Some(app) = handle.try_state::<App>() else {
                return;
            };
            // The first paint first.
            app.refresher_wakeup().wait_for(Duration::from_secs(1));
            let mut last_try_failed = !refresh_now(&app, mb_license::deadline::STARTUP_DEADLINE);
            loop {
                let Some(app) = handle.try_state::<App>() else {
                    return;
                };
                // No key yet, or the last try failed: look again in an hour. Otherwise the
                // rest of the day.
                let due_in = if last_try_failed || app.with_licence(|l| l.key().is_none()) {
                    RETRY_AFTER
                } else {
                    let age = now()
                        .millis()
                        .saturating_sub(app.entitlement().last_checked.millis());
                    let every = i64::try_from(CHECK_EVERY.as_millis()).unwrap_or(i64::MAX);
                    Duration::from_millis(u64::try_from(every.saturating_sub(age)).unwrap_or(0))
                };
                // Wake at least hourly so a plan that runs out at midnight is re-decided from
                // the copy on disk; the cloud is asked only when the day's check is due.
                let wait = due_in.min(RETRY_AFTER).max(Duration::from_secs(60));
                let woken = app.refresher_wakeup().wait_for(wait);
                let Some(app) = handle.try_state::<App>() else {
                    return;
                };
                if app.with_licence(|l| l.key().is_none()) {
                    last_try_failed = false;
                    continue;
                }
                if !woken && due_in > wait {
                    re_decide_and_tell(&app);
                    continue;
                }
                log_info!("checking the licence with the cloud");
                last_try_failed = !refresh_now(&app, mb_license::deadline::DEADLINE);
            }
        });
    if let Err(e) = spawned {
        log_warn!("the licence check could not be started: {e}");
    }
}

// The seats.

#[tauri::command]
pub fn account(app: tauri::State<'_, App>) -> UiResult<LicenceView> {
    account_on(&app)
}

#[tauri::command]
pub fn licence_shops(
    app: tauri::State<'_, App>,
    email: String,
    password: String,
) -> UiResult<crate::firstrun::OwnerSignInView> {
    licence_shops_on(&app, email, password)
}

#[tauri::command]
pub fn change_licence(
    app: tauri::State<'_, App>,
    door: LicenceDoor,
    move_here: bool,
    new_pin: String,
) -> UiResult<LicenceView> {
    change_licence_on(&app, door, move_here, new_pin)
}

#[tauri::command]
pub fn sign_out_licence(app: tauri::State<'_, App>) -> UiResult<LicenceView> {
    sign_out_on(&app)
}

#[tauri::command]
pub fn bring_licence_here(app: tauri::State<'_, App>) -> UiResult<LicenceView> {
    bring_here_on(&app)
}

#[tauri::command]
pub fn use_emergency_code(app: tauri::State<'_, App>, code: String) -> UiResult<LicenceView> {
    use_emergency_code_on(&app, code)
}

#[tauri::command]
pub fn refresh_licence(app: tauri::State<'_, App>) -> UiResult<LicenceView> {
    refresh_on(&app)
}
