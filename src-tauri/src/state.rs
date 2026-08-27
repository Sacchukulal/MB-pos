//! What the process holds for its whole life, and the channel it pushes down.

use std::sync::{Arc, Mutex, MutexGuard};

use mb_db::Db;
use mb_print::font::Font;
use mb_print::printer::{Engine, PrinterConfig, Role, Target};
use mb_print::queue::sqlite::SqliteStore;
use mb_print::queue::{JobStore, MemoryStore, Queue, QueueConfig};
use serde::Serialize;
use ts_rs::TS;

use crate::billing::CartState;
use crate::config::AppConfig;
use crate::session::{Sessions, stand_in_actor};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

/// The outlet every query is scoped to.
pub const OUTLET: &str = "outlet_default";

/// Everything the process holds.
#[derive(Debug)]
pub struct App {
    /// `None` on a first run, and that is a state rather than a failure: the window opens, and
    /// it offers to create a shop or restore a backup.
    shop: Mutex<Option<Shop>>,
    config: Mutex<AppConfig>,
    /// The cart lives in Rust.
    cart: Mutex<CartState>,
    /// One counter action at a time — see `App::begin_action`.
    action: Mutex<()>,
    /// The faces this counter can print in, loaded once each.
    faces: Arc<crate::typefaces::SystemFaces>,
    /// Who is at the counter.
    sessions: Sessions,
    /// The shop's settings, loaded once.
    shop_config: Mutex<crate::settings::ShopConfig>,
    /// Who answers "did the money arrive?".
    provider: std::sync::RwLock<Arc<dyn mb_core::provider::Provider>>,
    /// The counter as a server.
    network: Mutex<Option<Arc<crate::lan::Network>>>,
    /// The licence, and everything that talks to the cloud about it.
    licensing: Mutex<mb_license::Licensing>,
    /// The decided entitlement, held.
    entitlement: std::sync::RwLock<mb_license::Entitlement>,
    /// What the counter knows about updates.
    updates: Mutex<crate::updates::UpdateState>,
    /// Which till this machine is.
    terminal_id: String,
}

/// An open shop: the data and everything that hangs off it.
#[derive(Debug)]
pub struct Shop {
    pub db: Arc<Db>,
    pub path: std::path::PathBuf,
    pub queue: Queue,
}

impl App {
    pub fn new(config: AppConfig) -> Result<App, UiError> {
        let faces = crate::typefaces::SystemFaces::new().map_err(|e| words::from_print(&e))?;
        let sessions = Sessions::new();
        // A shop that does not exist yet has nothing to lock.
        sessions.begin(
            stand_in_actor("Counter", DEFAULT_STAFF),
            crate::flows::now(),
            true,
        );
        let now = crate::flows::now();
        // A test must not read the licence of whoever is running it.
        #[cfg(test)]
        let licensing = crate::licensing::for_tests();
        #[cfg(not(test))]
        let licensing = crate::licensing::start();
        let entitlement = licensing.entitlement(now, crate::flows::today(now));
        Ok(App {
            shop: Mutex::new(None),
            config: Mutex::new(config),
            cart: Mutex::new(CartState::default()),
            action: Mutex::new(()),
            faces: Arc::new(faces),
            sessions,
            shop_config: Mutex::new(crate::settings::ShopConfig::default()),
            provider: std::sync::RwLock::new(Arc::new(mb_core::provider::Manual)),
            network: Mutex::new(None),
            licensing: Mutex::new(licensing),
            entitlement: std::sync::RwLock::new(entitlement),
            updates: Mutex::new(crate::updates::UpdateState {
                running: crate::updates::Version::running().to_string(),
                is_dev_build: !crate::updates::is_a_release_build(),
                ..crate::updates::UpdateState::default()
            }),
            terminal_id: crate::terminals::me(&AppConfig::directory()).terminal_id,
        })
    }

    /// Which till this machine is.
    #[must_use]
    pub fn terminal_id(&self) -> &str {
        &self.terminal_id
    }

