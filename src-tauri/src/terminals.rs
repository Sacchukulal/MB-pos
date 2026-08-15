//! **The tills** — P27, scope 11.1, 11.2 and 11.3. Bodies over `&App` (D46).
//!
//! > Audit **E5**: *"One counter per shop. Full stop."*
//!
//! # What this file decides, and what it deliberately does not
//!
//! It decides **who this machine is** (its terminal row, its series prefix) and
//! **who the master is** (D139 — a person's choice, never an election). It does
//! not decide a single number: D135 gave every till its own series, so the
//! billing path never asks anything here, and B5 is untouched by construction.
//!
//! # Where the identity lives, and why not in the database
//!
//! Beside the config, exactly as P19 keeps the TLS key and P21 keeps the licence
//! (D79, D85). **A backup is restored onto another machine** (D27), and a
//! restore that resurrected the old machine's terminal id would give a shop two
//! tills claiming to be one — which is the collision this session exists to
//! prevent, arriving through the back door.

use std::path::{Path, PathBuf};

use mb_auth::audit::{AuditEntry, action};
use mb_auth::Permission;
use mb_db::repo::terminals::Terminal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// ---------------------------------------------------------------------------
// Who this machine is.
// ---------------------------------------------------------------------------

/// This till's own identity, kept beside the config and never in the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    pub terminal_id: String,
    /// The master this till joined, when it is a secondary. `None` on the
    /// master itself.
    #[serde(default)]
    pub master: Option<Link>,
}

/// How to reach the master, and the proof that we are allowed to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// `https://192.168.0.104:7331`.
    pub base: String,
    /// **The pin** (D80). Kept rather than re-fetched, so a stranger answering
    /// on that address tomorrow is refused without anybody being asked again.
    pub certificate_pem: String,
    pub device_id: String,
    pub secret: String,
}

fn identity_path(config_dir: &Path) -> PathBuf {
    config_dir.join("terminal.json")
}

