//! **A shop must be able to go back** — audit E9, and it is P22's whole point.
//!
//! > *"The update is all-or-nothing with no way back. If a release breaks
//! > billing at 8 pm on a Saturday, the shop is stuck."*
//!
//! And its twin, which is the same failure from the other end:
//!
//! > **I1:** *"The updater's 'you are on the latest version' toast and the
//! > update snackbar can both be dismissed and forgotten — a shop can sit on an
//! > old, buggy version for months (this already happened: one counter was left
//! > on 1.4.0)."*
//!
//! One says do not push an update at somebody; the other says do not let them
//! forget it exists. Both are answered by [`Offer`], which respects the shop's
//! closing time and comes back tomorrow.
//!
//! # What this session builds and what it does not
//!
//! There is no release server yet. [`Releases`] is the seam — the same shape
//! P21 gave `Cloud`, and for the same reason: a stub that ships means the paths
//! built here are the paths that run, rather than being wired up for the first
//! time by whoever builds the server. **No real HTTP in P22.**
//!
//! What IS real: the version arithmetic, the two verifications, the kept
//! installer, the start counter, the rollback, and the rollout decision.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::health::HealthRow;

// ---------------------------------------------------------------------------
// D96 — a version is a number.
// ---------------------------------------------------------------------------

/// **A version, as three numbers.**
///
/// > **ANDROID-G4:** *"Version comparison is by name, and installing a debug
/// > build with a real version number permanently strands that phone — it will
/// > be told it is already up to date. This already cost you version 2.4.3."*
///
/// `Ord` is derived, which is the whole fix: `1.10.0 > 1.9.0` because ten is
/// more than nine, and no string comparison anywhere in this product ever gets
/// the chance to disagree.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// What this build is.
    #[must_use]
    pub fn running() -> Version {
        env!("CARGO_PKG_VERSION")
            .parse()
            .unwrap_or(Version::new(0, 0, 0))
    }
}

impl std::str::FromStr for Version {
    type Err = ();

