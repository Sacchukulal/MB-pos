//! The cloud copy: the outbox goes up, the people list and the notices come down, and a new
//! computer gets the whole shop back.
//!
//! One thread, its own connection, off the billing path. It is woken when `sync_outbox` gains
//! rows, pushes at most once a minute, and backs off when the cloud is not there. Nothing here
//! can stop a bill: a push that fails leaves the rows where they are and says so in Health.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mb_auth::audit::{AuditEntry, action};
use mb_core::Timestamp;
use mb_db::Db;
use mb_db::repo::notices::CloudNotice;
use mb_db::repo::wire::{self, Restored};
use mb_license::cloud::DeviceLogin;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cloud::{Link, LinkError};
use crate::flows::{now, today};
use crate::state::{App, OUTLET, Pushed};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

/// Rows in one push. The cloud refuses more.
pub const BATCH: usize = 200;
/// At most one push a minute, however busy the counter.
pub const FLOOR: Duration = Duration::from_secs(60);
/// Health says "behind" from the third failure in a row.
pub const BEHIND_AFTER_FAILURES: u32 = 3;
/// A row the sender could not even read is given this many tries before it is put aside.
const READ_TRIES: i64 = 10;
/// How many rows one restore page asks for.
const PAGE: usize = 1000;

// What is written beside the licence.

/// `cloud.json`: where the sender has got to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncFile {
    /// The pull cursor the cloud handed back last; sent as `since`.
    pub cursor: Option<i64>,
    pub last_push_at: Option<i64>,
    pub last_pull_at: Option<i64>,
    /// Whole-batch failures in a row.
    pub failures: u32,
    /// The last thing that went wrong, as a sentence.
    pub last_error: Option<String>,
    /// The last row the cloud refused: "bills 0042 — a bill is never deleted; void it".
    pub last_refusal: Option<String>,
    /// The cloud said the login is dead. The sender stops until the licence changes.
    pub stopped: Option<String>,
    /// Not before this, milliseconds.
    pub next_try_at: Option<i64>,
}

impl SyncFile {
    #[must_use]
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("cloud.json")
    }

    #[must_use]
    pub fn load(dir: &Path) -> SyncFile {
        std::fs::read_to_string(SyncFile::path(dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) {
        let path = SyncFile::path(dir);
        let _ = std::fs::create_dir_all(dir);
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let temporary = path.with_extension("json.tmp");
            if std::fs::write(&temporary, text).is_ok() {
                let _ = std::fs::rename(&temporary, &path);
            }
        }
    }

    /// The cloud copy is this far behind: the age of the last push, once failures have piled
    /// up. `None` while things are fine.
    #[must_use]
    pub fn behind_by(&self, at: Timestamp) -> Option<Duration> {
        if self.failures < BEHIND_AFTER_FAILURES {
            return None;
        }
        let since = self.last_push_at.unwrap_or(0);
        Some(Duration::from_millis(
            u64::try_from(at.millis().saturating_sub(since)).unwrap_or(0),
        ))
    }
}

/// 1 min, 5 min, 15 min, 1 h, then hourly, then daily after ten failures in a row.
#[must_use]
pub const fn backoff(failures: u32) -> Duration {
    match failures {
        0 => Duration::ZERO,
        1 => Duration::from_secs(60),
        2 => Duration::from_secs(5 * 60),
        3 => Duration::from_secs(15 * 60),
        4..=9 => Duration::from_secs(3600),
        _ => Duration::from_secs(24 * 3600),
    }
}

/// What one run of the sender did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No shop, no login, nothing pending, or not yet time.
    Nothing,
    Pushed {
        applied: u64,
        refused: u64,
        /// Rows still waiting after this batch.
        pending: u64,
    },
    /// The whole batch stays. Backing off.
    Failed(String),
    /// The login is dead. Stopped until the licence changes.
    Stopped(String),
}

/// What a pull put into the shop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub staff: u32,
    pub pins: u32,
    pub roles: u32,
    pub notices: u32,
    /// The bell's number, after.
    pub unseen: u32,
    pub licence_changed: bool,
}

// The login, kept fresh.

/// Call under the login, refreshing it once when the access token has run out.
fn call(app: &App, name: &str, body: &Value) -> Result<Value, LinkError> {
    let Some(login) = app.device_login() else {
        return Err(LinkError::Dead(
            "This counter has no login to the cloud yet. Enter the licence key on the Account screen."
                .to_owned(),
        ));
    };
    let link = app.link();
    match link.rpc(name, body, &login.access_token) {
        Err(LinkError::Unauthorised) => {}
        other => return other,
    }
    let refreshed = match link.refresh_session(&login.refresh_token) {
        Ok(session) => session,
        Err(LinkError::Dead(why)) => {
            // The refresh token is spent. Drop the pair; the next licence check asks for a new
            // one — that is the one metered road, and it is the daily check anyway.
            app.set_device_login(None);
            app.refresher_wakeup().wake();
            return Err(LinkError::Dead(why));
        }
        Err(other) => return Err(other),
    };
    let fresh = DeviceLogin {
        access_token: refreshed.access_token.clone(),
        refresh_token: refreshed.refresh_token,
        expires_at: refreshed.expires_at,
        ..login
    };
    app.set_device_login(Some(fresh));
    link.rpc(name, body, &refreshed.access_token)
}

