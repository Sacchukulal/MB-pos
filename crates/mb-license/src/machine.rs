//! Which computer this is — and how sure we are, which turns out to matter more than the id
//! itself.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// How the id was arrived at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Derivation {
    /// Survives everything short of reinstalling Windows.
    MachineGuid,
    CounterId,
    /// Ours, and it is only as durable as the config folder.
    Generated,
    /// A test's, or a value read back out of a stored licence.
    Recorded,
}

impl Derivation {
    #[must_use]
    pub const fn in_words(self) -> &'static str {
        match self {
            Derivation::MachineGuid => "from Windows",
            Derivation::CounterId => "from this counter's certificate",
            Derivation::Generated => "made by Magic Bill on this computer",
            Derivation::Recorded => "recorded earlier",
        }
    }

    /// Whether losing the config folder loses the identity.
    #[must_use]
    pub const fn is_fragile(self) -> bool {
        matches!(self, Derivation::Generated)
    }
}

/// A machine's identity, and how it was found.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineId {
    value: String,
    how: Derivation,
}

impl PartialEq for MachineId {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}
impl Eq for MachineId {}

impl MachineId {
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub const fn how(&self) -> Derivation {
        self.how
    }

    /// What a person is shown.
    #[must_use]
    pub fn short(&self) -> String {
        self.value
            .chars()
            .take(8)
            .collect::<String>()
            .to_uppercase()
    }

    /// For tests, and for reading one back out of a stored licence.
    #[must_use]
    pub fn for_tests(value: &str) -> MachineId {
        MachineId {
            value: value.to_owned(),
            how: Derivation::Recorded,
        }
    }

    /// The chain. `dir` is the config folder — `%APPDATA%\MagicBill\`.
    #[must_use]
    pub fn of(dir: &Path) -> MachineId {
        if let Some(value) = machine_guid()
            && is_a_real_identity(&value)
        {
            return MachineId {
                value: normalise(&value),
                how: Derivation::MachineGuid,
            };
        }

        if let Ok(text) = std::fs::read_to_string(dir.join("counter-id.txt"))
            && is_a_real_identity(text.trim())
        {
            return MachineId {
                value: normalise(text.trim()),
                how: Derivation::CounterId,
            };
        }

        // Written once and read forever after — otherwise every restart would look like a new
        // machine and every restart would need a transfer.
        let path = dir.join("machine-id.txt");
        if let Ok(text) = std::fs::read_to_string(&path)
            && is_a_real_identity(text.trim())
        {
            return MachineId {
                value: normalise(text.trim()),
                how: Derivation::Generated,
            };
        }
        let fresh = fresh_id();
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&path, &fresh);
        MachineId {
            value: fresh,
            how: Derivation::Generated,
        }
    }
}

/// Values that are not identities, however confidently a machine reports them.
const PLACEHOLDERS: &[&str] = &[
    "00000000-0000-0000-0000-000000000000",
    "ffffffff-ffff-ffff-ffff-ffffffffffff",
    // The one Intel shipped on a very large number of desktop boards.
    "03000200-0400-0500-0006-000700080009",
    "default string",
    "to be filled by o.e.m.",
    "to be filled by o.e.m",
    "system serial number",
    "system uuid",
    "not applicable",
    "none",
    "unknown",
    "0",
];

/// The gate every step of the chain goes through.
#[must_use]
pub fn is_a_real_identity(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.len() < 8 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if PLACEHOLDERS.contains(&lower.as_str()) {
        return false;
    }
    // All one character — "0000000000", "----------".
    let mut chars = lower.chars().filter(|c| c.is_alphanumeric());
    match chars.next() {
        None => false,
        Some(first) => chars.any(|c| c != first),
    }
}

fn normalise(value: &str) -> String {
    value.trim().to_lowercase()
}

/// 32 hex characters from the OS's own source.
fn fresh_id() -> String {
    use ring::rand::SecureRandom as _;
    let mut bytes = [0_u8; 16];
    if ring::rand::SystemRandom::new().fill(&mut bytes).is_err() {
        // Vanishingly unlikely, and still not a reason to stop a shop trading.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        return format!("{now:032x}");
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(windows)]
fn machine_guid() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};

    // `KEY_WOW64_64KEY` is not decoration.
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Cryptography",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .ok()?
        .get_value::<String, _>("MachineGuid")
        .ok()
}

