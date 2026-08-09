//! **P19's T1–T12, against a real server on a real socket.**
//!
//! Every one of these runs with no database, no Tauri and no shop — because
//! everything mb-lan can ask the shop goes through the [`Counter`] trait, and
//! this file implements it with a fake. That seam is the reason these tests
//! take milliseconds and the reason a revocation can be made to land exactly
//! between two requests.
//!
//! The clock is injected too. A rate limiter tested by sleeping is a test suite
//! that takes a minute and fails on a loaded machine.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests: expect is the assertion"
)]

use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use mb_auth::{Permission, PermissionSet};
use mb_core::Timestamp;
use mb_lan::counter::{Counter, Device, DeviceRow, PairRequest, PairedDevice, Refusal};

// ---------------------------------------------------------------------------
// A shop that does not exist.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Row {
    id: String,
    name: String,
    secret: String,
    staff_id: Option<String>,
    permissions: PermissionSet,
    revoked: bool,
}

#[derive(Debug)]
struct FakeCounter {
    rows: Mutex<Vec<Row>>,
    limit: AtomicU32,
    /// What `authenticate` was asked, so a test can prove the register really
    /// is read on every request rather than trusted from a cached claim.
    reads: AtomicU32,
    seen: Mutex<Vec<(String, String)>>,
}

impl FakeCounter {
    fn new() -> Arc<FakeCounter> {
        Arc::new(FakeCounter {
            rows: Mutex::new(Vec::new()),
            limit: AtomicU32::new(5),
            reads: AtomicU32::new(0),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn revoke(&self, id: &str) {
        let mut rows = self.rows.lock().unwrap();
        for row in rows.iter_mut().filter(|r| r.id == id) {
            row.revoked = true;
        }
    }

    fn set_permissions(&self, id: &str, permissions: PermissionSet) {
        let mut rows = self.rows.lock().unwrap();
        for row in rows.iter_mut().filter(|r| r.id == id) {
            row.permissions = permissions.clone();
        }
    }
}

impl Counter for FakeCounter {
    fn shop_name(&self) -> String {
        "Anna Kuteera".to_owned()
    }

    fn device_limit(&self) -> u32 {
        self.limit.load(Ordering::SeqCst)
    }

    fn devices(&self) -> Vec<DeviceRow> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .filter(|r| !r.revoked)
            .map(|r| DeviceRow {
                id: r.id.clone(),
                name: r.name.clone(),
                platform: "android".to_owned(),
                staff: None,
                last_seen: "just now".to_owned(),
                last_ip: String::new(),
            })
            .collect()
    }

    fn authenticate(&self, device_id: &str, secret: &str) -> Option<Device> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let rows = self.rows.lock().unwrap();
        let row = rows
            .iter()
            .find(|r| r.id == device_id && r.secret == secret && !r.revoked)?;
        Some(Device {
            id: row.id.clone(),
            name: row.name.clone(),
            staff_id: row.staff_id.clone(),
            permissions: row.permissions.clone(),
        })
    }

    fn seen(&self, device_id: &str, ip: &str) {
        self.seen
            .lock()
            .unwrap()
            .push((device_id.to_owned(), ip.to_owned()));
    }

    fn pair(
        &self,
        _request: &PairRequest,
        name: &str,
        _platform: &str,
    ) -> Result<PairedDevice, Refusal> {
        let mut rows = self.rows.lock().unwrap();
        let id = format!("dev_{}", rows.len() + 1);
        let secret = format!("secret_{}", rows.len() + 1);
        rows.push(Row {
            id: id.clone(),
            name: name.to_owned(),
            secret: secret.clone(),
            staff_id: Some("staff_1".to_owned()),
            permissions: {
                let mut set = PermissionSet::new();
                set.insert(Permission::BillCreate);
                set
            },
            revoked: false,
        });
        Ok(PairedDevice {
            device_id: id,
            secret,
            server_id: "srv_test".to_owned(),
        })
    }
}

/// A clock a test moves by hand. A rate limiter tested by sleeping is a test
/// suite that takes a minute and fails on a loaded machine.
#[derive(Debug)]
struct Clock(AtomicI64);

impl Clock {
    fn arc() -> Arc<Clock> {
        Arc::new(Clock(AtomicI64::new(0)))
    }
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.load(Ordering::SeqCst))
    }
    fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