    /// Who to ask about a payment.
    #[must_use]
    pub fn provider(&self) -> Arc<dyn mb_core::provider::Provider> {
        match self.provider.read() {
            Ok(guard) => Arc::clone(&guard),
            // A poisoned lock must not stop a sale.
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }

    /// Put a stand-in provider in place.
    #[cfg(test)]
    pub fn use_provider(&self, provider: Arc<dyn mb_core::provider::Provider>) {
        match self.provider.write() {
            Ok(mut guard) => *guard = provider,
            Err(poisoned) => *poisoned.into_inner() = provider,
        }
    }

    /// Make this `App` a different till — before it opens a shop.
    #[cfg(test)]
    #[must_use]
    pub fn becoming_till(mut self, id: &str) -> App {
        self.terminal_id = id.to_owned();
        self
    }

    /// What the counter knows about updates.
    #[must_use]
    pub fn updates(&self) -> crate::updates::UpdateState {
        lock(&self.updates).clone()
    }

    /// Replace it, after a check.
    pub fn set_updates(&self, state: crate::updates::UpdateState) {
        *lock(&self.updates) = state;
    }

    /// Where releases come from.
    #[must_use]
    pub fn releases(&self) -> &dyn crate::updates::Releases {
        &crate::updates::NoReleaseServerYet
    }

    /// What this shop is entitled to, right now.
    #[must_use]
    pub fn entitlement(&self) -> mb_license::Entitlement {
        match self.entitlement.read() {
            Ok(held) => held.clone(),
            // A poisoned lock means another thread panicked while holding it.
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Do something with the licence, and re-decide afterwards.
    pub fn with_licensing<T>(&self, f: impl FnOnce(&mut mb_license::Licensing) -> T) -> T {
        let outcome = {
            let mut held = lock(&self.licensing);
            f(&mut held)
        };
        self.re_decide();
        outcome
    }

    /// Put a licensing subsystem in, and decide again.
    #[cfg(test)]
    pub fn use_licensing(&self, licensing: mb_license::Licensing) {
        *lock(&self.licensing) = licensing;
        self.re_decide();
    }

    /// Read the licence without changing it.
    pub fn with_licence<T>(&self, f: impl FnOnce(&mb_license::Licensing) -> T) -> T {
        let held = lock(&self.licensing);
        f(&held)
    }

    /// Decide again from the cached snapshot, and hold the answer.
    pub fn re_decide(&self) {
        let now = crate::flows::now();
        let fresh = {
            let held = lock(&self.licensing);
            held.entitlement(now, crate::flows::today(now))
        };
        if let Ok(mut slot) = self.entitlement.write() {
            *slot = fresh;
        }
    }

    /// Remember that the floor changed the order the cashier has open.
    pub fn note_floor_change(&self, change: crate::orders::FloorChange) {
        let _ = self.with_cart_mut(|state| {
            state.from_the_floor.push(change);
            Ok(())
        });
    }

    /// How many floor changes the cashier has not looked at.
    #[must_use]
    pub fn floor_changes_waiting(&self) -> u32 {
        self.with_cart(|state| Ok(u32::try_from(state.from_the_floor.len()).unwrap_or(u32::MAX)))
            .unwrap_or(0)
    }

    /// The counter as a server, if it is on.
    #[must_use]
    pub fn network(&self) -> Option<Arc<crate::lan::Network>> {
        lock(&self.network).clone()
    }

    pub fn set_network(&self, network: Option<Arc<crate::lan::Network>>) {
        *lock(&self.network) = network;
    }

    /// This counter's stable identity, for a phone that has to recognise it again after a DHCP
    /// move.
    #[must_use]
    pub fn lan_server_id(&self) -> String {
        lock(&self.network)
            .as_ref()
            .map(|n| n.server_id.clone())
            .unwrap_or_default()
    }

    /// What this shop has chosen.
    #[must_use]
    pub fn shop_config(&self) -> crate::settings::ShopConfig {
        lock(&self.shop_config).clone()
    }

    /// Every job this counter prints goes through here.
    pub fn print(&self, job: mb_print::queue::Job) -> UiResult<String> {
        let face = self.face_for(job.kind);
        let job = job.in_face(face);
        self.with_shop(|shop| shop.queue.enqueue(job).map_err(|e| words::from_print(&e)))
    }

    /// The same answer as `App::face_for`, for the test that proves the settings screen reaches
    /// the printer.
    #[cfg(test)]
    #[must_use]
    pub fn face_for_test(&self, kind: mb_print::queue::JobKind) -> Option<String> {
        self.face_for(kind)
    }

    /// Which typeface a kind of document is printed in.
    #[must_use]
    fn face_for(&self, kind: mb_print::queue::JobKind) -> Option<String> {
        let config = self.shop_config();
        let chosen = match kind {
            mb_print::queue::JobKind::Kitchen => config.kitchen.font,
            _ => config.receipt.font,
        };
        Some(chosen).filter(|f| !f.is_empty())
    }

    /// Read the configuration from the open shop and publish it.
    pub fn reload_shop_config(&self) {
        let read = self.with_shop(|shop| {
            shop.db
                .transaction(|tx| crate::settings::load(&mb_db::Repos::new(tx), OUTLET))
                .map_err(|e| words::from_db(&e))
        });
        match read {
            Ok(config) => self.publish_shop_config(config),
            Err(e) if e.code == "shop.none" => {}
            // With the detail, which is the half a support call needs.
            Err(e) => log_warn!(
                "this shop's settings could not be read ({e}{}); the counter is \
                 using the standard ones until that is fixed",
                e.detail
                    .as_deref()
                    .map(|d| format!(" — {d}"))
                    .unwrap_or_default()
            ),
        }
    }

    /// Put a configuration in place, including the one number that lives outside this struct.
    pub fn publish_shop_config(&self, config: crate::settings::ShopConfig) {
        crate::flows::set_day_rule(config.day.rule());
        *lock(&self.shop_config) = config;
    }

    #[must_use]
    pub fn sessions(&self) -> &Sessions {
        &self.sessions
    }

    /// Take ownership of an open database and start everything that hangs off it.
    pub fn open_shop(&self, db: Db, path: std::path::PathBuf) {
        let db = Arc::new(db);
        ensure_someone_can_bill(&db);
        ensure_the_roles_exist(&db);
        self.open_or_lock(&db);
        let queue = self.build_queue(&db);
        let previous = lock(&self.shop).replace(Shop {
            db,
            path: path.clone(),
            queue,
        });
        // Shutting the old queue down after the new one is in place, so there is never a window
        // with no queue at all.
        if let Some(old) = previous {
            old.queue.shutdown();
        }
        // After the shop is in place, because reading the settings needs a shop to read them
        // from.
        self.reload_shop_config();
        log_info!("the shop at {} is open", path.display());
    }

    pub fn rebuild_queue(&self) {
        let db = {
            let shop = lock(&self.shop);
            match shop.as_ref() {
                Some(shop) => Arc::clone(&shop.db),
                None => return,
            }
        };
        let fresh = self.build_queue(&db);
        let old = {
            let mut shop = lock(&self.shop);
            match shop.as_mut() {
                Some(shop) => Some(std::mem::replace(&mut shop.queue, fresh)),
                None => None,
            }
        };
        if let Some(old) = old {
            old.shutdown();
        }
    }

    /// Build the print queue from the printers the shop has configured.
    fn build_queue(&self, db: &Arc<Db>) -> Queue {
        // Before anything else, take the default off the placeholder if a real printer is
        // sitting behind it.
        retire_the_stand_in(db);

        let mut printers = db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .unwrap_or_else(|e| {
                log_warn!("the printer list could not be read ({e}); starting with none");
                Vec::new()
            })
            .iter()
            .map(printer_config_for)
            .collect::<Vec<_>>();

        // A shop with no printer set up must still be able to bill.
        if printers.is_empty() {
            log_warn!("no printer is set up; jobs will be spooled and printed nowhere");
            let row = fallback_row();
            match db.transaction(|tx| {
                mb_db::Repos::new(tx)
                    .settings()
                    .save_printer(OUTLET, &row, crate::flows::now())
            }) {
                Ok(()) => printers.push(printer_config_for(&row)),
                // Billing still works; printing does not, and the reason is written down where
                // an owner's support call can find it.
                Err(e) => log_warn!("the stand-in printer could not be saved ({e})"),
            }
        }

        log_info!(
            "starting the print queue with {} printer(s)",
            printers.len()
        );

        let store: Arc<dyn JobStore> = Arc::new(SqliteStore::new(Arc::clone(db), OUTLET));
        Queue::start(
            printers,
            store,
            Arc::clone(&self.faces) as Arc<dyn mb_print::font::Typefaces>,
            QueueConfig::default(),
        )
    }

    /// A queue with nowhere to store anything, for the moments there is no shop: a first run,
    /// and the minutes after a restore was requested.
    pub fn transient_queue(&self, printers: Vec<PrinterConfig>) -> Queue {
        Queue::start(
            printers,
            Arc::new(MemoryStore::new()),
            Arc::clone(&self.faces) as Arc<dyn mb_print::font::Typefaces>,
            QueueConfig::default(),
        )
    }

    pub fn with_shop<T>(&self, f: impl FnOnce(&Shop) -> UiResult<T>) -> UiResult<T> {
        let shop = lock(&self.shop);
        match shop.as_ref() {
            Some(shop) => f(shop),
            None => Err(words::no_shop_yet()),
        }
    }

    #[must_use]
    pub fn has_shop(&self) -> bool {
        lock(&self.shop).is_some()
    }

    /// The built-in face, for a caller that has no shop settings to consult.
    #[must_use]
    pub fn font(&self) -> Arc<Font> {
        self.faces.builtin()
    }

    /// The metrics a job of this kind will actually be drawn with.
    #[must_use]
    pub fn metrics_for(
        &self,
        kind: mb_print::queue::JobKind,
        printer: &mb_print::printer::PrinterConfig,
    ) -> (mb_print::metrics::Metrics, &'static str) {
        match printer.effective_engine() {
            mb_print::printer::Engine::Text => (
                mb_print::metrics::Metrics::printer_font(printer.paper),
                "text",
            ),
            mb_print::printer::Engine::Raster => (
                mb_print::metrics::Metrics::face(
                    printer.paper,
                    self.face_for(kind)
                        .map_or_else(|| self.font(), |key| self.face_named(&key)),
                ),
                "raster",
            ),
        }
    }

    /// The face a key names, the same way the queue resolves it.
    #[must_use]
    pub fn face_named(&self, key: &str) -> Arc<Font> {
        use mb_print::font::Typefaces;
        self.faces.face(Some(key))
    }

    pub fn config(&self) -> AppConfig {
        lock(&self.config).clone()
    }

    /// Change the stored configuration and write it, atomically.
    pub fn update_config(&self, f: impl FnOnce(&mut AppConfig)) {
        let mut config = lock(&self.config);
        f(&mut config);
        if let Err(e) = config.save() {
            // A window size that could not be saved is a small thing.
            log_warn!("the app configuration could not be saved: {e}");
        }
    }

    /// Stop everything, in the order that loses nothing.
    pub fn shutdown(&self) {
        if let Some(shop) = lock(&self.shop).take() {
            log_info!("shutting the print queue down");
            shop.queue.shutdown();
        }
    }
}

/// Who a bill is created by before anybody has logged in.
pub const DEFAULT_STAFF: &str = "staff_default";

/// Make sure the shop has somebody to bill as.
fn ensure_someone_can_bill(db: &Arc<Db>) {
    use mb_db::repo::people::{StaffMember, StaffStatus};

    let result = db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        if repos
            .people()
            .list_staff(OUTLET)?
            .iter()
            .any(|s| s.id.as_str() == DEFAULT_STAFF)
        {
            return Ok(false);
        }
        repos.people().save_staff(
            OUTLET,
            &StaffMember {
                id: mb_core::StaffId::new(DEFAULT_STAFF),
                name: "Counter".to_owned(),
                code: None,
                role_id: None,
                role_name: None,
                pin_hash: None,
                status: StaffStatus::Active,
                // No role, on purpose.
                permissions: mb_auth::PermissionSet::new(),
                max_discount_bp: None,
                max_discount: None,
            },
            crate::flows::now(),
        )?;
        Ok(true)
    });

    match result {
        Ok(true) => log_info!("added the default counter user so bills can be created"),
        Ok(false) => {}
        // Not fatal here: the failure will be reported in the cashier's own words at the moment
        // they try to bill, which is where it belongs.
        Err(e) => log_warn!("the default counter user could not be added ({e})"),
    }
}

/// The four roles a shop starts with.
fn ensure_the_roles_exist(db: &Arc<Db>) {
    let result = db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        if !repos.people().list_roles(OUTLET)?.is_empty() {
            return Ok(false);
        }
        for preset in mb_auth::RolePreset::ALL {
            repos
                .people()
                .save_role(OUTLET, &preset.shape(), crate::flows::now())?;
        }
        Ok(true)
    });

    match result {
        Ok(true) => log_info!("added the four starting roles"),
        Ok(false) => {}
        Err(e) => log_warn!("the starting roles could not be added ({e})"),
    }
}

