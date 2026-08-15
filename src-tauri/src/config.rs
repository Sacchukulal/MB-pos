//! The application config file — **audit A5, and it is a data-loss finding.**
//!
//! > *"v1 kept the database path in the browser's local storage, so clearing
//! > that storage — or an external drive changing its letter — showed the owner
//! > a first-run wizard with their live shop sitting three folders away."*
//!
//! `mb-db` says the same thing from the other side: *"`DbConfig::path` comes
//! from the caller and this crate never guesses it. It must be read from an
//! application config file on disk — **never from web local storage**."*
//!
//! This file holds the three things that must be known **before** anything
//! opens and that are not the shop's data: how big the window was, which theme,
//! and how large the text.
//!
//! # It deliberately does NOT hold the database path
//!
//! P05 already owns that, in `mb_db::locate` — a one-line
//! `database-location.txt` beside this file, readable and removable **without
//! opening SQLite**, because the whole point of A5 is that the database may be
//! the thing that is broken. Two files claiming the same fact is how they end
//! up disagreeing, so this one does not claim it. `locate::read_config` is the
//! answer to "where is the shop?", here and everywhere else.
//!
//! # Why the window state is here and not in the database
//!
//! Because the window has to open when the database cannot. A first run, a
//! failed migration and a restore in progress are all states where there is a
//! window and no database, and every one of them is a state where the owner is
//! already having a bad day.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The same folder `mb_db::locate` uses, so a shop's whole configuration is in
/// one place and a support call can ask for one folder.
const FILE: &str = "app-config.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WindowState {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximised: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub window: WindowState,
    /// Applied before the first paint so the window never flashes light and
    /// then goes dark. The names come from `ui/src/theme/themes.ts`, and this
    /// side deliberately does not validate them: a theme is data, and adding
    /// one must not require a Rust change (D21, owner's ruling 2026-08-04).
    pub theme: String,
    pub text_size: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            window: WindowState::default(),
            theme: "light".to_owned(),
            text_size: "normal".to_owned(),
        }
    }
}

impl AppConfig {
    /// `%APPDATA%\MagicBill\` on Windows — **P05's folder, not a second one.**
    #[must_use]
    pub fn directory() -> PathBuf {
        mb_db::locate::default_config_dir()
    }

    /// **Where Windows itself would put it**, ignoring `APPDATA`.
    ///
    /// [`AppConfig::directory`] reads `APPDATA`, which is how a second till can
    /// be run on one machine for D55's two-process check. This is the folder a
    /// shopkeeper's copy always uses, so the two differing means somebody set
    /// the variable on purpose — see the one-copy lock in `main.rs`.
    #[must_use]
    pub fn windows_default() -> PathBuf {
        std::env::var_os("USERPROFILE").map_or_else(
            // No `USERPROFILE` is not Windows as this product knows it. Answer
            // with `directory()` so the two compare EQUAL and the lock is taken
            // — the safe direction to be wrong in is "one copy".
            AppConfig::directory,
            |home| {
                PathBuf::from(home)
                    .join("AppData")
                    .join("Roaming")
                    .join("MagicBill")
            },
        )
    }

    #[must_use]
    pub fn path() -> PathBuf {
        AppConfig::directory().join(FILE)
    }

    /// Read it, or return the defaults.
    ///
    /// **A corrupt config is never fatal.** It costs the window position and a
    /// theme; it must not cost the shop its counter. The corrupt file is kept
    /// beside the new one, because it is the only record of where the database
    /// used to be — which is exactly what A5 is about.
    #[must_use]
    pub fn load_from(path: &Path) -> AppConfig {
        let Ok(text) = std::fs::read_to_string(path) else {
            return AppConfig::default();
        };
        match serde_json::from_str(&text) {
            Ok(config) => config,
            Err(e) => {
                crate::log_warn!(
                    "the config file at {} could not be read ({e}); keeping a copy \
                     as config.broken.json and starting from defaults",
                    path.display()
                );
                let _ = std::fs::copy(path, path.with_extension("broken.json"));
                AppConfig::default()
            }
        }
    }

    #[must_use]
    pub fn load() -> AppConfig {
        AppConfig::load_from(&AppConfig::path())
    }

    /// Write it, atomically.
    ///
    /// Through a temporary file and a rename, because the alternative is that a
    /// power cut while saving the window size leaves a shop with a config file
    /// that is half-written JSON — and then A5 happens for a new reason.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, path)
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&AppConfig::path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mb-config-{}-{label}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("config.json")
    }

    #[test]
    fn it_round_trips() {
        let path = scratch("round-trip");
        let config = AppConfig {
            window: WindowState {
                width: Some(1600),
                height: Some(900),
                x: Some(20),
                y: Some(40),
                maximised: true,
            },
            theme: "dark".to_owned(),
            text_size: "large".to_owned(),
        };
        config.save_to(&path).expect("saves");
        assert_eq!(AppConfig::load_from(&path), config);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_config_is_the_defaults_and_not_an_error() {
        let config = AppConfig::load_from(Path::new("nowhere-at-all.json"));
        assert_eq!(config.theme, "light");
        assert!(!config.window.maximised);
    }

    #[test]
    fn this_file_does_not_know_where_the_shop_is() {
        // P05's `locate` owns that, in its own one-line file. Two files
        // claiming the same fact is how they come to disagree — and the fact in
        // question is audit A5, which is a data-loss finding.
        let json = serde_json::to_string(&AppConfig::default()).expect("serialises");
        assert!(!json.contains("database"), "{json}");
    }

    #[test]
    fn a_corrupt_config_costs_a_theme_and_not_a_shop() {
        let path = scratch("corrupt");
        std::fs::write(&path, "{ this is not json").expect("writes");
        let config = AppConfig::load_from(&path);
        assert_eq!(config, AppConfig::default());
        // And the old one is kept, because it is the only record of where the
        // database was (audit A5).
        assert!(path.with_extension("broken.json").exists());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("broken.json"));
    }

    #[test]
    fn an_unknown_theme_name_is_not_rejected_here() {
        // A theme is data (D21). Adding one must never need a Rust change, so
        // this side stores the name and does not know the list.
        let path = scratch("unknown-theme");
        std::fs::write(&path, r#"{"theme":"midnight-blue-that-p17-added"}"#).expect("writes");
        assert_eq!(AppConfig::load_from(&path).theme, "midnight-blue-that-p17-added");
        let _ = std::fs::remove_file(&path);
    }
}
