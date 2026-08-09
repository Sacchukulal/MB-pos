//! **The counter's side of the road** — P19, decision D9.
//!
//! `mb-lan` owns the sockets and knows nothing about the shop. This file is the
//! other half of that seam: it implements `mb_lan::Counter` against the real
//! database, and it owns the four commands the panel needs.
//!
//! # The rule that shapes every line here
//!
//! **Nothing on this path may reach the writer connection while a phone is
//! waiting**, because the writer belongs to the till. So:
//!
//! * `authenticate` and `devices` use `Db::read_transaction` (P18 added it for
//!   exactly this);
//! * `seen` — which fires on EVERY request a phone makes — writes, and is
//!   therefore **batched**: it remembers in memory and flushes on a timer. A
//!   write on every poll would put fifteen phones on the till's writer.
//!
//! # The lock ordering, said out loud
//!
//! A handler holds `App`'s shop mutex for the length of one `with_shop` call
//! and never across an `await`. mb-lan is `async`; this file is not. The seam
//! between them is a plain function call that returns owned data, which is what
//! makes the deadlock impossible rather than merely unlikely — and the one time
//! that discipline slipped in P18, the test suite hung instead of failing.

use std::sync::{Arc, Mutex};

use mb_auth::audit::{AuditEntry, action};
use mb_auth::Permission;
use mb_core::{StaffId, Timestamp};
use mb_db::repo::devices::LanDevice;
use serde::Serialize;
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

/// Until P21 issues a licence with a plan on it, this is the number.
///
/// Fifteen is a large restaurant's floor staff. It is not a commercial
/// decision — it is a ceiling that stops a shop accidentally running a hundred
/// devices before anybody has measured what that does.
const DEFAULT_DEVICE_LIMIT: u32 = 15;

/// The bridge between mb-lan and the shop.
///
/// It holds a `tauri::AppHandle` and looks `App` up through it, exactly as
/// `push.rs` does. Not an `Arc<App>`: Tauri OWNS the `App`, and a second
/// owner living on a background thread would keep the whole shop — database,
/// print queue and all — alive after the window closed.
pub struct Bridge {
    handle: tauri::AppHandle,
    /// "Last seen", waiting to be written. See the module note.
    pending: Mutex<Vec<(String, String, Timestamp)>>,
}

impl Bridge {
    #[must_use]
    pub fn new(handle: tauri::AppHandle) -> Arc<Bridge> {
        Arc::new(Bridge {
            handle,
            pending: Mutex::new(Vec::new()),
        })
    }

    fn app(&self) -> Option<tauri::State<'_, App>> {
        use tauri::Manager as _;
        self.handle.try_state::<App>()
    }

    /// Write the "last seen" marks that have piled up.
    ///
    /// Called on a timer by `magic-bill`, never by a request. One transaction
    /// for all of them, which is one fsync for a minute of fifteen phones
    /// polling rather than one per poll (D23 and budget B5's reasoning).
    pub fn flush_seen(&self) {
        let batch: Vec<_> = {
            let mut pending = lock(&self.pending);
            if pending.is_empty() {
                return;
            }
            std::mem::take(&mut *pending)
        };
        let Some(handle) = self.app() else { return };
        let _ = handle.with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let repos = mb_db::Repos::new(tx);
                    for (id, ip, at) in &batch {
                        repos.devices().seen(OUTLET, id, *at, ip)?;
                    }
                    Ok(())
                })
                .map_err(|e| words::from_db(&e))
        });
    }
}

