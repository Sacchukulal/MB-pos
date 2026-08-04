//! A log on disk, from the first line of `main()` — **audit E7.**
//!
//! > *"There is no proper log on the counter. When you report a problem, there
//! > is nothing to read — the August investigation had to guess from database
//! > side effects because the desktop app's console cannot be read."*
//!
//! That is the whole requirement, and it is why this runs before anything else
//! can fail: the start-up sequence in [`crate::startup`] is the one that can
//! lose a shop, and it is the one nobody can reproduce afterwards.
//!
//! # Why not `tracing`
//!
//! R6. `tracing` + `tracing-subscriber` + `tracing-appender` is three crates
//! and a subscriber model, to write a line to a file. This is ninety lines, it
//! has no macro magic to learn, and it does the two things that actually
//! matter: it rotates by day so a counter that runs for a year does not have
//! one enormous file, and it keeps fourteen days so "send me the log" covers
//! the week somebody noticed.
//!
//! The trade is real and worth writing down: no spans, no structured fields, no
//! per-module filtering. If P22's diagnostics bundle ever needs those, swapping
//! this for `tracing` is a change to this file and to the `log!` calls, which
//! is the shape a replacement should have.

// Calendar arithmetic — the same Gregorian algorithm `mb_core::time` uses, and
// integer division is what it IS. D7's ban is about the money path; there is no
// money within a mile of this file, and a date computed in floats would be the
// actual bug.
#![allow(
    clippy::integer_division,
    clippy::cast_possible_truncation,
    reason = "a calendar and a clock, not money"
)]

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        })
    }
}

#[derive(Debug)]
struct Logger {
    dir: PathBuf,
    /// The day the open file belongs to, as days since the epoch, so the
    /// rotation check is an integer comparison rather than a date parse on
    /// every line.
    day: i64,
    file: Option<File>,
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();

/// How many days of logs to keep.
///
/// Fourteen, because a shop reports a problem on Monday that started the
/// Tuesday before, and because fourteen days of a counter's log is a couple of
/// megabytes rather than a diagnostics bundle nobody can email.
const KEEP_DAYS: i64 = 14;

/// Start logging. Called first, before anything that can fail.
pub fn start(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let logger = Logger {
        dir: dir.to_path_buf(),
        day: i64::MIN,
        file: None,
    };
    let _ = LOGGER.set(Mutex::new(logger));
    prune(dir);
}

/// Where the log files are, so the shell can offer to reveal them — because
/// "send me the log" must not require explaining a file path over the phone.
pub fn directory() -> Option<PathBuf> {
    LOGGER.get().map(|l| lock(l).dir.clone())
}

pub fn write(level: Level, module: &str, message: &str) {
    // A counter that cannot open its log file still bills. Logging is
    // diagnostics, not a dependency of the money path.
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let mut logger = lock(logger);
    let (day, time) = now();

    if logger.day != day || logger.file.is_none() {
        let path = logger.dir.join(format!("magic-bill-{}.log", date_string(day)));
        logger.file = OpenOptions::new().create(true).append(true).open(path).ok();
        logger.day = day;
    }

    if let Some(file) = logger.file.as_mut() {
        let _ = writeln!(file, "{time} {level} [{module}] {message}");
        // Flushed per line rather than buffered: the log's whole purpose is to
        // survive the thing that went wrong, and a buffered last line is
        // exactly the one that would have explained a crash.
        let _ = file.flush();
    }
}

/// Delete anything older than [`KEEP_DAYS`].
fn prune(dir: &Path) {
    let (today, _) = now();
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(stamp) = name
            .strip_prefix("magic-bill-")
            .and_then(|s| s.strip_suffix(".log"))
        else {
            continue;
        };
        if let Some(day) = parse_date(stamp)
            && today - day > KEEP_DAYS
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// `(days since epoch, "HH:MM:SS.mmm" in IST)`.
///
/// **IST, not UTC**, and deliberately: D19 fixes the product at +05:30 because
/// India has one zone and no DST, and a log a shopkeeper reads at 11 pm should
/// say 23:00. The same arithmetic as `mb_core::time`, without reaching into it
/// — a log line is not a business day (D5) and must never be mistaken for one.
fn now() -> (i64, String) {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0));
    let local = millis + 5 * 3_600_000 + 30 * 60_000;
    let day = local.div_euclid(86_400_000);
    let within = local.rem_euclid(86_400_000);
    let h = within.div_euclid(3_600_000);
    let m = within.rem_euclid(3_600_000).div_euclid(60_000);
    let s = within.rem_euclid(60_000).div_euclid(1_000);
    let ms = within.rem_euclid(1_000);
    (day, format!("{h:02}:{m:02}:{s:02}.{ms:03}"))
}

/// Civil date from days since the epoch — Howard Hinnant's algorithm, the same
/// one `mb_core::time` uses.
fn date_string(days: i64) -> String {
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

fn parse_date(text: &str) -> Option<i64> {
    let mut parts = text.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::logging::write($crate::logging::Level::Info, module_path!(), &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::logging::write($crate::logging::Level::Warn, module_path!(), &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::logging::write($crate::logging::Level::Error, module_path!(), &format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_round_trips() {
        for days in [0_i64, 20_669, 19_000, 25_000] {
            let text = date_string(days);
            assert_eq!(parse_date(&text), Some(days), "{text}");
        }
    }

    #[test]
    fn the_epoch_is_the_day_everyone_agrees_on() {
        assert_eq!(date_string(0), "1970-01-01");
        // 2026-08-04, the day this was written.
        assert_eq!(parse_date("2026-08-04"), Some(20_669));
    }
}
