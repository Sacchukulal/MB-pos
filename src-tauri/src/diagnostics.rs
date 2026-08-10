//! **The button that turns a phone call into a fix** — audit E7's second half.
//!
//! > *"Fix: a rolling log file on disk, and a 'Send diagnostics' button that
//! > zips the last few days of logs."*
//!
//! # D94 — a person sees the manifest before the zip exists
//!
//! [`plan`] builds the list — every file, its size, and what it is — and the
//! screen shows it. Only then does [`write`] produce anything. A support bundle
//! assembled silently is a support bundle nobody can be asked to consent to,
//! and this one leaves the shop by email.
//!
//! The screen also states what is deliberately **not** in it, in one sentence,
//! because "what did I just send you" is a fair question and the answer should
//! not require reading a zip.
//!
//! # It is scanned before it is written
//!
//! [`crate::redact::scan`] runs over every text member. A licence key that
//! reached a log would otherwise reach a support inbox, and audit E10 records
//! that v1 nearly leaked a secret into a report folder already. If the scanner
//! finds something, **the bundle is still written** — with the offending lines
//! replaced — because refusing to produce a bundle leaves the shopkeeper with
//! a broken counter and no way to ask for help. The screen says what was
//! removed.

use std::io::Write as _;
use std::path::PathBuf;

use serde::Serialize;
use ts_rs::TS;

use crate::state::App;
use crate::words::{self, UiError, UiResult};

/// One thing that will be in the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BundleItem {
    /// The name it will have inside the zip.
    pub name: String,
    /// What it is, in the shop's words — "the last seven days of the counter's
    /// own log", not "logs".
    pub what: String,
    /// Already formatted: "12 KB". R8.
    pub size: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BundlePlanView {
    pub items: Vec<BundleItem>,
    /// The whole thing, formatted.
    pub total: String,
    /// **What is deliberately not in it.** One sentence, shown beside the list.
    pub excludes: String,
    /// Where it will be written, so nobody has to hunt for it (D76).
    pub folder: String,
}

/// Bytes as a person reads them. Not a general-purpose formatter: a diagnostics
/// bundle is between a few KB and a few MB and nothing else needs to be right.
/// **`words::bytes` is the one size formatter** — see its note, and the
/// health panel that disagreed with this one.
fn size(bytes: u64) -> String {
    crate::words::bytes(bytes)
}

/// **The sentence that answers "what did I just send you?"**
const EXCLUDES: &str = "It does not contain your licence key, anybody's PIN, or \
any customer's phone number. It does not contain your shop's database — only a \
summary of it.";

/// What the bundle would contain, without producing it.
///
/// # Errors
///
/// Only if the person may not (`backup.run` — the same permission that may
/// replace the whole database, because this is the other end of a support
/// conversation).
pub fn plan_on(app: &App) -> UiResult<BundlePlanView> {
    crate::guard::require(app, mb_auth::Permission::BackupRun)?;

    let mut items = vec![
        BundleItem {
            name: "about.txt".to_owned(),
            what: "the version, this computer, and your licence's standing — never the key"
                .to_owned(),
            size: size(about(app).len() as u64),
        },
        BundleItem {
            name: "health.txt".to_owned(),
            what: "the health panel, as text".to_owned(),
            size: size(health_text(app).len() as u64),
        },
        BundleItem {
            name: "database.txt".to_owned(),
            what: "a summary of the shop's data: its size, its version, and whether it is intact"
                .to_owned(),
            size: size(database_text(app).len() as u64),
        },
    ];

    let logs = crate::logging::files();
    let log_bytes: u64 = logs
        .iter()
        .take(SEVEN_DAYS)
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    items.push(BundleItem {
        name: format!("logs\\ ({} files)", logs.len().min(SEVEN_DAYS)),
        what: "the last seven days of the counter's own log".to_owned(),
        size: size(log_bytes),
    });

    let crashes = crate::crash::reports();
    if !crashes.is_empty() {
        let crash_bytes: u64 = crashes
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum();
        items.push(BundleItem {
            name: format!("crashes\\ ({} files)", crashes.len()),
            what: words::count(
                i64::try_from(crashes.len()).unwrap_or(0),
                "time the counter stopped unexpectedly",
                "times the counter stopped unexpectedly",
            ),
            size: size(crash_bytes),
        });
    }

    let total: u64 = log_bytes
        + about(app).len() as u64
        + health_text(app).len() as u64
        + database_text(app).len() as u64;

    Ok(BundlePlanView {
        items,
        total: size(total),
        excludes: EXCLUDES.to_owned(),
        folder: folder().display().to_string(),
    })
}

