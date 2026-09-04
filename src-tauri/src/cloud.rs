//! The cloud, over HTTP — the one file in the counter that opens a socket to Magic Bill.
//!
//! `Http` speaks both halves of the contract: the licence office (`mb_license::Cloud`, from
//! `docs/LICENCE_PROTOCOL.md`) and everything under the counter's own login (`Link`, from
//! `MB-backend/docs/SYNC_PROTOCOL.md`). Every call carries the deadline its caller chose;
//! nothing here retries, sleeps, or decides what an answer means for the shop.

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use mb_core::Timestamp;
use mb_license::cloud::{Answer, Ask, Cloud, CloudError, DeviceLogin, Extras, SignedManifest};
use mb_license::SignedSnapshot;
use serde_json::{Value, json};

use crate::log_warn;

/// Where the cloud is. The anon key is public by design (the login is what matters — the
/// protocol says so); it lives in a text file rather than here so the secret scanner in
/// `hygiene_tests` keeps its one rule. `MB_CLOUD_URL` / `MB_CLOUD_ANON_KEY` override both at
/// run time, for the tests and for a staging project.
const CLOUD_URL: &str = include_str!("../cloud/url.txt");
const CLOUD_ANON_KEY: &str = include_str!("../cloud/anon_key.txt");

#[must_use]
pub fn cloud_url() -> String {
    std::env::var("MB_CLOUD_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| CLOUD_URL.to_owned())
        .trim()
        .trim_end_matches('/')
        .to_owned()
}

#[must_use]
pub fn anon_key() -> String {
    std::env::var("MB_CLOUD_ANON_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| CLOUD_ANON_KEY.to_owned())
        .trim()
        .to_owned()
}

/// How long a call under the login may take. The licence calls carry their own deadline.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(8);
/// A download is not a call.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

// What a call under the login can come back with.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// No network, DNS failure, the server is down, the deadline passed.
    #[error("we could not reach our server")]
    Unreachable,
    /// The access token has run out: refresh it and try once more.
    #[error("the counter's login has expired")]
    Unauthorised,
    /// The login is dead — the licence was released, moved or revoked. Stop; do not retry.
    #[error("{0}")]
    Dead(String),
    /// The server said no to this particular request, with a sentence.
    #[error("{0}")]
    Refused(String),
    /// The server had a problem of its own.
    #[error("our server had a problem: {0}")]
    Server(String),
    /// Something this build could not read.
    #[error("our server sent something this version could not read")]
    Unreadable,
}

/// A fresh pair of tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Timestamp,
}

/// One page of a REST read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Page {
    pub rows: Vec<Value>,
    /// The whole count, when the server said.
    pub total: Option<usize>,
}

/// An owner's own login — the account they made at magicbill.in — for the one minute the
/// first run needs it: to ask which shops are theirs. The token is never written down; the
/// counter's own login, handed over when the licence is activated, is what it keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerLogin {
    pub access_token: String,
    /// The name on the account, or the email's first half when the account has none.
    pub name: String,
    pub email: String,
}

/// Everything the sender, the pull, the restore and the updater need from the cloud.
pub trait Link: Send + Sync + std::fmt::Debug + 'static {
    /// `POST /auth/v1/token?grant_type=password`: the owner, by email and password.
    fn password_login(&self, _email: &str, _password: &str) -> Result<OwnerLogin, LinkError> {
        Err(LinkError::Unreachable)
    }
    /// `POST /rest/v1/rpc/{name}` under the login.
    fn rpc(&self, name: &str, body: &Value, token: &str) -> Result<Value, LinkError>;
    /// `GET /rest/v1/{path}` under the login, rows `from..=to`.
    fn rest(&self, path: &str, token: &str, from: usize, to: usize) -> Result<Page, LinkError>;
    /// `POST /functions/v1/{name}` under the login. The one Edge Function the counter calls
    /// this way is `phone-session` — a phone's cloud login, after Allow.
    fn edge(&self, _name: &str, _body: &Value, _token: &str) -> Result<Value, LinkError> {
        Err(LinkError::Unreachable)
    }
    /// A new access token from the refresh token. The old refresh token is spent either way.
    fn refresh_session(&self, refresh_token: &str) -> Result<Session, LinkError>;
    /// Fetch a file to `to`; answers its SHA-256 as lowercase hex.
    fn download(&self, url: &str, to: &Path) -> Result<String, LinkError>;
}

