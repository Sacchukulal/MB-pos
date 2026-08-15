//! **The other end of the wire** — P27, and the first client this crate has
//! ever had.
//!
//! P19 and P20 built a server because the client was an Android phone. A second
//! till is a client too, and it is a Rust one — so this is the same protocol,
//! the same TLS, the same pinned certificate and the same credential, driven
//! from inside the counter instead of from a phone.
//!
//! # It pins, exactly as a phone does (D80)
//!
//! The master's certificate is self-signed, so there is no authority to ask.
//! The secondary is given the certificate at pairing time — from a QR a person
//! held up — and **trusts that one certificate and nothing else**. A stranger
//! on the shop's WiFi cannot impersonate the master to a till that has already
//! joined, and the stated limit is unchanged: a till that has NEVER joined has
//! nothing to compare against.
//!
//! # What it is deliberately not
//!
//! It is not a sync engine. It carries three things — a join, a fact (D136) and
//! an intent (D137) — and every one of them is already idempotent on the other
//! side. There is no diff, no clock, no vector and no merge, because a settled
//! bill is immutable and a table has exactly one owner.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::intent::{Forwarded, Intent, Outcome, Receipt};

/// Why a call to the master did not happen.
///
/// **"The master is off" is not an error a shopkeeper should read as a
/// failure** — it is the ordinary state D138 is built for — so the caller
/// decides how loud each of these is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// Could not reach it at all: off, unplugged, a different network.
    Unreachable(String),
    /// Reached it and it said no — a revoked credential, a full licence.
    Refused { status: u16, message: String },
    /// Reached it and could not understand the answer.
    Unreadable(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Unreachable(why) => write!(f, "the main till could not be reached: {why}"),
            ClientError::Refused { status, message } => {
                write!(f, "the main till said no ({status}): {message}")
            }
            ClientError::Unreadable(why) => {
                write!(f, "the main till answered something unexpected: {why}")
            }
        }
    }
}

impl std::error::Error for ClientError {}

/// What a joined till holds so it can speak again tomorrow.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Credential {
    pub device_id: String,
    pub secret: String,
}

/// The master, as seen from another till.
#[derive(Debug, Clone)]
pub struct Master {
    base: String,
    http: reqwest::Client,
    credential: Option<Credential>,
    /// **The certificate this client is pinned to**, kept so a till that has
    /// just met a master can write down the exact one it checked (D80) rather
    /// than fetching it a second time — a second fetch could return a different
    /// certificate from the one the person's QR proved.
    certificate_pem: String,
}

impl Master {
    /// Build a client that trusts **one** certificate.
    ///
    /// # Errors
    ///
    /// When the certificate cannot be read, which means the pairing details are
    /// damaged rather than that the master is away.
    pub fn pinned(base: &str, certificate_pem: &str) -> Result<Master, ClientError> {
        let der = pem_to_der(certificate_pem)
            .ok_or_else(|| ClientError::Unreadable("that certificate is not readable".to_owned()))?;
        let certificate = reqwest::Certificate::from_der(&der)
            .map_err(|e| ClientError::Unreadable(e.to_string()))?;
        let http = reqwest::Client::builder()
            // **Short, and that is the point.** A till waiting thirty seconds
            // for a master that is switched off is a till a cashier thinks has
            // frozen. D92's rule — every call has a deadline enforced by the
            // CALLER — one crate along.
            .timeout(Duration::from_secs(5))
            .connect_timeout(Duration::from_secs(2))
            .add_root_certificate(certificate)
            .danger_accept_invalid_hostnames(false)
            .build()
            .map_err(|e| ClientError::Unreadable(e.to_string()))?;
        Ok(Master {
            base: base.trim_end_matches('/').to_owned(),
            http,
            credential: None,
            certificate_pem: certificate_pem.to_owned(),
        })
    }

    /// The certificate this client checked and is now pinned to.
    #[must_use]
    pub fn certificate_pem(&self) -> &str {
        &self.certificate_pem
    }

