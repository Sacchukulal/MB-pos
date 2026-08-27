//! The credential a phone holds.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::Engine as _;

use crate::error::AuthError;
use crate::pin::{PinHash, hash_secret, verify_secret};

/// 32 bytes. Not 16: this is a bearer credential with no second factor and no lockout, and the
/// only thing between it and an attacker is its length.
const SECRET_BYTES: usize = 32;

/// A device credential, in the one moment it exists in plain text.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceSecret(String);

impl DeviceSecret {
    /// For the pairing response.
    #[must_use]
    pub fn to_issue(&self) -> &str {
        &self.0
    }
}

/// A fresh credential and the hash to store.
pub fn new_device_secret() -> Result<(DeviceSecret, PinHash), AuthError> {
    let mut bytes = [0u8; SECRET_BYTES];
    OsRng.fill_bytes(&mut bytes);
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_secret(secret.as_bytes())?;
    Ok((DeviceSecret(secret), hash))
}

/// True when the phone's credential matches the stored hash.
#[must_use]
pub fn verify_device_secret(offered: &str, hash: &PinHash) -> bool {
    verify_secret(offered.as_bytes(), hash)
}

/// A short-lived pairing token, or any other one-use random string.
#[must_use]
pub fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes.clamp(8, 64)];
    OsRng.fill_bytes(&mut buffer);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buffer)
}

/// A short code a person reads off a screen and types into a phone.
#[must_use]
pub fn short_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    const LENGTH: usize = 6;
    let mut code = String::with_capacity(LENGTH + 1);
    // Rejection sampling, so every character is equally likely — the same reasoning as
    // `recovery::new_recovery_code`, and cheap.
    let limit = 256 - (256 % ALPHABET.len());
    while code.chars().filter(char::is_ascii_alphanumeric).count() < LENGTH {
        let mut one = [0u8; 1];
        OsRng.fill_bytes(&mut one);
        let value = usize::from(one[0]);
        if value >= limit {
            continue;
        }
        if let Some(&byte) = ALPHABET.get(value % ALPHABET.len()) {
            code.push(char::from(byte));
        }
        if code.len() == 3 {
            code.push('-');
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T1c. A credential verifies; a wrong one does not; and the stored form is not the
    /// credential.
    #[test]
    fn a_device_credential_verifies_and_is_never_stored_in_the_clear() {
        let (secret, hash) = new_device_secret().expect("issued");
        assert!(verify_device_secret(secret.to_issue(), &hash));
        assert!(!verify_device_secret("not it", &hash));
        assert!(
            !hash.as_str().contains(secret.to_issue()),
            "the credential is inside its own hash"
        );
        // Two devices with two credentials get two different hashes, so reading the table tells
        // an attacker nothing.
        let (_, other) = new_device_secret().expect("issued");
        assert_ne!(hash.as_str(), other.as_str());
    }

    #[test]
    fn a_short_code_can_be_read_across_a_counter() {
        let code = short_code();
        assert_eq!(code.len(), 7, "{code}");
        assert_eq!(code.chars().nth(3), Some('-'));
        for c in code.chars().filter(char::is_ascii_alphanumeric) {
            assert!(
                !matches!(c, 'O' | '0' | 'I' | '1' | 'L'),
                "{c} is a character somebody will read wrong"
            );
        }
        assert_ne!(short_code(), short_code(), "it is not random");
    }

    #[test]
    fn a_token_is_long_and_survives_a_url() {
        let token = random_token(32);
        assert!(token.len() >= 40, "{token}");
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{token} would have to be escaped in a URL or a QR"
        );
    }
}
