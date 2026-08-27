//! A shop must be able to go back.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::health::HealthRow;

// A version is a number.

/// A version, as three numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
        // Anything after a `-` or `+` is a pre-release or build tag, and this product does not
        // use them.
        let core = text.split(['-', '+']).next().unwrap_or(text);
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

/// Is this build a real release?
#[must_use]
pub const fn is_a_release_build() -> bool {
    !cfg!(debug_assertions)
}

// The manifest.

/// What a release says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: Version,
    /// Short, and in plain words.
    pub notes: String,
    pub url: String,
    /// Lowercase hex. ANDROID-G2 — "the downloaded update is never verified against a published
    /// fingerprint".
    pub sha256: String,
    pub rollout: Rollout,
}

/// Who gets it first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rollout {
    pub percent: u32,
    /// Named shops, whatever the percentage says.
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
        // FNV-1a over the machine id: tiny, stable across runs and across versions of the
        // standard library — which `DefaultHasher` explicitly is not, and a rollout that
        // changes when Rust updates is the flapping this exists to avoid.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in machine.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        u32::try_from(hash % 100).unwrap_or(0) < self.percent
    }
}

pub trait Releases: Send + Sync + 'static {
    /// The signed manifest, as `(json, signature_base64)`.
    fn latest(&self) -> Result<(String, String), String>;
}

/// Keys this build accepts a manifest from.
#[must_use]
pub fn trusted_keys() -> Vec<Vec<u8>> {
    mb_license::snapshot::trusted_keys()
}

/// Errors a person can be told about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The signature did not check out.
    NotOurs,
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
            Refused::Unreadable => {
                "We could not read what the update server sent. Nothing on this \
                 computer has changed."
            }
        }
    }
}

/// Check the manifest's signature and hand back what it says.
pub fn check(json: &str, signature: &str) -> Result<Manifest, Refused> {
    mb_license::verify_detached(json.as_bytes(), signature, &trusted_keys())
        .map_err(|_| Refused::NotOurs)?;
    serde_json::from_str(json).map_err(|_| Refused::Unreadable)
}

// Two failed starts roll back, and the counter counts them.

/// What is on disk beside the config, in `updates\starts.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Starts {
    /// The version these attempts belong to.
    pub version: String,
    pub attempts: u32,
}

/// Two. Not one, not three.
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

    /// Called once the counter is genuinely up, which means the window is visible AND the shop
    /// is open — or the first-run state was reached, which is a working counter with no shop
    /// yet.
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

    /// What is kept, if anything.
    #[must_use]
    pub fn load(dir: &Path) -> Option<Previous> {
        let text = std::fs::read_to_string(Previous::folder(dir).join("previous.json")).ok()?;
        let previous: Previous = serde_json::from_str(&text).ok()?;
        previous.installer.exists().then_some(previous)
    }
}

/// A cloud that is not there yet, and a stub that ships.
pub struct NoReleaseServerYet;

impl Releases for NoReleaseServerYet {
    fn latest(&self) -> Result<(String, String), String> {
        Err("there is no release server yet".to_owned())
    }
}

/// The sentence for "go back", including the case where there is nothing to go back to.
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

// Never by surprise, never forgotten.

/// What the counter knows about updates right now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    /// The version on offer, if any.
    pub available: Option<String>,
    pub notes: String,
    /// How many days this counter has been on its current version.
    pub days_on_this_version: u32,
    pub running: String,
    pub is_dev_build: bool,
}

/// How long is too long on one version.
pub const STALE_AFTER_DAYS: u32 = 30;

/// The health panel's row.
#[must_use]
pub fn health_row(state: &UpdateState) -> HealthRow {
    if state.is_dev_build {
        // A development build is never told it is up to date.
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
    HealthRow::ok(
        "update",
        "Version",
        format!("Version {}, up to date.", state.running),
    )
}

// The bodies, and the seats.

pub fn look_for_one_on(app: &crate::state::App) -> crate::words::UiResult<UpdateState> {
    crate::guard::require(app, mb_auth::Permission::ReportsView)?;
    let mut state = app.updates();

    // Everything below needs a release server.
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

/// Go back to the version before this one.
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
    // Running it is deliberately not done here.
    Ok(going_back_says(Some(&previous)))
}

#[tauri::command]
pub fn look_for_an_update(
    app: tauri::State<'_, crate::state::App>,
) -> crate::words::UiResult<UpdateState> {
    look_for_one_on(&app)
}

#[tauri::command]
pub fn go_back_a_version(
    app: tauri::State<'_, crate::state::App>,
) -> crate::words::UiResult<String> {
    go_back_on(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers_and_never_as_names() {
        let v = |s: &str| s.parse::<Version>().expect("parses");
        // The comparison a string sort gets wrong, and the one that cost 2.4.3.
        assert!(v("1.10.0") > v("1.9.0"));
        assert!(v("1.9.0") > v("1.8.9"));
        assert!(v("2.0.0") > v("1.99.99"));
        assert!(v("1.10.1") > v("1.10.0"));
        // And a string sort really would get it wrong, which is why this is a number: "1.10.0"
        // < "1.9.0" alphabetically.
        assert!("1.10.0" < "1.9.0");
    }

    #[test]
    fn an_equal_version_is_never_newer() {
        let running = Version::new(1, 4, 4);
        assert!(
            Version::new(1, 4, 4) <= running,
            "an equal version is not newer"
        );
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

    /// G4's other half. A dev build is never told it is up to date.
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

    /// A counter left on one version for months says so.
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
            ..UpdateState::default()
        };
        let row = health_row(&state);
        assert!(
            !row.is_ok(),
            "a dismissed update vanished from health — that is I1"
        );
        assert!(row.says.contains("1.5.0"));
    }

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
        // And different machines do not all get the same answer, or the percentage would mean
        // nothing.
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

        // Signed with our key, it is accepted — so the test above is testing the signature and
        // not the plumbing.
        let key = mb_license::snapshot::development_keypair().expect("a key");
        let signature = mb_license::snapshot::sign_detached(json.as_bytes(), &key);
        assert_eq!(
            check(&json, &signature).expect("accepted").version,
            manifest.version
        );
    }

    /// Two failed starts roll back; one does not; a good start clears.
    #[test]
    fn two_failed_starts_roll_back_and_one_does_not() {
        let dir = std::env::temp_dir().join(format!("mb-starts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let version = Version::new(1, 5, 0);

        Starts::attempted(&dir, version);
        assert!(
            !Starts::should_roll_back(&dir, version),
            "one bad start is a fluke"
        );

        Starts::attempted(&dir, version);
        assert!(
            !Starts::should_roll_back(&dir, version),
            "two is the limit, not past it"
        );

        Starts::attempted(&dir, version);
        assert!(
            Starts::should_roll_back(&dir, version),
            "three starts with no healthy one"
        );

        // A start that reaches a working counter clears the count.
        Starts::healthy(&dir);
        assert!(!Starts::should_roll_back(&dir, version));

        // And a NEW version starts its own count, so an old bad night does not roll back a good
        // release a year later.
        Starts::attempted(&dir, version);
        Starts::attempted(&dir, version);
        Starts::attempted(&dir, version);
        assert!(Starts::should_roll_back(&dir, version));
        assert!(!Starts::should_roll_back(&dir, Version::new(1, 6, 0)));

        let _ = std::fs::remove_dir_all(&dir);
    }

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
