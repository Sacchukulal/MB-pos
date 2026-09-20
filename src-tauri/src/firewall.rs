//! Windows Firewall, as it applies to this very program.
//!
//! The one reason a phone on the shop's WiFi cannot reach the counter is an inbound rule
//! against this exe — Windows asks once, on the first start, and a dismissed prompt becomes a
//! permanent BLOCK; an accepted one is an Allow for that one network profile only. The
//! counter therefore reads its own rules in process (`mb_winprint`), says so on the Phones
//! page and in Health, and can repair them with one elevated `netsh` line — the same line the
//! installer writes (`hooks.nsh`).

use serde::Serialize;
use ts_rs::TS;

/// What the firewall says about inbound connections to this exe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "snake_case")]
pub enum FirewallState {
    /// An enabled inbound Allow rule covering every network this PC is on, and no Block.
    Allowed,
    /// An enabled inbound Block rule for this exe on a network it is on — a dismissed prompt,
    /// usually.
    Blocked,
    /// No rule for a network this PC is on: Windows will ask, or quietly block on a public
    /// one.
    NoRule,
    /// The question could not be asked (not Windows, or the firewall service refused).
    Unknown,
}

impl FirewallState {
    #[must_use]
    pub const fn lets_phones_in(self) -> bool {
        matches!(self, FirewallState::Allowed)
    }
}

/// The last answer, so the Phones page and Health do not each read the rules. Read at LAN
/// start and after a repair.
static CACHE: std::sync::Mutex<FirewallState> = std::sync::Mutex::new(FirewallState::Unknown);

/// The last answer read.
#[must_use]
pub fn cached() -> FirewallState {
    *CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Ask again, remember, and return it.
pub fn refresh() -> FirewallState {
    let now = state();
    *CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = now;
    now
}

/// The name every rule this program writes carries, so a second run replaces, never stacks.
/// `hooks.nsh` spells the same name.
const RULE_NAME: &str = "Magic Bill counter";

/// `NET_FW_PROFILE2_ALL` — a rule for every profile carries every bit.
const EVERY_PROFILE: u32 = 0x7fff_ffff;

/// This program's own path, the way the firewall spells it.
fn own_exe() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
}

/// Ask the firewall about this exe. In process, well under a second, no child process.
#[must_use]
pub fn state() -> FirewallState {
    let Some(exe) = own_exe() else {
        return FirewallState::Unknown;
    };
    match mb_winprint::firewall_rules_for(&exe) {
        Ok(report) => judge(&report),
        Err(e) => {
            crate::log_warn!("Windows Firewall could not be read: {e}");
            FirewallState::Unknown
        }
    }
}

/// The rules, read against the networks this PC is on: any enabled Block on one of them
/// wins; else Allowed when enabled Allow rules between them cover every one of them; else
/// nothing. A rule for another profile alone (the Public-only Allow Windows writes when
/// its own prompt is accepted, on a PC that has since joined a Private WiFi) is no rule.
fn judge(report: &mb_winprint::FirewallReport) -> FirewallState {
    let here = if report.current_profiles == 0 {
        EVERY_PROFILE
    } else {
        report.current_profiles
    };
    let mut allowed_on = 0_u32;
    for rule in report.rules.iter().filter(|r| r.enabled) {
        if rule.profiles & here == 0 {
            continue;
        }
        if !rule.allows {
            return FirewallState::Blocked;
        }
        allowed_on |= rule.profiles;
    }
    if allowed_on & here == here {
        FirewallState::Allowed
    } else {
        FirewallState::NoRule
    }
}

/// The two `netsh` commands that put things right: every inbound rule against this exe goes
/// (Windows' own Block and its one-profile Allow alike), and one Allow for every profile takes
/// their place. `hooks.nsh` runs the same two at install, word for word.
#[must_use]
pub fn netsh_line(exe: &str) -> String {
    format!(
        "netsh advfirewall firewall delete rule name=all dir=in program=\"{exe}\" & \
         netsh advfirewall firewall add rule name=\"{RULE_NAME}\" dir=in action=allow \
         program=\"{exe}\" enable=yes profile=any"
    )
}

/// How long the repair is given. It is waiting for a person at a UAC prompt, which Windows
/// itself closes after two minutes.
const REPAIR_DEADLINE: std::time::Duration = std::time::Duration::from_secs(180);

/// Run `netsh_line` as administrator: Windows shows one UAC prompt, the person presses Yes
/// and the phones get in. Returns the state afterwards, read afresh — so the answer is what
/// the firewall now holds, whatever the prompt did.
#[must_use]
pub fn allow() -> FirewallState {
    let Some(exe) = own_exe() else {
        return FirewallState::Unknown;
    };
    let line = format!("/C {}", netsh_line(&exe));
    match mb_winprint::run_elevated("cmd.exe", &line, REPAIR_DEADLINE) {
        Ok(mb_winprint::Elevation::Finished { exit_code }) => {
            crate::log_info!("netsh wrote the firewall rule for this program (exit {exit_code})");
        }
        Ok(mb_winprint::Elevation::Refused) => {
            crate::log_warn!("the firewall repair was refused at the UAC prompt");
        }
        Ok(mb_winprint::Elevation::StillRunning) => {
            crate::log_warn!("the firewall repair did not finish in time");
        }
        Err(e) => crate::log_warn!("the firewall repair could not be started: {e}"),
    }
    refresh()
}

