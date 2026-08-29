//! The phones this counter serves.

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
    /// Argon2, never the credential.
    pub secret_hash: String,
    pub staff_id: Option<StaffId>,
    pub paired_at: Timestamp,
    pub paired_by: Option<StaffId>,
    pub last_seen_at: Option<Timestamp>,
    pub last_ip: Option<String>,
    pub revoked_at: Option<Timestamp>,
    /// The phone's own install id (migration 0010). None for a row from before, or a till.
    pub install_id: Option<String>,
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
    pub fn all(&self, outlet: &str) -> Result<Vec<LanDevice>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT id, name, platform, secret_hash, staff_id, paired_at, paired_by,
                    last_seen_at, last_ip, revoked_at, install_id
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
                last_seen_at: row
                    .get::<_, Option<i64>>(7)?
                    .map(encode::timestamp_from_sql),
                last_ip: row.get(8)?,
                revoked_at: row
                    .get::<_, Option<i64>>(9)?
                    .map(encode::timestamp_from_sql),
                install_id: row.get(10)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// One LIVE device, by id.
    pub fn live(&self, outlet: &str, id: &str) -> Result<Option<LanDevice>, DbError> {
        Ok(self
            .all(outlet)?
            .into_iter()
            .find(|d| d.id == id && d.is_live()))
    }

    /// The row a phone already has here, by its install id — live or removed.
    pub fn by_install(&self, outlet: &str, install: &str) -> Result<Option<LanDevice>, DbError> {
        Ok(self
            .all(outlet)?
            .into_iter()
            .find(|d| d.install_id.as_deref() == Some(install)))
    }

    /// Let a phone in. The same id again is the same phone back: its row is rewritten, with a
    /// new credential, and it is live again.
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
                                      paired_at, paired_by, install_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (id) DO UPDATE SET name         = excluded.name,
                                            platform     = excluded.platform,
                                            secret_hash  = excluded.secret_hash,
                                            staff_id     = excluded.staff_id,
                                            paired_at    = excluded.paired_at,
                                            paired_by    = excluded.paired_by,
                                            install_id   = excluded.install_id,
                                            last_seen_at = NULL,
                                            last_ip      = NULL,
                                            revoked_at   = NULL,
                                            revoked_by   = NULL",
            rusqlite::params![
                device.id,
                outlet,
                device.name.trim(),
                device.platform,
                device.secret_hash,
                device.staff_id.as_ref().map(StaffId::as_str),
                encode::timestamp_to_sql(device.paired_at),
                paired_by.map(StaffId::as_str),
                device.install_id,
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
    pub fn seen(&self, outlet: &str, id: &str, at: Timestamp, ip: &str) -> Result<(), DbError> {
        self.tx.execute(
            "UPDATE lan_devices SET last_seen_at = ?3, last_ip = ?4
              WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, id, encode::timestamp_to_sql(at), ip],
        )?;
        Ok(())
    }
}

/// The stored hash, read back with its shape checked.
pub fn hash_of(device: &LanDevice) -> Result<PinHash, DbError> {
    PinHash::from_stored(&device.secret_hash)
        .map_err(|_| DbError::invariant("a device's stored credential is not readable"))
}