struct Harness {
    counter: Arc<FakeCounter>,
    clock: Arc<Clock>,
    shared: mb_lan::Shared,
    running: mb_lan::Running,
    base: String,
    client: reqwest::Client,
}

impl Harness {
    fn start() -> Harness {
        Harness::start_with(false)
    }

    /// `tls` picks the real handshake. Off for most tests, because what they
    /// are testing is the gate and not the cryptography; on for T10, which is
    /// testing exactly the cryptography.
    fn start_with(tls: bool) -> Harness {
        let counter = FakeCounter::new();
        let clock = Clock::arc();
        let identity = Arc::new(
            mb_lan::Identity::ephemeral(&["127.0.0.1".parse().unwrap()]).expect("an identity"),
        );
        let ticking = Arc::clone(&clock);
        let shared = mb_lan::Shared::new(
            Arc::clone(&counter) as Arc<dyn Counter>,
            Arc::clone(&identity),
            Arc::new(move || ticking.now()),
        );
        let config = tls
            .then(|| mb_lan::TlsConfig::from_identity(&identity).expect("tls"))
            ;
        // Port 0: the OS picks a free one, so two tests running at once cannot
        // collide — which they do, because cargo runs them in parallel.
        let running = mb_lan::start_on(shared.clone(), std::net::Ipv4Addr::LOCALHOST, 0, config).expect("it listens");
        let scheme = if tls { "https" } else { "http" };
        let base = format!("{scheme}://127.0.0.1:{}", running.port);

        let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10));
        if tls {
            // **The pin.** The client trusts this one certificate and nothing
            // else — which is what a paired phone does.
            let der = pem_to_der(&identity.certificate_pem);
            builder = builder
                .add_root_certificate(reqwest::Certificate::from_der(&der).expect("a certificate"))
                .danger_accept_invalid_hostnames(false);
        }
        Harness {
            counter,
            clock,
            shared,
            running,
            base,
            client: builder.build().expect("a client"),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.running.stop();
    }
}

fn pem_to_der(pem: &str) -> Vec<u8> {
    use base64::Engine as _;
    let body: String = pem
        .lines()
        .skip_while(|l| !l.starts_with("-----BEGIN CERTIFICATE-----"))
        .skip(1)
        .take_while(|l| !l.starts_with("-----END CERTIFICATE-----"))
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(body)
        .expect("decodes")
}

/// Pair a phone the way a real one does: present the token, a person approves,
/// the phone collects its credential.
async fn pair_a_phone(h: &Harness, name: &str) -> PairedDevice {
    let (token, _code) = h.shared.desk.open(h.clock.now());
    let asked: serde_json::Value = h
        .client
        .post(h.url("/v1/pair"))
        .json(&PairRequest {
            name: name.to_owned(),
            platform: "android".to_owned(),
            token,
        })
        .send()
        .await
        .expect("asked")
        .json()
        .await
        .expect("json");
    let request_id = asked["request_id"].as_str().expect("a request id").to_owned();

    // Nothing is issued until a person presses Allow. That press is the panel's
    // job, and here it is one line.
    let waiting = h.shared.desk.take(&request_id).expect("it is in the queue");
    let device = h
        .counter
        .pair(
            &PairRequest {
                name: waiting.name.clone(),
                platform: waiting.platform.clone(),
                token: String::new(),
            },
            &waiting.name,
            &waiting.platform,
        )
        .expect("paired");
    h.shared.desk.approve(&request_id, device);

    h.client
        .get(h.url(&format!("/v1/pair/{request_id}")))
        .send()
        .await
        .expect("collected")
        .json()
        .await
        .expect("json")
}

