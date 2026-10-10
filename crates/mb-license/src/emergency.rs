//! Offline support grants. Only public verification keys belong in the counter.

use std::time::Duration;

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use mb_core::Timestamp;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{MachineId, snapshot};

const PURPOSE: &str = "magicbill-emergency-v1";
const SCOPE: &str = "existing-plan";
pub const MAX_HOURS: i64 = 255;
const HOUR: i64 = 3_600_000;
const MAX_CODE_BYTES: usize = 8192;

/// Signed authorization, verified again whenever used, including after a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Code {
    pub payload: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    purpose: String,
    scope: String,
    machine: String,
    licence_key: String,
    issued_at: Timestamp,
    not_after: Timestamp,
}

impl Code {
    /// Pasteable case-sensitive token; unlike the retired code it is not read aloud.
    #[must_use]
    pub fn to_read_out(&self) -> String {
        format!(
            "MB-E1.{}.{}",
            URL_SAFE_NO_PAD.encode(self.payload.as_bytes()),
            self.signature
        )
    }

    /// Replay identity is over the signed payload, independent of transport whitespace.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        ring::digest::digest(&ring::digest::SHA256, self.payload.as_bytes())
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Restore only this verified licence's features on this computer.
    pub fn verify(
        &self,
        machine: &MachineId,
        licence_key: &str,
        now: Timestamp,
    ) -> Result<Timestamp, EmergencyError> {
        self.verify_with_keys(machine, licence_key, now, &snapshot::trusted_keys())
    }