    fn from_str(text: &str) -> Result<Version, ()> {
        // Anything after a `-` or `+` is a pre-release or build tag, and this
        // product does not use them. Dropping rather than rejecting means a
        // manifest from a future release that does use them still parses to
        // something orderable, instead of the counter deciding it has no idea
        // what version is available.
        let core = text
            .split(['-', '+'])
            .next()
            .unwrap_or(text);
        let mut parts = core.split('.');
        let major = parts.next().and_then(|p| p.parse().ok()).ok_or(())?;
        let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        Ok(Version {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// **Is this build a real release?**
///
/// G4's other half. A development build carries a real version number and must
/// never be told it is up to date, because that is the state that stranded a
/// phone permanently: the counter believes it has the newest version, so it
/// never offers the real one.
#[must_use]
pub const fn is_a_release_build() -> bool {
    !cfg!(debug_assertions)
}

// ---------------------------------------------------------------------------
// The manifest.
// ---------------------------------------------------------------------------

/// What a release says about itself. Signed; see [`check`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: Version,
    /// **Short, and in plain words.** Long release notes buried v1's install
    /// button; this is shown in a paragraph, not a scrolling pane.
    pub notes: String,
    pub url: String,
    /// Lowercase hex. **ANDROID-G2** — *"the downloaded update is never
    /// verified against a published fingerprint"*.
    pub sha256: String,
    pub rollout: Rollout,
}

/// Who gets it first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rollout {
    /// 0-100.
    pub percent: u32,
    /// Named shops, whatever the percentage says. For a shop that has reported
    /// the bug this release fixes.
    #[serde(default)]
    pub shops: Vec<String>,
}

impl Default for Rollout {
    fn default() -> Self {
        Rollout {
            percent: 100,
            shops: Vec::new(),
        }
    }
}

impl Rollout {
    /// **D101 — the decision is STABLE for a given machine.**
    ///
    /// A dice rolled on every check flaps: the counter is offered 2.5.0 on
    /// Monday, told it is up to date on Tuesday, offered it again on Wednesday.
    /// Hashing the machine id means a shop that is in the first 5% stays in the
    /// first 5%, and a staged rollout can actually be watched.
    #[must_use]
    pub fn includes(&self, machine: &str, shop: &str) -> bool {
        if self.shops.iter().any(|s| s == shop) {
            return true;
        }
        if self.percent >= 100 {
            return true;
        }
        if self.percent == 0 {
            return false;
        }
        // FNV-1a over the machine id: tiny, stable across runs and across
        // versions of the standard library — which `DefaultHasher` explicitly
        // is not, and a rollout that changes when Rust updates is the flapping
        // this exists to avoid.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in machine.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        u32::try_from(hash % 100).unwrap_or(0) < self.percent
    }
}

/// The release channel. No HTTP in P22 — see the module note.
pub trait Releases: Send + Sync + 'static {
    /// The signed manifest, as `(json, signature_base64)`.
    ///
    /// # Errors
    ///
    /// A sentence, when the channel cannot be reached.
    fn latest(&self) -> Result<(String, String), String>;

    /// The package itself.
    ///
    /// # Errors
    ///
    /// A sentence.
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// Keys this build accepts a manifest from. P34/CI mints the real one; until
/// then the development key `mb-license` already carries is reused, because a
/// second development key is a second thing to remember to delete.
#[must_use]
pub fn trusted_keys() -> Vec<Vec<u8>> {
    mb_license::snapshot::trusted_keys()
}

/// Errors a person can be told about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The signature did not check out. **We did not publish this.**
    NotOurs,
    /// The download does not match the manifest's hash.
    Damaged,
    /// The manifest could not be read.
    Unreadable,
}

impl Refused {
    #[must_use]
    pub const fn says(self) -> &'static str {
        match self {
            Refused::NotOurs => {
                "That update was not published by us, so it has not been \
                 installed. Nothing on this computer has changed."
            }
            Refused::Damaged => {
                "The update did not download properly, so it has not been \
                 installed. Nothing on this computer has changed. Try again."
            }
            Refused::Unreadable => {
                "We could not read what the update server sent. Nothing on this \
                 computer has changed."
            }
        }
    }
}

/// **Check the manifest's signature and hand back what it says.**
///
/// # Errors
///
/// [`Refused`] — and nothing on disk has been touched, which is the point of
/// doing this first.
pub fn check(json: &str, signature: &str) -> Result<Manifest, Refused> {
    mb_license::verify_detached(json.as_bytes(), signature, &trusted_keys())
        .map_err(|_| Refused::NotOurs)?;
    serde_json::from_str(json).map_err(|_| Refused::Unreadable)
}

/// **And the package's hash, which answers a different question.**
///
/// The signature says *we published this release*. The hash says *this file is
/// the release, all of it*. A truncated download passes the first and fails the
/// second, which is exactly ANDROID-G2.
///
/// # Errors
///
/// [`Refused::Damaged`].
pub fn check_package(bytes: &[u8], manifest: &Manifest) -> Result<(), Refused> {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let hex: String = digest
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if hex.eq_ignore_ascii_case(&manifest.sha256) {
        Ok(())
    } else {
        Err(Refused::Damaged)
    }
}

// ---------------------------------------------------------------------------
// D98 — two failed starts roll back, and the counter counts them.
// ---------------------------------------------------------------------------

/// What is on disk beside the config, in `updates\starts.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Starts {
    /// The version these attempts belong to. A new version resets the count —
    /// otherwise a shop that had one bad start a year ago would roll back the
    /// first time anything else went wrong.
    pub version: String,
    pub attempts: u32,
}

/// **Two.** Not one, not three.
///
/// One is a fluke: a Windows update rebooting mid-start, a power cut, somebody
/// double-clicking twice. Three is a Saturday night with a queue at the till.
pub const ATTEMPTS_BEFORE_ROLLBACK: u32 = 2;