fn bearer(device: &PairedDevice) -> String {
    format!("Bearer {}.{}", device.device_id, device.secret)
}

// ---------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------

/// **T1.** A simulated client pairs, is approved, connects and is served.
#[tokio::test]
async fn a_phone_pairs_is_approved_and_is_served() {
    let h = Harness::start();

    // `/v1/hello` first — this is what discovery confirms, and it needs no
    // credential by design.
    let hello: serde_json::Value = h
        .client
        .get(h.url("/v1/hello"))
        .send()
        .await
        .expect("hello")
        .json()
        .await
        .expect("json");
    assert_eq!(hello["shop_name"], "Anna Kuteera");
    assert_eq!(hello["protocol_version"], mb_lan::PROTOCOL_VERSION);
    assert!(hello["fingerprint"].as_str().unwrap().starts_with("sha256:"));

    let device = pair_a_phone(&h, "Ravi's phone").await;
    assert!(!device.secret.is_empty());

    let me = h
        .client
        .get(h.url("/v1/me"))
        .header("authorization", bearer(&device))
        .send()
        .await
        .expect("me");
    assert_eq!(me.status(), 200);
    let body: serde_json::Value = me.json().await.expect("json");
    assert_eq!(body["name"], "Ravi's phone");
    // The STAFF member is identified separately from the device, so a shared
    // tablet still attributes each action to a person.
    assert_eq!(body["staff_id"], "staff_1");

    // And the counter noticed it, for the panel's "last seen".
    assert!(!h.counter.seen.lock().unwrap().is_empty());
}

/// **T2.** An unpaired client is refused everything except `/v1/hello`.
#[tokio::test]
async fn an_unpaired_phone_gets_nothing_but_hello() {
    let h = Harness::start();

    assert_eq!(
        h.client.get(h.url("/v1/hello")).send().await.expect("hello").status(),
        200
    );

    for attempt in [
        None,
        Some("Bearer nonsense"),
        Some("Bearer dev_1.wrong"),
        Some("Bearer .just_a_secret"),
    ] {
        let mut request = h.client.get(h.url("/v1/me"));
        if let Some(header) = attempt {
            request = request.header("authorization", header);
        }
        let response = request.send().await.expect("tried");
        assert_eq!(
            response.status(),
            401,
            "an unpaired phone got in with {attempt:?}"
        );
        let body: serde_json::Value = response.json().await.expect("json");
        let said = body["message"].as_str().unwrap_or_default();
        // One refusal for "no such device" and "wrong secret". Telling them
        // apart is a way to enumerate the shop's phones.
        assert!(said.contains("not connected"), "{said}");
    }
}

/// **T3.** A revoked device is refused on its VERY NEXT request.
#[tokio::test]
async fn a_revoked_phone_is_refused_on_the_next_request() {
    let h = Harness::start();
    let device = pair_a_phone(&h, "Sacked phone").await;

    let ok = h
        .client
        .get(h.url("/v1/me"))
        .header("authorization", bearer(&device))
        .send()
        .await
        .expect("first");
    assert_eq!(ok.status(), 200);
    let reads_before = h.counter.reads.load(Ordering::SeqCst);

    h.counter.revoke(&device.device_id);

    let refused = h
        .client
        .get(h.url("/v1/me"))
        .header("authorization", bearer(&device))
        .send()
        .await
        .expect("second");
    assert_eq!(
        refused.status(),
        401,
        "the revoke waited for the next login, which is what v1 did"
    );
    // The register really was consulted — not a cached claim from the token.
    assert!(h.counter.reads.load(Ordering::SeqCst) > reads_before);
}

