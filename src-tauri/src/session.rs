//! **Who is logged in, and when the screen locks itself.**
//!
//! # Locked means "no session". That is the whole of it.
//!
//! What locking deliberately does **not** touch:
//!
//! * the cart (P09's `Mutex<CartState>`) — a shift change at 9 pm cannot cost a
//!   table its order;
//! * the kitchen ledger — nothing is re-sent to the kitchen;
//! * the print queue and its threads — paper keeps coming out, and the
//!   persistent indicator stays visible on the lock screen, because a bill that
//!   printed wrong while the screen was locked is still the cashier's problem
//!   (audit D4);
//! * the database, which is not closed.
//!
//! # The idle clock is Rust's, and it is fed by work
//!
//! Every guarded command touches `last_seen` (see [`crate::guard::require`]).
//! Not mouse movement, not keystrokes crossing the IPC boundary — **work**. A
//! React timer would be a poll (budget M4, and §5 rule 6 says a 250 ms loop is
//! M4 gone before a single feature is written), and worse, it would be bypassed
//! by any screen that is not open.
//!
//! # The first day of a shop's life
//!
//! **If nobody has a PIN, the app does not lock.** It runs as the stand-in
//! counter user holding every permission, and the shell shows a banner that
//! cannot be dismissed. Requirement 3 — *a shop must be able to bill on its
//! first day* — outranks audit C1 on that one day, and this is the same shape
//! as `state::fallback_row`: a real thing, named for what it is, that a setup
//! screen later replaces.
//!
//! The moment the first PIN is set, the lock becomes live and the app locks
//! **immediately**. Proving the PIN works while that person is still standing
//! there is worth four seconds; finding out at 8 am tomorrow is not.

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use mb_auth::{Actor, PermissionSet};
use mb_core::{StaffId, Timestamp};

/// How long the counter may sit untouched before it locks itself.
///
/// P17 owns the setting. Until then it is a constant with its name on it,
/// rather than a number buried in a comparison.
pub const IDLE_LOCK: Duration = Duration::from_secs(5 * 60);

/// How often the idle thread looks. Fifteen seconds is well inside the
/// five-minute period and costs nothing measurable — a sleeping thread is not
/// a poll loop in the sense M4 cares about, because it does no work and touches
/// no screen unless something changed.
pub const IDLE_TICK: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Session {
    pub actor: Actor,
    pub since: Timestamp,
    pub last_seen: Timestamp,
    /// True for the stand-in counter user on a shop with no PINs — the banner
    /// reads off this, and so does "should we lock at all?".
    pub is_stand_in: bool,
}

/// The session, and the two questions everything else asks it.
#[derive(Debug, Default)]
pub struct Sessions {
    current: Mutex<Option<Session>>,
}

impl Sessions {
    #[must_use]
    pub fn new() -> Sessions {
        Sessions {
            current: Mutex::new(None),
        }
    }

    /// Who is at the counter, if anybody.
    #[must_use]
    pub fn current(&self) -> Option<Session> {
        lock(&self.current).clone()
    }

    /// Log somebody in — **or switch to them, keeping everything else.**
    ///
    /// Switching mid-order is the same call: the actor is replaced and nothing
    /// else is. The cart does not know this happened, which is exactly right —
    /// `orders.created_by` was decided when the first line went on, and
    /// `orders.settled_by` is decided when the money is taken.
    pub fn begin(&self, actor: Actor, now: Timestamp, is_stand_in: bool) {
        *lock(&self.current) = Some(Session {
            actor,
            since: now,
            last_seen: now,
            is_stand_in,
        });
    }

    /// Lock the screen. Returns who was there, for the audit row.
    pub fn end(&self) -> Option<Actor> {
        lock(&self.current).take().map(|s| s.actor)
    }

    /// Work happened. Called by the guard, on every guarded command.
    pub fn touch(&self, now: Timestamp) {
        if let Some(session) = lock(&self.current).as_mut() {
            session.last_seen = now;
        }
    }

    /// Has this session gone quiet for too long?
    ///
    /// A stand-in session never idles out: a shop with no PIN has nothing to
    /// lock to, and a lock screen with no way past it is worse than no lock.
    #[must_use]
    pub fn is_idle(&self, now: Timestamp, period: Duration) -> bool {
        let Some(session) = lock(&self.current).as_ref().cloned() else {
            return false;
        };
        if session.is_stand_in {
            return false;
        }
        let millis = now.millis().saturating_sub(session.last_seen.millis());
        u128::from(millis.max(0).unsigned_abs()) >= period.as_millis()
    }
}

/// The permissions the stand-in counter user holds on a shop's first day.
///
/// Everything — because the alternative is a shop that cannot reach its own
/// settings screen to set the first PIN, which is the state that would make
/// audit C1 permanent rather than fixed.
#[must_use]
pub fn stand_in_actor(name: &str, id: &str) -> Actor {
    Actor {
        staff_id: StaffId::new(id),
        name: name.to_owned(),
        role_id: None,
        role_name: Some("Nobody has signed in".to_owned()),
        permissions: PermissionSet::everything(),
        max_discount_bp: None,
        max_discount: None,
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now(seconds: i64) -> Timestamp {
        Timestamp::from_millis(1_770_000_000_000 + seconds * 1_000)
    }

    fn someone() -> Actor {
        let mut actor = stand_in_actor("Rekha", "staff_1");
        actor.role_name = Some("Cashier".to_owned());
        actor
    }

    #[test]
    fn nobody_is_logged_in_to_start_with() {
        let sessions = Sessions::new();
        assert!(sessions.current().is_none());
        assert!(!sessions.is_idle(now(9_999), IDLE_LOCK));
    }

    #[test]
    fn work_keeps_the_screen_open() {
        let sessions = Sessions::new();
        sessions.begin(someone(), now(0), false);
        assert!(sessions.is_idle(now(600), IDLE_LOCK), "it should have idled");
        sessions.touch(now(590));
        assert!(!sessions.is_idle(now(600), IDLE_LOCK), "work did not count");
    }

    #[test]
    fn the_stand_in_never_locks() {
        // A shop with no PIN has nothing to unlock with. A lock screen it
        // cannot get past is worse than no lock at all.
        let sessions = Sessions::new();
        sessions.begin(someone(), now(0), true);
        assert!(!sessions.is_idle(now(86_400), IDLE_LOCK));
    }

    #[test]
    fn switching_user_replaces_only_the_person() {
        let sessions = Sessions::new();
        sessions.begin(someone(), now(0), false);
        let mut other = someone();
        other.staff_id = StaffId::new("staff_2");
        other.name = "Ravi".to_owned();
        sessions.begin(other, now(60), false);

        let current = sessions.current().expect("somebody is here");
        assert_eq!(current.actor.name, "Ravi");
        assert_eq!(current.since, now(60));
    }

    #[test]
    fn ending_a_session_says_who_it_was() {
        let sessions = Sessions::new();
        sessions.begin(someone(), now(0), false);
        let who = sessions.end().expect("somebody was there");
        assert_eq!(who.name, "Rekha");
        assert!(sessions.current().is_none());
        assert!(sessions.end().is_none(), "locking twice is not an error");
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_lock_the_counter() {
        // Windows corrects the clock, and a counter that locks itself because
        // NTP moved time backwards is a counter nobody trusts.
        let sessions = Sessions::new();
        sessions.begin(someone(), now(1_000), false);
        assert!(!sessions.is_idle(now(0), IDLE_LOCK));
    }
}
