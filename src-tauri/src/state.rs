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

use crate::config::AppConfig;
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
    /// One face for every printer, loaded once (P07/D33). Rasterising a
    /// receipt is 2 ms with a warm glyph cache and rather more with a cold one,
    /// so there is exactly one.
    font: Arc<Font>,
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
        Ok(App {
            shop: Mutex::new(None),
            config: Mutex::new(config),
            font: Arc::new(font),
        })
    }

    /// Take ownership of an open database and start everything that hangs off
    /// it. Called once by start-up, and again if the owner creates or adopts a
    /// shop while the app is running.
    pub fn open_shop(&self, db: Db, path: std::path::PathBuf) {
        let db = Arc::new(db);
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
        log_info!("the shop at {} is open", path.display());
    }

    /// Build the print queue from the printers the shop has configured.
    ///
    /// **This is the other half of audit D4.** P07 built a queue that remembers
    /// a failed print; without something to construct it, feed it the printers
    /// and show what it knows, the shop is still in the position the finding
    /// describes — *"nothing remembers it"*.
    fn build_queue(&self, db: &Arc<Db>) -> Queue {
        let printers = db
            .transaction(|tx| mb_db::Repos::new(tx).settings().list_printers(OUTLET))
            .unwrap_or_else(|e| {
                log_warn!("the printer list could not be read ({e}); starting with none");
                Vec::new()
            })
            .iter()
            .map(printer_config_for)
            .collect::<Vec<_>>();

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
