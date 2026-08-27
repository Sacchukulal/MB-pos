//! The PIN, and the only decision that actually matters about it.

use std::fmt;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::error::AuthError;

/// A PIN is four digits.
pub const PIN_DIGITS: usize = 4;

/// The cost decision. OWASP's Argon2id minimum: 19 MiB, two passes, one lane.
const MEMORY_KIB: u32 = 19 * 1024;
const ITERATIONS: u32 = 2;
const LANES: u32 = 1;

fn argon() -> Result<Argon2<'static>, AuthError> {
    let params =
        Params::new(MEMORY_KIB, ITERATIONS, LANES, None).map_err(|_| AuthError::HashFailed)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// A PIN somebody typed, already checked for shape and never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Pin(String);

impl Pin {
    /// The rule, with a sentence for each way of breaking it.
    pub fn parse(typed: &str) -> Result<Pin, AuthError> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Err(AuthError::BadPin {
                what: "nothing was typed",
            });
        }
        if !typed.chars().all(|c| c.is_ascii_digit()) {
            return Err(AuthError::BadPin {
                what: "digits only",
            });
        }
        if typed.len() < PIN_DIGITS {
            return Err(AuthError::BadPin {
                what: "that is too short",
            });
        }
        if typed.len() > PIN_DIGITS {
            return Err(AuthError::BadPin {
                what: "that is too long",
            });
        }
        Ok(Pin(typed.to_owned()))
    }

    /// For `hash_pin` and `verify_pin`, and for nothing else.
    fn expose(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// The whole reason this type exists.
impl fmt::Debug for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pin(******)")
    }
}

/// A stored PIN, as a PHC string — `$argon2id$v=19$m=19456,t=2,p=1$…`.
#[derive(Clone, PartialEq, Eq)]
pub struct PinHash(String);

impl PinHash {
    /// The column value. Safe to store; useless to read.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Take one back out of the database.
    pub fn from_stored(stored: &str) -> Result<PinHash, AuthError> {
        let parsed = PasswordHash::new(stored).map_err(|_| AuthError::BadHash)?;
        if parsed.algorithm != argon2::ARGON2ID_IDENT || parsed.hash.is_none() {
            return Err(AuthError::BadHash);
        }
        Ok(PinHash(stored.to_owned()))
    }
}

impl fmt::Debug for PinHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PinHash(argon2id$…)")
    }
}

pub fn hash_pin(pin: &Pin) -> Result<PinHash, AuthError> {
    hash_secret(pin.expose())
}

/// The same Argon2, for a secret that is not a PIN.
pub fn hash_secret(secret: &[u8]) -> Result<PinHash, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon()?
        .hash_password(secret, &salt)
        .map_err(|_| AuthError::HashFailed)?;
    Ok(PinHash(hash.to_string()))
}

/// True when the secret matches.
#[must_use]
pub fn verify_secret(offered: &[u8], hash: &PinHash) -> bool {
    let Ok(parsed) = PasswordHash::new(&hash.0) else {
        return false;
    };
    let Ok(argon) = argon() else {
        return false;
    };
    argon.verify_password(offered, &parsed).is_ok()
}

/// True when the PIN matches.
#[must_use]
pub fn verify_pin(pin: &Pin, hash: &PinHash) -> bool {
    verify_secret(pin.expose(), hash)
}

// The recovery code borrows this machinery, and only this machinery.

/// The recovery code is hashed exactly like a PIN — same algorithm, same cost, same per-shop
/// salt — and these two functions exist so that `crate::recovery` never has to reach for
/// `argon2` itself.
pub(crate) fn hash_recovery(code: &crate::recovery::RecoveryCode) -> Result<PinHash, AuthError> {
    hash_secret(code.as_typed().as_bytes())
}

