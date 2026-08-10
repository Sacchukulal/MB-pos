//! **The licence, on the counter** — P21.
//!
//! `crates/mb-license` decides; this file wires the decision to the shop, to
//! the audit trail and to the screen.
//!
//! # The one thing to read before changing anything here
//!
//! > **PERFORMANCE §2.2:** *"Nothing in this table may ever be blocked by a
//! > report, a sync, a print job, **a licence check** or a backup. If any of
//! > those can delay any row here, the architecture is wrong, not the number."*
//!
//! So the entitlement is **decided on a timer and held** ([`App::entitlement`]),
//! and every gate in this file reads that held value. Nothing on the billing
//! path calls anything in this module, and
//! `the_billing_path_does_not_ask_about_the_licence` reads `billing.rs`,
//! `flows.rs` and `orders.rs` and proves it.
//!
//! # Where the gate is, and where it is not
//!
//! [`gate`] is called at the top of a command body, next to `guard::require`.
//! **Hiding a rail item is a courtesy; this is the control** — the same
//! sentence `guard` opens with, for the same reason, and `T10` calls every
//! gated command directly with a not-entitled entitlement.
//!
//! What is deliberately NOT gated: billing, printing, the local backup, and
//! **the day close**. The first three cannot be — `mb_license::Feature` has no
//! variant for them (D86). The day close could be and is not, because closing
//! the day is how a shop reconciles the cash in its drawer, and a shop locked
//! out of that at 11 pm has money it cannot account for. See
//! `Feature::REPORTS_DOES_NOT_MEAN_THE_DAY_CLOSE`.

use std::sync::Arc;

use mb_auth::Permission;
use mb_auth::audit::{AuditEntry, action};
use mb_license::{Cloud, Feature, Licensing, MachineId};
use serde::Serialize;
use ts_rs::TS;

use crate::flows::{now, today};
use crate::state::App;
use crate::words::{self, UiResult};
use crate::{guard, log_warn};

/// The one outlet this counter is. Same constant every other module uses.
const OUTLET: &str = "outlet_default";

/// **Build the licensing subsystem.**
///
/// Called once, from `App::new`. It touches the disk (the machine id and
/// `licence.json`) and **never the network** — budget S1 is 3.0 s to a usable
/// billing screen on an HDD, and a licence check on that path would be a
/// licence check on the path PERFORMANCE §2.2 forbids it from.
#[must_use]
pub fn start() -> Licensing {
    let dir = crate::config::AppConfig::directory();
    let machine = MachineId::of(&dir);
    Licensing::new(dir, machine, cloud(), env!("CARGO_PKG_VERSION"))
}

