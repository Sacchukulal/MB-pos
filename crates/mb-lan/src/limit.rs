//! Rate limits.

use std::collections::HashMap;
use std::sync::Mutex;

use mb_core::Timestamp;

/// A token bucket.
#[derive(Debug, Clone, Copy)]
pub struct Rate {
    /// How many requests can arrive at once.
    pub burst: u32,
    /// How many refill per second.
    pub per_second: u32,
}

impl Rate {
    /// A paired phone. Twenty at once covers a waiter opening a table and firing six lines;
    /// four a second sustained is faster than anybody types.
    pub const DEVICE: Rate = Rate {
        burst: 20,
        per_second: 4,
    };

    /// The tightest bucket in the product, and the reason is in the module note: this is the
    /// Argon2 door and it is open to anybody on the WiFi.
    pub const PAIRING: Rate = Rate {
        burst: 5,
        per_second: 0,
    };

    pub const HELLO: Rate = Rate {
        burst: 30,
        per_second: 5,
    };
}

/// Slow refill for the pairing bucket: one token every twelve seconds, which `per_second`
/// cannot express as an integer.
const PAIRING_REFILL_MS: i64 = 12_000;

/// How many keys a limiter will hold.
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
    #[allow(
        clippy::integer_division,
        reason = "a token bucket IS integer division: how many whole tokens \n                  refilled, and how many whole seconds until the next one. \n                  The remainder is not a rupee anybody lost"
    )]
    pub fn check(&self, key: &str, now: Timestamp) -> Result<(), u32> {
        let mut buckets = lock(&self.buckets);

        // Housekeeping, and the second half of it is the security half.
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
            // How long until one token is back.
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

    /// Let a device start again — used when a phone pairs, so its first seconds are not spent
    /// inside a bucket the pairing attempt drained.
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

    #[test]
    fn a_bucket_empties_and_fills_again() {
        let limiter = Limiter::new(Rate {
            burst: 3,
            per_second: 1,
        });

        for n in 0..3 {
            assert!(
                limiter.check("dev_1", at(0)).is_ok(),
                "request {n} was refused"
            );
        }
        let wait = limiter
            .check("dev_1", at(0))
            .expect_err("the fourth got through");
        assert!(
            wait >= 1,
            "a 429 with no Retry-After is a phone that retries into a wall"
        );

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

    /// The pairing bucket is the Argon2 door: five tries, then twelve seconds for each one
    /// back.
    #[test]
    fn the_pairing_door_is_the_tightest_one() {
        let limiter = Limiter::new(Rate::PAIRING);
        for n in 0..Rate::PAIRING.burst {
            assert!(limiter.check("1.2.3.4", at(0)).is_ok(), "try {n}");
        }
        let wait = limiter
            .check("1.2.3.4", at(0))
            .expect_err("six argon2 runs got through");
        assert_eq!(wait, 12, "the phone was not told how long to wait");
        assert!(limiter.check("1.2.3.4", at(12_000)).is_ok());
    }

    /// A limiter keyed by IP on an open network is a map somebody can grow by changing address.
    #[test]
    #[allow(
        clippy::integer_division,
        reason = "spreading 2,000 fake addresses over /24s"
    )]
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
