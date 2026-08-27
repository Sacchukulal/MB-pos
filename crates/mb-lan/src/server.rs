//! The server, and the one rule it must never break.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{ConnectInfo, State, ws::WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::counter::{Counter, Device, PairRequest, Refusal};
use crate::error::LanError;
use crate::limit::{Limiter, Rate};
use crate::pairing::Desk;

/// The protocol version. One integer, and a client on a different one is told to update in a
/// sentence rather than left to fail mysteriously.
pub const PROTOCOL_VERSION: u32 = 1;

/// What a phone that will not keep up is allowed to fall behind by.
const BROADCAST_DEPTH: usize = 64;

/// How long a socket may be silent before it is closed.
const IDLE: Duration = Duration::from_secs(90);

/// Who is on the other end of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer(pub SocketAddr);

impl Peer {
    /// The key a rate limiter uses, and the "last seen from" the panel shows.
    #[must_use]
    pub fn ip(&self) -> String {
        self.0.ip().to_string()
    }
}

impl
    axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, tokio::net::TcpListener>>
    for Peer
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, tokio::net::TcpListener>) -> Self {
        Peer(*stream.remote_addr())
    }
}

impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, TlsListener>> for Peer {
    fn connect_info(stream: axum::serve::IncomingStream<'_, TlsListener>) -> Self {
        Peer(*stream.remote_addr())
    }
}

/// One message pushed to every connected phone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Push {
    /// Monotonic, and it is the whole reconnection design.
    pub seq: u64,
    pub kind: String,
    pub body: serde_json::Value,
}

/// What a reconnecting phone is told.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "what", rename_all = "snake_case")]
pub enum Missed {
    /// Here is what you missed, in order.
    Since { pushes: Vec<Push> },
    /// You are further behind than the buffer goes.
    TooFarBehind { newest: u64 },
}

/// Where the server is listening, and whether it is reachable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Where {
    pub address: String,
    pub port: u16,
    pub fingerprint: String,
    pub server_id: String,
}

/// Everything a handler can reach.
#[derive(Clone)]
pub struct Shared {
    pub counter: Arc<dyn Counter>,
    pub desk: Arc<Desk>,
    pub identity: Arc<crate::identity::Identity>,
    pub clock: Arc<dyn Fn() -> mb_core::Timestamp + Send + Sync>,
    device_limiter: Arc<Limiter>,
    pair_limiter: Arc<Limiter>,
    hello_limiter: Arc<Limiter>,
    pushes: Arc<tokio::sync::broadcast::Sender<Push>>,
    history: Arc<std::sync::Mutex<std::collections::VecDeque<Push>>>,
    next_seq: Arc<std::sync::atomic::AtomicU64>,
}

