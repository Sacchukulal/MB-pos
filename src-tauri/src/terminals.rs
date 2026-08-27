//! The tills.

use std::path::{Path, PathBuf};

use mb_auth::Permission;
use mb_auth::audit::{AuditEntry, action};
use mb_db::repo::terminals::Terminal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// Who this machine is.

/// This till's own identity, kept beside the config and never in the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    pub terminal_id: String,
    /// The master this till joined, when it is a secondary.
    #[serde(default)]
    pub master: Option<Link>,
}

/// How to reach the master, and the proof that we are allowed to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub base: String,
    /// The pin. Kept rather than re-fetched, so a stranger answering on that address tomorrow
    /// is refused without anybody being asked again.
    pub certificate_pem: String,
    pub device_id: String,
    pub secret: String,
}

fn identity_path(config_dir: &Path) -> PathBuf {
    config_dir.join("terminal.json")
}

/// Read this machine's identity, or make one.
pub fn me(config_dir: &Path) -> Me {
    let path = identity_path(config_dir);
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(me) = serde_json::from_str::<Me>(&text)
    {
        return me;
    }
    // The first run of a shop that has never had a second till: this machine is
    // `terminal_default`, which is the row migration 0001 seeded and the one every existing
    // bill already points at.
    let me = Me {
        terminal_id: crate::billing::TERMINAL.to_owned(),
        master: None,
    };
    let _ = write_me(config_dir, &me);
    me
}

fn write_me(config_dir: &Path, me: &Me) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    std::fs::write(
        identity_path(config_dir),
        serde_json::to_string_pretty(me).unwrap_or_default(),
    )
}

// What the screen sees.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TerminalView {
    pub id: String,
    pub name: String,
    /// The series this till issues under — "A", "B".
    pub prefix: String,
    pub is_master: bool,
    /// True for the machine this screen is running on.
    pub is_this_one: bool,
    /// "seen 2 minutes ago", "never" — written in Rust.
    pub last_seen: String,
    /// "Bills print as A/0001" — the whole sentence, so no screen assembles it.
    pub numbers_say: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TillsView {
    pub tills: Vec<TerminalView>,
    /// This machine's row.
    pub me: String,
    pub is_master: bool,
    pub away_says: String,
    /// "3 bills waiting to reach the main till.".
    pub waiting_says: String,
    pub waiting: u32,
    /// How many tills the plan allows, and how many there are.
    pub allowed: u32,
    pub limit_says: String,
    pub may_manage: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TerminalEdit {
    pub id: String,
    pub name: String,
    pub prefix: String,
}

pub fn tills_on(app: &App) -> UiResult<TillsView> {
    let who = guard::require(app, Permission::ReportsView)?;
    let at = now();
    // The id comes from `App`, not from the file.
    let mine = Me {
        terminal_id: app.terminal_id().to_owned(),
        ..me(&crate::config::AppConfig::directory())
    };
    let waiting = crate::forwarding::waiting_on(app)
        .map(|w| w.len())
        .unwrap_or(0);
    let allowed = app.entitlement().limits.terminals;
    let stood_down = stood_down_says(app).unwrap_or_default();

    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let rows = repos.terminals().all(OUTLET)?;
                let master = rows.iter().find(|t| t.is_master).cloned();
                let is_master = master.as_ref().is_some_and(|m| m.id == mine.terminal_id);

                let tills = rows
                    .iter()
                    .map(|t| TerminalView {
                        id: t.id.clone(),
                        name: t.name.clone(),
                        prefix: t.series_prefix.clone(),
                        is_master: t.is_master,
                        is_this_one: t.id == mine.terminal_id,
                        // The machine drawing this screen is here by definition, and saying
                        // "never" about it is a lie the very first run tells: the heartbeat
                        // below runs AFTER this read, so a till's own row always said never
                        // until somebody opened the screen twice.
                        last_seen: if t.id == mine.terminal_id {
                            "just now".to_owned()
                        } else {
                            match t.last_seen_at {
                                Some(seen) => words::when(seen),
                                None => "never".to_owned(),
                            }
                        },
                        numbers_say: numbers_say(t),
                    })
                    .collect();

                Ok(TillsView {
                    tills,
                    me: mine.terminal_id.clone(),
                    is_master,
                    away_says: if !stood_down.is_empty() {
                        // The louder of the two: a till that does not know it has been replaced
                        // is a till whose book is stranded, and that has to be read before "the
                        // main till is off".
                        stood_down.clone()
                    } else if is_master || waiting == 0 {
                        String::new()
                    } else {
                        "The main till is off. This till can take counter and parcel \
                         bills — table service needs the main till."
                            .to_owned()
                    },
                    waiting_says: crate::forwarding::waiting_says(waiting),
                    waiting: crate::ipc::count(waiting as i64),
                    allowed,
                    limit_says: limit_says(rows.len(), allowed),
                    may_manage: who.must(Permission::SettingsStore).is_ok(),
                })
            })
            .map_err(|e| words::from_db(&e))
    })
    .inspect(|view| {
        // Touch our own row, so a person looking at the roster can see which tills are alive.
        let _ = app.with_shop(|shop| {
            shop.db
                .transaction(|tx| mb_db::Repos::new(tx).terminals().seen(OUTLET, &view.me, at))
                .map_err(|e| words::from_db(&e))
        });
    })
}

