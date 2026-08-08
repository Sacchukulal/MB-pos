//! Budget **B10** — a PIN submitted and the cashier back to billing.
//!
//! `docs/PERFORMANCE.md` §2.1: *B10 — PIN submitted → billing usable again:
//! **400 ms**, ceiling **800 ms**. Prompt: P11.*
//!
//! # The number this measures is a decision, not an accident
//!
//! A six-digit PIN is a million guesses. Against anybody holding a copy of the
//! `.db` file, the Argon2 cost parameter **is** the security — so making this
//! number smaller makes the product weaker, and making it larger makes a shift
//! change slower on an i3 with 4 GB. `pin.rs` picks OWASP's Argon2id minimum
//! (19 MiB, t = 2, p = 1) and this is where that choice is held to account.
//!
//! §3.1's rules, obeyed: assertions in release only; every run prints the
//! number; the assert is against the CEILING, not the budget; `std::time` only,
//! no criterion.
//!
//! ```text
//! cargo test -p mb-auth --release --test perf -- --nocapture
//! ```

#![allow(
    clippy::expect_used,
    clippy::integer_division,
    reason = "a stopwatch, not the money path"
)]

use std::time::{Duration, Instant};

use mb_auth::{Pin, hash_pin, verify_pin};

/// The B10 ceiling. The budget is 400 ms; §3.1 rule 3 says the assert is
/// against the ceiling, so a busy laptop does not turn into a red build.
const CEILING: Duration = Duration::from_millis(800);

/// Three readings, and report the middle one.
///
/// **Benchmark discipline, from the master plan:** a single B4 run straight
/// after a build once read 23.3 µs and looked like a 55% regression; three
/// clean runs gave 15.5–15.8.
fn three(mut f: impl FnMut()) -> Duration {
    let mut readings: Vec<Duration> = (0..3)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    readings.sort_unstable();
    readings[1]
}

#[test]
fn b10_a_pin_is_verified_inside_its_budget() {
    let pin = Pin::parse("246813").expect("a valid PIN");
    let stored = hash_pin(&pin).expect("hashes");

    let verify = three(|| {
        assert!(verify_pin(&pin, &stored));
    });
    let hash = three(|| {
        hash_pin(&pin).expect("hashes");
    });

    println!("B10 verify one PIN : {:?} (budget 400 ms, ceiling 800 ms)", verify);
    println!("B10 hash a new PIN : {:?}", hash);

    // **This is the whole of BACKEND-D1's other half.** v1 compared the typed
    // PIN against every active staff row; at this cost, ten staff would be ten
    // times this number before the cashier saw anything. Identity first, then
    // one verification.
    println!("B10 v1's ten-staff scan would have cost: {:?}", verify * 10);

    #[cfg(not(debug_assertions))]
    assert!(
        verify < CEILING,
        "B10: verifying a PIN took {verify:?}, over the {CEILING:?} ceiling"
    );
    let _ = CEILING;
}

/// A wrong PIN must cost the same as a right one.
///
/// Not a speed budget — a **timing** one. If a wrong PIN came back faster, the
/// lock screen would be telling somebody with a stopwatch which staff member
/// exists and which PIN is close, and no lockout helps with that.
#[test]
fn a_wrong_pin_costs_what_a_right_one_costs() {
    let right = Pin::parse("246813").expect("valid");
    let wrong = Pin::parse("111111").expect("valid");
    let stored = hash_pin(&right).expect("hashes");

    let good = three(|| {
        assert!(verify_pin(&right, &stored));
    });
    let bad = three(|| {
        assert!(!verify_pin(&wrong, &stored));
    });

    println!("right PIN: {good:?}   wrong PIN: {bad:?}");

    // Generous, because this is a laptop with other things running. It is
    // looking for an ORDER-of-magnitude difference — an early return — not for
    // constant time, which Argon2 gives us anyway.
    let ratio = good.as_millis().max(1) * 100 / bad.as_millis().max(1);
    assert!(
        (25..=400).contains(&ratio),
        "a wrong PIN answered in {bad:?} against {good:?} for a right one, \
         which is a timing oracle"
    );
}