impl mb_lan::Counter for Bridge {
    fn shop_name(&self) -> String {
        self.app()
            .map(|h| h.shop_config().store.name.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Magic Bill".to_owned())
    }

    fn device_limit(&self) -> u32 {
        // P21 owns licensing. Until it lands this is the ceiling, and the seam
        // is named rather than built — which is the same treatment the recovery
        // code's support path got at P11.
        DEFAULT_DEVICE_LIMIT
    }

    fn devices(&self) -> Vec<mb_lan::DeviceRow> {
        let Some(handle) = self.app() else {
            return Vec::new();
        };
        handle
            .with_shop(|shop| {
                shop.db
                    .read_transaction(|tx| mb_db::Repos::new(tx).devices().all(OUTLET))
                    .map_err(|e| words::from_db(&e))
            })
            .unwrap_or_default()
            .iter()
            .filter(|d| d.is_live())
            .map(row_of)
            .collect()
    }

    fn authenticate(&self, device_id: &str, secret: &str) -> Option<mb_lan::Device> {
        let handle = self.app()?;
        // **A READER, on every request.** The revoke has to bite immediately
        // (T3), and the writer belongs to the till.
        let device = handle
            .with_shop(|shop| {
                shop.db
                    .read_transaction(|tx| mb_db::Repos::new(tx).devices().live(OUTLET, device_id))
                    .map_err(|e| words::from_db(&e))
            })
            .ok()
            .flatten()?;

        let hash = mb_auth::PinHash::from_stored(&device.secret_hash).ok()?;
        if !mb_auth::verify_device_secret(secret, &hash) {
            return None;
        }

        // What this device's PERSON may do. A device with no staff member is a
        // shared tablet, and it gets the shop's floor permissions rather than
        // nothing — otherwise a tablet at the pass could not take an order,
        // which is the only thing it exists to do.
        let permissions = device
            .staff_id
            .as_ref()
            .and_then(|id| staff_permissions(&handle, id.as_str()))
            .unwrap_or_else(floor_permissions);

        Some(mb_lan::Device {
            id: device.id,
            name: device.name,
            staff_id: device.staff_id.map(|s| s.as_str().to_owned()),
            permissions,
        })
    }

    fn seen(&self, device_id: &str, ip: &str) {
        // Remembered, not written. `flush_seen` does the writing on a timer.
        let mut pending = lock(&self.pending);
        pending.retain(|(id, _, _)| id != device_id);
        pending.push((device_id.to_owned(), ip.to_owned(), now()));
    }

    fn pair(
        &self,
        _request: &mb_lan::PairRequest,
        name: &str,
        platform: &str,
    ) -> Result<mb_lan::PairedDevice, mb_lan::Refusal> {
        let handle = self
            .app()
            .ok_or_else(|| mb_lan::Refusal::Refused("The counter is closing.".to_owned()))?;
        let at = now();
        let (secret, hash) = mb_auth::new_device_secret()
            .map_err(|_| mb_lan::Refusal::Refused("The credential could not be made.".to_owned()))?;
        let id = format!("dev_{}", mb_auth::random_token(12));
        let who = handle.sessions().current().map(|s| s.actor.staff_id);

        handle
            .with_shop(|shop| {
                shop.db
                    .transaction(|tx| {
                        let repos = mb_db::Repos::new(tx);
                        repos.devices().pair(
                            OUTLET,
                            &LanDevice {
                                id: id.clone(),
                                name: name.to_owned(),
                                platform: platform.to_owned(),
                                secret_hash: hash.as_str().to_owned(),
                                staff_id: who.clone(),
                                paired_at: at,
                                paired_by: who.clone(),
                                last_seen_at: None,
                                last_ip: None,
                                revoked_at: None,
                            },
                            who.as_ref(),
                        )?;
                        // R11 — the same transaction as the thing it records.
                        repos.audit().append(
                            OUTLET,
                            &AuditEntry::new(at, today(at), who.clone(), action::DEVICE_PAIRED, "device")
                                .about(id.clone())
                                .with_after(serde_json::json!({
                                    "name": name,
                                    "platform": platform,
                                })),
                        )?;
                        Ok(())
                    })
                    .map_err(|e| words::from_db(&e))
            })
            .map_err(|e| mb_lan::Refusal::Refused(e.message))?;

        Ok(mb_lan::PairedDevice {
            device_id: id,
            secret: secret.to_issue().to_owned(),
            server_id: handle.lan_server_id(),
        })
    }
}

fn row_of(device: &LanDevice) -> mb_lan::DeviceRow {
    mb_lan::DeviceRow {
        id: device.id.clone(),
        name: device.name.clone(),
        platform: device.platform.clone(),
        staff: device.staff_id.as_ref().map(|s| s.as_str().to_owned()),
        last_seen: device
            .last_seen_at
            .map_or_else(|| "not yet".to_owned(), words::when),
        last_ip: device.last_ip.clone().unwrap_or_default(),
    }
}

fn staff_permissions(app: &App, staff_id: &str) -> Option<mb_auth::PermissionSet> {
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let person = repos.people().find_staff(OUTLET, staff_id)?;
                let Some(person) = person else {
                    return Ok(None);
                };
                let Some(role_id) = person.role_id else {
                    return Ok(None);
                };
                Ok(repos
                    .people()
                    .list_roles(OUTLET)?
                    .into_iter()
                    .find(|r| r.id == role_id)
                    .map(|r| r.permissions))
            })
            .map_err(|e| words::from_db(&e))
    })
    .ok()
    .flatten()
}

