//! **T4–T8 and T14–T17 — the print queue, and the two findings it exists for.**
//!
//! > **D3.** *"If the tandoor printer is off, the app waits its full 15-second
//! > timeout before the kitchen printer gets anything. In a rush that is a lost
//! > order."*
//!
//! > **D4.** *"A failed print is only a red message on screen. Nothing
//! > remembers it."*
//!
//! Everything here runs with **no hardware**: a fake transport for the failures
//! and the blocking, a real SQLite file for the survival test.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests: expect is the assertion"
)]

mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use mb_core::BusinessDay;
use mb_db::{Db, DbConfig, Repos};
use mb_print::doc::{Align, Document, Style};
use mb_print::font::Font;
use mb_print::paper::{Paper, PaperKind};
use mb_print::printer::{PrinterConfig, Target};
use mb_print::queue::sqlite::SqliteStore;
use mb_print::queue::{
    Job, JobKind, JobState, MemoryStore, Queue, QueueConfig, QueueEvent, StoredJob,
};
use mb_print::transport::{Transport, TransportError, TransportFactory};
use mb_print::queue::JobStore;

// ---------------------------------------------------------------------------
// A printer that does whatever the test needs it to.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Recorder {
    sent: Mutex<Vec<Vec<u8>>>,
    attempts: AtomicU32,
    /// Fail this many attempts before succeeding. `u32::MAX` never succeeds.
    fail_first: AtomicU32,
    permanent: Mutex<bool>,
    /// When set, `send` blocks until released — a printer that is switched on
    /// but has stopped answering, which is the D3 case.
    gate: Option<Arc<Gate>>,
}

#[derive(Debug, Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn wait(&self) {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        while !*open {
            open = self.changed.wait(open).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn release(&self) {
        *self.open.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.changed.notify_all();
    }
}

#[derive(Debug)]
struct FakeTransports {
    by_printer: Vec<(String, Arc<Recorder>)>,
}

impl FakeTransports {
    fn new(entries: Vec<(&str, Arc<Recorder>)>) -> FakeTransports {
        FakeTransports {
            by_printer: entries
                .into_iter()
                .map(|(name, r)| (name.to_owned(), r))
                .collect(),
        }
    }
}

impl TransportFactory for FakeTransports {
    fn open(
        &self,
        target: &Target,
        _timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        // The tests key a fake off the target's file path, which is the one
        // field a `Target::File` carries.
        let key = match target {
            Target::File { path } => path.display().to_string(),
            other => format!("{other:?}"),
        };
        let found = self
            .by_printer
            .iter()
            .find(|(name, _)| key.contains(name.as_str()))
            .map(|(_, r)| Arc::clone(r));
        match found {
            Some(recorder) => Ok(Box::new(FakeTransport { recorder })),
            None => Err(TransportError::Refused {
                target: key,
                reason: "no fake printer registered".to_owned(),
            }),
        }
    }
}

#[derive(Debug)]
struct FakeTransport {
    recorder: Arc<Recorder>,
}

impl Transport for FakeTransport {
    fn send(&mut self, bytes: &[u8], _document: &str) -> Result<(), TransportError> {
        if let Some(gate) = &self.recorder.gate {
            gate.wait();
        }
        let attempt = self.recorder.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        let fail_first = self.recorder.fail_first.load(Ordering::SeqCst);
        if attempt <= fail_first {
            let permanent = *self
                .recorder
                .permanent
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            return Err(if permanent {
                TransportError::Refused {
                    target: "fake".to_owned(),
                    reason: "there is no such printer".to_owned(),
                }
            } else {
                TransportError::Connect {
                    target: "fake".to_owned(),
                    reason: "the printer is switched off".to_owned(),
                }
            });
        }
        self.recorder
            .sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(bytes.to_vec());
        Ok(())
    }

    fn describe(&self) -> String {
        "fake printer".to_owned()
    }
}

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

fn font() -> Arc<Font> {
    Arc::new(Font::builtin().expect("the shipped face loads"))
}

