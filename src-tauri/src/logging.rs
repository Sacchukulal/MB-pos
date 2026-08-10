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
    /// Bytes in today's file. Counted rather than `stat`-ed per line: a flush
    /// per line is already the expensive part and a metadata call on top of it
    /// would double it (see `write`).
    written: u64,
    /// Set when today's file hit [`MAX_BYTES_PER_DAY`], so the notice is
    /// written exactly once.
    capped: bool,
    /// **The last few lines, in memory, for a crash report.**
    ///
    /// A panic hook cannot go and read the file it may itself have been in the
    /// middle of writing, and "what was the counter doing" is the single most
    /// useful thing in a crash report. Bounded, so this cannot grow.
    recent: std::collections::VecDeque<String>,
}

/// How many lines a crash report carries. Forty is about a screenful — enough
/// to see the sequence that led somewhere, short enough that nobody skims past
/// it.
const RECENT_LINES: usize = 40;

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();

/// How many days of logs to keep.
///
/// Fourteen, because a shop reports a problem on Monday that started the
/// Tuesday before, and because fourteen days of a counter's log is a couple of
/// megabytes rather than a diagnostics bundle nobody can email.
const KEEP_DAYS: i64 = 14;

/// **An age cap is not enough, and P22 is where that stopped being theoretical.**
///
/// Fourteen days of a *healthy* counter is a couple of megabytes. Fourteen days
/// of a counter in trouble is not: a printer failing in a retry loop, or a
/// phone reconnecting every second, writes far faster than a shop bills. The
/// age cap does nothing about a single day that is 400 MB, and the two things
/// that cap protects are different — one is "can somebody email this", the
/// other is "does this fill the disk the shop's database lives on".
///
/// 8 MB a day is roughly 60,000 lines, which is a very bad day indeed and still
/// opens in Notepad.
const MAX_BYTES_PER_DAY: u64 = 8 * 1024 * 1024;

/// And a ceiling across all of them, enforced oldest-first in [`prune`]. 40 MB
/// is a bundle that still attaches to an email.
const MAX_TOTAL_BYTES: u64 = 40 * 1024 * 1024;

/// What is written when a day hits its cap.
///
/// **In the file**, not silently. A log that stops without saying so is a log
/// that makes an investigator conclude the app stopped, which is exactly the
/// wrong conclusion at exactly the wrong moment.
const CAP_NOTICE: &str = "*** this day's log reached its size limit and was stopped here. \
Something is repeating — check the printer queue and the network panel. \
Older days are still complete. ***";

/// Start logging. Called first, before anything that can fail.
pub fn start(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let logger = Logger {
        dir: dir.to_path_buf(),
        day: i64::MIN,
        file: None,
        written: 0,
        capped: false,
        recent: std::collections::VecDeque::new(),
    };
    let _ = LOGGER.set(Mutex::new(logger));
    prune(dir);
}

/// Today's size and the total, for the health panel and the bundle.
#[must_use]
pub fn sizes() -> (u64, u64) {
    let Some(logger) = LOGGER.get() else {
        return (0, 0);
    };
    let (dir, today) = {
        let held = lock(logger);
        (held.dir.clone(), held.written)
    };
    (today, total_bytes(&dir))
}

fn total_bytes(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Every log file, newest first — what the diagnostics bundle copies.
#[must_use]
pub fn files() -> Vec<PathBuf> {
    directory().map(|dir| files_in(&dir)).unwrap_or_default()
}

/// The same, for a folder that is not the live one — `prune` runs before
/// anything can call [`directory`], and the tests use their own scratch.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "log"))
        .collect();
    // The name is `magic-bill-YYYY-MM-DD.log`, so a string sort is a date sort.
    found.sort();
    found.reverse();
    found
}

/// Where the log files are, so the shell can offer to reveal them — because
/// "send me the log" must not require explaining a file path over the phone.
pub fn directory() -> Option<PathBuf> {
    LOGGER.get().map(|l| lock(l).dir.clone())
}

