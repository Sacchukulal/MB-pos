//! **The one place a machine state becomes words** — crown jewel 14.
//!
//! > *"One place turns machine states into words (`statusCopy`). A bare
//! > 'Offline' shown for four different situations once cost you an evening."*
//!
//! The idea is rebuilt; the file is not. And audit **F8** is the other half:
//! *"some messages are still technical — errors show raw system text to a
//! restaurant owner."*
//!
//! So every error that crosses the IPC boundary carries three things:
//!
//! * a **code**, which is for us: it is stable, it is greppable, and it is what
//!   a support call can be matched against;
//! * a **message**, which is for the shopkeeper: it says what went wrong and
//!   what to do about it, in the words UI_GUIDELINES §6 asks for — *"never a
//!   system message, never an apology"*;
//! * a **detail**, which is the technical text, shown behind a disclosure and
//!   written to the log. It is never the thing the owner reads first.
//!
//! The rule that keeps this honest: **a `?` on a `DbError` or a `PrintError`
//! must not compile into a command.** Every error is converted here, by hand,
//! deliberately, because the conversion is where the sentence gets written.

use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct UiError {
    /// Stable, for us. Never shown.
    pub code: String,
    /// For the shopkeeper. Always shown.
    pub message: String,
    /// For the log and the "details" disclosure. Shown on request.
    pub detail: Option<String>,
}

