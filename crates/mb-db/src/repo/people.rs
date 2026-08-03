//! Staff, roles and permissions.
//!
//! **Permissions come back as a set of codes that exist as rows.** That is
//! BACKEND-G7: v1's permission map was free-form, "any key can be written; a
//! typo in a permission name silently means 'denied'". Here a permission that
//! is not a row cannot be granted — the foreign key refuses it — so this
//! repository can hand P11 a set and P11 can trust it.

use std::collections::BTreeSet;

use mb_core::{StaffId, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

/// Scope 9.15: a staff record is never deleted. Someone who left in March is
/// still on March's bills, March's audit trail and March's payroll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaffStatus {
    Active,
    Suspended,
    Left,
}

impl StaffStatus {
    const fn as_sql(self) -> &'static str {
        match self {
            StaffStatus::Active => "active",
            StaffStatus::Suspended => "suspended",
            StaffStatus::Left => "left",
        }
    }

    fn from_sql(s: &str) -> Result<Self, DbError> {
        match s {
            "active" => Ok(StaffStatus::Active),
            "suspended" => Ok(StaffStatus::Suspended),
            "left" => Ok(StaffStatus::Left),
            other => Err(DbError::BadValue {
                column: "staff.status",
                value: other.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffMember {
    pub id: StaffId,
    pub name: String,
    pub code: Option<String>,
    pub role_id: Option<String>,
    pub role_name: Option<String>,
    /// P11 chooses the hashing algorithm. This crate stores and returns the
    /// string and forms no opinion about it.
    pub pin_hash: Option<String>,
    pub status: StaffStatus,
    /// Every permission the role grants, as codes that exist in `permissions`.
    pub permissions: BTreeSet<String>,
}

#[derive(Debug)]
pub struct PeopleRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> PeopleRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        PeopleRepo { tx }
    }

    /// Every permission this build knows, straight from the seeded table.
    ///
    /// P11 builds its role screen from this and from nothing else, so a
    /// permission it can offer is a permission that exists.
    pub fn permission_codes(&self) -> Result<Vec<(String, String)>, DbError> {
        let mut stmt = self
            .tx
            .prepare_cached("SELECT code, description FROM permissions ORDER BY code")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn save_role(
        &self,
        outlet: &str,
        id: &str,
        name: &str,
        builtin: bool,
        permissions: &[&str],
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO roles (id, outlet_id, name, is_builtin) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name",
            rusqlite::params![id, outlet, name, encode::bool_to_sql(builtin)],
        )?;
        self.tx
            .execute("DELETE FROM role_permissions WHERE role_id = ?1", [id])?;
        for code in permissions {
            // A typo here is a foreign-key violation, not a silent denial.
            self.tx
                .execute(
                    "INSERT INTO role_permissions (role_id, permission_code) VALUES (?1, ?2)",
                    rusqlite::params![id, code],
                )
                .map_err(|e| match e {
                    rusqlite::Error::SqliteFailure(f, _)
                        if f.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        DbError::invariant(format!(
                            "\"{code}\" is not a permission this program has — \
                             see the permissions table for the list"
                        ))
                    }
                    other => DbError::Sqlite(other),
                })?;
        }
        OutboxRepo::new(self.tx).enqueue(outlet, "roles", id, Op::Upsert, at)
    }

    pub fn save_staff(
        &self,
        outlet: &str,
        staff: &StaffMember,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO staff (id, outlet_id, role_id, name, code, pin_hash, status,
                                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT (id) DO UPDATE SET role_id    = excluded.role_id,
                                            name       = excluded.name,
                                            code       = excluded.code,
                                            pin_hash   = excluded.pin_hash,
                                            status     = excluded.status,
                                            updated_at = excluded.updated_at",
            rusqlite::params![
                staff.id.as_str(),
                outlet,
                staff.role_id,
                staff.name,
                staff.code,
                staff.pin_hash,
                staff.status.as_sql(),
                encode::timestamp_to_sql(at),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "staff", staff.id.as_str(), Op::Upsert, at)
    }

    pub fn list_staff(&self, outlet: &str) -> Result<Vec<StaffMember>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT s.id, s.name, s.code, s.role_id, r.name, s.pin_hash, s.status
               FROM staff s LEFT JOIN roles r ON r.id = s.role_id
              WHERE s.outlet_id = ?1 ORDER BY s.name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;

        let mut staff = Vec::new();
        for row in rows {
            let (id, name, code, role_id, role_name, pin_hash, status) = row?;
            let permissions = match &role_id {
                Some(role) => self.permissions_for_role(role)?,
                None => BTreeSet::new(),
            };
            staff.push(StaffMember {
                id: StaffId::new(id),
                name,
                code,
                role_id,
                role_name,
                pin_hash,
                status: StaffStatus::from_sql(&status)?,
                permissions,
            });
        }
        Ok(staff)
    }

    fn permissions_for_role(&self, role_id: &str) -> Result<BTreeSet<String>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT permission_code FROM role_permissions WHERE role_id = ?1 ORDER BY 1",
        )?;
        let rows = stmt.query_map([role_id], |row| row.get::<_, String>(0))?;
        let mut out = BTreeSet::new();
        for row in rows {
            out.insert(row?);
        }
        Ok(out)
    }
}
