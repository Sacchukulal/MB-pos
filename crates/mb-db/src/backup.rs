//! Backup, verify and restore — the session's headline, and audit A1's fix.
//!
//! > *"Only bills are backed up. Everything else lives on one hard disk… There
//! > is no backup button and no restore button anywhere in the app. If that
//! > PC's disk fails tomorrow morning, you lose: the whole menu with prices,
//! > every setting you spent weeks tuning, all expense records, and — worst —
//! > **the record of who owes you how much money.**"*
//! >
//! > *"This alone justifies the rebuild."*
//!
//! Requirement 1 of the ten is *"a hard disk failure loses NOTHING. Backup and
//! restore work and have been tested by actually restoring onto a clean
//! machine."* Note what it does not say: it does not say a backup exists.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::conn::Db;
use crate::error::DbError;
use crate::migrate;

/// The tables whose row counts go into the manifest and are checked on verify.
///
/// Every table in the schema, hand-listed rather than discovered, so that a new
/// table has to be added here on purpose. A table nobody counted is a table
/// whose loss a verify would not notice.
///
/// **P25 found that the list had drifted** and made the rule a test rather than
/// a sentence — `tests/behaviour.rs::every_table_is_counted_or_named_as_not`
/// fails the build for a table that is in neither this list nor [`UNCOUNTED`].
/// Eleven tables had gone missing since P12, including every one of P19's paired
/// phones and every one of P24's kitchen tickets, so a backup of a shop that
/// used either could lose them and still verify clean. This is D40 again: *the
/// rules that erode are enforced by scripts, not by agreement.*
pub const COUNTED: &[&str] = &[
    "applied_events",
    "attachments",
    "audit_log",
    "bill_charges",
    "bill_lines",
    "bill_tax_rows",
    "bills",
    "cash_movements",
    "categories",
    "category_printers",
    "combo_components",
    "combos",
    "counters",
    "credit_adjustments",
    "customer_payments",
    "customers",
    "day_close_denominations",
    "day_closes",
    "dining_tables",
    "expense_categories",
    "expenses",
    "item_modifier_groups",
    "item_variants",
    "items",
    "kitchen_deliveries",
    "kitchen_ledger",
    "lan_devices",
    "material_balances",
    "material_units",
    "materials",
    "modifier_groups",
    "modifiers",
    "order_events",
    "order_line_modifiers",
    "order_lines",
    "orders",
    "outlets",
    "payments",
    "permissions",
    "printers",
    "purchase_lines",
    "purchase_order_lines",
    "purchase_orders",
    "purchases",
    "reasons",
    "recipe_lines",
    "recipes",
    "recurring_expenses",
    "refunds",
    "reprints",
    "reservations",
    "role_permissions",
    "roles",
    "sections",
    "settings",
    "staff",
    "stock_count_lines",
    "stock_counts",
    "stock_day_closes",
    "stock_movements",
    "stock_problems",
    "store_profile",
    "supplier_adjustments",
    "supplier_materials",
    "supplier_payments",
    "suppliers",
    "sync_outbox",
    "tax_class_rates",
    "tax_classes",
    "terminals",
    "waitlist",
];

/// Tables deliberately left out of the manifest, each with its reason.
///
/// Being on this list is a decision, which is the whole point of having it.
pub const UNCOUNTED: &[&str] = &[
    // The migration ledger. It is written by the engine before any of this
    // exists, and a restore checks the schema version on its own.
    "schema_version",
    // **D35: the print spool is a SPOOL, not a log.** Rows leave it when they
    // print, so its count is whatever happened to be queued at the second the
    // backup ran — a number that would differ on every verify for no reason.
    "print_jobs",
];

/// What sits beside a backup file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub taken_at_ms: i64,
    pub schema_version: u32,
    pub app_version: String,
    pub bytes: u64,
    pub checksum: String,
    /// `(table, rows)`, in table order.
    pub counts: Vec<(String, i64)>,
    /// **D132 — the photographs.** `(filename, bytes, checksum)`.
    ///
    /// Empty on every backup taken before P26 and on every shop that has never
    /// photographed an invoice, which is what makes an old single-file backup
    /// still restore.
    pub attachments: Vec<(String, u64, String)>,
}

