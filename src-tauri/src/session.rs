//! Who is logged in, and when the screen locks itself.

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use mb_auth::{Actor, PermissionSet};
use mb_core::{StaffId, Timestamp};

/// How long the counter may sit untouched before it locks itself, when the shop has not said.
#[cfg(test)]
pub const IDLE_LOCK: Duration = Duration::from_secs(5 * 60);

/// The shop's answer, as a duration.
#[must_use]
pub fn idle_lock_for(minutes: u32) -> Option<Duration> {
    if minutes == 0 {
        None
    } else {
        Some(Duration::from_secs(u64::from(minutes) * 60))
    }
}

/// How often the idle thread looks.
pub const IDLE_TICK: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Session {
    pub actor: Actor,
    pub last_seen: Timestamp,
    /// True for the stand-in counter user on a shop with no PINs — the banner reads off this,
    /// and so does "should we lock at all?".
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

    /// Log somebody in — or switch to them, keeping everything else.
    pub fn begin(&self, actor: Actor, now: Timestamp, is_stand_in: bool) {
        *lock(&self.current) = Some(Session {
            actor,
            last_seen: now,
            is_stand_in,
        });
    }

    pub fn end(&self) -> Option<Actor> {
        lock(&self.current).take().map(|s| s.actor)
    }

    /// Work happened. Called by the guard, on every guarded command.
    pub fn touch(&self, now: Timestamp) {
        if let Some(session) = lock(&self.current).as_mut() {
            session.last_seen = now;
        }
    }

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
#[must_use]
pub fn stand_in_actor(name: &str, id: &str) -> Actor {
    Actor {
        staff_id: StaffId::new(id),
        name: name.to_owned(),
        role_id: None,
        // Beside the name in the title bar this reads as a description of the till, not a
        // contradiction.
        role_name: Some("No PIN set".to_owned()),
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
        assert!(
            sessions.is_idle(now(600), IDLE_LOCK),
            "it should have idled"
        );
        sessions.touch(now(590));
        assert!(!sessions.is_idle(now(600), IDLE_LOCK), "work did not count");
    }

    #[test]
    fn the_stand_in_never_locks() {
        // A shop with no PIN has nothing to unlock with.
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
        // Windows corrects the clock, and a counter that locks itself because NTP moved time
        // backwards is a counter nobody trusts.
        let sessions = Sessions::new();
        sessions.begin(someone(), now(1_000), false);
        assert!(!sessions.is_idle(now(0), IDLE_LOCK));
    }
}