// Up.

/// One push, if there is anything to push and it is time. Never inside a settle: it runs on
/// its own thread with its own connection.
pub fn push_once(app: &App) -> Outcome {
    let at = now();
    let status = app.sync_status();
    if let Some(why) = status.stopped {
        return Outcome::Stopped(why);
    }
    if status.next_try_at.is_some_and(|t| at.millis() < t) {
        return Outcome::Nothing;
    }
    if app.device_login().is_none() {
        return Outcome::Nothing;
    }
    let Some(db) = app.shop_db() else {
        return Outcome::Nothing;
    };

    // Read the batch on a reader, so the writer is never held while the cloud is asked.
    let read = db.read_transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        let pending = repos.outbox().pending(BATCH)?;
        let total = repos.outbox().pending_count()?;
        let mut rows = Vec::new();
        let mut unreadable = Vec::new();
        for entry in &pending {
            match repos.wire().read(OUTLET, entry) {
                Ok(wire) => rows.extend(wire.into_iter().map(|w| w.to_json())),
                Err(e) => unreadable.push((entry.id.clone(), entry.attempts, e.to_string())),
            }
        }
        Ok((pending, total, rows, unreadable))
    });
    let (pending, total, rows, unreadable) = match read {
        Ok(read) => read,
        Err(e) => {
            log_warn!("the outbox could not be read for the cloud: {e}");
            return Outcome::Nothing;
        }
    };
    if !unreadable.is_empty() {
        // A row this build cannot shape is noted, tried again, and put aside after ten goes —
        // it must not stand in front of every bill behind it.
        let _ = db.transaction(|tx| {
            let outbox = mb_db::Repos::new(tx).outbox();
            for (id, attempts, why) in &unreadable {
                outbox.record_failure(id, why)?;
                if *attempts + 1 >= READ_TRIES {
                    log_warn!("outbox row {id} could not be read {READ_TRIES} times and is put aside: {why}");
                    outbox.mark_synced(&[id.as_str()], at)?;
                }
            }
            Ok(())
        });
    }
    if pending.is_empty() {
        return Outcome::Nothing;
    }
    let unreadable_ids: Vec<&str> = unreadable.iter().map(|(id, _, _)| id.as_str()).collect();
    let batch: Vec<&mb_db::repo::OutboxRow> = pending
        .iter()
        .filter(|e| !unreadable_ids.contains(&e.id.as_str()))
        .collect();
    if batch.is_empty() {
        return Outcome::Nothing;
    }
    let still_pending = u64::try_from(total).unwrap_or(0).saturating_sub(u64::try_from(batch.len()).unwrap_or(0));

    let body = json!({
        "rows": rows,
        "since": status.cursor,
        "pending": still_pending,
        "app_version": env!("CARGO_PKG_VERSION"),
    });
    let reply = match call(app, "mb_push", &body) {
        Ok(reply) => reply,
        Err(LinkError::Dead(why)) => {
            log_warn!("the cloud copy has stopped: {why}");
            app.update_sync(|s| {
                s.stopped = Some(why.clone());
                s.last_error = Some(why.clone());
            });
            return Outcome::Stopped(why);
        }
        Err(e) => {
            let sentence = words::from_link(&e).message;
            let failures = status.failures.saturating_add(1);
            let wait = backoff(failures);
            log_warn!("the push failed ({failures} in a row): {e}; next try in {}s", wait.as_secs());
            app.update_sync(|s| {
                s.failures = failures;
                s.last_error = Some(sentence.clone());
                s.next_try_at = Some(at.millis().saturating_add(i64::try_from(wait.as_millis()).unwrap_or(0)));
            });
            return Outcome::Failed(sentence);
        }
    };

    // Applied and refused rows are both done; a refused row is wrong, not late.
    let applied = reply.get("applied").and_then(Value::as_u64).unwrap_or(0);
    let refused: Vec<(String, String, String)> = reply
        .get("refused")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|r| {
                    (
                        r.get("table").and_then(Value::as_str).unwrap_or("").to_owned(),
                        r.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
                        r.get("reason").and_then(Value::as_str).unwrap_or("refused").to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let ids: Vec<&str> = batch.iter().map(|e| e.id.as_str()).collect();
    let marked = db.transaction(|tx| {
        let outbox = mb_db::Repos::new(tx).outbox();
        for (table, id, reason) in &refused {
            // The wire says `bills` for what the outbox calls `orders`.
            let table = if table == "bills" { "orders" } else { table.as_str() };
            outbox.record_failure(&mb_db::repo::OutboxRepo::entry_id(table, id), reason)?;
        }
        outbox.mark_synced(&ids, at)
    });
    if let Err(e) = marked {
        log_warn!("the pushed rows could not be marked: {e}");
    }
    let last_refusal = refused
        .last()
        .map(|(table, id, reason)| format!("{table} {id} — {reason}"));
    for (table, id, reason) in &refused {
        log_warn!("the cloud refused {table} {id}: {reason}");
    }

    let pulled = match reply.get("pull") {
        Some(pull) => apply_pull(app, pull),
        None => Ok(Applied::default()),
    };
    let cursor = reply
        .get("pull")
        .and_then(|p| p.get("cursor"))
        .and_then(Value::as_i64);
    app.update_sync(|s| {
        s.failures = 0;
        s.last_error = None;
        s.stopped = None;
        s.last_push_at = Some(at.millis());
        if let Some(cursor) = cursor {
            s.cursor = Some(cursor);
            s.last_pull_at = Some(at.millis());
        }
        if last_refusal.is_some() {
            s.last_refusal = last_refusal.clone();
        }
        s.next_try_at = Some(at.millis().saturating_add(i64::try_from(FLOOR.as_millis()).unwrap_or(60_000)));
    });
    match pulled {
        Ok(applied) => after_pull(app, &applied),
        Err(e) => log_warn!("what came down with the push could not be applied: {e}"),
    }
    log_info!(
        "pushed {} row(s) to the cloud: {applied} applied, {} refused, {still_pending} still waiting",
        batch.len(),
        refused.len()
    );
    Outcome::Pushed {
        applied,
        refused: u64::try_from(refused.len()).unwrap_or(0),
        pending: still_pending,
    }
}

// Down.

/// Ask the cloud for what changed, without pushing. The Staff screen and the bell.
pub fn pull_once(app: &App) -> UiResult<Applied> {
    if !app.has_shop() {
        return Err(words::no_shop_yet());
    }
    if app.device_login().is_none() {
        return Err(UiError::new(
            "cloud.no_login",
            "This counter is not connected to the cloud yet. Enter the licence key on the Account screen.",
        )
        .quietly());
    }
    let status = app.sync_status();
    let body = json!({ "since": status.cursor, "app_version": env!("CARGO_PKG_VERSION") });
    let pull = match call(app, "mb_pull", &body) {
        Ok(pull) => pull,
        Err(LinkError::Dead(why)) => {
            app.update_sync(|s| {
                s.stopped = Some(why.clone());
                s.last_error = Some(why.clone());
            });
            return Err(UiError::new("cloud.stopped", why));
        }
        Err(e) => return Err(words::from_link(&e)),
    };
    let applied = apply_pull(app, &pull).map_err(|e| words::from_db(&e))?;
    let cursor = pull.get("cursor").and_then(Value::as_i64);
    app.update_sync(|s| {
        if let Some(cursor) = cursor {
            s.cursor = Some(cursor);
        }
        s.last_pull_at = Some(now().millis());
    });
    after_pull(app, &applied);
    Ok(applied)
}

/// What every pull leads to: the bell, and a licence check when the cloud says so.
fn after_pull(app: &App, applied: &Applied) {
    if applied.licence_changed {
        app.refresher_wakeup().wake();
    }
    app.push(Pushed::Notices {
        unseen: applied.unseen,
    });
}

/// The people list and the notices, into the shop. Newest `updated_at` wins per row; every
/// row that lands is written to the history as coming from the phone.
pub fn apply_pull(app: &App, pull: &Value) -> Result<Applied, mb_db::DbError> {
    let Some(db) = app.shop_db() else {
        return Ok(Applied::default());
    };
    let at = now();
    let day = today(at);
    let rows = |key: &str| -> Vec<Value> {
        pull.get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let (staff, secrets, roles, notices) = (
        rows("staff"),
        rows("staff_secrets"),
        rows("roles"),
        rows("notices"),
    );
    let licence_changed = pull
        .get("licence_changed")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut applied = Applied {
        licence_changed,
        ..Applied::default()
    };
    db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        let note = |what: &'static str, id: &str| {
            repos.audit().append(
                OUTLET,
                &AuditEntry::new(at, day, None, what, "staff").about(id.to_owned()),
            )
        };

        // Roles before staff, so a new role's people can point at it.
        for role in &roles {
            let Some(id) = role.get("id").and_then(Value::as_str) else { continue };
            if role.get("deleted").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let empty = json!({});
            let data = role.get("data").unwrap_or(&empty);
            let shaped = wire::cloud_role_from(id, data);
            repos.people().apply_role_from_cloud(OUTLET, &shaped)?;
            note(action::ROLE_FROM_PHONE, id)?;
            applied.roles = applied.roles.saturating_add(1);
        }
        for person in &staff {
            let Some(id) = person.get("id").and_then(Value::as_str) else { continue };
            if person.get("deleted").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let updated_at = Timestamp::from_millis(person.get("updated_at").and_then(Value::as_i64).unwrap_or(0));
            let empty = json!({});
            let data = person.get("data").unwrap_or(&empty);
            let shaped = match wire::cloud_staff_from(id, updated_at, data) {
                Ok(shaped) => shaped,
                Err(e) => {
                    log_warn!("a staff row from the phone could not be read ({id}): {e}");
                    continue;
                }
            };
            if repos.people().apply_staff_from_cloud(OUTLET, &shaped)? {
                note(action::STAFF_FROM_PHONE, id)?;
                applied.staff = applied.staff.saturating_add(1);
            }
        }
        for secret in &secrets {
            let (Some(id), Some(hash)) = (
                secret.get("staff_id").and_then(Value::as_str),
                secret.get("pin_hash").and_then(Value::as_str),
            ) else {
                continue;
            };
            match repos.people().apply_pin_from_cloud(id, hash) {
                Ok(true) => {
                    note(action::STAFF_PIN_FROM_PHONE, id)?;
                    applied.pins = applied.pins.saturating_add(1);
                }
                Ok(false) => {}
                Err(e) => log_warn!("a PIN from the phone could not be applied ({id}): {e}"),
            }
        }
        let shaped: Vec<CloudNotice> = notices
            .iter()
            .filter_map(|n| {
                Some(CloudNotice {
                    id: n.get("id")?.as_str()?.to_owned(),
                    title: n.get("title").and_then(Value::as_str).unwrap_or("").to_owned(),
                    body: n.get("body").and_then(Value::as_str).unwrap_or("").to_owned(),
                    starts_at: Timestamp::from_millis(n.get("starts_at").and_then(Value::as_i64).unwrap_or(0)),
                    ends_at: n.get("ends_at").and_then(Value::as_i64).map(Timestamp::from_millis),
                    updated_at: Timestamp::from_millis(n.get("updated_at").and_then(Value::as_i64).unwrap_or(0)),
                    is_deleted: n.get("deleted").and_then(Value::as_bool).unwrap_or(false),
                })
            })
            .collect();
        applied.notices = u32::try_from(repos.notices().apply(OUTLET, &shaped)?).unwrap_or(u32::MAX);
        applied.unseen = repos.notices().unseen(OUTLET, at)?;
        Ok(())
    })?;
    if applied.staff + applied.pins + applied.roles + applied.notices > 0 {
        log_info!(
            "from the cloud: {} staff, {} PINs, {} roles, {} notices",
            applied.staff,
            applied.pins,
            applied.roles,
            applied.notices
        );
    }
    Ok(applied)
}