/// Read this machine's identity, or make one.
///
/// **The id is generated once and never derived from anything that travels.**
/// Not from the machine name (two shops buy the same model), not from the
/// database (a restore would clone it), not from the licence.
pub fn me(config_dir: &Path) -> Me {
    let path = identity_path(config_dir);
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(me) = serde_json::from_str::<Me>(&text)
    {
        return me;
    }
    // The first run of a shop that has never had a second till: this machine is
    // `terminal_default`, which is the row migration 0001 seeded and the one
    // every existing bill already points at. Inventing a new id here would
    // orphan the shop's whole history from its own till.
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

// ---------------------------------------------------------------------------
// What the screen sees.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TerminalView {
    pub id: String,
    pub name: String,
    /// **The series this till issues under** (D135) — "A", "B". Empty only
    /// while a shop has one till.
    pub prefix: String,
    pub is_master: bool,
    /// True for the machine this screen is running on.
    pub is_this_one: bool,
    /// "seen 2 minutes ago", "never" — written in Rust (R8).
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
    /// **D138's sentence**, and it is empty when there is nothing to say.
    pub away_says: String,
    /// "3 bills waiting to reach the main till."
    pub waiting_says: String,
    pub waiting: u32,
    /// How many tills the plan allows, and how many there are (D141).
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

// ---------------------------------------------------------------------------
// Reading.
// ---------------------------------------------------------------------------

pub fn tills_on(app: &App) -> UiResult<TillsView> {
    let who = guard::require(app, Permission::ReportsView)?;
    let at = now();
    // **The id comes from `App`, not from the file.** They are the same value —
    // `App` read the file at start-up — but only one of them can be trusted to
    // still be the id every bill on this machine was written under. Reading the
    // file again here would let a join half an hour ago make this screen
    // disagree with the book.
    let mine = Me {
        terminal_id: app.terminal_id().to_owned(),
        ..me(&crate::config::AppConfig::directory())
    };
    let waiting = crate::forwarding::waiting_on(app).map(|w| w.len()).unwrap_or(0);
    let allowed = app.entitlement().limits.terminals;

    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let rows = repos.terminals().all(OUTLET)?;
                let master = rows.iter().find(|t| t.is_master).cloned();
                let is_master =
                    master.as_ref().is_some_and(|m| m.id == mine.terminal_id);

                let tills = rows
                    .iter()
                    .map(|t| TerminalView {
                        id: t.id.clone(),
                        name: t.name.clone(),
                        prefix: t.series_prefix.clone(),
                        is_master: t.is_master,
                        is_this_one: t.id == mine.terminal_id,
                        last_seen: match t.last_seen_at {
                            Some(seen) => words::when(seen),
                            None => "never".to_owned(),
                        },
                        numbers_say: numbers_say(t),
                    })
                    .collect();

                Ok(TillsView {
                    tills,
                    me: mine.terminal_id.clone(),
                    is_master,
                    // **D138's sentence, and it is said from a fact rather
                    // than from a guess.** Bills waiting IS the master being
                    // away — nothing else leaves them queued — so this needs
                    // no heartbeat, no timeout and no third state that could
                    // disagree with the queue.
                    away_says: if is_master || waiting == 0 {
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
        // Touch our own row, so a person looking at the roster can see which
        // tills are alive. Deliberately outside the read above and deliberately
        // ignored if it fails: **a heartbeat must never sit on the writer
        // connection a bill needs**, and a missing "last seen" is a cosmetic
        // loss where a blocked settle is not.
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

/// **D141 — the licence counts tills at the door.**
fn limit_says(have: usize, allowed: u32) -> String {
    let have = u32::try_from(have).unwrap_or(u32::MAX);
    if have < allowed {
        return format!(
            "Your plan allows {}. You are using {have}.",
            words::count(i64::from(allowed), "till", "tills")
        );
    }
    format!(
        "Your plan allows {}, and all of them are in use. Another till can only \
         join on a bigger plan — the ones you have keep billing either way.",
        words::count(i64::from(allowed), "till", "tills")
    )
}

// ---------------------------------------------------------------------------
// Writing.
// ---------------------------------------------------------------------------

/// Rename a till or change its series prefix.
///
/// **The prefix refusal is the whole of D135's remaining risk**, and it comes
/// back as a sentence naming the till that already has it.
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
                // **D75** — a refusal a person must act on is a VALUE, not a
                // `DbError` that `words::from_db` would rewrite into "the
                // shop's data could not be read".
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

/// **D139 — move the master. A person did this.**
///
/// There is no election, and this is why: automatic failover between two
/// machines on a shop's WiFi is a split-brain generator. The switch reboots,
/// each till decides the other is dead, both become master, and the shop has
/// two floors. The failure that would "protect" against is a machine being off,
/// which a person in the room can already see.
///
/// The old master is **not consulted**, and that is the point — the machine that
/// failed is exactly the one that cannot hand over gracefully. When it comes
/// back it sees a later `master_since` than its own and stands down.
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

/// **Join a shop that already has a till** — P19's pairing, done by a till.
///
/// The person is holding the master's QR: it carries the address and the
/// fingerprint, and the master is showing a single-use token that a person
/// there presses Allow on. All three are needed, and the fingerprint is what
/// makes the address trustworthy (D80).
///
/// # What it writes, and where
///
/// The credential and the pin go **beside the config**, never in the database
/// (D79/D85): a backup is restored onto other machines, and one that carried a
/// terminal's identity would give a shop two tills claiming to be one.
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
        // **D135's one remaining risk, refused at the door.** A till joining a
        // shop that already has one must print under its own letter, or the two
        // series are the same series.
        return Err(UiError::new(
            "join.prefix",
            "Give this till its own short prefix — A, B, C — to go in front of \
             its numbers. Without it two tills would print the same bill number.",
        ));
    }

    // The client is async and this command is not. mb-lan owns the runtime —
    // the same boundary `server::start` drew — so nothing here mentions tokio.
    let master = mb_lan::Master::meet_blocking(&address, &fingerprint)
        .map_err(|e| UiError::new("join.failed", e.to_string()))?;
    // Two minutes, because a person has to walk to the other counter, read the
    // name on its screen and press Allow.
    let credential = master
        .join_blocking(&token, &name, std::time::Duration::from_secs(120))
        .map_err(|e| UiError::new("join.failed", e.to_string()))?;

    // **A new identity for this machine.** Not `terminal_default` — that id
    // belongs to the shop's first till and is on every bill it has ever
    // written.
    let id = format!("term_{}", at.millis());
    let config_dir = crate::config::AppConfig::directory();
    let mine = Me {
        terminal_id: id.clone(),
        master: Some(Link {
            base: address.trim_end_matches('/').to_owned(),
            // **The certificate the fingerprint proved**, not the one typed and
            // not one fetched again afterwards — a second fetch is a second
            // chance for a stranger to answer.
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

/// **Send what is waiting, now.** The button beside the queue, and the same
/// call the background sender makes.
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

/// **Stand down if somebody else has been made master since.**
///
/// Called at start-up. The old master is the machine that failed, so nothing in
/// the handover may require it to be reachable — this is how it finds out, on
/// its own, the next time it opens.
pub fn stand_down_if_replaced(app: &App) -> UiResult<bool> {
    let mine = me(&crate::config::AppConfig::directory());
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(master) = repos.terminals().master(OUTLET)? else {
                    return Ok(false);
                };
                Ok(master.id != mine.terminal_id && mine.master.is_some())
            })
            .map_err(|e| words::from_db(&e))
    })
}

// ---------------------------------------------------------------------------
// The commands (D46 — thin, and every body is above).
// ---------------------------------------------------------------------------

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

/// **Joining waits for a person to press Allow**, so it is `async` — Tauri runs
/// it off the UI thread and the screen keeps painting its spinner.
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

    /// **D141's sentence, both ways round.**
    ///
    /// The refusal has to say what the plan allows and — the half that matters —
    /// that the tills already working keep working. A shop reading "limit
    /// reached" with no second sentence assumes its counter is about to stop.
    #[test]
    fn the_licence_sentence_never_threatens_a_till_that_is_already_billing() {
        let room = limit_says(1, 3);
        assert!(room.contains("3 tills"), "{room}");
        assert!(room.contains("using 1"), "{room}");

        let full = limit_says(3, 3);
        assert!(full.contains("keep billing"), "{full}");
    }

    /// D135's sentence, and the empty case a one-till shop actually has.
    #[test]
    fn a_till_says_what_its_numbers_look_like() {
        let at = mb_core::Timestamp::from_millis(0);
        let mut till = Terminal::new("t1", "Counter", at);
        assert!(numbers_say(&till).contains("nothing in front"));
        till.series_prefix = "A/".to_owned();
        assert_eq!(numbers_say(&till), "Bills print as A/0001.");
    }
}
