//! Telling the kitchen exactly once.
//!
//! ```text
//!                   ack (on DRAW)            bump
//!   Pending ─────────────────────► Shown ──────────► Bumped
//!      │                             ▲                 │
//!      │ nobody acked in time        │ recall          │ recall
//!      ▼                             └─────────────────┘
//!   Printed ──────────────────────► (arrives at the screen marked
//!                                     already-printed: visible, greyed,
//!                                     silent, NOT new work)
//!
//!   any but Bumped ── close ──► Closed   (the counter is done with the order)
//! ```

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

/// How long the counter waits for the screen before reaching for paper.
pub const ACK_SECONDS: i64 = 20;

/// Where one ticket has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// Sent to the screen; nobody has drawn it yet.
    Pending,
    /// The screen drew it.
    Shown,
    /// A cook pressed bump.
    Bumped,
    /// Nobody acked in time, so it went to paper.
    Printed,
    /// The counter is done with the order: voided, cancelled and seen, or the day closed.
    Closed,
}

impl State {
    /// Is this ticket outstanding work for the kitchen?
    #[must_use]
    pub const fn is_new_work(self) -> bool {
        matches!(self, State::Pending | State::Shown)
    }

    /// Should the screen make a noise about it?
    #[must_use]
    pub const fn should_announce(self) -> bool {
        matches!(self, State::Pending)
    }
}

/// One ticket's journey to the kitchen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    /// The idempotency key. The same id applied twice is applied once.
    pub id: String,
    pub order_id: String,
    /// Which screen (and which printer) this belongs to.
    pub station: String,
    pub state: State,
    /// When the counter told the kitchen.
    pub sent_at: Timestamp,
    /// When a cook's screen drew it.
    pub shown_at: Option<Timestamp>,
    /// When a cook bumped it.
    pub bumped_at: Option<Timestamp>,
}

/// What the counter should do about a delivery, right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Leave it alone.
    Wait,
    /// Print it, and say so on the counter.
    PrintNow,
    /// Nothing to do — it is with the kitchen one way or the other.
    Settled,
}

/// Something a delivery cannot do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeliveryError {
    /// Bumping something the kitchen was never shown.
    #[error("that ticket has not been shown to the kitchen yet")]
    NotShown,
    /// Recalling something that was never bumped.
    #[error("that ticket was not bumped")]
    NotBumped,
    /// Acking a ticket that already went to paper.
    #[error("that ticket was already printed")]
    AlreadyPrinted,
}

impl Delivery {
    #[must_use]
    pub fn new(id: &str, order_id: &str, station: &str, sent_at: Timestamp) -> Delivery {
        Delivery {
            id: id.to_owned(),
            order_id: order_id.to_owned(),
            station: station.to_owned(),
            state: State::Pending,
            sent_at,
            shown_at: None,
            bumped_at: None,
        }
    }

    #[must_use]
    pub fn decide(&self, now: Timestamp, ack_seconds: i64) -> Action {
        match self.state {
            // The only state that can still go wrong.
            State::Pending => {
                let waited = now.millis().saturating_sub(self.sent_at.millis());
                if waited >= ack_seconds.saturating_mul(1_000) {
                    Action::PrintNow
                } else {
                    Action::Wait
                }
            }
            // Shown: a cook can see it.
            State::Shown | State::Bumped | State::Printed | State::Closed => Action::Settled,
        }
    }

    /// The screen drew it.
    pub fn shown(&mut self, at: Timestamp) -> Result<(), DeliveryError> {
        match self.state {
            State::Printed => Err(DeliveryError::AlreadyPrinted),
            // Idempotent: a second ack for the same draw changes nothing, which is what lets
            // the screen ack freely without coordinating.
            State::Shown | State::Bumped | State::Closed => Ok(()),
            State::Pending => {
                self.state = State::Shown;
                self.shown_at = Some(at);
                Ok(())
            }
        }
    }

    /// Nobody acked. The counter printed it.
    pub fn printed(&mut self) {
        if self.state == State::Pending {
            self.state = State::Printed;
        }
    }

