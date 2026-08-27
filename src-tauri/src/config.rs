//! The application config file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The same folder `mb_db::locate` uses, so a shop's whole configuration is in one place and a
/// support call can ask for one folder.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub window: WindowState,
}

impl AppConfig {
    /// `%APPDATA%\MagicBill\` on Windows.
    #[must_use]
    pub fn directory() -> PathBuf {
        mb_db::locate::default_config_dir()
    }

    /// Where Windows itself would put it, ignoring `APPDATA`.
    #[must_use]
    pub fn windows_default() -> PathBuf {
        std::env::var_os("USERPROFILE").map_or_else(
            // No `USERPROFILE` is not Windows as this product knows it.
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
        };
        config.save_to(&path).expect("saves");
        assert_eq!(AppConfig::load_from(&path), config);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_config_is_the_defaults_and_not_an_error() {
        let config = AppConfig::load_from(Path::new("nowhere-at-all.json"));
        assert!(!config.window.maximised);
    }

    #[test]
    fn this_file_does_not_know_where_the_shop_is() {
        let json = serde_json::to_string(&AppConfig::default()).expect("serialises");
        assert!(!json.contains("database"), "{json}");
    }

    #[test]
    fn a_corrupt_config_costs_a_theme_and_not_a_shop() {
        let path = scratch("corrupt");
        std::fs::write(&path, "{ this is not json").expect("writes");
        let config = AppConfig::load_from(&path);
        assert_eq!(config, AppConfig::default());
        assert!(path.with_extension("broken.json").exists());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("broken.json"));
    }
}