/// The cloud that is not there: what a test `App` starts with.
#[cfg(test)]
#[derive(Debug)]
pub struct NoLink;

#[cfg(test)]
impl Link for NoLink {
    fn rpc(&self, _: &str, _: &Value, _: &str) -> Result<Value, LinkError> {
        Err(LinkError::Unreachable)
    }
    fn rest(&self, _: &str, _: &str, _: usize, _: usize) -> Result<Page, LinkError> {
        Err(LinkError::Unreachable)
    }
    fn refresh_session(&self, _: &str) -> Result<Session, LinkError> {
        Err(LinkError::Unreachable)
    }
    fn download(&self, _: &str, _: &Path) -> Result<String, LinkError> {
        Err(LinkError::Unreachable)
    }
}

// The client.

pub struct Http {
    client: reqwest::Client,
    base: String,
    anon: String,
}

impl std::fmt::Debug for Http {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Http").field("base", &self.base).finish()
    }
}

impl Http {
    /// The shipped cloud.
    #[cfg(not(test))]
    #[must_use]
    pub fn new() -> Arc<Http> {
        Http::at(&cloud_url(), &anon_key())
    }

    /// A cloud somewhere else — a test's fake server.
    #[must_use]
    pub fn at(base: &str, anon: &str) -> Arc<Http> {
        let client = reqwest::Client::builder()
            .user_agent(format!("MagicBill-counter/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Arc::new(Http {
            client,
            base: base.trim_end_matches('/').to_owned(),
            anon: anon.to_owned(),
        })
    }

    fn run<T>(future: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(future)
    }

    /// This computer's name, so the owner's phone can tell two tills apart.
    fn machine_name() -> String {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    }