impl Shared {
    #[must_use]
    pub fn new(
        counter: Arc<dyn Counter>,
        identity: Arc<crate::identity::Identity>,
        clock: Arc<dyn Fn() -> mb_core::Timestamp + Send + Sync>,
    ) -> Shared {
        let (pushes, _) = tokio::sync::broadcast::channel(BROADCAST_DEPTH);
        Shared {
            counter,
            desk: Arc::new(Desk::new()),
            identity,
            clock,
            device_limiter: Arc::new(Limiter::new(Rate::DEVICE)),
            pair_limiter: Arc::new(Limiter::new(Rate::PAIRING)),
            hello_limiter: Arc::new(Limiter::new(Rate::HELLO)),
            pushes: Arc::new(pushes),
            history: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
            next_seq: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Send something to every connected phone.
    pub fn push(&self, kind: &str, body: serde_json::Value) -> u64 {
        let seq = self
            .next_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let message = Push {
            seq,
            kind: kind.to_owned(),
            body,
        };
        {
            let mut history = lock(&self.history);
            history.push_back(message.clone());
            while history.len() > BROADCAST_DEPTH {
                history.pop_front();
            }
        }
        let _ = self.pushes.send(message);
        seq
    }

    /// What a phone missed since `seq`.
    #[must_use]
    pub fn since(&self, seq: u64) -> Missed {
        let history = lock(&self.history);
        let newest = history.back().map_or(0, |p| p.seq);
        let oldest = history.front().map_or(0, |p| p.seq);
        if seq > 0 && oldest > 0 && seq + 1 < oldest {
            return Missed::TooFarBehind { newest };
        }
        Missed::Since {
            pushes: history.iter().filter(|p| p.seq > seq).cloned().collect(),
        }
    }
}

fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// The routes.

#[derive(Debug, Serialize)]
struct Hello {
    server_id: String,
    protocol_version: u32,
    shop_name: String,
    fingerprint: String,
    /// A joining TILL needs the certificate itself to pin it, and a phone reads this and
    /// ignores it.
    certificate_pem: String,
}

#[derive(Debug, Serialize)]
struct Trouble {
    message: String,
}

fn trouble(status: StatusCode, message: String) -> Response {
    (status, Json(Trouble { message })).into_response()
}

fn refused(r: &Refusal) -> Response {
    let status = StatusCode::from_u16(r.status()).unwrap_or(StatusCode::FORBIDDEN);
    trouble(status, r.message())
}

/// The router. Public so a test can drive it over plain TCP without TLS.
pub fn router(shared: Shared) -> Router {
    Router::new()
        .route("/v1/hello", get(hello))
        .route("/v1/pair", post(pair))
        .route("/v1/pair/{request_id}", get(pair_status))
        .route("/v1/me", get(me))
        .route("/v1/stream", get(stream))
        // What a phone came here to do.
        .route("/v1/intent", post(intent))
        .route("/v1/batch", post(batch))
        .route("/v1/forward", post(forward))
        .route("/v1/catalogue", get(catalogue))
        .with_state(shared)
}

/// One intent. Idempotent by its own id, so a phone that lost the reply retries this exact
/// request and gets the same answer.
async fn intent(
    State(shared): State<Shared>,
    headers: HeaderMap,
    ConnectInfo(from): ConnectInfo<Peer>,
    Json(intent): Json<crate::intent::Intent>,
) -> Response {
    let device = match authenticate(&shared, &headers, from) {
        Ok(d) => d,
        Err(response) => return response,
    };
    let outcome = shared.counter.apply(&device, &intent);
    answered(&outcome)
}

async fn forward(
    State(shared): State<Shared>,
    headers: HeaderMap,
    ConnectInfo(from): ConnectInfo<Peer>,
    Json(forwarded): Json<crate::intent::Forwarded>,
) -> Response {
    let device = match authenticate(&shared, &headers, from) {
        Ok(d) => d,
        Err(response) => return response,
    };
    Json(shared.counter.receive(&device, &forwarded)).into_response()
}

/// A batch a phone queued while it could not reach us.
async fn batch(
    State(shared): State<Shared>,
    headers: HeaderMap,
    ConnectInfo(from): ConnectInfo<Peer>,
    Json(batch): Json<crate::intent::Batch>,
) -> Response {
    let device = match authenticate(&shared, &headers, from) {
        Ok(d) => d,
        Err(response) => return response,
    };
    Json(shared.counter.apply_batch(&device, &batch)).into_response()
}

#[derive(Debug, Deserialize)]
struct CatalogueQuery {
    version: Option<String>,
}

async fn catalogue(
    State(shared): State<Shared>,
    headers: HeaderMap,
    ConnectInfo(from): ConnectInfo<Peer>,
    axum::extract::Query(query): axum::extract::Query<CatalogueQuery>,
) -> Response {
    if let Err(response) = authenticate(&shared, &headers, from) {
        return response;
    }
    match shared.counter.catalogue(query.version.as_deref()) {
        // 304, and it is the point of the version.
        None => StatusCode::NOT_MODIFIED.into_response(),
        Some(catalogue) => Json(catalogue).into_response(),
    }
}

/// An outcome, with the status its own kind implies.
fn answered(outcome: &crate::intent::Outcome) -> Response {
    use crate::intent::Outcome;
    let status = match outcome {
        Outcome::Ok { .. } => StatusCode::OK,
        // 409 and not 400: nothing was malformed.
        Outcome::Refused { .. } => StatusCode::CONFLICT,
        // 202: taken, not applied, waiting for a person.
        Outcome::Held { .. } => StatusCode::ACCEPTED,
    };
    (status, Json(outcome)).into_response()
}

/// Unauthenticated on purpose: this is how a phone finds out what it is talking to, including
/// whether its own version still fits.
async fn hello(State(shared): State<Shared>, ConnectInfo(from): ConnectInfo<Peer>) -> Response {
    if let Err(wait) = shared.hello_limiter.check(&from.ip(), (shared.clock)()) {
        return too_many(wait);
    }
    Json(Hello {
        server_id: shared.identity.server_id.clone(),
        protocol_version: PROTOCOL_VERSION,
        shop_name: shared.counter.shop_name(),
        fingerprint: shared.identity.fingerprint.clone(),
        certificate_pem: shared.identity.certificate_pem.clone(),
    })
    .into_response()
}

async fn pair(
    State(shared): State<Shared>,
    ConnectInfo(from): ConnectInfo<Peer>,
    Json(request): Json<PairRequest>,
) -> Response {
    let ip = from.ip();
    // The tightest bucket in the product, and the module note says why: this is the Argon2 door
    // and it is open to anybody on the WiFi.
    if let Err(wait) = shared.pair_limiter.check(&ip, (shared.clock)()) {
        return too_many(wait);
    }
    if request.name.trim().is_empty() {
        return trouble(
            StatusCode::BAD_REQUEST,
            "This phone did not say what it is called. The counter shows that \
             name to the person who allows it."
                .to_owned(),
        );
    }
    // The limit is checked BEFORE a person is asked, so nobody is shown an approval they are
    // not allowed to grant.
    if request.platform.trim().eq_ignore_ascii_case("till") {
        if let Err(says) = shared.counter.till_room() {
            return refused(&Refusal::TooManyDevices(says));
        }
    } else {
        let live = u32::try_from(shared.counter.devices().len()).unwrap_or(u32::MAX);
        let limit = shared.counter.device_limit();
        if live >= limit {
            return refused(&Refusal::TooManyDevices(format!(
                "This shop's plan allows {limit} {}. Remove one on the counter \
                 before adding another.",
                if limit == 1 { "phone" } else { "phones" }
            )));
        }
    }

    match shared.desk.present(
        &request.token,
        &request.name,
        &request.platform,
        &ip,
        (shared.clock)(),
    ) {
        Ok(request_id) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "request_id": request_id,
                "message": Refusal::WaitingForApproval.message(),
            })),
        )
            .into_response(),
        Err(r) => refused(&r),
    }
}

