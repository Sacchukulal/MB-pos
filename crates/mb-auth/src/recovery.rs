//! **How the owner gets back in, and why a waiter cannot walk the same path.**
//!
//! # First, what a PIN is for
//!
//! It defends against the person behind the counter and the person who wanders
//! behind it. It does **not** defend against somebody who takes the computer or
//! copies the database file — nothing on an offline counter can, because the
//! shop's machine must be able to read the shop's data. That paragraph is in
//! `docs/DECISIONS.md` too, because a security claim nobody wrote down is a
//! security claim nobody can check.
//!
//! # The code
//!
//! Ten characters from an alphabet with no `O`/`0` and no `I`/`1`/`l`, because
//! this gets read off a printed slip by somebody in a hurry, and a code that
//! cannot be transcribed is a code that gets photographed and left on a phone.
//!
//! Generated when the first PIN is set on a `staff.manage` role. **Shown once,
//! and printed on the shop's own printer** — there is a printer, and paper in a
//! drawer is a better place for this than a screenshot. Only its Argon2 hash is
//! stored.
//!
//! Using it sets a new PIN for one `staff.manage` staff member, kills the old
//! code, issues and prints a new one, and writes an audit row.
//!
//! # Why staff cannot abuse it
//!
//! * They never see it. It is shown once, at setup, to whoever set the shop up.
//! * It is not recoverable from the database — the same Argon2 as a PIN.
//! * 32^10 is about 2^50, so guessing is not a strategy, and each guess costs a
//!   full Argon2 verification.
//! * Every use is **loud**: a slip prints, an audit row is written, and P33
//!   syncs it to the owner's phone.
//!
//! # Why it is deliberately NOT rate-limited
//!
//! [`crate::lockout`] is per staff member so that a malicious waiter cannot
//! lock the owner out with five wrong guesses. That protection is worth nothing
//! if the recovery path can be locked instead — so it is not. The defence there
//! is the entropy and the Argon2 cost, not a counter.
//!
//! # If the code is lost as well
//!
//! Support, against the licence — and P21 owns the licence, so that path does
//! not exist yet. **The seam is named, not built.** Until P21 the honest answer
//! is a restored backup or a support visit, and the summary says so.

use argon2::password_hash::rand_core::{OsRng, RngCore};

use crate::error::AuthError;
use crate::pin::PinHash;

/// No `O`, `0`, `I`, `1` or `l`. Read off paper, typed by somebody in a hurry.
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// Ten characters, in two groups of five, so it can be read aloud.
pub const CODE_LENGTH: usize = 10;
const GROUP: usize = 5;

/// A recovery code, in the one moment it exists in plain text.
///
/// It has no `Debug`, on purpose. The one way it may leave this process is
/// [`RecoveryCode::to_print`], which is called by the code that puts it on
/// paper and on the screen — and being forced to name that call is the point.
#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryCode(String);

impl RecoveryCode {
    /// For the slip and for the one-time screen. Nowhere else.
    #[must_use]
    pub fn to_print(&self) -> String {
        let (left, right) = self.0.split_at(GROUP);
        format!("{left}-{right}")
    }

    /// What the owner types back in, in any shape they type it: lower case,
    /// with or without the dash, with stray spaces.
    #[must_use]
    pub fn normalise(typed: &str) -> String {
        typed
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_uppercase())
            .collect()
    }

    #[must_use]
    pub fn as_typed(&self) -> &str {
        &self.0
    }
}

/// A fresh code and the hash to store.
///
/// Returns both because the plain text must not be recoverable afterwards: the
/// caller prints one and stores the other, and there is no third option.
pub fn new_recovery_code() -> Result<(RecoveryCode, PinHash), AuthError> {
    let mut bytes = [0u8; CODE_LENGTH];
    OsRng.fill_bytes(&mut bytes);

    // Modulo bias: the alphabet is 31 characters and a byte is 256 values, so
    // the first eleven characters are very slightly likelier than the rest.
    // Rejection sampling instead — it costs a loop that almost never repeats,
    // and "almost" is not a word to use about the shop owner's last way in.
    let mut code = String::with_capacity(CODE_LENGTH);
    let limit = 256 - (256 % ALPHABET.len());
    while code.len() < CODE_LENGTH {
        let mut one = [0u8; 1];
        OsRng.fill_bytes(&mut one);
        let value = usize::from(one[0]);
        if value >= limit {
            continue;
        }
        let index = value % ALPHABET.len();
        match ALPHABET.get(index) {
            Some(&byte) => code.push(char::from(byte)),
            None => return Err(AuthError::HashFailed),
        }
    }

    let code = RecoveryCode(code);
    // Hashed exactly like a PIN, and for the same reason: the stored form must
    // be useless to whoever reads the table.
    let hash = crate::pin::hash_recovery(&code)?;
    Ok((code, hash))
}

/// True when this is the shop's code.
#[must_use]
pub fn verify_recovery_code(typed: &str, hash: &PinHash) -> bool {
    crate::pin::verify_recovery(&RecoveryCode(RecoveryCode::normalise(typed)), hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_round_trips_however_it_is_typed() {
        let (code, hash) = new_recovery_code().expect("generated");
        assert!(verify_recovery_code(code.as_typed(), &hash));
        assert!(verify_recovery_code(&code.to_print(), &hash), "with the dash");
        assert!(
            verify_recovery_code(&code.to_print().to_lowercase(), &hash),
            "typed in lower case"
        );
        assert!(
            verify_recovery_code(&format!(" {} ", code.to_print()), &hash),
            "with spaces"
        );
    }

    #[test]
    fn the_wrong_code_is_refused() {
        let (_, hash) = new_recovery_code().expect("generated");
        assert!(!verify_recovery_code("ABCDE-FGHJK", &hash));
    }

    #[test]
    fn the_alphabet_cannot_be_misread() {
        // The whole reason for a custom alphabet: this is read off a printed
        // slip, by somebody who is annoyed.
        for confusable in ['O', '0', 'I', '1', 'L'] {
            assert!(
                !ALPHABET.contains(&(confusable as u8)),
                "{confusable} can be misread"
            );
        }
        assert_eq!(ALPHABET.len(), 31);
    }

    #[test]
    fn two_codes_are_not_the_same_code() {
        let (a, _) = new_recovery_code().expect("generated");
        let (b, _) = new_recovery_code().expect("generated");
        assert_ne!(a.as_typed(), b.as_typed());
        assert_eq!(a.as_typed().len(), CODE_LENGTH);
    }

    #[test]
    fn it_prints_in_two_readable_halves() {
        let (code, _) = new_recovery_code().expect("generated");
        let printed = code.to_print();
        assert_eq!(printed.len(), CODE_LENGTH + 1);
        assert_eq!(printed.matches('-').count(), 1);
    }

    #[test]
    fn guessing_is_not_a_strategy() {
        // 31^10 ≈ 2^49.5. With an Argon2 verification per guess this is not a
        // number anybody attacks; it is a number they steal the laptop instead.
        // In integers, because D7 forbids floating point in this workspace and
        // a security claim is not the place to make an exception.
        let space = u128::try_from(ALPHABET.len())
            .expect("small")
            .pow(u32::try_from(CODE_LENGTH).expect("small"));
        assert!(space > (1u128 << 48), "{space} is not enough");
    }
}