/// What the Phones page says about it, and whether it should offer the button.
#[must_use]
pub fn words(state: FirewallState) -> (&'static str, bool) {
    match state {
        FirewallState::Allowed => ("Windows Firewall lets phones in.", false),
        FirewallState::Blocked => (
            "Windows Firewall is BLOCKING this program, so no phone can reach it. \
             Press the button to allow it (Windows asks once).",
            true,
        ),
        FirewallState::NoRule => (
            "Windows Firewall has no rule letting this program in on this network. On a \
             public WiFi it blocks quietly — press the button to allow it (Windows asks once).",
            true,
        ),
        // The button is offered here too. The rules could not be READ, which says nothing
        // about whether they let phones in — and writing an Allow rule can only help.
        FirewallState::Unknown => (
            "Windows Firewall could not be read. If a phone cannot find this counter, \
             press the button to allow this program through (Windows asks once).",
            true,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_winprint::{FirewallReport, FirewallRule};

    const PRIVATE: u32 = 2;
    const PUBLIC: u32 = 4;

    fn rule(enabled: bool, allows: bool, profiles: u32) -> FirewallRule {
        FirewallRule {
            name: String::new(),
            enabled,
            allows,
            profiles,
        }
    }

    fn on(current_profiles: u32, rules: Vec<FirewallRule>) -> FirewallReport {
        FirewallReport {
            current_profiles,
            rules,
        }
    }

    #[test]
    fn a_block_wins_over_an_allow() {
        let both = vec![rule(true, true, EVERY_PROFILE), rule(true, false, EVERY_PROFILE)];
        assert_eq!(judge(&on(PUBLIC, both.clone())), FirewallState::Blocked);
        let mut reversed = both;
        reversed.reverse();
        assert_eq!(judge(&on(PUBLIC, reversed)), FirewallState::Blocked);
    }

    #[test]
    fn a_disabled_block_does_not_count() {
        let rules = vec![rule(false, false, EVERY_PROFILE), rule(true, true, EVERY_PROFILE)];
        assert_eq!(judge(&on(PUBLIC, rules)), FirewallState::Allowed);
        let only = vec![rule(false, false, EVERY_PROFILE)];
        assert_eq!(judge(&on(PUBLIC, only)), FirewallState::NoRule);
    }

    #[test]
    fn no_rules_means_windows_will_ask() {
        assert_eq!(judge(&on(PUBLIC, Vec::new())), FirewallState::NoRule);
        assert_eq!(judge(&on(0, Vec::new())), FirewallState::NoRule);
    }

    /// The rule Windows writes when its own prompt is accepted is for the network of that
    /// moment only. The shop's WiFi marked Private later: no rule, and no prompt again.
    #[test]
    fn an_allow_for_another_network_is_no_rule_here() {
        let public_only = vec![rule(true, true, PUBLIC)];
        assert_eq!(judge(&on(PUBLIC, public_only.clone())), FirewallState::Allowed);
        assert_eq!(judge(&on(PRIVATE, public_only.clone())), FirewallState::NoRule);
        // On both at once (WiFi and a cable), one of them is not covered.
        assert_eq!(judge(&on(PUBLIC | PRIVATE, public_only)), FirewallState::NoRule);
        let each = vec![rule(true, true, PUBLIC), rule(true, true, PRIVATE)];
        assert_eq!(judge(&on(PUBLIC | PRIVATE, each)), FirewallState::Allowed);
    }

    #[test]
    fn a_block_for_another_network_does_not_block_here() {
        let rules = vec![rule(true, false, PRIVATE), rule(true, true, PUBLIC)];
        assert_eq!(judge(&on(PUBLIC, rules)), FirewallState::Allowed);
    }

    #[test]
    fn the_repair_line_is_what_the_installer_writes() {
        let line = netsh_line(r"C:\Users\A B\AppData\Local\Magic Bill\magic-bill.exe");
        let hooks = include_str!("../hooks.nsh");
        // The same two commands, with the installer's own spelling of the path.
        for piece in line
            .replace(r"C:\Users\A B\AppData\Local\Magic Bill\magic-bill.exe", "$INSTDIR\\magic-bill.exe")
            .split(" & ")
        {
            assert!(hooks.contains(piece), "hooks.nsh does not run: {piece}");
        }
    }

    #[test]
    fn only_the_allowed_state_lets_phones_in() {
        assert!(FirewallState::Allowed.lets_phones_in());
        for s in [FirewallState::Blocked, FirewallState::NoRule, FirewallState::Unknown] {
            assert!(!s.lets_phones_in());
            let (said, _) = words(s);
            assert!(said.contains("Firewall"), "{said}");
        }
        assert!(words(FirewallState::Blocked).1, "a block offers the button");
        // A read that failed says nothing about the rules, so the way to fix them is offered.
        assert!(words(FirewallState::Unknown).1, "unknown offers the button");
        assert!(!words(FirewallState::Allowed).1);
    }
}
