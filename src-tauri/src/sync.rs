//! The cloud copy: the outbox goes up, the people list and the notices come down, and a new
//! computer gets the whole shop back.
//!
//! One thread, its own connection, off the billing path. It is woken when `sync_outbox` gains
//! rows, pushes at most once a minute, and backs off when the cloud is not there. Nothing here
//! can stop a bill: a push that fails leaves the rows where they are and says so in Health.
//!
//! Two roads up, one sender: a bill of an unsealed day goes by `mb_push` into the cloud's
//! `bills`, the phone's live view; a bill of a sealed day goes only in its day file
//! (`mb_db::archive`), one gzip per business day in the cloud's Storage, the permanent
//! history. The same wake does both, and the daily housekeeping after them.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mb_auth::audit::{AuditEntry, action};
use mb_core::{BusinessDay, Timestamp};
use mb_db::Db;
use mb_db::archive::{self, DayFile};
use mb_db::repo::notices::CloudNotice;
use mb_db::repo::wire::{self, RestoreReport, WireRow};
use mb_license::cloud::DeviceLogin;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cloud::{Link, LinkError};
use crate::flows::{now, today};
use crate::state::{App, OUTLET, Pushed};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

/// Wire rows in one push. The cloud refuses more.
pub const BATCH: usize = 200;
/// Day files in one wake, so one wake stays bounded; the next wake continues.
pub const FILES_PER_WAKE: usize = 30;
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
    /// The business day housekeeping (log retention, checkpoint) last ran on.
    pub housekept_day: Option<i64>,
    /// The whole shop was queued once from its rows — a shop that billed before it had a
    /// cloud, or before the day file existed. Set once per licence.
    pub queued_whole_at: Option<i64>,
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

