//! **When the counter falls over, somebody has to be able to find out why** —
//! audit E8, and ANDROID-G5 one product along.
//!
//! > *"No crash reporting. If the app crashes at a customer's shop, we never
//! > find out."*
//!
//! # D95 — the report is always written; SENDING it is the choice
//!
//! Opt-in telemetry is the right default and this keeps it. But the two
//! questions are separate, and v1 conflated them into having neither:
//!
//! * **"may we send this to you?"** is the shop's decision, off until they say
//!   otherwise, with the explanation beside the switch;
//! * **"is there a file describing what happened?"** is not a decision at all.
//!   It is the thing that makes the shop's own support call solvable, and it
//!   costs them nothing.
//!
//! So the panic hook always writes `%APPDATA%\MagicBill\crashes\<when>.txt`,
//! and P22's diagnostics bundle picks it up — which means a shopkeeper who has
//! never agreed to send us anything can still press "Copy diagnostics" and
//! email the crash that just cost them a service.
//!
//! **There is nowhere to send it yet.** Phase 8 builds that. This session
//! builds the file, the switch, the setting and the seam — named, not
//! half-built, which is the same treatment P19 gave the device limit.
//!
//! # What a panic hook must never do
//!
//! Panic, block, or stop the process dying. A crash handler that hangs turns a
//! crash into a hang, and a hang is the one failure a shopkeeper cannot even
//! describe. Everything here is `let _ =`, there is no lock held across a
//! write, and the hook ends by calling the hook it replaced.

use std::fs;
use std::path::{Path, PathBuf};

/// Where reports go. Beside the config, like everything else that is this
/// machine's rather than this shop's (D85).
#[must_use]
pub fn directory() -> PathBuf {
    crate::config::AppConfig::directory().join("crashes")
}

/// Every report, newest first.
#[must_use]
pub fn reports() -> Vec<PathBuf> {
    reports_in(&directory())
}

fn reports_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    // Named `crash-YYYY-MM-DD-HHMMSS.txt`, so a string sort is a time sort.
    found.sort();
    found.reverse();
    found
}

/// **Install the hook.** Called once, from `main`, immediately after logging
/// starts — a panic before this point has the log and nothing else, which is
/// why logging goes first.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Written before anything else, because the next line could be the one
        // that fails.
        let _ = write_report(&directory(), &describe(info));
        // The log too: a crash that left a report but no log line is a crash
        // nobody looking at the log knows happened.
        crate::log_error!("the counter panicked — a crash report was written");
        // And then let the default hook do its job, which includes actually
        // ending the process. A hook that swallows this is a counter that
        // limps on in a state nobody has thought about.
        previous(info);
    }));
}

/// What the report says.
fn describe(info: &std::panic::PanicHookInfo<'_>) -> String {
    let what = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "something went wrong".to_owned());
    let where_ = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
    report_text(&what, where_.as_deref(), &crate::logging::recent())
}

/// The text, built separately so it can be tested without panicking.
///
/// **The recent log lines are the useful part.** "Attempt to subtract with
/// overflow in reports.rs:412" tells us where; the forty lines before it tell
/// us what the shopkeeper was doing, which is the half that makes it
/// reproducible.
#[must_use]
pub fn report_text(what: &str, where_: Option<&str>, recent: &[String]) -> String {
    let mut out = String::new();
    out.push_str("Magic Bill crash report\n");
    out.push_str("=======================\n\n");
    out.push_str(&format!("version   {}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("os        {}\n", std::env::consts::OS));
    out.push_str(&format!("when      {}\n", stamp()));
    out.push_str(&format!("what      {what}\n"));
    out.push_str(&format!("where     {}\n", where_.unwrap_or("not recorded")));
    out.push_str("\nwhat the counter was doing\n--------------------------\n");
    if recent.is_empty() {
        out.push_str("(nothing had been logged yet)\n");
    } else {
        for line in recent {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Write it. Never returns an error to a caller that could not act on one
/// anyway — the process is ending.
fn write_report(dir: &Path, text: &str) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("crash-{}.txt", stamp().replace([':', ' '], "-")));
    fs::write(&path, text)?;
    Ok(path)
}

/// `YYYY-MM-DD HH:MM:SS`, IST — the same fixed +05:30 the log uses (D19).
fn stamp() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0));
    let local = millis + 5 * 3_600_000 + 30 * 60_000;
    let day = local.div_euclid(86_400_000);
    let within = local.rem_euclid(86_400_000);
    #[allow(
        clippy::integer_division,
        reason = "a clock, not money — the same note logging.rs carries"
    )]
    let (h, m, s) = (
        within / 3_600_000,
        (within % 3_600_000) / 60_000,
        (within % 60_000) / 1_000,
    );
    format!("{} {h:02}:{m:02}:{s:02}", civil(day))
}

/// Howard Hinnant's algorithm again. Duplicated from `logging` on purpose:
/// this file must work when the logger does not exist, which is a state a
/// panic during start-up really reaches.
#[allow(
    clippy::integer_division,
    reason = "a calendar, not money"
)]
fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mb-crash-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// **T10, the half that can be tested without killing the test process.**
    ///
    /// The report is written whether or not reporting is switched on — there is
    /// no setting consulted anywhere in this file, and that is the decision.
    #[test]
    fn a_report_is_written_with_reporting_off() {
        let dir = scratch("written");
        let text = report_text(
            "attempt to subtract with overflow",
            Some("src/reports.rs:412:9"),
            &["12:04:11.001 INFO  [magic_bill::flows] order=ord_9f8e settled".to_owned()],
        );
        let path = write_report(&dir, &text).expect("writes");
        assert!(path.exists());

        let back = fs::read_to_string(&path).expect("reads");
        assert!(back.contains("attempt to subtract with overflow"));
        assert!(back.contains("src/reports.rs:412:9"));
        // The half that makes it reproducible.
        assert!(back.contains("ord_9f8e"), "the recent log lines were dropped");
        assert!(back.contains(env!("CARGO_PKG_VERSION")));

        assert_eq!(reports_in(&dir).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A crash before anything has been logged still produces a usable file,
    /// because a panic during start-up is exactly the one nobody can reproduce.
    #[test]
    fn a_crash_before_anything_was_logged_still_says_something() {
        let text = report_text("index out of bounds", None, &[]);
        assert!(text.contains("not recorded"));
        assert!(text.contains("nothing had been logged yet"));
        assert!(text.contains("index out of bounds"));
    }

    /// **A crash report goes in the bundle, so it is scanned like everything
    /// else.** The recent log lines are real log lines, and if one of them had
    /// a secret in it the report would carry it out of the shop.
    #[test]
    fn a_crash_report_is_scanned_like_any_other_file() {
        let text = report_text(
            "something went wrong",
            Some("src/lib.rs:1:1"),
            &["12:04:11.005 INFO  [magic_bill::ipc] Sachin signed in".to_owned()],
        );
        assert!(crate::redact::scan(&text).is_empty(), "{text}");
    }

    #[test]
    fn reports_come_back_newest_first() {
        let dir = scratch("order");
        for stamp in ["crash-2026-08-01-10-00-00", "crash-2026-08-09-11-00-00"] {
            fs::write(dir.join(format!("{stamp}.txt")), "x").expect("writes");
        }
        let found = reports_in(&dir);
        assert!(
            found[0].to_string_lossy().contains("2026-08-09"),
            "{found:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_stamp_is_a_filename_and_a_date() {
        let s = stamp();
        assert_eq!(s.len(), 19, "{s}");
        assert!(s.contains(':'));
        assert!(!s.replace([':', ' '], "-").contains(':'));
    }
}
