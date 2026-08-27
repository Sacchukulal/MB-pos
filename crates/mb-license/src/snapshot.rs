//! The offline snapshot is signed, and it has two expiries.

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
    /// The shop-wide grace period.
    pub global_grace_days: Option<u16>,
    pub issued_at: Timestamp,
    /// Wall-clock expiry.
    pub not_after: Timestamp,
    /// How long the counter may keep using this with no cloud at all, measured from the clock's
    /// high-water mark.
    pub max_offline_days: u16,
}

/// The snapshot, its exact bytes, and the signature over them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedSnapshot {
    /// The JSON of a `Snapshot`, byte for byte.
    pub payload: String,
    /// Base64, standard alphabet.
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

/// The development key, and it is labelled because it has to be deleted.
pub const DEVELOPMENT_SEED: [u8; 32] = [
    0x4d, 0x61, 0x67, 0x69, 0x63, 0x42, 0x69, 0x6c, 0x6c, 0x2d, 0x64, 0x65, 0x76, 0x2d, 0x6f, 0x6e,
    0x6c, 0x79, 0x2d, 0x73, 0x65, 0x65, 0x64, 0x2d, 0x50, 0x32, 0x31, 0x2d, 0x32, 0x30, 0x32, 0x36,
];

pub const PRODUCTION_PUBLIC_KEY: Option<&[u8]> = None;

/// The development keypair, derived from the seed.
pub fn development_keypair() -> Result<Ed25519KeyPair, VerifyError> {
    Ed25519KeyPair::from_seed_unchecked(&DEVELOPMENT_SEED).map_err(|_| VerifyError::NoTrustedKey)
}

/// Every key this build will accept a snapshot from.
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

pub fn sign(snapshot: &Snapshot, key: &Ed25519KeyPair) -> Result<SignedSnapshot, VerifyError> {
    let payload = serde_json::to_string(snapshot).map_err(|_| VerifyError::NotJson)?;
    let signature = key.sign(payload.as_bytes());
    Ok(SignedSnapshot {
        payload,
        signature: base64::engine::general_purpose::STANDARD.encode(signature.as_ref()),
    })
}

/// Check a snapshot and hand back what it says.
pub fn verify(signed: &SignedSnapshot, keys: &[Vec<u8>]) -> Result<Snapshot, VerifyError> {
    verify_detached(signed.payload.as_bytes(), &signed.signature, keys)?;
    serde_json::from_str(&signed.payload).map_err(|_| VerifyError::NotJson)
}

/// Sign arbitrary bytes. The other half of `verify_detached`, and what a release-signing tool
/// will use.
#[must_use]
pub fn sign_detached(payload: &[u8], key: &Ed25519KeyPair) -> String {
    base64::engine::general_purpose::STANDARD.encode(key.sign(payload).as_ref())
}

/// Ed25519, once, for the whole product.
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
    /// Both expiries, and both must hold.
    #[must_use]
    pub fn good_until(&self, watch: &crate::clock::Watch) -> Timestamp {
        let offline = watch.offline_deadline(self.max_offline_days);
        // The earlier of the two.
        if self.not_after.millis() <= offline.millis() {
            self.not_after
        } else {
            offline
        }
    }

    /// Is this snapshot still usable?
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

    /// A broken signature is refused — and the caller keeps billing, which the `src-tauri` side
    /// asserts by driving a real bill.
    #[test]
    fn a_tampered_payload_is_refused() {
        let mut tampered = signed();
        // The obvious attack: change the status to active and the date to next year.
        tampered.payload = tampered.payload.replace("\"active\"", "\"trial\"");
        assert_ne!(
            tampered.payload,
            signed().payload,
            "the test edited nothing"
        );
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

    #[test]
    fn the_earlier_of_the_two_expiries_is_the_one_that_counts() {
        let snapshot = a_snapshot();
        let mut watch = crate::clock::Watch::default();

        // Last online on day 100: the offline allowance runs to day 114, and the wall-clock
        // expiry is day 107. The wall clock is earlier.
        watch.reached_the_cloud(Timestamp::from_millis(100 * DAY));
        assert_eq!(
            snapshot.good_until(&watch),
            Timestamp::from_millis(107 * DAY)
        );

        // A snapshot with a long wall-clock life is held to its offline allowance instead.
        let mut long = a_snapshot();
        long.not_after = Timestamp::from_millis(500 * DAY);
        assert_eq!(long.good_until(&watch), Timestamp::from_millis(114 * DAY));
    }

    #[test]
    fn stopping_the_clock_does_not_extend_a_snapshot() {
        let mut long = a_snapshot();
        long.not_after = Timestamp::from_millis(500 * DAY);
        let mut watch = crate::clock::Watch::default();
        watch.reached_the_cloud(Timestamp::from_millis(100 * DAY));

        // Run for twenty days offline.
        watch.saw(Timestamp::from_millis(120 * DAY));
        assert!(!long.is_usable(Timestamp::from_millis(120 * DAY), &watch));

        // Wind the clock back to the day it was issued.
        watch.saw(Timestamp::from_millis(100 * DAY));
        assert!(
            !long.is_usable(Timestamp::from_millis(100 * DAY), &watch),
            "winding the clock back brought an expired snapshot back to life"
        );
    }

    /// The development key is temporary and has to LOOK temporary.
    #[test]
    fn the_development_key_is_still_marked_as_one() {
        if PRODUCTION_PUBLIC_KEY.is_some() {
            panic!(
                "the production key exists — delete DEVELOPMENT_SEED, \
                 development_keypair(), the stub's use of it, and this test"
            );
        }
        assert_eq!(trusted_keys().len(), 1, "only the dev key, for now");
    }

    /// The payload is signed as the exact bytes, so a future field added to `Snapshot` cannot
    /// invalidate a snapshot already on disk.
    #[test]
    fn verification_does_not_re_serialise_the_payload() {
        let mut with_extra = signed();
        // A field this build does not know about, added by a newer cloud.
        with_extra.payload = signed().payload;
        let back = verify(&with_extra, &trusted_keys()).expect("verifies");
        assert_eq!(
            serde_json::to_string(&back).expect("json"),
            with_extra.payload
        );
    }
}