impl Starts {
    #[must_use]
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("updates").join("starts.json")
    }

    #[must_use]
    pub fn load(dir: &Path) -> Starts {
        std::fs::read_to_string(Starts::path(dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save(&self, dir: &Path) {
        let path = Starts::path(dir);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Called before the window opens.
    pub fn attempted(dir: &Path, version: Version) {
        let mut starts = Starts::load(dir);
        if starts.version != version.to_string() {
            starts = Starts {
                version: version.to_string(),
                attempts: 0,
            };
        }
        starts.attempts = starts.attempts.saturating_add(1);
        starts.save(dir);
    }

    /// **Called once the counter is genuinely up**, which means the window is
    /// visible AND the shop is open — or the first-run state was reached, which
    /// is a working counter with no shop yet.
    ///
    /// Clearing it any earlier would mean an app that starts, paints, and then
    /// falls over on the first bill never rolls back.
    pub fn healthy(dir: &Path) {
        let path = Starts::path(dir);
        let _ = std::fs::remove_file(path);
    }

    /// Should this start roll back instead?
    #[must_use]
    pub fn should_roll_back(dir: &Path, version: Version) -> bool {
        let starts = Starts::load(dir);
        starts.version == version.to_string() && starts.attempts > ATTEMPTS_BEFORE_ROLLBACK
    }
}

/// The installer kept for going back, and what version it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Previous {
    pub version: Version,
    pub installer: PathBuf,
}

impl Previous {
    #[must_use]
    pub fn folder(dir: &Path) -> PathBuf {
        dir.join("updates").join("previous")
    }

    /// Where the installer for the version **currently running** is kept, if we
    /// were the ones who installed it.
    ///
    /// **This is the piece that makes rollback possible at all**, and it is why
    /// the first update after a hand-installed build has nothing to go back to:
    /// we can only keep an installer we downloaded. `going_back_says` puts that
    /// in words rather than greying a button out.
    #[must_use]
    pub fn installed_folder(dir: &Path) -> PathBuf {
        dir.join("updates").join("installed")
    }

    /// What is kept, if anything.
    #[must_use]
    pub fn load(dir: &Path) -> Option<Previous> {
        let text = std::fs::read_to_string(Previous::folder(dir).join("previous.json")).ok()?;
        let previous: Previous = serde_json::from_str(&text).ok()?;
        previous.installer.exists().then_some(previous)
    }

    fn write(&self, folder: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(folder)?;
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(folder.join("previous.json"), text)
    }
}

/// **Take a verified package and put it where it can be run — keeping what is
/// there now.**
///
/// The order is the whole of E9: the package is checked, the version currently
/// installed is moved aside to `previous/`, and only then is the new one
/// written. A failure at any step leaves the shop on what it already had.
///
/// Returns the installer to run. **It does not run it** — launching a process
/// that replaces this one is `main`'s business and Phase 8's, and separating
/// them is what lets this be tested.
///
/// # Errors
///
/// [`Refused`] when the package does not match its manifest, which is checked
/// **before anything on disk is touched**.
pub fn keep_and_place(
    dir: &Path,
    manifest: &Manifest,
    package: &[u8],
) -> Result<PathBuf, Refused> {
    check_package(package, manifest)?;

    let installed = Previous::installed_folder(dir);
    let previous = Previous::folder(dir);

    // 1. Whatever we installed last time becomes the way back.
    if let Ok(text) = std::fs::read_to_string(installed.join("installed.json"))
        && let Ok(current) = serde_json::from_str::<Previous>(&text)
        && current.installer.exists()
    {
        let _ = std::fs::create_dir_all(&previous);
        let landing = previous.join(
            current
                .installer
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("previous-setup.exe")),
        );
        if std::fs::rename(&current.installer, &landing).is_ok() {
            let _ = Previous {
                version: current.version,
                installer: landing,
            }
            .write(&previous);
        }
    }

    // 2. The new one goes down.
    let _ = std::fs::create_dir_all(&installed);
    let name = format!("magic-bill-{}-setup.exe", manifest.version);
    let path = installed.join(&name);
    std::fs::write(&path, package).map_err(|_| Refused::Damaged)?;
    let _ = Previous {
        version: manifest.version,
        installer: path.clone(),
    }
    .write(&installed);
    // `write` puts it in `previous.json`; the installed folder wants its own
    // name so the two cannot be confused when somebody looks in the folder.
    let _ = std::fs::rename(
        installed.join("previous.json"),
        installed.join("installed.json"),
    );

    Ok(path)
}

/// **A cloud that is not there yet, and a stub that ships.**
///
/// Same reasoning as P21's `cloud::Stub`: what ships is what was tested, so
/// Phase 8 replaces one constructor rather than wiring these paths up for the
/// first time. It answers "no update" by default, which is the honest state of
/// a product with no release server.
pub struct NoReleaseServerYet;

impl Releases for NoReleaseServerYet {
    fn latest(&self) -> Result<(String, String), String> {
        Err("there is no release server yet — Phase 8 builds it".to_owned())
    }

    fn fetch(&self, _url: &str) -> Result<Vec<u8>, String> {
        Err("there is no release server yet — Phase 8 builds it".to_owned())
    }
}

/// **The sentence for "go back", including the case where there is nothing to
/// go back to.**
///
/// A greyed-out button with no explanation is the thing a shopkeeper phones
/// about at eight on a Saturday, which is the exact moment this feature is
/// supposed to help.
#[must_use]
pub fn going_back_says(previous: Option<&Previous>) -> String {
    match previous {
        Some(p) => format!(
            "Go back to version {}. Magic Bill will close and reinstall the \
             older version — your shop's data is not touched.",
            p.version
        ),
        None => "There is no earlier version on this computer to go back to — \
                 this is the first version installed here."
            .to_owned(),
    }
}

// ---------------------------------------------------------------------------
// D99 — never by surprise, never forgotten.
// ---------------------------------------------------------------------------

/// What the counter knows about updates right now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    /// The version on offer, if any.
    pub available: Option<String>,
    pub notes: String,
    /// The business day it was last dismissed on, as `YYYY-MM-DD`. A dismissal
    /// lasts until the next day and no longer — I1.
    pub dismissed_on: Option<String>,
    /// How many days this counter has been on its current version.
    pub days_on_this_version: u32,
    pub running: String,
    pub is_dev_build: bool,
}