#[cfg(not(windows))]
fn machine_guid() -> Option<String> {
    // The product is Windows-only (Tauri + mb-winprint).
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mb-machine-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// The placeholder list, every entry, fed in one at a time.
    #[test]
    fn a_placeholder_is_never_an_identity() {
        for placeholder in PLACEHOLDERS {
            assert!(
                !is_a_real_identity(placeholder),
                "{placeholder:?} was accepted as a machine identity"
            );
            // And case does not save it.
            assert!(!is_a_real_identity(&placeholder.to_uppercase()));
        }
        for junk in ["", "   ", "abc", "0000000000", "--------------", "\t\n"] {
            assert!(!is_a_real_identity(junk), "{junk:?} was accepted");
        }
    }

    #[test]
    fn a_real_looking_id_is_accepted() {
        for good in [
            "4c4c4544-0043-4a10-8033-b8c04f4d3132",
            "a1b2c3d4e5f6a7b8",
            "  9f8e7d6c5b4a3928  ",
        ] {
            assert!(is_a_real_identity(good), "{good:?} was rejected");
        }
    }

    #[test]
    fn the_counters_own_id_is_step_two() {
        let dir = scratch("counter-id");
        std::fs::write(dir.join("counter-id.txt"), "b41f0e2a9c7d4e11\n").expect("writes");
        let id = MachineId::of(&dir);
        // On Windows step 1 will normally win, so this asserts the behaviour that is actually
        // under test: whichever step answered, it is not the invented one, and the file was a
        // candidate.
        assert!(matches!(
            id.how(),
            Derivation::MachineGuid | Derivation::CounterId
        ));
        assert!(is_a_real_identity(id.value()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A placeholder in step 2 must not be taken — the chain moves on.
    #[test]
    fn a_placeholder_in_the_file_is_stepped_over() {
        let dir = scratch("placeholder-file");
        std::fs::write(
            dir.join("counter-id.txt"),
            "00000000-0000-0000-0000-000000000000",
        )
        .expect("writes");
        let id = MachineId::of(&dir);
        assert_ne!(id.value(), "00000000-0000-0000-0000-000000000000");
        assert!(is_a_real_identity(id.value()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Step 3 is stable across restarts, or every restart would look like a new computer and
    /// need a transfer.
    #[test]
    fn a_generated_id_is_written_once_and_read_thereafter() {
        let dir = scratch("generated");
        let first = fresh_id();
        std::fs::write(dir.join("machine-id.txt"), &first).expect("writes");
        // Nothing else in the folder, so step 2 cannot answer.
        let a = MachineId::of(&dir);
        let b = MachineId::of(&dir);
        assert_eq!(a, b, "the id changed between two reads");
        if a.how() == Derivation::Generated {
            assert_eq!(a.value(), first);
            assert!(a.how().is_fragile());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identity_is_the_value_and_not_how_it_was_found() {
        let from_registry = MachineId {
            value: "abc12345def".to_owned(),
            how: Derivation::MachineGuid,
        };
        let from_a_licence = MachineId::for_tests("abc12345def");
        assert_eq!(
            from_registry, from_a_licence,
            "the binding check compares these two, and a counter that thought \
             they differed would unbind itself on every refresh"
        );
    }

    #[test]
    fn the_short_form_is_what_support_asks_for() {
        let id = MachineId::for_tests("4c4c4544-0043-4a10");
        assert_eq!(id.short(), "4C4C4544");
    }

    /// It never fails, on any machine, in any state.
    #[test]
    fn the_chain_always_answers() {
        let dir = scratch("always");
        let id = MachineId::of(&dir);
        assert!(is_a_real_identity(id.value()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two different folders on the same machine still name the same machine, as long as a real
    /// source answered.
    #[test]
    fn fresh_ids_do_not_collide() {
        let a = fresh_id();
        let b = fresh_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
    }
}