impl Manifest {
    fn to_text(&self) -> String {
        let mut s = String::new();
        s.push_str("magicbill-backup 1\n");
        s.push_str(&format!("taken_at_ms {}\n", self.taken_at_ms));
        s.push_str(&format!("schema_version {}\n", self.schema_version));
        s.push_str(&format!("app_version {}\n", self.app_version));
        s.push_str(&format!("bytes {}\n", self.bytes));
        s.push_str(&format!("checksum {}\n", self.checksum));
        for (table, rows) in &self.counts {
            s.push_str(&format!("count {table} {rows}\n"));
        }
        for (name, bytes, checksum) in &self.attachments {
            s.push_str(&format!("attachment {name} {bytes} {checksum}\n"));
        }
        s
    }

    fn parse(text: &str) -> Result<Manifest, DbError> {
        let bad = |what: &str| DbError::invariant(format!("the backup manifest has no {what}"));
        let mut taken_at_ms = None;
        let mut schema_version = None;
        let mut app_version = None;
        let mut bytes = None;
        let mut checksum = None;
        let mut counts = Vec::new();
        let mut attachments = Vec::new();

        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (first, second, third) = (parts.next(), parts.next(), parts.next());
            match (first, second, third) {
                (Some("taken_at_ms"), Some(v), _) => taken_at_ms = v.parse().ok(),
                (Some("schema_version"), Some(v), _) => schema_version = v.parse().ok(),
                (Some("app_version"), Some(v), _) => app_version = Some(v.to_owned()),
                (Some("bytes"), Some(v), _) => bytes = v.parse().ok(),
                (Some("checksum"), Some(v), _) => checksum = Some(v.to_owned()),
                (Some("count"), Some(table), Some(n)) => {
                    counts.push((table.to_owned(), n.parse().unwrap_or(-1)));
                }
                // **D132.** A manifest written before P26 has none of these, and
                // the empty vector is exactly what makes such a backup restore.
                (Some("attachment"), Some(name), Some(size)) => {
                    attachments.push((
                        name.to_owned(),
                        size.parse().unwrap_or(0),
                        parts.next().unwrap_or_default().to_owned(),
                    ));
                }
                _ => {}
            }
        }

        Ok(Manifest {
            taken_at_ms: taken_at_ms.ok_or_else(|| bad("taken_at_ms"))?,
            schema_version: schema_version.ok_or_else(|| bad("schema_version"))?,
            app_version: app_version.ok_or_else(|| bad("app_version"))?,
            bytes: bytes.ok_or_else(|| bad("bytes"))?,
            checksum: checksum.ok_or_else(|| bad("checksum"))?,
            counts,
            attachments,
        })
    }
}

/// **D132 — where the photographs live: beside the database, never inside it.**
///
/// A photographed invoice is ~200 KB and a shop takes ~100 deliveries a month.
/// Inside the database that is 240 MB a year that `VACUUM INTO` would copy on
/// every backup, spending R5 and R6 on pictures no query reads. So they are
/// files, and the backup carries the folder.
#[must_use]
pub fn attachments_dir(db: &Path) -> PathBuf {
    match db.parent() {
        Some(parent) => parent.join("attachments"),
        None => PathBuf::from("attachments"),
    }
}

/// The photographs that belong to one backup file.
#[must_use]
pub fn backup_attachments_dir(backup: &Path) -> PathBuf {
    let mut p = backup.as_os_str().to_os_string();
    p.push(".attachments");
    PathBuf::from(p)
}

/// Copy every file in `from` into `to`, returning `(name, bytes, checksum)` for
/// each. A folder that does not exist is not an error: it is a shop that has
/// never photographed an invoice.
fn copy_attachments(from: &Path, to: &Path) -> Result<Vec<(String, u64, String)>, DbError> {
    let Ok(entries) = std::fs::read_dir(from) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    let mut made = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        if !made {
            std::fs::create_dir_all(to).map_err(|e| {
                DbError::invariant(format!("could not create {}: {e}", to.display()))
            })?;
            made = true;
        }
        std::fs::copy(&path, to.join(&name)).map_err(|e| {
            DbError::invariant(format!("could not copy the photograph {name}: {e}"))
        })?;
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        out.push((name, bytes, file_checksum(&path)?));
    }
    out.sort();
    Ok(out)
}

/// One backup on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    pub path: PathBuf,
    pub manifest: Manifest,
}

impl Backup {
    #[must_use]
    pub fn manifest_path(&self) -> PathBuf {
        manifest_path(&self.path)
    }
}