impl App {
    /// Open unlocked, or open locked.
    fn open_or_lock(&self, db: &Arc<Db>) {
        match anybody_has_a_pin(db) {
            Ok(true) => {
                self.sessions.end();
                log_info!("this shop uses PINs; the counter is locked");
            }
            Ok(false) => {
                self.sessions.begin(
                    stand_in_actor("Counter", DEFAULT_STAFF),
                    crate::flows::now(),
                    true,
                );
                log_warn!("nobody has a PIN; anybody at this machine can do anything");
            }
            // A shop whose staff list will not read is a shop that must not silently open
            // unlocked.
            Err(e) => {
                self.sessions.end();
                log_warn!("the staff list could not be read ({e}); locking the counter");
            }
        }
    }

    /// Does anybody in this shop have a PIN?
    #[must_use]
    pub fn shop_has_a_pin(&self) -> bool {
        self.with_shop(|shop| Ok(anybody_has_a_pin(&shop.db).unwrap_or(false)))
            .unwrap_or(false)
    }

    /// Setting the FIRST PIN locks the app immediately — proving it works while that person is
    /// still standing there is worth four seconds.
    pub fn relock_if_this_was_the_first_pin(&self, had_a_pin: bool) {
        if had_a_pin || !self.shop_has_a_pin() {
            return;
        }
        let _ = self.with_shop(|shop| {
            let db = Arc::clone(&shop.db);
            self.open_or_lock(&db);
            Ok(())
        });
    }
}