/// The same thing on a scratch folder, so a test never reads — or writes — the
/// licence belonging to whoever is running it. See `App::new`.
#[cfg(test)]
#[must_use]
pub fn for_tests() -> Licensing {
    let dir = std::env::temp_dir().join(format!("mb-licence-blank-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(mb_license::LicenceFile::path(&dir));
    Licensing::new(dir, MachineId::for_tests("test-machine-0001"), cloud(), "test")
}

/// **The cloud, which does not exist yet.**
///
/// Phase 8 (P32-P34) builds the real one. Until then this is `mb-license`'s own
/// stub, signing with the development key — which means a counter today can be
/// activated, transferred, deactivated and emergency-unlocked end to end, and
/// the only thing that changes when the cloud lands is this function.
///
/// **It is not a mock hidden behind a `cfg(test)`.** The stub is what ships
/// today, so the paths this session built are the paths that run, and P34
/// replaces one line rather than discovering that nothing was ever wired up.
fn cloud() -> Arc<dyn Cloud> {
    Arc::new(mb_license::cloud::Stub::active(
        &MachineId::of(&crate::config::AppConfig::directory()),
        today(now()),
        now(),
    ))
}

// ---------------------------------------------------------------------------
// The gate.
// ---------------------------------------------------------------------------

/// **Refuse a feature this shop is not entitled to.**
///
/// Reads the held entitlement — no network, no database, no shop lock.
///
/// # Errors
///
/// The refusal, as the sentence `words::licence_refusal` writes.
pub fn gate(app: &App, feature: Feature) -> UiResult<()> {
    let entitlement = app.entitlement();
    match entitlement.may(feature) {
        Ok(()) => Ok(()),
        Err(refusal) => Err(words::licence_refusal(&refusal, &entitlement, today(now()))),
    }
}

/// **Every command this session puts behind the gate, and which feature.**
///
/// A table rather than a scattering of calls, for the reason
/// `guard::COMMAND_ACCESS` is a table: there will be a hundred more commands by
/// P30 and "everybody remembers to add the gate" is D40's definition of a rule
/// that erodes. `every_gated_command_is_refused_when_not_entitled` walks this
/// list and calls each one.
pub const GATED: &[(&str, Feature)] = &[
    // The reports screen and its exports.
    ("report_list", Feature::Reports),
    ("report", Feature::Reports),
    ("report_csv", Feature::Reports),
    ("report_pdf", Feature::Reports),
    ("dashboard", Feature::Reports),
    // Phones. P19's pairing desk and P20's intents.
    ("open_pairing", Feature::MobileOrdering),
    ("allow_device", Feature::MobileOrdering),
];

// ---------------------------------------------------------------------------
// What the screen sees.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct LicenceView {
    /// `fine` · `grace` · `expired` · `suspended` · `revoked` · `cancelled` ·
    /// `never-activated` · `trial-ended` · `needs-checking` ·
    /// `bound-elsewhere` · `emergency`.
    pub standing: String,
    /// The chip: "Active", "Grace period", "Not activated".
    pub chip: String,
    /// `ok`, `warn` or `danger` — and the words say it too (UI_GUIDELINES §2:
    /// colour is never the only carrier).
    pub tone: String,
    /// The sentence. Empty when there is nothing to say.
    pub headline: String,
    pub shop_name: String,
    pub plan_name: String,
    /// **"12 September", not a date field** (2.10).
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
    /// **BACKEND-C5's sentence**, when a deactivate could not reach the server.
    pub still_held: String,
    /// D90 — the clock went backwards.
    pub clock_note: String,
    /// Whether this person may press anything on this screen.
    pub may_manage: bool,
    pub is_activated: bool,
}

/// `ok`, `warn` or `danger`. Read by this screen and by `app_status`'s banner,
/// so the rail and the panel cannot disagree about how serious something is.
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

/// Build the view. **Never fails** — a licence screen that cannot draw is a
/// screen an owner cannot use to fix their licence.
pub fn view_on(app: &App) -> LicenceView {
    let at = now();
    let day = today(at);
    let entitlement = app.entitlement();
    let standing = entitlement.standing;

    let (machine, how, fragile, still_held, clock_note) = app.with_licence(|licensing| {
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
        (
            machine.short(),
            machine.how().in_words().to_owned(),
            machine.how().is_fragile(),
            held,
            clock,
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
    }
}

// ---------------------------------------------------------------------------
// The bodies. D46: each takes `&App`, so a test can drive a SEQUENCE.
// ---------------------------------------------------------------------------

/// Write one licensing row into the audit trail.
///
/// **Best effort, and deliberately so.** R11 says an audit row is written in
/// the same transaction as the thing it records — and the thing recorded here
/// happened on another company's server and in a file beside the config, so
/// there is no transaction to be in. A shop with no database yet (a first run,
/// which is exactly when somebody activates) has nowhere to write it at all.
/// Failing the activation because the note could not be filed would be the
/// tail wagging the dog.
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

pub fn account_on(app: &App) -> UiResult<LicenceView> {
    guard::require(app, Permission::ReportsView)?;
    Ok(view_on(app))
}

pub fn activate_on(app: &App, key: String, proof: String) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    let at = now();
    let outcome = app.with_licensing(|licensing| {
        licensing.activate(key.trim(), proof.trim(), at, mb_license::deadline::DEADLINE)
    });
    match outcome {
        Ok(()) => {
            note(app, action::LICENCE_ACTIVATED, key.trim());
            Ok(view_on(app))
        }
        Err(e) => {
            note(app, action::LICENCE_REFUSED, &format!("activate: {}", e.code()));
            Err(words::from_licence(&e))
        }
    }
}

pub fn start_trial_on(app: &App, contact: String) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    let at = now();
    let outcome = app.with_licensing(|licensing| {
        licensing.start_trial(contact.trim(), at, mb_license::deadline::DEADLINE)
    });
    match outcome {
        Ok(()) => {
            note(app, action::LICENCE_ACTIVATED, "trial");
            Ok(view_on(app))
        }
        Err(e) => Err(words::from_licence(&e)),
    }
}

/// **BACKEND-C5.** The local side always happens; the server side is attempted,
/// and when it cannot be reached the screen is told the licence is still held.
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
    Ok(view_on(app))
}

