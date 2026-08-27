//! The print queue.

pub mod sqlite;
pub mod store;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use mb_core::BusinessDay;
use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::drawer::DrawerConfig;
use crate::error::PrintError;
use crate::escpos::{self, JobOptions};
use crate::font::Typefaces;
use crate::layout::layout_for;
use crate::metrics::Metrics;
use crate::printer::{Engine, PrinterConfig};
use crate::raster::{RasterOptions, to_raster};
use crate::transport::{RealTransports, TransportError, TransportFactory};

pub use store::{JobStore, MemoryStore, StoreError, StoredJob};

/// What is being printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Bill,
    Kitchen,
    Label,
    Test,
    /// A drawer pulse with nothing to print — when the drawer hangs off a different printer
    /// from the one the bill went to.
    Drawer,
    DayClose,
    Delivery,
    /// The shop's recovery code, on paper.
    Recovery,
}

impl JobKind {
    /// Every kind, so a test can walk them.
    pub const ALL: &'static [JobKind] = &[
        JobKind::Bill,
        JobKind::Kitchen,
        JobKind::Label,
        JobKind::Test,
        JobKind::Drawer,
        JobKind::DayClose,
        JobKind::Delivery,
        JobKind::Recovery,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            JobKind::Bill => "bill",
            JobKind::Kitchen => "kitchen",
            JobKind::Label => "label",
            JobKind::Test => "test",
            JobKind::Drawer => "drawer",
            JobKind::DayClose => "day_close",
            JobKind::Delivery => "delivery",
            JobKind::Recovery => "recovery",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<JobKind> {
        match text {
            "bill" => Some(JobKind::Bill),
            "kitchen" => Some(JobKind::Kitchen),
            "label" => Some(JobKind::Label),
            "test" => Some(JobKind::Test),
            "drawer" => Some(JobKind::Drawer),
            "day_close" => Some(JobKind::DayClose),
            "delivery" => Some(JobKind::Delivery),
            "recovery" => Some(JobKind::Recovery),
            _ => None,
        }
    }

    /// Lower is sooner.
    #[must_use]
    pub const fn priority(self) -> i64 {
        match self {
            JobKind::Drawer => 5,
            JobKind::Bill => 10,
            JobKind::Kitchen => 20,
            // Behind a bill: a customer waiting to pay beats the slip that closes a day which
            // is already over.
            JobKind::DayClose => 30,
            // With the food, so it goes at kitchen speed and not at label speed: the rider is
            // standing there.
            JobKind::Delivery => 25,
            // Ahead of a bill, and it is the only thing that is.
            JobKind::Recovery => 6,
            JobKind::Test | JobKind::Label => 50,
        }
    }
}

/// What the queue is asked to print.
#[derive(Debug, Clone)]
pub struct Job {
    pub kind: JobKind,
    pub printer_id: String,
    pub document: Document,
    pub copies: u8,
    /// Why, in words the cashier's queue can show: "table 6", "reprint by Ravi", "test print".
    pub reason: Option<String>,
    /// Stamped by whoever created the job, never re-derived here.
    pub business_day: BusinessDay,
    /// Whether this job should also open the drawer.
    pub kick_drawer: bool,
    /// Which typeface to draw this with, as a key from `crate::font::FAMILIES`.
    pub font: Option<String>,
}

impl Job {
    #[must_use]
    pub fn new(
        kind: JobKind,
        printer_id: impl Into<String>,
        document: Document,
        business_day: BusinessDay,
    ) -> Job {
        Job {
            kind,
            printer_id: printer_id.into(),
            document,
            copies: 1,
            reason: None,
            business_day,
            kick_drawer: false,
            font: None,
        }
    }

    #[must_use]
    pub fn because(mut self, reason: impl Into<String>) -> Job {
        self.reason = Some(reason.into());
        self
    }

    /// The face this shop chose for this kind of document.
    #[must_use]
    pub fn in_face(mut self, font: Option<String>) -> Job {
        self.font = font;
        self
    }

    #[must_use]
    pub const fn opening_the_drawer(mut self, kick: bool) -> Job {
        self.kick_drawer = kick;
        self
    }
}

