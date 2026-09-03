//! The faces a shop may print in.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mb_print::font::{Font, Typefaces, DEFAULT_KEY};

use crate::{log_info, log_warn};

/// Every face this counter has loaded, by key, loaded once.
#[derive(Debug)]
pub struct SystemFaces {
    /// The default face, loaded on start so a missing choice always has something to print in.
    fallback: Arc<Font>,
    loaded: Mutex<BTreeMap<String, Option<Arc<Font>>>>,
}

impl SystemFaces {
    /// Fails only if the default face is missing from the computer's font folder, which
    /// `App::new` already treats as "cannot start".
    pub fn new() -> Result<SystemFaces, mb_print::PrintError> {
        Ok(SystemFaces {
            fallback: Arc::new(Font::default_face()?),
            loaded: Mutex::new(BTreeMap::new()),
        })
    }

    #[must_use]
    pub fn fallback(&self) -> Arc<Font> {
        Arc::clone(&self.fallback)
    }

    fn load(key: &str) -> Option<Arc<Font>> {
        let Some(family) = mb_print::font::family(key) else {
            log_warn!("\"{key}\" is not a typeface this build knows; printing with the default one");
            return None;
        };
        match family.load() {
            Ok(font) => {
                log_info!("printing with {} from {}", family.label, family.path().display());
                Some(Arc::new(font))
            }
            Err(e) => {
                log_warn!("{} will not load ({e}); printing with the default one", family.label);
                None
            }
        }
    }
}

impl Typefaces for SystemFaces {
    fn face(&self, key: Option<&str>) -> Arc<Font> {
        let Some(key) = key.filter(|k| !k.is_empty() && *k != DEFAULT_KEY) else {
            return self.fallback();
        };

        // Two locks rather than one held across the file read: a face being loaded for the
        // first time must not hold up a second printer's worker drawing a bill in a face that
        // is already warm.
        if let Some(found) = lock(&self.loaded).get(key) {
            return found.clone().unwrap_or_else(|| self.fallback());
        }
        let loaded = SystemFaces::load(key);
        lock(&self.loaded).insert(key.to_owned(), loaded.clone());
        loaded.unwrap_or_else(|| self.fallback())
    }
}

/// The same trade the glyph cache makes: a panicking worker thread must not stop the counter
/// printing for the rest of the shift.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_face_is_the_default_one_and_not_a_failure() {
        let faces = SystemFaces::new().expect("the default face loads");
        // Requirement 3: nothing about a typeface may stop a bill.
        let asked = faces.face(Some("a face nobody has ever installed"));
        assert!(Arc::ptr_eq(&asked, &faces.fallback()));
    }

    #[test]
    fn nothing_none_and_the_default_key_all_mean_the_same_face() {
        let faces = SystemFaces::new().expect("the default face loads");
        for key in [None, Some(""), Some(DEFAULT_KEY)] {
            assert!(Arc::ptr_eq(&faces.face(key), &faces.fallback()), "{key:?}");
        }
    }

    #[test]
    fn every_family_on_the_list_resolves_to_something_printable() {
        // Not "every family loads".
        let faces = SystemFaces::new().expect("the default face loads");
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
        let faces = SystemFaces::new().expect("the default face loads");
        let first = faces.face(Some("monospace"));
        let second = faces.face(Some("monospace"));
        assert!(
            Arc::ptr_eq(&first, &second),
            "the face cache is not caching, and every ticket re-parses a font file"
        );
    }
}
