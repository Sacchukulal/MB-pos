//! The day file: a shop's permanent history, one gzip of JSON lines per business day.
//!
//! The counter's SQLite is the truth and keeps every bill forever. The cloud's database holds
//! only the last few unsealed days; everything older lives in the cloud's Storage as one file
//! per day, built HERE from the rows, uploaded once by the sender, and replaced only when a
//! bill of that day changed after the upload. Both restores (a new PC, a new phone) read the
//! same file with the parsers each app already has.
//!
//! One rule, one function: `seal_state` says whether a day is sealed; `mark_dirty` is called
//! at the one place every bill save passes through; `pending` lists what has to go up;
//! `day_file` builds the bytes; `restore_line` reads one line back. Nothing here is a second
//! serialiser — every line is the wire row `mb_push` would send, from the same builders.

use std::io::Write as _;

use mb_core::{BusinessDay, Timestamp};
use rusqlite::{OptionalExtension as _, Transaction};
use serde_json::{Value, json};

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::OutboxRow;
use crate::repo::reports::SalesBy;
use crate::repo::wire::{RestoreReport, WireRepo, WireRow};

/// A day seals itself this many days after it ended, unless it was locked sooner.
pub const SEAL_AFTER_DAYS: i32 = 3;

/// The first line of every day file says what it is.
pub const HEADER_KIND: &str = "day";

/// The cloud's bucket for day files.
pub const BUCKET: &str = "bill-archives";

/// Where a day is: still changing, or sealed and archivable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealState {
    Open,
    Sealed,
}

/// A day's ledger row, as Health shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayLedger {
    pub day: BusinessDay,
    pub sealed_at: Option<Timestamp>,
    pub dirty_at: Option<Timestamp>,
    pub uploaded_at: Option<Timestamp>,
    pub sha256: Option<String>,
    pub bills: i64,
    pub bytes: i64,
    pub last_error: Option<String>,
}

/// The bytes of one day, ready to go up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayFile {
    pub day: BusinessDay,
    /// gzip of UTF-8 JSON lines.
    pub gz: Vec<u8>,
    /// SHA-256 of `gz`, lowercase hex.
    pub sha256: String,
    pub bills: usize,
}

impl DayFile {
    /// The object key: `{restaurant_id}/{yyyy}/{yyyy-mm-dd}.jsonl.gz`. Derived, never stored.
    #[must_use]
    pub fn key(restaurant_id: &str, day: BusinessDay) -> String {
        format!("{restaurant_id}/{}/{day}.jsonl.gz", day.to_ymd().0)
    }

    /// The day a file's name stands for, or `None` for a name that is not one.
    #[must_use]
    pub fn day_of_key(name: &str) -> Option<BusinessDay> {
        name.rsplit('/')
            .next()?
            .strip_suffix(".jsonl.gz")?
            .parse()
            .ok()
    }
}

