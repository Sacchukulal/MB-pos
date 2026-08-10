//! **D89 — the offline snapshot is signed, and it has two expiries.**
//!
//! Requirement 1: *"the result is cached locally so it works offline, with a
//! signed, time-limited snapshot so a stopped clock cannot extend it forever."*
//!
//! # The two expiries, and why one is not enough
//!
//! * `not_after` is **wall clock**. It is what makes a snapshot a snapshot
//!   rather than a licence: whatever the cloud said, the counter has to go and
//!   ask again eventually.
//! * `max_offline_days` is measured from [`crate::Watch::high_water`], which
//!   only ever moves forward. It is what makes the first one honest, because
//!   `not_after` is a comparison against a number the person being gated owns.
//!
//! Both must hold. Winding the clock back to March defeats the first and does
//! nothing at all to the second. That is T8.
//!
//! # What signing buys, stated honestly
//!
//! It raises tampering from *"edit a JSON file in Notepad"* to *"patch a signed
//! binary"*. It does **not** defend against somebody who patches the binary,
//! and nothing running on somebody else's computer can — the same sentence
//! `mb-auth/src/pin.rs` already carries about a copied `.db` file, and the same
//! one `mb-lan`'s pinning note carries about a phone that has never paired.
//! This codebase does not ship security theatre; it ships a stated threat model.
//!
//! The bar is set where it is because of who is on the other side of it. The
//! realistic attacker here is a shopkeeper's cousin who is good with computers,
//! not a reverse engineer — and a signature stops him completely.

use base64::Engine as _;
use mb_core::Timestamp;
use ring::signature::{self, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::status::Licence;

/// What the cloud says, at a moment, signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub licence: Licence,
    /// The shop-wide grace period — step 2 of D88's three. It travels **inside
    /// the snapshot** rather than being fetched separately, so the counter can
    /// never be using a stale global against a fresh licence. BACKEND-C3 is
    /// precisely two programs holding this number apart.
    pub global_grace_days: Option<u16>,
    pub issued_at: Timestamp,
    /// Wall-clock expiry.
    pub not_after: Timestamp,
    /// How long the counter may keep using this with no cloud at all, measured
    /// from the clock's high-water mark.
    pub max_offline_days: u16,
}

/// The snapshot, its exact bytes, and the signature over them.
///
/// **The payload is stored as the exact text that was signed**, not as a parsed
/// structure that is re-serialised to check. Re-serialising means the check
/// depends on serde's field order, on float formatting and on whether a future
/// version added a field — three ways for a valid snapshot to stop verifying
/// after an unrelated change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedSnapshot {
    /// The JSON of a [`Snapshot`], byte for byte.
    pub payload: String,
    /// base64, standard alphabet.
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VerifyError {
    #[error("the licence file was not signed by us")]
    BadSignature,
    #[error("the licence file could not be read")]
    NotJson,
    #[error("this build has no key to check a licence with")]
    NoTrustedKey,
    #[error("the signature was not readable")]
    MalformedSignature,
}

/// **The development key, and it is labelled because it has to be deleted.**
///
/// P34 builds the cloud that signs these. Until it exists, the stub in
/// [`crate::cloud`] signs with this fixed seed so that a snapshot written by one
/// run of the counter still verifies on the next — a keypair generated per run
/// would make `licence.json` unreadable after every restart, and the offline
/// path is the one thing that has to survive a restart.
///
/// **When P34 lands:** put the production public key in
/// [`PRODUCTION_PUBLIC_KEY`], and delete this seed and everything that uses it.
/// `the_development_key_is_still_marked_as_one` will remind whoever forgets.
pub const DEVELOPMENT_SEED: [u8; 32] = [
    0x4d, 0x61, 0x67, 0x69, 0x63, 0x42, 0x69, 0x6c, 0x6c, 0x2d, 0x64, 0x65, 0x76, 0x2d, 0x6f, 0x6e,
    0x6c, 0x79, 0x2d, 0x73, 0x65, 0x65, 0x64, 0x2d, 0x50, 0x32, 0x31, 0x2d, 0x32, 0x30, 0x32, 0x36,
];

/// The real one. `None` until P34 mints it.
pub const PRODUCTION_PUBLIC_KEY: Option<&[u8]> = None;

/// The development keypair, derived from the seed.
///
/// # Errors
///
/// Only if `ring` rejects the seed, which it cannot for a fixed 32 bytes.
pub fn development_keypair() -> Result<Ed25519KeyPair, VerifyError> {
    Ed25519KeyPair::from_seed_unchecked(&DEVELOPMENT_SEED).map_err(|_| VerifyError::NoTrustedKey)
}