/// **How long is too long on one version.** I1's counter sat on 1.4.0 for
/// months; thirty days is a release cycle and a half.
pub const STALE_AFTER_DAYS: u32 = 30;

/// The health panel's row. Everything I1 asks for is here: an available update
/// is always visible, and an old version says so in words.
#[must_use]
pub fn health_row(state: &UpdateState) -> HealthRow {
    if state.is_dev_build {
        // G4: a development build is never told it is up to date.
        return HealthRow::warn(
            "update",
            "Version",
            format!(
                "This is a development build ({}), not a released one. It will \
                 not be updated — install a release build before using this \
                 counter in a shop.",
                state.running
            ),
        );
    }
    if let Some(available) = &state.available {
        return HealthRow::warn(
            "update",
            "Version",
            format!(
                "Version {available} is ready to install — you are on {}. \
                 Open Settings and install it after closing.",
                state.running
            ),
        );
    }
    if state.days_on_this_version > STALE_AFTER_DAYS {
        return HealthRow::warn(
            "update",
            "Version",
            format!(
                "You have been on version {} for {}. Check for an update in \
                 Settings — a counter left on an old version misses fixes.",
                state.running,
                crate::words::count(i64::from(state.days_on_this_version), "day", "days"),
            ),
        );
    }
    HealthRow::ok("update", "Version", format!("Version {}, up to date.", state.running))
}

// ---------------------------------------------------------------------------
// The bodies, and the seats. D46: each takes `&App`.
// ---------------------------------------------------------------------------

/// **Is a dismissal still standing?**
///
/// I1's whole content: a dismissal lasts until the shop's next business day and
/// no longer. The **business** day, not midnight — a shop that closes at 1 a.m.
/// dismissing an update at half past midnight has not dismissed it for four
/// minutes.
#[must_use]
pub fn is_dismissed(state: &UpdateState, today: mb_core::BusinessDay) -> bool {
    state
        .dismissed_on
        .as_deref()
        .is_some_and(|on| on == today.to_string())
}