/// What a job carries across a power cut.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Payload {
    document: Document,
    kick_drawer: bool,
    /// Which typeface this job is drawn with — the shop chooses one for the bill and one for
    /// the kitchen ticket.
    #[serde(default)]
    font: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Pending,
    Printing,
    Failed,
    Parked,
    /// Never stored: a done job has no row.
    Done,
}

impl JobState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            JobState::Pending => "pending",
            JobState::Printing => "printing",
            JobState::Failed => "failed",
            JobState::Parked => "parked",
            JobState::Done => "done",
        }
    }
}

/// What the cashier's screen subscribes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueEvent {
    Queued {
        id: String,
        printer_id: String,
        kind: JobKind,
    },
    Started {
        id: String,
        attempt: i64,
    },
    Printed {
        id: String,
        engine: Engine,
    },
    Failed {
        id: String,
        attempt: i64,
        error: String,
        retry_in: Duration,
    },
    /// Nothing else will be tried.
    Parked {
        id: String,
        reason: String,
    },
    Retried {
        id: String,
    },
    Dismissed {
        id: String,
    },
}

/// One line of the queue, for a screen that attached late.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobStatus {
    pub id: String,
    pub printer_id: String,
    pub printer_name: String,
    pub kind: JobKind,
    pub state: JobState,
    pub attempts: i64,
    pub reason: Option<String>,
    pub last_error: Option<String>,
    pub engine_used: Option<Engine>,
}

#[derive(Debug, Clone, Copy)]
pub struct QueueConfig {
    /// Attempts before a job is parked.
    pub max_attempts: i64,
    /// The first wait; each retry doubles it.
    pub backoff: Duration,
    /// How long to wait for a socket.
    pub connect_timeout: Duration,
    /// How long one attempt may take before the job is parked and the printer's thread freed.
    pub job_deadline: Duration,
}

impl Default for QueueConfig {
    fn default() -> Self {
        QueueConfig {
            // Five attempts at 1, 2, 4 and 8 seconds is fifteen seconds of trying — long enough
            // for somebody to switch the printer on, short enough that a parked job is news
            // while the order is still hot.
            max_attempts: 5,
            backoff: Duration::from_secs(1),
            connect_timeout: Duration::from_secs(3),
            // A long raster bill on a slow serial port needs a minute; a hung printer must not
            // hold the bills behind it for days.
            job_deadline: Duration::from_secs(90),
        }
    }
}

/// The print queue.
#[derive(Debug)]
pub struct Queue {
    printers: Arc<BTreeMap<String, PrinterConfig>>,
    senders: BTreeMap<String, Sender<Message>>,
    workers: Vec<JoinHandle<()>>,
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    store: Arc<dyn JobStore>,
    subscribers: Mutex<Vec<Sender<QueueEvent>>>,
    statuses: Mutex<BTreeMap<String, JobStatus>>,
    printers: Arc<BTreeMap<String, PrinterConfig>>,
    faces: Arc<dyn Typefaces>,
    config: QueueConfig,
    transports: Arc<dyn TransportFactory>,
}

#[derive(Debug)]
enum Message {
    Job(Box<StoredJob>),
    Stop,
}