fn quick() -> QueueConfig {
    QueueConfig {
        max_attempts: 5,
        // Milliseconds instead of seconds. The *shape* of the backoff is what
        // matters — doubling, bounded — and a test that took fifteen seconds to
        // prove it would be a test people skip.
        backoff: Duration::from_millis(2),
        connect_timeout: Duration::from_millis(50),
    }
}

fn printer(id: &str) -> PrinterConfig {
    PrinterConfig::new(id, id, Target::File {
        path: std::path::PathBuf::from(format!("./{id}.bin")),
    })
}

fn ticket(printer_id: &str) -> Job {
    let mut doc = Document::new(Paper::new(PaperKind::Mm80));
    doc.text("KITCHEN", Style::new(2, true), Align::Centre);
    doc.line("2 Masala Dosa");
    Job::new(
        JobKind::Kitchen,
        printer_id,
        doc,
        BusinessDay::from_ymd(2026, 8, 3),
    )
    .because("table 6")
}

/// Wait for a condition, polling briefly. Threads are involved; a bare assert
/// straight after `enqueue` would be a race, and a `sleep(500)` would be a slow
/// test that still races.
fn until(mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

// ---------------------------------------------------------------------------

/// T4. **PARALLELISM — audit D3, proved.**
///
/// Two printers, one of which never answers. The healthy one must finish while
/// the other is still stuck, not after it.
#[test]
fn t4_a_dead_printer_does_not_delay_a_healthy_one() {
    let gate = Arc::new(Gate::default());
    let dead = Arc::new(Recorder {
        gate: Some(Arc::clone(&gate)),
        ..Recorder::default()
    });
    let healthy = Arc::new(Recorder::default());

    let transports = Arc::new(FakeTransports::new(vec![
        ("tandoor", Arc::clone(&dead)),
        ("kitchen", Arc::clone(&healthy)),
    ]));
    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![printer("tandoor"), printer("kitchen")],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        transports,
    );

    // The tandoor first, so that a queue which printed in order would have to
    // wait for it.
    queue.enqueue(ticket("tandoor")).expect("queued");
    queue.enqueue(ticket("kitchen")).expect("queued");

    let started = Instant::now();
    assert!(
        until(|| healthy.sent.lock().unwrap().len() == 1),
        "the kitchen printer never printed while the tandoor was stuck — \
         this is audit D3 and it is the reason for the session"
    );
    let elapsed = started.elapsed();
    assert!(
        dead.sent.lock().unwrap().is_empty(),
        "the stuck printer somehow finished"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "the healthy printer waited {elapsed:?} behind a dead one"
    );

    gate.release();
    queue.shutdown();
}

/// T5. Retry with backoff, bounded, then parked — with an exact count.
#[test]
fn t5_a_failing_job_is_retried_five_times_and_then_parked() {
    let printer_fake = Arc::new(Recorder::default());
    printer_fake.fail_first.store(u32::MAX, Ordering::SeqCst);

    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![printer("kitchen")],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![(
            "kitchen",
            Arc::clone(&printer_fake),
        )])),
    );

    let events = queue.subscribe();
    let id = queue.enqueue(ticket("kitchen")).expect("queued");

    let mut parked = None;
    while let Ok(event) = events.recv_timeout(Duration::from_secs(5)) {
        if let QueueEvent::Parked { id: parked_id, .. } = event {
            parked = Some(parked_id);
            break;
        }
    }
    assert_eq!(parked.as_deref(), Some(id.as_str()), "the job was never parked");
    assert_eq!(
        printer_fake.attempts.load(Ordering::SeqCst),
        5,
        "exactly five attempts — v1 had one path that retried for ever"
    );

    // And no sixth, ever.
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(printer_fake.attempts.load(Ordering::SeqCst), 5);

    // A parked job stays in the store. Nothing is ever silently dropped.
    let left = store.unfinished().expect("reads");
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].state, JobState::Parked.as_str());
    queue.shutdown();
}

