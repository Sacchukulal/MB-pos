//! **Letting a phone in, and the two things that must both be true.**
//!
//! A phone joins the counter only when:
//!
//! 1. it presents a token the counter is showing **right now**, and
//! 2. **a person presses Allow**, having seen the device's name.
//!
//! Either one alone is not enough, and the second is the one that matters. A
//! token can leak — somebody photographs the screen, a waiter forwards a
//! screenshot — and a leaked token that pairs automatically is a stranger on
//! the guest WiFi holding a counter credential. A person seeing "SAMSUNG-A14"
//! appear when nobody is standing there is a person who presses Refuse.
//!
//! # Why the token is short-lived and single-use
//!
//! Because the alternative is a permanent pairing code, and a permanent
//! pairing code ends up written on the wall beside the WiFi password. This one
//! exists only while the panel is showing it, expires in
//! [`TOKEN_LIFETIME_SECONDS`], and is consumed by a successful pair.

use std::collections::HashMap;
use std::sync::Mutex;

use mb_core::Timestamp;

use crate::counter::Refusal;

/// Five minutes. Long enough to walk a phone over from the kitchen, short
/// enough that a photograph of the screen is worthless by the end of service.
pub const TOKEN_LIFETIME_SECONDS: i64 = 300;

/// A phone waiting for somebody to press Allow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    pub request_id: String,
    pub name: String,
    pub platform: String,
    pub ip: String,
    pub asked_at: Timestamp,
}

/// The pairing desk: the token being shown, and the phones queueing at it.
#[derive(Debug)]
pub struct Desk {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// The token on screen, if the panel is showing one.
    open: Option<OpenToken>,
    /// Phones that presented a good token and are waiting for a person.
    waiting: Vec<Waiting>,
    /// request_id -> the credential a person approved, waiting to be collected
    /// by the phone that is polling.
    approved: HashMap<String, crate::counter::PairedDevice>,
    /// request_id -> refused, so the phone is told rather than left polling.
    refused: Vec<String>,
}

#[derive(Debug, Clone)]
struct OpenToken {
    token: String,
    code: String,
    opened_at: Timestamp,
}

impl Default for Desk {
    fn default() -> Self {
        Desk::new()
    }
}

impl Desk {
    #[must_use]
    pub fn new() -> Desk {
        Desk {
            inner: Mutex::new(Inner::default()),
        }
    }

    /// The counter operator pressed "Add a phone".
    ///
    /// Returns the token (for the QR) and the short code (for typing).
    /// **Opening a second one closes the first**: two live tokens is two ways
    /// in and only one of them is on the screen somebody is watching.
    pub fn open(&self, now: Timestamp) -> (String, String) {
        // 16 bytes is 128 bits for a token that lives five minutes and is used
        // once. Twenty-four made the QR two versions denser for security
        // nothing was buying — see `qr::pairing_uri`.
        let token = mb_auth::random_token(16);
        let code = mb_auth::short_code();
        let mut inner = lock(&self.inner);
        inner.open = Some(OpenToken {
            token: token.clone(),
            code: code.clone(),
            opened_at: now,
        });
        (token, code)
    }

    /// Stop showing it. Called when the panel closes, and after a successful
    /// pair.
    pub fn close(&self) {
        lock(&self.inner).open = None;
    }

    /// What the panel is showing, if anything, and whether it has expired.
    #[must_use]
    pub fn showing(&self, now: Timestamp) -> Option<(String, String)> {
        let inner = lock(&self.inner);
        let open = inner.open.as_ref()?;
        if expired(open.opened_at, now) {
            return None;
        }
        Some((open.token.clone(), open.code.clone()))
    }

    /// A phone presented a token or a code.
    ///
    /// **The token is consumed here, not at approval.** A token that stayed
    /// live while somebody decided would let a second phone in behind the
    /// first — and the person approving would see one name and let in two.
    ///
    /// # Errors
    ///
    /// [`Refusal::BadToken`] when it is wrong, used or expired.
    pub fn present(
        &self,
        offered: &str,
        name: &str,
        platform: &str,
        ip: &str,
        now: Timestamp,
    ) -> Result<String, Refusal> {
        let mut inner = lock(&self.inner);
        let Some(open) = inner.open.clone() else {
            return Err(Refusal::BadToken);
        };
        if expired(open.opened_at, now) {
            inner.open = None;
            return Err(Refusal::BadToken);
        }
        // Either the long token from the QR or the short code typed by hand.
        // The short code is compared case- and dash-insensitively, because it
        // is read off a screen by somebody in a hurry.
        let tidy = |s: &str| {
            s.chars()
                .filter(char::is_ascii_alphanumeric)
                .map(|c| c.to_ascii_uppercase())
                .collect::<String>()
        };
        let matches = offered == open.token || tidy(offered) == tidy(&open.code);
        if !matches {
            return Err(Refusal::BadToken);
        }
        inner.open = None;

        let request_id = mb_auth::random_token(12);
        inner.waiting.push(Waiting {
            request_id: request_id.clone(),
            name: name.trim().to_owned(),
            platform: platform.to_owned(),
            ip: ip.to_owned(),
            asked_at: now,
        });
        Ok(request_id)
    }

