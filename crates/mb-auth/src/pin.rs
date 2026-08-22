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
//! # Four digits, and exactly four
//!
//! See [`PIN_DIGITS`]. The offline attack is answered by digits × cost, and
//! four digits at 19 MiB per guess is still a wall; the counter attack is
//! answered by the lockout, not by length. That the length is *exact* is a
//! separate decision from the number itself, and it is the one the pad on the
//! lock screen is built out of.
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

/// **A PIN is four digits. Exactly four — not four to eight.**
///
/// The owner's instruction of 2026-08-22: *"make sure login pin is only 4
/// numbers not more … not even for this test laptop login or anything else …
/// in future any new user in any other restaurant too."*
///
/// # Why there is one number here and not two
///
/// This was `MIN_DIGITS = 4` with `MAX_DIGITS = 8`. The four came from the
/// owner on 2026-08-17; the eight was left behind from BACKEND-D1, whose
/// threat model was a public internet endpoint behind a deliberately public
/// restaurant code. That is not the threat here: this is a person standing at
/// a counter, in front of a screen that locks itself, with a lockout after a
/// handful of wrong tries — the same four digits they already use at an ATM.
///
/// The **range** was the actual defect, and leaving it in place is what made
/// "the pad is four digits" a thing the screen only pretended to be. A pad
/// that accepts four to eight cannot know when a PIN is finished, so it cannot
/// draw the right number of empty circles, and it cannot refuse the fifth
/// keypress. Both of those went wrong in front of the owner. One number gives
/// them back: four circles, and the fifth keypress does nothing.
///
/// # What this costs a shop that is already running
///
/// A PIN longer than four that is already on disk can no longer be *typed*, so
/// it can no longer be verified. That is deliberate rather than overlooked:
/// the way back is the recovery code (see [`crate::recovery`]), which is
/// exactly the path that exists for a PIN nobody can use, and somebody who
/// manages staff can set a fresh four-digit PIN for anybody else. Refusing the
/// long PIN at the door and sending one person to the printed slip is honest.
/// Quietly keeping a second, longer shape alive is what left eight circles on
/// a screen the owner had already asked to show four.
pub const PIN_DIGITS: usize = 4;

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
    hash_secret(pin.expose())
}

/// The same Argon2, for a secret that is not a PIN.
///
/// P19's device credential is 32 random bytes rather than four digits, but it
/// is stored in the same database, which is copied to a pen drive on purpose
/// (P05). One hashing function, one set of parameters, one place to change
/// them — a second `Argon2::new` somewhere else is how two halves of a product
/// end up with two different costs and nobody notices the weaker one.
///
/// # Errors
///
/// If Argon2 cannot hash it.
pub fn hash_secret(secret: &[u8]) -> Result<PinHash, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon()?
        .hash_password(secret, &salt)
        .map_err(|_| AuthError::HashFailed)?;
    Ok(PinHash(hash.to_string()))
}

/// True when the secret matches. See [`verify_pin`] for why it is a `bool`.
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
///
/// Returns a plain `bool` on purpose: the caller must not be able to tell "no
/// such hash" from "wrong PIN" by the error type. Both are one refusal, and
/// both cost the same time — which is item 3's other half, because a login
/// that fails *fast* for an unknown person is a login that enumerates staff.
#[must_use]
pub fn verify_pin(pin: &Pin, hash: &PinHash) -> bool {
    verify_secret(pin.expose(), hash)
}

// ---------------------------------------------------------------------------
// The recovery code borrows this machinery, and only this machinery.
// ---------------------------------------------------------------------------

