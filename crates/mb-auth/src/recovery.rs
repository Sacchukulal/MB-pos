use argon2::password_hash::rand_core::{OsRng, RngCore};

use crate::error::AuthError;
use crate::pin::PinHash;

/// No `O`, `0`, `I`, `1` or `l`.
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// Ten characters, in two groups of five, so it can be read aloud.
pub const CODE_LENGTH: usize = 10;
const GROUP: usize = 5;

/// A recovery code, in the one moment it exists in plain text.
#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryCode(String);

impl RecoveryCode {
    /// For the slip and for the one-time screen.
    #[must_use]
    pub fn to_print(&self) -> String {
        let (left, right) = self.0.split_at(GROUP);
        format!("{left}-{right}")
    }

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
pub fn new_recovery_code() -> Result<(RecoveryCode, PinHash), AuthError> {
    let mut bytes = [0u8; CODE_LENGTH];
    OsRng.fill_bytes(&mut bytes);

    // Modulo bias: the alphabet is 31 characters and a byte is 256 values, so the first eleven
    // characters are very slightly likelier than the rest.
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
    // Hashed exactly like a PIN, and for the same reason: the stored form must be useless to
    // whoever reads the table.
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
        assert!(
            verify_recovery_code(&code.to_print(), &hash),
            "with the dash"
        );
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
        // The whole reason for a custom alphabet: this is read off a printed slip, by somebody
        // who is annoyed.
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
        // 31^10 ≈ 2^49.5. With an Argon2 verification per guess this is not a number anybody
        // attacks; it is a number they steal the laptop instead.
        let space = u128::try_from(ALPHABET.len())
            .expect("small")
            .pow(u32::try_from(CODE_LENGTH).expect("small"));
        assert!(space > (1u128 << 48), "{space} is not enough");
    }
}
