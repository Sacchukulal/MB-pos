//! The licence, on the counter.

use std::sync::Arc;
use std::time::Duration;

use mb_auth::Permission;
use mb_auth::audit::{AuditEntry, action};
use mb_license::{Cloud, Feature, Licensing, MachineId};
use serde::Serialize;
use ts_rs::TS;

use crate::flows::{now, today};
use crate::state::{App, Pushed};
use crate::words::{self, UiResult};
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
    let dir = std::env::temp_dir().join(format!("mb-licence-blank-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(mb_license::LicenceFile::path(&dir));
    let machine = MachineId::for_tests("test-machine-0001");
    let cloud: Arc<dyn Cloud> = Arc::new(mb_license::cloud::Stub::active(&machine, today(now()), now()));
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
    pub shop_name: String,
    pub plan_name: String,
    /// "12 September", not a date field (2.10).
    pub renews_on: String,
    /// "Your plan renews on 12 September." Built here, shown as-is.
    pub renewal_sentence: String,
    pub registered_contact: String,
    /// This computer, for a support call: "4C4C4544".
    pub machine: String,
    /// "from Windows", "made by Magic Bill on this computer".
    pub machine_how: String,
    /// True when losing the config folder would lose the identity.
    pub machine_is_fragile: bool,
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
    pub is_activated: bool,
    /// What staff type on a phone to reach this shop. Empty until the cloud says.
    pub restaurant_code: String,
    /// The cloud copy, in one sentence.
    pub cloud_copy: String,
    /// `ok`, `warn` or `danger`, for the sentence above.
    pub cloud_tone: String,
    /// Where a trial starts. One sentence, no dialog.
    pub trial_sentence: String,
}

/// `ok`, `warn` or `danger`.
#[must_use]
pub const fn tone_for(standing: mb_license::Standing) -> &'static str {
    match standing {
        mb_license::Standing::Fine => "ok",
        mb_license::Standing::InGrace { .. }
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

/// The trial is the website's.
pub const TRIAL_SENTENCE: &str = "Start your free trial at magicbill.in, then enter the key here.";

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

/// Build the view. Never fails.
pub fn view_on(app: &App) -> LicenceView {
    let at = now();
    let day = today(at);
    let entitlement = app.entitlement();
    let standing = entitlement.standing;

    let (machine, how, fragile, still_held, clock_note, restaurant_code) =
        app.with_licence(|licensing| {
            let machine = licensing.machine().clone();
            let held = licensing
                .file()
                .pending_release
                .as_ref()
                .map(|_| {
                    "This computer has stopped using the licence, but we could not \
                     tell our server. The licence is still held — we will keep \
                     trying, and you can also release it from magicbill.in."
                        .to_owned()
                })
                .unwrap_or_default();
            let clock = match licensing.clock_says(at) {
                mb_license::ClockSays::Fine => String::new(),
                mb_license::ClockSays::WentBackwards { .. } => {
                    "This computer's clock is behind. Nothing is blocked, but we \
                     will need to check your licence online soon — it is worth \
                     checking the date and time."
                        .to_owned()
                }
            };
            let code = licensing
                .snapshot()
                .and_then(|s| s.licence.short_code)
                .unwrap_or_default();
            (
                machine.short(),
                machine.how().in_words().to_owned(),
                machine.how().is_fragile(),
                held,
                clock,
                code,
            )
        });

    let renews_on = entitlement
        .renews_on
        .map(|on| words::day(on, day))
        .unwrap_or_default();
    let renewal_sentence = match (entitlement.renews_on, standing) {
        (Some(on), mb_license::Standing::Fine) => {
            format!("Your plan renews on {}.", words::day(on, day))
        }
        _ => String::new(),
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
        shop_name: entitlement.shop_name.clone().unwrap_or_default(),
        plan_name: entitlement.plan_name.clone(),
        renews_on,
        renewal_sentence,
        registered_contact: String::new(),
        machine,
        machine_how: how,
        machine_is_fragile: fragile,
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
        may_manage: guard::require(app, Permission::LicenceManage).is_ok(),
        is_activated: !matches!(standing, mb_license::Standing::NeverActivated),
        restaurant_code,
        cloud_copy,
        cloud_tone: cloud_tone.to_owned(),
        trial_sentence: TRIAL_SENTENCE.to_owned(),
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
    let at = now();
    let entitlement = app.entitlement();
    app.push(Pushed::Licence {
        says: words::licence_banner(&entitlement, today(at)).unwrap_or_default(),
        tone: tone_for(entitlement.standing).to_owned(),
    });
    crate::updates::check_now(app);
}

pub fn account_on(app: &App) -> UiResult<LicenceView> {
    guard::require(app, Permission::ReportsView)?;
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

pub fn activate_on(app: &App, key: String) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    let at = now();
    let outcome = app.with_licensing(|licensing| {
        licensing.activate(key.trim(), at, mb_license::deadline::DEADLINE)
    });
    match outcome {
        Ok(()) => {
            note(app, action::LICENCE_ACTIVATED, key.trim());
            after_licence_change(app);
            Ok(view_on(app))
        }
        Err(e) => {
            note(
                app,
                action::LICENCE_REFUSED,
                &format!("activate: {}", e.code()),
            );
            Err(words::from_licence(&e))
        }
    }
}

pub fn deactivate_on(app: &App) -> UiResult<LicenceView> {
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
    after_licence_change(app);
    Ok(view_on(app))
}

pub fn transfer_here_on(app: &App, key: String) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    let at = now();
    let outcome = app.with_licensing(|licensing| {
        licensing.transfer(key.trim(), at, today(at), mb_license::deadline::DEADLINE)
    });
    match outcome {
        Ok(()) => {
            note(app, action::LICENCE_TRANSFERRED, key.trim());
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

/// Ask the cloud now, because somebody pressed the button.
pub fn refresh_on(app: &App) -> UiResult<LicenceView> {
    guard::require(app, Permission::ReportsView)?;
    refresh_now(app, mb_license::deadline::DEADLINE);
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
                let wait = if last_try_failed || app.with_licence(|l| l.key().is_none()) {
                    RETRY_AFTER
                } else {
                    let age = now().millis().saturating_sub(app.entitlement().last_checked.millis());
                    let every = i64::try_from(CHECK_EVERY.as_millis()).unwrap_or(i64::MAX);
                    Duration::from_millis(u64::try_from(every.saturating_sub(age)).unwrap_or(0))
                };
                app.refresher_wakeup().wait_for(wait.max(Duration::from_secs(60)));
                let Some(app) = handle.try_state::<App>() else {
                    return;
                };
                if app.with_licence(|l| l.key().is_none()) {
                    last_try_failed = false;
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
pub fn activate(app: tauri::State<'_, App>, key: String) -> UiResult<LicenceView> {
    activate_on(&app, key)
}

#[tauri::command]
pub fn deactivate(app: tauri::State<'_, App>) -> UiResult<LicenceView> {
    deactivate_on(&app)
}

#[tauri::command]
pub fn transfer_here(app: tauri::State<'_, App>, key: String) -> UiResult<LicenceView> {
    transfer_here_on(&app, key)
}

#[tauri::command]
pub fn use_emergency_code(app: tauri::State<'_, App>, code: String) -> UiResult<LicenceView> {
    use_emergency_code_on(&app, code)
}

#[tauri::command]
pub fn refresh_licence(app: tauri::State<'_, App>) -> UiResult<LicenceView> {
    refresh_on(&app)
}
