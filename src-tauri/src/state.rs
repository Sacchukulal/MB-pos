//! What the process holds for its whole life, and the channel it pushes down.
//!
//! # Two things live here and nothing else does
//!
//! The **database** and the **print queue**. Both are expensive to build, both
//! must outlive any one screen, and both are the kind of thing that gets
//! rebuilt per-request by accident if there is nowhere obvious to put them.
//!
//! Audit **E1** is the reason the shape matters: *"everything runs on a single
//! thread inside one window — the billing keyboard, the charts, the report
//! queries and the cloud bridge all share it. A heavy report on a slow PC can
//! make the search box stutter mid-rush."* The answer is structural: the
//! database has one writer and four readers (P04's `conn.rs`), the print queue
//! has a thread per printer (P07), and the UI thread does neither.
//!
//! # Rust pushes, React subscribes — budget M4
//!
//! `PERFORMANCE.md` §5 rule 6: *"no polling. Rust pushes state; React
//! subscribes. A 250 ms poll loop is M4 gone before a single feature is
//! written."* [`Push`] is that channel, and it is the only way state reaches a
//! screen without being asked for.

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
///
/// One outlet until P27 builds the screens for more; the *column* has existed
/// since P04, because D22 says a dimension found late has to be back-filled
/// across every table at once with a value nobody can verify.
pub const OUTLET: &str = "outlet_default";