// The thread.

/// Start the sender. It sleeps until the outbox gains rows or its backoff runs out.
pub fn start_sender(handle: &tauri::AppHandle) {
    use tauri::Manager as _;
    let handle = handle.clone();
    let spawned = std::thread::Builder::new()
        .name("mb-cloud".to_owned())
        .spawn(move || {
            loop {
                let Some(app) = handle.try_state::<App>() else {
                    return;
                };
                let wakeup = app.sender_wakeup();
                let at = now().millis();
                let wait = match app.sync_status().next_try_at {
                    Some(t) if t > at => Duration::from_millis(u64::try_from(t - at).unwrap_or(0)),
                    _ => Duration::from_secs(3600),
                };
                wakeup.wait_for(wait.max(Duration::from_secs(1)));
                let Some(app) = handle.try_state::<App>() else {
                    return;
                };
                match push_once(&app) {
                    // More waiting: come back after the floor.
                    Outcome::Pushed { pending, .. } if pending > 0 => app.sender_wakeup().wake(),
                    _ => {}
                }
            }
        });
    if let Err(e) = spawned {
        log_warn!("the cloud sender could not be started: {e}");
    }
}

// A new computer.

/// What a restore brought down.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreReport {
    pub bills: u32,
    pub rows: u32,
    pub staff: u32,
    pub roles: u32,
    pub days: u32,
    pub skipped: u32,
}