impl App {
    /// Write one line of history.
    pub fn record(&self, entry: &mb_auth::AuditEntry) {
        let written = self.with_shop(|shop| {
            shop.db
                .transaction(|tx| mb_db::Repos::new(tx).audit().append(OUTLET, entry))
                .map_err(|e| words::from_db(&e))
        });
        if let Err(e) = written {
            log_warn!(
                "\"{}\" could not be written to the history: {e}",
                entry.action
            );
        }
    }

    pub fn record_lock(&self, who: &mb_auth::Actor) {
        self.record(&mb_auth::AuditEntry::new(
            crate::flows::now(),
            crate::flows::today(crate::flows::now()),
            Some(who.staff_id.clone()),
            mb_auth::audit::action::LOCKED,
            "staff",
        ));
    }
}

/// Does anybody at this shop have a PIN at all?
pub fn anybody_has_a_pin(db: &Arc<Db>) -> Result<bool, mb_db::DbError> {
    Ok(db
        .transaction(|tx| mb_db::Repos::new(tx).people().list_staff(OUTLET))?
        .iter()
        .any(|s| s.pin_hash.is_some()))
}

/// The id of the printer that exists when no printer exists.
pub const NO_PRINTER: &str = "prn_none";

/// The printer a shop has before it has a printer — a real row.
fn retire_the_stand_in(db: &Arc<Db>) {
    let outcome = db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        let printers = repos.settings().list_printers(OUTLET)?;

        let placeholder_holds_it = printers
            .iter()
            .find(|p| p.is_default)
            .is_some_and(|p| p.id == NO_PRINTER);
        if !placeholder_holds_it {
            return Ok(None);
        }
        // Prefer one that can print a bill — moving the default onto a kitchen-only printer
        // would trade one silent failure for another.
        let Some(real) = printers
            .iter()
            .filter(|p| p.id != NO_PRINTER && p.kind != "none")
            .find(|p| p.role == "bill" || p.role == "both")
            .or_else(|| {
                printers
                    .iter()
                    .find(|p| p.id != NO_PRINTER && p.kind != "none")
            })
        else {
            // No real printer yet.
            return Ok(None);
        };

        let at = crate::flows::now();
        for printer in &printers {
            let wanted = printer.id == real.id;
            if printer.is_default != wanted {
                repos.settings().save_printer(
                    OUTLET,
                    &mb_db::repo::settings::Printer {
                        is_default: wanted,
                        ..printer.clone()
                    },
                    at,
                )?;
            }
        }
        Ok(Some(real.name.clone()))
    });

    match outcome {
        Ok(Some(name)) => log_warn!(
            "bills were going to the stand-in printer while \"{name}\" was set up; \
             they go to \"{name}\" now"
        ),
        Ok(None) => {}
        // Not fatal. The shop opens, and the settings screen now says in words that nothing is
        // printing.
        Err(e) => log_warn!("the default printer could not be checked ({e})"),
    }
}