/// Everything the process holds.
#[derive(Debug)]
pub struct App {
    /// `None` on a first run, and that is a state rather than a failure: the
    /// window opens, and it offers to create a shop or restore a backup.
    shop: Mutex<Option<Shop>>,
    config: Mutex<AppConfig>,
    /// **The cart lives in Rust** (P09). One counter, one cart, recomputed from
    /// scratch on every change — see billing.rs for why it is not in React.
    cart: Mutex<CartState>,
    /// One face for every printer, loaded once (P07/D33). Rasterising a
    /// receipt is 2 ms with a warm glyph cache and rather more with a cold one,
    /// so there is exactly one.
    font: Arc<Font>,
    /// **Who is at the counter** (P11). Deliberately beside the cart rather
    /// than inside it: locking the screen must not be able to touch an order,
    /// and two separate locks is the cheapest way to make that structural
    /// instead of remembered.
    sessions: Sessions,
    /// **The shop's settings, loaded once** (P17).
    ///
    /// Every one of these used to be `ReceiptSettings::default()` written at
    /// the point of use, which meant a shop could change none of them. It is
    /// held here rather than read per print because a kitchen ticket has 50 ms
    /// (budget B6) and a database round trip inside it would be spent on a
    /// dozen rows that change once a month.
    ///
    /// Replaced **wholesale** on save, so nothing on the printing path can ever
    /// see half of a change.
    shop_config: Mutex<crate::settings::ShopConfig>,
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
        let font = Font::builtin().map_err(|e| words::from_print(&e))?;
        let sessions = Sessions::new();
        // **A shop that does not exist yet has nothing to lock.**
        //
        // Found by running it: after P11 a first run opened straight onto the
        // lock screen, with an empty staff list and no way past it — because
        // `open_or_lock` is only reached through `open_shop`, and a first run
        // never opens one. Nobody could create the shop that would hold the PIN
        // that would let them in.
        //
        // Starting as the stand-in is the same rule item 9 already states, one
        // step earlier: a shop with no PIN does not lock, and a shop with no
        // *database* certainly has none. `open_shop` re-decides the moment
        // there is something to decide about.
        sessions.begin(
            stand_in_actor("Counter", DEFAULT_STAFF),
            crate::flows::now(),
            true,
        );
        Ok(App {
            shop: Mutex::new(None),
            config: Mutex::new(config),
            cart: Mutex::new(CartState::default()),
            font: Arc::new(font),
            sessions,
            shop_config: Mutex::new(crate::settings::ShopConfig::default()),
        })
    }

    /// What this shop has chosen. A clone, because the caller holds it across a
    /// render and the lock must not be.
    #[must_use]
    pub fn shop_config(&self) -> crate::settings::ShopConfig {
        lock(&self.shop_config).clone()
    }

    /// Read the configuration from the open shop and publish it.
    ///
    /// **A shop whose settings will not read keeps the defaults and says so.**
    /// D7 forbids a silent default for a value that IS there and is the wrong
    /// type — `settings::load` returns an error for exactly that — but refusing
    /// to open the shop at all would mean one bad row stops a restaurant
    /// billing, and requirement 3 says it must not.
    pub fn reload_shop_config(&self) {
        let read = self.with_shop(|shop| {
            shop.db
                .transaction(|tx| crate::settings::load(&mb_db::Repos::new(tx), OUTLET))
                .map_err(|e| words::from_db(&e))
        });
        match read {
            Ok(config) => self.publish_shop_config(config),
            Err(e) if e.code == "shop.none" => {}
            Err(e) => log_warn!(
                "this shop's settings could not be read ({e}); the counter is \
                 using the standard ones until that is fixed"
            ),
        }
    }

    /// Put a configuration in place, including the one number that lives
    /// outside this struct (D70).
    pub fn publish_shop_config(&self, config: crate::settings::ShopConfig) {
        crate::flows::set_day_rule(config.day.rule());
        *lock(&self.shop_config) = config;
    }

    #[must_use]
    pub fn sessions(&self) -> &Sessions {
        &self.sessions
    }

    /// Take ownership of an open database and start everything that hangs off
    /// it. Called once by start-up, and again if the owner creates or adopts a
    /// shop while the app is running.
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
        // Shutting the old queue down after the new one is in place, so there
        // is never a window with no queue at all.
        if let Some(old) = previous {
            old.queue.shutdown();
        }
        // **After the shop is in place**, because reading the settings needs a
        // shop to read them from. Before this line the counter is on the
        // standard settings, which is the right state for the seconds a first
        // run spends with no database.
        self.reload_shop_config();
        log_info!("the shop at {} is open", path.display());
    }

    /// Build the print queue from the printers the shop has configured.
    ///
    /// **This is the other half of audit D4.** P07 built a queue that remembers
    /// a failed print; without something to construct it, feed it the printers
    /// and show what it knows, the shop is still in the position the finding
    /// describes — *"nothing remembers it"*.
    fn build_queue(&self, db: &Arc<Db>) -> Queue {
        let mut printers = db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .unwrap_or_else(|e| {
                log_warn!("the printer list could not be read ({e}); starting with none");
                Vec::new()
            })
            .iter()
            .map(printer_config_for)
            .collect::<Vec<_>>();

        // **A shop with no printer set up must still be able to bill** —
        // requirement 3 of the ten, and the state every shop is in on its first
        // day. See `fallback_row`: it is saved, not invented, so the queue's
        // threads and the spool's foreign key agree with the settings screen
        // about which printers exist.
        if printers.is_empty() {
            log_warn!("no printer is set up; jobs will be spooled and printed nowhere");
            let row = fallback_row();
            match db.transaction(|tx| {
                mb_db::Repos::new(tx)
                    .settings()
                    .save_printer(OUTLET, &row, crate::flows::now())
            }) {
                Ok(()) => printers.push(printer_config_for(&row)),
                // Billing still works; printing does not, and the reason is
                // written down where an owner's support call can find it.
                Err(e) => log_warn!("the stand-in printer could not be saved ({e})"),
            }
        }

        log_info!("starting the print queue with {} printer(s)", printers.len());

        let store: Arc<dyn JobStore> = Arc::new(SqliteStore::new(Arc::clone(db), OUTLET));
        Queue::start(
            printers,
            store,
            Arc::clone(&self.font),
            QueueConfig::default(),
        )
    }

    /// A queue with nowhere to store anything, for the moments there is no
    /// shop: a first run, and the minutes after a restore was requested.
    ///
    /// **This is why P07 made durability a port** (D32): a test print during
    /// setup, and any print while the database is being replaced, both happen
    /// with nothing open. A queue that required storage could not help the
    /// person whose storage is what went wrong.
    pub fn transient_queue(&self, printers: Vec<PrinterConfig>) -> Queue {
        Queue::start(
            printers,
            Arc::new(MemoryStore::new()),
            Arc::clone(&self.font),
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

    #[must_use]
    pub fn font(&self) -> Arc<Font> {
        Arc::clone(&self.font)
    }

    pub fn config(&self) -> AppConfig {
        lock(&self.config).clone()
    }

    /// Change the stored configuration and write it, atomically.
    pub fn update_config(&self, f: impl FnOnce(&mut AppConfig)) {
        let mut config = lock(&self.config);
        f(&mut config);
        if let Err(e) = config.save() {
            // A window size that could not be saved is a small thing. Saying
            // nothing about it is not.
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
///
/// P11 owns staff, roles and PINs, and will replace this with whoever is at the
/// counter. Until then every order is `created_by` this id — and `orders` has
/// `created_by TEXT NOT NULL REFERENCES staff (id)`, so the row has to be there
/// or nothing can be billed at all.
pub const DEFAULT_STAFF: &str = "staff_default";

/// Make sure the shop has somebody to bill as.
///
/// The migration seeds the outlet and the terminal an order points at; it does
/// **not** seed staff, because people are P11's and inventing a shop's staff
/// list is a product decision this is not entitled to make. One row, named for
/// what it is, is the smallest thing that keeps requirement 3 true — *a shop
/// must be able to bill on its first day.*
///
/// Found by settling a bill: "FOREIGN KEY constraint failed", from `created_by`
/// pointing at a person who did not exist. The bill's money had already been
/// taken on screen.
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
                // **No role, on purpose.** The stand-in's authority lives in
                // the in-memory session (`session::stand_in_actor`) and only
                // while no PIN exists anywhere. Giving this row the Owner role
                // would make `active_administrators` count a person who can
                // never log in, and the "last administrator" rule would then be
                // satisfied by nobody.
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
        // Not fatal here: the failure will be reported in the cashier's own
        // words at the moment they try to bill, which is where it belongs.
        Err(e) => log_warn!("the default counter user could not be added ({e})"),
    }
}

/// The four roles a shop starts with.
///
/// Seeded only when there are none: a shop that has renamed "Waiter" to
/// "Steward" and taken a permission off it must not find them back tomorrow.
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
    /// **Open unlocked, or open locked** — P11 item 9, and the first decision
    /// this app makes about a shop.
    ///
    /// If nobody has a PIN, there is nothing to unlock with, so the counter
    /// opens as the stand-in user and the shell nags. The moment one PIN
    /// exists, the app opens locked.
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
            // A shop whose staff list will not read is a shop that must not
            // silently open unlocked. Locked is the safe direction to be wrong
            // in, and the lock screen will show the same failure.
            Err(e) => {
                self.sessions.end();
                log_warn!("the staff list could not be read ({e}); locking the counter");
            }
        }
    }

    /// Does anybody in this shop have a PIN?
    ///
    /// `false` on a shop that will not answer, which is the same direction
    /// [`App::open_or_lock`] is wrong in: the caller uses this to decide
    /// whether to lock, and locking is safe.
    #[must_use]
    pub fn shop_has_a_pin(&self) -> bool {
        self.with_shop(|shop| Ok(anybody_has_a_pin(&shop.db).unwrap_or(false)))
            .unwrap_or(false)
    }

    /// **Setting the FIRST PIN locks the app immediately** — proving it works
    /// while that person is still standing there is worth four seconds.
    ///
    /// `had_a_pin` is what the shop looked like *before* the change, and it is
    /// the whole of the rule.
    ///
    /// Found by driving a shop's first day end to end: the first version
    /// re-evaluated on every PIN change, so an owner setting PINs for four
    /// staff was thrown out to the lock screen after each one — and after
    /// setting their OWN first, they could not set anybody else's until they
    /// had signed back in. That is not "prove it works", it is a shop that
    /// fights the person setting it up.
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
    ///
    /// **Best effort, and deliberately so, for this one caller shape.** An
    /// audit row that describes something which has already happened — a lock,
    /// a logout, a refusal — must not be able to fail the thing it describes.
    /// Where the row is evidence *of a change*, it goes in the same transaction
    /// as the change instead, and there is no version of that path which uses
    /// this function (see `flows::complete_bill` and `ipc::save_staff_member`).
    pub fn record(&self, entry: &mb_auth::AuditEntry) {
        let written = self.with_shop(|shop| {
            shop.db
                .transaction(|tx| mb_db::Repos::new(tx).audit().append(OUTLET, entry))
                .map_err(|e| words::from_db(&e))
        });
        if let Err(e) = written {
            log_warn!("\"{}\" could not be written to the history: {e}", entry.action);
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

/// The printer a shop has before it has a printer — **a real row.**
///
/// Requirement 3 of the ten: a shop must be able to bill on its first day, and
/// on that day nobody has set a printer up. `kind = 'none'` is the case the
/// schema already wrote down for this — *"it accepts jobs and prints nothing"* —
/// and P17 turns it into a real one by editing this row.
///
/// # It is a row, and that is the point
///
/// Two attempts at this got it wrong by inventing a `PrinterConfig` at job
/// time instead, and each failed differently and only when run:
///
/// 1. the queue runs a thread per printer it was **started** with and refuses a
///    job addressed to any other — *"there is no printer prn_none"*;
/// 2. the spool row has `printer_id REFERENCES printers (id)`, so even once the
///    thread existed the durable write was refused — *"FOREIGN KEY constraint
///    failed"*.
///
/// Both are the same mistake: a printer that some of the system believes in.
/// Saving the row means there is one answer to "what printers are there?", and
/// the queue, the spool and the settings screen all read it.
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

/// mb-db stores a printer as a row of strings; mb-print wants a typed
/// configuration. **This is the only place the two vocabularies meet**, and it
/// is deliberately here rather than in either crate: P07's D32 keeps mb-print's
/// knowledge of the database down to one module, and mb-db must not learn what
/// a `Target` is.
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
        // "none", and anything a newer build wrote that this one does not
        // know. A printer we cannot understand must not stop the shop billing
        // (requirement 3), so it accepts jobs and prints nothing.
        _ => Target::None,
    };

    let mut printer = PrinterConfig::new(&row.id, &row.name, target);
    printer.paper = mb_print::paper::Paper {
        kind: match row.paper_mm {
            58 => mb_print::paper::PaperKind::Mm58,
            100 => mb_print::paper::PaperKind::Mm100,
            _ => mb_print::paper::PaperKind::Mm80,
        },
        // Scope 7.11 — the correction the owner nudged in from the test print.
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

// ---------------------------------------------------------------------------
// What the screens are told, without asking.
// ---------------------------------------------------------------------------

/// Everything Rust pushes. One enum, because one channel is easier to reason
/// about than five, and because a screen that has just attached wants the lot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Pushed {
    /// The print queue changed. Carries the whole snapshot rather than a delta:
    /// it is a handful of rows, and a delta is a thing that can be missed.
    PrintQueue { jobs: Vec<PrintJobView> },
    /// The screen locked itself, or somebody signed in or out. Pushed rather
    /// than polled: the idle timer is Rust's (P11), so React learns about it
    /// the same way it learns about a print job.
    Session {
        /// `None` when the screen is locked.
        who: Option<String>,
        role: Option<String>,
        /// True while nobody has a PIN — the shell's undismissable banner.
        stand_in: bool,
    },
    /// A stub with the right shape, so P21 fills it in rather than reshaping
    /// the UI (R10 — there is no licence code in this session).
    Licence { state: String },
    /// Likewise, for P33.
    Sync { state: String },
}

/// A print job, as a screen shows it.
///
/// Deliberately not mb-print's `JobStatus`: that type is the queue's own
/// vocabulary, and the shell shows a sentence and a colour. Converting here
/// means P07's types can change without a screen changing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PrintJobView {
    pub id: String,
    pub printer: String,
    /// "Bill", "Kitchen ticket" — words, not tags (UI_GUIDELINES §6).
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
    /// A receiver of everything the print queue does, for [`crate::push`].
    ///
    /// `None` when there is no shop — a first run has no queue to listen to,
    /// and that is a state rather than a failure.
    pub fn subscribe_to_queue(&self) -> Option<std::sync::mpsc::Receiver<mb_print::queue::QueueEvent>> {
        lock(&self.shop).as_ref().map(|shop| shop.queue.subscribe())
    }

    /// Everything unfinished, in the words a screen shows.
    ///
    /// From `snapshot()` rather than from the event stream, which is P07's own
    /// reasoning: *"a screen that attached after the `Parked` event would
    /// otherwise be blind to the one thing it exists to show."*
    pub fn print_queue_snapshot(&self) -> Vec<PrintJobView> {
        lock(&self.shop).as_ref().map_or_else(Vec::new, |shop| {
            shop.queue.snapshot().iter().map(crate::ipc::to_view).collect()
        })
    }
}