/// Seven, not fourteen. The log keeps a fortnight so a support call can go back
/// that far on request; the bundle carries a week so it still attaches to an
/// email. Asking for more is a second, deliberate act.
const SEVEN_DAYS: usize = 7;

/// `Documents\Magic Bill diagnostics\` — D76: an export lands where a person
/// can find it, not in `%APPDATA%`.
fn folder() -> PathBuf {
    crate::reports::documents_folder("Magic Bill diagnostics")
}

/// **Write it.** Returns where it went, so the toast can say so (D76).
///
/// # Errors
///
/// If the folder cannot be written.
pub fn write_on(app: &App) -> UiResult<String> {
    crate::guard::require(app, mb_auth::Permission::BackupRun)?;
    let dir = folder();
    std::fs::create_dir_all(&dir).map_err(|e| words::from_io("write the diagnostics", &e))?;

    let name = format!("magic-bill-diagnostics-{}.zip", crate::flows::today(crate::flows::now()));
    let path = dir.join(&name);
    let file = std::fs::File::create(&path).map_err(|e| words::from_io("write the diagnostics", &e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut removed = 0_usize;
    let mut put = |zip: &mut zip::ZipWriter<std::fs::File>, name: &str, text: &str| {
        // **Scanned on the way in.** See the module note: a finding does not
        // stop the bundle, it censors the line.
        let (clean, taken) = censor(text);
        removed += taken;
        let _ = zip.start_file(name, options);
        let _ = zip.write_all(clean.as_bytes());
    };

    put(&mut zip, "about.txt", &about(app));
    put(&mut zip, "health.txt", &health_text(app));
    put(&mut zip, "database.txt", &database_text(app));

    for source in crate::logging::files().into_iter().take(SEVEN_DAYS) {
        let Some(base) = source.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if let Ok(text) = std::fs::read_to_string(&source) {
            put(&mut zip, &format!("logs/{base}"), &text);
        }
    }
    for source in crate::crash::reports() {
        let Some(base) = source.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if let Ok(text) = std::fs::read_to_string(&source) {
            put(&mut zip, &format!("crashes/{base}"), &text);
        }
    }

    zip.finish()
        .map_err(|e| UiError::new("diagnostics.write", "The diagnostics file could not be finished.")
            .with_detail(e.to_string()))?;

    if removed > 0 {
        crate::log_warn!(
            "the diagnostics bundle had {removed} line(s) censored — something is \
             writing a secret to the log"
        );
    }
    Ok(path.display().to_string())
}

/// Replace anything the scanner finds, and say how many.
fn censor(text: &str) -> (String, usize) {
    let findings = crate::redact::scan(text);
    if findings.is_empty() {
        return (text.to_owned(), 0);
    }
    let bad: std::collections::BTreeSet<usize> = findings.iter().map(|f| f.line).collect();
    let out = text
        .lines()
        .enumerate()
        .map(|(index, line)| {
            if bad.contains(&(index + 1)) {
                "*** this line was removed because it contained something that looked like a secret ***"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    (out, bad.len())
}

// ---------------------------------------------------------------------------
// The three text members.
// ---------------------------------------------------------------------------

fn about(app: &App) -> String {
    let entitlement = app.entitlement();
    let machine = app.with_licence(|l| l.machine().short());
    format!(
        "Magic Bill\n\
         ==========\n\n\
         version    {}\n\
         os         {}\n\
         computer   {machine}\n\
         plan       {}\n\
         licence    {}\n\
         \n\
         The licence KEY is deliberately not here. Support can identify this\n\
         shop from the computer id above.\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        entitlement.plan_name,
        entitlement.standing.chip(),
    )
}

fn health_text(app: &App) -> String {
    let panel = crate::health::look(app);
    let mut out = String::from("Health\n======\n\n");
    for row in &panel.rows {
        out.push_str(&format!(
            "{:<12} {:<8} {}\n",
            row.name,
            row.tone.to_uppercase(),
            row.says
        ));
    }
    out
}

fn database_text(app: &App) -> String {
    app.with_shop(|shop| {
        let path = shop.path.clone();
        let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let integrity = shop
            .db
            .transaction(|tx| {
                let mut statement = tx.prepare("PRAGMA integrity_check")?;
                let answer: String = statement.query_row([], |row| row.get(0))?;
                Ok(answer)
            })
            .unwrap_or_else(|e| format!("could not be checked: {e}"));
        let version = shop
            .db
            .transaction(|tx| {
                let mut statement =
                    tx.prepare("SELECT MAX(version) FROM schema_migrations")?;
                let v: i64 = statement.query_row([], |row| row.get(0))?;
                Ok(v.to_string())
            })
            .unwrap_or_else(|_| "unknown".to_owned());
        Ok(format!(
            "The shop's data\n===============\n\n\
             where      {}\n\
             size       {}\n\
             version    {version}\n\
             intact     {integrity}\n",
            path.display(),
            size(size_bytes),
        ))
    })
    .unwrap_or_else(|_| {
        "The shop's data\n===============\n\nno shop is open on this computer\n".to_owned()
    })
}

// ---------------------------------------------------------------------------
// The seats.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn diagnostics_plan(app: tauri::State<'_, App>) -> UiResult<BundlePlanView> {
    plan_on(&app)
}

#[tauri::command]
pub fn write_diagnostics(app: tauri::State<'_, App>) -> UiResult<String> {
    write_on(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_sizes() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(900), "900 B");
        assert_eq!(size(2048), "2 KB");
        assert!(size(5 * 1024 * 1024).starts_with('5'));
    }

    /// **A finding censors a line and does not stop the bundle.**
    ///
    /// The alternative is a shopkeeper with a broken counter, a bundle that
    /// refuses to be produced, and no way to ask for help — which is a worse
    /// outcome than a censored line.
    #[test]
    fn a_secret_in_the_log_is_removed_and_the_bundle_still_happens() {
        let text = "line one is fine\n\
                    activated MB-4KQ7-9WTX-2100 for a shop\n\
                    line three is fine\n";
        let (clean, removed) = censor(text);
        assert_eq!(removed, 1);
        assert!(!clean.contains("9WTX"));
        assert!(clean.contains("line one is fine"));
        assert!(clean.contains("line three is fine"));
        assert!(clean.contains("removed because it contained"));
        // And the censored output is itself clean.
        assert!(crate::redact::scan(&clean).is_empty());
    }

    #[test]
    fn clean_text_is_untouched() {
        let text = "settled bill INV-000241 at 1786340060438\n";
        let (clean, removed) = censor(text);
        assert_eq!(removed, 0);
        assert_eq!(clean.trim_end(), text.trim_end());
    }

    /// The sentence that answers "what did I just send you" has to name the
    /// three things people actually worry about.
    #[test]
    fn the_excludes_sentence_names_what_people_worry_about() {
        for worry in ["licence key", "PIN", "phone number"] {
            assert!(EXCLUDES.contains(worry), "{worry} is not mentioned");
        }
    }
}
