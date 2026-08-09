//! **The credential a phone holds** — P19, and the same reasoning as a PIN
//! with one thing changed.
//!
//! A PIN is short because a person types it at a counter under pressure, and
//! [`crate::lockout`] is what makes four digits survivable. A device credential
//! is never typed by anybody: it is issued once, over TLS, while a person
//! watches, and then it lives in the phone's keystore. So it is **32 random
//! bytes**, and there is no lockout to design — guessing 2^256 is not a
//! strategy, and each guess still costs a full Argon2 verification.
//!
//! # Why it is hashed at all, when it is already unguessable
//!
//! Because `magic-bill`'s database is **copied to a pen drive on purpose**
//! (P05) and restored onto other machines. A plaintext credential in it is a
//! database that hands out access to whoever finds the drive — including a
//! future ex-employee who took the backup home. The Argon2 cost is irrelevant
//! against a 256-bit secret; storing only the hash is not.
//!
//! # What this does NOT defend against
//!
//! Somebody who takes the phone. The credential is in its keystore and the
//! phone is in a waiter's apron. That is why the counter can **revoke** a
//! device and why revocation bites on the next request rather than the next
//! login — see `mb-lan`, and the `lan_devices` table's own note.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::Engine as _;

use crate::error::AuthError;
use crate::pin::{PinHash, hash_secret, verify_secret};

/// 32 bytes. Not 16: this is a bearer credential with no second factor and no
/// lockout, and the only thing between it and an attacker is its length.
const SECRET_BYTES: usize = 32;

/// A device credential, in the one moment it exists in plain text.
///
/// No `Debug`, deliberately, and the only way it leaves this process is
/// [`DeviceSecret::to_issue`] — which is called by the pairing response and by
/// nothing else. Being forced to name that call is the point (the same trick
/// [`crate::recovery::RecoveryCode`] uses).
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceSecret(String);

impl DeviceSecret {
    /// For the pairing response. Nowhere else.
    #[must_use]
    pub fn to_issue(&self) -> &str {
        &self.0
    }
}

/// A fresh credential and the hash to store.
///
/// Returns both, because the plain text must not be recoverable afterwards:
/// the caller sends one to the phone and stores the other, and there is no
/// third option.
///
/// # Errors
///
/// If Argon2 cannot hash it.
pub fn new_device_secret() -> Result<(DeviceSecret, PinHash), AuthError> {
    let mut bytes = [0u8; SECRET_BYTES];
    OsRng.fill_bytes(&mut bytes);
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_secret(secret.as_bytes())?;
    Ok((DeviceSecret(secret), hash))
}

/// True when the phone's credential matches the stored hash.
///
/// A plain `bool`, like [`crate::verify_pin`]: the caller must not be able to
/// tell "this device does not exist" from "that is the wrong secret", because
/// the difference is a way to enumerate the shop's devices.
#[must_use]
pub fn verify_device_secret(offered: &str, hash: &PinHash) -> bool {
    verify_secret(offered.as_bytes(), hash)
}

/// A short-lived pairing token, or any other one-use random string.
///
/// Base64url so it survives a QR, a URL and a copy-paste unchanged.
#[must_use]
pub fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes.clamp(8, 64)];
    OsRng.fill_bytes(&mut buffer);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buffer)
}

/// A short code a person reads off a screen and types into a phone.
///
/// Same alphabet as the recovery code and for the same reason: no `O`/`0`, no
/// `I`/`1`/`l`, because this is read aloud across a counter in a noisy room.
#[must_use]
pub fn short_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    const LENGTH: usize = 6;
    let mut code = String::with_capacity(LENGTH + 1);
    // Rejection sampling, so every character is equally likely — the same
    // reasoning as `recovery::new_recovery_code`, and cheap.
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

    /// **T1c.** A credential verifies; a wrong one does not; and the stored
    /// form is not the credential.
    #[test]
    fn a_device_credential_verifies_and_is_never_stored_in_the_clear() {
        let (secret, hash) = new_device_secret().expect("issued");
        assert!(verify_device_secret(secret.to_issue(), &hash));
        assert!(!verify_device_secret("not it", &hash));
        assert!(
            !hash.as_str().contains(secret.to_issue()),
            "the credential is inside its own hash"
        );
        // Two devices with two credentials get two different hashes, so
        // reading the table tells an attacker nothing.
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
            token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{token} would have to be escaped in a URL or a QR"
        );
    }
}