fn fallback_row() -> mb_db::repo::settings::Printer {
    mb_db::repo::settings::Printer {
        id: NO_PRINTER.to_owned(),
        name: "No printer set up yet".to_owned(),
        kind: "none".to_owned(),
        address: None,
        paper_mm: 80,
        is_default: true,
        can_kick_drawer: false,
        offset_x_mm: 0,
        offset_y_mm: 0,
        role: "both".to_owned(),
        engine: "raster".to_owned(),
        is_bold_dark: false,
    }
}

/// Mb-db stores a printer as a row of strings; mb-print wants a typed configuration.
pub fn printer_config_for(row: &mb_db::repo::settings::Printer) -> PrinterConfig {
    let target = match row.kind.as_str() {
        "spooler" => Target::Spooler {
            name: row.address.clone().unwrap_or_default(),
        },
        "network" => {
            let address = row.address.clone().unwrap_or_default();
            let (host, port) = address
                .rsplit_once(':')
                .map_or((address.as_str(), 9100_u16), |(h, p)| {
                    (h, p.parse().unwrap_or(9100))
                });
            Target::Network {
                host: host.to_owned(),
                port,
            }
        }
        "serial" => Target::Serial {
            port: row.address.clone().unwrap_or_default(),
            baud: 9600,
        },
        // "none", and anything a newer build wrote that this one does not know.
        _ => Target::None,
    };

    let mut printer = PrinterConfig::new(&row.id, &row.name, target);
    printer.paper = mb_print::paper::Paper {
        kind: match row.paper_mm {
            58 => mb_print::paper::PaperKind::Mm58,
            100 => mb_print::paper::PaperKind::Mm100,
            _ => mb_print::paper::PaperKind::Mm80,
        },
        offset: mb_print::paper::Offset::new(
            i32::try_from(row.offset_x_mm).unwrap_or(0),
            i32::try_from(row.offset_y_mm).unwrap_or(0),
        ),
    };
    printer.engine = if row.engine == "text" {
        Engine::Text
    } else {
        Engine::Raster
    };
    printer.role = match row.role.as_str() {
        "bill" => Role::Bill,
        "kitchen" => Role::Kitchen,
        _ => Role::Both,
    };
    printer.caps.drawer = row.can_kick_drawer;
    printer.drawer.enabled = row.can_kick_drawer;
    printer.bold_dark = row.is_bold_dark;
    printer.is_default = row.is_default;
    printer
}

