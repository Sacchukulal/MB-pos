//! The code support reads out over the phone when a shop's PC has died.

use std::time::Duration;

use mb_core::Timestamp;
use ring::hmac;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The shared secret. See the module note about what this does not protect.
const SUPPORT_SECRET: [u8; 32] = [
    0x6d, 0x62, 0x2d, 0x65, 0x6d, 0x65, 0x72, 0x67, 0x65, 0x6e, 0x63, 0x79, 0x2d, 0x75, 0x6e, 0x6c,
    0x6f, 0x63, 0x6b, 0x2d, 0x50, 0x32, 0x31, 0x2d, 0x64, 0x65, 0x76, 0x2d, 0x6b, 0x65, 0x79, 0x21,
];

/// Crockford's alphabet: no `I`, no `L`, no `O`, no `U`.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Twenty characters — 100 bits.
const CHARS: usize = 20;
/// 24 of those bits are the payload; the other 76 are the tag.
const PAYLOAD_BITS: u32 = 24;
const TAG_BITS: u32 = 76;

/// What support reads out and what an owner types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Code(String);

impl Code {
    /// Grouped for reading aloud: `K7M2Q-9XR4T-BW8HN-3PZ6D`.
    #[must_use]
    pub fn to_read_out(&self) -> String {
        self.0
            .as_bytes()
            .chunks(5)
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// The single-use record. A hash, not the code: `licence.json` sits in a folder a support
    /// engineer may be looking at, and a used code is still a valid code for its machine until
    /// it expires.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let digest = ring::digest::digest(&ring::digest::SHA256, self.0.as_bytes());
        digest
            .as_ref()
            .iter()
            .take(12)
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EmergencyError {
    /// Wrong code, or a code for another computer.
    #[error("that code is not right for this computer")]
    NotRecognised,
    #[error("that code has already been used on this computer")]
    AlreadyUsed,
    #[error("that code has run out")]
    Expired,
    #[error("too many tries")]
    TooManyTries { wait: Duration },
}

/// Five, then a wait.
pub const MAX_TRIES: u32 = 5;
/// Fifteen minutes. A support call lasts longer than that, so an owner who has genuinely
/// mistyped five times and is still on the phone waits once.
pub const LOCKOUT: Duration = Duration::from_secs(15 * 60);

/// Mint one. Support's side, and the tests'.
#[must_use]
pub fn mint(machine: &crate::MachineId, issue_day: i32, hours: u8) -> Code {
    let payload = pack(issue_day, hours);
    let tag = tag_for(machine, payload);
    // 100 bits: the payload in the top 24, the tag in the bottom 76.
    let bits = (u128::from(payload) << TAG_BITS) | tag;
    Code(render(bits))
}

/// Check one, and say when it runs out.
pub fn redeem(
    typed: &str,
    machine: &crate::MachineId,
    now: Timestamp,
    used: &[String],
) -> Result<(Code, Timestamp), EmergencyError> {
    let cleaned = normalise(typed);
    let Some(bits) = parse(&cleaned) else {
        return Err(EmergencyError::NotRecognised);
    };
    let code = Code(cleaned);

    let payload = u32::try_from(bits >> TAG_BITS).unwrap_or(0);
    let tag = bits & ((1_u128 << TAG_BITS) - 1);

    // Constant time, and not `hmac::verify`: that compares against the FULL 32-byte digest and
    // this tag is deliberately truncated to 76 bits, so it would refuse every real code.
    let expected = tag_for(machine, payload);
    if !same_tag(tag, expected) {
        return Err(EmergencyError::NotRecognised);
    }

    if used.contains(&code.fingerprint()) {
        return Err(EmergencyError::AlreadyUsed);
    }

    let (issue_day, hours) = unpack(payload);
    // Valid from the START of its issue day, UTC.
    let until = Timestamp::from_millis(
        i64::from(issue_day)
            .saturating_mul(86_400_000)
            .saturating_add(i64::from(hours).saturating_mul(3_600_000)),
    );
    if now.millis() >= until.millis() {
        return Err(EmergencyError::Expired);
    }
    Ok((code, until))
}

// The bit work. Small, and every piece of it has a test.

/// Eight of the payload's bits are the hours; the rest are the day.
const HOUR_BITS: u32 = 8;
/// The issue day gets whatever is left — 16 bits, so 65,536 days, which is 179 years and
/// therefore not a limit anybody will meet.
const DAY_BITS: u32 = PAYLOAD_BITS - HOUR_BITS;

fn pack(issue_day: i32, hours: u8) -> u32 {
    let wrap = 1_i32 << DAY_BITS;
    let day = u32::try_from(issue_day.rem_euclid(wrap)).unwrap_or(0);
    (day << HOUR_BITS) | u32::from(hours)
}

fn unpack(payload: u32) -> (i32, u8) {
    let day = i32::try_from(payload >> HOUR_BITS).unwrap_or(0);
    let hours = u8::try_from(payload & ((1 << HOUR_BITS) - 1)).unwrap_or(0);
    (day, hours)
}

/// What the HMAC is over.
fn message(machine: &crate::MachineId, payload: u32) -> Vec<u8> {
    let mut message = Vec::with_capacity(machine.value().len() + 4);
    message.extend_from_slice(machine.value().as_bytes());
    message.extend_from_slice(&payload.to_be_bytes());
    message
}

fn tag_for(machine: &crate::MachineId, payload: u32) -> u128 {
    let key = hmac::Key::new(hmac::HMAC_SHA256, &SUPPORT_SECRET);
    let full = hmac::sign(&key, &message(machine, payload));
    let mut top = [0_u8; 16];
    top.copy_from_slice(&full.as_ref()[..16]);
    // The top 76 bits of the digest.
    u128::from_be_bytes(top) >> (128 - TAG_BITS)
}

/// Constant-time comparison of two tags.
fn same_tag(a: u128, b: u128) -> bool {
    let (left, right) = (a.to_be_bytes(), b.to_be_bytes());
    let mut differences = 0_u8;
    for index in 0..left.len() {
        differences |= left[index] ^ right[index];
    }
    std::hint::black_box(differences) == 0
}

fn render(bits: u128) -> String {
    let mut out = String::with_capacity(CHARS);
    for position in (0..CHARS).rev() {
        let shift = u32::try_from(position).unwrap_or(0) * 5;
        let index = usize::try_from((bits >> shift) & 0x1f).unwrap_or(0);
        out.push(char::from(ALPHABET[index.min(31)]));
    }
    out
}

fn parse(cleaned: &str) -> Option<u128> {
    if cleaned.len() != CHARS {
        return None;
    }
    let mut bits: u128 = 0;
    for c in cleaned.bytes() {
        let index = ALPHABET.iter().position(|a| *a == c)?;
        bits = (bits << 5) | u128::try_from(index).ok()?;
    }
    Some(bits)
}

/// What an owner typed, turned into what support said.
#[must_use]
pub fn normalise(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect()
}

/// How long to wait after too many tries.
#[must_use]
pub fn wait_after(tries: u32) -> Option<Duration> {
    if tries >= MAX_TRIES {
        Some(LOCKOUT)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MachineId;

    const DAY: i64 = 86_400_000;

    fn a_machine() -> MachineId {
        MachineId::for_tests("4c4c4544-0043-4a10-8033-b8c04f4d3132")
    }

    fn another_machine() -> MachineId {
        MachineId::for_tests("9f8e7d6c-5b4a-3928-1716-0504030201ff")
    }

    /// Day 20,000 is 2024-10-04 — a real-looking day well inside range.
    const TODAY: i32 = 20_000;

    fn now_on(day: i32, hour: i64) -> Timestamp {
        Timestamp::from_millis(i64::from(day) * DAY + hour * 3_600_000)
    }

    #[test]
    fn a_minted_code_is_twenty_characters_in_four_groups() {
        let code = mint(&a_machine(), TODAY, 72);
        assert_eq!(code.0.len(), CHARS);
        let read_out = code.to_read_out();
        assert_eq!(read_out.len(), CHARS + 3);
        assert_eq!(read_out.matches('-').count(), 3);
        for group in read_out.split('-') {
            assert_eq!(group.len(), 5);
        }
        // And nothing in it can be misheard.
        for c in code.0.chars() {
            assert!(!"ILOU".contains(c), "{c} is not in Crockford base32");
        }
    }

    #[test]
    fn a_good_code_is_redeemed_and_carries_its_own_expiry() {
        let code = mint(&a_machine(), TODAY, 72);
        let (_, until) = redeem(&code.to_read_out(), &a_machine(), now_on(TODAY, 10), &[])
            .expect("it should be accepted");
        assert_eq!(until, now_on(TODAY, 72));
    }

    /// A code minted for another machine is refused.
    #[test]
    fn a_code_for_another_computer_is_refused() {
        let theirs = mint(&another_machine(), TODAY, 72);
        assert_eq!(
            redeem(&theirs.to_read_out(), &a_machine(), now_on(TODAY, 1), &[]),
            Err(EmergencyError::NotRecognised)
        );
    }

    /// Single use. A replay is refused.
    #[test]
    fn a_replay_is_refused() {
        let code = mint(&a_machine(), TODAY, 72);
        let (redeemed, _) =
            redeem(&code.to_read_out(), &a_machine(), now_on(TODAY, 1), &[]).expect("first use");
        let used = vec![redeemed.fingerprint()];
        assert_eq!(
            redeem(&code.to_read_out(), &a_machine(), now_on(TODAY, 2), &used),
            Err(EmergencyError::AlreadyUsed)
        );
    }

    /// Time limited. 72 hours from the start of its issue day.
    #[test]
    fn a_code_runs_out() {
        let code = mint(&a_machine(), TODAY, 72);
        assert!(
            redeem(
                &code.to_read_out(),
                &a_machine(),
                now_on(TODAY + 2, 23),
                &[]
            )
            .is_ok()
        );
        assert_eq!(
            redeem(&code.to_read_out(), &a_machine(), now_on(TODAY + 3, 1), &[]),
            Err(EmergencyError::Expired)
        );
    }

    /// An owner types what they heard, in whatever shape.
    #[test]
    fn spacing_case_and_the_two_classic_mishearings_are_forgiven() {
        let code = mint(&a_machine(), TODAY, 72);
        let spoken = code.to_read_out();
        for variant in [
            spoken.to_lowercase(),
            spoken.replace('-', " "),
            spoken.replace('-', ""),
            format!("  {spoken}  "),
            spoken.replace('0', "O"),
            // ...and "one", and typed an l.
            spoken.replace('1', "l"),
        ] {
            assert!(
                redeem(&variant, &a_machine(), now_on(TODAY, 1), &[]).is_ok(),
                "{variant:?} was refused"
            );
        }
    }

    #[test]
    fn nonsense_is_refused_rather_than_crashing() {
        for junk in [
            "",
            "hello",
            "K7M2Q-9XR4T",
            &"Z".repeat(200),
            "!!!!!!!!!!!!!!!!!!!!",
        ] {
            assert_eq!(
                redeem(junk, &a_machine(), now_on(TODAY, 1), &[]),
                Err(EmergencyError::NotRecognised),
                "{junk:?}"
            );
        }
    }

    /// One character wrong is a different code, and a different code fails.
    #[test]
    fn a_single_wrong_character_is_refused() {
        let code = mint(&a_machine(), TODAY, 72).0;
        let mut wrong = code.clone();
        let first = wrong.remove(0);
        wrong.insert(0, if first == '7' { '8' } else { '7' });
        assert_ne!(wrong, code);
        assert_eq!(
            redeem(&wrong, &a_machine(), now_on(TODAY, 1), &[]),
            Err(EmergencyError::NotRecognised)
        );
    }

    #[test]
    fn the_sixth_try_in_a_row_is_made_to_wait() {
        assert_eq!(wait_after(0), None);
        assert_eq!(wait_after(4), None);
        assert_eq!(wait_after(MAX_TRIES), Some(LOCKOUT));
        assert_eq!(wait_after(50), Some(LOCKOUT));
    }

    #[test]
    fn the_fingerprint_is_not_the_code() {
        let code = mint(&a_machine(), TODAY, 72);
        let print = code.fingerprint();
        assert_eq!(print.len(), 24);
        assert!(!print.contains(&code.0));
    }

    #[test]
    fn the_payload_round_trips() {
        for (day, hours) in [(0, 1_u8), (TODAY, 72), (65_535, 255), (1, 0)] {
            assert_eq!(unpack(pack(day, hours)), (day, hours));
        }
    }

    /// The bit budget. Twenty characters of five bits each, split between the payload and the
    /// tag with nothing left over and nothing borrowed.
    #[test]
    fn the_bit_budget_adds_up() {
        assert_eq!(PAYLOAD_BITS + TAG_BITS, 100);
        assert_eq!(CHARS * 5, 100);
        assert_eq!(DAY_BITS, 16);
        assert_eq!(ALPHABET.len(), 32);
    }

    #[test]
    fn every_minted_code_is_different_per_day_and_per_machine() {
        let a = mint(&a_machine(), TODAY, 72);
        let b = mint(&a_machine(), TODAY + 1, 72);
        let c = mint(&another_machine(), TODAY, 72);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }
}
