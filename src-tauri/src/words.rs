//! The one place a machine state becomes words.

use serde::Serialize;
use ts_rs::TS;

/// How loudly to say it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    /// Something failed, or was refused.
    #[default]
    Problem,
    /// Nothing to do, or already done.
    Notice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct UiError {
    /// Stable, for us. Never shown.
    pub code: String,
    /// For the shopkeeper. Always shown.
    pub message: String,
    /// For the log and the "details" disclosure.
    pub detail: Option<String>,
    /// How loudly to say it.
    pub tone: Tone,
}

impl UiError {
    pub fn new(code: &str, message: impl Into<String>) -> UiError {
        UiError {
            code: code.to_owned(),
            message: message.into(),
            detail: None,
            tone: Tone::Problem,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> UiError {
        self.detail = Some(detail.into());
        self
    }

    /// Nothing went wrong. Say it quietly.
    #[must_use]
    pub fn quietly(mut self) -> UiError {
        self.tone = Tone::Notice;
        self
    }
}

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

pub type UiResult<T> = Result<T, UiError>;

// The conversions. One function per source of failure, and each one is a sentence somebody
// chose.

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
        // NOT a case for `Invariant`.
        _ => (
            "db.failed",
            "The shop's data could not be read. Nothing has been changed.".to_owned(),
        ),
    };
    UiError::new(code, message).with_detail(detail)
}

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

/// Anything at the edge of the operating system — a file that will not open, a folder that
/// cannot be made.
#[must_use]
pub fn from_io(what: &str, error: &std::io::Error) -> UiError {
    UiError::new("io.failed", format!("{what} could not be completed."))
        .with_detail(error.to_string())
}

/// The state a screen asks for before it exists.
#[must_use]
pub fn no_shop_yet() -> UiError {
    UiError::new(
        "shop.none",
        "No shop has been set up on this computer yet. Create a new shop, or \
         restore a backup.",
    )
}

/// An instant, as a person at a counter reads it: `8 Aug, 4:32 pm`.
#[must_use]
pub fn count(how_many: i64, one: &str, many: &str) -> String {
    match how_many {
        0 => format!("no {many}"),
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// "Counter 2", "Counter 2 and the parcel window", "A, B and C" — a list a person reads out
/// loud rather than one a program prints.
#[must_use]
pub fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
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

/// A size a person reads.
#[must_use]
#[allow(
    clippy::integer_division,
    reason = "a file size for a person, not money"
)]
pub fn bytes(count: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    if count < KB {
        format!("{count} B")
    } else if count < MB {
        format!("{} KB", count / KB)
    } else if count < GB {
        format!("{}.{} MB", count / MB, (count % MB) / (MB / 10))
    } else {
        format!("{}.{} GB", count / GB, (count % GB) / (GB / 10))
    }
}

/// "Tuesday" — from the day count; 1970-01-01 was a Thursday.
#[must_use]
pub fn weekday(day: mb_core::BusinessDay) -> &'static str {
    const NAMES: [&str; 7] = [
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
    ];
    let index = day.days_since_epoch().rem_euclid(7);
    NAMES
        .get(usize::try_from(index).unwrap_or(0))
        .copied()
        .unwrap_or("?")
}

/// "Tuesday 2 September" — the way a person names a day they can still remember.
#[must_use]
pub fn day_with_weekday(day: mb_core::BusinessDay, today: mb_core::BusinessDay) -> String {
    format!("{} {}", weekday(day), self::day(day, today))
}

/// "9 August", or "9 August 2025" when it was not this year.
#[must_use]
pub fn day(business_day: mb_core::BusinessDay, today: mb_core::BusinessDay) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let (year, month, date) = business_day.to_ymd();
    let (this_year, _, _) = today.to_ymd();
    let name = MONTHS
        .get(usize::try_from(month.saturating_sub(1)).unwrap_or(0))
        .copied()
        .unwrap_or("?");
    if year == this_year {
        format!("{date} {name}")
    } else {
        format!("{date} {name} {year}")
    }
}

// The licence.