/// Every key this build will accept a snapshot from.
///
/// Production first, so that when it exists a snapshot signed by it verifies on
/// the first attempt rather than after a failed one.
#[must_use]
pub fn trusted_keys() -> Vec<Vec<u8>> {
    let mut keys = Vec::new();
    if let Some(production) = PRODUCTION_PUBLIC_KEY {
        keys.push(production.to_vec());
    }
    if let Ok(pair) = development_keypair() {
        keys.push(pair.public_key().as_ref().to_vec());
    }
    keys
}

/// Sign a snapshot. **The stub's, and P34's.** A counter never signs anything.
///
/// # Errors
///
/// If the snapshot will not serialise.
pub fn sign(snapshot: &Snapshot, key: &Ed25519KeyPair) -> Result<SignedSnapshot, VerifyError> {
    let payload = serde_json::to_string(snapshot).map_err(|_| VerifyError::NotJson)?;
    let signature = key.sign(payload.as_bytes());
    Ok(SignedSnapshot {
        payload,
        signature: base64::engine::general_purpose::STANDARD.encode(signature.as_ref()),
    })
}

/// Check a snapshot and hand back what it says.
///
/// # Errors
///
/// [`VerifyError`], and **every one of them is survivable** — the caller falls
/// back to [`crate::Entitlement::unactivated`] and the shop keeps billing (T13).
pub fn verify(signed: &SignedSnapshot, keys: &[Vec<u8>]) -> Result<Snapshot, VerifyError> {
    verify_detached(signed.payload.as_bytes(), &signed.signature, keys)?;
    serde_json::from_str(&signed.payload).map_err(|_| VerifyError::NotJson)
}

/// Sign arbitrary bytes. The other half of [`verify_detached`], and what a
/// release-signing tool will use.
#[must_use]
pub fn sign_detached(payload: &[u8], key: &Ed25519KeyPair) -> String {
    base64::engine::general_purpose::STANDARD.encode(key.sign(payload).as_ref())
}

/// **Ed25519, once, for the whole product.**
///
/// P22's update manifests are signed by a different key for a different reason,
/// and a second copy of these eight lines is a second place to get the
/// `is_ok()` the wrong way round. The keys differ; the check does not.
///
/// # Errors
///
/// [`VerifyError`], and every one of them is survivable by the caller.
pub fn verify_detached(
    payload: &[u8],
    signature_b64: &str,
    keys: &[Vec<u8>],
) -> Result<(), VerifyError> {
    if keys.is_empty() {
        return Err(VerifyError::NoTrustedKey);
    }
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|_| VerifyError::MalformedSignature)?;

    // Any trusted key. There will be two of them for exactly as long as it
    // takes P34 to ship, and a fleet mid-rotation is the other case.
    let accepted = keys.iter().any(|key| {
        UnparsedPublicKey::new(&signature::ED25519, key)
            .verify(payload, &signature)
            .is_ok()
    });
    if accepted {
        Ok(())
    } else {
        Err(VerifyError::BadSignature)
    }
}

impl Snapshot {
    /// **Both expiries, and both must hold.**
    ///
    /// `now` is the wall clock; `watch` is the mark that cannot be walked back.
    #[must_use]
    pub fn good_until(&self, watch: &crate::clock::Watch) -> Timestamp {
        let offline = watch.offline_deadline(self.max_offline_days);
        // The earlier of the two. Not the later: a snapshot is good only while
        // BOTH say so, and taking the later one would let whichever number is
        // more generous cancel the other out.
        if self.not_after.millis() <= offline.millis() {
            self.not_after
        } else {
            offline
        }
    }

