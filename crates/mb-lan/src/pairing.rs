//! Letting a phone in, and the two things that must both be true.

use std::collections::HashMap;
use std::sync::Mutex;

use mb_core::Timestamp;

use crate::counter::Refusal;

/// Five minutes. Long enough to walk a phone over from the kitchen, short enough that a
/// photograph of the screen is worthless by the end of service.
pub const TOKEN_LIFETIME_SECONDS: i64 = 300;

/// A phone that presented a good token and has not been let in yet: it is waiting for somebody
/// at the counter to say whose it is and press Allow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    pub request_id: String,
    pub name: String,
    pub platform: String,
    pub install: Option<String>,
    pub ip: String,
    pub asked_at: Timestamp,
}

/// The pairing desk: the code being shown, and the phones queueing at it.
///
/// The code stays up for as long as the panel shows it, and it ROTATES the moment a phone uses
/// it: a screenshot of the screen is worth one presentation, never two — and the next waiter in
/// the queue scans a fresh code without anybody pressing "Add a phone" again.
#[derive(Debug)]
pub struct Desk {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// The token on screen, if the panel is showing one.
    open: Option<OpenToken>,
    /// Phones that presented a good token and are not yet in.
    waiting: Vec<Waiting>,
    /// Request_id -> the credential a person approved, waiting to be collected by the phone
    /// that is polling.
    approved: HashMap<String, crate::counter::PairedDevice>,
    /// Request_id -> refused, so the phone is told rather than left polling.
    refused: Vec<String>,
}

#[derive(Debug, Clone)]
struct OpenToken {
    token: String,
    code: String,
    opened_at: Timestamp,
}

impl OpenToken {
    fn fresh(now: Timestamp) -> OpenToken {
        // 16 bytes is 128 bits for a token that lives five minutes and is used once.
        OpenToken {
            token: mb_auth::random_token(16),
            code: mb_auth::short_code(),
            opened_at: now,
        }
    }
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
    pub fn open(&self, now: Timestamp) -> (String, String) {
        let fresh = OpenToken::fresh(now);
        let pair = (fresh.token.clone(), fresh.code.clone());
        lock(&self.inner).open = Some(fresh);
        pair
    }

    /// Stop showing it. Called when the panel closes.
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

    /// A phone presented a token or a code. A good one is spent on the spot and the panel
    /// gets a fresh one, so the queue behind this phone keeps moving.
    pub fn present(
        &self,
        offered: &str,
        name: &str,
        platform: &str,
        install: Option<&str>,
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
        // Spent. The panel keeps showing — a new code — until somebody closes it.
        inner.open = Some(OpenToken::fresh(now));

        let request_id = mb_auth::random_token(12);
        inner.waiting.push(Waiting {
            request_id: request_id.clone(),
            name: name.trim().to_owned(),
            platform: platform.to_owned(),
            install: install.map(str::to_owned),
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

    /// One of them, still in the queue.
    #[must_use]
    pub fn peek(&self, request_id: &str) -> Option<Waiting> {
        lock(&self.inner)
            .waiting
            .iter()
            .find(|w| w.request_id == request_id)
            .cloned()
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

/// A poisoned pairing desk is not a reason to stop the counter serving.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_millis(seconds * 1_000)
    }

    /// A token expires, and a used one cannot be used twice.
    #[test]
    fn a_token_is_single_use_and_short_lived() {
        let desk = Desk::new();
        let (token, _) = desk.open(at(0));

        // Used once.
        desk.present(&token, "Ravi's phone", "android", None, "192.168.1.31", at(10))
            .expect("the first phone is queued");
        assert_eq!(
            desk.present(&token, "A stranger", "android", None, "192.168.1.99", at(11)),
            Err(Refusal::BadToken),
            "a token that pairs twice is a screenshot that pairs a stranger"
        );

        // A fresh token, left on screen too long.
        let (stale, _) = desk.open(at(0));
        assert!(desk.showing(at(TOKEN_LIFETIME_SECONDS + 1)).is_none());
        assert_eq!(
            desk.present(
                &stale,
                "Late phone",
                "android",
                None,
                "1.2.3.4",
                at(TOKEN_LIFETIME_SECONDS + 1)
            ),
            Err(Refusal::BadToken)
        );
    }

    /// The panel keeps showing after a phone uses the code — a NEW code, so the next waiter
    /// in the queue scans without anybody pressing "Add a phone" again.
    #[test]
    fn a_used_code_is_replaced_and_the_panel_stays_open() {
        let desk = Desk::new();
        let (first, first_code) = desk.open(at(0));
        desk.present(&first, "Ravi's phone", "android", None, "1.2.3.4", at(5))
            .expect("queued");
        let (second, second_code) = desk.showing(at(6)).expect("still showing");
        assert_ne!(first, second, "the spent token is still on the screen");
        assert_ne!(first_code, second_code);
        desk.present(&second, "Anita's phone", "android", None, "1.2.3.5", at(7))
            .expect("the next phone pairs on the fresh code");
        assert_eq!(desk.waiting().len(), 2);
    }

    #[test]
    fn the_short_code_is_forgiving_about_how_it_is_typed() {
        let desk = Desk::new();
        let (_, code) = desk.open(at(0));
        let sloppy = code.to_lowercase().replace('-', " ");
        assert!(
            desk.present(&sloppy, "Phone", "android", None, "1.2.3.4", at(1))
                .is_ok(),
            "{code} typed as {sloppy} was refused"
        );
    }

    /// Nothing is issued until a person presses Allow, or the phone proves its person.
    #[test]
    fn a_good_token_is_not_a_credential() {
        let desk = Desk::new();
        let (token, _) = desk.open(at(0));
        let request = desk
            .present(&token, "Kitchen tablet", "android", None, "1.2.3.4", at(1))
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
            .present(&token, "Unknown phone", "android", None, "1.2.3.4", at(1))
            .expect("queued");
        desk.refuse(&request);
        assert_eq!(desk.collect(&request), Err(Refusal::BadToken));
        assert!(desk.waiting().is_empty());
    }
}