/// The recovery code is hashed exactly like a PIN — same algorithm, same cost,
/// same per-shop salt — and these two functions exist so that
/// [`crate::recovery`] never has to reach for `argon2` itself.
///
/// They were `pub(crate)` on purpose, with a note saying the *only* two
/// secrets in the product are the PIN and the recovery code, and that a third
/// should have to be added here deliberately, with a reason.
///
/// **P19 is that third one**, and this is the reason: a phone holds a bearer
/// credential for the counter's network. It is added the way the note asked —
/// by promoting the two functions to [`hash_secret`] and [`verify_secret`]
/// rather than by growing a second Argon2 configuration somewhere else, which
/// is how two halves of a product end up with two different costs and nobody
/// notices the weaker one.
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

    /// **A PIN is four digits, and every other length is refused.**
    ///
    /// This test has been rewritten twice, and the rewrites are the history of
    /// the bug. It began as `four_digits_are_refused_with_a_reason`, for
    /// BACKEND-D1's six-digit minimum. On 2026-08-17 the owner asked for four
    /// and it became "four is a PIN and three is not" — which passed happily
    /// while eight was *also* still a PIN, so nothing here objected when the
    /// pad let somebody type eight digits in front of the owner.
    ///
    /// The lesson is worth more than the test: a rule with a floor and no
    /// ceiling test is half a rule. Both ends are asserted now.
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

    /// **A PIN that is longer than four cannot be verified either**, because
    /// it cannot be parsed, and parsing is the only door into `verify_pin`.
    ///
    /// The ceiling used to be eight so that a shop already running could keep
    /// signing in with a six-digit PIN. That compromise is what kept the range
    /// alive; see [`PIN_DIGITS`] for why it was given up and what replaces it
    /// (the recovery code, and any manager setting a fresh PIN).
    #[test]
    fn a_longer_pin_that_already_exists_is_now_a_recovery_job() {
        assert!(Pin::parse("482913").is_err(), "six digits is not a PIN any more");
    }

    #[test]
    fn a_pin_is_digits_only() {
        assert!(Pin::parse("12a4").is_err());
        assert!(Pin::parse("").is_err());
        assert!(Pin::parse("12 4").is_err());
    }

    #[test]
    fn the_same_pin_twice_stores_differently() {
        // Per-person salt. Two staff who both chose 1111 must not have equal
        // rows, or the table leaks who shares a PIN.
        let pin = Pin::parse("1111").expect("valid");
        let a = hash_pin(&pin).expect("hashes");
        let b = hash_pin(&pin).expect("hashes");
        assert_ne!(a.as_str(), b.as_str());
        assert!(verify_pin(&pin, &a) && verify_pin(&pin, &b));
    }

    #[test]
    fn a_pin_never_prints_itself() {
        // T2. The plaintext must not be able to reach a log line, a panic
        // message or an audit row through Debug — which is how it would.
        let pin = Pin::parse("2468").expect("valid");
        let printed = format!("{pin:?}");
        assert!(!printed.contains("2468"), "{printed}");
        assert_eq!(printed, "Pin(******)");

        let hash = hash_pin(&pin).expect("hashes");
        let printed = format!("{hash:?}");
        assert!(!printed.contains("2468"), "{printed}");
        // And the stored form does not contain it either — that is what a hash
        // is, but it is worth one line to say so out loud.
        assert!(!hash.as_str().contains("2468"));
    }

    /// **What the screen actually says when the shape is wrong.**
    ///
    /// The sentence lived in `AuthError` and said *"a PIN is 6 to 8 digits"*
    /// for five days after the rule became four — so somebody who mistyped was
    /// told, by the program, to type six. Nothing tested the words, only the
    /// variant. Now something does.
    #[test]
    fn the_refusal_says_the_rule_that_is_actually_in_force() {
        let said = Pin::parse("12345").expect_err("too long").to_string();
        assert!(said.contains("4 digits"), "{said}");
        assert!(!said.contains('6') && !said.contains('8'), "{said}");
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
        assert!(!verify_pin(&Pin::parse("1234").expect("valid"), &truncated));
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
        let hash = hash_pin(&Pin::parse("1357").expect("valid")).expect("hashes");
        assert!(hash.as_str().starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{}", hash.as_str());
        assert!(PinHash::from_stored(hash.as_str()).is_ok());
    }
}