    fn verify_with_keys(
        &self,
        machine: &MachineId,
        licence_key: &str,
        now: Timestamp,
        keys: &[Vec<u8>],
    ) -> Result<Timestamp, EmergencyError> {
        if self.payload.len() > MAX_CODE_BYTES || self.signature.len() > 128 {
            return Err(EmergencyError::NotRecognised);
        }
        snapshot::verify_detached(self.payload.as_bytes(), &self.signature, keys)
            .map_err(|_| EmergencyError::NotRecognised)?;
        let grant: Grant =
            serde_json::from_str(&self.payload).map_err(|_| EmergencyError::NotRecognised)?;
        let duration = grant
            .not_after
            .millis()
            .checked_sub(grant.issued_at.millis());
        if grant.purpose != PURPOSE
            || grant.scope != SCOPE
            || grant.machine != machine.value()
            || grant.licence_key != licence_key
            || licence_key.is_empty()
            || grant.issued_at.millis() < 0
            || !duration.is_some_and(|duration| (1..=MAX_HOURS * HOUR).contains(&duration))
            || now.millis() < grant.issued_at.millis()
        {
            return Err(EmergencyError::NotRecognised);
        }
        if now.millis() >= grant.not_after.millis() {
            return Err(EmergencyError::Expired);
        }
        Ok(grant.not_after)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EmergencyError {
    #[error("that support code is not right for this computer and licence")]
    NotRecognised,
    #[error("that code has already been used on this computer")]
    AlreadyUsed,
    #[error("that code has run out")]
    Expired,
    #[error("too many tries")]
    TooManyTries { wait: Duration },
}

pub const MAX_TRIES: u32 = 5;
pub const LOCKOUT: Duration = Duration::from_secs(15 * 60);

pub fn redeem(
    typed: &str,
    machine: &MachineId,
    licence_key: &str,
    now: Timestamp,
    used: &[String],
) -> Result<(Code, Timestamp), EmergencyError> {
    if typed.len() > MAX_CODE_BYTES {
        return Err(EmergencyError::NotRecognised);
    }
    let cleaned: String = typed.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let mut parts = cleaned.split('.');
    if parts.next() != Some("MB-E1") {
        return Err(EmergencyError::NotRecognised);
    }
    let payload = parts.next().ok_or(EmergencyError::NotRecognised)?;
    let signature = parts.next().ok_or(EmergencyError::NotRecognised)?;
    if parts.next().is_some() || STANDARD.decode(signature).is_err() {
        return Err(EmergencyError::NotRecognised);
    }
    let payload = String::from_utf8(
        URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| EmergencyError::NotRecognised)?,
    )
    .map_err(|_| EmergencyError::NotRecognised)?;
    let code = Code {
        payload,
        signature: signature.to_owned(),
    };
    let until = code.verify(machine, licence_key, now)?;
    if used.contains(&code.fingerprint()) {
        return Err(EmergencyError::AlreadyUsed);
    }
    Ok((code, until))
}

#[must_use]
pub fn wait_after(tries: u32) -> Option<Duration> {
    (tries >= MAX_TRIES).then_some(LOCKOUT)
}

/// Development fixtures only. Release counters contain no emergency grant issuer.
#[cfg(any(test, debug_assertions))]
pub fn mint(
    machine: &MachineId,
    licence_key: &str,
    issue_day: i32,
    hours: u8,
) -> Result<Code, snapshot::VerifyError> {
    let issued = i64::from(issue_day) * 86_400_000;
    let grant = Grant {
        purpose: PURPOSE.to_owned(),
        scope: SCOPE.to_owned(),
        machine: machine.value().to_owned(),
        licence_key: licence_key.to_owned(),
        issued_at: Timestamp::from_millis(issued),
        not_after: Timestamp::from_millis(issued + i64::from(hours) * HOUR),
    };
    let payload = serde_json::to_string(&grant).map_err(|_| snapshot::VerifyError::NotJson)?;
    let signature = snapshot::sign_detached(payload.as_bytes(), &snapshot::development_keypair()?);
    Ok(Code { payload, signature })
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEY: &str = "MB-TEST-0001";
    const TODAY: i32 = 20_000;
    fn machine() -> MachineId {
        MachineId::for_tests("test-machine")
    }
    fn now(hour: i64) -> Timestamp {
        Timestamp::from_millis(i64::from(TODAY) * 86_400_000 + hour * HOUR)
    }
    fn code() -> Code {
        mint(&machine(), KEY, TODAY, 72).expect("test grant")
    }

    #[test]
    fn a_support_grant_round_trips_and_survives_transport_whitespace() {
        let code = code();
        let token = format!("  {}\r\n", code.to_read_out().replace('.', ".\n"));
        let (back, until) = redeem(&token, &machine(), KEY, now(10), &[]).expect("redeemed");
        assert_eq!(back, code);
        assert_eq!(until, now(72));
    }

    #[test]
    fn support_tool_fixture_is_accepted_by_the_counter() {
        let grant: Code = serde_json::from_str(include_str!("../tests/fixtures/emergency-v1.json"))
            .expect("support tool fixture");
        let (back, until) =
            redeem(&grant.to_read_out(), &machine(), KEY, now(10), &[]).expect("support protocol");
        assert_eq!(back, grant);
        assert_eq!(until, now(72));
    }

    #[test]
    fn grant_is_bound_to_machine_licence_and_valid_time() {
        let code = code();
        assert_eq!(
            code.verify(&MachineId::for_tests("other"), KEY, now(1)),
            Err(EmergencyError::NotRecognised)
        );
        assert_eq!(
            code.verify(&machine(), "MB-OTHER", now(1)),
            Err(EmergencyError::NotRecognised)
        );
        assert_eq!(
            code.verify(&machine(), KEY, now(-1)),
            Err(EmergencyError::NotRecognised)
        );
        assert_eq!(
            code.verify(&machine(), KEY, now(72)),
            Err(EmergencyError::Expired)
        );
        assert!(code.verify(&machine(), KEY, now(71)).is_ok());
    }

    #[test]
    fn edited_time_scope_machine_key_and_signature_are_rejected() {
        let original = code();
        for (field, replacement) in [
            ("not_after", serde_json::json!(now(240))),
            ("scope", serde_json::json!("all-features")),
            ("machine", serde_json::json!("other")),
            ("licence_key", serde_json::json!("MB-OTHER")),
        ] {
            let mut grant: serde_json::Value =
                serde_json::from_str(&original.payload).expect("json");
            grant[field] = replacement;
            let edited = Code {
                payload: grant.to_string(),
                signature: original.signature.clone(),
            };
            assert_eq!(
                edited.verify(&machine(), KEY, now(1)),
                Err(EmergencyError::NotRecognised),
                "{field}"
            );
        }
        let edited = Code {
            payload: original.payload,
            signature: STANDARD.encode([0_u8; 64]),
        };
        assert_eq!(
            edited.verify(&machine(), KEY, now(1)),
            Err(EmergencyError::NotRecognised)
        );
    }

    #[test]
    fn even_signed_grants_need_the_supported_purpose_scope_and_duration() {
        let original = code();
        for (field, replacement) in [
            ("purpose", serde_json::json!("another-product")),
            ("scope", serde_json::json!("all-features")),
            ("not_after", serde_json::json!(now(256))),
            ("not_after", serde_json::json!(now(0))),
        ] {
            let mut grant: serde_json::Value =
                serde_json::from_str(&original.payload).expect("json");
            grant[field] = replacement;
            let payload = grant.to_string();
            let signature = snapshot::sign_detached(
                payload.as_bytes(),
                &snapshot::development_keypair().expect("key"),
            );
            assert_eq!(
                Code { payload, signature }.verify(&machine(), KEY, now(1)),
                Err(EmergencyError::NotRecognised)
            );
        }
    }

    #[test]
    fn replay_is_refused_and_cannot_extend_expiry() {
        let code = code();
        assert_eq!(
            redeem(
                &code.to_read_out(),
                &machine(),
                KEY,
                now(2),
                &[code.fingerprint()]
            ),
            Err(EmergencyError::AlreadyUsed)
        );
        assert_eq!(
            redeem(&code.to_read_out(), &machine(), KEY, now(72), &[]),
            Err(EmergencyError::Expired)
        );
    }

    #[test]
    fn legacy_and_malformed_codes_fail_closed() {
        for junk in [
            "",
            "K7M2Q-9XR4T-BW8HN-3PZ6D",
            "MB-E1.invalid.invalid",
            "MB-E1.a.b.c",
            &"Z".repeat(9000),
        ] {
            assert_eq!(
                redeem(junk, &machine(), KEY, now(1), &[]),
                Err(EmergencyError::NotRecognised)
            );
        }
    }

    #[test]
    fn release_keys_refuse_development_grants() {
        assert_eq!(
            code().verify_with_keys(&machine(), KEY, now(1), &snapshot::keys_for(true)),
            Err(EmergencyError::NotRecognised)
        );
        assert_eq!(
            code().verify_with_keys(&machine(), KEY, now(1), &[]),
            Err(EmergencyError::NotRecognised)
        );
    }

    #[test]
    fn five_failed_attempts_require_a_wait() {
        assert_eq!(wait_after(4), None);
        assert_eq!(wait_after(MAX_TRIES), Some(LOCKOUT));
    }
}