fn manifest_path(db: &Path) -> PathBuf {
    let mut p = db.as_os_str().to_os_string();
    p.push(".manifest");
    PathBuf::from(p)
}

/// Take a backup of the live database while the shop keeps billing.
///
/// **Not a file copy.** A file copy can catch a half-written page and produce a
/// backup that opens fine and is wrong — the failure mode you never find out
/// about until the day you need it.
///
/// The copy itself is [`Db::backup_to`], which is `VACUUM INTO` on a reader.
/// That reverses this session's prompt — see that function's comment for what
/// implementing the online backup API turned up and why a backup that may never
/// finish is worse than one without a progress bar.
///
/// WAL means the copy reads a snapshot, so **the cashier never waits** — `t7`
/// runs settles on another thread throughout and asserts the copy is still
/// consistent.
pub fn take(db: &Db, to: &Path, app_version: &str) -> Result<Backup, DbError> {
    if let Some(parent) = to.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            DbError::invariant(format!("could not create {}: {e}", parent.display()))
        })?;
    }

    db.backup_to(to)?;

    // **D132 — a backup is the database AND the photographs.** They are copied
    // before the manifest is written, because the manifest is what says they
    // are there and a manifest that promises a file it has not written is worse
    // than no manifest at all.
    let attachments =
        copy_attachments(&attachments_dir(db.path()), &backup_attachments_dir(to))?;

    let conn = open_read_only(to)?;
    let manifest = Manifest {
        taken_at_ms: now_ms(),
        schema_version: read_schema_version(&conn)?,
        app_version: app_version.to_owned(),
        bytes: std::fs::metadata(to).map(|m| m.len()).unwrap_or(0),
        checksum: file_checksum(to)?,
        counts: count_rows(&conn)?,
        attachments,
    };
    drop(conn);

    std::fs::write(manifest_path(to), manifest.to_text())
        .map_err(|e| DbError::invariant(format!("could not write the backup manifest: {e}")))?;

    Ok(Backup {
        path: to.to_path_buf(),
        manifest,
    })
}

/// Copy a finished backup somewhere else.
///
/// **A backup on the same failing disk is not a backup.** The second location
/// is a pen drive or a network path, and a failure to write it is a WARNING and
/// not a failed backup — a missing pen drive must never stop the shop backing
/// up at all.
pub fn copy_to_second_location(backup: &Backup, dir: &Path) -> Result<PathBuf, DbError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| DbError::invariant(format!("could not create {}: {e}", dir.display())))?;
    let name = backup
        .path
        .file_name()
        .ok_or_else(|| DbError::invariant("the backup has no file name"))?;
    let target = dir.join(name);
    std::fs::copy(&backup.path, &target)
        .map_err(|e| DbError::invariant(format!("could not copy the backup: {e}")))?;
    std::fs::copy(backup.manifest_path(), manifest_path(&target))
        .map_err(|e| DbError::invariant(format!("could not copy the manifest: {e}")))?;
    // D132: the second copy carries the photographs too, or it is not a second
    // copy of the same thing.
    copy_attachments(
        &backup_attachments_dir(&backup.path),
        &backup_attachments_dir(&target),
    )?;
    Ok(target)
}

/// What a verify found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub path: PathBuf,
    pub integrity_ok: bool,
    pub foreign_keys_ok: bool,
    pub checksum_ok: bool,
    /// Tables whose row count disagrees with the manifest: `(table, manifest,
    /// actual)`.
    pub count_mismatches: Vec<(String, i64, i64)>,
    pub schema_version: u32,
    /// **D132.** Photographs the manifest promises that are missing or do not
    /// match their checksum. A picture of a ₹40,000 invoice that silently rots
    /// is exactly the failure a verify exists to find.
    pub bad_attachments: Vec<String>,
    /// How many photographs this backup carries, for the health line.
    pub attachment_count: usize,
}