    /// One licence call. The reply is the status and the body, or nothing at all.
    fn licence_call(&self, op: &str, body: &Value) -> Result<(u16, Value), CloudError> {
        let url = format!("{}/functions/v1/licence/{op}", self.base);
        let request = self
            .client
            .post(&url)
            .header("apikey", &self.anon)
            .timeout(CALL_TIMEOUT)
            .json(body);
        let (status, text) = Http::run(async move {
            let response = request.send().await?;
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            Ok::<_, reqwest::Error>((status, text))
        })
        .map_err(|e| {
            log_warn!("the licence office could not be reached for {op}: {e}");
            CloudError::Unreachable
        })?;
        let value: Value = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).map_err(|_| CloudError::Unreadable)?
        };
        Ok((status, value))
    }

    /// The protocol's error table, applied to a licence reply.
    fn refuse(status: u16, body: &Value) -> CloudError {
        let code = body.get("code").and_then(Value::as_str).unwrap_or("");
        let message = body
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Refused.")
            .to_owned();
        match status {
            401 => CloudError::NotRecognised,
            409 => CloudError::BoundElsewhere {
                machine: body
                    .get("machine")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            },
            403 => CloudError::Refused(message),
            429 if code == "too_soon" => CloudError::TooSoon {
                days_left: u16::try_from(body.get("days_left").and_then(Value::as_u64).unwrap_or(1))
                    .unwrap_or(u16::MAX),
            },
            // Rate limited: the server wrote the sentence.
            429 => CloudError::Refused(message),
            // A 5xx and everything else the protocol does not name: as good as unreachable.
            s if s >= 500 => CloudError::Unreachable,
            _ => CloudError::Unreadable,
        }
    }

    /// A licence reply into an answer.
    fn answer(body: &Value) -> Result<Answer, CloudError> {
        let payload = body
            .get("payload")
            .and_then(Value::as_str)
            .ok_or(CloudError::Unreadable)?;
        let signature = body
            .get("signature")
            .and_then(Value::as_str)
            .ok_or(CloudError::Unreadable)?;
        let device = body.get("device").and_then(|d| {
            let session = d.get("session")?;
            Some(DeviceLogin {
                device_id: d.get("id")?.as_str()?.to_owned(),
                restaurant_id: d.get("restaurant_id")?.as_str()?.to_owned(),
                access_token: session.get("access_token")?.as_str()?.to_owned(),
                refresh_token: session.get("refresh_token")?.as_str()?.to_owned(),
                expires_at: expires_at_of(session),
            })
        });
        let extras: Option<Extras> = match body.get("extras") {
            Some(Value::Null) | None => None,
            Some(e) => serde_json::from_value(e.clone()).ok(),
        };
        Ok(Answer {
            snapshot: SignedSnapshot {
                payload: payload.to_owned(),
                signature: signature.to_owned(),
            },
            device,
            extras,
        })
    }

    /// One request under the login: the status and the body text.
    fn under_login(
        &self,
        request: reqwest::RequestBuilder,
        token: &str,
    ) -> Result<(u16, String, Option<String>), LinkError> {
        let request = request
            .header("apikey", &self.anon)
            .bearer_auth(token)
            .timeout(CALL_TIMEOUT);
        Http::run(async move {
            let response = request.send().await?;
            let status = response.status().as_u16();
            let range = response
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let text = response.text().await.unwrap_or_default();
            Ok::<_, reqwest::Error>((status, text, range))
        })
        .map_err(|e| {
            log_warn!("the cloud could not be reached: {e}");
            LinkError::Unreachable
        })
    }

    /// The `message` of a reply body, or a plain "Refused.".
    fn sentence_of(text: &str) -> String {
        let body: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        body.get("message")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
            .unwrap_or("Refused.")
            .to_owned()
    }

    /// PostgREST's error shape, into the four things a caller can do about it.
    fn link_error(status: u16, text: &str) -> LinkError {
        let body: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let code = body.get("code").and_then(Value::as_str).unwrap_or("");
        let message = body
            .get("message")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
            .unwrap_or("Refused.")
            .to_owned();
        match status {
            401 => LinkError::Unauthorised,
            // `42501` is Postgres for "insufficient privilege": the login no longer belongs to
            // a live counter.
            403 => LinkError::Dead(message),
            _ if code == "42501" => LinkError::Dead(message),
            s if s >= 500 => LinkError::Server(message),
            _ => LinkError::Refused(message),
        }
    }
}

/// Supabase says `expires_at` in seconds.
fn expires_at_of(session: &Value) -> Timestamp {
    session
        .get("expires_at")
        .and_then(Value::as_i64)
        .map_or(Timestamp::EPOCH, |s| {
            Timestamp::from_millis(s.saturating_mul(1000))
        })
}

impl Cloud for Http {
    fn activate(&self, ask: &Ask) -> Result<Answer, CloudError> {
        let (status, body) =
            self.licence_call("activate", &json!({ "ask": ask, "machine_name": Http::machine_name() }))?;
        if status != 200 {
            return Err(Http::refuse(status, &body));
        }
        Http::answer(&body)
    }

    fn refresh(&self, ask: &Ask, want_login: bool) -> Result<Answer, CloudError> {
        let (status, body) = self.licence_call("refresh", &json!({ "ask": ask, "session": want_login }))?;
        if status != 200 {
            return Err(Http::refuse(status, &body));
        }
        Http::answer(&body)
    }

    fn release(&self, ask: &Ask) -> Result<(), CloudError> {
        let (status, body) = self.licence_call("release", &json!({ "ask": ask }))?;
        match status {
            200 | 204 => Ok(()),
            _ => Err(Http::refuse(status, &body)),
        }
    }