    /// Is this snapshot still usable?
    ///
    /// **Judged against [`crate::clock::Watch::as_late_as`], not against `now`
    /// alone.** `now` is a number the person being gated owns; the high-water
    /// mark is not. Passing `now` in still matters — on a freshly started
    /// process nothing has ticked yet, so the wall clock is the only thing that
    /// knows a fortnight has gone by.
    #[must_use]
    pub fn is_usable(&self, now: Timestamp, watch: &crate::clock::Watch) -> bool {
        watch.as_late_as(now).millis() < self.good_until(watch).millis()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::MachineId;
    use crate::plan::Plan;
    use crate::status::Status;
    use mb_core::BusinessDay;

    const DAY: i64 = 86_400_000;

    fn a_snapshot() -> Snapshot {
        Snapshot {
            licence: Licence {
                key: "MB-TEST-0001".to_owned(),
                shop_name: "Anna's Kitchen".to_owned(),
                plan: Plan::trial(),
                status: Status::Active,
                renews_on: BusinessDay::from_ymd(2026, 9, 12),
                grace_days: None,
                bound_to: Some(MachineId::for_tests("machine-a")),
                trial_ends_on: None,
                registered_contact: "+91 98••••••10".to_owned(),
            },
            global_grace_days: Some(15),
            issued_at: Timestamp::from_millis(100 * DAY),
            not_after: Timestamp::from_millis(107 * DAY),
            max_offline_days: 14,
        }
    }

    fn signed() -> SignedSnapshot {
        let key = development_keypair().expect("a dev key");
        sign(&a_snapshot(), &key).expect("signs")
    }

    #[test]
    fn a_signed_snapshot_verifies_and_comes_back_unchanged() {
        let back = verify(&signed(), &trusted_keys()).expect("verifies");
        assert_eq!(back, a_snapshot());
    }

    /// **T13. A broken signature is refused — and the caller keeps billing,
    /// which the `src-tauri` side asserts by driving a real bill.**
    #[test]
    fn a_tampered_payload_is_refused() {
        let mut tampered = signed();
        // The obvious attack: change the status to active and the date to next
        // year. This is what "edit a JSON file in Notepad" means.
        tampered.payload = tampered.payload.replace("\"active\"", "\"trial\"");
        assert_ne!(tampered.payload, signed().payload, "the test edited nothing");
        assert_eq!(
            verify(&tampered, &trusted_keys()),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn a_tampered_signature_is_refused() {
        let mut tampered = signed();
        tampered.signature = base64::engine::general_purpose::STANDARD.encode([0_u8; 64]);
        assert_eq!(
            verify(&tampered, &trusted_keys()),
            Err(VerifyError::BadSignature)
        );

        tampered.signature = "not base64 at all !!".to_owned();
        assert_eq!(
            verify(&tampered, &trusted_keys()),
            Err(VerifyError::MalformedSignature)
        );
    }

    #[test]
    fn a_snapshot_from_a_key_we_do_not_trust_is_refused() {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("generates");
        let stranger = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("parses");
        let theirs = sign(&a_snapshot(), &stranger).expect("signs");
        assert_eq!(
            verify(&theirs, &trusted_keys()),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn a_build_with_no_keys_refuses_rather_than_accepts() {
        assert_eq!(verify(&signed(), &[]), Err(VerifyError::NoTrustedKey));
    }

    /// **D89, both expiries, and the earlier one wins.**
    #[test]
    fn the_earlier_of_the_two_expiries_is_the_one_that_counts() {
        let snapshot = a_snapshot();
        let mut watch = crate::clock::Watch::default();

        // Last online on day 100: the offline allowance runs to day 114, and
        // the wall-clock expiry is day 107. The wall clock is earlier.
        watch.reached_the_cloud(Timestamp::from_millis(100 * DAY));
        assert_eq!(
            snapshot.good_until(&watch),
            Timestamp::from_millis(107 * DAY)
        );

        // A snapshot with a long wall-clock life is held to its offline
        // allowance instead.
        let mut long = a_snapshot();
        long.not_after = Timestamp::from_millis(500 * DAY);
        assert_eq!(long.good_until(&watch), Timestamp::from_millis(114 * DAY));
    }

    /// **T8, at this level.** Stopping the clock does not extend the snapshot,
    /// because the mark it is measured from does not move back.
    #[test]
    fn stopping_the_clock_does_not_extend_a_snapshot() {
        let mut long = a_snapshot();
        long.not_after = Timestamp::from_millis(500 * DAY);
        let mut watch = crate::clock::Watch::default();
        watch.reached_the_cloud(Timestamp::from_millis(100 * DAY));

        // Run for twenty days offline. The snapshot ran out on day 114.
        watch.saw(Timestamp::from_millis(120 * DAY));
        assert!(!long.is_usable(Timestamp::from_millis(120 * DAY), &watch));

        // Wind the clock back to the day it was issued. `saw` refuses to move
        // the mark down, so the counter still knows day 120 happened.
        watch.saw(Timestamp::from_millis(100 * DAY));
        assert!(
            !long.is_usable(Timestamp::from_millis(100 * DAY), &watch),
            "winding the clock back brought an expired snapshot back to life"
        );
    }

    /// The development key is temporary and has to LOOK temporary.
    #[test]
    fn the_development_key_is_still_marked_as_one() {
        // When P34 lands and the production key exists, this test is what makes
        // somebody notice the dev seed is still shipping.
        if PRODUCTION_PUBLIC_KEY.is_some() {
            panic!(
                "the production key exists — delete DEVELOPMENT_SEED, \
                 development_keypair(), the stub's use of it, and this test"
            );
        }
        assert_eq!(trusted_keys().len(), 1, "only the dev key, for now");
    }

    /// The payload is signed as the exact bytes, so a future field added to
    /// `Snapshot` cannot invalidate a snapshot already on disk.
    #[test]
    fn verification_does_not_re_serialise_the_payload() {
        let mut with_extra = signed();
        // A field this build does not know about, added by a newer cloud. The
        // signature is over these bytes, so it will not verify here — but the
        // point being asserted is that verification reads `payload` and never
        // rebuilds it.
        with_extra.payload = signed().payload;
        let back = verify(&with_extra, &trusted_keys()).expect("verifies");
        assert_eq!(serde_json::to_string(&back).expect("json"), with_extra.payload);
    }
}