impl VerifyReport {
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.integrity_ok
            && self.foreign_keys_ok
            && self.checksum_ok
            && self.count_mismatches.is_empty()
            && self.bad_attachments.is_empty()
    }

    /// One line an owner could read, for the health panel (P22).
    #[must_use]
    pub fn summary(&self) -> String {
        if self.is_ok() {
            return format!("{} verified", self.path.display());
        }
        let mut why = Vec::new();
        if !self.integrity_ok {
            why.push("the file is damaged".to_owned());
        }
        if !self.foreign_keys_ok {
            why.push("some rows point at rows that are not there".to_owned());
        }
        if !self.checksum_ok {
            why.push("the file does not match its manifest".to_owned());
        }
        if !self.count_mismatches.is_empty() {
            why.push(format!("{} table(s) have the wrong row count", self.count_mismatches.len()));
        }
        if !self.bad_attachments.is_empty() {
            why.push(format!(
                "{} photograph(s) are missing or damaged",
                self.bad_attachments.len()
            ));
        }
        format!("{} FAILED: {}", self.path.display(), why.join("; "))
    }
}

/// Check a backup before trusting it. **A backup that has never been restored
/// is a rumour**, and this is the last chance to find out before the day it
/// matters.
///
/// Four checks, and the second one is the one people forget: `integrity_check`
/// does not look at foreign keys, so a backup with orphaned bill lines passes
/// it and restores into a broken shop.
pub fn verify(path: &Path) -> Result<VerifyReport, DbError> {
    let conn = open_read_only(path)?;

    let integrity: String =
        conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    let integrity_ok = integrity == "ok";

    let foreign_keys_ok = {
        let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
        let mut rows = stmt.query([])?;
        rows.next()?.is_none()
    };

    let schema_version = read_schema_version(&conn)?;
    let actual = count_rows(&conn)?;
    drop(conn);

    let (checksum_ok, count_mismatches, bad_attachments, attachment_count) =
        match read_manifest(path) {
            Ok(manifest) => {
                let checksum_ok = file_checksum(path)? == manifest.checksum;
                let mut mismatches = Vec::new();
                for (table, expected) in &manifest.counts {
                    let found = actual
                        .iter()
                        .find(|(t, _)| t == table)
                        .map_or(-1, |(_, n)| *n);
                    if found != *expected {
                        mismatches.push((table.clone(), *expected, found));
                    }
                }

                // **D132** — and this is the check that makes a photograph a
                // promise rather than a hope. A file that is there but no longer
                // hashes the same is the failure nobody would otherwise see.
                let dir = backup_attachments_dir(path);
                let mut bad = Vec::new();
                for (name, bytes, checksum) in &manifest.attachments {
                    let file = dir.join(name);
                    let size = std::fs::metadata(&file).map(|m| m.len()).ok();
                    let matches = size == Some(*bytes)
                        && file_checksum(&file).map(|c| &c == checksum).unwrap_or(false);
                    if !matches {
                        bad.push(name.clone());
                    }
                }
                (checksum_ok, mismatches, bad, manifest.attachments.len())
            }
            // No manifest is itself a failure: an unverifiable backup is not a
            // backup, and quietly passing it is how a rumour becomes a policy.
            Err(_) => (false, Vec::new(), Vec::new(), 0),
        };

    Ok(VerifyReport {
        path: path.to_path_buf(),
        integrity_ok,
        foreign_keys_ok,
        checksum_ok,
        count_mismatches,
        schema_version,
        bad_attachments,
        attachment_count,
    })
}

/// What a restore did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub from: PathBuf,
    pub to: PathBuf,
    pub taken_at_ms: i64,
    pub restored_schema_version: u32,
    pub migrated_to: u32,
    pub safety_copy: Option<PathBuf>,
    /// True when the restored database failed its own verification, or could
    /// not be migrated forward, and the safety copy was put back.
    pub rolled_back: bool,
    /// Why, when `rolled_back`. Something a health panel can show.
    pub failure: Option<String>,
    pub counts: Vec<(String, i64)>,
}