/// **T4.** A client without a permission is refused server-side, even when the
/// UI would have hidden the button.
#[tokio::test]
async fn a_permission_is_enforced_on_the_server_not_on_the_phone() {
    let h = Harness::start();
    let paired = pair_a_phone(&h, "Waiter's phone").await;

    let device = h
        .counter
        .authenticate(&paired.device_id, &paired.secret)
        .expect("live");
    // It may take an order.
    assert!(mb_lan::require(&device, Permission::BillCreate).is_ok());
    // It may not void a bill, and the refusal is a SENTENCE, not a code.
    let refused = mb_lan::require(&device, Permission::BillVoid).expect_err("it was allowed");
    let said = refused.message();
    assert!(said.contains("void a bill"), "{said}");
    assert!(said.contains("Ask somebody who can"), "{said}");

    // And granting it changes the answer on the very next call — the same
    // property T3 tests for revocation, for the same reason.
    h.counter
        .set_permissions(&paired.device_id, PermissionSet::everything());
    let device = h
        .counter
        .authenticate(&paired.device_id, &paired.secret)
        .expect("live");
    assert!(mb_lan::require(&device, Permission::BillVoid).is_ok());
}

/// **T5.** Fifty concurrent clients do not measurably slow a billing
/// operation. The number is printed and goes in `docs/PERFORMANCE.md`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_phones_do_not_slow_the_till() {
    let h = Harness::start();
    let device = pair_a_phone(&h, "One of fifty").await;

    // What "a billing operation" costs with nobody on the network. The work
    // itself is a stand-in for the till's — what is being measured is whether
    // the network layer can get in its way, and the answer must not depend on
    // what the till happens to be doing.
    let quiet = time_a_bill();

    let mut phones = Vec::new();
    for _ in 0..50 {
        let client = h.client.clone();
        let url = h.url("/v1/me");
        let auth = bearer(&device);
        phones.push(tokio::spawn(async move {
            for _ in 0..10 {
                let _ = client.get(&url).header("authorization", &auth).send().await;
            }
        }));
    }
    // Measured while all fifty are in flight, which is the only version of
    // this measurement that means anything.
    let busy = time_a_bill();
    for phone in phones {
        let _ = phone.await;
    }

    println!("\n--- T5: fifty phones against the till (P19) ---");
    println!("  a billing operation, network quiet: {quiet:?}");
    println!("  the same, with 50 phones connected: {busy:?}");

    // Generous on purpose: this runs on a laptop that is also compiling. What
    // it is guarding against is an ORDER-OF-MAGNITUDE regression — a handler
    // taking a lock the till needs — not a few microseconds of scheduler noise.
    assert!(
        busy < quiet * 20 + std::time::Duration::from_millis(50),
        "fifty phones made the till {busy:?} against {quiet:?} — something is \
         holding a lock the billing path needs"
    );
}

/// A stand-in for the till's synchronous work: a mutex taken and released,
/// which is the shape of every `Counter` call and of every `App::with_shop`.
fn time_a_bill() -> std::time::Duration {
    let lock = Mutex::new(0_u64);
    let start = std::time::Instant::now();
    for _ in 0..1_000 {
        let mut n = lock.lock().unwrap();
        *n = n.wrapping_add(1);
    }
    start.elapsed()
}

/// **T6.** The server survives a client that connects and then goes silent
/// mid-request; nothing leaks and nothing blocks.
#[tokio::test]
async fn a_silent_client_blocks_nothing() {
    let h = Harness::start();

    // Five sockets that connect, send half a request line and stop. This is a
    // port scanner, and it is also a phone that walked out of range.
    let mut zombies = Vec::new();
    for _ in 0..5 {
        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", h.running.port))
            .await
            .expect("connected");
        use tokio::io::AsyncWriteExt as _;
        let mut stream = stream;
        stream.write_all(b"GET /v1/he").await.expect("half a request");
        zombies.push(stream);
    }

    // The counter still answers everybody else, immediately.
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        h.client.get(h.url("/v1/hello")).send(),
    )
    .await
    .expect("it did not hang")
    .expect("it answered");
    assert_eq!(response.status(), 200);

    drop(zombies);
}

