//! **Rate limits — the thing v1's cloud did not have anywhere.**
//!
//! > BACKEND finding: *"No Edge Function has any rate limit; `staff-login`
//! > bcrypt-compares against every active staff row → unauthenticated
//! > quota/CPU burn vector."*
//!
//! The same shape of hole exists here and is worse, because the attacker is on
//! the same WiFi and has no round-trip to Mumbai to slow them down. `/v1/pair`
//! runs Argon2 — 19 MiB and two passes, deliberately (see `mb-auth::pin`) — so
//! an unauthenticated caller who can invoke it in a loop can eat the counter's
//! memory bandwidth during a rush. That is the attack this file exists for.
//!
//! # Two buckets, because there are two populations
//!
//! * **per device**, once a phone is paired: a burst a real waiter can produce
//!   and a sustained rate they cannot;
//! * **per IP**, for the unauthenticated routes, which is where somebody who
//!   is *not* paired lives. `/v1/pair` gets the tightest bucket in the product.
//!
//! # A limited request is answered, never dropped
//!
//! R3. `429` with a `Retry-After`, so the phone backs off deliberately instead
//! of retrying into a wall. And the bucket **refills** — T9 tests both halves,
//! because a limiter that engages and never recovers is an outage.

use std::collections::HashMap;
use std::sync::Mutex;

use mb_core::Timestamp;

/// A token bucket.
///
/// Chosen over a fixed window because a fixed window lets a waiter be refused
/// for something they did fifty-nine seconds ago, which is the behaviour that
/// makes people mash the button.
#[derive(Debug, Clone, Copy)]
pub struct Rate {
    /// How many requests can arrive at once.
    pub burst: u32,
    /// How many refill per second.
    pub per_second: u32,
}

impl Rate {
    /// A paired phone. Twenty at once covers a waiter opening a table and
    /// firing six lines; four a second sustained is faster than anybody types.
    pub const DEVICE: Rate = Rate {
        burst: 20,
        per_second: 4,
    };

    /// **The tightest bucket in the product**, and the reason is in the module
    /// note: this is the Argon2 door and it is open to anybody on the WiFi.
    /// Five tries, one back every twelve seconds.
    pub const PAIRING: Rate = Rate {
        burst: 5,
        per_second: 0,
    };

    /// `/v1/hello`. Cheap to answer, so this only stops a flood.
    pub const HELLO: Rate = Rate {
        burst: 30,
        per_second: 5,
    };
}

/// Slow refill for the pairing bucket: one token every twelve seconds, which
/// `per_second` cannot express as an integer.
const PAIRING_REFILL_MS: i64 = 12_000;

/// How many keys a limiter will hold. A shop has fifteen phones; five hundred
/// is generous for the IP-keyed buckets and small enough that the map is a few
/// tens of kilobytes at worst.
const CAPACITY: usize = 512;

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: u32,
    last: Timestamp,
}