/// The phone polls this while somebody at the counter decides.
async fn pair_status(
    State(shared): State<Shared>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
    ConnectInfo(from): ConnectInfo<Peer>,
) -> Response {
    if let Err(wait) = shared.hello_limiter.check(&from.ip(), (shared.clock)()) {
        return too_many(wait);
    }
    match shared.desk.collect(&request_id) {
        Ok(Some(device)) => {
            // A phone that has just paired starts with a full bucket: its first seconds must
            // not be spent inside a limit the pairing attempt drained.
            shared.device_limiter.forget(&device.device_id);
            Json(device).into_response()
        }
        Ok(None) => (
            StatusCode::ACCEPTED,
            Json(Trouble {
                message: Refusal::WaitingForApproval.message(),
            }),
        )
            .into_response(),
        Err(r) => refused(&r),
    }
}

async fn me(
    State(shared): State<Shared>,
    headers: HeaderMap,
    ConnectInfo(from): ConnectInfo<Peer>,
) -> Response {
    let device = match authenticate(&shared, &headers, from) {
        Ok(d) => d,
        Err(response) => return response,
    };
    Json(serde_json::json!({
        "device_id": device.id,
        "name": device.name,
        "staff_id": device.staff_id,
        "may": device
            .permissions
            .iter()
            .map(|p| p.code())
            .collect::<Vec<_>>(),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    /// The last sequence this phone saw.
    since: Option<u64>,
}

async fn stream(
    State(shared): State<Shared>,
    headers: HeaderMap,
    ConnectInfo(from): ConnectInfo<Peer>,
    axum::extract::Query(query): axum::extract::Query<StreamQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let device = match authenticate(&shared, &headers, from) {
        Ok(d) => d,
        Err(response) => return response,
    };
    let missed = shared.since(query.since.unwrap_or(0));
    let mut receiver = shared.pushes.subscribe();
    let device_id = device.id;

    upgrade.on_upgrade(move |mut socket| async move {
        use axum::extract::ws::Message;

        // What it missed, first and in order, before anything live.
        if let Ok(text) = serde_json::to_string(&missed)
            && socket.send(Message::Text(text.into())).await.is_err()
        {
            return;
        }

        loop {
            tokio::select! {
                // A phone that cannot keep up is DISCONNECTED, not allowed to grow a queue in
                // the counter's memory.
                received = receiver.recv() => match received {
                    Ok(push) => {
                        let Ok(text) = serde_json::to_string(&push) else { continue };
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                // Anything the phone says, plus its own liveness.
                from_phone = tokio::time::timeout(IDLE, socket.recv()) => match from_phone {
                    Ok(Some(Ok(Message::Close(_))) | None) | Err(_) => break,
                    Ok(Some(Err(_))) => break,
                    Ok(Some(Ok(_))) => {}
                },
            }
        }
        let _ = device_id;
    })
}

fn too_many(seconds: u32) -> Response {
    let mut response = trouble(
        StatusCode::TOO_MANY_REQUESTS,
        format!("The counter is busy. Try again in {seconds} seconds."),
    );
    if let Ok(value) = seconds.to_string().parse() {
        response.headers_mut().insert("retry-after", value);
    }
    response
}

/// The gate. Reads the register on every request.
#[allow(
    clippy::result_large_err,
    reason = "the Err IS an axum Response, because a refusal here is a whole \n              HTTP answer with a sentence in it — boxing it would cost an \n              allocation on the path a rejected flood takes"
)]
fn authenticate(shared: &Shared, headers: &HeaderMap, from: Peer) -> Result<Device, Response> {
    // Version first, so a client that is too old is told to update rather than told its
    // credential is bad.
    if let Some(offered) = headers.get("x-magicbill-version") {
        let said = offered.to_str().unwrap_or("");
        if said != PROTOCOL_VERSION.to_string() {
            return Err(trouble(StatusCode::UPGRADE_REQUIRED, upgrade_message(said)));
        }
    }

    let Some(raw) = headers.get("authorization").and_then(|v| v.to_str().ok()) else {
        return Err(refused(&Refusal::NotPaired));
    };
    let Some(credential) = raw.strip_prefix("Bearer ") else {
        return Err(refused(&Refusal::NotPaired));
    };
    let Some((device_id, secret)) = credential.split_once('.') else {
        return Err(refused(&Refusal::NotPaired));
    };

    let key = if device_id.is_empty() {
        from.ip()
    } else {
        device_id.to_owned()
    };
    if let Err(wait) = shared.device_limiter.check(&key, (shared.clock)()) {
        return Err(too_many(wait));
    }

    // One refusal for "no such device" and "wrong secret".
    let Some(device) = shared.counter.authenticate(device_id, secret) else {
        return Err(refused(&Refusal::NotPaired));
    };
    shared.counter.seen(&device.id, &from.ip());
    Ok(device)
}

/// The sentence a phone on the wrong version shows.
#[must_use]
pub fn upgrade_message(offered: &str) -> String {
    let theirs: Option<u32> = offered.parse().ok();
    match theirs {
        Some(v) if v < PROTOCOL_VERSION => {
            "This phone's Magic Bill app is older than the counter's. Update \
             it from the Play Store."
                .to_owned()
        }
        Some(_) => "This phone's Magic Bill app is newer than the counter's. Update \
             Magic Bill on the counter PC."
            .to_owned(),
        None => "This phone did not say which version it is. Update the app.".to_owned(),
    }
}

/// Check a permission for a request, server-side.
pub fn require(device: &Device, need: mb_auth::Permission) -> Result<(), Refusal> {
    if device.permissions.has(need) {
        return Ok(());
    }
    Err(Refusal::NotAllowed(need.what().to_owned()))
}

// Running it.

/// A running server. Dropping it stops it.
pub struct Running {
    pub port: u16,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running").field("port", &self.port).finish()
    }
}

impl Running {
    /// Stop it and wait.
    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start the server on its own runtime, on its own thread.
pub fn start(shared: Shared, port: u16, tls: Option<TlsConfig>) -> Result<Running, LanError> {
    start_on(shared, Ipv4Addr::UNSPECIFIED, port, tls)
}

/// The same, on one interface.
pub fn start_on(
    shared: Shared,
    interface: Ipv4Addr,
    port: u16,
    tls: Option<TlsConfig>,
) -> Result<Running, LanError> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u16, LanError>>();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    let thread = std::thread::Builder::new()
        .name("mb-lan".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = ready_tx.send(Err(LanError::Io(e.to_string())));
                    return;
                }
            };
            runtime.block_on(serve(shared, interface, port, tls, ready_tx, stop_rx));
        })
        .map_err(|e| LanError::Io(e.to_string()))?;

    match ready_rx.recv() {
        Ok(Ok(bound)) => Ok(Running {
            port: bound,
            stop: Some(stop_tx),
            thread: Some(thread),
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(LanError::Io(
            "the network thread stopped before it started".into(),
        )),
    }
}

/// The TLS material, already parsed.
#[derive(Clone)]
pub struct TlsConfig(Arc<rustls::ServerConfig>);

impl std::fmt::Debug for TlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TlsConfig")
    }
}