/// Which line of the report a row belongs on.
#[derive(Debug, Clone, Copy)]
enum Line {
    Bills,
    Rows,
    Staff,
    Roles,
    Days,
}

impl RestoreReport {
    fn count(&mut self, outcome: Restored, line: Line) {
        let slot = match outcome {
            Restored::Skipped => &mut self.skipped,
            Restored::Written => match line {
                Line::Bills => &mut self.bills,
                Line::Rows => &mut self.rows,
                Line::Staff => &mut self.staff,
                Line::Roles => &mut self.roles,
                Line::Days => &mut self.days,
            },
        };
        *slot = slot.saturating_add(1);
    }

    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "{}, {}, {} and {} came down from the cloud.",
            words::count(i64::from(self.bills), "bill", "bills"),
            words::count(i64::from(self.staff), "staff member", "staff members"),
            words::count(i64::from(self.rows), "other row", "other rows"),
            words::count(i64::from(self.days), "day of totals", "days of totals"),
        )
    }
}

/// Every row of one REST path, page by page.
fn read_all(link: &dyn Link, token: &str, path: &str) -> Result<Vec<Value>, LinkError> {
    let mut out = Vec::new();
    let mut from = 0;
    loop {
        let page = link.rest(path, token, from, from + PAGE - 1)?;
        let got = page.rows.len();
        out.extend(page.rows);
        from += got;
        let done = got < PAGE || page.total.is_some_and(|t| from >= t);
        if done {
            return Ok(out);
        }
    }
}

/// `"2026-08-27"` → days since 1970-01-01.
fn day_from_iso(text: &str) -> Option<i64> {
    let mut parts = text.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.get(..2)?.parse().ok()?;
    Some(i64::from(
        mb_core::BusinessDay::from_ymd(year, month, day).days_since_epoch(),
    ))
}