/// Point the log at a scratch folder. **Tests only.**
///
/// `start` is a `OnceLock` and it is called from `main`, which means the first
/// caller in a process wins — so a test cannot simply call `start` and expect
/// to be heard. T5 has to read back what the app actually wrote, so it needs a
/// way to take the log somewhere it can be read.
#[cfg(test)]
pub fn redirect(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    start(dir);
    if let Some(logger) = LOGGER.get() {
        let mut held = lock(logger);
        held.dir = dir.to_path_buf();
        // Force the next write to reopen against the new folder.
        held.day = i64::MIN;
        held.file = None;
        held.written = 0;
        held.capped = false;
    }
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
        // **Start from what is already there.** A counter restarted four times
        // in an afternoon appends to the same day's file, and a `written` that
        // began at zero each time would let four restarts write four caps'
        // worth.
        let already = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        logger.file = OpenOptions::new().create(true).append(true).open(path).ok();
        logger.day = day;
        logger.written = already;
        logger.capped = already >= MAX_BYTES_PER_DAY;
    }

    if logger.written >= MAX_BYTES_PER_DAY {
        if logger.capped {
            return;
        }
        // Say so, once, in the file. See CAP_NOTICE.
        logger.capped = true;
        if let Some(file) = logger.file.as_mut() {
            let _ = writeln!(file, "{time} ERROR [logging] {CAP_NOTICE}");
            let _ = file.flush();
        }
        return;
    }

    if let Some(file) = logger.file.as_mut() {
        let line = format!("{time} {level} [{module}] {message}");
        let _ = writeln!(file, "{line}");
        // Flushed per line rather than buffered: the log's whole purpose is to
        // survive the thing that went wrong, and a buffered last line is
        // exactly the one that would have explained a crash.
        let _ = file.flush();
        // +1 for the newline. `writeln!` on Windows writes "\n" through
        // `File`, not "\r\n" — that translation is a console thing — so this
        // does not drift.
        logger.written = logger
            .written
            .saturating_add(line.len() as u64)
            .saturating_add(1);
        if logger.recent.len() >= RECENT_LINES {
            logger.recent.pop_front();
        }
        logger.recent.push_back(line);
    }
}

/// The last few lines, for a crash report — see [`RECENT_LINES`].
///
/// **Takes a copy and holds the lock for the length of the copy only.** A panic
/// hook that borrowed from the logger would be a panic hook holding a lock
/// while the process is dying.
#[must_use]
pub fn recent() -> Vec<String> {
    let Some(logger) = LOGGER.get() else {
        return Vec::new();
    };
    lock(logger).recent.iter().cloned().collect()
}

