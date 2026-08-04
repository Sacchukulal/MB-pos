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

#[cfg(test)]
mod tests {
    use super::*;

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