    /// A cook bumped it.
    pub fn bump(&mut self, at: Timestamp) -> Result<(), DeliveryError> {
        match self.state {
            State::Pending => Err(DeliveryError::NotShown),
            State::Bumped | State::Closed => Ok(()),
            State::Shown | State::Printed => {
                self.state = State::Bumped;
                self.bumped_at = Some(at);
                Ok(())
            }
        }
    }

    /// A cook bumped the wrong ticket.
    pub fn recall(&mut self) -> Result<(), DeliveryError> {
        if self.state != State::Bumped {
            return Err(DeliveryError::NotBumped);
        }
        // Back to where it came from, not to Pending.
        self.state = if self.shown_at.is_some() {
            State::Shown
        } else {
            State::Printed
        };
        self.bumped_at = None;
        Ok(())
    }

    /// Nothing more for the kitchen. A bumped card stays bumped: the cook's time is a fact.
    pub fn close(&mut self) {
        if self.state != State::Bumped {
            self.state = State::Closed;
        }
    }

    /// How long the kitchen took, once it is done.
    #[must_use]
    pub fn took_millis(&self) -> Option<i64> {
        self.bumped_at
            .map(|at| at.millis().saturating_sub(self.sent_at.millis()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_millis(seconds * 1_000)
    }

    fn a_ticket() -> Delivery {
        Delivery::new("dlv_1", "ord_9f8e", "tandoor", at(0))
    }

    /// The happy path: the screen draws it, a cook bumps it, and the counter never reaches for
    /// paper.
    #[test]
    fn a_screen_that_is_working_keeps_the_paper_in_the_printer() {
        let mut ticket = a_ticket();
        assert_eq!(ticket.decide(at(3), ACK_SECONDS), Action::Wait);

        ticket.shown(at(4)).expect("the screen drew it");
        assert_eq!(ticket.state, State::Shown);
        // Even long past the deadline: it is on a screen, so there is nothing to print.
        assert_eq!(ticket.decide(at(600), ACK_SECONDS), Action::Settled);

        ticket.bump(at(400)).expect("a cook bumped it");
        assert_eq!(ticket.took_millis(), Some(400_000));
    }

    #[test]
    fn a_closed_ticket_is_nothing_more_for_the_kitchen() {
        let mut ticket = a_ticket();
        ticket.close();
        assert_eq!(ticket.state, State::Closed);
        assert!(!ticket.state.is_new_work());
        assert_eq!(ticket.decide(at(9_999), ACK_SECONDS), Action::Settled);
        assert!(ticket.shown(at(1)).is_ok());
        assert_eq!(ticket.state, State::Closed, "a late draw brought it back");

        let mut done = a_ticket();
        done.shown(at(1)).expect("drawn");
        done.bump(at(300)).expect("bumped");
        done.close();
        assert_eq!(done.state, State::Bumped, "closing erased the cook's time");
    }

    #[test]
    fn a_screen_that_never_draws_it_sends_the_ticket_to_paper() {
        let mut ticket = a_ticket();
        assert_eq!(
            ticket.decide(at(ACK_SECONDS - 1), ACK_SECONDS),
            Action::Wait
        );
        assert_eq!(
            ticket.decide(at(ACK_SECONDS), ACK_SECONDS),
            Action::PrintNow
        );

        ticket.printed();
        assert_eq!(ticket.state, State::Printed);
        // And once printed, the counter stops thinking about it.
        assert_eq!(ticket.decide(at(9_999), ACK_SECONDS), Action::Settled);
    }

    #[test]
    fn a_screen_that_comes_back_cannot_turn_printed_work_into_screen_work() {
        let mut ticket = a_ticket();
        ticket.printed();

        assert_eq!(ticket.shown(at(120)), Err(DeliveryError::AlreadyPrinted));
        assert_eq!(ticket.state, State::Printed, "a late ack moved it");
        assert!(
            !ticket.state.is_new_work(),
            "the paper is already in the kitchen"
        );
        assert!(
            !ticket.state.should_announce(),
            "and it must not make a noise"
        );
    }

    /// A printed ticket is still visible to a cook — it just is not NEW work.
    #[test]
    fn a_printed_ticket_is_visible_but_not_counted() {
        let printed = State::Printed;
        assert!(!printed.is_new_work());
        assert!(!printed.should_announce());
        // And it can still be bumped, or it would sit outstanding forever.
        let mut ticket = a_ticket();
        ticket.printed();
        ticket
            .bump(at(300))
            .expect("a cook cleared the printed ticket");
        assert_eq!(ticket.state, State::Bumped);
        assert_eq!(ticket.took_millis(), Some(300_000));
    }

    #[test]
    fn every_step_is_idempotent() {
        let mut ticket = a_ticket();
        ticket.shown(at(2)).expect("drawn");
        ticket.shown(at(5)).expect("drawn again");
        assert_eq!(
            ticket.shown_at,
            Some(at(2)),
            "the second ack moved the time"
        );

        ticket.bump(at(100)).expect("bumped");
        ticket.bump(at(200)).expect("bumped again");
        assert_eq!(
            ticket.bumped_at,
            Some(at(100)),
            "the second bump moved the time"
        );

        // And printing twice, from a timer that fired twice.
        let mut other = a_ticket();
        other.printed();
        other.printed();
        assert_eq!(other.state, State::Printed);
    }

    /// A cook bumps the wrong ticket — Tuesday.
    #[test]
    fn a_recalled_ticket_goes_back_to_where_it_came_from() {
        let mut on_screen = a_ticket();
        on_screen.shown(at(2)).expect("drawn");
        on_screen.bump(at(100)).expect("bumped");
        on_screen.recall().expect("recalled");
        assert_eq!(on_screen.state, State::Shown, "it was on the screen");
        assert_eq!(on_screen.bumped_at, None);
        assert_eq!(on_screen.took_millis(), None);

        // A recalled PRINTED ticket goes back to Printed, not to Pending.
        let mut on_paper = a_ticket();
        on_paper.printed();
        on_paper.bump(at(100)).expect("bumped");
        on_paper.recall().expect("recalled");
        assert_eq!(on_paper.state, State::Printed);
        assert_eq!(
            on_paper.decide(at(9_999), ACK_SECONDS),
            Action::Settled,
            "a recall put the ticket back in the print queue"
        );
    }

    #[test]
    fn nothing_can_be_bumped_before_the_kitchen_has_seen_it() {
        let mut ticket = a_ticket();
        assert_eq!(ticket.bump(at(10)), Err(DeliveryError::NotShown));
        assert_eq!(ticket.recall(), Err(DeliveryError::NotBumped));
    }

    /// The deadline is a kitchen number, not a network one — see `ACK_SECONDS`.
    #[test]
    fn the_deadline_is_measured_in_a_cooks_patience() {
        assert!(
            (10..=30).contains(&ACK_SECONDS),
            "if this has been tuned like a network timeout, read its note"
        );
    }

    #[test]
    fn eight_hours_of_service_decides_correctly_throughout() {
        let mut printed = 0;
        for n in 0..200_i64 {
            // One every 144 seconds is eight hours of a busy kitchen.
            let sent = at(n * 144);
            let ticket = Delivery::new(&format!("dlv_{n}"), "ord", "tandoor", sent);
            // The screen draws every other one; the rest fall to paper.
            if n % 2 == 0 {
                let mut drawn = ticket.clone();
                drawn.shown(at(n * 144 + 2)).expect("drawn");
                assert_eq!(drawn.decide(at(8 * 3_600), ACK_SECONDS), Action::Settled);
            } else if ticket.decide(at(n * 144 + ACK_SECONDS), ACK_SECONDS) == Action::PrintNow {
                printed += 1;
            }
        }
        assert_eq!(printed, 100, "every unacked ticket must reach paper");
    }
}