/// What the shell shows, if anything.
#[must_use]
pub fn offer(state: &UpdateState, today: mb_core::BusinessDay) -> Option<String> {
    let available = state.available.as_ref()?;
    if is_dismissed(state, today) {
        return None;
    }
    Some(format!(
        "Version {available} is ready to install. It will not interrupt you — \
         install it after closing."
    ))
}

pub fn look_for_one_on(app: &crate::state::App) -> crate::words::UiResult<UpdateState> {
    crate::guard::require(app, mb_auth::Permission::ReportsView)?;
    let mut state = app.updates();

    // Everything below needs a release server. Until Phase 8 there is none, and
    // saying so beats pretending to have looked.
    match app.releases().latest() {
        Ok((json, signature)) => match check(&json, &signature) {
            Ok(manifest) => {
                let machine = app.with_licence(|l| l.machine().value().to_owned());
                let shop = app.entitlement().shop_name.unwrap_or_default();
                let newer = manifest.version > Version::running();
                let mine = manifest.rollout.includes(&machine, &shop);
                if newer && mine {
                    state.available = Some(manifest.version.to_string());
                    state.notes = manifest.notes;
                    // A NEW version clears an old dismissal: the thing that was
                    // waved away is not the thing being offered now.
                    state.dismissed_on = None;
                } else {
                    state.available = None;
                }
            }
            Err(refused) => {
                crate::log_warn!("an update manifest was refused: {}", refused.says());
                state.available = None;
            }
        },
        Err(why) => crate::log_info!("no update check: {why}"),
    }

    app.set_updates(state.clone());
    Ok(state)
}

/// **I1's fix, and the reason it is a business day.**
pub fn dismiss_on(app: &crate::state::App) -> crate::words::UiResult<UpdateState> {
    crate::guard::require(app, mb_auth::Permission::SettingsStore)?;
    let mut state = app.updates();
    state.dismissed_on = Some(crate::flows::today(crate::flows::now()).to_string());
    app.set_updates(state.clone());
    Ok(state)
}

/// Go back to the version before this one.
///
/// # Errors
///
/// When there is nothing to go back to — and the message says so in words
/// rather than the button being greyed out with no explanation.
pub fn go_back_on(app: &crate::state::App) -> crate::words::UiResult<String> {
    crate::guard::require(app, mb_auth::Permission::SettingsStore)?;
    let dir = crate::config::AppConfig::directory();
    let Some(previous) = Previous::load(&dir) else {
        return Err(crate::words::UiError::new(
            "update.no_previous",
            going_back_says(None),
        ));
    };
    crate::log_warn!(
        "going back to version {} — the installer is {}",
        previous.version,
        previous.installer.display()
    );
    // **Running it is deliberately not done here.** Launching a process that
    // replaces this one is `main`'s business; this returns the path so the
    // screen can confirm, and so this is testable.
    Ok(going_back_says(Some(&previous)))
}

#[tauri::command]
pub fn look_for_an_update(app: tauri::State<'_, crate::state::App>) -> crate::words::UiResult<UpdateState> {
    look_for_one_on(&app)
}

#[tauri::command]
pub fn dismiss_update(app: tauri::State<'_, crate::state::App>) -> crate::words::UiResult<UpdateState> {
    dismiss_on(&app)
}

