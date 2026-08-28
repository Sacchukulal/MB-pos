//! Staff, roles and permissions.

use mb_auth::{Permission, PermissionSet, PinHash, RoleShape};
use mb_core::{Money, StaffId, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

/// A staff record is never deleted.
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

    pub fn from_sql(s: &str) -> Result<Self, DbError> {
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

/// A staff row as it comes down from the cloud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudStaff {
    pub id: String,
    pub role_id: Option<String>,
    pub name: String,
    pub code: Option<String>,
    pub phone: Option<String>,
    pub joined_on: Option<mb_core::BusinessDay>,
    pub status: StaffStatus,
    pub designation: Option<String>,
    pub department: Option<String>,
    pub is_rider: bool,
    pub employment_type: String,
    pub left_on: Option<mb_core::BusinessDay>,
    pub can_login_on_phone: bool,
    pub updated_at: Timestamp,
}

/// A role as it comes down from the cloud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudRole {
    pub id: String,
    pub name: String,
    pub is_builtin: bool,
    pub max_discount_bp: Option<i64>,
    pub max_discount_paise: Option<i64>,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffMember {
    pub id: StaffId,
    pub name: String,
    pub code: Option<String>,
    pub role_id: Option<String>,
    pub role_name: Option<String>,
    /// Argon2id, chosen and owned by `mb-auth`.
    pub pin_hash: Option<String>,
    pub status: StaffStatus,
    /// Every permission the role grants.
    pub permissions: PermissionSet,
    /// 12, from the role.
    pub max_discount_bp: Option<u32>,
    pub max_discount: Option<Money>,
}

impl StaffMember {
    /// The stored PIN, parsed — or `DbError::BadValue` if the column has been truncated or
    /// edited.
    pub fn pin(&self) -> Result<Option<PinHash>, DbError> {
        match &self.pin_hash {
            None => Ok(None),
            Some(stored) => PinHash::from_stored(stored)
                .map(Some)
                .map_err(|_| DbError::BadValue {
                    column: "staff.pin_hash",
                    value: "unreadable".to_owned(),
                }),
        }
    }
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

    /// Save a role and the exact set of permissions it grants.
    pub fn save_role(&self, outlet: &str, role: &RoleShape, at: Timestamp) -> Result<(), DbError> {
        let id = role.id.as_str();
        self.tx.execute(
            "INSERT INTO roles (id, outlet_id, name, is_builtin, max_discount_bp,
                                max_discount_paise)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET name               = excluded.name,
                                            max_discount_bp    = excluded.max_discount_bp,
                                            max_discount_paise = excluded.max_discount_paise",
            rusqlite::params![
                id,
                outlet,
                role.name,
                encode::bool_to_sql(role.is_builtin),
                role.max_discount_bp,
                role.max_discount.map(encode::money_to_sql),
            ],
        )?;
        self.tx
            .execute("DELETE FROM role_permissions WHERE role_id = ?1", [id])?;
        for code in role.permissions.codes() {
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

    /// A staff row the owner's phone wrote. Applied when it is newer than what is here; never
    /// queued back up, because the cloud already has it. Answers whether it was applied.
    pub fn apply_staff_from_cloud(&self, outlet: &str, row: &CloudStaff) -> Result<bool, DbError> {
        // A role that has not arrived yet (the cloud does the same): keep the person, the role
        // lands on the next pull.
        let role_id = match &row.role_id {
            Some(role) => {
                let known: bool = self.tx.query_row(
                    "SELECT EXISTS (SELECT 1 FROM roles WHERE id = ?1)",
                    [role],
                    |r| r.get(0),
                )?;
                known.then(|| role.clone())
            }
            None => None,
        };
        let n = self.tx.execute(
            "INSERT INTO staff (id, outlet_id, role_id, name, code, phone, joined_on, status, designation,
                                department, is_rider, employment_type, left_on, can_login_on_phone,
                                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
             ON CONFLICT (id) DO UPDATE SET role_id            = excluded.role_id,
                                            name               = excluded.name,
                                            code               = excluded.code,
                                            phone              = excluded.phone,
                                            joined_on          = excluded.joined_on,
                                            status             = excluded.status,
                                            designation        = excluded.designation,
                                            department         = excluded.department,
                                            is_rider           = excluded.is_rider,
                                            employment_type    = excluded.employment_type,
                                            left_on            = excluded.left_on,
                                            can_login_on_phone = excluded.can_login_on_phone,
                                            updated_at         = excluded.updated_at
             WHERE staff.updated_at < excluded.updated_at",
            rusqlite::params![
                row.id,
                outlet,
                role_id,
                row.name,
                row.code,
                row.phone,
                row.joined_on.map(encode::business_day_to_sql),
                row.status.as_sql(),
                row.designation,
                row.department,
                encode::bool_to_sql(row.is_rider),
                row.employment_type,
                row.left_on.map(encode::business_day_to_sql),
                encode::bool_to_sql(row.can_login_on_phone),
                encode::timestamp_to_sql(row.updated_at),
            ],
        )?;
        Ok(n > 0)
    }

    /// A PIN the owner set on the phone. The hash arrives as the phone made it.
    pub fn apply_pin_from_cloud(&self, staff_id: &str, pin_hash: &str) -> Result<bool, DbError> {
        if !pin_hash.starts_with("$argon2id$") {
            return Err(DbError::BadValue {
                column: "staff.pin_hash",
                value: "not an argon2id hash".to_owned(),
            });
        }
        let n = self.tx.execute(
            "UPDATE staff SET pin_hash = ?2 WHERE id = ?1 AND (pin_hash IS NULL OR pin_hash <> ?2)",
            rusqlite::params![staff_id, pin_hash],
        )?;
        Ok(n > 0)
    }

    /// A role the owner's phone wrote, with the exact permissions it grants. Codes this
    /// program does not know are dropped, not refused.
    pub fn apply_role_from_cloud(&self, outlet: &str, role: &CloudRole) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO roles (id, outlet_id, name, is_builtin, max_discount_bp, max_discount_paise)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET name               = excluded.name,
                                            max_discount_bp    = excluded.max_discount_bp,
                                            max_discount_paise = excluded.max_discount_paise",
            rusqlite::params![
                role.id,
                outlet,
                role.name,
                encode::bool_to_sql(role.is_builtin),
                role.max_discount_bp,
                role.max_discount_paise,
            ],
        )?;
        self.tx
            .execute("DELETE FROM role_permissions WHERE role_id = ?1", [&role.id])?;
        for code in &role.permissions {
            if Permission::from_code(code).is_err() {
                continue;
            }
            self.tx.execute(
                "INSERT INTO role_permissions (role_id, permission_code) VALUES (?1, ?2)",
                rusqlite::params![role.id, code],
            )?;
        }
        Ok(())
    }

    pub fn list_staff(&self, outlet: &str) -> Result<Vec<StaffMember>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT s.id, s.name, s.code, s.role_id, r.name, s.pin_hash, s.status,
                    r.max_discount_bp, r.max_discount_paise
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
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<i64>>(8)?,
            ))
        })?;

        let mut staff = Vec::new();
        for row in rows {
            let (id, name, code, role_id, role_name, pin_hash, status, max_bp, max_paise) = row?;
            let permissions = match &role_id {
                Some(role) => self.permissions_for_role(role)?,
                None => PermissionSet::new(),
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
                max_discount_bp: max_bp.map(|bp| u32::try_from(bp).unwrap_or(0)),
                max_discount: max_paise.map(encode::money_from_sql),
            });
        }
        Ok(staff)
    }

    /// One person, by id — what a login needs, and the reason it is a query rather than a
    /// filter over `Self::list_staff`.
    pub fn find_staff(&self, outlet: &str, id: &str) -> Result<Option<StaffMember>, DbError> {
        Ok(self
            .list_staff(outlet)?
            .into_iter()
            .find(|s| s.id.as_str() == id))
    }

    /// Every role a shop has, for the roles screen.
    pub fn list_roles(&self, outlet: &str) -> Result<Vec<RoleShape>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, is_builtin, max_discount_bp, max_discount_paise
               FROM roles WHERE outlet_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?;

        let mut roles = Vec::new();
        for row in rows {
            let (id, name, builtin, max_bp, max_paise) = row?;
            let permissions = self.permissions_for_role(&id)?;
            roles.push(RoleShape {
                id,
                name,
                is_builtin: encode::bool_from_sql(builtin, "roles.is_builtin")?,
                permissions,
                max_discount_bp: max_bp.map(|bp| u32::try_from(bp).unwrap_or(0)),
                max_discount: max_paise.map(encode::money_from_sql),
            });
        }
        Ok(roles)
    }

    /// How many active people can still administer this shop.
    pub fn active_administrators(&self, outlet: &str) -> Result<Vec<StaffId>, DbError> {
        Ok(self
            .list_staff(outlet)?
            .into_iter()
            .filter(|s| {
                s.status == StaffStatus::Active && s.permissions.has(Permission::StaffManage)
            })
            .map(|s| s.id)
            .collect())
    }

    fn permissions_for_role(&self, role_id: &str) -> Result<PermissionSet, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT permission_code FROM role_permissions WHERE role_id = ?1 ORDER BY 1",
        )?;
        let rows = stmt.query_map([role_id], |row| row.get::<_, String>(0))?;
        let mut codes = Vec::new();
        for row in rows {
            codes.push(row?);
        }
        // BACKEND-G7's other half. A code the program does not know is an error at the row, not
        // a permission quietly dropped from the set — which is what a silent "denied" looked
        // like from behind the counter.
        PermissionSet::from_codes(codes).map_err(|e| DbError::BadValue {
            column: "role_permissions.permission_code",
            value: e.to_string(),
        })
    }
}