/// Job ids are unique within a process and across restarts.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl Queue {
    /// Start the workers and resume anything the last run left behind.
    pub fn start(
        printers: Vec<PrinterConfig>,
        store: Arc<dyn JobStore>,
        faces: Arc<dyn Typefaces>,
        config: QueueConfig,
    ) -> Queue {
        Queue::start_with_transports(printers, store, faces, config, Arc::new(RealTransports))
    }

    /// The same, with the transports supplied.
    pub fn start_with_transports(
        printers: Vec<PrinterConfig>,
        store: Arc<dyn JobStore>,
        faces: Arc<dyn Typefaces>,
        config: QueueConfig,
        transports: Arc<dyn TransportFactory>,
    ) -> Queue {
        let map: BTreeMap<String, PrinterConfig> =
            printers.into_iter().map(|p| (p.id.clone(), p)).collect();
        let printers = Arc::new(map);

        let shared = Arc::new(Shared {
            store,
            subscribers: Mutex::new(Vec::new()),
            statuses: Mutex::new(BTreeMap::new()),
            printers: Arc::clone(&printers),
            faces,
            transports,
            config,
        });

        let mut senders = BTreeMap::new();
        let mut workers = Vec::new();
        for id in printers.keys() {
            let (tx, rx) = channel::<Message>();
            senders.insert(id.clone(), tx);
            let worker_shared = Arc::clone(&shared);
            let printer_id = id.clone();
            // One thread per printer.
            let handle = std::thread::Builder::new()
                .name(format!("mb-print-{printer_id}"))
                .spawn(move || run_worker(&worker_shared, &printer_id, &rx));
            if let Ok(handle) = handle {
                workers.push(handle);
            }
        }

        let queue = Queue {
            printers,
            senders,
            workers,
            shared,
        };
        queue.resume();
        queue
    }

    /// Everything the billing thread does.
    pub fn enqueue(&self, job: Job) -> Result<String, PrintError> {
        let printer = self.printers.get(&job.printer_id).ok_or_else(|| {
            PrintError::invalid(format!("there is no printer {}", job.printer_id))
        })?;

        // A kitchen-only printer never receives a bill, and a bill-only printer never receives
        // a ticket.
        let allowed = match job.kind {
            JobKind::Bill => printer.role.accepts_bill(),
            JobKind::Kitchen => printer.role.accepts_kitchen(),
            // A closing slip goes wherever a bill would, and a shop with only a kitchen printer
            // must still be able to print one — the alternative is a day that cannot be closed
            // on paper.
            JobKind::DayClose
            | JobKind::Label
            | JobKind::Test
            | JobKind::Drawer
            | JobKind::Delivery
            | JobKind::Recovery => true,
        };
        if !allowed {
            return Err(PrintError::invalid(format!(
                "{} is a {:?} printer and cannot print a {}",
                printer.name,
                printer.role,
                job.kind.as_str()
            )));
        }

        let payload = serde_json::to_string(&Payload {
            document: job.document,
            kick_drawer: job.kick_drawer,
            font: job.font.clone(),
        })
        .map_err(|e| PrintError::invalid(format!("that document cannot be queued: {e}")))?;

        let created_at = now_millis();
        let id = format!(
            "job_{created_at}_{}",
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let stored = StoredJob {
            id: id.clone(),
            printer_id: job.printer_id.clone(),
            kind: job.kind.as_str().to_owned(),
            state: JobState::Pending.as_str().to_owned(),
            copies: i64::from(job.copies.max(1)),
            priority: job.kind.priority(),
            attempts: 0,
            payload,
            reason: job.reason.clone(),
            last_error: None,
            engine_used: None,
            business_day: i64::from(job.business_day.days_since_epoch()),
            created_at,
        };

        // Durable BEFORE the caller is let go.
        self.shared
            .store
            .save(&stored)
            .map_err(|e| PrintError::invalid(e.to_string()))?;

        self.shared.set_status(&stored, JobState::Pending);
        self.shared.publish(&QueueEvent::Queued {
            id: id.clone(),
            printer_id: job.printer_id.clone(),
            kind: job.kind,
        });
        self.dispatch(stored);
        Ok(id)
    }

    /// A receiver of everything that happens.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<QueueEvent> {
        let (tx, rx) = channel();
        lock(&self.shared.subscribers).push(tx);
        rx
    }

    /// Every unfinished job, for a screen that attached late — because a subscriber that missed
    /// the `Parked` event would otherwise be blind to the one thing it exists to show.
    #[must_use]
    pub fn snapshot(&self) -> Vec<JobStatus> {
        lock(&self.shared.statuses).values().cloned().collect()
    }

    /// Put a parked job back.
    pub fn retry(&self, id: &str) -> Result<(), PrintError> {
        let jobs = self
            .shared
            .store
            .unfinished()
            .map_err(|e| PrintError::invalid(e.to_string()))?;
        let Some(mut job) = jobs.into_iter().find(|j| j.id == id) else {
            return Err(PrintError::invalid(format!("there is no print job {id}")));
        };
        job.attempts = 0;
        job.state = JobState::Pending.as_str().to_owned();
        let _ = self
            .shared
            .store
            .update(id, job.state.as_str(), 0, None, None);
        self.shared.set_status(&job, JobState::Pending);
        self.shared
            .publish(&QueueEvent::Retried { id: id.to_owned() });
        self.dispatch(job);
        Ok(())
    }

    /// Throw a parked job away.
    pub fn dismiss(&self, id: &str) -> Result<(), PrintError> {
        self.shared
            .store
            .remove(id)
            .map_err(|e| PrintError::invalid(e.to_string()))?;
        lock(&self.shared.statuses).remove(id);
        self.shared
            .publish(&QueueEvent::Dismissed { id: id.to_owned() });
        Ok(())
    }

    /// Stop every worker and wait for the job in hand to finish.
    pub fn shutdown(self) {
        for sender in self.senders.values() {
            let _ = sender.send(Message::Stop);
        }
        for worker in self.workers {
            let _ = worker.join();
        }
    }

    /// Carry on where the last run stopped.
    fn resume(&self) {
        let Ok(jobs) = self.shared.store.unfinished() else {
            return;
        };
        for job in jobs {
            if !self.printers.contains_key(&job.printer_id) {
                self.shared.park(
                    &job,
                    &format!(
                        "the printer {} no longer exists, so this could not be printed",
                        job.printer_id
                    ),
                );
            } else if job.state == JobState::Printing.as_str() {
                // Nobody knows whether the paper came out. A person does.
                self.shared.park(
                    &job,
                    "Magic Bill closed while this was printing. Check the paper, then press \
                     Try again or Dismiss",
                );
            } else {
                self.shared.set_status(&job, JobState::Pending);
                self.dispatch(job);
            }
        }
    }

    fn dispatch(&self, job: StoredJob) {
        if let Some(sender) = self.senders.get(&job.printer_id) {
            let _ = sender.send(Message::Job(Box::new(job)));
        }
    }
}