impl TlsConfig {
    /// Build one from the counter's own certificate.
    pub fn from_identity(identity: &crate::identity::Identity) -> Result<TlsConfig, LanError> {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};

        let der = crate::identity::der_of(&identity.certificate_pem)
            .ok_or_else(|| LanError::Identity("the certificate could not be decoded".into()))?;
        let key = key_der(&identity.key_pem)
            .ok_or_else(|| LanError::Identity("the private key could not be decoded".into()))?;

        let config = rustls::ServerConfig::builder()
            // No client certificates. The phone proves itself with a bearer credential over the
            // encrypted channel — issuing a client certificate per phone would be a second PKI
            // to manage on a machine that has no administrator.
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(der)],
                PrivateKeyDer::try_from(key).map_err(|e| LanError::Identity(e.to_string()))?,
            )
            .map_err(|e| LanError::Identity(e.to_string()))?;
        Ok(TlsConfig(Arc::new(config)))
    }
}

fn key_der(pem: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let body: String = pem
        .lines()
        .skip_while(|l| !l.starts_with("-----BEGIN"))
        .skip(1)
        .take_while(|l| !l.starts_with("-----END"))
        .collect();
    base64::engine::general_purpose::STANDARD.decode(body).ok()
}

async fn serve(
    shared: Shared,
    interface: Ipv4Addr,
    port: u16,
    tls: Option<TlsConfig>,
    ready: std::sync::mpsc::Sender<Result<u16, LanError>>,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) {
    let address = SocketAddr::new(IpAddr::V4(interface), port);
    let listener = match tokio::net::TcpListener::bind(address).await {
        Ok(l) => l,
        Err(e) => {
            let _ = ready.send(Err(LanError::Listen {
                port,
                why: e.to_string(),
            }));
            return;
        }
    };
    let bound = listener.local_addr().map_or(port, |a| a.port());
    let _ = ready.send(Ok(bound));

    let app = router(shared).into_make_service_with_connect_info::<Peer>();
    let shutdown = async move {
        let _ = (&mut stop).await;
    };

    match tls {
        None => {
            // Plain TCP. Used by the tests, and by nothing else.
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(shutdown)
                .await;
        }
        Some(config) => {
            let _ = axum::serve(TlsListener::start(listener, config), app)
                .with_graceful_shutdown(shutdown)
                .await;
        }
    }
}

/// A listener that hands axum sockets which have already finished their TLS handshake.
struct TlsListener {
    ready: tokio::sync::mpsc::Receiver<(
        tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
        SocketAddr,
    )>,
    local: SocketAddr,
}

impl TlsListener {
    fn start(listener: tokio::net::TcpListener, config: TlsConfig) -> TlsListener {
        let local = listener
            .local_addr()
            .unwrap_or_else(|_| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
        // Bounded: a burst of connections must not become an unbounded queue inside the
        // counter.
        let (tx, ready) = tokio::sync::mpsc::channel(16);
        let acceptor = tokio_rustls::TlsAcceptor::from(config.0);
        tokio::spawn(async move {
            loop {
                let Ok((socket, peer)) = listener.accept().await else {
                    continue;
                };
                let acceptor = acceptor.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let handshake =
                        tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(socket)).await;
                    if let Ok(Ok(stream)) = handshake {
                        let _ = tx.send((stream, peer)).await;
                    }
                });
            }
        });
        TlsListener { ready, local }
    }
}

/// Ten seconds to complete a TLS handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            if let Some(ready) = self.ready.recv().await {
                return ready;
            }
            // The feeding task is gone, which only happens when the runtime is shutting down.
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local)
    }
}