    /// **Meet a master for the first time, with a fingerprint a person
    /// carried** (D80).
    ///
    /// The QR the master is showing holds its address and its fingerprint. This
    /// fetches the certificate itself over a connection that trusts nothing,
    /// checks its fingerprint against the one from the QR, and only then builds
    /// a client pinned to it.
    ///
    /// **The wire is not what makes this safe — the QR is.** A stranger who
    /// answers on that address hands over a certificate whose fingerprint does
    /// not match what the person is holding, and this refuses. That is the same
    /// trust decision a phone makes, made by a till.
    ///
    /// # Errors
    ///
    /// [`ClientError::Unreachable`] when nothing answered, and
    /// [`ClientError::Refused`] when something did and **was not the master** —
    /// which is the one that matters.
    pub async fn meet(base: &str, expected_fingerprint: &str) -> Result<Master, ClientError> {
        let unverified = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .connect_timeout(Duration::from_secs(2))
            // Only for this one call, only to read a certificate we are about
            // to check by hand against the QR.
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(|e| ClientError::Unreadable(e.to_string()))?;
        let hello: serde_json::Value = unverified
            .get(format!("{}/v1/hello", base.trim_end_matches('/')))
            .send()
            .await
            .map_err(|e| ClientError::Unreachable(e.to_string()))?
            .json()
            .await
            .map_err(|e| ClientError::Unreadable(e.to_string()))?;

        let pem = hello
            .get("certificate_pem")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let seen = crate::identity::fingerprint_of(pem).unwrap_or_default();
        if seen.is_empty() || !same_fingerprint(&seen, expected_fingerprint) {
            return Err(ClientError::Refused {
                status: 0,
                message: "That is not the till on the code. Somebody else answered on \
                          this address — show the code again and check the address."
                    .to_owned(),
            });
        }
        Master::pinned(base, pem)
    }

    #[must_use]
    pub fn as_device(mut self, credential: Credential) -> Master {
        self.credential = Some(credential);
        self
    }

    /// Is the master there? Answers without a credential, so a till that has
    /// not joined yet can still ask.
    pub async fn hello(&self) -> Result<serde_json::Value, ClientError> {
        self.get("/v1/hello").await
    }

    /// **Join** — the same pairing a phone does (P19 §3), with a token a person
    /// is holding up on the master's screen.
    ///
    /// Pairing is two steps because **a person is in the middle of it**: the
    /// post asks, and somebody at the master reads the name and presses Allow.
    /// This waits for them, up to `patience`, exactly as the phone's screen
    /// does — so the caller gets a credential or a sentence and never a
    /// half-finished join it has to remember to come back to.
    ///
    /// # Errors
    ///
    /// [`ClientError::Refused`] when the master says no — a spent token, a full
    /// plan (D141), or nobody pressing Allow before `patience` runs out.
    pub async fn join(
        &self,
        token: &str,
        name: &str,
        patience: Duration,
    ) -> Result<Credential, ClientError> {
        let asked: serde_json::Value = self
            .post_json(
                "/v1/pair",
                &serde_json::json!({
                    "token": token,
                    "name": name,
                    // A till, not a phone. The master's pairing panel shows
                    // this, so the person pressing Allow sees what is joining —
                    // and D141 counts a till against a different line.
                    "platform": "till",
                }),
            )
            .await?;

        // The master may approve instantly (a token it was already holding), in
        // which case there is nothing to poll for.
        if let Some(credential) = credential_in(&asked) {
            return Ok(credential);
        }
        let request_id = asked
            .get("request_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ClientError::Unreadable("the main till did not say what to wait for".to_owned())
            })?
            .to_owned();

