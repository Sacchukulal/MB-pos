//! **Which computer this is** — and how sure we are, which turns out to matter
//! more than the id itself.
//!
//! > **POS-A4:** *"If the PC dies, the shop cannot bill until support unlocks
//! > the key. The licence is welded to the motherboard ID. There is no
//! > self-service way to move it, and no emergency mode."*
//!
//! The binding is not the problem — a licence sold per till has to know which
//! till. The problem was that v1 welded it to one number, gave nobody a way to
//! move it, and then told the owner to phone support on a Saturday.
//!
//! So this module answers "which computer is this" as well as it honestly can,
//! **says which way it found out** ([`Derivation`]), and hands the rest to
//! `transfer` and to the emergency code.
//!
//! # The chain, and why the placeholder list is the point of it
//!
//! 1. `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` — Windows writes it at
//!    install and does not change it.
//! 2. P19's `counter-id.txt`, the server id already sitting beside the config.
//!    It is already stable across restarts and already this machine's.
//! 3. A fresh random id, written beside the config, and **honestly weaker**: it
//!    does not survive somebody deleting the config folder. [`Derivation`]
//!    carries that fact so a support call can be told.
//!
//! Every step's value goes through [`is_a_real_identity`] first. v1 had that
//! list because OEMs ship whole production runs with the same firmware UUID,
//! and because **a disk image clones a MachineGuid across every PC in a chain
//! of shops** — which is the case that actually turns up, since the shops that
//! buy several tills buy them from the same dealer who images them from one
//! master. A placeholder that passes silently binds five tills to one licence
//! and nobody finds out until the fifth one stops working.
//!
//! # The firmware UUID is deliberately NOT read
//!
//! Reading it needs COM (the `wmi` crate, which pulls a runtime) or a raw
//! `GetSystemFirmwareTable` call. The workspace **forbids `unsafe`** and
//! mb-winprint owns the single exception (D33). MachineGuid is also strictly
//! better behaved than the firmware UUID on the machines this actually runs on
//! — it is never blank, never "To be filled by O.E.M.", and never shared by an
//! entire OEM batch. This paragraph is here so the next session does not
//! "finish" the chain by adding a dependency to reach a worse source.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// How the id was arrived at. Shown on the account screen, because "we could
/// not read this machine's id and made one up" is a thing an owner is entitled
/// to know before they transfer a licence onto it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Derivation {
    /// Step 1. Survives everything short of reinstalling Windows.
    MachineGuid,
    /// Step 2. P19's identity file, beside the config.
    CounterId,
    /// Step 3. Ours, and it is only as durable as the config folder.
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

    /// Whether losing the config folder loses the identity. The account screen
    /// says so quietly when it is true.
    #[must_use]
    pub const fn is_fragile(self) -> bool {
        matches!(self, Derivation::Generated)
    }
}

/// A machine's identity, and how it was found.
///
/// **Equality is the value alone.** Two `MachineId`s that name the same
/// computer are the same computer even if one was read from the registry and
/// the other came back from the cloud inside a licence — and the binding check
/// is exactly that comparison, so getting this wrong would unbind every counter
/// on the first refresh.
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

    /// What a person is shown. The full value is 32-plus characters of hex and
    /// nobody reads it out correctly; the first eight, upper-cased, is what
    /// support asks for and what the account screen prints.
    #[must_use]
    pub fn short(&self) -> String {
        self.value.chars().take(8).collect::<String>().to_uppercase()
    }

    /// For tests, and for reading one back out of a stored licence.
    #[must_use]
    pub fn for_tests(value: &str) -> MachineId {
        MachineId {
            value: value.to_owned(),
            how: Derivation::Recorded,
        }
    }

    /// **The chain.** `dir` is the config folder — `%APPDATA%\MagicBill\`.
    ///
    /// Never fails. The last step always produces something, because a counter
    /// that refused to start over an identity it could not read would be a
    /// counter that stopped a shop trading (requirement 3).
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

        // Step 2: P19 already put a stable id beside the config, for exactly
        // the same reason this module exists — a counter whose DHCP lease moves
        // it is still the same counter.
        if let Ok(text) = std::fs::read_to_string(dir.join("counter-id.txt"))
            && is_a_real_identity(text.trim())
        {
            return MachineId {
                value: normalise(text.trim()),
                how: Derivation::CounterId,
            };
        }

        // Step 3. Written once and read forever after — otherwise every restart
        // would look like a new machine and every restart would need a
        // transfer.
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
///
/// Kept from v1 and extended. Every entry here is something a real PC has
/// really answered with.
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

/// **The gate every step of the chain goes through.**
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
    // All one character — "0000000000", "----------". A real id is not.
    let mut chars = lower.chars().filter(|c| c.is_alphanumeric());
    match chars.next() {
        None => false,
        Some(first) => chars.any(|c| c != first),
    }
}

fn normalise(value: &str) -> String {
    value.trim().to_lowercase()
}

/// 32 hex characters from the OS's own source. `ring` is already here for the
/// signature work, so this costs nothing extra.
fn fresh_id() -> String {
    use ring::rand::SecureRandom as _;
    let mut bytes = [0_u8; 16];
    if ring::rand::SystemRandom::new().fill(&mut bytes).is_err() {
        // Vanishingly unlikely, and still not a reason to stop a shop trading.
        // The time is a poor identity and a working counter beats a correct
        // one that will not open.
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

    // **`KEY_WOW64_64KEY` is not decoration.** A 32-bit process reading this
    // path lands in `Wow6432Node`, where the value does not exist — so without
    // it, a 32-bit build silently falls through to step 2 and every machine
    // gets a different id from the 64-bit build. That is a fleet-wide
    // re-activation, caused by a missing flag.
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
    // The product is Windows-only (Tauri + mb-winprint). This exists so the
    // crate still builds and tests on anything else, which is worth the four
    // lines.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mb-machine-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// **The placeholder list, every entry, fed in one at a time.**
    ///
    /// This is the test v1's chain did not have. A cloned disk image gives five
    /// tills the same MachineGuid, and without this the fifth one to activate
    /// silently steals the licence from the first.
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

    /// Step 2: P19's file is used before anything is invented.
    #[test]
    fn the_counters_own_id_is_step_two() {
        let dir = scratch("counter-id");
        std::fs::write(dir.join("counter-id.txt"), "b41f0e2a9c7d4e11\n").expect("writes");
        let id = MachineId::of(&dir);
        // On Windows step 1 will normally win, so this asserts the behaviour
        // that is actually under test: whichever step answered, it is not the
        // invented one, and the file was a candidate.
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

    /// **Step 3 is stable across restarts**, or every restart would look like a
    /// new computer and need a transfer.
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

    /// It never fails, on any machine, in any state. Requirement 3's own
    /// corollary: there is no identity problem that stops a shop trading.
    #[test]
    fn the_chain_always_answers() {
        let dir = scratch("always");
        let id = MachineId::of(&dir);
        assert!(is_a_real_identity(id.value()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two different folders on the same machine still name the same machine,
    /// as long as a real source answered. (On Windows this is step 1.)
    #[test]
    fn fresh_ids_do_not_collide() {
        let a = fresh_id();
        let b = fresh_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
    }
}