#[tauri::command]
pub fn go_back_a_version(app: tauri::State<'_, crate::state::App>) -> crate::words::UiResult<String> {
    go_back_on(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **T4, and it is ANDROID-G4 by name.**
    #[test]
    fn versions_compare_as_numbers_and_never_as_names() {
        let v = |s: &str| s.parse::<Version>().expect("parses");
        // The comparison a string sort gets wrong, and the one that cost 2.4.3.
        assert!(v("1.10.0") > v("1.9.0"));
        assert!(v("1.9.0") > v("1.8.9"));
        assert!(v("2.0.0") > v("1.99.99"));
        assert!(v("1.10.1") > v("1.10.0"));
        // And a string sort really would get it wrong, which is why this is a
        // number: "1.10.0" < "1.9.0" alphabetically.
        assert!("1.10.0" < "1.9.0");
    }

    #[test]
    fn an_equal_version_is_never_newer() {
        let running = Version::new(1, 4, 4);
        assert!(Version::new(1, 4, 4) <= running, "an equal version is not newer");
        assert!(Version::new(1, 4, 5) > running);
    }

    #[test]
    fn a_version_with_a_tag_still_parses_to_something_orderable() {
        let v = |s: &str| s.parse::<Version>().expect("parses");
        assert_eq!(v("2.1.0-beta.3"), Version::new(2, 1, 0));
        assert_eq!(v("2.1.0+build7"), Version::new(2, 1, 0));
        assert_eq!(v("2.1"), Version::new(2, 1, 0));
    }

    #[test]
    fn nonsense_is_refused_rather_than_becoming_zero() {
        assert!("".parse::<Version>().is_err());
        assert!("latest".parse::<Version>().is_err());
    }

    /// **G4's other half.** A dev build is never told it is up to date.
    #[test]
    fn a_development_build_is_told_what_it_is() {
        let state = UpdateState {
            is_dev_build: true,
            running: "0.1.0".to_owned(),
            ..UpdateState::default()
        };
        let row = health_row(&state);
        assert!(!row.is_ok());
        assert!(row.says.contains("development build"), "{}", row.says);
        assert!(!row.says.contains("up to date"));
    }

    /// **I1.** A counter left on one version for months says so.
    #[test]
    fn an_old_version_says_how_long_it_has_been_old() {
        let state = UpdateState {
            running: "1.4.0".to_owned(),
            days_on_this_version: 200,
            ..UpdateState::default()
        };
        let row = health_row(&state);
        assert!(!row.is_ok());
        assert!(row.says.contains("200 days"), "{}", row.says);
        assert!(row.says.contains("Check for an update"));
    }

    #[test]
    fn an_available_update_is_always_visible_in_health() {
        let state = UpdateState {
            running: "1.4.4".to_owned(),
            available: Some("1.5.0".to_owned()),
            // Even having been dismissed today.
            dismissed_on: Some("2026-08-10".to_owned()),
            ..UpdateState::default()
        };
        let row = health_row(&state);
        assert!(!row.is_ok(), "a dismissed update vanished from health — that is I1");
        assert!(row.says.contains("1.5.0"));
    }

    /// **T12 — the rollout decision is stable.**
    #[test]
    fn a_staged_rollout_gives_one_machine_the_same_answer_every_time() {
        let rollout = Rollout {
            percent: 5,
            shops: Vec::new(),
        };
        let machine = "4c4c4544-0043-4a10-8033-b8c04f4d3132";
        let first = rollout.includes(machine, "shop_1");
        for _ in 0..50 {
            assert_eq!(rollout.includes(machine, "shop_1"), first);
        }
        // And different machines do not all get the same answer, or the
        // percentage would mean nothing.
        let answers: Vec<bool> = (0..200)
            .map(|n| rollout.includes(&format!("machine-{n}"), "shop_1"))
            .collect();
        assert!(answers.iter().any(|a| *a), "nobody was included at 5%");
        assert!(answers.iter().any(|a| !*a), "everybody was included at 5%");
    }

    #[test]
    fn a_named_shop_is_included_whatever_the_percentage_says() {
        let rollout = Rollout {
            percent: 0,
            shops: vec!["shop_annas".to_owned()],
        };
        assert!(rollout.includes("any-machine", "shop_annas"));
        assert!(!rollout.includes("any-machine", "shop_other"));
    }

    #[test]
    fn a_full_rollout_includes_everybody_and_a_zero_one_nobody() {
        let all = Rollout {
            percent: 100,
            shops: Vec::new(),
        };
        let none = Rollout {
            percent: 0,
            shops: Vec::new(),
        };
        for n in 0..20 {
            let m = format!("machine-{n}");
            assert!(all.includes(&m, "s"));
            assert!(!none.includes(&m, "s"));
        }
    }

    /// **T1, the second half.** A truncated download passes the signature and
    /// fails the hash, which is why there are two checks.
    #[test]
    fn a_damaged_package_is_refused() {
        let manifest = Manifest {
            version: Version::new(1, 5, 0),
            notes: "Faster reports.".to_owned(),
            url: "https://example.invalid/mb.exe".to_owned(),
            // sha256 of "the real package"
            sha256: {
                let d = ring::digest::digest(&ring::digest::SHA256, b"the real package");
                d.as_ref().iter().map(|b| format!("{b:02x}")).collect()
            },
            rollout: Rollout::default(),
        };
        assert!(check_package(b"the real package", &manifest).is_ok());
        assert_eq!(
            check_package(b"the real pack", &manifest),
            Err(Refused::Damaged)
        );
        // And the refusal says nothing was changed, which is the fact the
        // shopkeeper needs.
        assert!(Refused::Damaged.says().contains("Nothing on this computer has changed"));
    }

    /// **T1, the first half.** An unsigned or wrongly signed manifest is
    /// refused, and refused before anything is downloaded.
    #[test]
    fn a_manifest_we_did_not_sign_is_refused() {
        let manifest = Manifest {
            version: Version::new(9, 9, 9),
            notes: "Trust me.".to_owned(),
            url: "https://evil.invalid/mb.exe".to_owned(),
            sha256: "00".repeat(32),
            rollout: Rollout::default(),
        };
        let json = serde_json::to_string(&manifest).expect("serialises");
        assert_eq!(check(&json, "bm90IGEgc2lnbmF0dXJl"), Err(Refused::NotOurs));

        // Signed with our key, it is accepted — so the test above is testing
        // the signature and not the plumbing.
        let key = mb_license::snapshot::development_keypair().expect("a key");
        let signature = mb_license::snapshot::sign_detached(json.as_bytes(), &key);
        assert_eq!(
            check(&json, &signature).expect("accepted").version,
            manifest.version
        );
    }

    /// **T3.** Two failed starts roll back; one does not; a good start clears.
    #[test]
    fn two_failed_starts_roll_back_and_one_does_not() {
        let dir = std::env::temp_dir().join(format!("mb-starts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let version = Version::new(1, 5, 0);

        Starts::attempted(&dir, version);
        assert!(!Starts::should_roll_back(&dir, version), "one bad start is a fluke");

        Starts::attempted(&dir, version);
        assert!(!Starts::should_roll_back(&dir, version), "two is the limit, not past it");

        Starts::attempted(&dir, version);
        assert!(Starts::should_roll_back(&dir, version), "three starts with no healthy one");

        // A start that reaches a working counter clears the count.
        Starts::healthy(&dir);
        assert!(!Starts::should_roll_back(&dir, version));

        // And a NEW version starts its own count, so an old bad night does not
        // roll back a good release a year later.
        Starts::attempted(&dir, version);
        Starts::attempted(&dir, version);
        Starts::attempted(&dir, version);
        assert!(Starts::should_roll_back(&dir, version));
        assert!(!Starts::should_roll_back(&dir, Version::new(1, 6, 0)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mb-updates-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn a_manifest(version: Version, package: &[u8]) -> Manifest {
        Manifest {
            version,
            notes: "Faster reports.".to_owned(),
            url: "https://example.invalid/mb.exe".to_owned(),
            sha256: {
                let d = ring::digest::digest(&ring::digest::SHA256, package);
                d.as_ref().iter().map(|b| format!("{b:02x}")).collect()
            },
            rollout: Rollout::default(),
        }
    }

    /// **T2 — the way back exists because the way forward kept it.**
    ///
    /// Install 1.5.0, then 1.6.0, and 1.5.0's installer is what "go back"
    /// runs. The first install has nothing to keep, which is the honest state
    /// of a hand-installed build and the case `going_back_says` puts in words.
    #[test]
    fn installing_keeps_the_version_it_replaced() {
        let dir = scratch("keep");

        // The first update after a hand-installed build: nothing to go back to.
        let first = b"the 1.5.0 package";
        let path = keep_and_place(&dir, &a_manifest(Version::new(1, 5, 0), first), first)
            .expect("placed");
        assert!(path.exists());
        assert!(
            Previous::load(&dir).is_none(),
            "there was nothing installed by us before, so there is nothing to go back to"
        );

        // The second one keeps the first.
        let second = b"the 1.6.0 package";
        keep_and_place(&dir, &a_manifest(Version::new(1, 6, 0), second), second)
            .expect("placed");
        let previous = Previous::load(&dir).expect("1.5.0 was kept");
        assert_eq!(previous.version, Version::new(1, 5, 0));
        assert_eq!(
            std::fs::read(&previous.installer).expect("readable"),
            first,
            "the kept installer is not the one that was there"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **T1, and the ordering that makes it matter.** A damaged package is
    /// refused BEFORE anything on disk is replaced.
    #[test]
    fn a_damaged_package_never_reaches_the_disk() {
        let dir = scratch("damaged");
        let good = b"the 1.5.0 package";
        keep_and_place(&dir, &a_manifest(Version::new(1, 5, 0), good), good).expect("placed");
        let before: Vec<_> = std::fs::read_dir(Previous::installed_folder(&dir))
            .expect("readable")
            .flatten()
            .map(|e| e.file_name())
            .collect();

        let manifest = a_manifest(Version::new(1, 6, 0), b"what was published");
        assert_eq!(
            keep_and_place(&dir, &manifest, b"what actually arrived"),
            Err(Refused::Damaged)
        );

        let after: Vec<_> = std::fs::read_dir(Previous::installed_folder(&dir))
            .expect("readable")
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(before, after, "a refused package changed something on disk");
        assert!(
            Previous::load(&dir).is_none(),
            "a refused package moved the running version aside"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **T11 — I1's fix, both halves.**
    ///
    /// A dismissal lasts until the shop's next BUSINESS day and no longer, and
    /// the health panel shows the update the whole time.
    #[test]
    fn an_update_dismissed_today_comes_back_tomorrow() {
        let today = mb_core::BusinessDay::from_ymd(2026, 8, 10);
        let tomorrow = today.next();
        let state = UpdateState {
            running: "1.4.4".to_owned(),
            available: Some("1.5.0".to_owned()),
            dismissed_on: Some(today.to_string()),
            ..UpdateState::default()
        };

        assert!(is_dismissed(&state, today));
        assert!(offer(&state, today).is_none(), "it was dismissed today");

        assert!(!is_dismissed(&state, tomorrow));
        let back = offer(&state, tomorrow).expect("it came back");
        assert!(back.contains("1.5.0"));
        // And it never interrupts: the offer says so.
        assert!(back.contains("will not interrupt you"), "{back}");

        // **The panel shows it the whole time**, which is the half v1's
        // swiped-away snackbar did not have.
        assert!(!health_row(&state).is_ok());
    }

    #[test]
    fn nothing_is_offered_when_there_is_nothing_to_offer() {
        let today = mb_core::BusinessDay::from_ymd(2026, 8, 10);
        let state = UpdateState {
            running: "1.4.4".to_owned(),
            ..UpdateState::default()
        };
        assert!(offer(&state, today).is_none());
        assert!(health_row(&state).is_ok());
    }

    /// **T2's other half.** No previous version says so, in words.
    #[test]
    fn there_is_a_sentence_for_having_nothing_to_go_back_to() {
        let says = going_back_says(None);
        assert!(says.contains("first version installed here"), "{says}");

        let with = going_back_says(Some(&Previous {
            version: Version::new(1, 4, 4),
            installer: PathBuf::from("x.exe"),
        }));
        assert!(with.contains("1.4.4"));
        // The reassurance that matters most.
        assert!(with.contains("shop's data is not touched"));
    }
}