/// The first business day that is not yet sealed by age: bills from here on travel by push.
#[must_use]
pub fn unsealed_from(today: BusinessDay) -> BusinessDay {
    BusinessDay::from_days_since_epoch(
        today
            .days_since_epoch()
            .saturating_sub(archive::SEAL_AFTER_DAYS)
            .saturating_add(1),
    )
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

/// An RPC under the login, refreshing it once when the access token has run out.
fn call(app: &App, name: &str, body: &Value) -> Result<Value, LinkError> {
    under_login(app, |link, token| link.rpc(name, body, token))
}

/// An Edge Function under the login, the same way.
pub fn edge(app: &App, name: &str, body: &Value) -> Result<Value, LinkError> {
    under_login(app, |link, token| link.edge(name, body, token))
}

/// Run one call under the login, refreshing it once when the access token has run out.
fn under_login<T>(
    app: &App,
    go: impl Fn(&dyn crate::cloud::Link, &str) -> Result<T, LinkError>,
) -> Result<T, LinkError> {
    let Some(login) = app.device_login() else {
        return Err(LinkError::Dead(
            "This counter has no login to the cloud yet. Enter the licence key on the Account screen."
                .to_owned(),
        ));
    };
    let link = app.link();
    match go(link.as_ref(), &login.access_token) {
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
    go(link.as_ref(), &refreshed.access_token)
}

/// What the sender does with an answer that is not per-row: a dead login stops it; anything
/// else backs off. The one place the ladder is climbed, for a push and for a file alike.
fn fell(app: &App, at: Timestamp, e: &LinkError) -> Outcome {
    match e {
        LinkError::Dead(why) => {
            log_warn!("the cloud copy has stopped: {why}");
            app.update_sync(|s| {
                s.stopped = Some(why.clone());
                s.last_error = Some(why.clone());
            });
            Outcome::Stopped(why.clone())
        }
        e => {
            let sentence = words::from_link(e).message;
            let failures = app.sync_status().failures.saturating_add(1);
            let wait = backoff(failures);
            log_warn!("the cloud could not be reached ({failures} in a row): {e}; next try in {}s", wait.as_secs());
            app.update_sync(|s| {
                s.failures = failures;
                s.last_error = Some(sentence.clone());
                s.next_try_at = Some(at.millis().saturating_add(i64::try_from(wait.as_millis()).unwrap_or(0)));
            });
            Outcome::Failed(sentence)
        }
    }
}

/// The sender may go: a login, a shop, not stopped, not before its time.
fn may_go(app: &App, at: Timestamp) -> Result<(), Outcome> {
    let status = app.sync_status();
    if let Some(why) = status.stopped {
        return Err(Outcome::Stopped(why));
    }
    if status.next_try_at.is_some_and(|t| at.millis() < t) {
        return Err(Outcome::Nothing);
    }
    if app.device_login().is_none() || app.shop_db().is_none() {
        return Err(Outcome::Nothing);
    }
    Ok(())
}

// Up.

/// One batch, read on a reader: the entries it covers, their wire rows, and what was routed
/// elsewhere. Stops before `BATCH` wire rows, counting rows and not entries — one day's
/// item totals fan out to a row per item sold.
#[derive(Debug, Default)]
struct Batch {
    /// Entries whose rows are in `rows`, in order.
    entries: Vec<mb_db::repo::OutboxRow>,
    rows: Vec<Value>,
    /// `(wire table, wire id)` → the outbox entry id, so a refusal names its entry exactly.
    sent: Vec<((String, String), String)>,
    /// Entries of a sealed day: the day file carries them, the push does not.
    to_file: Vec<String>,
    unreadable: Vec<(String, i64, String)>,
    total: i64,
}

fn build_batch(repos: &mb_db::Repos<'_>, today: BusinessDay) -> Result<Batch, mb_db::DbError> {
    let mut batch = Batch {
        total: repos.outbox().pending_count()?,
        ..Batch::default()
    };
    for entry in repos.outbox().pending(BATCH)? {
        // The routing rule, in one place: a bill of a sealed day, and its item and category
        // totals, travel in the day file. The day's one totals row still goes up, so the
        // phone's figures follow a late void; it is one small row.
        if entry.table_name != "day_totals"
            && let Some(day) = repos.archive().day_of_entry(&entry)?
            && repos.archive().is_sealed(OUTLET, day, today)?
        {
            batch.to_file.push(entry.id.clone());
            continue;
        }
        let wire = match repos.wire().read(OUTLET, &entry) {
            Ok(wire) => wire,
            Err(e) => {
                batch.unreadable.push((entry.id.clone(), entry.attempts, e.to_string()));
                continue;
            }
        };
        if !batch.rows.is_empty() && batch.rows.len() + wire.len() > BATCH {
            break;
        }
        for row in &wire {
            batch.sent.push(((row.table.clone(), row.id.clone()), entry.id.clone()));
        }
        batch.rows.extend(wire.iter().map(WireRow::to_json));
        batch.entries.push(entry);
    }
    Ok(batch)
}

/// One push, if there is anything to push and it is time. Never inside a settle: it runs on
/// its own thread with its own connection.
pub fn push_once(app: &App) -> Outcome {
    let at = now();
    if let Err(outcome) = may_go(app, at) {
        return outcome;
    }
    let Some(db) = app.shop_db() else {
        return Outcome::Nothing;
    };
    let today = today(at);

    // Read the batch on a reader, so the writer is never held while the cloud is asked.
    let batch = match db.read_transaction(|tx| build_batch(&mb_db::Repos::new(tx), today)) {
        Ok(batch) => batch,
        Err(e) => {
            log_warn!("the outbox could not be read for the cloud: {e}");
            return Outcome::Nothing;
        }
    };
    if !batch.to_file.is_empty() || !batch.unreadable.is_empty() {
        let _ = db.transaction(|tx| {
            let outbox = mb_db::Repos::new(tx).outbox();
            // Done: their day's file carries them.
            let ids: Vec<&str> = batch.to_file.iter().map(String::as_str).collect();
            outbox.mark_synced(&ids, at)?;
            // A row this build cannot shape is noted, tried again, and put aside after ten goes —
            // it must not stand in front of every bill behind it.
            for (id, attempts, why) in &batch.unreadable {
                outbox.record_failure(id, why)?;
                if *attempts + 1 >= READ_TRIES {
                    log_warn!("outbox row {id} could not be read {READ_TRIES} times and is put aside: {why}");
                    outbox.mark_synced(&[id.as_str()], at)?;
                }
            }
            Ok(())
        });
    }
    if batch.rows.is_empty() {
        let routed = u64::try_from(batch.to_file.len()).unwrap_or(0);
        return if routed > 0 {
            // Nothing went over the wire, but the outbox moved: come straight back for what is
            // behind it.
            Outcome::Pushed {
                applied: 0,
                refused: 0,
                pending: u64::try_from(batch.total).unwrap_or(0).saturating_sub(routed),
            }
        } else {
            Outcome::Nothing
        };
    }
    let covered = u64::try_from(batch.entries.len() + batch.to_file.len()).unwrap_or(0);
    let still_pending = u64::try_from(batch.total).unwrap_or(0).saturating_sub(covered);

    // One entry can fan out past the cap (a day with 300 items sold): its rows go in as many
    // pushes as they need, and the entry is done when the last of them landed.
    let mut applied = 0_u64;
    let mut refused: Vec<(String, String, String)> = Vec::new();
    let mut pulled = Applied::default();
    let mut cursor = None;
    for (n, chunk) in batch.rows.chunks(BATCH).enumerate() {
        let last = (n + 1) * BATCH >= batch.rows.len();
        let body = json!({
            "rows": chunk,
            "since": app.sync_status().cursor,
            "pending": if last { still_pending } else { still_pending.saturating_add(1) },
            "app_version": env!("CARGO_PKG_VERSION"),
        });
        let reply = match call(app, "mb_push", &body) {
            Ok(reply) => reply,
            Err(e) => return fell(app, at, &e),
        };
        applied = applied.saturating_add(reply.get("applied").and_then(Value::as_u64).unwrap_or(0));
        refused.extend(
            reply
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
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        );
        if let Some(pull) = reply.get("pull") {
            // The cursor moves only once what came down is in the shop: a pull that would not
            // apply is asked for again, not skipped forever.
            match apply_pull(app, pull) {
                Ok(applied_now) => {
                    pulled = applied_now;
                    cursor = pull.get("cursor").and_then(Value::as_i64);
                }
                Err(e) => log_warn!("what came down with the push could not be applied: {e}"),
            }
        }
    }

    // Applied and refused rows are both done; a refused row is wrong, not late. A refusal
    // names its outbox entry through the batch, whatever the cloud calls the table.
    let ids: Vec<&str> = batch.entries.iter().map(|e| e.id.as_str()).collect();
    let marked = db.transaction(|tx| {
        let outbox = mb_db::Repos::new(tx).outbox();
        for (table, id, reason) in &refused {
            let entry = batch
                .sent
                .iter()
                .find(|((t, i), _)| t == table && i == id)
                .map(|(_, entry)| entry.clone())
                .unwrap_or_else(|| mb_db::repo::OutboxRepo::entry_id(table, id));
            outbox.record_failure(&entry, reason)?;
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
    after_pull(app, &pulled);
    log_info!(
        "pushed {} row(s) to the cloud: {applied} applied, {} refused, {} left to their day files, {still_pending} still waiting",
        batch.rows.len(),
        refused.len(),
        batch.to_file.len()
    );
    Outcome::Pushed {
        applied,
        refused: u64::try_from(refused.len()).unwrap_or(0),
        pending: still_pending,
    }
}

// The day files.

/// What one archive step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Archived {
    /// Nothing sealed is waiting, or the sender may not go.
    Nothing,
    /// This many files went up; `more` says whether the next wake has work.
    Uploaded { files: usize, more: bool },
    Failed(String),
    Stopped(String),
}

/// Every sealed day whose file has not gone up, or changed since it did: build the file from
/// the rows, put it in the bucket, note it. Oldest first, at most `FILES_PER_WAKE` per wake.
/// Not polling: the sender is here because it woke for the outbox, and the day file is what a
/// sealed bill becomes.
pub fn archive_once(app: &App) -> Archived {
    let at = now();
    if app.sync_status().stopped.is_some() || app.device_login().is_none() {
        return Archived::Nothing;
    }
    let (Some(db), Some(login)) = (app.shop_db(), app.device_login()) else {
        return Archived::Nothing;
    };
    let today = today(at);
    let pending = match db.read_transaction(|tx| mb_db::Repos::new(tx).archive().pending(OUTLET, today, FILES_PER_WAKE + 1)) {
        Ok(pending) => pending,
        Err(e) => {
            log_warn!("the days to archive could not be read: {e}");
            return Archived::Nothing;
        }
    };
    if pending.is_empty() {
        return Archived::Nothing;
    }
    let more = pending.len() > FILES_PER_WAKE;
    let mut files = 0;
    for day in pending.into_iter().take(FILES_PER_WAKE) {
        let built = db.read_transaction(|tx| {
            mb_db::Repos::new(tx)
                .archive()
                .day_file(OUTLET, day, at, env!("CARGO_PKG_VERSION"))
        });
        let file = match built {
            Ok(file) => file,
            Err(e) => {
                log_warn!("the day file of {day} could not be built: {e}");
                let _ = db.transaction(|tx| mb_db::Repos::new(tx).archive().record_failure(OUTLET, day, &e.to_string()));
                continue;
            }
        };
        let key = DayFile::key(&login.restaurant_id, day);
        let put = under_login(app, |link, token| {
            link.put_object(archive::BUCKET, &key, &file.gz, "application/gzip", token)
        });
        if let Err(e) = put {
            let _ = db.transaction(|tx| {
                mb_db::Repos::new(tx)
                    .archive()
                    .record_failure(OUTLET, day, &words::from_link(&e).message)
            });
            return match fell(app, at, &e) {
                Outcome::Stopped(why) => Archived::Stopped(why),
                Outcome::Failed(why) => Archived::Failed(why),
                _ => Archived::Nothing,
            };
        }
        if let Err(e) = db.transaction(|tx| mb_db::Repos::new(tx).archive().record_upload(OUTLET, &file, at)) {
            log_warn!("the day file of {day} went up but could not be noted: {e}");
        }
        files += 1;
        log_info!("day file {key}: {} bill(s), {} bytes", file.bills, file.gz.len());
    }
    app.update_sync(|s| {
        s.failures = 0;
        s.last_error = None;
    });
    Archived::Uploaded { files, more }
}

/// Once a business day, off the billing path: the log retention and the WAL checkpoint. The
/// day it ran is kept beside the cursor, so a shop that never idles still runs it once.
pub fn housekeeping_once(app: &App) {
    let at = now();
    let day = today(at);
    if app.sync_status().housekept_day == Some(i64::from(day.days_since_epoch())) {
        return;
    }
    let Some(db) = app.shop_db() else {
        return;
    };
    match mb_db::retention::prune(&db, at) {
        Ok(pruned) => {
            app.update_sync(|s| s.housekept_day = Some(i64::from(day.days_since_epoch())));
            if pruned.total() > 0 {
                log_info!(
                    "housekeeping: {} log row(s) past their retention removed ({})",
                    pruned.total(),
                    pruned
                        .rows
                        .iter()
                        .filter(|(_, n)| *n > 0)
                        .map(|(t, n)| format!("{t} {n}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        Err(e) => log_warn!("housekeeping could not run: {e}"),
    }
}

/// The whole shop, queued from its rows, once per licence: a shop that billed before it had a
/// cloud has nothing in the cloud from those days, and no back-fill existed. The sealed days
/// go by their files (`archive_once` reads them off the bills); the rest goes by push.
pub fn queue_whole_shop_once(app: &App) {
    if app.sync_status().queued_whole_at.is_some() || app.device_login().is_none() {
        return;
    }
    let at = now();
    match queue_whole_shop(app, at) {
        Ok(n) => {
            app.update_sync(|s| s.queued_whole_at = Some(at.millis()));
            log_info!("the whole shop was queued for the cloud once: {n} row(s)");
        }
        Err(e) => log_warn!("the whole shop could not be queued for the cloud: {e}"),
    }
}

/// Queue the whole shop from its rows, and forget which day files went up — a new licence is
/// a new shop in the cloud, and a pen-drive restore does not know what the cloud has seen.
pub fn queue_whole_shop(app: &App, at: Timestamp) -> Result<usize, mb_db::DbError> {
    let Some(db) = app.shop_db() else {
        return Ok(0);
    };
    db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        repos.archive().forget_uploads(OUTLET)?;
        repos.outbox().queue_shop(OUTLET, unsealed_from(today(at)), at)
    })
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

        // Roles before staff, so a new role's people can point at it. Both take the same road
        // a restore takes: the one row writer, newest wins.
        for (table, list) in [("roles", &roles), ("staff", &staff)] {
            for row in list {
                let Some(id) = row.get("id").and_then(Value::as_str) else { continue };
                if row.get("deleted").and_then(Value::as_bool).unwrap_or(false) && table == "roles" {
                    continue;
                }
                let updated_at = Timestamp::from_millis(row.get("updated_at").and_then(Value::as_i64).unwrap_or(0));
                let mut data = row.get("data").cloned().unwrap_or_else(|| json!({}));
                if let (Value::Object(map), Some(deleted)) = (&mut data, row.get("deleted")) {
                    map.insert("deleted".to_owned(), deleted.clone());
                }
                match repos.wire().restore_row(OUTLET, table, id, updated_at, &data) {
                    Ok(wire::Restored::Written) => {
                        if table == "roles" {
                            note(action::ROLE_FROM_PHONE, id)?;
                            applied.roles = applied.roles.saturating_add(1);
                        } else {
                            note(action::STAFF_FROM_PHONE, id)?;
                            applied.staff = applied.staff.saturating_add(1);
                        }
                    }
                    Ok(wire::Restored::Skipped | wire::Restored::Unchanged) => {}
                    Err(e) => log_warn!("a {table} row from the phone could not be read ({id}): {e}"),
                }
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

/// Start the sender. It sleeps until the outbox gains rows or its backoff runs out. One wake
/// does the push, then the day files, then the day's housekeeping.
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
                let more = wake(&app);
                if more {
                    app.sender_wakeup().wake();
                }
            }
        });
    if let Err(e) = spawned {
        log_warn!("the cloud sender could not be started: {e}");
    }
}

/// One wake of the sender, whole. True when there is more to do than one wake may.
pub fn wake(app: &App) -> bool {
    queue_whole_shop_once(app);
    let pushed = push_once(app);
    let more_rows = matches!(pushed, Outcome::Pushed { pending, .. } if pending > 0);
    let archived = match pushed {
        Outcome::Failed(_) | Outcome::Stopped(_) => Archived::Nothing,
        _ => archive_once(app),
    };
    housekeeping_once(app);
    more_rows || matches!(archived, Archived::Uploaded { more: true, .. })
}

// A new computer.

/// A sentence for what a restore brought down.
#[must_use]
pub fn restore_sentence(report: &RestoreReport) -> String {
    format!(
        "{}, {}, {} and {} came down from the cloud.",
        words::count(i64::from(report.bills), "bill", "bills"),
        words::count(i64::from(report.staff), "staff member", "staff members"),
        words::count(i64::from(report.rows), "other row", "other rows"),
        words::count(i64::from(report.days), "day of totals", "days of totals"),
    )
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

/// A REST row's `date` and `timestamptz` columns back into the counter's integers, through the
/// one parser mb-core has for the cloud's spellings.
fn counter_shaped(mut row: Value) -> Value {
    if let Value::Object(map) = &mut row {
        for (key, value) in map.iter_mut() {
            let Some(text) = value.as_str() else { continue };
            let converted = if key == "business_day" || key == "joined_on" || key == "left_on" {
                text.parse::<BusinessDay>().ok().map(|d| Value::from(i64::from(d.days_since_epoch())))
            } else if key.ends_with("_at") || key == "at" {
                Timestamp::parse_iso(text).map(|t| Value::from(t.millis()))
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

/// A REST row of a typed table as the wire row the counter would have sent for it.
fn wire_row_of(table: &str, row: Value) -> Option<WireRow> {
    let data = counter_shaped(row);
    let id = data.get("id").and_then(Value::as_str)?.to_owned();
    Some(WireRow {
        table: table.to_owned(),
        id,
        updated_at: Timestamp::from_millis(data.get("updated_at").and_then(Value::as_i64).unwrap_or(0)),
        deleted: data.get("deleted_at").is_some_and(|v| !v.is_null()),
        data,
    })
}

/// The previous counter's unsent rows, as the cloud last heard: a restore onto a new PC must
/// not quietly lose what the old one never sent.
fn pending_elsewhere(link: &dyn Link, token: &str, rid: &str) -> Result<Option<(String, i64)>, LinkError> {
    let states = read_all(
        link,
        token,
        &format!("sync_state?restaurant_id=eq.{rid}&select=device_id,pending_reported,last_push_at&order=last_push_at.desc"),
    )?;
    Ok(states.into_iter().find_map(|s| {
        let pending = s.get("pending_reported").and_then(Value::as_i64).unwrap_or(0);
        (pending > 0).then(|| {
            (
                s.get("last_push_at")
                    .and_then(Value::as_str)
                    .and_then(Timestamp::parse_iso)
                    .map(words::when)
                    .unwrap_or_else(|| "some time ago".to_owned()),
                pending,
            )
        })
    }))
}

/// Bring the whole shop down into `db` under the counter's login, before anybody opens it:
/// the cloud's tables first (the masters, the people, the last few days of bills), then every
/// day file, newest first. Everything from the tables lands in one transaction; each file in
/// its own. Afterwards the outbox is empty, because the cloud already has it all.
pub fn restore_into(app: &App, db: &Db, login: &DeviceLogin) -> UiResult<RestoreReport> {
    let link = app.link();
    let token = login.access_token.as_str();
    let rid = login.restaurant_id.as_str();
    let read = |path: String| read_all(link.as_ref(), token, &path).map_err(|e| words::from_link(&e));

    if let Some((when, pending)) = pending_elsewhere(link.as_ref(), token, rid).map_err(|e| words::from_link(&e))? {
        return Err(UiError::new(
            "restore.pending_elsewhere",
            format!(
                "The previous computer still had {} it had not sent to the cloud when it was last seen, {when}. \
                 Connect it to the internet and let it finish, or bring its data here on a pen drive, then try again.",
                words::count(pending, "row", "rows")
            ),
        ));
    }

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
    // `id` last, so a page boundary never falls between two bills of the same moment.
    let bills = read(format!("bills?restaurant_id=eq.{rid}&select=*&order=business_day,created_at,id"))?;
    let days = read(format!("day_totals?restaurant_id=eq.{rid}&select=*&order=business_day"))?;

    let at = now();
    let mut rows: Vec<WireRow> = Vec::new();
    for row in &boxed {
        let (Some(table), Some(row_id), Some(payload)) = (
            row.get("table_name").and_then(Value::as_str),
            row.get("row_id").and_then(Value::as_str),
            row.get("payload"),
        ) else {
            continue;
        };
        rows.push(WireRow {
            table: table.to_owned(),
            id: row_id.to_owned(),
            updated_at: at,
            deleted: row.get("deleted_at").is_some_and(|v| !v.is_null()),
            data: payload.clone(),
        });
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
    for mut role in roles {
        let Some(id) = role.get("id").and_then(Value::as_str).map(str::to_owned) else { continue };
        if let Value::Object(map) = &mut role {
            map.insert("permissions".to_owned(), Value::Array(grants.remove(&id).unwrap_or_default()));
        }
        rows.extend(wire_row_of("roles", role));
    }
    rows.extend(staff.into_iter().filter_map(|row| wire_row_of("staff", row)));
    for (table, list) in typed {
        rows.extend(list.into_iter().filter_map(|row| wire_row_of(&table, row)));
    }
    rows.extend(bills.into_iter().filter_map(|row| wire_row_of("bills", row)));
    for day in days {
        let data = counter_shaped(day);
        rows.push(WireRow {
            table: "day_totals".to_owned(),
            id: data.get("business_day").and_then(Value::as_i64).unwrap_or(0).to_string(),
            updated_at: Timestamp::from_millis(data.get("updated_at").and_then(Value::as_i64).unwrap_or(0)),
            deleted: false,
            data,
        });
    }
    let counters_came_down = rows.iter().any(|r| r.table == mb_db::numbering::TABLE);

    let mut report = RestoreReport::default();
    db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        repos.wire().restore_rows(OUTLET, rows, &mut report)?;
        for secret in &secrets {
            if let (Some(id), Some(hash)) = (
                secret.get("staff_id").and_then(Value::as_str),
                secret.get("pin_hash").and_then(Value::as_str),
            ) {
                let _ = repos.people().apply_pin_from_cloud(id, hash)?;
            }
        }
        Ok(())
    })
    .map_err(|e| words::from_db(&e))?;
    for failed in &report.failed {
        log_warn!("restore: could not bring back {failed}");
    }
    app.push(Pushed::Restore {
        says: format!("{} — now the history, day by day.", restore_sentence(&report)),
    });

    // The permanent history: every day file, newest first, each in its own transaction, and
    // each noted as here so the sender never uploads what came down.
    let mut files = list_day_files(link.as_ref(), token, rid).map_err(|e| words::from_link(&e))?;
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let total = files.len();
    let scratch = std::env::temp_dir().join(format!("magicbill-restore-{}", std::process::id()));
    for (n, (day, key)) in files.iter().enumerate() {
        app.push(Pushed::Restore {
            says: format!(
                "Bringing back {day} — file {} of {total}, {} so far.",
                n + 1,
                words::count(i64::from(report.bills), "bill", "bills")
            ),
        });
        let text = match fetch_day_file(app, link.as_ref(), key, &scratch) {
            Ok(text) => text,
            Err(e) => {
                log_warn!("restore: the day file {key} could not be fetched: {e}");
                report.failed.push(format!("{key}: {e}"));
                continue;
            }
        };
        let landed = db.transaction(|tx| {
            let repos = mb_db::Repos::new(tx);
            repos.archive().restore_text(OUTLET, &text, &mut report)?;
            repos.archive().record_upload(
                OUTLET,
                &DayFile {
                    day: *day,
                    gz: Vec::new(),
                    sha256: String::new(),
                    bills: 0,
                },
                at,
            )
        });
        if let Err(e) = landed {
            log_warn!("restore: the day file {key} would not land: {e}");
            report.failed.push(format!("{key}: {e}"));
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);

    db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);
        // The cloud already has everything that was just written.
        let cleared = repos.outbox().clear_backlog(at)?;
        log_info!("restore: {cleared} outbox row(s) cleared — the cloud already has them");

        // The orders that came back are the proof of what was issued: every counter is
        // caught up to them, so the next bill is the one after the last, not number 1
        // again. A cloud written before the counters travelled holds no shape for a series;
        // then the shape is read back from the newest printed number.
        for moved in mb_db::numbering::catch_up(tx)? {
            log_info!(
                "restore: the {} counter of {} caught up with its orders: {} → {}",
                moved.kind.as_sql(),
                moved.terminal,
                moved.from.map_or_else(|| "nothing issued".to_owned(), |n| n.to_string()),
                moved.to
            );
        }
        if !counters_came_down {
            for adopted in mb_db::numbering::adopt_format_from_orders(tx)? {
                log_info!(
                    "restore: the {} counter of {} took its shape from its newest number: prefix `{}`{}",
                    adopted.kind.as_sql(),
                    adopted.terminal,
                    adopted.prefix,
                    adopted
                        .pad_width
                        .map_or_else(String::new, |w| format!(", padded to {w}"))
                );
            }
        }
        // And the cloud learns the series as they now stand, so the next computer needs none
        // of this.
        mb_db::numbering::queue_all(tx, at)?;

        repos.audit().append(
            OUTLET,
            &AuditEntry::new(at, today(at), None, action::CLOUD_RESTORED, "shop")
                .about(restore_sentence(&report)),
        )?;
        Ok(())
    })
    .map_err(|e| words::from_db(&e))?;
    // Everything the cloud has is here; nothing is owed to it from before.
    app.update_sync(|s| s.queued_whole_at = Some(at.millis()));
    log_info!("restore from the cloud: {}", restore_sentence(&report));
    Ok(report)
}

/// Every day file of the shop: the year folders, then the files in each.
fn list_day_files(link: &dyn Link, token: &str, rid: &str) -> Result<Vec<(BusinessDay, String)>, LinkError> {
    let mut out = Vec::new();
    for year in link.list_objects(archive::BUCKET, &format!("{rid}/"), token)? {
        if year.name.parse::<u32>().is_err() {
            continue;
        }
        for file in link.list_objects(archive::BUCKET, &format!("{rid}/{}/", year.name), token)? {
            if let Some(day) = DayFile::day_of_key(&file.name) {
                out.push((day, format!("{rid}/{}/{}", year.name, file.name)));
            }
        }
    }
    Ok(out)
}

/// One day file, fetched under the login and gunzipped to text.
fn fetch_day_file(app: &App, link: &dyn Link, key: &str, scratch: &Path) -> Result<String, LinkError> {
    let to = scratch.join(key.replace('/', "_"));
    let url = link.object_url(archive::BUCKET, key);
    under_login(app, |link, token| link.download(&url, Some(token), &to, &mut |_, _| {}))?;
    let gz = std::fs::read(&to).map_err(|e| LinkError::Server(e.to_string()))?;
    let _ = std::fs::remove_file(&to);
    archive::gunzip(&gz).map_err(|_| LinkError::Unreadable)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mb_license::Status;

    use super::*;
    use crate::cloud::{Link, LinkError, Page, Session, StoredObject};
    use crate::licence_tests::{a_bill_is_taken, a_trading_shop, licence_in};
    use crate::signin_tests::Scratch;

    /// A cloud that answers what the test told it to, and remembers what it was asked —
    /// calls, and files put in its bucket.
    #[derive(Debug, Default)]
    struct FakeLink {
        /// What the next calls answer with, in order; the last one stays.
        answers: Mutex<std::collections::VecDeque<Result<Value, LinkError>>>,
        pushes: Mutex<Vec<Value>>,
        tokens_seen: Mutex<Vec<String>>,
        refreshes: Mutex<u32>,
        /// The bucket: key → bytes.
        objects: Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
        /// What a `put_object` answers, when it is not to succeed.
        put_answer: Mutex<Option<LinkError>>,
        /// What the REST tables answer, by the path's first word; `sync_state` answers `states`.
        states: Mutex<Vec<Value>>,
        tables: Mutex<std::collections::BTreeMap<String, Vec<Value>>>,
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

        fn keys(&self) -> Vec<String> {
            self.objects.lock().unwrap().keys().cloned().collect()
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
        fn rest(&self, path: &str, _: &str, _: usize, _: usize) -> Result<Page, LinkError> {
            if path.starts_with("sync_state") {
                return Ok(Page { rows: self.states.lock().unwrap().clone(), total: None });
            }
            let table = path.split('?').next().unwrap_or("");
            let rows = self.tables.lock().unwrap().get(table).cloned().unwrap_or_default();
            Ok(Page { total: Some(rows.len()), rows })
        }
        fn refresh_session(&self, _: &str) -> Result<Session, LinkError> {
            *self.refreshes.lock().unwrap() += 1;
            Ok(Session {
                access_token: "fresh-access".to_owned(),
                refresh_token: "fresh-refresh".to_owned(),
                expires_at: Timestamp::from_millis(1),
            })
        }
        fn download(
            &self,
            url: &str,
            token: Option<&str>,
            to: &Path,
            _: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<String, LinkError> {
            if token.is_none() {
                return Err(LinkError::Unreachable);
            }
            let key = url.trim_start_matches("fake://");
            let bytes = self
                .objects
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or(LinkError::Refused("no such file".to_owned()))?;
            if let Some(parent) = to.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(to, &bytes).map_err(|e| LinkError::Server(e.to_string()))?;
            Ok(crate::cloud::sha256_hex(&bytes))
        }
        fn put_object(&self, _bucket: &str, key: &str, bytes: &[u8], content_type: &str, token: &str) -> Result<(), LinkError> {
            self.tokens_seen.lock().unwrap().push(token.to_owned());
            if let Some(e) = self.put_answer.lock().unwrap().clone() {
                return Err(e);
            }
            assert_eq!(content_type, "application/gzip");
            self.objects.lock().unwrap().insert(key.to_owned(), bytes.to_vec());
            Ok(())
        }
        fn list_objects(&self, _bucket: &str, prefix: &str, _token: &str) -> Result<Vec<StoredObject>, LinkError> {
            let objects = self.objects.lock().unwrap();
            let mut names: Vec<String> = objects
                .keys()
                .filter_map(|k| k.strip_prefix(prefix))
                .map(|rest| rest.split('/').next().unwrap_or("").to_owned())
                .collect();
            names.sort();
            names.dedup();
            Ok(names
                .into_iter()
                .map(|name| StoredObject {
                    name,
                    updated_at: "2026-09-20T00:00:00+00:00".to_owned(),
                    size: 0,
                })
                .collect())
        }
        fn object_url(&self, _bucket: &str, key: &str) -> String {
            format!("fake://{key}")
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

    fn an_ok_push(applied: u64, cursor: i64) -> Value {
        json!({ "applied": applied, "refused": [], "pull": a_pull(cursor) })
    }

    /// Move a settled bill (and its day) back in time, so its day is sealed by age.
    fn age_the_bill(app: &App, number: &str, days: i32) -> BusinessDay {
        let at = now();
        let old_day = BusinessDay::from_days_since_epoch(today(at).days_since_epoch() - days);
        app.shop_db()
            .expect("a shop")
            .transaction(|tx| {
                tx.execute(
                    "UPDATE orders SET business_day = ?2, created_at = created_at - ?3, settled_at = settled_at - ?3 WHERE bill_number_formatted = ?1",
                    (number, i64::from(old_day.days_since_epoch()), i64::from(days) * 86_400_000),
                )?;
                // The outbox entries for the old day, as a save would have written them.
                let repos = mb_db::Repos::new(tx);
                let key = old_day.days_since_epoch().to_string();
                for table in wire::TOTALS_TABLES {
                    repos.outbox().enqueue(OUTLET, table, &key, mb_db::repo::Op::Upsert, at)?;
                }
                Ok(())
            })
            .expect("aged");
        old_day
    }

    #[test]
    fn a_push_marks_the_batch_done_keeps_the_cursor_and_notes_a_refusal() {
        let scratch = Scratch::new("sync_push");
        let (app, link) = a_connected_shop(&scratch, "push");
        let waiting = pending(&app);
        assert!(waiting > 0, "a built shop queues its rows");

        link.will_answer(Ok(json!({
            "applied": waiting - 1,
            "refused": [{ "table": "menu_items", "id": "itm_tea", "reason": "ZZZ not today" }],
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
        link.will_answer(Ok(an_ok_push(1, 1)));
        let outcome = push_once(&app);
        assert!(matches!(outcome, Outcome::Pushed { .. }), "{outcome:?}");
        assert_eq!(*link.refreshes.lock().unwrap(), 1);
        let tokens = link.tokens_seen.lock().unwrap().clone();
        assert_eq!(tokens.first().map(String::as_str), Some(before.as_str()));
        assert_eq!(tokens.last().map(String::as_str), Some("fresh-access"));
        assert_eq!(app.device_login().expect("kept").access_token, "fresh-access");
    }

    /// A settled one-line bill of `item` on `day`, saved the way a settle saves it — enough
    /// for the day's item totals to name the item.
    fn a_settled_bill_of(app: &App, n: i64, item: &str, day: BusinessDay, at: Timestamp) {
        use mb_core::{BillInput, Cart, ItemId, Money, Payment, PaymentMode, Qty, Registration, Settlement, StaffId, TaxRate};
        let db = app.shop_db().expect("a shop");
        let mut cart = Cart::new();
        cart.add(
            mb_core::ItemSnapshot::new(ItemId::new(item), format!("Item {n}"), Money::from_paise(1_000), TaxRate::ZERO),
            Qty::from_whole(1).expect("one"),
            None,
            vec![],
        )
        .expect("added");
        let bill = mb_core::compute_bill(BillInput::new(&cart, Registration::Regular)).expect("a bill");
        let mut settlement = Settlement::new();
        settlement
            .add(Payment::new(PaymentMode::Cash, bill.grand_total).expect("a payment"))
            .expect("paid");
        let id = mb_core::OrderId::new(format!("ord_fan_{n}"));
        let mut draft = mb_core::DraftOrder::new(id, day, at, mb_core::Placement::Parcel, StaffId::new(crate::state::DEFAULT_STAFF));
        draft.core.cart = cart;
        let number = mb_core::Claimed { value: u64::try_from(n).unwrap_or(0) + 1000, formatted: format!("F{n}"), business_day: day };
        let open = mb_core::OpenOrder { core: draft.core, token: number.clone(), bill_number: Some(number) };
        let settled = open.settle(bill, settlement, at, StaffId::new(crate::state::DEFAULT_STAFF)).expect("settled");
        db.transaction(|tx| mb_db::Repos::new(tx).orders().save(OUTLET, app.terminal_id(), &mb_core::AnyOrder::Settled(settled)))
            .expect("saved");
    }

    #[test]
    fn a_batch_never_exceeds_two_hundred_wire_rows_and_a_fan_out_goes_in_chunks() {
        let scratch = Scratch::new("sync_batch");
        let (app, link) = a_connected_shop(&scratch, "batch");
        let at = now();
        let day = today(at);
        let db = app.shop_db().expect("a shop");
        // Three hundred items, each sold once today: one day_item_totals entry fans out past
        // the cap.
        db.transaction(|tx| {
            for n in 0..300 {
                tx.execute(
                    "INSERT INTO items (id, outlet_id, category_id, name, unit_price, tax_class_id, price_basis, is_available, sort_order, is_open_price, updated_at, created_at)
                     SELECT 'itm_' || ?1, outlet_id, category_id, 'Item ' || ?1, 1000, tax_class_id, price_basis, 1, 0, 0, ?2, ?2 FROM items WHERE id = 'itm_tea'",
                    (n, at.millis()),
                )?;
            }
            Ok(())
        })
        .expect("items");
        for n in 0..300 {
            a_settled_bill_of(&app, n, &format!("itm_{n}"), day, at);
        }
        // Drain everything but the day's item totals, so the fan-out is the whole batch.
        db.transaction(|tx| {
            let outbox = mb_db::Repos::new(tx).outbox();
            let all = outbox.pending(usize::MAX)?;
            let ids: Vec<&str> = all.iter().filter(|r| r.table_name != "day_item_totals").map(|r| r.id.as_str()).collect();
            outbox.mark_synced(&ids, at)
        })
        .expect("drained");
        assert_eq!(pending(&app), 1);

        link.now_answers(Ok(an_ok_push(200, 9)));
        let outcome = push_once(&app);
        assert!(matches!(outcome, Outcome::Pushed { pending: 0, .. }), "{outcome:?}");
        let pushes = link.pushes.lock().unwrap().clone();
        assert!(pushes.len() >= 2, "a fan-out of 300 rows needs two pushes, got {}", pushes.len());
        for push in &pushes {
            assert!(push["rows"].as_array().is_some_and(|r| r.len() <= BATCH), "a push carried more than {BATCH} rows");
        }
        let rows: usize = pushes.iter().map(|p| p["rows"].as_array().map_or(0, Vec::len)).sum();
        assert!(rows >= 300, "{rows} rows went up for 300 items");
        assert_eq!(pending(&app), 0, "the entry is done once its last chunk landed");
    }

    #[test]
    fn a_bill_of_a_sealed_day_never_leaves_by_push_and_its_file_goes_up() {
        let scratch = Scratch::new("sync_sealed");
        let (app, link) = a_connected_shop(&scratch, "sealed");
        let number = a_bill_is_taken(&app);
        let old_day = age_the_bill(&app, &number, archive::SEAL_AFTER_DAYS + 2);

        link.now_answers(Ok(an_ok_push(0, 1)));
        let outcome = push_once(&app);
        // The masters went up; the bill and its item totals did not.
        assert!(matches!(outcome, Outcome::Pushed { .. }), "{outcome:?}");
        for push in link.pushes.lock().unwrap().iter() {
            for row in push["rows"].as_array().expect("rows") {
                assert_ne!(row["table"], "bills", "a bill of a sealed day went by push");
                assert_ne!(row["table"], "day_item_totals", "a sealed day's item totals went by push");
            }
        }
        assert_eq!(pending(&app), 0, "the sealed rows are done: their file carries them");

        // The archive step builds and uploads the day's file, once.
        let archived = archive_once(&app);
        assert!(matches!(archived, Archived::Uploaded { files: 1, more: false }), "{archived:?}");
        let rid = app.device_login().expect("login").restaurant_id;
        assert_eq!(link.keys(), vec![DayFile::key(&rid, old_day)]);
        assert_eq!(archive_once(&app), Archived::Nothing, "uploaded once");
        let gz = link.objects.lock().unwrap().get(&DayFile::key(&rid, old_day)).cloned().expect("the file");
        let text = archive::gunzip(&gz).expect("gunzip");
        assert!(text.contains(&format!("\"bill_number\":\"{number}\"")), "the bill is in its day file");

        // A void of that bill after the seal: by the file again, not by push.
        let id: String = app
            .shop_db()
            .expect("a shop")
            .read(|c| Ok(c.query_row("SELECT id FROM orders WHERE bill_number_formatted = ?1", [&number], |r| r.get(0))?))
            .expect("id");
        app.shop_db()
            .expect("a shop")
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mb_core::AnyOrder::Settled(settled)) = repos.orders().find(&mb_core::OrderId::new(&id))? else {
                    return Err(mb_db::DbError::invariant("settled"));
                };
                let voided = settled
                    .void("late", mb_core::StaffId::new(crate::state::DEFAULT_STAFF), now())
                    .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
                repos.orders().save(OUTLET, app.terminal_id(), &mb_core::AnyOrder::Voided(voided))
            })
            .expect("voided");
        app.update_sync(|s| s.next_try_at = None);
        let before = link.pushes.lock().unwrap().len();
        let _ = push_once(&app);
        let after = link.pushes.lock().unwrap().clone();
        for push in after.iter().skip(before) {
            for row in push["rows"].as_array().expect("rows") {
                assert_ne!(row["table"], "bills", "a void of a sealed day went by push");
            }
        }
        assert!(matches!(archive_once(&app), Archived::Uploaded { files: 1, .. }), "the dirty day went up again");
        let gz = link.objects.lock().unwrap().get(&DayFile::key(&rid, old_day)).cloned().expect("the file");
        assert!(archive::gunzip(&gz).expect("gunzip").contains("\"status\":\"voided\""));
        // And Health knows nothing is waiting.
        let (says, tone) = crate::licensing::cloud_copy_says(&app, now());
        assert_eq!(tone, "ok", "{says}");
    }

    #[test]
    fn a_file_that_will_not_go_up_is_noted_and_the_ladder_climbs() {
        let scratch = Scratch::new("sync_file_fail");
        let (app, link) = a_connected_shop(&scratch, "file_fail");
        let number = a_bill_is_taken(&app);
        let old_day = age_the_bill(&app, &number, archive::SEAL_AFTER_DAYS + 1);
        *link.put_answer.lock().unwrap() = Some(LinkError::Server("bucket down".to_owned()));
        assert!(matches!(archive_once(&app), Archived::Failed(_)));
        assert_eq!(app.sync_status().failures, 1);
        let ledger = app
            .shop_db()
            .expect("a shop")
            .read_transaction(|tx| mb_db::Repos::new(tx).archive().ledger(OUTLET, old_day))
            .expect("ledger")
            .expect("noted");
        assert!(ledger.last_error.is_some());
        assert!(ledger.uploaded_at.is_none());
        let (says, tone) = crate::licensing::cloud_copy_says(&app, now());
        assert_eq!(tone, "warn");
        assert!(says.contains("day file"), "{says}");
        // A dead login stops the sender here too.
        app.update_sync(|s| s.next_try_at = None);
        *link.put_answer.lock().unwrap() = Some(LinkError::Dead("revoked".to_owned()));
        assert!(matches!(archive_once(&app), Archived::Stopped(_)));
        assert!(app.sync_status().stopped.is_some());
    }

    #[test]
    fn a_restore_brings_the_day_files_down_and_refuses_while_rows_are_pending_elsewhere() {
        let scratch = Scratch::new("sync_restore_files");
        let (app, link) = a_connected_shop(&scratch, "restore_files");
        let number = a_bill_is_taken(&app);
        let old_day = age_the_bill(&app, &number, archive::SEAL_AFTER_DAYS + 5);
        link.now_answers(Ok(an_ok_push(0, 1)));
        let _ = push_once(&app);
        assert!(matches!(archive_once(&app), Archived::Uploaded { files: 1, .. }));
        let login = app.device_login().expect("login");
        let rid = login.restaurant_id.clone();
        assert_eq!(link.keys(), vec![DayFile::key(&rid, old_day)]);

        // The cloud's tables hold the masters a bill names: the till and the person.
        let terminal = app
            .shop_db()
            .expect("a shop")
            .read_transaction(|tx| mb_db::Repos::new(tx).wire().whole_rows("terminals", app.terminal_id(), now()))
            .expect("the till")
            .pop()
            .expect("one row");
        link.tables.lock().unwrap().insert(
            "shop_rows".to_owned(),
            vec![json!({ "table_name": "terminals", "row_id": app.terminal_id(), "payload": terminal.data, "updated_at": "2026-09-01T00:00:00+00:00", "deleted_at": null })],
        );
        let mut item = app
            .shop_db()
            .expect("a shop")
            .read_transaction(|tx| mb_db::Repos::new(tx).wire().whole_rows("items", "itm_tea", now()))
            .expect("the tea")
            .pop()
            .expect("one row")
            .data;
        item[wire::ROW_TABLE_KEY] = json!("items");
        link.tables.lock().unwrap().insert(
            "menu_items".to_owned(),
            vec![json!({ "id": "itm_tea", "name": "Masala Tea", "unit_price_paise": 2500, "tax_rate_bp": 500, "is_available": true, "sort_order": 0, "row": item, "updated_at": "2026-09-01T00:00:00+00:00", "deleted_at": null })],
        );
        link.tables.lock().unwrap().insert(
            "staff".to_owned(),
            vec![json!({ "id": crate::state::DEFAULT_STAFF, "name": "Owner", "status": "active", "employment_type": "full_time", "is_rider": false, "updated_at": "2026-09-01T00:00:00+00:00", "deleted_at": null })],
        );
        let other = Scratch::new("sync_restore_files_down");
        let db = mb_db::Db::open(&mb_db::DbConfig::new(other.dir().join("down.db"))).expect("open");
        // The previous computer still holds rows: refused, with the count.
        *link.states.lock().unwrap() = vec![json!({ "device_id": "old", "pending_reported": 7, "last_push_at": "2026-09-01T10:00:00+00:00" })];
        let refused = restore_into(&app, &db, &login).expect_err("refused while rows are pending elsewhere");
        assert_eq!(refused.code, "restore.pending_elsewhere");
        assert!(refused.message.contains("7 rows"), "{}", refused.message);

        // Nothing pending elsewhere: the tables are empty, the history comes down from the file.
        link.states.lock().unwrap().clear();
        let report = restore_into(&app, &db, &login).expect("restored");
        let (terminals, staff, tills): (i64, i64, String) = db
            .read(|c| Ok((c.query_row("SELECT count(*) FROM terminals", [], |r| r.get(0))?, c.query_row("SELECT count(*) FROM staff", [], |r| r.get(0))?, c.query_row("SELECT COALESCE(group_concat(id), '') FROM terminals", [], |r| r.get(0))?)))
            .expect("counts");
        assert!(report.failed.is_empty(), "{:?} (terminals {terminals} [{tills}], staff {staff}, wanted {})", report.failed, app.terminal_id());
        assert_eq!(report.bills, 1, "{report:?}");
        assert_eq!(report.days, 1);
        let back: i64 = db
            .read(|c| Ok(c.query_row("SELECT count(*) FROM orders WHERE bill_number_formatted = ?1", [&number], |r| r.get(0))?))
            .expect("count");
        assert_eq!(back, 1);
        // The day is noted as up, so the sender never uploads what came down.
        let ledger = db
            .read_transaction(|tx| mb_db::Repos::new(tx).archive().ledger(OUTLET, old_day))
            .expect("ledger")
            .expect("noted");
        assert!(ledger.uploaded_at.is_some());
        // Nothing is owed from before, except the counters as they now stand.
        let left = db.read_transaction(|tx| mb_db::Repos::new(tx).outbox().pending(usize::MAX)).expect("pending");
        assert!(left.iter().all(|e| e.table_name == mb_db::numbering::TABLE), "{left:?}");
        assert!(app.sync_status().queued_whole_at.is_some(), "nothing is owed from before");
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
                        "data": { "role_id": "role_phone", "name": "Phone Person", "status": "active",
                                  "employment_type": "full_time" } }],
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

        // An older copy changes nothing — a staff row, and a role too.
        let older = json!({ "cursor": 8,
            "staff": [{ "id": "staff_phone", "updated_at": at - 5000, "deleted": false,
                        "data": { "name": "Stale Name", "status": "active", "employment_type": "full_time" } }],
            "roles": [{ "id": "role_phone", "updated_at": at - 5000, "deleted": false,
                        "data": { "name": "Stale Role", "is_builtin": false, "permissions": [] } }] });
        let again = apply_pull(&app, &older).expect("applied");
        assert_eq!((again.staff, again.roles), (0, 0));
        let people = app
            .shop_db()
            .expect("shop")
            .read_transaction(|tx| mb_db::Repos::new(tx).people().list_staff(OUTLET))
            .expect("staff");
        let person = people.iter().find(|p| p.id.as_str() == "staff_phone").expect("still there");
        assert_eq!(person.role_name.as_deref(), Some("From the phone"), "an older role copy changed nothing");

        // A person the phone deleted comes back as having left: bills name people.
        let gone = json!({ "cursor": 9,
            "staff": [{ "id": "staff_phone", "updated_at": at + 5000, "deleted": true,
                        "data": { "name": "Phone Person", "status": "active", "employment_type": "full_time" } }] });
        assert_eq!(apply_pull(&app, &gone).expect("applied").staff, 1);
        let people = app
            .shop_db()
            .expect("shop")
            .read_transaction(|tx| mb_db::Repos::new(tx).people().list_staff(OUTLET))
            .expect("staff");
        let person = people.iter().find(|p| p.id.as_str() == "staff_phone").expect("never deleted");
        assert_eq!(person.status, mb_db::repo::people::StaffStatus::Left);

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
    fn housekeeping_runs_once_a_day_and_the_whole_shop_is_queued_once() {
        let scratch = Scratch::new("sync_housekeeping");
        let (app, _link) = a_connected_shop(&scratch, "housekeeping");
        assert!(app.sync_status().housekept_day.is_none());
        housekeeping_once(&app);
        let day = app.sync_status().housekept_day.expect("ran");
        assert_eq!(day, i64::from(today(now()).days_since_epoch()));
        housekeeping_once(&app);
        assert_eq!(app.sync_status().housekept_day, Some(day));

        let before = pending(&app);
        queue_whole_shop_once(&app);
        assert!(app.sync_status().queued_whole_at.is_some());
        assert!(pending(&app) >= before);
        let again = pending(&app);
        queue_whole_shop_once(&app);
        assert_eq!(pending(&app), again, "once");
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
        assert_eq!("1970-01-02".parse::<BusinessDay>().map(|d| d.days_since_epoch()).ok(), Some(1));
        assert_eq!("2026-08-27".parse::<BusinessDay>().map(|d| d.days_since_epoch()).ok(), Some(20692));
        assert_eq!(Timestamp::parse_iso("1970-01-01T00:00:01Z").map(Timestamp::millis), Some(1_000));
        assert_eq!(Timestamp::parse_iso("1970-01-01T00:00:01.5+00:00").map(Timestamp::millis), Some(1_500));
        // +05:30 is behind UTC by five and a half hours.
        assert_eq!(Timestamp::parse_iso("1970-01-01T05:30:00+05:30").map(Timestamp::millis), Some(0));
        assert_eq!(
            Timestamp::parse_iso("2026-08-27T10:11:12.123456+00:00").map(Timestamp::millis),
            Some(20692 * 86_400_000 + 36_672_123)
        );
        assert_eq!(Timestamp::parse_iso("nonsense"), None);
        let shaped = counter_shaped(json!({
            "business_day": "2026-08-27", "created_at": "2026-08-27T00:00:00+00:00", "name": "x", "at": "1970-01-01T00:00:02Z"
        }));
        assert_eq!(shaped["business_day"], 20692);
        assert_eq!(shaped["created_at"], 20692_i64 * 86_400_000);
        assert_eq!(shaped["at"], 2000);
        assert_eq!(shaped["name"], "x");
    }
}
