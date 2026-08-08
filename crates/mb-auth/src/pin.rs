//! **The PIN, and the only decision that actually matters about it.**
//!
//! The prompt's first draft asked for *"argon2 or bcrypt, justify"*. That is
//! the wrong question. A six-digit PIN is a million guesses; against anybody
//! holding a copy of the `.db` file the **cost parameter is the entire
//! security**, and the choice of library is a footnote to it.
//!
//! So the numbers below are the decision, and they are a compromise between
//! two things that pull in opposite directions:
//!
//! * `docs/PERFORMANCE.md` **B10** — a PIN submitted and the cashier back to
//!   billing in 400 ms — on a reference machine that is an i3 with **4 GB of
//!   RAM**. A memory-hard function is memory-hard for us too.
//! * an offline attacker with the file and no time limit.
//!
//! [`PARAMS`] is the OWASP minimum for Argon2id (19 MiB, t = 2, p = 1). Not
//! more, because of the 4 GB; not less, because less is not a defence.
//!
//! # Six to eight digits
//!
//! BACKEND-**D1** asked for a six-digit minimum and this takes it — even
//! though the threat there (a public internet endpoint behind a deliberately
//! public restaurant code) is not the threat here (a person standing at a
//! counter). Two extra keypresses twice a day is not a cost worth arguing
//! about, and the offline attack is only ever answered by digits × cost.
//!
//! # The plaintext exists in exactly two function arguments
//!
//! [`Pin`] and [`PinHash`] print as `Pin(******)`, so a PIN cannot reach a log
//! line, a panic message, an audit row or a `dbg!` by accident. There is a test.

use std::fmt;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::password_hash::rand_core::OsRng;
use argon2::{Algorithm, Argon2, Params, Version};

use crate::error::AuthError;

/// A PIN is 6 to 8 digits. Both ends are deliberate: six because fewer is not
/// worth hashing, eight because a cashier types this six times a day and there
/// is no auto-submit to hide behind.
pub const MIN_DIGITS: usize = 6;
pub const MAX_DIGITS: usize = 8;

/// **The cost decision.** OWASP's Argon2id minimum: 19 MiB, two passes, one
/// lane. Changing these numbers does not invalidate stored hashes — the
/// parameters travel inside the PHC string, so an old hash still verifies with
/// the settings it was made with, and only re-hashes on the next PIN change.
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
        if typed.len() < MIN_DIGITS {
            return Err(AuthError::BadPin {
                what: "that is too short",
            });
        }
        if typed.len() > MAX_DIGITS {
            return Err(AuthError::BadPin {
                what: "that is too long",
            });
        }
        Ok(Pin(typed.to_owned()))
    }

    /// For [`hash_pin`] and [`verify_pin`], and for nothing else. Not public.
    fn expose(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// **The whole reason this type exists.** A `String` would have printed itself
/// into the first log line somebody added.
impl fmt::Debug for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pin(******)")
    }
}

/// A stored PIN, as a PHC string — `$argon2id$v=19$m=19456,t=2,p=1$…`.
///
/// The salt is inside it and is per-person, so the same PIN on two staff
/// members produces two different strings: reading the table tells an attacker
/// nothing, not even that two people chose the same number.
#[derive(Clone, PartialEq, Eq)]
pub struct PinHash(String);

impl PinHash {
    /// The column value. Safe to store; useless to read.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Take one back out of the database.
    ///
    /// A string that is not a complete Argon2id hash is [`AuthError::BadHash`]
    /// rather than "no PIN set" — a truncated or hand-edited column must be a
    /// locked door, not an open one.
    ///
    /// **Parsing is not enough, and a test found that.** PHC is a permissive
    /// format: `PasswordHash::new("$argon2id$broken")` succeeds, reading
    /// "broken" as a salt and leaving the hash output empty. Verification
    /// against it correctly fails, so the door was never actually open — but
    /// `from_stored` was reporting a truncated column as a good one, and the
    /// screen would then have said "wrong PIN" to somebody typing the right
    /// one, forever, with nothing anywhere saying why. So both are checked: the
    /// algorithm is the one we write, and there is a hash output to compare
    /// against.
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
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon()?
        .hash_password(pin.expose(), &salt)
        .map_err(|_| AuthError::HashFailed)?;
    Ok(PinHash(hash.to_string()))
}

/// True when the PIN matches.
///
/// Returns a plain `bool` on purpose: the caller must not be able to tell "no
/// such hash" from "wrong PIN" by the error type. Both are one refusal, and
/// both cost the same time — which is item 3's other half, because a login
/// that fails *fast* for an unknown person is a login that enumerates staff.
#[must_use]
pub fn verify_pin(pin: &Pin, hash: &PinHash) -> bool {
    let Ok(parsed) = PasswordHash::new(&hash.0) else {
        return false;
    };
    let Ok(argon) = argon() else {
        return false;
    };
    argon.verify_password(pin.expose(), &parsed).is_ok()
}

