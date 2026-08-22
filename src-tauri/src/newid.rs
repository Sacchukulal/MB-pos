//! **Where a new row's id comes from.**
//!
//! # The bug this exists to end
//!
//! Twenty-odd places built an id out of the clock — `format!("adj_{}",
//! at.millis())` and its cousins. Two rows saved in the same thousandth of a
//! second get the same id, and two rows cannot share one.
//!
//! Found by accident: the test suite failed twice, on two different screens,
//! with `UNIQUE constraint failed: credit_adjustments.id` and then
//! `purchases.id`. A computer is fast enough to hit it; a person on one till
//! very rarely is. **Two tills in one shop are, and this product supports two
//! tills** — `mb_core::ids` says so in its own first paragraph:
//!
//! > *"An autoincrement id collides the moment two machines create a row at the
//! > same second, and there is no way to repair that afterwards without
//! > renumbering history."*
//!
//! Text ids were chosen for exactly this reason and then filled in from the
//! clock anyway, which is the same collision with extra steps.
//!
//! # Why it was worse than an error message
//!
//! Most writes are a plain insert, so a collision is refused and the shopkeeper
//! reads *"The shop's data could not be read."* Ugly, and nothing is lost.
//!
//! **`expenses` is an upsert**, because saving an edited spend reuses its id.
//! So a collision there did not refuse — the second spend silently **replaced**
//! the first. Money quietly missing from a day's list, with nothing on screen
//! to say why. That is the half of this bug that was worth stopping.
//!
//! # What an id looks like now
//!
//! `adj_mt4bb0ee_7fk3x9qz`
//!
//! * the prefix, so a row is recognisable in a log or a database browser;
//! * the clock in base 36, so ids still sort into the order they were made,
//!   which is what made the clock attractive in the first place;
//! * **and a random tail**, which is the part that makes a collision a
//!   coincidence nobody will ever see rather than a busy Saturday.
//!
//! # What this is NOT for
//!
//! An id that is *derived* from something real, and must be. `close_{day}` and
//! `float_{day}` are built from the business day on purpose — one close per
//! day, and a second one has to collide so the database refuses it. Randomness
//! there would quietly allow two closes for one day. They are left alone, and
//! `scripts/check-ids.mjs` knows about them.

use mb_core::Timestamp;

/// How many random characters go on the end.
///
/// Ten of base 36 is about 51 bits. Two ids made in the same millisecond
/// collide with a probability of roughly one in two thousand million million —
/// which is the point at which "it will not happen" stops being optimism.
const TAIL: usize = 10;

/// **A fresh id for a new row.** The only way one is made.
///
/// `prefix` is the short word that says what the row is: `adj`, `exp`, `ord`.
/// It is not checked, because a bad one is a typo somebody reads once in a log
/// rather than a bug that reaches a shop.
#[must_use]
pub fn fresh(prefix: &str) -> String {
    format!("{prefix}_{}_{}", base36(crate::flows::now().millis()), tail())
}

/// The same, when the caller already has the timestamp it is writing with.
///
/// Most callers do — the row's `at` and its id should agree about when it was
/// made, and re-reading the clock here would let them differ by a millisecond
/// for no reason.
#[must_use]
pub fn fresh_at(prefix: &str, at: Timestamp) -> String {
    format!("{prefix}_{}_{}", base36(at.millis()), tail())
}

/// **The random half on its own**, for the two things in the product that need
/// uniqueness but are not ids: a purchase order's default NUMBER (which a
/// person reads) and a backup's file name.
#[must_use]
pub fn tail_only() -> String {
    tail()
}

/// Lower-case base 36, so an id stays short and stays sortable.
fn base36(mut value: i64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value <= 0 {
        // A clock before 1970 is a machine with its date wrong, not a reason to
        // fail. The random tail still makes the id unique.
        return "0".to_owned();
    }
    let mut out = Vec::new();
    while value > 0 {
        let index = usize::try_from(value % 36).unwrap_or(0);
        out.push(*DIGITS.get(index).unwrap_or(&b'0'));
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_else(|_| "0".to_owned())
}

/// The random half.
///
/// `mb_auth::random_token` is the product's one source of randomness (it is
/// `OsRng`), and borrowing it here rather than adding a second one is the same
/// argument `mb_auth::pin::hash_secret` makes about Argon2: two sources of
/// randomness is one place for the weaker one to hide.
///
/// Its output is base64url, which carries `-` and `_` and both cases. Those are
/// filtered out and the rest lower-cased, because an id ends up in log lines,
/// file names and the occasional support screenshot, and a mixed-case id with
/// punctuation in it is one somebody transcribes wrong.
fn tail() -> String {
    // Ask for plenty and keep what survives the filter — 24 bytes is 32 base64
    // characters, of which well over ten are always alphanumeric.
    let raw = mb_auth::random_token(24);
    let kept: String = raw
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .take(TAIL)
        .collect();
    if kept.len() == TAIL {
        return kept;
    }
    // **Never return something shorter than asked for.** Unreachable with 24
    // bytes, and a short tail is a weaker id rather than an obvious failure, so
    // it is padded from the clock rather than trusted.
    format!("{kept:0<TAIL$}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn an_id_wears_its_prefix() {
        let id = fresh("adj");
        assert!(id.starts_with("adj_"), "{id}");
    }

    /// **The bug, as a test.** Ten thousand ids made as fast as the machine can
    /// — which is far faster than a shop, and is exactly the condition that
    /// made the suite flake.
    #[test]
    fn ten_thousand_ids_made_at_once_are_ten_thousand_different_ids() {
        let made: HashSet<String> = (0..10_000).map(|_| fresh("exp")).collect();
        assert_eq!(made.len(), 10_000, "two ids collided");
    }

    /// The same millisecond, stated outright rather than hoped for.
    #[test]
    fn two_ids_from_one_timestamp_are_still_two_ids() {
        let at = Timestamp::from_millis(1_786_894_318_627);
        let made: HashSet<String> = (0..1_000).map(|_| fresh_at("crp", at)).collect();
        assert_eq!(made.len(), 1_000, "the clock is still deciding the id");
    }

    /// Sortable, because that is what the clock was giving us and it is worth
    /// keeping. Base 36 of a growing number grows.
    #[test]
    fn ids_made_later_sort_after_ids_made_earlier() {
        let early = fresh_at("ord", Timestamp::from_millis(1_700_000_000_000));
        let late = fresh_at("ord", Timestamp::from_millis(1_800_000_000_000));
        assert!(early < late, "{early} should sort before {late}");
    }

    /// An id ends up in log lines and support screenshots.
    #[test]
    fn an_id_is_lower_case_letters_and_digits_and_underscores() {
        for _ in 0..200 {
            let id = fresh("stk");
            assert!(
                id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{id} has something in it that a person would mistype",
            );
        }
    }

    #[test]
    fn the_random_tail_is_always_the_full_length() {
        for _ in 0..200 {
            let id = fresh("x");
            let tail = id.rsplit('_').next().unwrap_or_default();
            assert_eq!(tail.len(), TAIL, "{id}");
        }
    }

    #[test]
    fn base36_is_base36() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(base36(-1), "0", "a machine with its date wrong is not a panic");
    }
}