#[test]
fn t5_a_job_that_fails_twice_and_then_works_is_not_parked() {
    let flaky = Arc::new(Recorder::default());
    flaky.fail_first.store(2, Ordering::SeqCst);

    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![printer("kitchen")],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("kitchen", Arc::clone(&flaky))])),
    );

    queue.enqueue(ticket("kitchen")).expect("queued");
    assert!(until(|| flaky.sent.lock().unwrap().len() == 1));
    assert_eq!(flaky.attempts.load(Ordering::SeqCst), 3);
    assert!(until(|| store.is_empty()), "a printed job left a row behind");
    queue.shutdown();
}

/// A failure that retrying cannot fix is parked at once. Five attempts at a
/// printer that does not exist is fifteen seconds of pretending.
#[test]
fn t5_a_permanent_failure_is_parked_without_retrying() {
    let gone = Arc::new(Recorder::default());
    gone.fail_first.store(u32::MAX, Ordering::SeqCst);
    *gone.permanent.lock().unwrap() = true;

    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![printer("kitchen")],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("kitchen", Arc::clone(&gone))])),
    );

    queue.enqueue(ticket("kitchen")).expect("queued");
    assert!(until(|| {
        store
            .unfinished()
            .expect("reads")
            .first()
            .is_some_and(|j| j.state == JobState::Parked.as_str())
    }));
    assert_eq!(gone.attempts.load(Ordering::SeqCst), 1);
    queue.shutdown();
}

/// T6. **THE QUEUE SURVIVES A RESTART** — a power cut, in one test, against a
/// real database file.
#[test]
fn t6_a_queued_ticket_survives_the_process_that_queued_it() {
    let scratch = common::Scratch::new("queue-restart");
    let db = Arc::new(Db::open(&DbConfig::new(scratch.path("shop.db"))).expect("opens"));
    common::seed_printer(&db, "kitchen");

    let store = Arc::new(SqliteStore::new(Arc::clone(&db), common::OUTLET));

    // A printer that is switched off for the whole of the first run.
    let off = Arc::new(Recorder::default());
    off.fail_first.store(u32::MAX, Ordering::SeqCst);
    let first = Queue::start_with_transports(
        vec![printer("kitchen")],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("kitchen", Arc::clone(&off))])),
    );
    let id = first.enqueue(ticket("kitchen")).expect("queued");

    // The row is durable BEFORE the caller was let go, so it is there now.
    let stored = db
        .transaction(|tx| Repos::new(tx).print_jobs().unfinished(common::OUTLET))
        .expect("reads");
    assert_eq!(stored.len(), 1, "the ticket was not written durably");
    assert_eq!(stored[0].id, id);

    first.shutdown();

    // The counter comes back up. The printer is on this time.
    let on = Arc::new(Recorder::default());
    let second = Queue::start_with_transports(
        vec![printer("kitchen")],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("kitchen", Arc::clone(&on))])),
    );

    assert!(
        until(|| on.sent.lock().unwrap().len() == 1),
        "the kitchen ticket did not survive the restart — this is D4 and a \
         power cut in the middle of a rush"
    );
    second.shutdown();

    // T16: and it left nothing behind (D35).
    let left = db
        .transaction(|tx| Repos::new(tx).print_jobs().count(common::OUTLET))
        .expect("counts");
    assert_eq!(left, 0, "the spool is not a log");
}

