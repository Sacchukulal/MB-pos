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

const CEILING: Duration = Duration::from_millis(800);

/// Three readings, and report the middle one.
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
    let pin = Pin::parse("2468").expect("a valid PIN");
    let stored = hash_pin(&pin).expect("hashes");

    let verify = three(|| {
        assert!(verify_pin(&pin, &stored));
    });
    let hash = three(|| {
        hash_pin(&pin).expect("hashes");
    });

    println!(
        "B10 verify one PIN : {:?} (budget 400 ms, ceiling 800 ms)",
        verify
    );
    println!("B10 hash a new PIN : {:?}", hash);

    // This is the whole of BACKEND-D1's other half.
    println!("B10 v1's ten-staff scan would have cost: {:?}", verify * 10);

    #[cfg(not(debug_assertions))]
    assert!(
        verify < CEILING,
        "B10: verifying a PIN took {verify:?}, over the {CEILING:?} ceiling"
    );
    let _ = CEILING;
}

/// A wrong PIN must cost the same as a right one.
#[test]
fn a_wrong_pin_costs_what_a_right_one_costs() {
    let right = Pin::parse("2468").expect("valid");
    let wrong = Pin::parse("1111").expect("valid");
    let stored = hash_pin(&right).expect("hashes");

    let good = three(|| {
        assert!(verify_pin(&right, &stored));
    });
    let bad = three(|| {
        assert!(!verify_pin(&wrong, &stored));
    });

    println!("right PIN: {good:?}   wrong PIN: {bad:?}");

    // Generous, because this is a laptop with other things running.
    let ratio = good.as_millis().max(1) * 100 / bad.as_millis().max(1);
    assert!(
        (25..=400).contains(&ratio),
        "a wrong PIN answered in {bad:?} against {good:?} for a right one, \
         which is a timing oracle"
    );
}