// What the screens are told, without asking.

/// Everything Rust pushes. One enum, because one channel is easier to reason about than five,
/// and because a screen that has just attached wants the lot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Pushed {
    /// The print queue changed.
    PrintQueue { jobs: Vec<PrintJobView> },
    /// The screen locked itself, or somebody signed in or out.
    Session {
        /// `None` when the screen is locked.
        who: Option<String>,
        role: Option<String>,
        /// True while nobody has a PIN — the shell's undismissable banner.
        stand_in: bool,
    },
    /// A phone is asking to join.
    Pairing {
        /// How many phones are waiting for somebody to press Allow.
        waiting: u32,
    },
    /// The floor changed the order the cashier has open.
    FloorChanged { waiting: u32 },
    /// This till is holding bills the main till has not taken yet.
    Tills {
        /// How many bills are queued here.
        waiting: u32,
        /// The whole sentence, empty when there is nothing to say.
        says: String,
    },
    /// What the customer is being shown.
    CustomerBill {
        /// Every line, already priced and formatted.
        lines: Vec<DisplayLine>,
        total: String,
        /// The heading: the shop's name, or what the shop typed for an idle display.
        title: String,
        /// The UPI QR's payload, when there is one to show at payment.
        qr: String,
        /// True when there is nothing on the bill, so the display shows the shop's name instead
        /// of an empty table.
        idle: bool,
    },
}