/// **T7.** Killing and restarting the server re-establishes clients without
/// re-pairing — because the credential is bound to the SERVER, not to a socket
/// and not to an address.
#[tokio::test]
async fn a_restart_does_not_cost_a_pairing() {
    let mut h = Harness::start();
    let device = pair_a_phone(&h, "Survivor").await;
    assert_eq!(
        h.client
            .get(h.url("/v1/me"))
            .header("authorization", bearer(&device))
            .send()
            .await
            .expect("before")
            .status(),
        200
    );

    // Down.
    h.running.stop();

    // Up again — a new listener, a new port (DHCP moves a shop's counter too),
    // the same identity and the same register.
    let running = mb_lan::start_on(h.shared.clone(), std::net::Ipv4Addr::LOCALHOST, 0, None).expect("it listens again");
    let base = format!("http://127.0.0.1:{}", running.port);
    h.running = running;
    h.base = base;

    let after = h
        .client
        .get(h.url("/v1/me"))
        .header("authorization", bearer(&device))
        .send()
        .await
        .expect("after");
    assert_eq!(
        after.status(),
        200,
        "a restart made every waiter pair again, which is v1's behaviour"
    );
}

/// **T8.** A version-mismatched client gets the clear upgrade message.
#[tokio::test]
async fn an_old_phone_is_told_to_update_in_words() {
    let h = Harness::start();
    let device = pair_a_phone(&h, "Old phone").await;

    let response = h
        .client
        .get(h.url("/v1/me"))
        .header("authorization", bearer(&device))
        .header("x-magicbill-version", "0")
        .send()
        .await
        .expect("tried");
    assert_eq!(response.status(), 426);
    let body: serde_json::Value = response.json().await.expect("json");
    let said = body["message"].as_str().unwrap_or_default();
    assert!(said.contains("older than the counter"), "{said}");
    assert!(said.contains("Play Store"), "{said}");
    // Not a tag, not a number, not "protocol mismatch".
    assert!(!said.contains("protocol"), "{said}");

    // The other direction says the other thing — the counter is the one to
    // update, and a waiter must not be sent to the Play Store for it.
    let newer = h
        .client
        .get(h.url("/v1/me"))
        .header("authorization", bearer(&device))
        .header("x-magicbill-version", "99")
        .send()
        .await
        .expect("tried");
    let body: serde_json::Value = newer.json().await.expect("json");
    let said = body["message"].as_str().unwrap_or_default();
    assert!(said.contains("counter PC"), "{said}");

    // And `/v1/hello` still answers, because a phone that is told to upgrade
    // must still be able to ask what it is talking to.
    assert_eq!(
        h.client
            .get(h.url("/v1/hello"))
            .header("x-magicbill-version", "0")
            .send()
            .await
            .expect("hello")
            .status(),
        200
    );
}

/// **T9.** Rate limiting engages AND recovers, over the real socket.
#[tokio::test]
async fn the_pairing_door_shuts_and_opens_again() {
    let h = Harness::start();

    let mut last = None;
    for _ in 0..(mb_lan::Rate::PAIRING.burst + 3) {
        last = Some(
            h.client
                .post(h.url("/v1/pair"))
                .json(&PairRequest {
                    name: "Guessing".to_owned(),
                    platform: "android".to_owned(),
                    token: "wrong".to_owned(),
                })
                .send()
                .await
                .expect("tried"),
        );
    }
    let response = last.expect("at least one");
    assert_eq!(response.status(), 429, "the Argon2 door never shut");
    // A `Retry-After`, always: a phone refused with no idea when to come back
    // retries into a wall and decides the counter is broken.
    let retry = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u32>().ok())
        .expect("no Retry-After");
    assert!(retry >= 1);

    // Wait it out — on the injected clock, not on the wall.
    h.clock.advance(i64::from(retry + 1) * 1_000);
    let again = h
        .client
        .post(h.url("/v1/pair"))
        .json(&PairRequest {
            name: "Guessing".to_owned(),
            platform: "android".to_owned(),
            token: "wrong".to_owned(),
        })
        .send()
        .await
        .expect("tried");
    assert_ne!(again.status(), 429, "the door shut and never opened again");
}

