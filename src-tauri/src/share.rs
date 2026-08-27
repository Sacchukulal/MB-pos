use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::state::App;
use crate::words::{UiError, UiResult};

/// What was shared, and what the screen must still tell the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ShareView {
    /// The summary itself, so the screen can put it on the clipboard without composing a word
    /// of it.
    pub text: String,
    /// The limit, said out loud — empty for the channels that have none.
    pub caveat: String,
    /// What happened: "WhatsApp is opening", "Copied", "The folder is open".
    pub says: String,
}

/// Where a shared summary is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// The one people actually use.
    Copy,
    WhatsApp,
    Email,
    /// Open `Documents\Magic Bill reports\`, where the PDF already is.
    Folder,
}

/// Build the summary and hand it to the operating system.
pub fn share_report_on(
    app: &App,
    id: String,
    period: crate::reports::PeriodArg,
    channel: Channel,
) -> UiResult<ShareView> {
    let report = crate::reports::report_on(app, id, period)?;
    let text = summarise(&report);
    hand_over(&text, &report.title, channel)
}

/// The report as a message somebody would actually send.
fn summarise(report: &crate::reports::ReportView) -> String {
    const ROWS: usize = 12;
    let mut out = String::new();
    out.push_str(&format!("{}\n{}\n\n", report.title, report.subtitle));

    for row in report.rows.iter().take(ROWS) {
        let label = row.first().cloned().unwrap_or_default();
        let value = row.last().cloned().unwrap_or_default();
        if label.is_empty() && value.is_empty() {
            continue;
        }
        out.push_str(&format!("{label}  {value}\n"));
    }
    if report.rows.len() > ROWS {
        out.push_str(&format!("… and {} more\n", report.rows.len() - ROWS));
    }
    if let Some(totals) = &report.totals {
        out.push_str(&format!(
            "\n{}  {}\n",
            totals.first().cloned().unwrap_or_default(),
            totals.last().cloned().unwrap_or_default()
        ));
    }
    if let Some(compare) = &report.compare {
        out.push_str(&format!("\n{}\n", compare.summary));
    }
    out.push_str("\nMagic Bill");
    out
}

fn hand_over(text: &str, title: &str, channel: Channel) -> UiResult<ShareView> {
    match channel {
        // Nothing to launch: the screen puts it on the clipboard.
        Channel::Copy => Ok(ShareView {
            text: text.to_owned(),
            caveat: String::new(),
            says: "Copied. Paste it wherever you like.".to_owned(),
        }),
        Channel::WhatsApp => {
            launch(&format!("whatsapp://send?text={}", escape(text)))?;
            Ok(ShareView {
                text: text.to_owned(),
                caveat: "The summary goes as text. If you want the full sheet, save the PDF \
                         and attach it yourself."
                    .to_owned(),
                says: "WhatsApp is opening. Pick who to send it to.".to_owned(),
            })
        }
        Channel::Email => {
            launch(&format!(
                "mailto:?subject={}&body={}",
                escape(title),
                escape(text)
            ))?;
            Ok(ShareView {
                text: text.to_owned(),
                caveat: "The summary goes in the message. Attach the PDF yourself if you \
                         need the full sheet."
                    .to_owned(),
                says: "Your mail program is opening.".to_owned(),
            })
        }
        Channel::Folder => {
            let folder = crate::reports::export_folder();
            std::fs::create_dir_all(&folder).map_err(|e| {
                UiError::new("share.folder", "That folder could not be opened.")
                    .with_detail(e.to_string())
            })?;
            launch(&folder.display().to_string())?;
            Ok(ShareView {
                text: text.to_owned(),
                caveat: String::new(),
                says: format!("{} is open.", folder.display()),
            })
        }
    }
}

/// Hand a URL or a folder to Windows.
#[cfg(windows)]
fn launch(target: &str) -> UiResult<()> {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW: without it a black console box flashes on the counter every time
    // somebody shares anything.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", target])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            UiError::new(
                "share.launch",
                "Nothing on this computer is set up to open that. Copy the summary \
                 instead and paste it where you want it.",
            )
            .with_detail(e.to_string())
        })
}

#[cfg(not(windows))]
fn launch(_target: &str) -> UiResult<()> {
    Err(UiError::new(
        "share.launch",
        "Sharing opens WhatsApp or your mail program, and this build cannot do that. \
         Copy the summary instead.",
    ))
}

/// Percent-encode, because a report has spaces, newlines, `&` and `₹` in it and every one of
/// them ends a URL early.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[tauri::command]
pub fn share_report(
    app: tauri::State<'_, App>,
    id: String,
    period: crate::reports::PeriodArg,
    channel: Channel,
) -> UiResult<ShareView> {
    share_report_on(&app, id, period, channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_is_short_enough_to_read_on_a_phone() {
        let report = crate::reports::ReportView {
            id: "sales_day".to_owned(),
            title: "Sales by day".to_owned(),
            subtitle: "1 Aug to 9 Aug 2026".to_owned(),
            columns: Vec::new(),
            rows: (0..30)
                .map(|n| vec![format!("Day {n}"), format!("{n}00.00")])
                .collect(),
            totals: Some(vec!["Total".to_owned(), "43,500.00".to_owned()]),
            compare: None,
            notes: Vec::new(),
        };
        let text = summarise(&report);
        assert!(text.contains("Sales by day"));
        assert!(text.contains("Total  43,500.00"));
        // It says what it left out rather than pretending there was no more.
        assert!(text.contains("and 18 more"));
        // Twelve rows, a total, a comparison and the two blank lines between them: about twenty
        // lines, which is a WhatsApp message somebody reads rather than one they scroll past.
        assert!(
            text.lines().count() <= 22,
            "a shared summary is {} lines — it has to fit on a phone",
            text.lines().count()
        );
    }

    #[test]
    fn the_escape_survives_a_rupee_sign_and_an_ampersand() {
        // A URL that ends at the first `&` sends half a report and nobody notices, because the
        // half that arrives looks complete.
        let escaped = escape("₹1,200 cash & card\nnext line");
        assert!(!escaped.contains('&'));
        assert!(!escaped.contains('\n'));
        assert!(!escaped.contains(' '));
        assert!(escaped.contains("%26"), "the ampersand is encoded");
    }
}