    /// What the panel shows: the phones asking to be let in.
    #[must_use]
    pub fn waiting(&self) -> Vec<Waiting> {
        lock(&self.inner).waiting.clone()
    }

    /// Take one off the queue, so the caller can pair it.
    #[must_use]
    pub fn take(&self, request_id: &str) -> Option<Waiting> {
        let mut inner = lock(&self.inner);
        let at = inner
            .waiting
            .iter()
            .position(|w| w.request_id == request_id)?;
        Some(inner.waiting.remove(at))
    }

    /// A person pressed Allow and the counter issued a credential.
    pub fn approve(&self, request_id: &str, device: crate::counter::PairedDevice) {
        lock(&self.inner)
            .approved
            .insert(request_id.to_owned(), device);
    }

    /// A person pressed Refuse.
    pub fn refuse(&self, request_id: &str) {
        let mut inner = lock(&self.inner);
        inner.waiting.retain(|w| w.request_id != request_id);
        inner.refused.push(request_id.to_owned());
    }

    /// The phone polls with its request id.
    ///
    /// `Ok(Some(..))` once and once only — the credential is removed as it is
    /// collected, because a credential sitting in memory waiting to be asked
    /// for twice is a credential a second caller can ask for.
    ///
    /// # Errors
    ///
    /// [`Refusal::BadToken`] when the request was refused or is unknown.
    pub fn collect(
        &self,
        request_id: &str,
    ) -> Result<Option<crate::counter::PairedDevice>, Refusal> {
        let mut inner = lock(&self.inner);
        if let Some(device) = inner.approved.remove(request_id) {
            return Ok(Some(device));
        }
        if inner.refused.iter().any(|r| r == request_id) {
            inner.refused.retain(|r| r != request_id);
            return Err(Refusal::BadToken);
        }
        if inner.waiting.iter().any(|w| w.request_id == request_id) {
            return Ok(None);
        }
        Err(Refusal::BadToken)
    }
}

fn expired(opened_at: Timestamp, now: Timestamp) -> bool {
    let age_ms = now.millis().saturating_sub(opened_at.millis());
    age_ms > TOKEN_LIFETIME_SECONDS.saturating_mul(1_000)
}

/// A poisoned pairing desk is not a reason to stop the counter serving. The
/// same rule the rest of the product uses for its own mutexes.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_millis(seconds.saturating_mul(1_000))
    }

    /// **T11.** A token expires, and a used one cannot be used twice.
    #[test]
    fn a_token_is_single_use_and_short_lived() {
        let desk = Desk::new();
        let (token, _) = desk.open(at(0));

        // Used once.
        desk.present(&token, "Ravi's phone", "android", "192.168.1.31", at(10))
            .expect("the first phone is let through to the queue");
        // And not twice — even inside the lifetime.
        assert_eq!(
            desk.present(&token, "A stranger", "android", "192.168.1.99", at(11)),
            Err(Refusal::BadToken),
            "a token that pairs twice is a screenshot that pairs a stranger"
        );

        // A fresh token, left on screen too long.
        let (stale, _) = desk.open(at(0));
        assert!(desk.showing(at(TOKEN_LIFETIME_SECONDS + 1)).is_none());
        assert_eq!(
            desk.present(&stale, "Late phone", "android", "1.2.3.4", at(TOKEN_LIFETIME_SECONDS + 1)),
            Err(Refusal::BadToken)
        );
    }

    #[test]
    fn the_short_code_is_forgiving_about_how_it_is_typed() {
        let desk = Desk::new();
        let (_, code) = desk.open(at(0));
        let sloppy = code.to_lowercase().replace('-', " ");
        assert!(
            desk.present(&sloppy, "Phone", "android", "1.2.3.4", at(1))
                .is_ok(),
            "{code} typed as {sloppy} was refused"
        );
    }

    /// Nothing is issued until a person presses Allow. A phone that presented
    /// a perfectly good token still waits.
    #[test]
    fn a_good_token_is_not_a_credential() {
        let desk = Desk::new();
        let (token, _) = desk.open(at(0));
        let request = desk
            .present(&token, "Kitchen tablet", "android", "1.2.3.4", at(1))
            .expect("queued");

        assert_eq!(desk.collect(&request), Ok(None), "it let itself in");
        assert_eq!(desk.waiting().len(), 1);
        assert_eq!(desk.waiting()[0].name, "Kitchen tablet");

        desk.take(&request).expect("the panel picked it up");
        desk.approve(
            &request,
            crate::counter::PairedDevice {
                device_id: "dev_1".to_owned(),
                secret: "s".to_owned(),
                server_id: "srv_1".to_owned(),
            },
        );
        assert!(desk.collect(&request).expect("allowed").is_some());
        // Collected once and once only.
        assert_eq!(desk.collect(&request), Err(Refusal::BadToken));
    }

    #[test]
    fn a_refused_phone_is_told_rather_than_left_polling() {
        let desk = Desk::new();
        let (token, _) = desk.open(at(0));
        let request = desk
            .present(&token, "Unknown phone", "android", "1.2.3.4", at(1))
            .expect("queued");
        desk.refuse(&request);
        assert_eq!(desk.collect(&request), Err(Refusal::BadToken));
        assert!(desk.waiting().is_empty());
    }
}