/// "Bills print as A/0001." — the sentence, composed here (§6).
fn numbers_say(terminal: &Terminal) -> String {
    if terminal.series_prefix.is_empty() {
        return "Bills print as 0001, with nothing in front.".to_owned();
    }
    format!("Bills print as {}0001.", terminal.series_prefix)
}

/// The licence counts tills at the door.
fn limit_says(have: usize, allowed: u32) -> String {
    let have = u32::try_from(have).unwrap_or(u32::MAX);
    let plan = words::count(i64::from(allowed), "till", "tills");
    if have < allowed {
        return format!("Your plan allows {plan}. You are using {have}.");
    }
    if have == allowed {
        return format!(
            "Your plan allows {plan}, and all of them are in use. Another till \
             can only join on a bigger plan — the ones you have keep billing \
             either way."
        );
    }
    format!(
        "You are using {}, and your plan allows {plan}. Every one of them keeps \
         billing — nothing stops. A bigger plan is what lets you add another.",
        words::count(i64::from(have), "till", "tills")
    )
}

/// Rename a till or change its series prefix.
pub fn save_till_on(app: &App, edit: TerminalEdit) -> UiResult<TillsView> {
    let who = guard::require(app, Permission::SettingsStore)?;
    let at = now();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| -> Result<Result<(), UiError>, mb_db::DbError> {
                let repos = mb_db::Repos::new(tx);
                let Some(existing) = repos.terminals().find(OUTLET, &edit.id)? else {
                    return Ok(Err(UiError::new(
                        "till.missing",
                        "That till is not on file.",
                    )));
                };
                let updated = Terminal {
                    name: edit.name.trim().to_owned(),
                    series_prefix: edit.prefix.trim().to_owned(),
                    ..existing
                };
                // A refusal a person must act on is a VALUE, not a `DbError` that
                // `words::from_db` would rewrite into "the shop's data could not be read".
                if let Err(refusal) = repos.terminals().save(OUTLET, &updated, at) {
                    return Ok(Err(UiError::new("till.prefix", refusal.to_string())));
                }
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        today(at),
                        Some(who.staff_id.clone()),
                        action::TERMINAL_SAVED,
                        "terminal",
                    )
                    .about(edit.id.clone())
                    .with_after(serde_json::json!({
                        "name": updated.name,
                        "prefix": updated.series_prefix,
                    })),
                )?;
                Ok(Ok(()))
            })
            .map_err(|e| words::from_db(&e))
    })??;
    tills_on(app)
}

/// Move the master.
pub fn make_master_on(app: &App, id: String) -> UiResult<TillsView> {
    let who = guard::require(app, Permission::SettingsStore)?;
    let at = now();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.terminals().make_master(OUTLET, &id, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        today(at),
                        Some(who.staff_id.clone()),
                        action::MASTER_MOVED,
                        "terminal",
                    )
                    .about(id.clone())
                    .with_after(serde_json::json!({ "master": id })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;
    crate::log_info!("the main till is now {id}");
    tills_on(app)
}

/// Join a shop that already has a till.
pub fn join_on(
    app: &App,
    address: String,
    fingerprint: String,
    token: String,
    name: String,
    prefix: String,
) -> UiResult<TillsView> {
    let who = guard::require(app, Permission::SettingsStore)?;
    let at = now();
    if prefix.trim().is_empty() {
        return Err(UiError::new(
            "join.prefix",
            "Give this till its own short prefix — A, B, C — to go in front of \
             its numbers. Without it two tills would print the same bill number.",
        ));
    }

    // The client is async and this command is not.
    let master = mb_lan::Master::meet_blocking(&address, &fingerprint)
        .map_err(|e| UiError::new("join.failed", e.to_string()))?;
    // Two minutes, because a person has to walk to the other counter, read the name on its
    // screen and press Allow.
    let credential = master
        .join_blocking(&token, &name, std::time::Duration::from_secs(120))
        .map_err(|e| UiError::new("join.failed", e.to_string()))?;

    // A new identity for this machine.
    let id = crate::newid::fresh_at("term", at);
    let config_dir = crate::config::AppConfig::directory();
    let mine = Me {
        terminal_id: id.clone(),
        master: Some(Link {
            base: address.trim_end_matches('/').to_owned(),
            // The certificate the fingerprint proved, not the one typed and not one fetched
            // again afterwards — a second fetch is a second chance for a stranger to answer.
            certificate_pem: master.certificate_pem().to_owned(),
            device_id: credential.device_id,
            secret: credential.secret,
        }),
    };
    write_me(&config_dir, &mine).map_err(|e| {
        UiError::new(
            "join.write",
            "This till joined but could not remember it. Check the disk is not full.",
        )
        .with_detail(e.to_string())
    })?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| -> Result<Result<(), UiError>, mb_db::DbError> {
                let repos = mb_db::Repos::new(tx);
                let mut row = Terminal::new(id.clone(), name.trim(), at);
                row.series_prefix = prefix.trim().to_owned();
                if let Err(refusal) = repos.terminals().save(OUTLET, &row, at) {
                    return Ok(Err(UiError::new("join.prefix", refusal.to_string())));
                }
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        today(at),
                        Some(who.staff_id.clone()),
                        action::TERMINAL_JOINED,
                        "terminal",
                    )
                    .about(id.clone())
                    .with_after(serde_json::json!({ "name": name, "prefix": prefix })),
                )?;
                Ok(Ok(()))
            })
            .map_err(|e| words::from_db(&e))
    })??;

    crate::log_info!("this till joined the shop as {id}");
    tills_on(app)
}