/// T7. A printer that cannot raster prints as text, and the job says so.
#[test]
fn t7_raster_falls_back_to_text_and_records_which_path_was_used() {
    let fake = Arc::new(Recorder::default());
    let mut config = printer("kitchen");
    config.caps.raster = false;

    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![config],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("kitchen", Arc::clone(&fake))])),
    );

    let events = queue.subscribe();
    queue.enqueue(ticket("kitchen")).expect("queued");

    let mut engine = None;
    while let Ok(event) = events.recv_timeout(Duration::from_secs(5)) {
        if let QueueEvent::Printed { engine: used, .. } = event {
            engine = Some(used);
            break;
        }
    }
    assert_eq!(engine, Some(mb_print::printer::Engine::Text));

    // And the bytes really are the text path: no GS v 0 anywhere.
    let sent = fake.sent.lock().unwrap();
    let bytes = sent.first().expect("something was sent");
    assert!(
        !bytes.windows(3).any(|w| w == [0x1D, b'v', b'0']),
        "a printer with no raster support was sent a raster image"
    );
    assert!(
        bytes.windows(7).any(|w| w == b"KITCHEN"),
        "the text path did not send the text"
    );
    drop(sent);
    queue.shutdown();
}

/// T8. A parked job is visible, can be retried, and can be dismissed — and
/// nothing else removes one.
#[test]
fn t8_a_parked_job_is_visible_and_a_person_decides_what_happens_to_it() {
    let fake = Arc::new(Recorder::default());
    fake.fail_first.store(u32::MAX, Ordering::SeqCst);

    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![printer("kitchen")],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("kitchen", Arc::clone(&fake))])),
    );

    let id = queue.enqueue(ticket("kitchen")).expect("queued");
    assert!(until(|| queue
        .snapshot()
        .iter()
        .any(|s| s.state == JobState::Parked)));

    // The cashier can see it, with its reason and which printer it was for.
    let snapshot = queue.snapshot();
    let job = snapshot.iter().find(|s| s.id == id).expect("in the snapshot");
    assert_eq!(job.printer_name, "kitchen");
    assert_eq!(job.reason.as_deref(), Some("table 6"));
    assert!(job.last_error.is_some(), "a parked job with no reason is a shrug");

    // Retry: the printer is on now.
    fake.fail_first.store(0, Ordering::SeqCst);
    queue.retry(&id).expect("retries");
    assert!(until(|| fake.sent.lock().unwrap().len() == 1));

    // Dismiss removes a job, and it is the only other thing that does.
    //
    // **The job is parked first, deliberately.** The first version enqueued
    // one onto a printer that was now working and dismissed it in the very
    // next statement — a race with the worker thread that had already picked
    // it up, which passed on its own and failed once in a full `--workspace`
    // run under load. Parking it also matches the only dismissal a cashier can
    // actually perform: the button lives on the parked row.
    fake.fail_first.store(u32::MAX, Ordering::SeqCst);
    let second = queue.enqueue(ticket("kitchen")).expect("queued");
    assert!(until(|| queue
        .snapshot()
        .iter()
        .any(|s| s.id == second && s.state == JobState::Parked)));
    queue.dismiss(&second).expect("dismisses");
    assert!(!queue.snapshot().iter().any(|s| s.id == second));
    queue.shutdown();
}

/// T14. A payload that cannot be understood is parked, not retried.
#[test]
fn t14_a_job_that_cannot_be_read_back_is_parked_at_once() {
    let fake = Arc::new(Recorder::default());
    let store = Arc::new(MemoryStore::new());

    // A row as a corrupted disk would hand it back.
    store
        .save(&StoredJob {
            id: "job_broken".to_owned(),
            printer_id: "kitchen".to_owned(),
            kind: "kitchen".to_owned(),
            state: "pending".to_owned(),
            copies: 1,
            priority: 20,
            attempts: 0,
            payload: "{ this is not a document".to_owned(),
            reason: Some("table 6".to_owned()),
            last_error: None,
            engine_used: None,
            business_day: 20_669,
            created_at: 1_770_000_000_000,
        })
        .expect("saves");

    let queue = Queue::start_with_transports(
        vec![printer("kitchen")],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("kitchen", Arc::clone(&fake))])),
    );

    assert!(until(|| queue
        .snapshot()
        .iter()
        .any(|s| s.state == JobState::Parked)));
    assert_eq!(
        fake.attempts.load(Ordering::SeqCst),
        0,
        "a corrupt row was sent to a printer"
    );
    queue.shutdown();
}