/// Put a database file in place, taking its stale journal with it.
///
/// The `-wal` and `-shm` files belong to the database that WAS there, not to
/// the one arriving; leaving them lets SQLite replay a journal written against
/// a different file, which is corruption with extra steps.
///
/// **If the target cannot be replaced, this refuses rather than half-writing.**
/// On Windows an open file cannot be removed, so a restore aimed at a database
/// the app still has open fails here with something a person can read, instead
/// of overwriting the pages underneath a live connection and producing
/// "malformed database schema" ten seconds later. That is a real thing that
/// happened while writing this session's tests, and it is the reason the
/// sanctioned path is [`request_restore`] plus a restart, not a call from a
/// running app. On platforms where an open file *can* be unlinked the guard
/// does not fire — the flow is what protects you there, not the filesystem.
fn restore_files(src: &Path, dst: &Path) -> Result<(), DbError> {
    for suffix in ["-wal", "-shm"] {
        let mut side = dst.as_os_str().to_os_string();
        side.push(suffix);
        let side = PathBuf::from(side);
        if side.exists() && std::fs::remove_file(&side).is_err() {
            return Err(DbError::invariant(format!(
                "{} is still in use — close Magic Bill before restoring",
                dst.display()
            )));
        }
    }
    if dst.exists() && std::fs::remove_file(dst).is_err() {
        return Err(DbError::invariant(format!(
            "{} is still in use — close Magic Bill before restoring",
            dst.display()
        )));
    }
    std::fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| DbError::invariant(format!("could not write the restored database: {e}")))
}

/// Put a backup back.
///
/// **This takes paths, never a `&Db`, and that is the refusal expressed in
/// types.** The first draft of this session said restore "refuses to run while
/// the app holds the database open" and then left no way for a restore to ever
/// happen — the only program that can restore is the one that must not be
/// running. There is no overload here that accepts an open handle, so the
/// illegal thing is unrepresentable rather than checked, which is the same fix
/// P03 used for the order lifecycle.
///
/// The app therefore restores **at startup, before it opens the database** —
/// see [`request_restore`] and [`pending_restore`].
///
/// The order of the steps is the design:
///
/// 1. Verify the backup. Refuse before touching anything.
/// 2. Refuse a **newer** schema; migrate an **older** one forward, which is the
///    ordinary case after an update and not an exception.
/// 3. Take a safety copy of the current database, named so a human can find it.
/// 4. Restore.
/// 5. Verify the result.
/// 6. If step 5 fails, **put the safety copy back** — a half-restored shop is
///    worse than the broken one you started with.
///
/// The type-level half of that first paragraph, as a test rather than a claim —
/// there is no overload taking an open database:
///
/// ```compile_fail
/// # use std::path::Path;
/// # fn cannot_restore_over_a_live_database(db: &mb_db::Db, from: &Path) {
/// mb_db::backup::restore(from, db);
/// # }
/// ```
///
/// And the same thing spelled the way it is meant to be used, from a path:
///
/// ```no_run
/// # use std::path::Path;
/// # fn restore_at_startup(from: &Path, to: &Path) -> Result<(), mb_db::DbError> {
/// let report = mb_db::backup::restore(from, to)?;
/// assert!(!report.rolled_back);
/// # Ok(())
/// # }
/// ```
pub fn restore(from: &Path, to: &Path) -> Result<RestoreReport, DbError> {
    let report = verify(from)?;
    if !report.is_ok() {
        return Err(DbError::invariant(format!(
            "refusing to restore: {}",
            report.summary()
        )));
    }

    let known = migrate::latest_version();
    if report.schema_version > known {
        return Err(DbError::NewerSchema {
            found: report.schema_version,
            known,
        });
    }

    let manifest = read_manifest(from)?;

    // Step 3. Named for a human at 9 am, not for a machine.
    let safety_copy = if to.exists() {
        let name = format!(
            "before-restore-{}.db",
            to.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "shop".to_owned())
        );
        let path = to.with_file_name(name);
        std::fs::copy(to, &path).map_err(|e| {
            DbError::invariant(format!("could not take a safety copy before restoring: {e}"))
        })?;
        Some(path)
    } else {
        None
    };

    restore_files(from, to)?;

    // Step 2b, and this is correction (f): an OLDER backup is the ordinary
    // case, so bring it up to what this build expects, exactly as `Db::open`
    // would.
    //
    // **A failure here rolls back too**, which the first version of this
    // function did not do: it had already overwritten the target, so returning
    // an error left the shop holding a database that had been replaced and then
    // not migrated. A backup whose ledger is damaged is a real way to reach
    // this, and it is exactly the moment not to make things worse.
    let migrate_result = (|| -> Result<u32, DbError> {
        let mut conn = Connection::open(to).map_err(|source| DbError::Open {
            path: to.to_path_buf(),
            source,
        })?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate::apply_all(&mut conn)?;
        Ok(migrate::latest_version())
    })();

    let (migrated_to, failure) = match migrate_result {
        Ok(v) => (v, None),
        Err(e) => (report.schema_version, Some(e.to_string())),
    };

    // Step 5. Verified WITHOUT the manifest's counts, because migrating
    // forward may legitimately change them; integrity and foreign keys are the
    // checks that still mean something here.
    let structure_ok = failure.is_none() && verify_structure(to).unwrap_or(false);
    if !structure_ok {
        if let Some(safety) = &safety_copy {
            restore_files(safety, to)?;
        }
        return Ok(RestoreReport {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            taken_at_ms: manifest.taken_at_ms,
            restored_schema_version: report.schema_version,
            migrated_to,
            safety_copy,
            rolled_back: true,
            failure: Some(failure.unwrap_or_else(|| {
                "the restored database did not pass its own integrity check".to_owned()
            })),
            counts: Vec::new(),
        });
    }

    // **D132 — the photographs come back too**, and only once the database is
    // known good. They are copied and never moved, so a failed restore that
    // rolls back still leaves the backup complete.
    copy_attachments(&backup_attachments_dir(from), &attachments_dir(to))?;

    // The outbox knows nothing about what the cloud has seen since this backup
    // was taken. See `OutboxRepo::requeue_everything` for why the whole thing
    // is marked pending rather than half of it.
    let counts = {
        let conn = Connection::open(to).map_err(|source| DbError::Open {
            path: to.to_path_buf(),
            source,
        })?;
        conn.execute(
            "UPDATE sync_outbox SET synced_at = NULL, attempts = 0, last_error = NULL",
            [],
        )?;
        count_rows(&conn)?
    };

    Ok(RestoreReport {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        taken_at_ms: manifest.taken_at_ms,
        restored_schema_version: report.schema_version,
        migrated_to,
        safety_copy,
        rolled_back: false,
        failure: None,
        counts,
    })
}