/// `"2026-08-27T10:11:12.123456+00:00"` → milliseconds since the epoch, UTC.
pub fn ms_from_iso(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once(['T', ' '])?;
    let days = day_from_iso(date)?;
    let (clock, zone) = match rest.find(['+', '-', 'Z']) {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "Z"),
    };
    let mut hms = clock.split(':');
    let h: i64 = hms.next()?.parse().ok()?;
    let m: i64 = hms.next()?.parse().ok()?;
    let sec = hms.next().unwrap_or("0");
    let (s, frac) = sec.split_once('.').unwrap_or((sec, ""));
    let s: i64 = s.parse().ok()?;
    let millis: i64 = format!("{:0<3}", frac.get(..3).unwrap_or(frac)).parse().ok()?;
    let offset_min: i64 = match zone {
        "Z" | "" => 0,
        z => {
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let body = &z[1..];
            let (zh, zm) = body.split_once(':').unwrap_or((body, "0"));
            sign * (zh.parse::<i64>().ok()? * 60 + zm.parse::<i64>().ok()?)
        }
    };
    Some(days * 86_400_000 + (h * 3600 + m * 60 + s) * 1000 + millis - offset_min * 60_000)
}

/// A REST row's `date` and `timestamptz` columns back into the counter's integers.
fn counter_shaped(mut row: Value) -> Value {
    if let Value::Object(map) = &mut row {
        for (key, value) in map.iter_mut() {
            let Some(text) = value.as_str() else { continue };
            let converted = if key == "business_day" || key == "joined_on" || key == "left_on" {
                day_from_iso(text).map(Value::from)
            } else if key.ends_with("_at") || key == "at" {
                ms_from_iso(text).map(Value::from)
            } else {
                None
            };
            if let Some(v) = converted {
                *value = v;
            }
        }
    }
    row
}