pub fn transfer_here_on(app: &App, key: String, proof: String) -> UiResult<LicenceView> {
    guard::require(app, Permission::LicenceManage)?;
    let at = now();
    let outcome = app.with_licensing(|licensing| {
        licensing.transfer(
            key.trim(),
            proof.trim(),
            at,
            today(at),
            mb_license::deadline::DEADLINE,
        )
    });
    match outcome {
        Ok(()) => {
            note(app, action::LICENCE_TRANSFERRED, key.trim());
            Ok(view_on(app))
        }
        Err(e) => {
            note(app, action::LICENCE_REFUSED, &format!("transfer: {}", e.code()));
            Err(words::from_licence(&e))
        }
    }
}

/// POS-A4's offline half. Audited with the person who typed it.
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
            note(app, action::LICENCE_REFUSED, &format!("emergency: {}", e.code()));
            Err(words::from_licence(&e))
        }
    }
}

/// Ask the cloud now, because somebody pressed the button.
///
/// **Not an error when the cloud is unreachable.** The cached answer is still
/// good and the screen already says when it was last checked; turning "we could
/// not reach the server" into a red dialog would teach an owner that the button
/// is broken.
pub fn refresh_on(app: &App) -> UiResult<LicenceView> {
    guard::require(app, Permission::ReportsView)?;
    let at = now();
    if let Err(e) =
        app.with_licensing(|licensing| licensing.refresh(at, mb_license::deadline::DEADLINE))
    {
        log_warn!("the licence could not be checked: {e}");
    }
    Ok(view_on(app))
}

/// The background check. Called from `main` after the window is up, never
/// before — S1 is 3.0 s to a usable billing screen and this is not on that
/// path.
pub fn refresh_quietly(app: &App) {
    let at = now();
    let outcome = app.with_licensing(|licensing| {
        let _ = licensing.tick(at);
        licensing.refresh(at, mb_license::deadline::DEADLINE)
    });
    if let Err(e) = outcome {
        log_warn!("the background licence check did not complete: {e}");
    }
}

// ---------------------------------------------------------------------------
// The seats.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn account(app: tauri::State<'_, App>) -> UiResult<LicenceView> {
    account_on(&app)
}

#[tauri::command]
pub fn activate(app: tauri::State<'_, App>, key: String, proof: String) -> UiResult<LicenceView> {
    activate_on(&app, key, proof)
}

#[tauri::command]
pub fn start_trial(app: tauri::State<'_, App>, contact: String) -> UiResult<LicenceView> {
    start_trial_on(&app, contact)
}

#[tauri::command]
pub fn deactivate(app: tauri::State<'_, App>) -> UiResult<LicenceView> {
    deactivate_on(&app)
}

#[tauri::command]
pub fn transfer_here(
    app: tauri::State<'_, App>,
    key: String,
    proof: String,
) -> UiResult<LicenceView> {
    transfer_here_on(&app, key, proof)
}

#[tauri::command]
pub fn use_emergency_code(app: tauri::State<'_, App>, code: String) -> UiResult<LicenceView> {
    use_emergency_code_on(&app, code)
}

#[tauri::command]
pub fn refresh_licence(app: tauri::State<'_, App>) -> UiResult<LicenceView> {
    refresh_on(&app)
}