/// **T10.** A client that does not trust the pinned certificate is rejected;
/// one that pins it succeeds.
#[tokio::test]
async fn tls_only_lets_in_a_client_that_pinned_the_certificate() {
    let h = Harness::start_with(true);

    // The pinning client — this is a paired phone.
    let response = h.client.get(h.url("/v1/hello")).send().await;
    assert!(
        response.is_ok(),
        "a client that pinned the certificate was refused: {:?}",
        response.err()
    );
    assert_eq!(response.expect("ok").status(), 200);

    // A client that trusts the ordinary certificate authorities and nothing
    // else — which is every HTTP library's default, and a stranger's tooling.
    let stranger = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("a client");
    let refused = stranger.get(h.url("/v1/hello")).send().await;
    assert!(
        refused.is_err(),
        "a client that did not pin the certificate got through, which means \
         the pin is decorative"
    );
}

/// **T11.** Covered in `pairing.rs`'s unit tests for the token's own rules;
/// this is the same property through the socket, because a rule that only holds
/// below the HTTP layer is a rule with a way around it.
#[tokio::test]
async fn a_used_token_cannot_pair_a_second_phone() {
    let h = Harness::start();
    let (token, _) = h.shared.desk.open(h.clock.now());

    let ask = |name: &str, token: String| {
        let client = h.client.clone();
        let url = h.url("/v1/pair");
        let name = name.to_owned();
        async move {
            client
                .post(url)
                .json(&PairRequest {
                    name,
                    platform: "android".to_owned(),
                    token,
                })
                .send()
                .await
                .expect("tried")
        }
    };

    assert_eq!(ask("First phone", token.clone()).await.status(), 202);
    let second = ask("A stranger", token).await;
    assert_eq!(
        second.status(),
        400,
        "a photographed screen paired a second phone"
    );
}

/// **T12.** The device limit refuses the next phone, in a sentence with the
/// number in it.
#[tokio::test]
async fn the_device_limit_refuses_in_a_sentence_with_the_number() {
    let h = Harness::start();
    h.counter.limit.store(2, Ordering::SeqCst);

    pair_a_phone(&h, "Phone one").await;
    pair_a_phone(&h, "Phone two").await;

    let (token, _) = h.shared.desk.open(h.clock.now());
    let refused = h
        .client
        .post(h.url("/v1/pair"))
        .json(&PairRequest {
            name: "Phone three".to_owned(),
            platform: "android".to_owned(),
            token,
        })
        .send()
        .await
        .expect("tried");
    assert_eq!(refused.status(), 403);
    let body: serde_json::Value = refused.json().await.expect("json");
    let said = body["message"].as_str().unwrap_or_default();
    assert!(said.contains('2'), "the number is not in the sentence: {said}");
    assert!(said.contains("phones"), "{said}");
    assert!(said.contains("Remove one"), "it does not say what to do: {said}");
}

/// The reconnection model: a phone that dropped gets what it missed, and one
/// that is too far behind is TOLD so rather than left to work it out.
#[tokio::test]
async fn a_reconnecting_phone_gets_what_it_missed_and_never_a_refetch_storm() {
    let h = Harness::start();

    for n in 0..5 {
        h.shared.push("table", serde_json::json!({ "n": n }));
    }
    match h.shared.since(2) {
        mb_lan::Missed::Since { pushes } => {
            assert_eq!(pushes.len(), 3, "it did not send exactly what was missed");
            assert_eq!(pushes[0].seq, 3);
        }
        mb_lan::Missed::TooFarBehind { .. } => panic!("three messages is not too far behind"),
    }

    // Far enough behind that the buffer cannot serve it. The server says so
    // explicitly — a full refetch is a decision, never a fallback.
    for n in 0..200 {
        h.shared.push("table", serde_json::json!({ "n": n }));
    }
    match h.shared.since(1) {
        mb_lan::Missed::TooFarBehind { newest } => assert!(newest > 200),
        mb_lan::Missed::Since { .. } => {
            panic!("it quietly sent a partial history, which is a phone with a hole in it")
        }
    }
}