#[derive(Debug)]
pub struct ArchiveRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> ArchiveRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        ArchiveRepo { tx }
    }

    /// The one rule: a day is sealed when it is locked, or when `SEAL_AFTER_DAYS` have passed
    /// since it ended.
    pub fn seal_state(&self, outlet: &str, day: BusinessDay, today: BusinessDay) -> Result<SealState, DbError> {
        if day.days_until(today) >= SEAL_AFTER_DAYS {
            return Ok(SealState::Sealed);
        }
        let locked = crate::repo::days::DaysRepo::new(self.tx).is_locked(outlet, day)?;
        Ok(if locked { SealState::Sealed } else { SealState::Open })
    }

    pub fn is_sealed(&self, outlet: &str, day: BusinessDay, today: BusinessDay) -> Result<bool, DbError> {
        Ok(self.seal_state(outlet, day, today)? == SealState::Sealed)
    }

    /// The day an outbox entry belongs to, when it belongs to one: a bill's day, or a totals
    /// row's. The sender asks this to route a sealed day's rows into the file instead of the
    /// push.
    pub fn day_of_entry(&self, entry: &OutboxRow) -> Result<Option<BusinessDay>, DbError> {
        match entry.table_name.as_str() {
            "orders" => {
                let day: Option<i64> = self
                    .tx
                    .query_row("SELECT business_day FROM orders WHERE id = ?1", [&entry.row_id], |r| r.get(0))
                    .optional()?;
                day.map(|d| encode::business_day_from_sql(d, "orders.business_day")).transpose()
            }
            "day_item_totals" | "day_category_totals" => entry
                .row_id
                .parse::<i64>()
                .ok()
                .map(|d| encode::business_day_from_sql(d, "sync_outbox.row_id"))
                .transpose(),
            _ => Ok(None),
        }
    }

    /// A bill, refund or revert of this day was just saved. Called where the save happens.
    pub fn mark_dirty(&self, outlet: &str, day: BusinessDay, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO archive_days (outlet_id, business_day, dirty_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (outlet_id, business_day) DO UPDATE SET dirty_at = excluded.dirty_at",
            rusqlite::params![outlet, encode::business_day_to_sql(day), encode::timestamp_to_sql(at)],
        )?;
        Ok(())
    }

    /// Sealed days whose file has to go up: never uploaded, or dirty since. Oldest first, so a
    /// shop back from two months offline sends its history in order. Days are read off the
    /// bills themselves, so a shop that billed before this ledger existed is archived whole.
    pub fn pending(&self, outlet: &str, today: BusinessDay, limit: usize) -> Result<Vec<BusinessDay>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT DISTINCT o.business_day
               FROM orders o
               LEFT JOIN archive_days a ON a.outlet_id = o.outlet_id AND a.business_day = o.business_day
               LEFT JOIN business_days d ON d.outlet_id = o.outlet_id AND d.business_day = o.business_day
              WHERE o.outlet_id = ?1
                AND (o.state IN ('settled', 'voided') OR (o.state = 'cancelled' AND o.bill_number_value IS NOT NULL))
                AND (a.uploaded_at IS NULL OR a.dirty_at > a.uploaded_at)
                AND (o.business_day <= ?2 OR d.is_locked = 1)
              ORDER BY o.business_day
              LIMIT ?3",
        )?;
        let sealed_before = encode::business_day_to_sql(today).saturating_sub(i64::from(SEAL_AFTER_DAYS));
        let rows = stmt.query_map(
            rusqlite::params![outlet, sealed_before, i64::try_from(limit).unwrap_or(i64::MAX)],
            |r| r.get::<_, i64>(0),
        )?;
        let mut out = Vec::new();
        for day in rows {
            out.push(encode::business_day_from_sql(day?, "orders.business_day")?);
        }
        Ok(out)
    }

    /// How many days are waiting, for Health.
    pub fn pending_count(&self, outlet: &str, today: BusinessDay) -> Result<usize, DbError> {
        Ok(self.pending(outlet, today, usize::MAX)?.len())
    }

    /// The newest thing that went wrong, for Health.
    pub fn last_error(&self, outlet: &str) -> Result<Option<String>, DbError> {
        Ok(self
            .tx
            .query_row(
                "SELECT last_error FROM archive_days WHERE outlet_id = ?1 AND last_error IS NOT NULL
                  ORDER BY business_day DESC LIMIT 1",
                [outlet],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn ledger(&self, outlet: &str, day: BusinessDay) -> Result<Option<DayLedger>, DbError> {
        self.tx
            .query_row(
                "SELECT business_day, sealed_at, dirty_at, uploaded_at, sha256, bills, bytes, last_error
                   FROM archive_days WHERE outlet_id = ?1 AND business_day = ?2",
                rusqlite::params![outlet, encode::business_day_to_sql(day)],
                |r| {
                    Ok(DayLedger {
                        day: BusinessDay::from_days_since_epoch(i32::try_from(r.get::<_, i64>(0)?).unwrap_or(0)),
                        sealed_at: r.get::<_, Option<i64>>(1)?.map(encode::timestamp_from_sql),
                        dirty_at: r.get::<_, Option<i64>>(2)?.map(encode::timestamp_from_sql),
                        uploaded_at: r.get::<_, Option<i64>>(3)?.map(encode::timestamp_from_sql),
                        sha256: r.get(4)?,
                        bills: r.get(5)?,
                        bytes: r.get(6)?,
                        last_error: r.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(DbError::from)
    }

    /// Forget which files went up: a new licence is a new shop in the cloud, and every day
    /// file goes up again under it.
    pub fn forget_uploads(&self, outlet: &str) -> Result<usize, DbError> {
        Ok(self.tx.execute(
            "UPDATE archive_days SET uploaded_at = NULL, sha256 = NULL, last_error = NULL WHERE outlet_id = ?1",
            [outlet],
        )?)
    }

    /// The file went up.
    pub fn record_upload(&self, outlet: &str, file: &DayFile, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO archive_days (outlet_id, business_day, sealed_at, uploaded_at, sha256, bills, bytes, last_error)
             VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, NULL)
             ON CONFLICT (outlet_id, business_day) DO UPDATE SET
                 sealed_at   = COALESCE(archive_days.sealed_at, excluded.sealed_at),
                 uploaded_at = excluded.uploaded_at,
                 sha256      = excluded.sha256,
                 bills       = excluded.bills,
                 bytes       = excluded.bytes,
                 last_error  = NULL",
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(file.day),
                encode::timestamp_to_sql(at),
                file.sha256,
                i64::try_from(file.bills).unwrap_or(i64::MAX),
                i64::try_from(file.gz.len()).unwrap_or(i64::MAX),
            ],
        )?;
        Ok(())
    }

    /// The file did not go up, and why. The day stays pending.
    pub fn record_failure(&self, outlet: &str, day: BusinessDay, why: &str) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO archive_days (outlet_id, business_day, last_error) VALUES (?1, ?2, ?3)
             ON CONFLICT (outlet_id, business_day) DO UPDATE SET last_error = excluded.last_error",
            rusqlite::params![outlet, encode::business_day_to_sql(day), why],
        )?;
        Ok(())
    }

    /// Build the day's file from the rows: the header line, then one wire row per line —
    /// bills by `created_at` then id, each followed by its refunds and reverts. Deterministic,
    /// so the same day always gives the same bytes and the same hash.
    pub fn day_file(&self, outlet: &str, day: BusinessDay, sealed_at: Timestamp, app_version: &str) -> Result<DayFile, DbError> {
        let wire = WireRepo::new(self.tx);
        let key = encode::business_day_to_sql(day).to_string();

        let mut stmt = self.tx.prepare_cached(
            "SELECT id FROM orders
              WHERE outlet_id = ?1 AND business_day = ?2
                AND (state IN ('settled', 'voided') OR (state = 'cancelled' AND bill_number_value IS NOT NULL))
              ORDER BY created_at, id",
        )?;
        let ids = stmt
            .query_map(rusqlite::params![outlet, encode::business_day_to_sql(day)], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut lines: Vec<WireRow> = Vec::with_capacity(ids.len());
        let mut bills = 0_usize;
        for id in &ids {
            let Some(bill) = wire.order_row(outlet, id)? else {
                continue;
            };
            bills += 1;
            lines.push(bill);
            for (table, sql) in [
                ("refunds", "SELECT id FROM refunds WHERE order_id = ?1 ORDER BY refunded_at, id"),
                ("bill_reverts", "SELECT id FROM bill_reverts WHERE order_id = ?1 ORDER BY reverted_at, id"),
            ] {
                let mut stmt = self.tx.prepare_cached(sql)?;
                let children = stmt
                    .query_map([id], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                for child in children {
                    lines.extend(wire.whole_rows(table, &child, sealed_at)?);
                }
            }
        }

        let header = json!({
            "$kind": HEADER_KIND,
            "business_day": encode::business_day_to_sql(day),
            "schema": crate::migrate::latest_version(),
            "app_version": app_version,
            "sealed_at": sealed_at.millis(),
            "bills": bills,
            "day_totals": wire.day_totals(outlet, &key, sealed_at)?.map(|r| r.data).unwrap_or(Value::Null),
            "day_item_totals": wire.day_group_totals(outlet, &key, sealed_at, SalesBy::Item)?.into_iter().map(|r| r.data).collect::<Vec<_>>(),
            "day_category_totals": wire.day_group_totals(outlet, &key, sealed_at, SalesBy::Category)?.into_iter().map(|r| r.data).collect::<Vec<_>>(),
        });

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut write_line = |line: &Value| -> Result<(), DbError> {
            serde_json::to_writer(&mut encoder, line).map_err(|e| DbError::invariant(e.to_string()))?;
            encoder.write_all(b"\n").map_err(|e| DbError::invariant(e.to_string()))
        };
        write_line(&header)?;
        for line in &lines {
            write_line(&line.to_json())?;
        }
        let gz = encoder.finish().map_err(|e| DbError::invariant(e.to_string()))?;
        let sha256 = hex(&mb_auth::audit::sha256(&gz));
        Ok(DayFile { day, gz, sha256, bills })
    }

    /// One line of a day file as the wire row it is. The header's totals become the day's
    /// `day_totals` row (the item and category totals inside it are the phone's; nothing on
    /// the counter reads them). This is the only reader of a day file.
    pub fn row_of_line(line: &str) -> Result<WireRow, DbError> {
        let value: Value = serde_json::from_str(line)
            .map_err(|e| DbError::invariant(format!("a day file line would not read: {e}")))?;
        if value.get("$kind").and_then(Value::as_str) == Some(HEADER_KIND) {
            let totals = value.get("day_totals").cloned().unwrap_or(Value::Null);
            let day = totals.get("business_day").and_then(Value::as_i64).unwrap_or(0);
            return Ok(WireRow {
                table: "day_totals".to_owned(),
                id: day.to_string(),
                updated_at: Timestamp::from_millis(value.get("sealed_at").and_then(Value::as_i64).unwrap_or(0)),
                deleted: !totals.is_object(),
                data: totals,
            });
        }
        WireRow::from_json(&value)
    }

    /// A whole day file's text back into this database, line by line, through the same
    /// ordered writer a cloud restore uses. A line that will not read is counted and named,
    /// never fatal. The caller has already gunzipped.
    pub fn restore_text(&self, outlet: &str, text: &str, report: &mut RestoreReport) -> Result<(), DbError> {
        let mut rows = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            match Self::row_of_line(line) {
                Ok(row) => rows.push(row),
                Err(e) => {
                    report.skipped = report.skipped.saturating_add(1);
                    report.failed.push(format!("line {}…: {e}", line.chars().take(40).collect::<String>()));
                }
            }
        }
        WireRepo::new(self.tx).restore_rows(outlet, rows, report)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A day file's text back out of its gzip. The one place a day file is unpacked.
pub fn gunzip(gz: &[u8]) -> Result<String, DbError> {
    use std::io::Read as _;
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .map_err(|e| DbError::invariant(format!("a day file would not unpack: {e}")))?;
    Ok(text)
}