/// The banner, or nothing at all.
#[must_use]
pub fn licence_banner(
    entitlement: &mb_license::Entitlement,
    today: mb_core::BusinessDay,
) -> Option<String> {
    let renews = entitlement
        .renews_on
        .map(|on| day(on, today))
        .unwrap_or_else(|| "recently".to_owned());
    Some(match entitlement.standing {
        // Nothing to say. A banner on a shop that has paid is noise, and noise is what makes
        // the real one invisible.
        mb_license::Standing::Fine => return None,
        mb_license::Standing::InGrace { days_left } => format!(
            "Your plan ended on {renews}. Everything keeps working for another {}.",
            count(i64::from(days_left), "day", "days")
        ),
        mb_license::Standing::Expired => format!(
            "Your plan ran out on {renews}. Reports and phone ordering are paused \
             until it is renewed — billing and printing are not affected."
        ),
        mb_license::Standing::Suspended => {
            "This licence has been suspended. Please call us. You can still bill \
             and print as usual."
                .to_owned()
        }
        mb_license::Standing::Revoked => {
            "This licence has been stopped. Please call us. You can still bill \
             and print as usual."
                .to_owned()
        }
        // Their choice. Do not scold them for it.
        mb_license::Standing::Cancelled => {
            "Your plan is cancelled. Billing and printing carry on working — \
             choose a plan whenever you are ready."
                .to_owned()
        }
        mb_license::Standing::TrialEnded => format!(
            "Your trial ended on {renews}. Billing and printing carry on working \
             — choose a plan to get the rest back."
        ),
        mb_license::Standing::NeverActivated => {
            "This computer has no licence yet. You can bill and print — enter \
             your licence key, or start a free trial at magicbill.in."
                .to_owned()
        }
        mb_license::Standing::NeedsChecking => {
            "We have not been able to check your licence for a while. Reports \
             and phone ordering are paused until we can — billing and printing \
             are not affected."
                .to_owned()
        }
        mb_license::Standing::BoundElsewhere => {
            "This licence belongs to another computer. Billing and printing work \
             here — open Account to move the licence to this one."
                .to_owned()
        }
        mb_license::Standing::Emergency { until } => format!(
            "Emergency unlock: everything works until {}. Please renew or move \
             your licence before then.",
            when(until)
        ),
    })
}

/// A refusal, when the gate said no.
#[must_use]
pub fn licence_refusal(
    refusal: &mb_license::Refusal,
    entitlement: &mb_license::Entitlement,
    today: mb_core::BusinessDay,
) -> UiError {
    let message = match refusal.why {
        mb_license::Why::NotInThePlan => {
            format!("Your plan does not include {}.", refusal.feature.in_words())
        }
        // The banner already says this well, and saying it the same way twice is what makes a
        // person believe it.
        mb_license::Why::NotOperating(_) => {
            licence_banner(entitlement, today).unwrap_or_else(|| {
                format!("{} is not available right now.", refusal.feature.in_words())
            })
        }
    };
    UiError::new(refusal.code(), message).with_detail(format!(
        "{} · {}",
        refusal.feature.code(),
        entitlement.standing.code()
    ))
}

/// A licensing error — the cloud, the file, the emergency code, the cooldown.
#[must_use]
pub fn from_licence(error: &mb_license::LicenceError) -> UiError {
    let message = match error {
        mb_license::LicenceError::TooSoon { days_left } => format!(
            "This licence was moved recently. It can be moved again in {}.",
            count(i64::from(*days_left), "day", "days")
        ),
        mb_license::LicenceError::Timedout => {
            "Our server did not answer. Nothing has changed — please try again \
             in a moment."
                .to_owned()
        }
        mb_license::LicenceError::Cloud(mb_license::CloudError::Unreachable) => {
            "We could not reach our server. Check the internet connection and \
             try again — billing is not affected."
                .to_owned()
        }
        // The server's own sentence, shown as-is.
        mb_license::LicenceError::Cloud(mb_license::CloudError::Refused(said)) => said.clone(),
        mb_license::LicenceError::Cloud(mb_license::CloudError::BoundElsewhere { machine }) => {
            format!(
                "This licence is being used on another computer ({machine}). \
                 Use Move it here to bring it over."
            )
        }
        mb_license::LicenceError::Cloud(mb_license::CloudError::NotRecognised) => {
            "That licence key was not recognised. Check it against the one on your \
             receipt, or sign in at magicbill.in to see it."
                .to_owned()
        }
        other => sentence(&other.to_string()),
    };
    UiError::new(error.code(), message)
}

/// A call under the counter's login — the cloud copy, the pull, a download.
#[must_use]
pub fn from_link(error: &crate::cloud::LinkError) -> UiError {
    let (code, message) = match error {
        crate::cloud::LinkError::Unreachable => (
            "cloud.unreachable",
            "We could not reach our server. Check the internet connection — billing is not \
             affected, and the copy catches up on its own."
                .to_owned(),
        ),
        crate::cloud::LinkError::Unauthorised => (
            "cloud.login",
            "The counter's login to the cloud has expired. It is renewed on its own; try again \
             in a minute."
                .to_owned(),
        ),
        // The server's own sentence, shown as-is.
        crate::cloud::LinkError::Dead(said) | crate::cloud::LinkError::Refused(said) => {
            ("cloud.refused", sentence(said))
        }
        crate::cloud::LinkError::Server(_) => (
            "cloud.server",
            "Our server had a problem. Nothing has changed here — try again in a few minutes."
                .to_owned(),
        ),
        crate::cloud::LinkError::Unreadable => (
            "cloud.unreadable",
            "Our server sent something this version could not read. Update Magic Bill if an \
             update is waiting."
                .to_owned(),
        ),
    };
    UiError::new(code, message).with_detail(error.to_string())
}