    fn transfer(&self, ask: &Ask) -> Result<Answer, CloudError> {
        let (status, body) =
            self.licence_call("transfer", &json!({ "ask": ask, "machine_name": Http::machine_name() }))?;
        if status != 200 {
            return Err(Http::refuse(status, &body));
        }
        Http::answer(&body)
    }
}

impl Link for Http {
    fn password_login(&self, email: &str, password: &str) -> Result<OwnerLogin, LinkError> {
        let url = format!("{}/auth/v1/token?grant_type=password", self.base);
        let request = self
            .client
            .post(&url)
            .header("apikey", &self.anon)
            .timeout(CALL_TIMEOUT)
            .json(&json!({ "email": email, "password": password }));
        let (status, text) = Http::run(async move {
            let response = request.send().await?;
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            Ok::<_, reqwest::Error>((status, text))
        })
        .map_err(|e| {
            log_warn!("the sign-in could not reach our server: {e}");
            LinkError::Unreachable
        })?;
        match status {
            200 => {}
            429 => {
                return Err(LinkError::Refused(
                    "Too many tries. Wait a minute, then sign in again.".to_owned(),
                ));
            }
            s if s >= 500 => return Err(Http::link_error(status, &text)),
            // A wrong password, an unknown email, a disabled account: one sentence, because
            // saying which is telling a stranger which emails have accounts.
            _ => {
                return Err(LinkError::Refused(
                    "That email and password do not match a Magic Bill account. Check them, \
                     or reset the password at magicbill.in."
                        .to_owned(),
                ));
            }
        }
        let body: Value = serde_json::from_str(&text).map_err(|_| LinkError::Unreadable)?;
        let access_token = body
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or(LinkError::Unreadable)?
            .to_owned();
        let user = body.get("user").cloned().unwrap_or(Value::Null);
        let email = user
            .get("email")
            .and_then(Value::as_str)
            .unwrap_or(email)
            .to_owned();
        let name = ["name", "full_name"]
            .iter()
            .filter_map(|k| user.get("user_metadata")?.get(k)?.as_str())
            .map(str::trim)
            .find(|n| !n.is_empty())
            .map_or_else(
                || email.split('@').next().unwrap_or("Owner").to_owned(),
                str::to_owned,
            );
        Ok(OwnerLogin {
            access_token,
            name,
            email,
        })
    }

    fn rpc(&self, name: &str, body: &Value, token: &str) -> Result<Value, LinkError> {
        let url = format!("{}/rest/v1/rpc/{name}", self.base);
        let request = self.client.post(&url).json(body);
        let (status, text, _) = self.under_login(request, token)?;
        if !(200..300).contains(&status) {
            return Err(Http::link_error(status, &text));
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|_| LinkError::Unreadable)
    }

    fn edge(&self, name: &str, body: &Value, token: &str) -> Result<Value, LinkError> {
        let url = format!("{}/functions/v1/{name}", self.base);
        let request = self.client.post(&url).json(body);
        let (status, text, _) = self.under_login(request, token)?;
        if !(200..300).contains(&status) {
            // A function writes its own sentence; 401 is the login and is refreshed like any
            // other call. Everything else is the function's answer, not the login's.
            return Err(match status {
                401 => LinkError::Unauthorised,
                s if s >= 500 => LinkError::Server(Http::sentence_of(&text)),
                _ => LinkError::Refused(Http::sentence_of(&text)),
            });
        }
        serde_json::from_str(&text).map_err(|_| LinkError::Unreadable)
    }