impl Shared {
    fn publish(&self, event: &QueueEvent) {
        // A subscriber whose receiver has been dropped is a screen that closed; it is removed
        // rather than treated as a failure.
        lock(&self.subscribers).retain(|s| s.send(event.clone()).is_ok());
    }

    fn set_status(&self, job: &StoredJob, state: JobState) {
        let printer_name = self
            .printers
            .get(&job.printer_id)
            .map_or_else(|| job.printer_id.clone(), |p| p.name.clone());
        let status = JobStatus {
            id: job.id.clone(),
            printer_id: job.printer_id.clone(),
            printer_name,
            kind: JobKind::parse(&job.kind).unwrap_or(JobKind::Bill),
            state,
            attempts: job.attempts,
            reason: job.reason.clone(),
            last_error: job.last_error.clone(),
            engine_used: match job.engine_used.as_deref() {
                Some("raster") => Some(Engine::Raster),
                Some("text") => Some(Engine::Text),
                _ => None,
            },
        };
        lock(&self.statuses).insert(job.id.clone(), status);
    }

    /// Give up, visibly. Never silently.
    fn park(&self, job: &StoredJob, reason: &str) {
        let mut parked = job.clone();
        parked.state = JobState::Parked.as_str().to_owned();
        parked.last_error = Some(reason.to_owned());
        let _ = self.store.update(
            &job.id,
            JobState::Parked.as_str(),
            job.attempts,
            Some(reason),
            None,
        );
        self.set_status(&parked, JobState::Parked);
        self.publish(&QueueEvent::Parked {
            id: job.id.clone(),
            reason: reason.to_owned(),
        });
    }
}