pub(crate) fn verify_recovery(code: &crate::recovery::RecoveryCode, hash: &PinHash) -> bool {
    verify_secret(code.as_typed().as_bytes(), hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_round_trips() {
        let pin = Pin::parse("2468").expect("four digits");
        let hash = hash_pin(&pin).expect("hashes");
        assert!(verify_pin(&pin, &hash));
    }

    #[test]
    fn the_wrong_pin_is_refused() {
        let hash = hash_pin(&Pin::parse("2468").expect("valid")).expect("hashes");
        assert!(!verify_pin(&Pin::parse("2469").expect("valid"), &hash));
    }

    /// A PIN is four digits, and every other length is refused.
    #[test]
    fn a_pin_is_four_digits_and_no_other_length() {
        assert!(Pin::parse("1234").is_ok(), "four digits is what a PIN is");

        assert_eq!(
            Pin::parse("123"),
            Err(AuthError::BadPin {
                what: "that is too short"
            })
        );
        for too_long in ["12345", "123456", "12345678"] {
            assert_eq!(
                Pin::parse(too_long),
                Err(AuthError::BadPin {
                    what: "that is too long"
                }),
                "{too_long:?} was accepted"
            );
        }
    }

    /// A PIN that is longer than four cannot be verified either, because it cannot be parsed,
    /// and parsing is the only door into `verify_pin`.
    #[test]
    fn a_longer_pin_that_already_exists_is_now_a_recovery_job() {
        assert!(
            Pin::parse("482913").is_err(),
            "six digits is not a PIN any more"
        );
    }

    #[test]
    fn a_pin_is_digits_only() {
        assert!(Pin::parse("12a4").is_err());
        assert!(Pin::parse("").is_err());
        assert!(Pin::parse("12 4").is_err());
    }

    #[test]
    fn the_same_pin_twice_stores_differently() {
        // Per-person salt. Two staff who both chose 1111 must not have equal rows, or the table
        // leaks who shares a PIN.
        let pin = Pin::parse("1111").expect("valid");
        let a = hash_pin(&pin).expect("hashes");
        let b = hash_pin(&pin).expect("hashes");
        assert_ne!(a.as_str(), b.as_str());
        assert!(verify_pin(&pin, &a) && verify_pin(&pin, &b));
    }

    #[test]
    fn a_pin_never_prints_itself() {
        let pin = Pin::parse("2468").expect("valid");
        let printed = format!("{pin:?}");
        assert!(!printed.contains("2468"), "{printed}");
        assert_eq!(printed, "Pin(******)");

        let hash = hash_pin(&pin).expect("hashes");
        let printed = format!("{hash:?}");
        assert!(!printed.contains("2468"), "{printed}");
        // And the stored form does not contain it either — that is what a hash is, but it is
        // worth one line to say so out loud.
        assert!(!hash.as_str().contains("2468"));
    }

    /// What the screen actually says when the shape is wrong.
    #[test]
    fn the_refusal_says_the_rule_that_is_actually_in_force() {
        let said = Pin::parse("12345").expect_err("too long").to_string();
        assert!(said.contains("4 digits"), "{said}");
        assert!(!said.contains('6') && !said.contains('8'), "{said}");
    }

    #[test]
    fn a_corrupted_hash_is_a_locked_door_not_an_open_one() {
        // Each of these PARSES as PHC.
        for corrupt in [
            "$argon2id$broken",
            "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ", // salt, no hash
            "",
            "not a hash at all",
        ] {
            assert_eq!(
                PinHash::from_stored(corrupt),
                Err(AuthError::BadHash),
                "{corrupt:?} was accepted"
            );
        }
        let truncated = PinHash("$argon2id$v=19$m=19456,t=2".to_owned());
        assert!(!verify_pin(&Pin::parse("1234").expect("valid"), &truncated));
    }

    #[test]
    fn a_hash_from_another_algorithm_is_refused() {
        // A shop restored from a build that used something else — or a row somebody pasted in —
        // must not be treated as a PIN we can check.
        let bcrypt_shaped = "$2b$12$abcdefghijklmnopqrstuv";
        assert_eq!(PinHash::from_stored(bcrypt_shaped), Err(AuthError::BadHash));
    }

    #[test]
    fn the_stored_form_carries_its_own_parameters() {
        // Which is why raising the cost later does not invalidate a shop's existing PINs.
        let hash = hash_pin(&Pin::parse("1357").expect("valid")).expect("hashes");
        assert!(
            hash.as_str().starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{}",
            hash.as_str()
        );
        assert!(PinHash::from_stored(hash.as_str()).is_ok());
    }
}