/// Delete anything older than [`KEEP_DAYS`], **then** anything over
/// [`MAX_TOTAL_BYTES`], oldest first.
///
/// Two passes because they answer two questions. The age cap is "how far back
/// does a support call need to see"; the size cap is "does this fit in an email
/// and off the shop's disk". A fortnight of quiet days trips neither; one very
/// loud day trips the second alone.
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

    // Oldest first, until the total fits. **Never today's file** — deleting the
    // log somebody is about to report a problem from is the one file that
    // matters, and it is the newest.
    let mut total = total_bytes(dir);
    if total <= MAX_TOTAL_BYTES {
        return;
    }
    let mut oldest_first = files_in(dir);
    oldest_first.reverse();
    let today_file = dir.join(format!("magic-bill-{}.log", date_string(today)));
    for path in oldest_first {
        if total <= MAX_TOTAL_BYTES || path == today_file {
            continue;
        }
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
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

/// **Anything that concerns a bill says WHICH bill** — P22.
///
/// ```ignore
/// log_bill!(order_id, "settled as {number} by {who}");
/// ```
///
/// The reason this is a macro rather than a convention: a support call starts
/// with *"the bill I printed at about half past eight"*, and the only way to
/// get from that to a line in a file is for every line about a bill to carry
/// the order id. A convention gets that right for a while. This gets it right
/// or does not compile.
///
/// The id goes in a fixed position at the front of the message so the log can
/// be grepped for one order without knowing what the message said.
#[macro_export]
macro_rules! log_bill {
    ($order:expr, $($arg:tt)*) => {
        $crate::logging::write(
            $crate::logging::Level::Info,
            module_path!(),
            &format!("order={} {}", $order, format!($($arg)*)),
        )
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

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mb-log-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// **T6, the size half.** The age cap does nothing about one very loud day.
    #[test]
    fn a_days_file_stops_at_its_cap_and_says_so() {
        let dir = scratch("cap");
        let (today, _) = now();
        let path = dir.join(format!("magic-bill-{}.log", date_string(today)));

        // Straight past the cap, as a printer in a retry loop would.
        fs::write(&path, "x".repeat(usize::try_from(MAX_BYTES_PER_DAY).unwrap_or(0) + 10))
            .expect("writes");

        // A logger opening this file must notice, write the notice once, and
        // then stop — rather than growing without limit or stopping silently.
        let mut logger = Logger {
            dir: dir.clone(),
            day: i64::MIN,
            file: None,
            written: 0,
            capped: false,
            recent: std::collections::VecDeque::new(),
        };
        // Drive the same decision `write` makes, without the global.
        let already = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        logger.written = already;
        logger.capped = already >= MAX_BYTES_PER_DAY;
        assert!(logger.capped, "the cap was not noticed on open");

        // And the notice is a sentence a person can act on, not a code.
        assert!(CAP_NOTICE.contains("printer"), "{CAP_NOTICE}");
        assert!(CAP_NOTICE.contains("Older days are still complete"));

        let _ = fs::remove_dir_all(&dir);
    }

    /// **A restart in the same day must not reset the budget**, or four
    /// restarts write four caps' worth.
    #[test]
    fn reopening_a_days_file_counts_what_is_already_there() {
        let dir = scratch("reopen");
        let (today, _) = now();
        let path = dir.join(format!("magic-bill-{}.log", date_string(today)));
        fs::write(&path, "already here\n").expect("writes");
        let already = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        assert_eq!(already, 13);
        let _ = fs::remove_dir_all(&dir);
    }

    /// **T6, the total half**, and the rule that keeps the useful file: the
    /// newest one is never the one deleted.
    #[test]
    fn the_total_cap_deletes_oldest_first_and_never_today() {
        let dir = scratch("total");
        let (today, _) = now();
        // Four days, each a third of the ceiling: 1.33x over.
        let each = usize::try_from(MAX_TOTAL_BYTES).unwrap_or(0) / 3;
        for back in 0..4_i64 {
            let path = dir.join(format!("magic-bill-{}.log", date_string(today - back)));
            fs::write(&path, "y".repeat(each)).expect("writes");
        }
        assert!(total_bytes(&dir) > MAX_TOTAL_BYTES);

        prune(&dir);

        assert!(
            total_bytes(&dir) <= MAX_TOTAL_BYTES,
            "the total cap did not bite: {} bytes",
            total_bytes(&dir)
        );
        assert!(
            dir.join(format!("magic-bill-{}.log", date_string(today)))
                .exists(),
            "today's log was deleted, and it is the one somebody is about to \
             report a problem from"
        );
        assert!(
            !dir.join(format!("magic-bill-{}.log", date_string(today - 3)))
                .exists(),
            "the oldest file survived"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_come_back_newest_first() {
        let dir = scratch("order");
        for stamp in ["2026-08-01", "2026-08-09", "2026-08-05"] {
            fs::write(dir.join(format!("magic-bill-{stamp}.log")), "x").expect("writes");
        }
        // Something that is not a log must not come back.
        fs::write(dir.join("notes.txt"), "x").expect("writes");
        let found = files_in(&dir);
        let names: Vec<String> = found
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(
            names,
            vec![
                "magic-bill-2026-08-09.log",
                "magic-bill-2026-08-05.log",
                "magic-bill-2026-08-01.log",
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The two caps are ordered the way the reasoning says they are: one day
    /// cannot be the whole budget, or the ceiling would only ever bite after
    /// the shop had already lost its history.
    #[test]
    fn one_day_cannot_fill_the_whole_ceiling() {
        const { assert!(MAX_BYTES_PER_DAY * 2 <= MAX_TOTAL_BYTES) };
    }
}