/// One printer's worker.
fn run_worker(shared: &Arc<Shared>, printer_id: &str, rx: &Receiver<Message>) {
    let mut pending: BinaryHeap<Reverse<Queued>> = BinaryHeap::new();
    let mut arrival = 0_u64;

    loop {
        if pending.is_empty() {
            match rx.recv() {
                Ok(Message::Job(job)) => {
                    arrival += 1;
                    pending.push(Reverse(Queued::new(*job, arrival)));
                }
                Ok(Message::Stop) | Err(_) => return,
            }
        }
        // Take everything else that is already waiting, so the heap can choose.
        loop {
            match rx.try_recv() {
                Ok(Message::Job(job)) => {
                    arrival += 1;
                    pending.push(Reverse(Queued::new(*job, arrival)));
                }
                Ok(Message::Stop) => return,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        let Some(Reverse(next)) = pending.pop() else {
            continue;
        };
        run_job(shared, printer_id, next.job);
    }
}

/// A job in the worker's heap: by priority, then by arrival.
#[derive(Debug, PartialEq, Eq)]
struct Queued {
    priority: i64,
    arrival: u64,
    job: StoredJob,
}

impl Queued {
    fn new(job: StoredJob, arrival: u64) -> Queued {
        Queued {
            priority: job.priority,
            arrival,
            job,
        }
    }
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then(self.arrival.cmp(&other.arrival))
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn run_job(shared: &Arc<Shared>, printer_id: &str, mut job: StoredJob) {
    let Some(printer) = shared.printers.get(printer_id) else {
        shared.park(&job, "that printer is no longer configured");
        return;
    };

    // A payload that will not parse will not parse the fifth time either.
    let payload: Payload = match serde_json::from_str(&job.payload) {
        Ok(payload) => payload,
        Err(e) => {
            shared.park(&job, &format!("this job cannot be read back: {e}"));
            return;
        }
    };

    let mut attempt = 0_i64;
    loop {
        attempt += 1;
        job.attempts = attempt;
        let _ = shared
            .store
            .update(&job.id, JobState::Printing.as_str(), attempt, None, None);
        shared.set_status(&job, JobState::Printing);
        shared.publish(&QueueEvent::Started {
            id: job.id.clone(),
            attempt,
        });

        match print_within(shared, printer, &job, &payload) {
            Ok(engine) => {
                job.engine_used = Some(engine_name(engine).to_owned());
                // Printed means gone.
                let _ = shared.store.remove(&job.id);
                shared.set_status(&job, JobState::Done);
                shared.publish(&QueueEvent::Printed {
                    id: job.id.clone(),
                    engine,
                });
                return;
            }
            Err(failure) => {
                let permanent = failure.permanent;
                let message = failure.message;
                if permanent {
                    shared.park(&job, &message);
                    return;
                }
                if attempt >= shared.config.max_attempts {
                    shared.park(
                        &job,
                        &format!("gave up after {attempt} attempts: {message}"),
                    );
                    return;
                }

                // Doubling from the configured base: 1, 2, 4, 8 seconds.
                let shift = u32::try_from(attempt - 1).unwrap_or(0).min(16);
                let wait = shared.config.backoff.saturating_mul(1_u32 << shift);
                job.last_error = Some(message.clone());
                let _ = shared.store.update(
                    &job.id,
                    JobState::Failed.as_str(),
                    attempt,
                    Some(&message),
                    None,
                );
                shared.set_status(&job, JobState::Failed);
                shared.publish(&QueueEvent::Failed {
                    id: job.id.clone(),
                    attempt,
                    error: message,
                    retry_in: wait,
                });
                // Sleeping here holds up this printer only, which is right: its jobs must stay
                // in order, and every other printer has its own thread.
                std::thread::sleep(wait);
            }
        }
    }
}

/// `print_once` with the worker's thread back after `job_deadline`: a printer that hangs
/// mid-write parks its job instead of holding every job behind it.
fn print_within(
    shared: &Arc<Shared>,
    printer: &PrinterConfig,
    job: &StoredJob,
    payload: &Payload,
) -> Result<Engine, Failure> {
    let deadline = shared.config.job_deadline;
    let name = format!("mb-print-{}-job", printer.id);
    let (tx, rx) = channel::<Result<Engine, Failure>>();
    let (shared, printer, job, payload) = (
        Arc::clone(shared),
        printer.clone(),
        job.clone(),
        payload.clone(),
    );
    let spawned = std::thread::Builder::new().name(name).spawn(move || {
        // A receiver that gave up is a job already parked; the answer has nowhere to go.
        let _ = tx.send(print_once(&shared, &printer, &job, &payload));
    });
    if spawned.is_err() {
        return Err(Failure {
            message: "a thread for the printer could not be started".to_owned(),
            permanent: false,
        });
    }
    match rx.recv_timeout(deadline) {
        Ok(result) => result,
        Err(RecvTimeoutError::Disconnected) => Err(Failure {
            message: "the printer's thread stopped without answering".to_owned(),
            permanent: false,
        }),
        // Parked, not retried: the bytes may still be on their way, and a retry could print
        // the bill twice.
        Err(RecvTimeoutError::Timeout) => Err(Failure {
            message: format!(
                "the printer did not finish within {} seconds. Check it, then press Try again",
                deadline.as_secs()
            ),
            permanent: true,
        }),
    }
}

/// A failure, and whether trying again could ever help.
#[derive(Debug)]
struct Failure {
    message: String,
    permanent: bool,
}

fn print_once(
    shared: &Arc<Shared>,
    printer: &PrinterConfig,
    job: &StoredJob,
    payload: &Payload,
) -> Result<Engine, Failure> {
    // The printer decides its own paper and its own offset.
    let mut document = payload.document.clone();
    document.paper = printer.paper;

    // Laid out with the metrics of the engine that will draw it.
    let mut engine = printer.effective_engine();
    let metrics = match engine {
        Engine::Text => Metrics::printer_font(printer.paper),
        Engine::Raster => Metrics::face(printer.paper, shared.faces.face(payload.font.as_deref())),
    };
    let laid = layout_for(&document, &metrics).map_err(|e| Failure {
        message: e.to_string(),
        // An amount wider than the paper is a template bug, not a printer that is switched off.
        permanent: true,
    })?;

    let options = JobOptions {
        cut: !matches!(JobKind::parse(&job.kind), Some(JobKind::Drawer)),
        feed_lines: 3,
        drawer: payload.kick_drawer.then_some(printer.drawer),
        bold_dark: printer.bold_dark,
    };

    let bytes = match engine {
        Engine::Raster => {
            let raster = to_raster(
                &laid,
                &metrics,
                RasterOptions {
                    native_qr: printer.caps.native_qr,
                    ..RasterOptions::default()
                },
            );
            match raster {
                Ok(raster) => escpos::encode_raster(&raster, &printer.caps, &options),
                Err(_) => {
                    // The fallback, and it is a fallback rather than the mechanism: a printer
                    // set to Text is never "upgraded".
                    engine = Engine::Text;
                    escpos::encode_text(&laid, &printer.caps, &options)
                }
            }
        }
        Engine::Text => escpos::encode_text(&laid, &printer.caps, &options),
    };

    // Copies are one trip: the same stream repeated, with one connect.
    let copies = usize::try_from(job.copies.max(1)).unwrap_or(1);
    let mut stream = Vec::with_capacity(bytes.len() * copies);
    for _ in 0..copies {
        stream.extend_from_slice(&bytes);
    }

    let name = describe_job(job, printer);
    let mut transport = shared
        .transports
        .open(&printer.target, shared.config.connect_timeout)
        .map_err(to_failure)?;
    transport.send(&stream, &name).map_err(to_failure)?;
    Ok(engine)
}

fn to_failure(e: TransportError) -> Failure {
    Failure {
        permanent: e.is_permanent(),
        message: e.to_string(),
    }
}

/// The job name a shop sees in the Windows print queue window.
fn describe_job(job: &StoredJob, printer: &PrinterConfig) -> String {
    match &job.reason {
        Some(reason) => format!("Magic Bill — {} ({reason}) — {}", job.kind, printer.name),
        None => format!("Magic Bill — {} — {}", job.kind, printer.name),
    }
}

const fn engine_name(engine: Engine) -> &'static str {
    match engine {
        Engine::Raster => "raster",
        Engine::Text => "text",
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The drawer's own tiny job, for when the drawer hangs off a printer the bill did not go to.
#[must_use]
pub fn drawer_job(
    printer_id: impl Into<String>,
    paper: crate::paper::Paper,
    business_day: BusinessDay,
    _config: DrawerConfig,
) -> Job {
    // An empty document: the bytes that matter are `ESC p`, which `JobOptions` adds, and there
    // is nothing to print.
    let document = Document::new(paper);
    Job::new(JobKind::Drawer, printer_id, document, business_day)
        .because("open the cash drawer")
        .opening_the_drawer(true)
}