/// T15. Enqueue does not wait for a printer — with every worker stuck.
#[test]
fn t15_enqueue_returns_while_every_printer_is_stuck() {
    let gate = Arc::new(Gate::default());
    let stuck = Arc::new(Recorder {
        gate: Some(Arc::clone(&gate)),
        ..Recorder::default()
    });

    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![printer("kitchen")],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("kitchen", Arc::clone(&stuck))])),
    );

    // Fill the worker, then time the next one.
    queue.enqueue(ticket("kitchen")).expect("queued");
    std::thread::sleep(Duration::from_millis(20));

    let started = Instant::now();
    queue.enqueue(ticket("kitchen")).expect("queued");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(150),
        "enqueue took {elapsed:?} with a stuck printer — budget B6 is 50 ms, \
         ceiling 150, and requirement 3 says billing never stops"
    );

    gate.release();
    queue.shutdown();
}

/// T16. A bill overtakes a queue full of kitchen tickets.
#[test]
fn t16_a_bill_does_not_wait_behind_forty_kitchen_tickets() {
    let gate = Arc::new(Gate::default());
    let fake = Arc::new(Recorder {
        gate: Some(Arc::clone(&gate)),
        ..Recorder::default()
    });

    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![printer("counter")],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("counter", Arc::clone(&fake))])),
    );

    let events = queue.subscribe();
    for _ in 0..40 {
        queue.enqueue(ticket("counter")).expect("queued");
    }
    let mut bill_doc = Document::new(Paper::new(PaperKind::Mm80));
    bill_doc.line("TOTAL 646.00");
    let bill = queue
        .enqueue(Job::new(
            JobKind::Bill,
            "counter",
            bill_doc,
            BusinessDay::from_ymd(2026, 8, 3),
        ))
        .expect("queued");

    gate.release();

    // The first job was already in hand when the bill arrived, so the bill is
    // second at worst — the claim is that it does not wait for the other 39.
    let mut printed = Vec::new();
    while printed.len() < 3 {
        match events.recv_timeout(Duration::from_secs(5)) {
            Ok(QueueEvent::Printed { id, .. }) => printed.push(id),
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
        }
    }
    assert!(
        printed.contains(&bill),
        "a customer waited behind forty kitchen tickets"
    );
    queue.shutdown();
}

/// T17. Capabilities are obeyed: no blade, no cut command.
#[test]
fn t17_a_printer_that_cannot_cut_is_never_sent_a_cut() {
    let with_blade = Arc::new(Recorder::default());
    let without = Arc::new(Recorder::default());

    let mut cutter = printer("cutter");
    cutter.caps.cut = true;
    let mut blunt = printer("blunt");
    blunt.caps.cut = false;

    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![cutter, blunt],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![
            ("cutter", Arc::clone(&with_blade)),
            ("blunt", Arc::clone(&without)),
        ])),
    );

    queue.enqueue(ticket("cutter")).expect("queued");
    queue.enqueue(ticket("blunt")).expect("queued");
    assert!(until(|| with_blade.sent.lock().unwrap().len() == 1
        && without.sent.lock().unwrap().len() == 1));

    let cut = [0x1D, b'V'];
    assert!(
        with_blade.sent.lock().unwrap()[0]
            .windows(2)
            .any(|w| w == cut),
        "a printer with a blade was never told to cut"
    );
    assert!(
        !without.sent.lock().unwrap()[0]
            .windows(2)
            .any(|w| w == cut),
        "a printer with no blade was sent a cut"
    );
    queue.shutdown();
}