/// An error's `Display` turned into a sentence: a capital at the front and a full stop at the
/// end.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let capitalised = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => return String::new(),
    };
    if capitalised.ends_with(['.', '!', '?']) {
        capitalised
    } else {
        format!("{capitalised}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Found by driving the activation dialog.
    #[test]
    fn an_error_becomes_a_sentence() {
        assert_eq!(
            sentence("that licence key and code did not match"),
            "That licence key and code did not match."
        );
        // Already a sentence: left alone rather than given two full stops.
        assert_eq!(
            sentence("We could not reach our server."),
            "We could not reach our server."
        );
        assert_eq!(sentence("Is that right?"), "Is that right?");
        assert_eq!(sentence(""), "");
    }

    /// Every licensing sentence says what still works.
    #[test]
    fn every_licence_banner_says_billing_carries_on() {
        let today = mb_core::BusinessDay::from_ymd(2026, 8, 10);
        for standing in [
            mb_license::Standing::InGrace { days_left: 3 },
            mb_license::Standing::Expired,
            mb_license::Standing::Suspended,
            mb_license::Standing::Revoked,
            mb_license::Standing::Cancelled,
            mb_license::Standing::TrialEnded,
            mb_license::Standing::NeverActivated,
            mb_license::Standing::NeedsChecking,
            mb_license::Standing::BoundElsewhere,
        ] {
            let mut entitlement = mb_license::Entitlement::unactivated(mb_core::Timestamp::EPOCH);
            entitlement.standing = standing;
            entitlement.renews_on = Some(mb_core::BusinessDay::from_ymd(2026, 8, 2));
            let banner = licence_banner(&entitlement, today)
                .unwrap_or_else(|| panic!("{standing:?} had nothing to say"));
            // Either it names billing, or it says everything still works — which is the
            // stronger claim and the one grace makes.
            let says = banner.to_lowercase();
            assert!(
                says.contains("bill") || says.contains("everything keeps working"),
                "{standing:?} does not tell the owner what still works: {banner}"
            );
            assert!(banner.ends_with('.'), "{standing:?}: {banner}");
        }
        // And a shop that has paid is told nothing at all.
        let mut fine = mb_license::Entitlement::unactivated(mb_core::Timestamp::EPOCH);
        fine.standing = mb_license::Standing::Fine;
        assert_eq!(licence_banner(&fine, today), None);
    }

    /// The bug this function exists to make impossible.
    #[test]
    fn a_count_and_its_noun_agree() {
        assert_eq!(count(0, "bill", "bills"), "no bills");
        assert_eq!(count(1, "bill", "bills"), "1 bill");
        assert_eq!(count(2, "bill", "bills"), "2 bills");
        assert_eq!(count(48, "customer", "customers"), "48 customers");
        // Irregular plurals are the caller's to give, which is why both words are arguments
        // rather than one plus an "s".
        assert_eq!(count(1, "penny", "pence"), "1 penny");
        assert_eq!(count(3, "penny", "pence"), "3 pence");
    }

    #[test]
    fn the_clock_reads_the_way_a_shopkeeper_says_it() {
        let at = mb_core::Timestamp::from_millis(1_786_186_920_000);
        assert_eq!(when(at), "8 Aug, 4:32 pm");
    }

    #[test]
    fn midnight_and_noon_both_read_as_twelve() {
        // The two the modulo gets wrong if nobody looks: 0 and 12 are both "12", not "0".
        let midnight = mb_core::Timestamp::from_millis(1_786_213_800_000);
        assert_eq!(when(midnight), "9 Aug, 12:00 am");
        // 06:30 UTC = 12:00 IST.
        let noon = mb_core::Timestamp::from_millis(1_786_170_600_000);
        assert_eq!(when(noon), "8 Aug, 12:00 pm");
    }

    #[test]
    fn a_shopkeeper_never_reads_a_system_message() {
        // The technical text exists; it is just not the thing shown.
        let db = mb_db::DbError::invariant("SQLITE_BUSY: database is locked");
        let ui = from_db(&db);
        assert!(!ui.message.contains("SQLITE"));
        assert!(ui.detail.is_some_and(|d| d.contains("SQLITE")));
    }

    #[test]
    fn every_message_says_what_to_do() {
        let messages = [
            from_db(&mb_db::DbError::NewerSchema { found: 2, known: 1 }),
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
        // The code is what a support call matches on, so it is named and dotted rather than a
        // number somebody has to look up.
        assert_eq!(no_shop_yet().code, "shop.none");
    }
}