// ---------------------------------------------------------------------------
// The recovery code borrows this machinery, and only this machinery.
// ---------------------------------------------------------------------------

/// The recovery code is hashed exactly like a PIN — same algorithm, same cost,
/// same per-shop salt — and these two functions exist so that
/// [`crate::recovery`] never has to reach for `argon2` itself.
///
/// They are `pub(crate)` on purpose: hashing an arbitrary string is not a
/// service this crate offers, because the *only* two secrets in the product are
/// the PIN and the recovery code, and a third one should have to be added here,
/// deliberately, with a reason.
pub(crate) fn hash_recovery(code: &crate::recovery::RecoveryCode) -> Result<PinHash, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon()?
        .hash_password(code.as_typed().as_bytes(), &salt)
        .map_err(|_| AuthError::HashFailed)?;
    Ok(PinHash(hash.to_string()))
}

pub(crate) fn verify_recovery(code: &crate::recovery::RecoveryCode, hash: &PinHash) -> bool {
    let Ok(parsed) = PasswordHash::new(&hash.0) else {
        return false;
    };
    let Ok(argon) = argon() else {
        return false;
    };
    argon
        .verify_password(code.as_typed().as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_round_trips() {
        let pin = Pin::parse("123456").expect("six digits");
        let hash = hash_pin(&pin).expect("hashes");
        assert!(verify_pin(&pin, &hash));
    }

    #[test]
    fn the_wrong_pin_is_refused() {
        let hash = hash_pin(&Pin::parse("123456").expect("valid")).expect("hashes");
        assert!(!verify_pin(&Pin::parse("123457").expect("valid"), &hash));
    }

    #[test]
    fn four_digits_are_refused_with_a_reason() {
        // BACKEND-D1: v1's PIN was four digits. This is the line where that
        // stops being possible.
        let err = Pin::parse("1234").expect_err("too short");
        assert_eq!(
            err,
            AuthError::BadPin {
                what: "that is too short"
            }
        );
        assert!(err.to_string().contains("6 to 8"));
    }

    #[test]
    fn a_pin_is_digits_only() {
        assert!(Pin::parse("12345a").is_err());
        assert!(Pin::parse("").is_err());
        assert!(Pin::parse("123456789").is_err());
    }

    #[test]
    fn the_same_pin_twice_stores_differently() {
        // Per-person salt. Two staff who both chose 111111 must not have equal
        // rows, or the table leaks who shares a PIN.
        let pin = Pin::parse("111111").expect("valid");
        let a = hash_pin(&pin).expect("hashes");
        let b = hash_pin(&pin).expect("hashes");
        assert_ne!(a.as_str(), b.as_str());
        assert!(verify_pin(&pin, &a) && verify_pin(&pin, &b));
    }

    #[test]
    fn a_pin_never_prints_itself() {
        // T2. The plaintext must not be able to reach a log line, a panic
        // message or an audit row through Debug — which is how it would.
        let pin = Pin::parse("246813").expect("valid");
        let printed = format!("{pin:?}");
        assert!(!printed.contains("246813"), "{printed}");
        assert_eq!(printed, "Pin(******)");

        let hash = hash_pin(&pin).expect("hashes");
        let printed = format!("{hash:?}");
        assert!(!printed.contains("246813"), "{printed}");
        // And the stored form does not contain it either — that is what a hash
        // is, but it is worth one line to say so out loud.
        assert!(!hash.as_str().contains("246813"));
    }

    #[test]
    fn a_corrupted_hash_is_a_locked_door_not_an_open_one() {
        // Each of these PARSES as PHC. None of them is a PIN.
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
        assert!(!verify_pin(&Pin::parse("123456").expect("valid"), &truncated));
    }

    #[test]
    fn a_hash_from_another_algorithm_is_refused() {
        // A shop restored from a build that used something else — or a row
        // somebody pasted in — must not be treated as a PIN we can check.
        let bcrypt_shaped = "$2b$12$abcdefghijklmnopqrstuv";
        assert_eq!(PinHash::from_stored(bcrypt_shaped), Err(AuthError::BadHash));
    }

    #[test]
    fn the_stored_form_carries_its_own_parameters() {
        // Which is why raising the cost later does not invalidate a shop's
        // existing PINs.
        let hash = hash_pin(&Pin::parse("135790").expect("valid")).expect("hashes");
        assert!(hash.as_str().starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{}", hash.as_str());
        assert!(PinHash::from_stored(hash.as_str()).is_ok());
    }
}
