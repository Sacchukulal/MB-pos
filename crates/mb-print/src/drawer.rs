//! The cash drawer.

use mb_core::Settlement;
use serde::{Deserialize, Serialize};

use crate::template::Copy;

/// Which pin of the printer's drawer socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawerPin {
    #[default]
    Pin2,
    Pin5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawerConfig {
    pub enabled: bool,
    pub pin: DrawerPin,
    /// Only open when some of the money was cash.
    pub cash_only: bool,
    /// How long the solenoid is energised, in milliseconds.
    pub on_ms: u16,
    /// And how long before it may fire again.
    pub off_ms: u16,
}

impl Default for DrawerConfig {
    fn default() -> Self {
        DrawerConfig {
            enabled: false,
            pin: DrawerPin::Pin2,
            cash_only: true,
            // 50 ms is long enough for every solenoid on the market and short enough not to
            // cook one.
            on_ms: 50,
            off_ms: 50,
        }
    }
}

impl DrawerConfig {
    /// `ESC p` counts in units of two milliseconds, and one byte of them.
    #[must_use]
    pub fn on_units(self) -> u8 {
        clamp_units(self.on_ms)
    }

    #[must_use]
    pub fn off_units(self) -> u8 {
        clamp_units(self.off_ms)
    }
}

/// `ESC p` counts in units of two milliseconds, in one byte — so 510 ms is the longest pulse
/// the command can express, and anything longer is clamped rather than wrapped round to
/// something short.
fn clamp_units(ms: u16) -> u8 {
    u8::try_from(ms.div_euclid(2)).unwrap_or(u8::MAX)
}

/// Whether this settlement, printed as this kind of copy, may open the drawer.
#[must_use]
pub fn should_kick(
    config: &DrawerConfig,
    settlement: Option<&Settlement>,
    copy: Copy,
    can_kick: bool,
) -> bool {
    if !config.enabled || !can_kick {
        return false;
    }

    // A reprint never opens the drawer, whatever the settings say.
    if !matches!(copy, Copy::Original) {
        return false;
    }

    let Some(settlement) = settlement else {
        // No settlement means this is not a bill — a kitchen ticket, a label, a test print.
        return false;
    };

    if !config.cash_only {
        return true;
    }
    // A split payment with any cash in it still needs the drawer: the cashier has notes in
    // their hand either way.
    settlement
        .payments()
        .iter()
        .any(|p| p.mode.is_cash() && p.amount.paise() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_core::{Money, Payment, PaymentMode};

    fn cash() -> Settlement {
        let mut s = Settlement::new();
        s.add(Payment::new(PaymentMode::Cash, Money::from_paise(50_000)).expect("payment"))
            .expect("add");
        s
    }

    fn card() -> Settlement {
        let mut s = Settlement::new();
        s.add(Payment::new(PaymentMode::Card, Money::from_paise(50_000)).expect("payment"))
            .expect("add");
        s
    }

    fn split() -> Settlement {
        let mut s = Settlement::new();
        s.add(Payment::new(PaymentMode::Card, Money::from_paise(30_000)).expect("payment"))
            .expect("add");
        s.add(Payment::new(PaymentMode::Cash, Money::from_paise(20_000)).expect("payment"))
            .expect("add");
        s
    }

    fn on() -> DrawerConfig {
        DrawerConfig {
            enabled: true,
            ..DrawerConfig::default()
        }
    }

    #[test]
    fn cash_opens_it_and_card_does_not() {
        assert!(should_kick(&on(), Some(&cash()), Copy::Original, true));
        assert!(!should_kick(&on(), Some(&card()), Copy::Original, true));
    }

    #[test]
    fn a_split_with_cash_in_it_opens_it() {
        assert!(should_kick(&on(), Some(&split()), Copy::Original, true));
    }

    #[test]
    fn cash_only_off_opens_it_for_a_card_too() {
        let config = DrawerConfig {
            cash_only: false,
            ..on()
        };
        assert!(should_kick(&config, Some(&card()), Copy::Original, true));
    }

    #[test]
    fn a_reprint_never_opens_it() {
        // The cash-control rule, and it beats every setting above it.
        let config = DrawerConfig {
            cash_only: false,
            ..on()
        };
        assert!(!should_kick(
            &config,
            Some(&cash()),
            Copy::Duplicate { number: 2 },
            true
        ));
        assert!(!should_kick(
            &config,
            Some(&cash()),
            Copy::Voided {
                reason: "wrong table".to_owned()
            },
            true
        ));
    }

    #[test]
    fn a_ticket_with_no_settlement_never_opens_it() {
        assert!(!should_kick(&on(), None, Copy::Original, true));
    }

    #[test]
    fn a_printer_with_no_drawer_socket_never_opens_it() {
        assert!(!should_kick(&on(), Some(&cash()), Copy::Original, false));
    }

    #[test]
    fn the_pulse_is_in_two_millisecond_units_and_cannot_overflow() {
        assert_eq!(DrawerConfig::default().on_units(), 25);
        let silly = DrawerConfig {
            on_ms: 60_000,
            ..DrawerConfig::default()
        };
        assert_eq!(silly.on_units(), 255);
    }
}
