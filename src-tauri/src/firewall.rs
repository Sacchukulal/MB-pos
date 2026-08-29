//! Windows Firewall, as it applies to this very program.
//!
//! The one reason a phone on the shop's WiFi cannot reach the counter is an inbound rule
//! against this exe — Windows asks once, on the first start, and a dismissed prompt becomes a
//! permanent BLOCK. The counter therefore reads its own rules, says so on the Phones page and
//! in Health, and can repair them with one elevated command.

use serde::Serialize;
use ts_rs::TS;

/// What the firewall says about inbound connections to this exe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "snake_case")]
pub enum FirewallState {
    /// An enabled inbound Allow rule, and no Block rule, for this exe.
    Allowed,
    /// An enabled inbound Block rule for this exe — a dismissed prompt, usually.
    Blocked,
    /// No rule either way: Windows will ask, or quietly block on a public network.
    NoRule,
    /// The question could not be asked (not Windows, or PowerShell refused).
    Unknown,
}

impl FirewallState {
    #[must_use]
    pub const fn lets_phones_in(self) -> bool {
        matches!(self, FirewallState::Allowed)
    }
}

/// The last answer, so the Phones page and Health do not each wait on PowerShell. Read at
/// LAN start and after a repair.
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
const RULE_NAME: &str = "Magic Bill counter";

/// This program's own path, the way the firewall spells it.
fn own_exe() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
}

/// Ask the firewall about this exe. Never blocks the caller for long: PowerShell is given a few
/// seconds, and silence is `Unknown`.
#[must_use]
pub fn state() -> FirewallState {
    let Some(exe) = own_exe() else {
        return FirewallState::Unknown;
    };
    if !cfg!(windows) {
        return FirewallState::Unknown;
    }
    // One line per rule: "<Enabled> <Action> <Direction>".
    let script = format!(
        "$p = '{}'; Get-NetFirewallApplicationFilter -ErrorAction SilentlyContinue | \
         Where-Object {{ $_.Program -and ($_.Program -ieq $p) }} | Get-NetFirewallRule | \
         Where-Object {{ $_.Direction -eq 'Inbound' }} | \
         ForEach-Object {{ \"$($_.Enabled) $($_.Action)\" }}",
        exe.replace('\'', "''")
    );
    match powershell(&script) {
        Some(text) => judge(&text),
        None => FirewallState::Unknown,
    }
}

/// The rules, read: any enabled Block wins; else an enabled Allow; else nothing.
fn judge(lines: &str) -> FirewallState {
    let mut allowed = false;
    for line in lines.lines() {
        let line = line.trim();
        if !line.starts_with("True") {
            continue;
        }
        if line.ends_with("Block") {
            return FirewallState::Blocked;
        }
        if line.ends_with("Allow") {
            allowed = true;
        }
    }
    if allowed {
        FirewallState::Allowed
    } else {
        FirewallState::NoRule
    }
}

/// Delete every Block rule against this exe and write one Allow rule for every network
/// profile. Needs administrator rights, so Windows shows one UAC prompt; the person presses
/// Yes and the phones get in. Returns the state afterwards.
#[must_use]
pub fn allow() -> FirewallState {
    let Some(exe) = own_exe() else {
        return FirewallState::Unknown;
    };
    if !cfg!(windows) {
        return FirewallState::Unknown;
    }
    let inner = format!(
        "$p = '{exe}'; \
         Get-NetFirewallApplicationFilter -ErrorAction SilentlyContinue | \
         Where-Object {{ $_.Program -and ($_.Program -ieq $p) }} | Get-NetFirewallRule | \
         Where-Object {{ $_.Direction -eq 'Inbound' -and $_.Action -eq 'Block' }} | \
         Remove-NetFirewallRule; \
         Get-NetFirewallRule -DisplayName '{RULE_NAME}' -ErrorAction SilentlyContinue | \
         Where-Object {{ ($_ | Get-NetFirewallApplicationFilter).Program -ieq $p }} | \
         Remove-NetFirewallRule; \
         New-NetFirewallRule -DisplayName '{RULE_NAME}' -Direction Inbound -Action Allow \
         -Program $p -Profile Any -Enabled True | Out-Null",
        exe = exe.replace('\'', "''"),
    );
    // Run it elevated and wait for it, so the answer below is the truth.
    let outer = format!(
        "Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList \
         '-NoProfile','-ExecutionPolicy','Bypass','-Command','{}'",
        inner.replace('\'', "''"),
    );
    let _ = powershell(&outer);
    refresh()
}

/// Run one PowerShell command with no window, and give it a moment.
fn powershell(command: &str) -> Option<String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
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
            "Windows Firewall has no rule for this program yet. On a public WiFi it \
             blocks quietly — press the button to allow it (Windows asks once).",
            true,
        ),
        FirewallState::Unknown => (
            "Windows Firewall could not be read. If a phone cannot find this counter, \
             allow Magic Bill in Windows Firewall.",
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_wins_over_an_allow() {
        assert_eq!(judge("True Allow\nTrue Block\n"), FirewallState::Blocked);
        assert_eq!(judge("True Block\nTrue Allow\n"), FirewallState::Blocked);
    }

    #[test]
    fn a_disabled_block_does_not_count() {
        assert_eq!(judge("False Block\nTrue Allow\n"), FirewallState::Allowed);
        assert_eq!(judge("False Block\n"), FirewallState::NoRule);
    }

    #[test]
    fn no_rules_means_windows_will_ask() {
        assert_eq!(judge(""), FirewallState::NoRule);
        assert_eq!(judge("\n  \n"), FirewallState::NoRule);
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
        assert!(!words(FirewallState::Allowed).1);
    }
}