impl UiError {
    pub fn new(code: &str, message: impl Into<String>) -> UiError {
        UiError {
            code: code.to_owned(),
            message: message.into(),
            detail: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> UiError {
        self.detail = Some(detail.into());
        self
    }
}

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

pub type UiResult<T> = Result<T, UiError>;

// ---------------------------------------------------------------------------
// The conversions. One function per source of failure, and each one is a
// sentence somebody chose.
// ---------------------------------------------------------------------------

/// Storage.
///
/// The distinctions that matter to a shopkeeper are not the ones `DbError`
/// draws, so this collapses them into the four situations they can actually do
/// something about: the file is missing, the file is being used by something
/// else, the file is not one of ours, and everything else.
#[must_use]
pub fn from_db(error: &mb_db::DbError) -> UiError {
    let detail = error.to_string();
    let (code, message) = match error {
        mb_db::DbError::Open { path, .. } => (
            "db.open",
            format!(
                "The shop's data file could not be opened.\n{}\n\nCheck that the \
                 drive is connected and that no other copy of Magic Bill is running.",
                path.display()
            ),
        ),
        mb_db::DbError::MigrationChanged { .. } => (
            "db.migration_changed",
            "This data file was made by a different build of Magic Bill and cannot \
             be opened safely. Do not delete it — send it to support."
                .to_owned(),
        ),
        mb_db::DbError::NewerSchema { .. } => (
            "db.newer_schema",
            "This data file was made by a NEWER version of Magic Bill. Update to \
             the latest version and open it again."
                .to_owned(),
        ),
        // NOT a case for `Invariant`. Its text reads like a sentence for a
        // person — "a dine-in order needs a table before it can be opened" —
        // and surfacing it was tempting after a real toast said "the shop's
        // data could not be read" for exactly that. But the same variant also
        // carries "SQLITE_BUSY: database is locked", which is audit F8's whole
        // complaint, and `a_shopkeeper_never_reads_a_system_message` below
        // caught the attempt. The rules a cashier can act on get a guard and
        // words of their own where they are broken — see `flows::complete_bill`
        // and its `bill.no_table`.
        _ => (
            "db.failed",
            "The shop's data could not be read. Nothing has been changed.".to_owned(),
        ),
    };
    UiError::new(code, message).with_detail(detail)
}


/// Printing.
#[must_use]
pub fn from_print(error: &mb_print::PrintError) -> UiError {
    let detail = error.to_string();
    let (code, message) = match error {
        mb_print::PrintError::AmountTooWide { .. } => (
            "print.amount_too_wide",
            "An amount on this bill is too wide for the paper. Use wider paper, or \
             check the amount."
                .to_owned(),
        ),
        mb_print::PrintError::Invalid(_) => (
            "print.invalid",
            "This could not be printed. Nothing has been sent to the printer.".to_owned(),
        ),
    };
    UiError::new(code, message).with_detail(detail)
}

/// Anything at the edge of the operating system — a file that will not open, a
/// folder that cannot be made.
#[must_use]
pub fn from_io(what: &str, error: &std::io::Error) -> UiError {
    UiError::new("io.failed", format!("{what} could not be completed."))
        .with_detail(error.to_string())
}

/// The state a screen asks for before it exists.
///
/// A first run and a restore in progress are both real, and both must produce a
/// sentence rather than a crash — the app opens into them (P08 item 2 step 5).
#[must_use]
pub fn no_shop_yet() -> UiError {
    UiError::new(
        "shop.none",
        "No shop has been set up on this computer yet. Create a new shop, or \
         restore a backup.",
    )
}

/// An instant, as a person at a counter reads it: `8 Aug, 4:32 pm`.
///
/// **Formatted in Rust, like money** (D39, R8). TypeScript has `toLocale…`, and
/// using it would put a second clock in the product — one that reads the
/// machine's timezone rather than the shop's, and would therefore disagree with
/// the business day (D19: India is a fixed +05:30 and everything here is exact
/// because of it).
///
/// The workspace denies `integer_division` because D7 is about money, where a
/// dropped remainder is a rupee somebody lost. Sixty seconds really do make a
/// minute, and the remainder is the other half of the answer rather than a loss.
/// **A count and its noun, with the right ending.**
///
/// "3 bills", "1 bill", "no bills" — never "1 bills" and never "0 bill(s)".
///
/// P17 shipped "1 have been issued" and P18 shipped "in the 1 days before" and
/// "0 bill(s)", all three found by looking at the screen rather than by a test.
/// They are the same bug: a sentence assembled from a number and a noun, by
/// somebody who had a plural in mind. One function, so it is assembled once.
///
/// The zero case says "no bills" rather than "0 bills" because that is how a
/// person reads a figure out loud, and because a screen full of zeroes is
/// harder to scan than a screen that says nothing happened.
#[must_use]
pub fn count(how_many: i64, one: &str, many: &str) -> String {
    match how_many {
        0 => format!("no {many}"),
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

#[must_use]
#[allow(
    clippy::integer_division,
    reason = "a clock is the one place a remainder is not a loss"
)]
pub fn when(at: mb_core::Timestamp) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (days, seconds) = at.to_local_parts(mb_core::UtcOffset::INDIA);
    let (_, month, day) = mb_core::time::civil_from_days(days);
    let minutes_of_day = seconds / 60;
    let hour24 = minutes_of_day / 60;
    let minute = minutes_of_day % 60;
    let suffix = if hour24 < 12 { "am" } else { "pm" };
    // 0 is midnight and 12 is noon, both of which read as 12.
    let hour = match hour24 % 12 {
        0 => 12,
        h => h,
    };
    let month_name = MONTHS
        .get(usize::try_from(month.saturating_sub(1)).unwrap_or(0))
        .copied()
        .unwrap_or("?");
    format!("{day} {month_name}, {hour}:{minute:02} {suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The bug this function exists to make impossible.** Three shipped:
    /// "1 have been issued" (P17), "in the 1 days before" and "0 bill(s)"
    /// (P18) — all found by looking at a screen, none by a test.
    #[test]
    fn a_count_and_its_noun_agree() {
        assert_eq!(count(0, "bill", "bills"), "no bills");
        assert_eq!(count(1, "bill", "bills"), "1 bill");
        assert_eq!(count(2, "bill", "bills"), "2 bills");
        assert_eq!(count(48, "customer", "customers"), "48 customers");
        // Irregular plurals are the caller's to give, which is why both words
        // are arguments rather than one plus an "s".
        assert_eq!(count(1, "penny", "pence"), "1 penny");
        assert_eq!(count(3, "penny", "pence"), "3 pence");
    }

    #[test]
    fn the_clock_reads_the_way_a_shopkeeper_says_it() {
        // 2026-08-08, 11:02:00 UTC — which is 4:32 pm in the shop.
        let at = mb_core::Timestamp::from_millis(1_786_186_920_000);
        assert_eq!(when(at), "8 Aug, 4:32 pm");
    }

    #[test]
    fn midnight_and_noon_both_read_as_twelve() {
        // The two the modulo gets wrong if nobody looks: 0 and 12 are both
        // "12", not "0".
        // 2026-08-08 18:30 UTC = 2026-08-09 00:00 IST.
        let midnight = mb_core::Timestamp::from_millis(1_786_213_800_000);
        assert_eq!(when(midnight), "9 Aug, 12:00 am");
        // 06:30 UTC = 12:00 IST.
        let noon = mb_core::Timestamp::from_millis(1_786_170_600_000);
        assert_eq!(when(noon), "8 Aug, 12:00 pm");
    }

    #[test]
    fn a_shopkeeper_never_reads_a_system_message() {
        // Audit F8. The technical text exists; it is just not the thing shown.
        let db = mb_db::DbError::invariant("SQLITE_BUSY: database is locked");
        let ui = from_db(&db);
        assert!(!ui.message.contains("SQLITE"));
        assert!(ui.detail.is_some_and(|d| d.contains("SQLITE")));
    }

    #[test]
    fn every_message_says_what_to_do() {
        // UI_GUIDELINES §6: "errors say what went wrong AND what to do".
        // Checked crudely on purpose — a test that demanded a specific phrasing
        // would be a test that stops anybody improving the words.
        let messages = [
            from_db(&mb_db::DbError::NewerSchema {
                found: 2,
                known: 1,
            }),
            no_shop_yet(),
        ];
        for m in messages {
            assert!(
                m.message.len() > 40,
                "\"{}\" is too short to say what to do",
                m.message
            );
            assert!(!m.message.to_lowercase().contains("sorry"), "no apologies");
        }
    }

    #[test]
    fn a_code_is_stable_and_a_message_is_not() {
        // The code is what a support call matches on, so it is named and dotted
        // rather than a number somebody has to look up.
        assert_eq!(no_shop_yet().code, "shop.none");
    }
}
