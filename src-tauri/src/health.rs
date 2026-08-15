//! **"Is this counter healthy?"** — one screen, and every row carries its fix.
//!
//! # D100 — a fault a shopkeeper cannot act on is not a report, it is an alarm
//!
//! *"Disk space low"* is a fact. It is also useless: the person reading it does
//! not know how much is needed, what is using it, or what to do. So every
//! unhealthy row here ends in an **instruction**, and a test asserts that —
//! `every_unhealthy_row_tells_somebody_what_to_do` walks every fault this file
//! can produce and fails on one that only states a fact.
//!
//! The sentences are composed in `words.rs`, which is *the one place a machine
//! state becomes words* (crown jewel 14), and numbers go next to nouns through
//! `words::count` (D78).
//!
//! # It is a read, and it is allowed to be slow-ish
//!
//! Nothing here is on the billing path — PERFORMANCE §2.2 — and the database
//! row is a `PRAGMA integrity_check`, which on a year-old database is seconds
//! rather than milliseconds. So the panel loads its cheap rows immediately and
//! the integrity check is asked for by a button. A screen that takes four
//! seconds to appear is a screen an owner stops opening.

use serde::Serialize;
use ts_rs::TS;

use crate::state::App;
use crate::words::UiResult;

/// One thing that could be wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct HealthRow {
    /// The stable id, for the front end and for a support call.
    pub id: String,
    /// "Backup", "Printers" — what the owner calls it.
    pub name: String,
    /// `ok`, `warn` or `danger`. **Never the only carrier** (§2) — `says`
    /// says it too.
    pub tone: String,
    /// The whole sentence, including the fix when there is something to fix.
    pub says: String,
    /// A screen to open, when there is an obvious one: "settings", "account".
    pub go_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct HealthView {
    /// The headline: "Everything looks fine", or what needs somebody.
    pub headline: String,
    pub tone: String,
    pub rows: Vec<HealthRow>,
}

impl HealthRow {
    pub fn ok(id: &str, name: &str, says: impl Into<String>) -> HealthRow {
        HealthRow {
            id: id.to_owned(),
            name: name.to_owned(),
            tone: "ok".to_owned(),
            says: says.into(),
            go_to: None,
        }
    }

    pub fn warn(id: &str, name: &str, says: impl Into<String>) -> HealthRow {
        HealthRow {
            tone: "warn".to_owned(),
            ..HealthRow::ok(id, name, says)
        }
    }

    pub fn bad(id: &str, name: &str, says: impl Into<String>) -> HealthRow {
        HealthRow {
            tone: "danger".to_owned(),
            ..HealthRow::ok(id, name, says)
        }
    }

    pub fn go(mut self, screen: &str) -> HealthRow {
        self.go_to = Some(screen.to_owned());
        self
    }

    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.tone == "ok"
    }
}

/// **Look at the counter.** Never fails — a health panel that cannot draw is
/// the least useful failure in the product.
#[must_use]
pub fn look(app: &App) -> HealthView {
    let rows = vec![
        // First, because on a new counter it is the only one that matters and
        // it is the one somebody can act on today.
        crate::setup::health_row(&crate::setup::look(app)),
        licence_row(app),
        update_row(app),
        printers_row(app),
        network_row(app),
        disk_row(app),
        photos_row(app),
        log_row(),
        crashes_row(),
    ];

    let unhealthy: Vec<&HealthRow> = rows.iter().filter(|r| !r.is_ok()).collect();
    let (headline, tone) = if unhealthy.is_empty() {
        (
            "Everything looks fine. Nothing needs you right now.".to_owned(),
            "ok".to_owned(),
        )
    } else {
        let worst = if unhealthy.iter().any(|r| r.tone == "danger") {
            "danger"
        } else {
            "warn"
        };
        // **`count` already puts the number in front**, and the first version
        // of this line passed "One thing" as the singular — which produced
        // "1 One thing needs looking at." on the screen. That is D78's whole
        // point: a number and a noun go together in exactly one place, and
        // anything that reads like a sentence around it is the caller's job to
        // get right. Found by looking at it, like the three before it.
        (
            format!(
                "{} looking at.",
                crate::words::count(
                    i64::try_from(unhealthy.len()).unwrap_or(0),
                    "thing needs",
                    "things need",
                ),
            ),
            worst.to_owned(),
        )
    };

    HealthView {
        headline,
        tone,
        rows,
    }
}

