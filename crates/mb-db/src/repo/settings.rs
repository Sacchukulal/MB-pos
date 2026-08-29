//! Typed settings, the store profile, and printers.

use mb_core::{Money, PriceBasis, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

/// The five things a setting can be.
pub trait SettingValue: Sized {
    const TYPE_TAG: &'static str;
    fn to_setting(&self) -> String;
    fn from_setting(raw: &str) -> Result<Self, DbError>;
}

impl SettingValue for i64 {
    const TYPE_TAG: &'static str = "int";
    fn to_setting(&self) -> String {
        self.to_string()
    }
    fn from_setting(raw: &str) -> Result<Self, DbError> {
        raw.parse().map_err(|_| DbError::BadValue {
            column: "settings.value",
            value: raw.to_owned(),
        })
    }
}

impl SettingValue for bool {
    const TYPE_TAG: &'static str = "bool";
    fn to_setting(&self) -> String {
        i64::from(*self).to_string()
    }
    fn from_setting(raw: &str) -> Result<Self, DbError> {
        match raw {
            "0" => Ok(false),
            "1" => Ok(true),
            other => Err(DbError::BadValue {
                column: "settings.value",
                value: other.to_owned(),
            }),
        }
    }
}

impl SettingValue for String {
    const TYPE_TAG: &'static str = "text";
    fn to_setting(&self) -> String {
        self.clone()
    }
    fn from_setting(raw: &str) -> Result<Self, DbError> {
        Ok(raw.to_owned())
    }
}

/// Paise, like everywhere else.
impl SettingValue for Money {
    const TYPE_TAG: &'static str = "money";
    fn to_setting(&self) -> String {
        self.paise().to_string()
    }
    fn from_setting(raw: &str) -> Result<Self, DbError> {
        raw.parse::<i64>()
            .map(Money::from_paise)
            .map_err(|_| DbError::BadValue {
                column: "settings.value",
                value: raw.to_owned(),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreProfile {
    pub name: String,
    pub address: String,
    pub phone: Option<String>,
    pub gstin: Option<String>,
    pub fssai: Option<String>,
    pub state_code: Option<String>,
    pub upi_id: Option<String>,
    pub upi_merchant_name: Option<String>,
    pub upi_reference: Option<String>,
    /// `unregistered`, `composition` or `regular`.
    pub registration: String,
    /// Whether a menu price already contains its tax, unless a slab or an item says otherwise.
    pub price_basis: PriceBasis,
}

/// 11 lives on this: `offset_x_mm` / `offset_y_mm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printer {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub address: Option<String>,
    pub paper_mm: i64,
    pub is_default: bool,
    pub can_kick_drawer: bool,
    pub offset_x_mm: i64,
    pub offset_y_mm: i64,
    /// `bill`, `kitchen` or `both`.
    pub role: String,
    /// `raster` or `text`.
    pub engine: String,
    pub is_bold_dark: bool,
}

#[derive(Debug)]
pub struct SettingsRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> SettingsRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        SettingsRepo { tx }
    }

    /// `None` means nobody has set it.
    pub fn get<T: SettingValue>(&self, outlet: &str, key: &str) -> Result<Option<T>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT value, value_type FROM settings WHERE outlet_id = ?1 AND key = ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![outlet, key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let value: String = row.get(0)?;
        let stored_type: String = row.get(1)?;
        if stored_type != T::TYPE_TAG {
            return Err(DbError::invariant(format!(
                "setting \"{key}\" is stored as {stored_type} and was read as {} — \
                 one of the two is wrong, and guessing would be worse",
                T::TYPE_TAG
            )));
        }
        T::from_setting(&value).map(Some)
    }

    pub fn set<T: SettingValue>(
        &self,
        outlet: &str,
        key: &str,
        value: &T,
        at: Timestamp,
        by: Option<&str>,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO settings (outlet_id, key, value, value_type, updated_at, updated_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (outlet_id, key) DO UPDATE SET value      = excluded.value,
                                                        value_type = excluded.value_type,
                                                        updated_at = excluded.updated_at,
                                                        updated_by = excluded.updated_by",
            rusqlite::params![
                outlet,
                key,
                value.to_setting(),
                T::TYPE_TAG,
                encode::timestamp_to_sql(at),
                by,
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "settings", key, Op::Upsert, at)
    }

    pub fn save_store_profile(
        &self,
        outlet: &str,
        profile: &StoreProfile,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO store_profile (outlet_id, name, address, phone, gstin, fssai, state_code,
                                        upi_id, upi_merchant_name, upi_reference, registration,
                                        price_basis, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?13, ?12)
             ON CONFLICT (outlet_id) DO UPDATE SET name              = excluded.name,
                                                   address           = excluded.address,
                                                   phone             = excluded.phone,
                                                   gstin             = excluded.gstin,
                                                   fssai             = excluded.fssai,
                                                   state_code        = excluded.state_code,
                                                   upi_id            = excluded.upi_id,
                                                   upi_merchant_name = excluded.upi_merchant_name,
                                                   upi_reference     = excluded.upi_reference,
                                                   registration      = excluded.registration,
                                                   price_basis       = excluded.price_basis,
                                                   updated_at        = excluded.updated_at",
            rusqlite::params![
                outlet,
                profile.name,
                profile.address,
                profile.phone,
                profile.gstin,
                profile.fssai,
                profile.state_code,
                profile.upi_id,
                profile.upi_merchant_name,
                profile.upi_reference,
                profile.registration,
                encode::timestamp_to_sql(at),
                encode::price_basis_to_sql(profile.price_basis),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "store_profile", outlet, Op::Upsert, at)
    }

    pub fn store_profile(&self, outlet: &str) -> Result<Option<StoreProfile>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT name, address, phone, gstin, fssai, state_code, upi_id, upi_merchant_name,
                    upi_reference, registration, price_basis
               FROM store_profile WHERE outlet_id = ?1",
        )?;
        let mut rows = stmt.query([outlet])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(StoreProfile {
            name: row.get(0)?,
            address: row.get(1)?,
            phone: row.get(2)?,
            gstin: row.get(3)?,
            fssai: row.get(4)?,
            state_code: row.get(5)?,
            upi_id: row.get(6)?,
            upi_merchant_name: row.get(7)?,
            upi_reference: row.get(8)?,
            registration: row.get(9)?,
            price_basis: encode::price_basis_from_sql(&row.get::<_, String>(10)?)?,
        }))
    }

    pub fn save_printer(
        &self,
        outlet: &str,
        printer: &Printer,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO printers (id, outlet_id, name, kind, address, paper_mm, is_default,
                                   can_kick_drawer, offset_x_mm, offset_y_mm,
                                   role, engine, is_bold_dark)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT (id) DO UPDATE SET name            = excluded.name,
                                            kind            = excluded.kind,
                                            address         = excluded.address,
                                            paper_mm        = excluded.paper_mm,
                                            is_default      = excluded.is_default,
                                            can_kick_drawer = excluded.can_kick_drawer,
                                            offset_x_mm     = excluded.offset_x_mm,
                                            offset_y_mm     = excluded.offset_y_mm,
                                            role            = excluded.role,
                                            engine          = excluded.engine,
                                            is_bold_dark    = excluded.is_bold_dark",
            rusqlite::params![
                printer.id,
                outlet,
                printer.name,
                printer.kind,
                printer.address,
                printer.paper_mm,
                encode::bool_to_sql(printer.is_default),
                encode::bool_to_sql(printer.can_kick_drawer),
                printer.offset_x_mm,
                printer.offset_y_mm,
                printer.role,
                printer.engine,
                encode::bool_to_sql(printer.is_bold_dark),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "printers", &printer.id, Op::Upsert, at)
    }

    /// Delete a printer, and let the foreign keys refuse if they must.
    pub fn delete_printer(&self, outlet: &str, id: &str, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "DELETE FROM printers WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, id],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "printers", id, Op::Delete, at)
    }

    /// Which printer a category's kitchen tickets go to.
    pub fn category_printers(&self, outlet: &str) -> Result<Vec<(String, String)>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT cp.category_id, cp.printer_id
               FROM category_printers cp
               JOIN printers p ON p.id = cp.printer_id
              WHERE p.outlet_id = ?1",
        )?;
        let rows = stmt.query_map([outlet], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Send this category's tickets to that printer, or `None` to stop.
    pub fn route_category(
        &self,
        outlet: &str,
        category_id: &str,
        printer_id: Option<&str>,
        at: Timestamp,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "DELETE FROM category_printers WHERE category_id = ?1",
            [category_id],
        )?;
        if let Some(printer) = printer_id {
            self.tx.execute(
                "INSERT INTO category_printers (category_id, printer_id) VALUES (?1, ?2)",
                rusqlite::params![category_id, printer],
            )?;
        }
        OutboxRepo::new(self.tx).enqueue(outlet, "category_printers", category_id, Op::Upsert, at)
    }

    pub fn list_printers(&self, outlet: &str) -> Result<Vec<Printer>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, name, kind, address, paper_mm, is_default, can_kick_drawer,
                    offset_x_mm, offset_y_mm, role, engine, is_bold_dark
               FROM printers WHERE outlet_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map([outlet], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, i64>(11)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, kind, address, paper_mm, is_default, drawer, x, y, role, engine, dark) =
                row?;
            out.push(Printer {
                id,
                name,
                kind,
                address,
                paper_mm,
                is_default: encode::bool_from_sql(is_default, "printers.is_default")?,
                can_kick_drawer: encode::bool_from_sql(drawer, "printers.can_kick_drawer")?,
                offset_x_mm: x,
                offset_y_mm: y,
                role,
                engine,
                is_bold_dark: encode::bool_from_sql(dark, "printers.is_bold_dark")?,
            });
        }
        Ok(out)
    }
}
