//! **The phones this counter serves** — P19, decision D9.
//!
//! One table, four operations, and one rule that shapes all of them:
//! **a revoked device is not deleted.** D47 again — a correction is a state and
//! never a deletion — because an owner asking *"which phone was that, and who
//! took it off?"* three months later needs the row to still be there.
//!
//! It lives here rather than in `mb-lan` because `mb-lan` writes no SQL and
//! knows no business rule; and rather than in `magic-bill` because that crate
//! writes no SQL either (audit E3).

use mb_auth::PinHash;
use mb_core::{StaffId, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

/// One phone, as it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanDevice {
    pub id: String,
    pub name: String,
    pub platform: String,
    /// **Argon2, never the credential.** This database is copied to a pen drive
    /// on purpose (P05).
    pub secret_hash: String,
    pub staff_id: Option<StaffId>,
    pub paired_at: Timestamp,
    pub paired_by: Option<StaffId>,
    pub last_seen_at: Option<Timestamp>,
    pub last_ip: Option<String>,
    pub revoked_at: Option<Timestamp>,
}

impl LanDevice {
    #[must_use]
    pub const fn is_live(&self) -> bool {
        self.revoked_at.is_none()
    }
}

#[derive(Debug)]
pub struct DevicesRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> DevicesRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        DevicesRepo { tx }
    }

    /// Every device, revoked ones last.
    ///
    /// The panel shows both: a revoked phone that has disappeared from the list
    /// is a question an owner cannot answer.
    pub fn all(&self, outlet: &str) -> Result<Vec<LanDevice>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT id, name, platform, secret_hash, staff_id, paired_at, paired_by,
                    last_seen_at, last_ip, revoked_at
               FROM lan_devices WHERE outlet_id = ?1
           ORDER BY revoked_at IS NOT NULL, paired_at DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![outlet], |row| {
            Ok(LanDevice {
                id: row.get(0)?,
                name: row.get(1)?,
                platform: row.get(2)?,
                secret_hash: row.get(3)?,
                staff_id: row.get::<_, Option<String>>(4)?.map(StaffId::new),
                paired_at: encode::timestamp_from_sql(row.get(5)?),
                paired_by: row.get::<_, Option<String>>(6)?.map(StaffId::new),
                last_seen_at: row.get::<_, Option<i64>>(7)?.map(encode::timestamp_from_sql),
                last_ip: row.get(8)?,
                revoked_at: row.get::<_, Option<i64>>(9)?.map(encode::timestamp_from_sql),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// One LIVE device, by id.
    ///
    /// **Filtered on `revoked_at IS NULL` in the SQL, not by the caller.** That
    /// is the whole of T3: the authentication path asks for a live device, so a
    /// revocation bites on the next request rather than the next login, and
    /// there is no way to forget the filter at a call site.
    pub fn live(&self, outlet: &str, id: &str) -> Result<Option<LanDevice>, DbError> {
        Ok(self
            .all(outlet)?
            .into_iter()
            .find(|d| d.id == id && d.is_live()))
    }

    /// Let a phone in.
    pub fn pair(
        &self,
        outlet: &str,
        device: &LanDevice,
        paired_by: Option<&StaffId>,
    ) -> Result<(), DbError> {
        if device.name.trim().is_empty() {
            return Err(DbError::invariant(
                "a device needs a name — it is what the person approving it sees",
            ));
        }
        self.tx.execute(
            "INSERT INTO lan_devices (id, outlet_id, name, platform, secret_hash, staff_id,
                                      paired_at, paired_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                device.id,
                outlet,
                device.name.trim(),
                device.platform,
                device.secret_hash,
                device.staff_id.as_ref().map(StaffId::as_str),
                encode::timestamp_to_sql(device.paired_at),
                paired_by.map(StaffId::as_str),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "lan_devices",
            &device.id,
            Op::Upsert,
            device.paired_at,
        )
    }

    /// Take a phone off the counter.
    ///
    /// An UPDATE and never a DELETE. Returns false when there was nothing live
    /// to revoke, so the screen can say "that phone was already removed"
    /// instead of pretending it did something.
    pub fn revoke(
        &self,
        outlet: &str,
        id: &str,
        at: Timestamp,
        by: Option<&StaffId>,
    ) -> Result<bool, DbError> {
        let changed = self.tx.execute(
            "UPDATE lan_devices SET revoked_at = ?3, revoked_by = ?4
              WHERE outlet_id = ?1 AND id = ?2 AND revoked_at IS NULL",
            rusqlite::params![
                outlet,
                id,
                encode::timestamp_to_sql(at),
                by.map(StaffId::as_str),
            ],
        )?;
        if changed > 0 {
            OutboxRepo::new(self.tx).enqueue(outlet, "lan_devices", id, Op::Upsert, at)?;
        }
        Ok(changed > 0)
    }

    /// "Last seen", for the panel.
    ///
    /// **No outbox row**, deliberately: this is written on the phone's traffic,
    /// and D16's free-tier budget does not survive a sync row per poll. The
    /// print spool made the same call for the same reason.
    pub fn seen(
        &self,
        outlet: &str,
        id: &str,
        at: Timestamp,
        ip: &str,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "UPDATE lan_devices SET last_seen_at = ?3, last_ip = ?4
              WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, id, encode::timestamp_to_sql(at), ip],
        )?;
        Ok(())
    }
}

/// The stored hash, read back with its shape checked.
///
/// # Errors
///
/// When the column is not a complete Argon2id hash — which must be a locked
/// door and never an open one, exactly as it is for a PIN.
pub fn hash_of(device: &LanDevice) -> Result<PinHash, DbError> {
    PinHash::from_stored(&device.secret_hash)
        .map_err(|_| DbError::invariant("a device's stored credential is not readable"))
}