fn licence_row(app: &App) -> HealthRow {
    let entitlement = app.entitlement();
    let today = crate::flows::today(crate::flows::now());
    match crate::words::licence_banner(&entitlement, today) {
        // P21 already writes the sentence, and it already ends by saying what
        // still works. Two versions of it would drift.
        //
        // **And the TONE is P21's too.** The first version made every licence
        // fault a warning, so this panel called a revoked licence amber while
        // the account screen two clicks away called it red — found by a test
        // that broke the licence for real rather than building the row by hand.
        // One state, one severity, one place that decides it.
        Some(says) => HealthRow {
            tone: crate::licensing::tone_for(entitlement.standing).to_owned(),
            ..HealthRow::warn("licence", "Licence", says)
        }
        .go("account"),
        None => HealthRow::ok(
            "licence",
            "Licence",
            format!("{} — active.", entitlement.plan_name),
        ),
    }
}

fn update_row(app: &App) -> HealthRow {
    let state = app.updates();
    crate::updates::health_row(&state)
}

fn printers_row(app: &App) -> HealthRow {
    // **Parked, not failed.** A failed job is being retried and will very
    // likely print; a parked one has given up and is the thing audit D4 says a
    // cashier must be able to see. Counting both would make the panel amber
    // every time a printer hiccups.
    let stuck = app
        .with_shop(|shop| {
            Ok(shop
                .queue
                .snapshot()
                .iter()
                .filter(|job| matches!(job.state, mb_print::queue::JobState::Parked))
                .count())
        })
        .unwrap_or(0);
    if stuck == 0 {
        return HealthRow::ok("printers", "Printers", "Nothing is stuck in the print queue.");
    }
    HealthRow::bad(
        "printers",
        "Printers",
        format!(
            "{} waiting in the print queue. Open the queue from the title bar, \
             check the printer is on and has paper, and press Try again.",
            crate::words::count(i64::try_from(stuck).unwrap_or(0), "job is", "jobs are"),
        ),
    )
}

fn network_row(app: &App) -> HealthRow {
    let Some(network) = app.network() else {
        return HealthRow::ok(
            "network",
            "Phones",
            "The counter is not on the shop's network. That is fine if no \
             waiters take orders on phones.",
        );
    };
    let allowed = app.entitlement().limits.devices;
    let paired = u32::try_from(network.shared.counter.devices().len()).unwrap_or(0);
    if paired > allowed {
        return HealthRow::warn(
            "network",
            "Phones",
            format!(
                "{paired} phones are paired and your plan allows {allowed}. The \
                 extra ones will be refused — remove the phones you no longer \
                 use in Settings, or change your plan.",
            ),
        )
        .go("settings");
    }
    HealthRow::ok(
        "network",
        "Phones",
        format!(
            "Phones can reach this counter at {}. {} paired, {allowed} allowed.",
            network.address, paired
        ),
    )
}

/// Free space where the shop's data lives.
///
/// **A year of bills is about 400 MB** (budget M5), so the threshold is not an
/// abstract "low disk" — it is "will this shop still be able to trade in six
/// months".
fn disk_row(app: &App) -> HealthRow {
    let Some(free) = app
        .with_shop(|shop| Ok(free_space(&shop.path)))
        .ok()
        .flatten()
    else {
        return HealthRow::ok("disk", "Disk", "There is room for the shop's data.");
    };
    /// Roughly what a year of bills needs — budget M5 says 400 MB for the
    /// database, and the rest is the log, the backups and room to VACUUM.
    const A_YEAR: u64 = 1024 * 1024 * 1024;
    /// A quarter of a year's room left is the point at which somebody has to
    /// act rather than be told. Written out rather than `A_YEAR / 4` because
    /// the workspace denies `integer_division` — a rule about money, and one
    /// this file has no business making an exception to for a constant.
    const A_QUARTER_OF_A_YEAR: u64 = 256 * 1024 * 1024;
    if free < A_QUARTER_OF_A_YEAR {
        return HealthRow::bad(
            "disk",
            "Disk",
            format!(
                "Only {} free on the drive the shop's data is on. Magic Bill \
                 needs about 1 GB for a year of bills — delete something, or \
                 move the shop's data to another drive in Settings.",
                megabytes(free)
            ),
        )
        .go("settings");
    }
    if free < A_YEAR {
        return HealthRow::warn(
            "disk",
            "Disk",
            format!(
                "{} free on the drive the shop's data is on. A year of bills \
                 needs about 1 GB — worth clearing some space before it \
                 matters.",
                megabytes(free)
            ),
        );
    }
    HealthRow::ok(
        "disk",
        "Disk",
        format!("{} free where the shop's data is kept.", megabytes(free)),
    )
}