/// Integrity and foreign keys only — used after a restore, where the manifest's
/// row counts may legitimately have changed by migrating forward.
fn verify_structure(path: &Path) -> Result<bool, DbError> {
    let conn = open_read_only(path)?;
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Ok(false);
    }
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let mut rows = stmt.query([])?;
    Ok(rows.next()?.is_none())
}

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

/// Keep 7 daily and 4 weekly, prune the rest.
///
/// **The newest is never pruned**, under any circumstance — not by a retention
/// bug, not by a clock that jumped backwards, which is why it is picked out
/// explicitly rather than trusted to fall out of the arithmetic.
pub fn prune(dir: &Path, now_ms: i64) -> Result<Vec<PathBuf>, DbError> {
    const DAY_MS: i64 = 86_400_000;
    const KEEP_DAILY: i64 = 7;
    const KEEP_WEEKLY: i64 = 4;

    let _ = now_ms;

    let mut backups = list(dir)?;
    if backups.is_empty() {
        return Ok(Vec::new());
    }
    // Newest first.
    backups.sort_by(|a, b| b.manifest.taken_at_ms.cmp(&a.manifest.taken_at_ms));

    let mut keep = std::collections::BTreeSet::new();
    // Rule zero, applied before anything else can reason its way out of it:
    // **the newest is never pruned.** Not by an off-by-one in the arithmetic
    // below, and not by a counter PC whose clock jumped backwards after a
    // reboot — which is why this does not depend on `now_ms` at all.
    keep.insert(backups[0].path.clone());

    // Counted, not windowed. "The last 7 daily and the last 4 weekly" is a
    // count of backups, and expressing it as an age window makes the answer
    // depend on how often the scheduler happened to fire.
    let daily_slots = usize::try_from(KEEP_DAILY).unwrap_or(0);
    let weekly_slots = usize::try_from(KEEP_WEEKLY).unwrap_or(0);
    let mut seen_days = std::collections::BTreeSet::new();
    let mut seen_weeks = std::collections::BTreeSet::new();
    for backup in &backups {
        let day = backup.manifest.taken_at_ms.div_euclid(DAY_MS);
        let week = day.div_euclid(7);

        let is_new_daily = seen_days.len() < daily_slots && seen_days.insert(day);
        let is_new_weekly = !is_new_daily
            && seen_weeks.len() < weekly_slots
            && !seen_days.contains(&day)
            && seen_weeks.insert(week);

        if is_new_daily || is_new_weekly {
            keep.insert(backup.path.clone());
        }
    }

    let mut pruned = Vec::new();
    for backup in backups {
        if keep.contains(&backup.path) {
            continue;
        }
        let _ = std::fs::remove_file(backup.manifest_path());
        std::fs::remove_file(&backup.path).map_err(|e| {
            DbError::invariant(format!(
                "could not prune {}: {e}",
                backup.path.display()
            ))
        })?;
        pruned.push(backup.path);
    }
    Ok(pruned)
}

