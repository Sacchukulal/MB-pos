//! **D92 — every cloud call has a deadline, and the deadline is the caller's.**
//!
//! > *"Nothing here may hang: v1 deadlocked its whole cloud path on a socket a
//! > PC suspend had killed."*
//!
//! # Why the deadline cannot belong to the callee
//!
//! The obvious design is a `timeout` argument on each [`crate::Cloud`] method,
//! honoured by the implementation. It does not work, and v1 is the proof: its
//! HTTP client *had* a timeout. The socket that hung was one a laptop suspend
//! had killed underneath the client, in a state where the library was still
//! waiting on a read that would never return and never time out — the timeout
//! was armed on an operation that had already been overtaken.
//!
//! A deadline that the thing being waited on is responsible for enforcing is
//! not a deadline. It is a request.
//!
//! So [`within`] runs the work **on a worker thread** and waits on a channel
//! with `recv_timeout`. When the limit passes, the caller stops waiting and
//! moves on. The worker may be left behind holding a dead socket; that is a
//! leaked thread and it is the correct trade, because the alternative is a
//! counter that does not repaint while a shopkeeper stands at it.
//!
//! # What this costs, stated plainly
//!
//! One thread per call that goes past its deadline, until the OS tears the
//! socket down. Licensing calls happen at startup, on a timer measured in
//! hours, and when somebody presses a button — not per bill, not per keystroke.
//! Ten leaked threads over a day of a broken network is a few hundred KB
//! against budget M2's 350 MB. If a future feature ever calls this per bill,
//! that arithmetic changes and this paragraph is where to start.

use std::sync::mpsc;
use std::time::Duration;

/// The work did not finish in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("that took too long, so we stopped waiting")]
pub struct Timedout;

/// **How long anything licensing-related may take before the counter gives
/// up.**
///
/// Eight seconds is chosen from the other end: it is the longest a shopkeeper
/// who has just pressed Activate will wait without deciding the program has
/// crashed. It is not chosen from what a network typically needs, because if
/// the network needs longer than that the honest answer is "we could not reach
/// our server, we will keep trying" — which is a sentence, and a sentence is
/// better than a frozen window.
pub const DEADLINE: Duration = Duration::from_secs(8);

/// The startup check gets a shorter one. Budget S1 is 3.0 s to a usable billing
/// screen, and a licence refresh **must never be on that path** — it runs after
/// the window is up. This is the belt to that braces: even if a future session
/// moves the call, it cannot cost more than a second.
pub const STARTUP_DEADLINE: Duration = Duration::from_secs(1);

/// Run `work`, and stop waiting after `limit`.
///
/// # Errors
///
/// [`Timedout`] when the limit passes first. The work may still be running.
pub fn within<T, F>(limit: Duration, work: F) -> Result<T, Timedout>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    // The result is sent on a channel the caller owns, so a worker that
    // finishes *after* the deadline sends into a dropped receiver and is
    // ignored rather than blocking on nobody.
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(limit).map_err(|_| Timedout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn work_that_finishes_comes_back() {
        let answer = within(Duration::from_secs(5), || 2 + 2).expect("finished");
        assert_eq!(answer, 4);
    }

    /// **T9. The test v1 could not have passed.**
    ///
    /// A stub that never responds — the socket a suspend killed — and the
    /// caller comes back anyway, with a refusal rather than a frozen counter.
    #[test]
    fn a_call_that_never_answers_rejects_rather_than_hangs() {
        let started = std::time::Instant::now();
        let result = within(Duration::from_millis(120), || {
            // Never returns. This is the whole point.
            std::thread::sleep(Duration::from_secs(60));
            "an answer that arrives after the shop has closed"
        });
        assert_eq!(result, Err(Timedout));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the caller waited for the work instead of for the deadline"
        );
    }

    /// A late answer must not panic the worker or the caller. The receiver is
    /// gone by then, and `send` on a dropped channel is an error we ignore.
    #[test]
    fn a_late_answer_is_dropped_quietly() {
        let landed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&landed);
        let result = within(Duration::from_millis(50), move || {
            std::thread::sleep(Duration::from_millis(250));
            flag.store(true, Ordering::SeqCst);
            7
        });
        assert_eq!(result, Err(Timedout));
        std::thread::sleep(Duration::from_millis(400));
        // The worker finished, into nothing, and nothing fell over.
        assert!(landed.load(Ordering::SeqCst));
    }

    /// The two constants are ordered the way the reasoning says they are.
    #[test]
    fn the_startup_deadline_is_the_tighter_one() {
        assert!(STARTUP_DEADLINE < DEADLINE);
        assert!(DEADLINE <= Duration::from_secs(10), "a shopkeeper is standing there");
    }
}