    fn rest(&self, path: &str, token: &str, from: usize, to: usize) -> Result<Page, LinkError> {
        let url = format!("{}/rest/v1/{path}", self.base);
        let request = self
            .client
            .get(&url)
            .header("Range-Unit", "items")
            .header("Range", format!("{from}-{to}"))
            .header("Prefer", "count=exact");
        let (status, text, range) = self.under_login(request, token)?;
        if !(200..300).contains(&status) {
            return Err(Http::link_error(status, &text));
        }
        let rows: Vec<Value> = serde_json::from_str(&text).map_err(|_| LinkError::Unreadable)?;
        // `Content-Range: 0-999/5000`, or `*/0` for nothing at all.
        let total = range
            .as_deref()
            .and_then(|r| r.rsplit('/').next())
            .and_then(|n| n.parse().ok());
        Ok(Page { rows, total })
    }

    fn refresh_session(&self, refresh_token: &str) -> Result<Session, LinkError> {
        let url = format!("{}/auth/v1/token?grant_type=refresh_token", self.base);
        let request = self
            .client
            .post(&url)
            .header("apikey", &self.anon)
            .timeout(CALL_TIMEOUT)
            .json(&json!({ "refresh_token": refresh_token }));
        let (status, text) = Http::run(async move {
            let response = request.send().await?;
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            Ok::<_, reqwest::Error>((status, text))
        })
        .map_err(|e| {
            log_warn!("the login could not be refreshed: {e}");
            LinkError::Unreachable
        })?;
        if status >= 500 {
            return Err(Http::link_error(status, &text));
        }
        if status != 200 {
            // A spent or unknown refresh token: this login is over. The next licence check
            // asks for a new one.
            return Err(LinkError::Dead(
                "The counter's login to the cloud has run out. It is renewed at the next licence check."
                    .to_owned(),
            ));
        }
        let body: Value = serde_json::from_str(&text).map_err(|_| LinkError::Unreadable)?;
        Ok(Session {
            access_token: body
                .get("access_token")
                .and_then(Value::as_str)
                .ok_or(LinkError::Unreadable)?
                .to_owned(),
            refresh_token: body
                .get("refresh_token")
                .and_then(Value::as_str)
                .ok_or(LinkError::Unreadable)?
                .to_owned(),
            expires_at: expires_at_of(&body),
        })
    }

    fn download(&self, url: &str, to: &Path) -> Result<String, LinkError> {
        let request = self.client.get(url).timeout(DOWNLOAD_TIMEOUT);
        let (status, bytes) = Http::run(async move {
            let response = request.send().await?;
            let status = response.status().as_u16();
            let bytes = response.bytes().await?;
            Ok::<_, reqwest::Error>((status, bytes))
        })
        .map_err(|e| {
            log_warn!("the download failed: {e}");
            LinkError::Unreachable
        })?;
        if status != 200 {
            return Err(LinkError::Server(format!("the file answered {status}")));
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| LinkError::Server(e.to_string()))?;
        }
        std::fs::write(to, &bytes).map_err(|e| LinkError::Server(e.to_string()))?;
        Ok(sha256_hex(&bytes))
    }
}

/// SHA-256, lowercase hex — what a manifest's `sha256` is compared with.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    digest
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// A thread that sleeps until it is needed.

/// Wake a waiting thread now, rather than at its next tick.
#[derive(Debug, Default)]
pub struct Wakeup {
    flag: Mutex<bool>,
    changed: Condvar,
}

impl Wakeup {
    #[must_use]
    pub fn new() -> Arc<Wakeup> {
        Arc::new(Wakeup::default())
    }

    pub fn wake(&self) {
        let mut flag = self.flag.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *flag = true;
        self.changed.notify_all();
    }

    /// Wait up to `limit`. True when woken on purpose, false when the time simply passed.
    pub fn wait_for(&self, limit: Duration) -> bool {
        let flag = self.flag.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut flag, _) = self
            .changed
            .wait_timeout_while(flag, limit, |woken| !*woken)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let woken = *flag;
        *flag = false;
        woken
    }
}

// The release shelf.

/// What the last licence check said the newest release is.
pub struct CloudReleases {
    pub release: Option<SignedManifest>,
}

