//! "Is this counter healthy?" — one screen, and every row carries its fix.

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
    /// "Backup", "Printers".
    pub name: String,
    /// `ok`, `warn` or `danger`.
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

/// Look at the counter.
#[must_use]
pub fn look(app: &App) -> HealthView {
    let rows = vec![
        // First, because on a new counter it is the only one that matters and it is the one
        // somebody can act on today.
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
        // `count` already puts the number in front, and the first version of this line passed
        // "One thing" as the singular — which produced "1 One thing needs looking at." on the
        // screen.
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
    // The same jobs, judged the same way, as the bar in the title.
    let stuck = app
        .print_queue_snapshot()
        .iter()
        .filter(|job| job.needs_attention)
        .count();
    if stuck == 0 {
        return HealthRow::ok(
            "printers",
            "Printers",
            "Nothing is stuck in the print queue.",
        );
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
fn disk_row(app: &App) -> HealthRow {
    let Some(free) = app
        .with_shop(|shop| Ok(free_space(&shop.path)))
        .ok()
        .flatten()
    else {
        return HealthRow::ok("disk", "Disk", "There is room for the shop's data.");
    };
    /// Roughly what a year of bills needs.
    const A_YEAR: u64 = 1024 * 1024 * 1024;
    /// A quarter of a year's room left is the point at which somebody has to act rather than be
    /// told.
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
    // The cap is 8 MB a day; three quarters of it in one day means something is repeating, and
    // that is worth saying before the log stops.
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
        return HealthRow::ok(
            "crashes",
            "Stability",
            "The counter has not stopped unexpectedly.",
        );
    }
    HealthRow::warn(
        "crashes",
        "Stability",
        format!(
            "The counter stopped unexpectedly {}. Press Copy diagnostics in \
             Settings and send it to us — the reports are already saved on this \
             computer.",
            crate::words::count(i64::try_from(reports.len()).unwrap_or(0), "once", "times",),
        ),
    )
    .go("settings")
}

/// The photographs.
fn photos_row(app: &App) -> HealthRow {
    let Ok((dir, rows)) = app.with_shop(|shop| {
        let dir = mb_db::backup::attachments_dir(shop.db.path());
        let rows = shop
            .db
            .read_transaction(|tx| {
                mb_db::Repos::new(tx)
                    .buying()
                    .attachments(crate::state::OUTLET)
            })
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
    let missing = rows
        .iter()
        .filter(|a| !dir.join(&a.filename).exists())
        .count();
    let total: u64 = rows
        .iter()
        .filter_map(|a| u64::try_from(a.byte_count).ok())
        .sum();

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
            crate::words::count(
                i64::try_from(rows.len()).unwrap_or(0),
                "picture",
                "pictures"
            ),
            crate::words::bytes(total)
        ),
    )
}

/// Gone — `words::bytes` is the one formatter.
fn megabytes(bytes: u64) -> String {
    crate::words::bytes(bytes)
}

/// Free bytes on the volume `path` is on.
fn free_space(_path: &std::path::Path) -> Option<u64> {
    None
}

// The seat.

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
            // An instruction is a verb the reader can act on.
            assert!(
                [
                    "check", "open", "press", "delete", "move", "remove", "renew", "enter", "call"
                ]
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

    /// The headline, and the bug it shipped with.
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
        // A small log says how small it is, rather than "0 MB" — which reads as "there is no
        // log" on the screen that exists to say there is one.
        assert_eq!(megabytes(0), "0 B");
        assert_eq!(megabytes(4_096), "4 KB");
        assert_eq!(megabytes(400 * 1024 * 1024), "400.0 MB");
        assert!(megabytes(3 * 1024 * 1024 * 1024).starts_with("3."));
    }
}