        let deadline = std::time::Instant::now() + patience;
        loop {
            let status: serde_json::Value = self.get(&format!("/v1/pair/{request_id}")).await?;
            if let Some(credential) = credential_in(&status) {
                return Ok(credential);
            }
            if std::time::Instant::now() >= deadline {
                return Err(ClientError::Refused {
                    status: 0,
                    message: "Nobody allowed this till at the main counter. Show the \
                              code again and ask somebody there to press Allow."
                        .to_owned(),
                });
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// **Send facts** (D136). Retryable for ever: the master is idempotent on
    /// each order's id, so sending twice is the same as sending once.
    pub async fn forward(&self, batch: &Forwarded) -> Result<Receipt, ClientError> {
        self.post_json("/v1/forward", batch).await
    }

    /// **Ask the master to do something to the floor** (D137) — open a table,
    /// add to it, settle it. The answer already carries a sentence a person
    /// reads (D84).
    pub async fn apply(&self, intent: &Intent) -> Result<Outcome, ClientError> {
        self.post_json("/v1/intent", intent).await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        let mut request = self.http.get(format!("{}{path}", self.base));
        request = self.signed(request);
        self.read(request).await
    }

    async fn post_json<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        let mut request = self.http.post(format!("{}{path}", self.base)).json(body);
        request = self.signed(request);
        self.read(request).await
    }

    /// `Authorization: Bearer <device>.<secret>` and the protocol version —
    /// **the same two headers a phone sends**, read by the same `authenticate`
    /// on the other side. A second shape here would be a second door.
    fn signed(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let request =
            request.header("x-magicbill-version", crate::PROTOCOL_VERSION.to_string());
        match &self.credential {
            Some(credential) => request.header(
                "authorization",
                format!("Bearer {}.{}", credential.device_id, credential.secret),
            ),
            None => request,
        }
    }

    async fn read<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, ClientError> {
        let response = request
            .send()
            .await
            .map_err(|e| ClientError::Unreachable(e.to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| ClientError::Unreadable(e.to_string()))?;
        if !status.is_success() {
            // The server's refusals already carry a sentence written for a
            // person (D84); this hands it on rather than composing a second
            // one out of a status code.
            let message = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .or_else(|| v.get("error"))
                        .and_then(|m| m.as_str().map(str::to_owned))
                })
                .unwrap_or_else(|| text.clone());
            return Err(ClientError::Refused {
                status: status.as_u16(),
                message,
            });
        }
        serde_json::from_str(&text).map_err(|e| ClientError::Unreadable(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// The blocking face of it.
// ---------------------------------------------------------------------------

/// **One runtime for every client call this process makes**, built the first
/// time one is made and never on a master that makes none.
///
/// The counter is not an async program: its commands are ordinary functions and
/// its sender is an ordinary thread. Rather than each of them building a runtime
/// — a thread pool created and destroyed around every two-second tick — they
/// share this one, and `mb-lan` keeps the async in the crate that already owns
/// it. `src-tauri` names tokio nowhere, which is the same boundary
/// `server::start` drew for the server.
///
/// **Multi-threaded with one worker, deliberately.** A join waits up to two
/// minutes for a person to press Allow, and on a current-thread runtime that
/// wait would own the driver and stall the sender behind it.
fn shared_runtime() -> Option<&'static tokio::runtime::Runtime> {
    static RUNTIME: std::sync::OnceLock<Option<tokio::runtime::Runtime>> =
        std::sync::OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("mb-lan-client")
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
}

fn on_runtime<T>(
    future: impl std::future::Future<Output = Result<T, ClientError>>,
) -> Result<T, ClientError> {
    let Some(runtime) = shared_runtime() else {
        return Err(ClientError::Unreachable(
            "this till could not start networking".to_owned(),
        ));
    };
    runtime.block_on(future)
}

impl Master {
    /// [`Master::meet`], for a caller with no runtime.
    ///
    /// # Errors
    ///
    /// As [`Master::meet`].
    pub fn meet_blocking(base: &str, expected_fingerprint: &str) -> Result<Master, ClientError> {
        on_runtime(Master::meet(base, expected_fingerprint))
    }

    /// [`Master::join`], for a caller with no runtime.
    ///
    /// # Errors
    ///
    /// As [`Master::join`].
    pub fn join_blocking(
        &self,
        token: &str,
        name: &str,
        patience: Duration,
    ) -> Result<Credential, ClientError> {
        on_runtime(self.join(token, name, patience))
    }

    /// [`Master::forward`], for a caller with no runtime.
    ///
    /// # Errors
    ///
    /// As [`Master::forward`].
    pub fn forward_blocking(&self, batch: &Forwarded) -> Result<Receipt, ClientError> {
        on_runtime(self.forward(batch))
    }

    /// [`Master::apply`], for a caller with no runtime.
    ///
    /// # Errors
    ///
    /// As [`Master::apply`].
    pub fn apply_blocking(&self, intent: &Intent) -> Result<Outcome, ClientError> {
        on_runtime(self.apply(intent))
    }
}

/// A credential out of a pairing answer, when there is one.
///
/// **Both halves or neither.** A body carrying an id and no secret is a body
/// that has not finished being approved, and treating it as a join would leave
/// a till holding an identity it cannot prove.
fn credential_in(body: &serde_json::Value) -> Option<Credential> {
    let device_id = body.get("device_id")?.as_str()?;
    let secret = body.get("secret")?.as_str()?;
    if device_id.is_empty() || secret.is_empty() {
        return None;
    }
    Some(Credential {
        device_id: device_id.to_owned(),
        secret: secret.to_owned(),
    })
}

/// Two fingerprints, however they happen to be written down.
///
/// The QR carries a compact form and `fingerprint_of` produces a colon-
/// separated one, so comparing them literally would refuse every honest join
/// and accept nothing. Case and punctuation are noise; the hex digits are the
/// fingerprint.
fn same_fingerprint(a: &str, b: &str) -> bool {
    let strip = |s: &str| {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect::<String>()
    };
    let (a, b) = (strip(a), strip(b));
    !a.is_empty() && a == b
}

/// The one line of certificate handling this file owns.
fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let body: String = pem
        .lines()
        .skip_while(|l| !l.starts_with("-----BEGIN CERTIFICATE-----"))
        .skip(1)
        .take_while(|l| !l.starts_with("-----END CERTIFICATE-----"))
        .collect();
    if body.is_empty() {
        return None;
    }
    base64::engine::general_purpose::STANDARD.decode(body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_certificate_that_is_not_one_is_refused_before_anything_is_sent() {
        // The pin is the whole security story (D80), so a damaged one must be a
        // refusal here rather than a connection that quietly trusts nothing in
        // particular.
        assert!(matches!(
            Master::pinned("https://127.0.0.1:7331", "not a certificate"),
            Err(ClientError::Unreadable(_))
        ));
    }

    #[test]
    fn being_unable_to_reach_the_master_reads_as_a_state_and_not_a_crash() {
        // D138: "the main till is off" is the ordinary state this feature is
        // built for, and the sentence has to sound like one.
        let says = ClientError::Unreachable("connection refused".to_owned()).to_string();
        assert!(says.contains("could not be reached"), "{says}");
    }
}

#[cfg(test)]
mod fingerprint_tests {
    use super::same_fingerprint;

    /// **The join a QR makes possible, and the one it must refuse.**
    ///
    /// The two sides write a fingerprint differently — the QR compactly, the
    /// certificate reader with colons — so a literal comparison would refuse
    /// every honest till in the country. And a fingerprint that differs by one
    /// character is a stranger answering on the master's address.
    #[test]
    fn punctuation_is_noise_and_a_different_hash_is_a_stranger() {
        assert!(same_fingerprint("AB:CD:12", "abcd12"));
        assert!(same_fingerprint("ab cd 12", "AB-CD-12"));
        assert!(!same_fingerprint("ABCD12", "ABCD13"));
        // An empty one is never a match: "nothing answered" must not read as
        // "it matched".
        assert!(!same_fingerprint("", ""));
        assert!(!same_fingerprint("", "abcd12"));
    }
}