impl crate::updates::Releases for CloudReleases {
    fn latest(&self) -> Result<(String, String), String> {
        match &self.release {
            Some(signed) => Ok((signed.manifest.clone(), signed.signature.clone())),
            None => Err("no release has been published yet".to_owned()),
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A server that answers one request with what the test chose.

    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    /// What arrived: the request line, the headers and the body.
    #[derive(Debug, Clone, Default)]
    pub struct Seen {
        pub line: String,
        pub headers: Vec<(String, String)>,
        pub body: String,
    }

    impl Seen {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        }
    }

    /// Serve `replies` in order, one per request, then stop. Answers the base URL and the
    /// receiver of what was seen.
    pub fn serve(
        replies: Vec<(u16, &'static str, String)>,
    ) -> (String, std::sync::mpsc::Receiver<Seen>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().expect("an address").port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (status, extra_header, body) in replies {
                let Ok((mut socket, _)) = listener.accept() else {
                    return;
                };
                let _ = socket.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let mut raw = Vec::new();
                let mut chunk = [0_u8; 4096];
                let mut content_length = 0_usize;
                let mut head_end = None;
                while let Ok(n) = socket.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&chunk[..n]);
                    if head_end.is_none()
                        && let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n")
                    {
                        head_end = Some(at + 4);
                        let head = String::from_utf8_lossy(&raw[..at]).to_string();
                        content_length = head
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.trim()
                                    .eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse().ok())
                                    .flatten()
                            })
                            .unwrap_or(0);
                    }
                    if let Some(end) = head_end
                        && raw.len() >= end + content_length
                    {
                        break;
                    }
                }
                let end = head_end.unwrap_or(raw.len());
                let head = String::from_utf8_lossy(&raw[..end]).to_string();
                let mut lines = head.lines();
                let line = lines.next().unwrap_or_default().to_owned();
                let headers = lines
                    .filter_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        Some((k.trim().to_owned(), v.trim().to_owned()))
                    })
                    .collect();
                let received = String::from_utf8_lossy(&raw[end..]).to_string();
                let _ = tx.send(Seen {
                    line,
                    headers,
                    body: received,
                });
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n{body}",
                    body.len(),
                    if extra_header.is_empty() {
                        String::new()
                    } else {
                        format!("{extra_header}\r\n")
                    }
                );
                let _ = socket.write_all(reply.as_bytes());
                let _ = socket.flush();
            }
        });
        (format!("http://127.0.0.1:{port}"), rx)
    }
}

#[cfg(test)]
mod tests {
    use super::fake::serve;
    use super::*;
    use mb_license::MachineId;

    fn an_ask() -> Ask {
        Ask {
            key: "MB-TEST-0000-0000".to_owned(),
            machine: MachineId::for_tests("4c4c4544-0043-4a10-8033-b8c04f4d3132"),
            counter_version: "0.1.0".to_owned(),
        }
    }

    fn signed_by_the_dev_key() -> (String, String) {
        let key = mb_license::snapshot::development_keypair().expect("a key");
        let payload = r#"{"licence":{"key":"MB-TEST-0000-0000","shop_name":"Test","plan":{"code":"p","name":"Plan","features":["reports"],"limits":{"devices":2,"terminals":1}},"status":"active","renews_on":20700,"grace_days":null,"bound_to":null,"trial_ends_on":null,"registered_contact":"+91 98••••••10"},"global_grace_days":10,"issued_at":1,"not_after":9999999999999,"max_offline_days":14}"#;
        let signature = mb_license::snapshot::sign_detached(payload.as_bytes(), &key);
        (payload.to_owned(), signature)
    }

