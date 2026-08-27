//! Where a new row's id comes from.

use mb_core::Timestamp;

/// How many random characters go on the end.
const TAIL: usize = 10;

/// The same, when the caller already has the timestamp it is writing with.
#[must_use]
pub fn fresh_at(prefix: &str, at: Timestamp) -> String {
    format!("{prefix}_{}_{}", base36(at.millis()), tail())
}

/// The random half on its own, for the two things in the product that need uniqueness but are
/// not ids: a purchase order's default NUMBER (which a person reads) and a backup's file name.
#[must_use]
pub fn tail_only() -> String {
    tail()
}

/// Lower-case base 36, so an id stays short and stays sortable.
fn base36(mut value: i64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value <= 0 {
        // A clock before 1970 is a machine with its date wrong, not a reason to fail.
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
fn tail() -> String {
    // Ask for plenty and keep what survives the filter — 24 bytes is 32 base64 characters, of
    // which well over ten are always alphanumeric.
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
    // Never return something shorter than asked for.
    format!("{kept:0<TAIL$}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn an_id_wears_its_prefix() {
        let id = fresh_at("adj", crate::flows::now());
        assert!(id.starts_with("adj_"), "{id}");
    }

    /// The bug, as a test.
    #[test]
    fn ten_thousand_ids_made_at_once_are_ten_thousand_different_ids() {
        let made: HashSet<String> = (0..10_000)
            .map(|_| fresh_at("exp", crate::flows::now()))
            .collect();
        assert_eq!(made.len(), 10_000, "two ids collided");
    }

    /// The same millisecond, stated outright rather than hoped for.
    #[test]
    fn two_ids_from_one_timestamp_are_still_two_ids() {
        let at = Timestamp::from_millis(1_786_894_318_627);
        let made: HashSet<String> = (0..1_000).map(|_| fresh_at("crp", at)).collect();
        assert_eq!(made.len(), 1_000, "the clock is still deciding the id");
    }

    /// Sortable, because that is what the clock was giving us and it is worth keeping.
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
            let id = fresh_at("stk", crate::flows::now());
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{id} has something in it that a person would mistype",
            );
        }
    }

    #[test]
    fn the_random_tail_is_always_the_full_length() {
        for _ in 0..200 {
            let id = fresh_at("x", crate::flows::now());
            let tail = id.rsplit('_').next().unwrap_or_default();
            assert_eq!(tail.len(), TAIL, "{id}");
        }
    }

    #[test]
    fn base36_is_base36() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(
            base36(-1),
            "0",
            "a machine with its date wrong is not a panic"
        );
    }
}