/// What a shared tablet may do: take an order, and nothing else.
///
/// **Not "everything" and not "nothing".** Nothing makes the tablet useless;
/// everything puts voids and reports on a device four people touch in an hour,
/// which is audit C1 rebuilt in a different room.
fn floor_permissions() -> mb_auth::PermissionSet {
    let mut set = mb_auth::PermissionSet::new();
    set.insert(Permission::BillCreate);
    set
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// The panel.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct NetworkView {
    /// **The sentence at the top**, and it is the whole point of the screen:
    /// "Phones can reach this counter at 192.168.1.7." — or the opposite, with
    /// what to check.
    pub headline: String,
    /// `ok`, `warn` or `danger`. The headline says it in words too (§2).
    pub tone: String,
    pub address: String,
    pub port: u32,
    pub fingerprint: String,
    /// Written when the counter's certificate is new, because every phone must
    /// then be added again — and fifteen waiters discovering that one at a
    /// time during a rush is the alternative to saying so here.
    pub certificate_note: String,
    pub devices: Vec<DeviceRowView>,
    pub waiting: Vec<WaitingView>,
    /// The QR, as rows of `#`/`.` — drawn by the screen as a CSS grid. Empty
    /// unless pairing is open.
    pub qr: Vec<String>,
    pub code: String,
    pub may_pair: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DeviceRowView {
    pub id: String,
    pub name: String,
    pub platform: String,
    /// "Ravi", or "shared" when no one person owns it.
    pub staff: String,
    pub last_seen: String,
    pub last_ip: String,
    pub is_live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct WaitingView {
    pub request_id: String,
    pub name: String,
    pub platform: String,
    pub ip: String,
    /// The whole sentence: "SM-A146B is asking to join, from 192.168.1.31."
    pub says: String,
}

pub fn view_on(app: &App) -> UiResult<NetworkView> {
    let who = guard::require(app, Permission::ReportsView)?;
    let may_pair = who.must(Permission::DevicesPair).is_ok();
    let network = app.network();

    let devices = app
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| mb_db::Repos::new(tx).devices().all(OUTLET))
                .map_err(|e| words::from_db(&e))
        })
        .unwrap_or_default();

    let showing = network
        .as_ref()
        .and_then(|n| n.shared.desk.showing(now()));
    let (qr, code) = match (&network, &showing) {
        (Some(n), Some((token, code))) => {
            let uri = mb_lan::qr::pairing_uri(&n.address, n.port, &n.fingerprint, token);
            let rows = mb_lan::qr::matrix(&uri)
                .map(|m| {
                    m.iter()
                        .map(|row| row.iter().map(|d| if *d { '#' } else { '.' }).collect())
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default();
            (rows, code.clone())
        }
        _ => (Vec::new(), String::new()),
    };

    let (headline, tone) = match &network {
        None => (
            "Phones cannot reach this counter — the network is switched off. \
             Turn it on to take orders from a phone."
                .to_owned(),
            "warn".to_owned(),
        ),
        Some(n) if n.address.is_empty() => (
            "This counter is not on a network. Connect it to the shop's WiFi \
             or plug in the network cable, and phones will find it."
                .to_owned(),
            "danger".to_owned(),
        ),
        // **"Listening" is not "reachable", and this sentence must not pretend
        // otherwise.** Found by running it: the very first launch raised a
        // Windows Firewall prompt, and a shopkeeper who presses Cancel has a
        // counter that is listening perfectly and that no phone can see. The
        // panel cannot tell the difference from inside the process — an
        // inbound packet that never arrives looks exactly like no phone
        // trying — so it says what it knows and then says what to check.
        Some(n) => (
            format!(
                "This counter is waiting for phones at {} on port {}. If a \
                 phone cannot find it, Windows Firewall is the usual reason: \
                 allow Magic Bill on private networks.",
                n.address, n.port
            ),
            "ok".to_owned(),
        ),
    };

    Ok(NetworkView {
        headline,
        tone,
        address: network.as_ref().map(|n| n.address.clone()).unwrap_or_default(),
        port: network.as_ref().map_or(0, |n| u32::from(n.port)),
        fingerprint: network
            .as_ref()
            .map(|n| n.fingerprint.clone())
            .unwrap_or_default(),
        certificate_note: if network.as_ref().is_some_and(|n| n.is_new_certificate) {
            "This counter has a new security certificate, so every phone has \
             to be added again."
                .to_owned()
        } else {
            String::new()
        },
        devices: devices
            .iter()
            .map(|d| DeviceRowView {
                id: d.id.clone(),
                name: d.name.clone(),
                platform: d.platform.clone(),
                staff: d
                    .staff_id
                    .as_ref()
                    .and_then(|s| staff_name(app, s.as_str()))
                    .unwrap_or_else(|| "shared".to_owned()),
                last_seen: d
                    .last_seen_at
                    .map_or_else(|| "not yet".to_owned(), words::when),
                last_ip: d.last_ip.clone().unwrap_or_default(),
                is_live: d.is_live(),
            })
            .collect(),
        waiting: network
            .as_ref()
            .map(|n| {
                n.shared
                    .desk
                    .waiting()
                    .iter()
                    .map(|w| WaitingView {
                        request_id: w.request_id.clone(),
                        name: w.name.clone(),
                        platform: w.platform.clone(),
                        ip: w.ip.clone(),
                        says: format!("{} is asking to join, from {}.", w.name, w.ip),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        qr,
        code,
        may_pair,
    })
}

fn staff_name(app: &App, staff_id: &str) -> Option<String> {
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| mb_db::Repos::new(tx).people().find_staff(OUTLET, staff_id))
            .map_err(|e| words::from_db(&e))
    })
    .ok()
    .flatten()
    .map(|p| p.name)
}

/// Show a pairing code. **Nothing is issued by this** — it only opens the door
/// that a person then has to walk somebody through.
pub fn open_pairing_on(app: &App) -> UiResult<NetworkView> {
    guard::require(app, Permission::DevicesPair)?;
    let network = app.network().ok_or_else(|| {
        UiError::new(
            "lan.off",
            "The counter's network is switched off, so there is nothing for a \
             phone to join.",
        )
    })?;
    network.shared.desk.open(now());
    view_on(app)
}

pub fn close_pairing_on(app: &App) -> UiResult<NetworkView> {
    guard::require(app, Permission::DevicesPair)?;
    if let Some(network) = app.network() {
        network.shared.desk.close();
    }
    view_on(app)
}

/// **A person pressed Allow.** This is the second of the two things that must
/// both be true, and it is the one that matters — see `mb_lan::pairing`.
pub fn allow_on(app: &App, request_id: String) -> UiResult<NetworkView> {
    guard::require(app, Permission::DevicesPair)?;
    let network = app.network().ok_or_else(|| {
        UiError::new("lan.off", "The counter's network is switched off.")
    })?;
    let waiting = network.shared.desk.take(&request_id).ok_or_else(|| {
        UiError::new(
            "lan.gone",
            "That phone is no longer asking. Show a new code and try again.",
        )
    })?;

    let device = mb_lan::Counter::pair(
        network.bridge.as_ref(),
        &mb_lan::PairRequest {
            name: waiting.name.clone(),
            platform: waiting.platform.clone(),
            token: String::new(),
        },
        &waiting.name,
        &waiting.platform,
    )
    .map_err(|r| UiError::new("lan.refused", r.message()))?;

    network.shared.desk.approve(&request_id, device);
    network.shared.desk.close();
    view_on(app)
}

pub fn refuse_on(app: &App, request_id: String) -> UiResult<NetworkView> {
    guard::require(app, Permission::DevicesPair)?;
    if let Some(network) = app.network() {
        network.shared.desk.refuse(&request_id);
    }
    view_on(app)
}

/// Take a phone off the counter.
pub fn revoke_on(app: &App, device_id: String) -> UiResult<NetworkView> {
    let who = guard::require(app, Permission::DevicesPair)?;
    let at = now();
    let removed = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let removed =
                    repos
                        .devices()
                        .revoke(OUTLET, &device_id, at, Some(&who.staff_id))?;
                if removed {
                    repos.audit().append(
                        OUTLET,
                        &AuditEntry::new(
                            at,
                            today(at),
                            Some(who.staff_id.clone()),
                            action::DEVICE_REVOKED,
                            "device",
                        )
                        .about(device_id.clone()),
                    )?;
                }
                Ok(removed)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    if !removed {
        return Err(UiError::new(
            "lan.already_off",
            "That phone had already been removed.",
        ));
    }
    view_on(app)
}

// ---------------------------------------------------------------------------
// The seats.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn network(app: tauri::State<'_, App>) -> UiResult<NetworkView> {
    view_on(&app)
}

#[tauri::command]
pub fn open_pairing(app: tauri::State<'_, App>) -> UiResult<NetworkView> {
    open_pairing_on(&app)
}

#[tauri::command]
pub fn close_pairing(app: tauri::State<'_, App>) -> UiResult<NetworkView> {
    close_pairing_on(&app)
}

#[tauri::command]
pub fn allow_device(app: tauri::State<'_, App>, request_id: String) -> UiResult<NetworkView> {
    allow_on(&app, request_id)
}

#[tauri::command]
pub fn refuse_device(app: tauri::State<'_, App>, request_id: String) -> UiResult<NetworkView> {
    refuse_on(&app, request_id)
}

#[tauri::command]
pub fn revoke_device(app: tauri::State<'_, App>, device_id: String) -> UiResult<NetworkView> {
    revoke_on(&app, device_id)
}

/// What `App` keeps while the server is up.
pub struct Network {
    pub shared: mb_lan::Shared,
    pub bridge: Arc<Bridge>,
    pub address: String,
    pub port: u16,
    pub fingerprint: String,
    pub server_id: String,
    pub is_new_certificate: bool,
    /// Dropping these stops the server and withdraws the advertisement.
    pub running: mb_lan::Running,
    pub advertisement: Option<mb_lan::discovery::Advertisement>,
}

impl std::fmt::Debug for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Network")
            .field("address", &self.address)
            .field("port", &self.port)
            .finish()
    }
}