    #[test]
    fn every_status_in_the_protocol_table_maps_to_its_error() {
        let cases = vec![
            (401, r#"{"code":"not_recognised"}"#, CloudError::NotRecognised),
            (
                409,
                r#"{"code":"bound_elsewhere","machine":"4c4c4544"}"#,
                CloudError::BoundElsewhere {
                    machine: "4c4c4544".to_owned(),
                },
            ),
            (
                403,
                r#"{"code":"refused","message":"This licence has been revoked. Ring support."}"#,
                CloudError::Refused("This licence has been revoked. Ring support.".to_owned()),
            ),
            (
                429,
                r#"{"code":"too_soon","days_left":12}"#,
                CloudError::TooSoon { days_left: 12 },
            ),
            (500, r#"{"code":"unreadable"}"#, CloudError::Unreachable),
            (200, "this is not json", CloudError::Unreadable),
        ];
        let replies = cases
            .iter()
            .map(|(status, body, _)| (*status, "", (*body).to_owned()))
            .collect();
        let (base, seen) = serve(replies);
        let http = Http::at(&base, "anon-key");
        for (status, _, expected) in cases {
            let got = http.refresh(&an_ask(), false).expect_err("refused");
            assert_eq!(got, expected, "status {status}");
            let request = seen.recv().expect("seen");
            assert_eq!(request.header("apikey"), Some("anon-key"));
            assert!(request.line.starts_with("POST /functions/v1/licence/refresh"), "{}", request.line);
        }
    }

    #[test]
    fn a_good_reply_carries_the_snapshot_the_login_and_the_extras() {
        let (payload, signature) = signed_by_the_dev_key();
        let body = json!({
            "payload": payload, "signature": signature,
            "device": { "id": "dev-1", "restaurant_id": "rest-1",
                        "session": { "access_token": "at", "refresh_token": "rt", "expires_at": 1_700_000_000 } },
            "extras": { "unread_notices": 2, "release": null }
        });
        let (base, seen) = serve(vec![(200, "", body.to_string())]);
        let http = Http::at(&base, "anon-key");
        let answer = http.activate(&an_ask()).expect("activates");
        assert_eq!(answer.snapshot.payload, payload);
        let device = answer.device.expect("a login");
        assert_eq!(device.device_id, "dev-1");
        assert_eq!(device.restaurant_id, "rest-1");
        assert_eq!(device.access_token, "at");
        assert_eq!(device.expires_at, Timestamp::from_millis(1_700_000_000_000));
        assert_eq!(answer.extras.expect("extras").unread_notices, 2);
        // And it verifies, so a test that reaches here has proven the payload travelled intact.
        mb_license::snapshot::verify(&answer.snapshot, &mb_license::snapshot::trusted_keys())
            .expect("verifies");
        let request = seen.recv().expect("seen");
        let sent: Value = serde_json::from_str(&request.body).expect("json");
        assert_eq!(sent["ask"]["key"], "MB-TEST-0000-0000");
        assert_eq!(sent["ask"]["machine"]["value"], "4c4c4544-0043-4a10-8033-b8c04f4d3132");
    }

    #[test]
    fn a_server_that_is_not_there_is_unreachable_and_nothing_else() {
        // A port nobody is listening on.
        let http = Http::at("http://127.0.0.1:9", "anon-key");
        assert_eq!(http.refresh(&an_ask(), false), Err(CloudError::Unreachable));
        assert_eq!(
            http.rpc("mb_push", &json!({}), "token"),
            Err(LinkError::Unreachable)
        );
    }

    #[test]
    fn an_rpc_carries_the_login_and_reads_the_four_kinds_of_no() {
        let (base, seen) = serve(vec![
            (200, "", r#"{"applied":3,"refused":[]}"#.to_owned()),
            (401, "", r#"{"code":"PGRST301","message":"JWT expired"}"#.to_owned()),
            (403, "", r#"{"code":"42501","message":"this licence has been revoked"}"#.to_owned()),
            (400, "", r#"{"code":"22023","message":"at most 200 rows in one push"}"#.to_owned()),
            (503, "", r#"{"message":"down"}"#.to_owned()),
        ]);
        let http = Http::at(&base, "anon-key");
        let ok = http.rpc("mb_push", &json!({"rows": []}), "the-token").expect("ok");
        assert_eq!(ok["applied"], 3);
        let request = seen.recv().expect("seen");
        assert!(request.line.starts_with("POST /rest/v1/rpc/mb_push "), "{}", request.line);
        assert_eq!(request.header("authorization"), Some("Bearer the-token"));
        assert_eq!(request.header("apikey"), Some("anon-key"));

        assert_eq!(http.rpc("mb_push", &json!({}), "t"), Err(LinkError::Unauthorised));
        assert_eq!(
            http.rpc("mb_push", &json!({}), "t"),
            Err(LinkError::Dead("this licence has been revoked".to_owned()))
        );
        assert_eq!(
            http.rpc("mb_push", &json!({}), "t"),
            Err(LinkError::Refused("at most 200 rows in one push".to_owned()))
        );
        assert!(matches!(http.rpc("mb_push", &json!({}), "t"), Err(LinkError::Server(_))));
    }

    #[test]
    fn a_rest_page_asks_for_its_range_and_reads_the_count() {
        let (base, seen) = serve(vec![(
            206,
            "Content-Range: 0-1/57",
            r#"[{"id":"a"},{"id":"b"}]"#.to_owned(),
        )]);
        let http = Http::at(&base, "anon-key");
        let page = http
            .rest("shop_rows?select=*&order=table_name", "tok", 0, 1)
            .expect("a page");
        assert_eq!(page.rows.len(), 2);
        assert_eq!(page.total, Some(57));
        let request = seen.recv().expect("seen");
        assert_eq!(request.header("range"), Some("0-1"));
        assert_eq!(request.header("range-unit"), Some("items"));
    }

    #[test]
    fn a_refresh_token_that_works_gives_a_new_pair_and_one_that_does_not_ends_the_login() {
        let (base, seen) = serve(vec![
            (
                200,
                "",
                r#"{"access_token":"new-at","refresh_token":"new-rt","expires_at":1700000000}"#.to_owned(),
            ),
            (400, "", r#"{"error":"invalid_grant"}"#.to_owned()),
        ]);
        let http = Http::at(&base, "anon-key");
        let session = http.refresh_session("old-rt").expect("refreshed");
        assert_eq!(session.access_token, "new-at");
        assert_eq!(session.refresh_token, "new-rt");
        let request = seen.recv().expect("seen");
        assert!(request.line.starts_with("POST /auth/v1/token?grant_type=refresh_token"));
        assert!(request.body.contains("old-rt"));
        assert!(matches!(http.refresh_session("old-rt"), Err(LinkError::Dead(_))));
    }

    #[test]
    fn a_download_lands_on_disk_with_its_fingerprint() {
        let (base, _seen) = serve(vec![(200, "", "hello installer".to_owned())]);
        let http = Http::at(&base, "anon-key");
        let dir = std::env::temp_dir().join(format!("mb-download-{}", std::process::id()));
        let to = dir.join("incoming").join("x.exe");
        let sha = http.download(&format!("{base}/x.exe"), &to).expect("downloaded");
        assert_eq!(std::fs::read_to_string(&to).expect("the file"), "hello installer");
        assert_eq!(sha, sha256_hex(b"hello installer"));
        assert_eq!(sha.len(), 64);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wakeup_returns_early_when_woken_and_on_time_when_not() {
        let wakeup = Wakeup::new();
        let started = std::time::Instant::now();
        assert!(!wakeup.wait_for(Duration::from_millis(60)));
        assert!(started.elapsed() >= Duration::from_millis(50));

        let other = Arc::clone(&wakeup);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            other.wake();
        });
        let started = std::time::Instant::now();
        assert!(wakeup.wait_for(Duration::from_secs(10)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn the_shipped_cloud_is_named_and_the_environment_can_move_it() {
        assert!(cloud_url().starts_with("https://"), "{}", cloud_url());
        assert!(!anon_key().is_empty());
        assert!(!cloud_url().ends_with('/'));
    }
}
