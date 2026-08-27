//! Every cloud call has a deadline, and the deadline is the caller's.

use std::sync::mpsc;
use std::time::Duration;

/// The work did not finish in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("that took too long, so we stopped waiting")]
pub struct Timedout;

/// How long anything licensing-related may take before the counter gives up.
pub const DEADLINE: Duration = Duration::from_secs(8);

/// The startup check gets a shorter one.
pub const STARTUP_DEADLINE: Duration = Duration::from_secs(1);

/// Run `work`, and stop waiting after `limit`.
pub fn within<T, F>(limit: Duration, work: F) -> Result<T, Timedout>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    // The result is sent on a channel the caller owns, so a worker that finishes after the
    // deadline sends into a dropped receiver and is ignored rather than blocking on nobody.
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

    /// A late answer must not panic the worker or the caller.
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
        assert!(
            DEADLINE <= Duration::from_secs(10),
            "a shopkeeper is standing there"
        );
    }
}
