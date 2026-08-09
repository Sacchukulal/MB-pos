//! Typed settings, the store profile, and printers.
//!
//! # Typed, not stringly-typed
//!
//! `get::<i64>("token.pad_width")` either returns a number, or `None` because
//! nobody has set it, or an **error** because what is stored is not a number.
//! It never returns zero. D7: nothing may be silently lossy, and a settings
//! reader that defaults on a type mismatch is the quietest possible way to
//! print the wrong bill.
//!
//! This is also audit E6's fix at the API level. v1 saved settings as "one
//! giant command with 41 numbered slots… this has already caused a 'reuse slot
//! 39 for four columns' patch in the past. It is a silent-wrong-data machine."
//! Here one setting is one row, written by name, with its type beside it.

use mb_core::{Money, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

/// The five things a setting can be. Stored in `settings.value_type` so a
/// reader can parse the text without asking the code that wrote it.
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

/// Paise, like everywhere else. A setting that holds money is not text that
/// happens to look like a number.
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
    /// Audit Part 3's third UPI field, added at P17.
    pub upi_reference: Option<String>,
    pub is_composition: bool,
    /// `intra` or `inter`. **The default only** — each bill stores its own,
    /// because a B2B customer in another state changes it for that bill alone
    /// (scope 2.4).
    ///
    /// Text rather than an enum for the reason `Printer::role` gives: the enum
    /// belongs to mb-core, a second copy here would be two lists to keep in
    /// step, and the schema's CHECK is what stops a typo reaching the disk.
    ///
    /// **The column has existed since P04 and nothing read it until P17.**
    pub default_place_of_supply: String,
}

/// Scope 7.11 lives on this: `offset_x_mm` / `offset_y_mm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printer {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub address: Option<String>,
    pub paper_mm: i64,
    pub is_default: bool,
    pub can_kick_drawer: bool,
    /// Whole millimetres, signed. The owner nudges these from the test print
    /// until the text lands correctly on the paper (P07).
    pub offset_x_mm: i64,
    pub offset_y_mm: i64,
    /// `bill`, `kitchen` or `both`. Text rather than an enum because the enum
    /// belongs to mb-print and a second copy of it here would be two lists to
    /// keep in step; the schema's CHECK is what stops a typo reaching the disk.
    pub role: String,
    /// `raster` or `text` — v1's "Print engine: Graphics or Text".
    pub engine: String,
    /// v1's "Bold & Dark".
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

    /// `None` means nobody has set it. A type mismatch is an error, not a
    /// default.
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
                                        upi_id, upi_merchant_name, upi_reference, is_composition,
                                        default_place_of_supply, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT (outlet_id) DO UPDATE SET name              = excluded.name,
                                                   address           = excluded.address,
                                                   phone             = excluded.phone,
                                                   gstin             = excluded.gstin,
                                                   fssai             = excluded.fssai,
                                                   state_code        = excluded.state_code,
                                                   upi_id            = excluded.upi_id,
                                                   upi_merchant_name = excluded.upi_merchant_name,
                                                   upi_reference     = excluded.upi_reference,
                                                   is_composition    = excluded.is_composition,
                                                   default_place_of_supply
                                                       = excluded.default_place_of_supply,
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
                encode::bool_to_sql(profile.is_composition),
                profile.default_place_of_supply,
                encode::timestamp_to_sql(at),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "store_profile", outlet, Op::Upsert, at)
    }

    pub fn store_profile(&self, outlet: &str) -> Result<Option<StoreProfile>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT name, address, phone, gstin, fssai, state_code, upi_id, upi_merchant_name,
                    upi_reference, is_composition, default_place_of_supply
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
            is_composition: encode::bool_from_sql(row.get(9)?, "store_profile.is_composition")?,
            default_place_of_supply: row.get(10)?,
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

    /// **Delete a printer, and let the foreign keys refuse if they must.**
    ///
    /// `print_jobs.printer_id` references this row, so a printer with paper in
    /// the spool cannot go — and that is the right answer: the alternative is a
    /// job addressed to a printer nobody believes in, which is the bug
    /// `state::fallback_row` exists to describe.
    pub fn delete_printer(&self, outlet: &str, id: &str, at: Timestamp) -> Result<(), DbError> {
        self.tx.execute(
            "DELETE FROM printers WHERE outlet_id = ?1 AND id = ?2",
            rusqlite::params![outlet, id],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "printers", id, Op::Delete, at)
    }

    /// Scope 3.1 — which printer a category's kitchen tickets go to.
    ///
    /// `(category, printer)` pairs. A category with no row goes wherever the
    /// kitchen tickets go by default, which is `RoutingTable`'s own rule.
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
    ///
    /// **One printer per category**, so the route is replaced rather than
    /// added to: the table's primary key allows several, and a category whose
    /// food printed in two places is a kitchen cooking everything twice.
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
