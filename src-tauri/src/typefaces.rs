//! **The faces a shop may print in** — P31, and the owner's fourth item:
//! *"Fonts for bill and KOT: 5-6 choices."*
//!
//! # Why the faces are not in the installer
//!
//! Five more `.ttf` files in `mb-print/assets/` is about 700 KB against S4's
//! 20 MB installer, and five more licences to check. Every family in
//! [`mb_print::font::FAMILIES`] ships with Windows 10 and 11, `Font::load`
//! already takes bytes from anywhere — the seam crown jewel 17 needs for a
//! Kannada face — and this reads them out of `%SystemRoot%\Fonts`. It costs
//! nothing in the download.
//!
//! # Why this is here and not in `mb-print`
//!
//! Two things resolving a face needs that a layout library must not have: a
//! file system with an opinion about where Windows keeps typefaces, and
//! somewhere to say *"Cascadia Mono is not installed on this computer"*. D31
//! put the operating system in `mb-winprint`; the log lives in this crate. So
//! `mb-print` keeps the list of families — a fact about typefaces — and asks
//! for the rest through [`mb_print::font::Typefaces`], exactly as it already
//! asks for its transports and its storage.
//!
//! # It cannot fail
//!
//! A shop whose chosen face has been uninstalled between one Tuesday and the
//! next gets the built-in one and a line in the log. Requirement 3 of the ten:
//! **billing does not stop**, and the only thing worse than the wrong typeface
//! is no bill.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mb_print::font::{Font, Typefaces};

use crate::{log_info, log_warn};

/// Every face this counter has loaded, by key, loaded once.
///
/// One of these lives for the life of the process and is shared by every
/// printer's worker thread — the same bargain `Font` makes about its glyph
/// cache. Parsing a face is a few milliseconds, and doing it forty times a
/// lunch rush would be forty times too many.
///
/// **A failure is remembered too** (`None` in the map), so a font somebody
/// uninstalled is one line in the log and not one per ticket.
#[derive(Debug)]
pub struct SystemFaces {
    builtin: Arc<Font>,
    loaded: Mutex<BTreeMap<String, Option<Arc<Font>>>>,
}

impl SystemFaces {
    /// Fails only if the built-in face will not load, which means the install
    /// is corrupt — and `App::new` already treats that as "cannot start".
    pub fn new() -> Result<SystemFaces, mb_print::PrintError> {
        Ok(SystemFaces {
            builtin: Arc::new(Font::builtin()?),
            loaded: Mutex::new(BTreeMap::new()),
        })
    }

    #[must_use]
    pub fn builtin(&self) -> Arc<Font> {
        Arc::clone(&self.builtin)
    }

    /// Where Windows keeps its typefaces.
    fn font_dir() -> std::path::PathBuf {
        std::env::var_os("SystemRoot")
            .map_or_else(
                || std::path::PathBuf::from("C:\\Windows"),
                std::path::PathBuf::from,
            )
            .join("Fonts")
    }

    fn load(key: &str) -> Option<Arc<Font>> {
        let Some(family) = mb_print::font::family(key) else {
            log_warn!("\"{key}\" is not a typeface this build knows; printing with the built-in one");
            return None;
        };
        // The built-in has no file, and asking for it by name is not a problem
        // to report.
        let file = family.file?;

        let path = SystemFaces::font_dir().join(file);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                log_warn!(
                    "{} is not installed on this computer ({e}); printing with the built-in one",
                    family.label
                );
                return None;
            }
        };
        match Font::load(&bytes, family.label) {
            Ok(font) => {
                log_info!("printing with {} from {}", family.label, path.display());
                Some(Arc::new(font))
            }
            Err(e) => {
                log_warn!(
                    "{} will not load ({e}); printing with the built-in one",
                    family.label
                );
                None
            }
        }
    }
}

impl Typefaces for SystemFaces {
    fn face(&self, key: Option<&str>) -> Arc<Font> {
        let Some(key) = key.filter(|k| !k.is_empty() && *k != "builtin") else {
            return self.builtin();
        };

        // Two locks rather than one held across the file read: a face being
        // loaded for the first time must not hold up a second printer's worker
        // drawing a bill in a face that is already warm. The worst case is two
        // threads parsing the same file once each on the same second, which
        // costs milliseconds and cannot be wrong.
        if let Some(found) = lock(&self.loaded).get(key) {
            return found.clone().unwrap_or_else(|| self.builtin());
        }
        let loaded = SystemFaces::load(key);
        lock(&self.loaded).insert(key.to_owned(), loaded.clone());
        loaded.unwrap_or_else(|| self.builtin())
    }
}

/// The same trade the glyph cache makes: a panicking worker thread must not
/// stop the counter printing for the rest of the shift.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_face_is_the_built_in_one_and_not_a_failure() {
        let faces = SystemFaces::new().expect("the built-in face loads");
        // Requirement 3: nothing about a typeface may stop a bill.
        let asked = faces.face(Some("a face nobody has ever installed"));
        assert!(Arc::ptr_eq(&asked, &faces.builtin()));
    }

    #[test]
    fn nothing_none_and_builtin_all_mean_the_same_face() {
        let faces = SystemFaces::new().expect("the built-in face loads");
        for key in [None, Some(""), Some("builtin")] {
            assert!(Arc::ptr_eq(&faces.face(key), &faces.builtin()), "{key:?}");
        }
    }

    #[test]
    fn every_family_on_the_list_resolves_to_something_printable() {
        // **Not "every family loads".** Windows Server has no Cascadia Mono and
        // a stripped image may have no Consolas, so asserting the file is there
        // would be asserting something about the machine running the test. What
        // must be true is that ASKING is always safe and always answers with a
        // face — which is the property a shop depends on.
        let faces = SystemFaces::new().expect("the built-in face loads");
        for family in mb_print::font::FAMILIES {
            let font = faces.face(Some(family.key));
            let cell = font.cell_for_cap(15);
            assert!(
                !font.glyph('M', cell).is_blank(),
                "{} gave a face that cannot draw an M",
                family.key
            );
        }
    }

    #[test]
    fn a_face_is_loaded_once_and_then_shared() {
        let faces = SystemFaces::new().expect("the built-in face loads");
        let first = faces.face(Some("consolas"));
        let second = faces.face(Some("consolas"));
        assert!(
            Arc::ptr_eq(&first, &second),
            "the face cache is not caching, and every ticket re-parses a font file"
        );
    }
}