/// Send what is waiting, now.
pub fn send_now_on(app: &App) -> UiResult<TillsView> {
    guard::require(app, Permission::BillCreate)?;
    let mine = me(&crate::config::AppConfig::directory());
    let Some(link) = mine.master else {
        return Err(UiError::new(
            "forward.master",
            "This is the main till. Its bills are already here.",
        ));
    };
    let master = mb_lan::Master::pinned(&link.base, &link.certificate_pem)
        .map_err(|e| UiError::new("forward.pin", e.to_string()))?
        .as_device(mb_lan::Credential {
            device_id: link.device_id,
            secret: link.secret,
        });
    let sent = crate::forwarding::send_once(app, &master)?;
    crate::log_info!("{sent} bills went across to the main till");
    tills_on(app)
}

/// Stand down if somebody else has been made master since.
pub fn stood_down_says(app: &App) -> UiResult<String> {
    let mine = me(&crate::config::AppConfig::directory());
    let id = app.terminal_id().to_owned();
    let master = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| mb_db::Repos::new(tx).terminals().master(OUTLET))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(master) = master else {
        return Ok(String::new()); // A shop with one till and no roles.
    };
    if master.id == id || mine.master.is_some() {
        // Either this IS the master, or it already knows it is a secondary and holds the
        // credential to reach one.
        return Ok(String::new());
    }
    Ok(format!(
        "{} is the main till now. This one keeps billing and keeps its own \
         numbers, but its bills stay here until somebody joins it to {} again \
         on the Tills screen.",
        master.name, master.name
    ))
}

/// Say it once at start-up, into the log, so support can see it.
pub fn check_the_master_at_startup(app: &App) {
    match stood_down_says(app) {
        Ok(says) if !says.is_empty() => crate::log_warn!("{says}"),
        Ok(_) => {}
        // A shop that will not answer is a shop that has not opened yet.
        Err(e) => crate::log_warn!("the tills could not be read at start-up: {}", e.message),
    }
}

// The commands.

#[tauri::command]
pub fn tills(app: tauri::State<'_, App>) -> UiResult<TillsView> {
    tills_on(&app)
}

#[tauri::command]
pub fn save_till(app: tauri::State<'_, App>, edit: TerminalEdit) -> UiResult<TillsView> {
    save_till_on(&app, edit)
}

#[tauri::command]
pub fn make_master(app: tauri::State<'_, App>, id: String) -> UiResult<TillsView> {
    make_master_on(&app, id)
}

/// Joining waits for a person to press Allow, so it is `async` — Tauri runs it off the UI
/// thread and the screen keeps painting its spinner.
#[tauri::command]
pub async fn join_master(
    app: tauri::State<'_, App>,
    address: String,
    fingerprint: String,
    token: String,
    name: String,
    prefix: String,
) -> UiResult<TillsView> {
    join_on(&app, address, fingerprint, token, name, prefix)
}

#[tauri::command]
pub async fn send_waiting_bills(app: tauri::State<'_, App>) -> UiResult<TillsView> {
    send_now_on(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_licence_sentence_never_threatens_a_till_that_is_already_billing() {
        let room = limit_says(1, 3);
        assert!(room.contains("3 tills"), "{room}");
        assert!(room.contains("using 1"), "{room}");

        let full = limit_says(3, 3);
        assert!(full.contains("keep billing"), "{full}");

        // And the case a real shop hits: OVER the limit, everything still billing.
        let over = limit_says(2, 1);
        assert!(over.starts_with("You are using 2 tills"), "{over}");
        assert!(over.contains("allows 1 till"), "{over}");
        assert!(over.contains("nothing stops"), "{over}");
    }

    #[test]
    fn a_till_says_what_its_numbers_look_like() {
        let at = mb_core::Timestamp::from_millis(0);
        let mut till = Terminal::new("t1", "Counter", at);
        assert!(numbers_say(&till).contains("nothing in front"));
        till.series_prefix = "A/".to_owned();
        assert_eq!(numbers_say(&till), "Bills print as A/0001.");
    }
}