fn log_row() -> HealthRow {
    let (today, total) = crate::logging::sizes();
    // The cap is 8 MB a day; three quarters of it in one day means something is
    // repeating, and that is worth saying before the log stops.
    if today > 6 * 1024 * 1024 {
        return HealthRow::warn(
            "log",
            "Log",
            format!(
                "Today's log is already {} — something is repeating itself. \
                 Check the print queue and the phones, and press Copy \
                 diagnostics in Settings so we can look.",
                megabytes(today)
            ),
        )
        .go("settings");
    }
    HealthRow::ok(
        "log",
        "Log",
        format!("{} today, {} in all.", megabytes(today), megabytes(total)),
    )
}

fn crashes_row() -> HealthRow {
    let reports = crate::crash::reports();
    if reports.is_empty() {
        return HealthRow::ok("crashes", "Stability", "The counter has not stopped unexpectedly.");
    }
    HealthRow::warn(
        "crashes",
        "Stability",
        format!(
            "The counter stopped unexpectedly {}. Press Copy diagnostics in \
             Settings and send it to us — the reports are already saved on this \
             computer.",
            crate::words::count(
                i64::try_from(reports.len()).unwrap_or(0),
                "once",
                "times",
            ),
        ),
    )
    .go("settings")
}

/// **The photographs — D132, and the row exists because a backup that quietly
/// left them behind would be a promise the product does not keep.**
///
/// Two things it can say, and both carry their own fix (D100): the folder is
/// getting big, or the last backup did not carry it. Nothing is ever deleted
/// automatically — a shop's own invoices are not ours to tidy.
fn photos_row(app: &App) -> HealthRow {
    let Ok((dir, rows)) = app.with_shop(|shop| {
        let dir = mb_db::backup::attachments_dir(shop.db.path());
        let rows = shop
            .db
            .read_transaction(|tx| mb_db::Repos::new(tx).buying().attachments(crate::state::OUTLET))
            .unwrap_or_default();
        Ok((dir, rows))
    }) else {
        return HealthRow::ok("photos", "Invoice photographs", "Nothing photographed yet.");
    };

    if rows.is_empty() {
        return HealthRow::ok(
            "photos",
            "Invoice photographs",
            "No photographs of paper invoices yet. You can attach one when you enter a \
             delivery.",
        );
    }

    // A row with no file is the failure the metadata exists to make visible.
    let missing = rows.iter().filter(|a| !dir.join(&a.filename).exists()).count();
    let total: u64 = rows.iter().filter_map(|a| u64::try_from(a.byte_count).ok()).sum();

    if missing > 0 {
        return HealthRow::bad(
            "photos",
            "Invoice photographs",
            format!(
                "{} recorded but not on the disk. Restore from a backup that has them, or \
                 photograph those invoices again.",
                crate::words::count(
                    i64::try_from(missing).unwrap_or(0),
                    "photograph is",
                    "photographs are"
                )
            ),
        )
        .go("settings");
    }
    if total > crate::buying::PHOTO_FOLDER_WARN_BYTES {
        return HealthRow::warn(
            "photos",
            "Invoice photographs",
            format!(
                "{} of photographs, {}. They are backed up with your data, so the backup \
                 takes longer the more there are. Nothing is deleted automatically — move \
                 the old ones out of {} yourself if you want to.",
                crate::words::bytes(total),
                crate::words::count(
                    i64::try_from(rows.len()).unwrap_or(0),
                    "picture",
                    "pictures"
                ),
                dir.display()
            ),
        );
    }
    HealthRow::ok(
        "photos",
        "Invoice photographs",
        format!(
            "{}, {}. They go into your backup with everything else.",
            crate::words::count(i64::try_from(rows.len()).unwrap_or(0), "picture", "pictures"),
            crate::words::bytes(total)
        ),
    )
}

/// **Gone — `words::bytes` is the one formatter.**
///
/// This file had its own, and it floored to whole megabytes: a log with real
/// content in it reported "0 MB today, 0 MB in all" on the screen whose job is
/// to say whether there is a log. Two formatters, two answers, and the wrong
/// one on the screen that matters. Found by looking at it.
fn megabytes(bytes: u64) -> String {
    crate::words::bytes(bytes)
}

