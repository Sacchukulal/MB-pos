//! When the counter falls over, somebody has to be able to find out why.

use std::fs;
use std::path::{Path, PathBuf};

/// Where reports go. Beside the config, like everything else that is this machine's rather than
/// this shop's.
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

/// Install the hook. Called once, from `main`, immediately after logging starts — a panic
/// before this point has the log and nothing else, which is why logging goes first.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Written before anything else, because the next line could be the one that fails.
        let _ = write_report(&directory(), &describe(info));
        // The log too: a crash that left a report but no log line is a crash nobody looking at
        // the log knows happened.
        crate::log_error!("the counter panicked — a crash report was written");
        // And then let the default hook do its job, which includes actually ending the process.
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

/// Write it. Never returns an error to a caller that could not act on one anyway — the process
/// is ending.
fn write_report(dir: &Path, text: &str) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("crash-{}.txt", stamp().replace([':', ' '], "-")));
    fs::write(&path, text)?;
    Ok(path)
}

/// `YYYY-MM-DD HH:MM:SS`, IST — the same fixed +05:30 the log uses.
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

/// Howard Hinnant's algorithm again.
#[allow(clippy::integer_division, reason = "a calendar, not money")]
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
        assert!(
            back.contains("ord_9f8e"),
            "the recent log lines were dropped"
        );
        assert!(back.contains(env!("CARGO_PKG_VERSION")));

        assert_eq!(reports_in(&dir).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A crash before anything has been logged still produces a usable file, because a panic
    /// during start-up is exactly the one nobody can reproduce.
    #[test]
    fn a_crash_before_anything_was_logged_still_says_something() {
        let text = report_text("index out of bounds", None, &[]);
        assert!(text.contains("not recorded"));
        assert!(text.contains("nothing had been logged yet"));
        assert!(text.contains("index out of bounds"));
    }

    /// A crash report goes in the bundle, so it is scanned like everything else.
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