/// One limiter per route family, keyed by device id or by IP.
#[derive(Debug)]
pub struct Limiter {
    rate: Rate,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl Limiter {
    #[must_use]
    pub fn new(rate: Rate) -> Limiter {
        Limiter {
            rate,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// `Ok(())` to serve; `Err(seconds)` to answer 429 with a `Retry-After`.
    ///
    /// # Errors
    ///
    /// How many seconds to wait, when the bucket is empty.
    #[allow(
        clippy::integer_division,
        reason = "a token bucket IS integer division: how many whole tokens \n                  refilled, and how many whole seconds until the next one. \n                  The remainder is not a rupee anybody lost"
    )]
    pub fn check(&self, key: &str, now: Timestamp) -> Result<(), u32> {
        let mut buckets = lock(&self.buckets);

        // **Housekeeping, and the second half of it is the security half.**
        //
        // A limiter keyed by IP on an open network is a map an attacker can
        // grow by changing address — and worse, a fresh key means a fresh full
        // bucket, so rotating addresses is a way around the limit entirely.
        //
        // A full bucket carries no information, so those go first. And if that
        // frees nothing, a NEW key is refused rather than admitted: at that
        // point the counter is being flooded from hundreds of addresses, and
        // "no" is the correct answer to every one of them. Found by a test
        // that grew the map to two thousand entries.
        if buckets.len() >= CAPACITY {
            let full = self.rate.burst;
            buckets.retain(|_, b| b.tokens < full);
            if buckets.len() >= CAPACITY && !buckets.contains_key(key) {
                return Err(1);
            }
        }

        let bucket = buckets.entry(key.to_owned()).or_insert(Bucket {
            tokens: self.rate.burst,
            last: now,
        });

        let elapsed_ms = now.millis().saturating_sub(bucket.last.millis()).max(0);
        let refilled = if self.rate.per_second == 0 {
            u32::try_from(elapsed_ms / PAIRING_REFILL_MS).unwrap_or(u32::MAX)
        } else {
            u32::try_from(
                elapsed_ms
                    .saturating_mul(i64::from(self.rate.per_second))
                    .saturating_div(1_000),
            )
            .unwrap_or(u32::MAX)
        };
        if refilled > 0 {
            bucket.tokens = bucket.tokens.saturating_add(refilled).min(self.rate.burst);
            bucket.last = now;
        }

        if bucket.tokens == 0 {
            // How long until one token is back. Rounded UP, because telling a
            // phone to retry a moment too early produces a second 429 and a
            // phone that decides the counter is broken.
            let wait = if self.rate.per_second == 0 {
                let gap_ms = PAIRING_REFILL_MS.saturating_sub(elapsed_ms).max(0);
                u32::try_from(gap_ms.saturating_add(999) / 1_000).unwrap_or(u32::MAX)
            } else {
                1
            };
            return Err(wait.max(1));
        }

        bucket.tokens = bucket.tokens.saturating_sub(1);
        if bucket.tokens == self.rate.burst.saturating_sub(1) {
            bucket.last = now;
        }
        Ok(())
    }

    /// Let a device start again — used when a phone pairs, so its first
    /// seconds are not spent inside a bucket the pairing attempt drained.
    pub fn forget(&self, key: &str) {
        lock(&self.buckets).remove(key);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_millis(ms)
    }

    /// **T9, both halves.** It engages, and it recovers — a limiter that only
    /// does the first is an outage with a justification.
    #[test]
    fn a_bucket_empties_and_fills_again() {
        let limiter = Limiter::new(Rate {
            burst: 3,
            per_second: 1,
        });

        for n in 0..3 {
            assert!(limiter.check("dev_1", at(0)).is_ok(), "request {n} was refused");
        }
        let wait = limiter.check("dev_1", at(0)).expect_err("the fourth got through");
        assert!(wait >= 1, "a 429 with no Retry-After is a phone that retries into a wall");

        // A second later, one token is back and exactly one gets through.
        assert!(limiter.check("dev_1", at(1_000)).is_ok());
        assert!(limiter.check("dev_1", at(1_000)).is_err());

        // Long enough, and it is full again — never more than full.
        for n in 0..3 {
            assert!(
                limiter.check("dev_1", at(60_000)).is_ok(),
                "request {n} after a minute of quiet was refused"
            );
        }
        assert!(limiter.check("dev_1", at(60_000)).is_err());
    }

    /// One phone hammering the counter must not refuse a different phone.
    #[test]
    fn one_device_cannot_starve_another() {
        let limiter = Limiter::new(Rate {
            burst: 2,
            per_second: 1,
        });
        assert!(limiter.check("dev_loud", at(0)).is_ok());
        assert!(limiter.check("dev_loud", at(0)).is_ok());
        assert!(limiter.check("dev_loud", at(0)).is_err());
        assert!(
            limiter.check("dev_quiet", at(0)).is_ok(),
            "a shared bucket is one waiter taking the counter down"
        );
    }

    /// The pairing bucket is the Argon2 door: five tries, then twelve seconds
    /// for each one back.
    #[test]
    fn the_pairing_door_is_the_tightest_one() {
        let limiter = Limiter::new(Rate::PAIRING);
        for n in 0..Rate::PAIRING.burst {
            assert!(limiter.check("1.2.3.4", at(0)).is_ok(), "try {n}");
        }
        let wait = limiter.check("1.2.3.4", at(0)).expect_err("six argon2 runs got through");
        assert_eq!(wait, 12, "the phone was not told how long to wait");
        assert!(limiter.check("1.2.3.4", at(12_000)).is_ok());
    }

    /// A limiter keyed by IP on an open network is a map somebody can grow by
    /// changing address. It must not grow without bound.
    #[test]
    #[allow(clippy::integer_division, reason = "spreading 2,000 fake addresses over /24s")]
    fn the_map_does_not_grow_for_ever() {
        let limiter = Limiter::new(Rate::HELLO);
        for n in 0..2_000 {
            let _ = limiter.check(&format!("10.0.{}.{}", n / 256, n % 256), at(0));
        }
        assert!(
            lock(&limiter.buckets).len() <= CAPACITY,
            "an attacker changing IP can grow the counter's memory"
        );
    }
}