impl App {
    /// Read the cart.
    pub fn with_cart<T>(&self, f: impl FnOnce(&CartState) -> UiResult<T>) -> UiResult<T> {
        f(&lock(&self.cart))
    }

    /// Change the cart, and get the new view back.
    ///
    /// One lock for the whole change **and** the recompute, so two commands
    /// arriving together can never interleave into a bill that reflects half of
    /// each. The cart is one counter's work in progress; there is no contention
    /// to optimise for.
    pub fn with_cart_mut<T>(&self, f: impl FnOnce(&mut CartState) -> UiResult<T>) -> UiResult<T> {
        f(&mut lock(&self.cart))
    }

    /// One menu row, by id.
    ///
    /// Read at the moment of adding, so the cart line is frozen from what the
    /// menu says *now* — crown jewel 4: *"frozen item snapshots on every order;
    /// old bills never change when you change a price."*
    pub fn find_menu_item(&self, id: &str) -> UiResult<mb_db::repo::menu::MenuItem> {
        self.with_shop(|shop| {
            let items = shop
                .db
                .transaction(|tx| mb_db::Repos::new(tx).menu().list_items(OUTLET, true))
                .map_err(|e| crate::words::from_db(&e))?;
            items
                .into_iter()
                .find(|item| item.id.as_str() == id)
                .ok_or_else(|| {
                    UiError::new(
                        "menu.unknown",
                        "That item is not on the menu any more. Refresh and try again.",
                    )
                })
        })
    }
}