/// The drawer pulse rides on the bill when the bill printer is the drawer's.
#[test]
fn t9_the_drawer_pulse_is_on_the_bill_that_asked_for_it() {
    let fake = Arc::new(Recorder::default());
    let mut config = printer("counter");
    config.drawer.enabled = true;
    config.caps.drawer = true;

    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![config],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("counter", Arc::clone(&fake))])),
    );

    let mut doc = Document::new(Paper::new(PaperKind::Mm80));
    doc.line("TOTAL 646.00");
    queue
        .enqueue(
            Job::new(
                JobKind::Bill,
                "counter",
                doc.clone(),
                BusinessDay::from_ymd(2026, 8, 3),
            )
            .opening_the_drawer(true),
        )
        .expect("queued");
    queue
        .enqueue(Job::new(
            JobKind::Bill,
            "counter",
            doc,
            BusinessDay::from_ymd(2026, 8, 3),
        ))
        .expect("queued");

    assert!(until(|| fake.sent.lock().unwrap().len() == 2));
    let sent = fake.sent.lock().unwrap();
    let pulse = [0x1B, b'p'];
    assert!(
        sent[0].windows(2).any(|w| w == pulse),
        "the cash bill did not open the drawer"
    );
    assert!(
        !sent[1].windows(2).any(|w| w == pulse),
        "a bill that did not ask for the drawer opened it anyway"
    );
    drop(sent);
    queue.shutdown();
}

/// A kitchen-only printer refuses a bill, at the last place it can be caught.
#[test]
fn a_kitchen_printer_refuses_a_bill() {
    use mb_print::printer::Role;

    let fake = Arc::new(Recorder::default());
    let config = printer("kitchen").with_role(Role::Kitchen);

    let store = Arc::new(MemoryStore::new());
    let queue = Queue::start_with_transports(
        vec![config],
        Arc::clone(&store) as Arc<dyn mb_print::queue::JobStore>,
        font(),
        quick(),
        Arc::new(FakeTransports::new(vec![("kitchen", Arc::clone(&fake))])),
    );

    let mut doc = Document::new(Paper::new(PaperKind::Mm80));
    doc.line("TOTAL 646.00");
    let refused = queue.enqueue(Job::new(
        JobKind::Bill,
        "kitchen",
        doc,
        BusinessDay::from_ymd(2026, 8, 3),
    ));
    assert!(refused.is_err(), "a customer's bill went to the tandoor");
    queue.shutdown();
}

/// **Every kind of job this queue can make is a kind the database accepts** —
/// P30, and it is here because P30 found two that were not.
///
/// `print_jobs.kind` carries a CHECK listing the kinds, exactly as
/// `permissions` carries the vocabulary of "no": a typo becomes a constraint
/// violation instead of a silent unknown row. The cost of that is two lists,
/// and the two lists drifted — `day_close` arrived at P18 and `delivery` at
/// P29, and neither was ever added to the schema. **Printing a Z-report was
/// refused by the database**, on the one path where a shop most needs paper,
/// and nothing noticed for twelve sessions.
///
/// So it is a test now (D40). It walks `JobKind::ALL` rather than a list
/// written here, so adding a kind and forgetting the schema fails at once.
#[test]
fn every_job_kind_the_queue_can_make_is_allowed_by_the_schema() {
    let scratch = common::Scratch::new("queue-kinds");
    let db = Db::open(&DbConfig::new(scratch.path("shop.db"))).expect("opens");
    common::seed_printer(&db, "kitchen");

    for kind in mb_print::queue::JobKind::ALL {
        let tag = kind.as_str();
        let wrote = db.transaction(|tx| {
            Repos::new(tx).print_jobs().save(
                common::OUTLET,
                &mb_db::repo::PrintJobRow {
                    id: format!("job_{tag}"),
                    printer_id: "kitchen".to_owned(),
                    kind: tag.to_owned(),
                    state: "pending".to_owned(),
                    copies: 1,
                    priority: kind.priority(),
                    attempts: 0,
                    payload: "{}".to_owned(),
                    reason: None,
                    last_error: None,
                    engine_used: None,
                    business_day: BusinessDay::from_days_since_epoch(20_000),
                    created_at: mb_core::Timestamp::from_millis(0),
                },
                mb_core::Timestamp::from_millis(0),
            )
        });
        assert!(
            wrote.is_ok(),
            "the queue can produce a {tag:?} job and the database refuses it: {wrote:?}"
        );
    }
}