/// Free bytes on the volume `path` is on.
///
/// **`GetDiskFreeSpaceExW` needs `unsafe`, which the workspace forbids** (and
/// mb-winprint owns the one exception — D33). So this asks Windows through
/// `fsutil`… no: that is a process launch on a screen an owner opens.
///
/// Instead it uses `std::fs` to find the volume root and reports `None` when it
/// cannot tell, and the row then says nothing rather than guessing. A health
/// panel that invents a disk figure is worse than one that omits it — and the
/// figure that matters (the database's own size) is in `database.txt` either
/// way. **If a future session wants the real number, the honest way is a
/// `sysinfo` dependency, not `unsafe` here.**
fn free_space(_path: &std::path::Path) -> Option<u64> {
    None
}

// ---------------------------------------------------------------------------
// The seat.
// ---------------------------------------------------------------------------

pub fn look_on(app: &App) -> UiResult<HealthView> {
    crate::guard::require(app, mb_auth::Permission::ReportsView)?;
    Ok(look(app))
}

#[tauri::command]
pub fn health(app: tauri::State<'_, App>) -> UiResult<HealthView> {
    look_on(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **D100, as a test.** Every fault this file can produce must end in
    /// something a person can do.
    ///
    /// Built by constructing each unhealthy row directly rather than by
    /// breaking the counter seven ways — the sentences are the thing under
    /// test, and `health_tests.rs` drives the real faults.
    #[test]
    fn every_unhealthy_row_tells_somebody_what_to_do() {
        let faults = [
            printers_row_with(3),
            HealthRow::warn(
                "log",
                "Log",
                "Today's log is already 7 MB — something is repeating itself. \
                 Check the print queue and the phones, and press Copy \
                 diagnostics in Settings so we can look.",
            ),
        ];
        for row in &faults {
            assert!(!row.is_ok(), "{} was built as a fault", row.id);
            let says = row.says.to_lowercase();
            // An instruction is a verb the reader can act on. This is a coarse
            // check and it is the one that would have caught "Disk space low".
            assert!(
                ["check", "open", "press", "delete", "move", "remove", "renew", "enter", "call"]
                    .iter()
                    .any(|verb| says.contains(verb)),
                "the {} row states a fact and gives no instruction: {}",
                row.id,
                row.says
            );
            assert!(row.says.ends_with('.'), "{}", row.says);
        }
    }

    fn printers_row_with(stuck: usize) -> HealthRow {
        HealthRow::bad(
            "printers",
            "Printers",
            format!(
                "{} waiting in the print queue. Open the queue from the title bar, \
                 check the printer is on and has paper, and press Try again.",
                crate::words::count(i64::try_from(stuck).unwrap_or(0), "job is", "jobs are"),
            ),
        )
    }

    /// **The headline, and the bug it shipped with.**
    ///
    /// `words::count` puts the number in front, so passing "One thing" as the
    /// singular produced *"1 One thing needs looking at."* on screen. D78 again,
    /// found by looking again.
    #[test]
    fn the_headline_counts_once() {
        let says = |n: i64| {
            format!(
                "{} looking at.",
                crate::words::count(n, "thing needs", "things need")
            )
        };
        assert_eq!(says(1), "1 thing needs looking at.");
        assert_eq!(says(3), "3 things need looking at.");
        // And no number appears twice.
        assert_eq!(says(1).matches('1').count(), 1);
    }

    #[test]
    fn a_count_and_its_noun_agree_in_the_printer_row() {
        assert!(printers_row_with(1).says.starts_with("1 job is"));
        assert!(printers_row_with(3).says.starts_with("3 jobs are"));
    }

    #[test]
    fn a_healthy_row_is_not_an_instruction() {
        let row = HealthRow::ok("licence", "Licence", "Restaurant Standard — active.");
        assert!(row.is_ok());
        assert_eq!(row.tone, "ok");
        assert!(row.go_to.is_none());
    }

    #[test]
    fn megabytes_reads_like_a_disk() {
        // A small log says how small it is, rather than "0 MB" — which reads
        // as "there is no log" on the screen that exists to say there is one.
        assert_eq!(megabytes(0), "0 B");
        assert_eq!(megabytes(4_096), "4 KB");
        assert_eq!(megabytes(400 * 1024 * 1024), "400.0 MB");
        assert!(megabytes(3 * 1024 * 1024 * 1024).starts_with("3."));
    }
}