/// Unused today, and named rather than guessed: [`StaffId`] is what the audit
/// rows above carry.
const _: Option<StaffId> = None;

// ---------------------------------------------------------------------------
// Starting it.
// ---------------------------------------------------------------------------

/// Bring the counter onto the network.
///
/// Called once at start-up, from `main`. **It never returns an error upward**:
/// a counter that refuses to start because a port is taken is a counter that
/// stops a restaurant billing, and requirement 3 says it must not. Everything
/// that goes wrong is logged and shows up on the panel as a sentence.
pub fn start(handle: &tauri::AppHandle) {
    use tauri::Manager as _;

    let Some(app) = handle.try_state::<App>() else {
        return;
    };
    let config_dir = crate::config::AppConfig::directory();
    let addresses = mb_lan::identity::local_addresses();

    let identity = match mb_lan::Identity::load_or_create(
        &mb_lan::identity::folder(&config_dir),
        &addresses,
    ) {
        Ok(identity) => Arc::new(identity),
        Err(e) => {
            crate::log_error!("the counter's network identity could not be made: {e}");
            return;
        }
    };
    if identity.is_new {
        crate::log_info!(
            "this counter has a NEW network certificate; every phone will have \
             to be added again"
        );
    }

    let bridge = Bridge::new(handle.clone());
    let shared = mb_lan::Shared::new(
        Arc::clone(&bridge) as Arc<dyn mb_lan::Counter>,
        Arc::clone(&identity),
        Arc::new(now),
    );
    let tls = match mb_lan::TlsConfig::from_identity(&identity) {
        Ok(tls) => tls,
        Err(e) => {
            crate::log_error!("the counter's certificate could not be used: {e}");
            return;
        }
    };

    let running = match mb_lan::start(shared.clone(), mb_lan::DEFAULT_PORT, Some(tls)) {
        Ok(running) => running,
        Err(e) => {
            crate::log_error!("the counter could not go onto the network: {e}");
            return;
        }
    };

    let address = addresses
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();

    // mDNS is the convenient path, never the necessary one. A shop whose router
    // blocks multicast still adds phones by QR, so a failure here is logged and
    // the counter carries on.
    let advertisement = mb_lan::discovery::Advertisement::start(
        &app.shop_config().store.name,
        running.port,
        &addresses,
        &[
            ("id", identity.server_id.as_str()),
            ("v", "1"),
            ("fp", identity.fingerprint.as_str()),
        ],
    )
    .inspect_err(|e| crate::log_warn!("phones will not find this counter automatically: {e}"))
    .ok();

    crate::log_info!(
        "the counter is on the network at {address}:{}",
        running.port
    );

    app.set_network(Some(Arc::new(Network {
        shared,
        bridge: Arc::clone(&bridge),
        address,
        port: running.port,
        fingerprint: identity.fingerprint.clone(),
        server_id: identity.server_id.clone(),
        is_new_certificate: identity.is_new,
        running,
        advertisement,
    })));

    // The "last seen" flush. **A timer and not a write per request**: fifteen
    // phones polling would otherwise put fifteen writes a second on the
    // connection the till settles bills with.
    let ticking = handle.clone();
    std::thread::Builder::new()
        .name("mb-lan-seen".to_owned())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(30));
                let Some(app) = ticking.try_state::<App>() else {
                    return;
                };
                let Some(network) = app.network() else { continue };
                network.bridge.flush_seen();
            }
        })
        .ok();
}