/// Bring the whole shop down into `db` under the counter's login, before anybody opens it.
/// Everything lands in one transaction; afterwards the outbox is empty, because the cloud
/// already has it all.
pub fn restore_into(app: &App, db: &Db, login: &DeviceLogin) -> UiResult<RestoreReport> {
    let link = app.link();
    let token = login.access_token.as_str();
    let rid = login.restaurant_id.as_str();
    let read = |path: String| read_all(link.as_ref(), token, &path).map_err(|e| words::from_link(&e));

    let boxed = read(format!(
        "shop_rows?restaurant_id=eq.{rid}&select=table_name,row_id,payload,updated_at,deleted_at&order=table_name,row_id"
    ))?;
    let permissions = read(format!("role_permissions?restaurant_id=eq.{rid}&select=role_id,permission_code"))?;
    let roles = read(format!("roles?restaurant_id=eq.{rid}&select=*&order=id"))?;
    let staff = read(format!("staff?restaurant_id=eq.{rid}&select=*&order=id"))?;
    let secrets = read(format!("staff_secrets?restaurant_id=eq.{rid}&select=staff_id,pin_hash"))?;
    let typed: Vec<(String, Vec<Value>)> = [
        "menu_categories",
        "menu_items",
        "customers",
        "customer_ledger",
        "expense_categories",
        "expenses",
        "cash_movements",
    ]
    .into_iter()
    .map(|table| {
        read(format!("{table}?restaurant_id=eq.{rid}&select=*&order=id")).map(|rows| (table.to_owned(), rows))
    })
    .collect::<Result<_, _>>()?;
    let bills = read(format!("bills?restaurant_id=eq.{rid}&select=*&order=business_day,created_at"))?;
    let days = read(format!("day_totals?restaurant_id=eq.{rid}&select=*&order=business_day"))?;

    let at = now();
    let mut report = RestoreReport::default();
    db.transaction(|tx| {
        // Foreign keys are checked at the end, so the order the cloud hands rows back in does
        // not matter.
        tx.execute_batch("PRAGMA defer_foreign_keys = ON")?;
        let repos = mb_db::Repos::new(tx);
        let wire = repos.wire();

        for row in &boxed {
            if !row.get("deleted_at").is_none_or(Value::is_null) {
                continue;
            }
            let (Some(table), Some(payload)) = (
                row.get("table_name").and_then(Value::as_str),
                row.get("payload"),
            ) else {
                continue;
            };
            let written = wire.write_boxed(table, payload)?;
            report.count(if written { Restored::Written } else { Restored::Skipped }, Line::Rows);
        }
        let mut grants: std::collections::BTreeMap<String, Vec<Value>> = std::collections::BTreeMap::new();
        for grant in &permissions {
            if let (Some(role), Some(code)) = (
                grant.get("role_id").and_then(Value::as_str),
                grant.get("permission_code").cloned(),
            ) {
                grants.entry(role.to_owned()).or_default().push(code);
            }
        }
        for role in roles {
            let Some(id) = role.get("id").and_then(Value::as_str).map(str::to_owned) else { continue };
            if !role.get("deleted_at").is_none_or(Value::is_null) {
                continue;
            }
            let mut data = role;
            if let Value::Object(map) = &mut data {
                map.insert("permissions".to_owned(), Value::Array(grants.remove(&id).unwrap_or_default()));
            }
            let outcome = wire.restore_row(OUTLET, "roles", &id, at, &data)?;
            report.count(outcome, Line::Roles);
        }
        for person in staff {
            let Some(id) = person.get("id").and_then(Value::as_str).map(str::to_owned) else { continue };
            if !person.get("deleted_at").is_none_or(Value::is_null) {
                continue;
            }
            let data = counter_shaped(person);
            let updated_at = Timestamp::from_millis(data.get("updated_at").and_then(Value::as_i64).unwrap_or(0));
            let outcome = wire.restore_row(OUTLET, "staff", &id, updated_at, &data)?;
            report.count(outcome, Line::Staff);
        }
        for secret in &secrets {
            if let (Some(id), Some(hash)) = (
                secret.get("staff_id").and_then(Value::as_str),
                secret.get("pin_hash").and_then(Value::as_str),
            ) {
                let _ = repos.people().apply_pin_from_cloud(id, hash)?;
            }
        }
        for (table, rows) in typed {
            for row in rows {
                let Some(id) = row.get("id").and_then(Value::as_str).map(str::to_owned) else { continue };
                if !row.get("deleted_at").is_none_or(Value::is_null) {
                    continue;
                }
                let data = counter_shaped(row);
                let updated_at = Timestamp::from_millis(data.get("updated_at").and_then(Value::as_i64).unwrap_or(0));
                let outcome = wire.restore_row(OUTLET, &table, &id, updated_at, &data)?;
                report.count(outcome, Line::Rows);
            }
        }
        for bill in bills {
            let Some(id) = bill.get("id").and_then(Value::as_str).map(str::to_owned) else { continue };
            let data = counter_shaped(bill);
            let updated_at = Timestamp::from_millis(data.get("updated_at").and_then(Value::as_i64).unwrap_or(0));
            match wire.restore_row(OUTLET, "bills", &id, updated_at, &data) {
                Ok(outcome) => report.count(outcome, Line::Bills),
                // One bill that will not rebuild must not lose the other nine thousand.
                Err(e) => {
                    log_warn!("bill {id} could not be brought back: {e}");
                    report.skipped = report.skipped.saturating_add(1);
                }
            }
        }
        for day in days {
            let data = counter_shaped(day);
            let updated_at = Timestamp::from_millis(data.get("updated_at").and_then(Value::as_i64).unwrap_or(0));
            let id = data.get("business_day").and_then(Value::as_i64).unwrap_or(0).to_string();
            let outcome = wire.restore_row(OUTLET, "day_totals", &id, updated_at, &data)?;
            report.count(outcome, Line::Days);
        }

        // The cloud already has everything that was just written.
        let cleared = repos.outbox().clear_backlog(at)?;
        log_info!("restore: {cleared} outbox row(s) cleared — the cloud already has them");
        repos.audit().append(
            OUTLET,
            &AuditEntry::new(at, today(at), None, action::CLOUD_RESTORED, "shop")
                .about(report.sentence()),
        )?;
        Ok(())
    })
    .map_err(|e| words::from_db(&e))?;
    log_info!("restore from the cloud: {}", report.sentence());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mb_license::Status;

    use super::*;
    use crate::cloud::{Link, LinkError, Page, Session};
    use crate::licence_tests::{a_trading_shop, licence_in};
    use crate::signin_tests::Scratch;

    /// A cloud that answers what the test told it to, and remembers what it was asked.
    #[derive(Debug, Default)]
    struct FakeLink {
        /// What the next calls answer with, in order; the last one stays.
        answers: Mutex<std::collections::VecDeque<Result<Value, LinkError>>>,
        pushes: Mutex<Vec<Value>>,
        tokens_seen: Mutex<Vec<String>>,
        refreshes: Mutex<u32>,
    }

    impl FakeLink {
        fn will_answer(&self, answer: Result<Value, LinkError>) {
            self.answers.lock().unwrap().push_back(answer);
        }

        /// Forget what was queued; answer this from now on.
        fn now_answers(&self, answer: Result<Value, LinkError>) {
            let mut answers = self.answers.lock().unwrap();
            answers.clear();
            answers.push_back(answer);
        }
    }

    impl Link for FakeLink {
        fn rpc(&self, _name: &str, body: &Value, token: &str) -> Result<Value, LinkError> {
            self.pushes.lock().unwrap().push(body.clone());
            self.tokens_seen.lock().unwrap().push(token.to_owned());
            let mut answers = self.answers.lock().unwrap();
            let next = answers.front().cloned().unwrap_or(Err(LinkError::Unreachable));
            if answers.len() > 1 {
                answers.pop_front();
            }
            next
        }
        fn rest(&self, _: &str, _: &str, _: usize, _: usize) -> Result<Page, LinkError> {
            Err(LinkError::Unreachable)
        }
        fn refresh_session(&self, _: &str) -> Result<Session, LinkError> {
            *self.refreshes.lock().unwrap() += 1;
            Ok(Session {
                access_token: "fresh-access".to_owned(),
                refresh_token: "fresh-refresh".to_owned(),
                expires_at: Timestamp::from_millis(1),
            })
        }
        fn download(&self, _: &str, _: &Path) -> Result<String, LinkError> {
            Err(LinkError::Unreachable)
        }
    }

    /// A trading shop with a licence (and so a login) and a fake cloud behind it.
    fn a_connected_shop(scratch: &Scratch, label: &str) -> (App, Arc<FakeLink>) {
        let app = a_trading_shop(scratch, label);
        app.use_licensing(licence_in(scratch, label, Status::Active, 20));
        assert!(app.device_login().is_some(), "the stub hands out a login on activation");
        let link = Arc::new(FakeLink::default());
        app.use_link(Arc::clone(&link) as Arc<dyn Link>);
        app.update_sync(|s| *s = SyncFile::default());
        (app, link)
    }

    fn pending(app: &App) -> i64 {
        app.shop_db()
            .expect("a shop")
            .read_transaction(|tx| mb_db::Repos::new(tx).outbox().pending_count())
            .expect("count")
    }

    fn a_pull(cursor: i64) -> Value {
        json!({ "cursor": cursor, "staff": [], "staff_secrets": [], "roles": [], "notices": [], "licence_changed": false })
    }

    #[test]
    fn a_push_marks_the_batch_done_keeps_the_cursor_and_notes_a_refusal() {
        let scratch = Scratch::new("sync_push");
        let (app, link) = a_connected_shop(&scratch, "push");
        let waiting = pending(&app);
        assert!(waiting > 0, "a built shop queues its rows");

        link.will_answer(Ok(json!({
            "applied": waiting - 1,
            "refused": [{ "table": "items", "id": "itm_tea", "reason": "ZZZ not today" }],
            "pull": a_pull(4242),
        })));
        let outcome = push_once(&app);
        assert!(matches!(outcome, Outcome::Pushed { refused: 1, pending: 0, .. }), "{outcome:?}");
        assert_eq!(pending(&app), 0, "applied and refused rows are both done");
        let status = app.sync_status();
        assert_eq!(status.cursor, Some(4242));
        assert_eq!(status.failures, 0);
        assert!(status.last_push_at.is_some());
        assert!(status.next_try_at.is_some(), "the floor: not again for a minute");
        assert!(status.last_refusal.as_deref().is_some_and(|r| r.contains("ZZZ not today")), "{status:?}");
        // What went up had the protocol's shape.
        let sent = link.pushes.lock().unwrap().first().cloned().expect("one push");
        assert!(sent["rows"].as_array().is_some_and(|r| !r.is_empty()));
        assert_eq!(sent["pending"], 0);
        assert_eq!(sent["app_version"], env!("CARGO_PKG_VERSION"));
        assert!(sent["since"].is_null(), "the first push has no cursor");
        // And the second push, within the floor, does nothing.
        assert_eq!(push_once(&app), Outcome::Nothing);
    }

    #[test]
    fn a_failed_push_leaves_the_rows_and_backs_off_a_dead_login_stops() {
        let scratch = Scratch::new("sync_fail");
        let (app, link) = a_connected_shop(&scratch, "fail");
        let waiting = pending(&app);

        link.will_answer(Err(LinkError::Server("down".to_owned())));
        assert!(matches!(push_once(&app), Outcome::Failed(_)));
        assert_eq!(pending(&app), waiting, "nothing was marked");
        let status = app.sync_status();
        assert_eq!(status.failures, 1);
        assert!(status.next_try_at.is_some());
        assert!(status.last_error.is_some());
        // Not yet time: nothing happens.
        assert_eq!(push_once(&app), Outcome::Nothing);
        // Time passes.
        app.update_sync(|s| s.next_try_at = None);
        link.now_answers(Err(LinkError::Dead("this licence has been revoked".to_owned())));
        assert!(matches!(push_once(&app), Outcome::Stopped(_)));
        assert!(app.sync_status().stopped.is_some());
        assert_eq!(pending(&app), waiting);
        // Stopped stays stopped until the licence changes.
        assert!(matches!(push_once(&app), Outcome::Stopped(_)));
        crate::licensing::after_licence_change(&app);
        assert!(app.sync_status().stopped.is_none());
    }

    #[test]
    fn an_expired_login_is_refreshed_once_and_the_push_tried_again() {
        let scratch = Scratch::new("sync_refresh");
        let (app, link) = a_connected_shop(&scratch, "refresh");
        let before = app.device_login().expect("a login").access_token;

        // The first answer is 401; the fake refresh hands out "fresh-access"; the retry lands.
        link.will_answer(Err(LinkError::Unauthorised));
        link.will_answer(Ok(json!({ "applied": 1, "refused": [], "pull": a_pull(1) })));
        let outcome = push_once(&app);
        assert!(matches!(outcome, Outcome::Pushed { .. }), "{outcome:?}");
        assert_eq!(*link.refreshes.lock().unwrap(), 1);
        let tokens = link.tokens_seen.lock().unwrap().clone();
        assert_eq!(tokens.first().map(String::as_str), Some(before.as_str()));
        assert_eq!(tokens.last().map(String::as_str), Some("fresh-access"));
        assert_eq!(app.device_login().expect("kept").access_token, "fresh-access");
    }

    #[test]
    fn what_comes_down_is_applied_newest_wins_and_written_to_the_history() {
        let scratch = Scratch::new("sync_pull");
        let (app, _link) = a_connected_shop(&scratch, "pull");
        let at = now().millis();
        let pull = json!({
            "cursor": 7,
            "roles": [{ "id": "role_phone", "updated_at": at, "deleted": false,
                        "data": { "name": "From the phone", "is_builtin": false, "permissions": ["bill.create", "no.such.permission"] } }],
            "staff": [{ "id": "staff_phone", "updated_at": at, "deleted": false,
                        "data": { "role_id": "role_phone", "name": "Phone Person", "code": "PP1", "status": "active",
                                  "employment_type": "full_time", "can_login_on_phone": true } }],
            "staff_secrets": [{ "staff_id": "staff_phone", "pin_hash": "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA", "updated_at": at }],
            "notices": [{ "id": "n1", "title": "Hello", "body": "A notice.", "starts_at": at - 1000, "ends_at": null, "updated_at": at, "deleted": false }],
            "licence_changed": true
        });
        let applied = apply_pull(&app, &pull).expect("applied");
        assert_eq!((applied.roles, applied.staff, applied.pins, applied.notices, applied.unseen), (1, 1, 1, 1, 1));
        assert!(applied.licence_changed);

        let people = app
            .shop_db()
            .expect("shop")
            .read_transaction(|tx| mb_db::Repos::new(tx).people().list_staff(OUTLET))
            .expect("staff");
        let person = people.iter().find(|p| p.id.as_str() == "staff_phone").expect("the phone's person");
        assert_eq!(person.name, "Phone Person");
        assert_eq!(person.role_name.as_deref(), Some("From the phone"));
        assert!(person.pin_hash.is_some());
        assert!(person.permissions.has(mb_auth::Permission::BillCreate), "the known permission came through");

        // An older copy changes nothing.
        let older = json!({ "cursor": 8, "staff": [{ "id": "staff_phone", "updated_at": at - 5000, "deleted": false,
                             "data": { "name": "Stale Name", "status": "active", "employment_type": "full_time" } }] });
        let again = apply_pull(&app, &older).expect("applied");
        assert_eq!(again.staff, 0);

        // The history says where it came from.
        let history = app
            .shop_db()
            .expect("shop")
            .read_transaction(|tx| {
                mb_db::Repos::new(tx)
                    .audit()
                    .list(OUTLET, &mb_db::repo::AuditFilter { limit: 50, ..mb_db::repo::AuditFilter::default() })
            })
            .expect("history");
        let actions: Vec<&str> = history.iter().map(|e| e.action.as_str()).collect();
        for expected in [action::ROLE_FROM_PHONE, action::STAFF_FROM_PHONE, action::STAFF_PIN_FROM_PHONE] {
            assert!(actions.contains(&expected), "{expected} is not in the history: {actions:?}");
        }
    }

    #[test]
    fn the_backoff_climbs_and_then_settles_on_daily() {
        assert_eq!(backoff(0), Duration::ZERO);
        assert_eq!(backoff(1), Duration::from_secs(60));
        assert_eq!(backoff(2), Duration::from_secs(300));
        assert_eq!(backoff(3), Duration::from_secs(900));
        assert_eq!(backoff(4), Duration::from_secs(3600));
        assert_eq!(backoff(9), Duration::from_secs(3600));
        assert_eq!(backoff(10), Duration::from_secs(86_400));
        assert_eq!(backoff(500), Duration::from_secs(86_400));
    }

    #[test]
    fn behind_is_said_from_the_third_failure() {
        let mut file = SyncFile {
            last_push_at: Some(1_000),
            failures: 2,
            ..SyncFile::default()
        };
        let at = Timestamp::from_millis(3_601_000);
        assert_eq!(file.behind_by(at), None);
        file.failures = 3;
        assert_eq!(file.behind_by(at), Some(Duration::from_millis(3_600_000)));
    }

    #[test]
    fn the_file_survives_a_round_trip_and_starts_empty() {
        let dir = std::env::temp_dir().join(format!("mb-cloud-json-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(SyncFile::load(&dir), SyncFile::default());
        let file = SyncFile {
            cursor: Some(42),
            failures: 3,
            last_error: Some("x".to_owned()),
            ..SyncFile::default()
        };
        file.save(&dir);
        assert_eq!(SyncFile::load(&dir), file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_clouds_dates_come_back_as_the_counters_integers() {
        assert_eq!(day_from_iso("1970-01-02"), Some(1));
        assert_eq!(day_from_iso("2026-08-27"), Some(20692));
        assert_eq!(ms_from_iso("1970-01-01T00:00:01Z"), Some(1_000));
        assert_eq!(ms_from_iso("1970-01-01T00:00:01.5+00:00"), Some(1_500));
        // +05:30 is behind UTC by five and a half hours.
        assert_eq!(ms_from_iso("1970-01-01T05:30:00+05:30"), Some(0));
        assert_eq!(ms_from_iso("2026-08-27T10:11:12.123456+00:00"), Some(20692 * 86_400_000 + 36_672_123));
        assert_eq!(ms_from_iso("nonsense"), None);
        let shaped = counter_shaped(json!({
            "business_day": "2026-08-27", "created_at": "2026-08-27T00:00:00+00:00", "name": "x", "at": "1970-01-01T00:00:02Z"
        }));
        assert_eq!(shaped["business_day"], 20692);
        assert_eq!(shaped["created_at"], 20692_i64 * 86_400_000);
        assert_eq!(shaped["at"], 2000);
        assert_eq!(shaped["name"], "x");
    }
}