/// Every backup in a folder, with its manifest. Files without a readable
/// manifest are skipped — they are not backups this program can offer.
pub fn list(dir: &Path) -> Result<Vec<Backup>, DbError> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => {
            return Err(DbError::invariant(format!(
                "could not read {}: {e}",
                dir.display()
            )));
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "db") {
            continue;
        }
        if let Ok(manifest) = read_manifest(&path) {
            out.push(Backup { path, manifest });
        }
    }
    out.sort_by(|a, b| a.manifest.taken_at_ms.cmp(&b.manifest.taken_at_ms));
    Ok(out)
}

// ---------------------------------------------------------------------------
// Restore-before-open: the seam P08 and P22 use
// ---------------------------------------------------------------------------

/// A restore the owner has asked for, waiting for the next launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRestore {
    pub from: PathBuf,
}

fn pending_path(config_dir: &Path) -> PathBuf {
    config_dir.join("pending-restore.txt")
}

/// Record that a restore should happen next time the app starts.
///
/// A plain file beside the config, deliberately: it must be readable and
/// removable **without opening SQLite**, because the whole point is that the
/// database may be the thing that is broken.
pub fn request_restore(config_dir: &Path, from: &Path) -> Result<(), DbError> {
    std::fs::create_dir_all(config_dir)
        .map_err(|e| DbError::invariant(format!("could not create {}: {e}", config_dir.display())))?;
    std::fs::write(pending_path(config_dir), from.to_string_lossy().as_bytes())
        .map_err(|e| DbError::invariant(format!("could not record the restore request: {e}")))
}

/// P08 calls this **before `Db::open`**. P22 owns the screen and the restart.
pub fn pending_restore(config_dir: &Path) -> Option<PendingRestore> {
    let text = std::fs::read_to_string(pending_path(config_dir)).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PendingRestore {
        from: PathBuf::from(trimmed),
    })
}

/// Clear the request once it has been carried out — success or failure. A
/// request that survives its own attempt is a boot loop.
pub fn clear_pending_restore(config_dir: &Path) -> Result<(), DbError> {
    match std::fs::remove_file(pending_path(config_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(DbError::invariant(format!(
            "could not clear the restore request: {e}"
        ))),
    }
}

// ---------------------------------------------------------------------------

fn open_read_only(path: &Path) -> Result<Connection, DbError> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|source| {
        DbError::Open {
            path: path.to_path_buf(),
            source,
        }
    })
}

fn read_schema_version(conn: &Connection) -> Result<u32, DbError> {
    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get(0),
    )?;
    u32::try_from(version).map_err(|_| DbError::OutOfRange {
        column: "schema_version.version",
        expected: "migration version",
    })
}

fn count_rows(conn: &Connection) -> Result<Vec<(String, i64)>, DbError> {
    let mut out = Vec::with_capacity(COUNTED.len());
    for table in COUNTED {
        let n: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        out.push(((*table).to_owned(), n));
    }
    Ok(out)
}

fn read_manifest(db: &Path) -> Result<Manifest, DbError> {
    let text = std::fs::read_to_string(manifest_path(db))
        .map_err(|e| DbError::invariant(format!("this backup has no manifest: {e}")))?;
    Manifest::parse(&text)
}

/// FNV-1a over the file, in 64 KB chunks.
///
/// The same reasoning as the migration checksum (R6): this is a tripwire
/// against corruption and truncation, not a security control, and nobody is
/// forging a backup they also control the manifest of. A cryptographic hash
/// would cost a dependency and buy nothing.
fn file_checksum(path: &Path) -> Result<String, DbError> {
    use std::io::Read;

    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut file = std::fs::File::open(path)
        .map_err(|e| DbError::invariant(format!("could not read {}: {e}", path.display())))?;
    let mut hash = OFFSET;
    let mut buf = vec![0_u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| DbError::invariant(format!("could not read {}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        for byte in &buf[..n] {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    Ok(format!("{hash:016x}"))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