/// One line, as the customer sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DisplayLine {
    pub name: String,
    pub qty: String,
    pub amount: String,
}

/// A print job, as a screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PrintJobView {
    pub id: String,
    pub printer: String,
    /// "Bill", "Kitchen ticket" — words, not tags.
    pub what: String,
    /// "Waiting", "Printing", "Failed — will try again", "Not printed".
    pub state: String,
    /// True only for a parked job: the one the cashier has to see.
    pub needs_attention: bool,
    pub reason: Option<String>,
    pub last_error: Option<String>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl App {
    /// A receiver of everything the print queue does, for `crate::push`.
    pub fn subscribe_to_queue(
        &self,
    ) -> Option<std::sync::mpsc::Receiver<mb_print::queue::QueueEvent>> {
        lock(&self.shop).as_ref().map(|shop| shop.queue.subscribe())
    }

    /// Everything unfinished, in the words a screen shows.
    pub fn print_queue_snapshot(&self) -> Vec<PrintJobView> {
        lock(&self.shop).as_ref().map_or_else(Vec::new, |shop| {
            shop.queue
                .snapshot()
                .iter()
                .map(crate::ipc::to_view)
                .collect()
        })
    }
}

impl App {
    /// Read the cart. The counter does one thing at a time.
    pub fn begin_action(&self) -> MutexGuard<'_, ()> {
        lock(&self.action)
    }

    pub fn with_cart<T>(&self, f: impl FnOnce(&CartState) -> UiResult<T>) -> UiResult<T> {
        f(&lock(&self.cart))
    }

    /// Change the cart, and get the new view back.
    pub fn with_cart_mut<T>(&self, f: impl FnOnce(&mut CartState) -> UiResult<T>) -> UiResult<T> {
        f(&mut lock(&self.cart))
    }

    /// One menu row, by id — and only one that is on the menu right now.
    pub fn find_menu_item(&self, id: &str) -> UiResult<mb_db::repo::menu::MenuItem> {
        self.with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    mb_db::Repos::new(tx)
                        .menu()
                        .find_item(&mb_core::ItemId::new(id))
                })
                .map_err(|e| crate::words::from_db(&e))?
                .filter(|item| item.is_available)
                .ok_or_else(|| {
                    UiError::new(
                        "menu.unknown",
                        "That item is not on the menu any more. Refresh and try again.",
                    )
                })
        })
    }
}
