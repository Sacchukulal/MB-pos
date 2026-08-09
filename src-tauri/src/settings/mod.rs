//! **The whole of a shop's configuration, as one value and one table.**
//!
//! # The finding this closes
//!
//! Audit **E6**: v1 saved its settings as *"one giant command with 41 numbered
//! slots… this has already caused a 'reuse slot 39 for four columns' patch in
//! the past. It is a silent-wrong-data machine."*
//!
//! P04 fixed the storage — one row per setting, written by name, with its type
//! beside it. It did not fix the thing underneath, which is that E6 is a
//! **duplication** bug rather than a syntax one: the same fact (what a setting
//! is called, what it may hold, what it defaults to) was written down in three
//! places that had to be kept in step by hand.
//!
//! So there is exactly one table — [`catalog::CATALOG`] — and everything falls
//! out of it:
//!
//! | | |
//! |---|---|
//! | load | start from the defaults, walk the table, read each row |
//! | save | walk the table, write **only what differs** — that is field-by-field saving, structurally |
//! | reset a section | write each of its entries' defaults |
//! | export / import | the table as JSON |
//! | search | label + group + synonyms, one function |
//! | validate | [`value::Kind`], in one place, refusing rather than coercing |
//!
//! And the guard that keeps it true: a field added to [`ShopConfig`] with no
//! catalogue entry **fails the build** (see `catalog`'s `the_catalogue_is_the_
//! whole_of_the_configuration`). Two lists that drift is E6's actual mechanism;
//! a test that compares them is the only thing that stops it recurring.
//!
//! # What is deliberately NOT here
//!
//! Records, not scalars: the **printers** (a list, its own table), the
//! **counters** (per terminal, and setting one backwards is a legal question —
//! `numbering::set_next` guards it), and the **menu**. Each has its own
//! commands. Putting a list into a key-value catalogue would mean inventing
//! keys like `printer.3.paper_mm`, which is E6 wearing a different hat.
//!
//! The **store profile** IS here, even though it lives in its own table,
//! because it is nine scalars a person types into a form — see
//! [`Storage::Store`].

pub mod backup;
pub mod catalog;
pub mod ipc;
pub mod numbering;
pub mod printers;
pub mod sample;
pub mod value;

use mb_core::{Money, Timestamp};
use mb_db::error::DbError;
use mb_db::Repos;
use mb_print::settings::{KitchenSettings, ReceiptSettings};
use serde::{Deserialize, Serialize};

use crate::search::MatchMode;
use value::{Invalid, Value};

/// The shop, as a person fills it in.
///
/// **Not `mb_db`'s `StoreProfile`, and the difference is `Option`.** Every
/// field here is a `String`, and empty means "not filled in yet" — which is
/// what a form gives back, what the catalogue can validate uniformly, and what
/// a bill already knows how to omit. The conversion at the storage boundary is
/// the one place `""` and `NULL` meet, so there is one rule rather than a
/// scattering of `unwrap_or_default`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub name: String,
    pub address: String,
    pub phone: String,
    pub gstin: String,
    pub fssai: String,
    /// The two-digit GST state code. It decides CGST+SGST against IGST
    /// (scope 2.4) and it is half of the GSTIN check.
    pub state_code: String,
    pub upi_id: String,
    pub upi_merchant_name: String,
    pub upi_reference: String,
    /// Scope 2.10. A tax fact with a printing consequence.
    pub is_composition: bool,
    /// `intra` or `inter` — the default only; each bill stores its own.
    pub default_place_of_supply: String,
}

impl Default for Store {
    fn default() -> Self {
        Store {
            name: String::new(),
            address: String::new(),
            phone: String::new(),
            gstin: String::new(),
            fssai: String::new(),
            state_code: String::new(),
            upi_id: String::new(),
            upi_merchant_name: String::new(),
            upi_reference: String::new(),
            is_composition: false,
            default_place_of_supply: "intra".to_owned(),
        }
    }
}

impl Store {
    /// `""` becomes `NULL` here and nowhere else.
    #[must_use]
    pub fn to_profile(&self) -> mb_db::repo::settings::StoreProfile {
        fn some(text: &str) -> Option<String> {
            if text.is_empty() {
                None
            } else {
                Some(text.to_owned())
            }
        }
        mb_db::repo::settings::StoreProfile {
            name: self.name.clone(),
            address: self.address.clone(),
            phone: some(&self.phone),
            gstin: some(&self.gstin),
            fssai: some(&self.fssai),
            state_code: some(&self.state_code),
            upi_id: some(&self.upi_id),
            upi_merchant_name: some(&self.upi_merchant_name),
            upi_reference: some(&self.upi_reference),
            is_composition: self.is_composition,
            default_place_of_supply: self.default_place_of_supply.clone(),
        }
    }

    #[must_use]
    pub fn from_profile(profile: &mb_db::repo::settings::StoreProfile) -> Store {
        Store {
            name: profile.name.clone(),
            address: profile.address.clone(),
            phone: profile.phone.clone().unwrap_or_default(),
            gstin: profile.gstin.clone().unwrap_or_default(),
            fssai: profile.fssai.clone().unwrap_or_default(),
            state_code: profile.state_code.clone().unwrap_or_default(),
            upi_id: profile.upi_id.clone().unwrap_or_default(),
            upi_merchant_name: profile.upi_merchant_name.clone().unwrap_or_default(),
            upi_reference: profile.upi_reference.clone().unwrap_or_default(),
            is_composition: profile.is_composition,
            default_place_of_supply: profile.default_place_of_supply.clone(),
        }
    }

    /// What the printed bill's header needs.
    #[must_use]
    pub fn to_print_store(&self) -> mb_print::template::Store {
        let profile = self.to_profile();
        mb_print::template::Store {
            name: profile.name,
            address: profile.address,
            phone: profile.phone,
            gstin: profile.gstin,
            fssai: profile.fssai,
            state_code: profile.state_code,
            upi_id: profile.upi_id,
            upi_merchant_name: profile.upi_merchant_name,
            upi_reference: profile.upi_reference,
            is_composition: profile.is_composition,
        }
    }

    /// Scope 2.4 — which pair of taxes this shop charges by default.
    #[must_use]
    pub fn place_of_supply(&self) -> mb_core::PlaceOfSupply {
        if self.default_place_of_supply == "inter" {
            mb_core::PlaceOfSupply::Inter
        } else {
            mb_core::PlaceOfSupply::Intra
        }
    }
}

/// How the counter behaves while somebody is billing on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Billing {
    pub search_mode: MatchMode,
    pub rounding: mb_core::RoundingMode,
    /// v1 kept this in the browser's local storage, which is audit A5's
    /// smaller cousin: it was lost whenever the storage was cleared.
    pub lock_order_type: bool,
    pub locked_order_type: mb_core::OrderType,
    pub confirm_before_kitchen: bool,
    pub confirm_before_bill: bool,
    /// v1's "disable KOT entirely" — a counter-only shop with no kitchen.
    pub kitchen_ticket_off: bool,
    /// 0 means never. Read by `session::IDLE_LOCK`'s caller.
    pub idle_lock_minutes: u32,
    /// Scope 1.14. Basis points, so 5% is 500 — and **0 means the shop does
    /// not charge it**, which is why there is no separate on/off tick beside
    /// it. Two fields meaning the same thing is a bug waiting for the day they
    /// disagree (`Charge`'s own words about `taxable`).
    pub service_charge_bp: u32,
    /// Its OWN rate, because a service charge is taxed at 18% on a bill of 5%
    /// food and that mixed case is the whole reason `Charge` carries a rate.
    pub service_charge_tax_bp: u32,
    pub packing_charge: Money,
    pub packing_charge_tax_bp: u32,
    pub delivery_charge: Money,
    pub delivery_charge_tax_bp: u32,
}

impl Default for Billing {
    fn default() -> Self {
        Billing {
            search_mode: MatchMode::Contains,
            rounding: mb_core::RoundingMode::NearestRupee,
            lock_order_type: false,
            locked_order_type: mb_core::OrderType::DineIn,
            confirm_before_kitchen: false,
            confirm_before_bill: false,
            kitchen_ticket_off: false,
            idle_lock_minutes: 10,
            // Every charge off by default. A shop that has not been asked has
            // not agreed to charge its customers anything.
            service_charge_bp: 0,
            service_charge_tax_bp: 1_800,
            packing_charge: Money::ZERO,
            packing_charge_tax_bp: 500,
            delivery_charge: Money::ZERO,
            delivery_charge_tax_bp: 500,
        }
    }
}

impl Billing {
    /// The charges this order type attracts, ready for `BillInput`.
    ///
    /// **Order type decides.** Service on a table, packing on a parcel,
    /// delivery on a delivery — a shop that charged packing on a dine-in bill
    /// would be describing something that did not happen.
    #[must_use]
    pub fn charges_for(&self, order_type: mb_core::OrderType) -> Vec<mb_core::Charge> {
        use mb_core::{Charge, ChargeKind, OrderType, TaxRate};
        let rate = |bp: u32| TaxRate::from_basis_points(bp).unwrap_or(TaxRate::ZERO);
        let mut out = Vec::new();
        match order_type {
            OrderType::DineIn => {
                if self.service_charge_bp > 0 {
                    out.push(Charge::percent(
                        ChargeKind::Service,
                        "Service Charge",
                        self.service_charge_bp,
                        rate(self.service_charge_tax_bp),
                    ));
                }
            }
            OrderType::Parcel | OrderType::SelfService => {
                if !self.packing_charge.is_zero() {
                    out.push(Charge::flat(
                        ChargeKind::Packing,
                        "Packing",
                        self.packing_charge,
                        rate(self.packing_charge_tax_bp),
                    ));
                }
            }
            OrderType::Delivery => {
                if !self.delivery_charge.is_zero() {
                    out.push(Charge::flat(
                        ChargeKind::Delivery,
                        "Delivery",
                        self.delivery_charge,
                        rate(self.delivery_charge_tax_bp),
                    ));
                }
            }
        }
        out
    }
}

/// Decision D5 on a screen: when the shop's day starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Day {
    /// Minutes past midnight. 300 is 5 a.m., the default every Indian
    /// restaurant recognises.
    pub starts_at_minutes: u32,
    /// P18. Above this difference between counted and expected cash, closing
    /// the day asks why — and the answer goes on the slip and in the history.
    pub variance_reason_above: mb_core::Money,
    /// Whether the drawer keeps change overnight.
    pub carry_float: bool,
    pub float_amount: mb_core::Money,
}

impl Default for Day {
    fn default() -> Self {
        Day {
            starts_at_minutes: u32::from(mb_core::DayRule::DEFAULT.starts_at_minutes()),
            // ₹20. Small enough that a real shortage is caught, large enough
            // that a rounded-off bill does not make a cashier fill in a form.
            variance_reason_above: mb_core::Money::from_paise(2_000),
            carry_float: false,
            float_amount: mb_core::Money::ZERO,
        }
    }
}

impl Day {
    /// **Clamped, and this is the one place coercion is right.** A stored value
    /// outside a day cannot be shown to the cashier as an error — the day rule
    /// is read on the billing path, and refusing to compute a business day
    /// would stop the shop billing over a settings typo. `DayRule::new` already
    /// refuses out of range; the *screen* refuses first (T4), so a value that
    /// gets here past validation is corrupt data, not a person's mistake.
    #[must_use]
    pub fn rule(self) -> mb_core::DayRule {
        u16::try_from(self.starts_at_minutes)
            .ok()
            .and_then(mb_core::DayRule::new)
            .unwrap_or(mb_core::DayRule::DEFAULT)
    }
}

/// Scope 13.11 and 13.12 — the look and the language.
///
/// **Only the language is here.** The theme and the text size live in
/// `AppConfig`, on the machine, and that is deliberate: they have to be applied
/// before the first paint and they have to work when the database will not
/// open. A shop's language is the shop's — it is on the receipt — so it is a
/// setting like any other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// `en`, `hi` or `kn`. **Only English is installed today** (P23 does the
    /// rest), and the screen says so rather than offering a choice that
    /// silently does nothing — which is audit F8.
    pub language: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            language: "en".to_owned(),
        }
    }
}

/// What a new menu item and a new bill assume about tax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Tax {
    /// A `tax_classes.id` from P13, or empty for "ask me". Free text rather
    /// than a choice because the list is the shop's own data.
    pub default_class_id: String,
    /// Scope 2.11 — whether a price typed into the menu already contains tax.
    pub prices_include_tax: bool,
}

/// Where backups go and how many are kept (13.1, and audit A1 is why).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupPolicy {
    /// Empty means the usual place beside the database.
    pub folder: String,
    /// A2's second location — a pen drive, a network share. Empty means none,
    /// and the screen says loudly that one copy is not a backup.
    pub second_folder: String,
    /// 0 means only when somebody presses the button.
    pub every_hours: u32,
    pub keep_count: u32,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        BackupPolicy {
            folder: String::new(),
            second_folder: String::new(),
            every_hours: 24,
            keep_count: 30,
        }
    }
}

/// **The whole configuration, as one value.**
///
/// Held by `App` and replaced wholesale on save, so nothing on the printing
/// path ever reads a half-applied change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ShopConfig {
    pub store: Store,
    pub receipt: ReceiptSettings,
    pub kitchen: KitchenSettings,
    pub billing: Billing,
    pub day: Day,
    pub tax: Tax,
    pub backup: BackupPolicy,
    pub appearance: Appearance,
}

/// One setting that changed, for the audit trail and for the screen's summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub key: &'static str,
    pub label: &'static str,
    pub before: String,
    pub after: String,
}

/// **Read the shop's configuration.**
///
/// A missing row is the default — that is what "nobody has set it" means. A row
/// stored as the *wrong type* is an error and names the key: D7, and the
/// alternative is a shop printing at 5% because a `"twelve"` read back as zero.
pub fn load(repos: &Repos<'_>, outlet: &str) -> Result<ShopConfig, DbError> {
    let settings = repos.settings();

    let mut config = ShopConfig::default();
    if let Some(profile) = settings.store_profile(outlet)? {
        config.store = Store::from_profile(&profile);
    }

    for entry in catalog::CATALOG {
        if entry.storage != Storage::Row {
            continue;
        }
        let stored = match (entry.read)(&ShopConfig::default()) {
            Value::Bool(_) => settings.get::<bool>(outlet, entry.key)?.map(Value::Bool),
            Value::Int(_) => settings.get::<i64>(outlet, entry.key)?.map(Value::Int),
            Value::Money(_) => settings.get::<Money>(outlet, entry.key)?.map(Value::Money),
            Value::Text(_) => settings.get::<String>(outlet, entry.key)?.map(Value::Text),
        };
        let Some(stored) = stored else { continue };
        // **A stored value is checked on the way IN, not only on the way out.**
        // A limit that tightened in a later build must fail loudly on the old
        // value rather than printing it.
        entry.kind.check(&stored).map_err(|e| {
            DbError::invariant(format!("the setting \"{}\" is wrong: {}", entry.key, e.message))
        })?;
        (entry.write)(&mut config, &stored).map_err(|e| {
            DbError::invariant(format!("the setting \"{}\" is wrong: {}", entry.key, e.message))
        })?;
    }

    Ok(config)
}

/// **Write only what changed.**
///
/// This is P17's T3 — *"changing one setting cannot write another"* —
/// structurally rather than by care. There is no path here that writes a
/// setting whose value is the same, so a screen that posts the whole form back
/// (which every screen does) touches exactly the rows the person edited.
///
/// The returned list is what the audit row is built from, with the before AND
/// the after value: a settings change nobody can attribute is how a shop's bill
/// numbering changes overnight and nobody knows who did it.
pub fn save_changes(
    repos: &Repos<'_>,
    outlet: &str,
    old: &ShopConfig,
    new: &ShopConfig,
    at: Timestamp,
    by: Option<&str>,
) -> Result<Vec<Changed>, DbError> {
    let settings = repos.settings();

    let mut changed = Vec::new();
    let mut store_moved = false;

    for entry in catalog::CATALOG {
        let before = (entry.read)(old);
        let after = (entry.read)(new);
        if before == after {
            continue;
        }
        match entry.storage {
            Storage::Store => store_moved = true,
            Storage::Row => match &after {
                Value::Bool(b) => settings.set(outlet, entry.key, b, at, by)?,
                Value::Int(n) => settings.set(outlet, entry.key, n, at, by)?,
                Value::Money(m) => settings.set(outlet, entry.key, m, at, by)?,
                Value::Text(t) => settings.set(outlet, entry.key, t, at, by)?,
            },
        }
        changed.push(Changed {
            key: entry.key,
            label: entry.label,
            before: describe(&before),
            after: describe(&after),
        });
    }

    // One row, one write — but only if something in it actually moved, or a
    // shop editing its footer message would restamp its own GSTIN.
    if store_moved {
        settings.save_store_profile(outlet, &new.store.to_profile(), at)?;
    }

    Ok(changed)
}

/// A value, in the words the history screen shows. Never a tag.
#[must_use]
pub fn describe(value: &Value) -> String {
    match value {
        Value::Bool(true) => "on".to_owned(),
        Value::Bool(false) => "off".to_owned(),
        Value::Int(n) => n.to_string(),
        Value::Money(m) => m.to_plain_string(),
        Value::Text(t) if t.is_empty() => "(nothing)".to_owned(),
        Value::Text(t) => t.clone(),
    }
}

/// Put one section back to how it shipped.
///
/// Returns the configuration it *would* write, so the screen can say how many
/// settings this will change before it changes them — the same dry-run shape
/// P13's CSV import uses, and for the same reason.
#[must_use]
pub fn reset_group(config: &ShopConfig, group: catalog::Group) -> ShopConfig {
    let defaults = ShopConfig::default();
    let mut out = config.clone();
    for entry in catalog::CATALOG {
        if entry.group != group {
            continue;
        }
        // A default cannot fail its own validation, and if one ever did the
        // catalogue is wrong — which `every_default_is_valid` fails the build
        // over.
        let _ = (entry.write)(&mut out, &(entry.read)(&defaults));
    }
    out
}

/// Where a setting is kept.
///
/// Two answers, and the second one exists because the store profile is nine
/// scalars in a form that happen to live in their own table (it predates the
/// `settings` table and is read by SQL that joins on it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    /// A row in `settings`, by key.
    Row,
    /// A column of `store_profile`.
    Store,
}

/// Search every setting, by label, by section, or by a word a person would
/// actually type.
///
/// P17's T9. An owner must never hunt: "QR", "roundoff", "thank you" and
/// "5 am" all have to land on something.
#[must_use]
pub fn search(text: &str) -> Vec<&'static catalog::Entry> {
    let needle = text.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    catalog::CATALOG
        .iter()
        .filter(|entry| {
            entry.label.to_lowercase().contains(&needle)
                || entry.group.label().to_lowercase().contains(&needle)
                || entry.key.contains(&needle)
                || entry.synonyms.iter().any(|s| s.contains(&needle))
        })
        .collect()
}

/// Every setting as `key -> value`, for the export file.
#[must_use]
pub fn to_map(config: &ShopConfig) -> std::collections::BTreeMap<String, serde_json::Value> {
    catalog::CATALOG
        .iter()
        .map(|entry| {
            let value = match (entry.read)(config) {
                Value::Bool(b) => serde_json::Value::Bool(b),
                Value::Int(n) => serde_json::Value::from(n),
                Value::Money(m) => serde_json::Value::from(m.paise()),
                Value::Text(t) => serde_json::Value::String(t),
            };
            (entry.key.to_owned(), value)
        })
        .collect()
}

/// What an import found, **before anything is written**.
///
/// P17's T15: a file with one bad value changes nothing. The dry run is the
/// feature, exactly as it is for P13's CSV import — the difference between
/// "it failed" and "line 40 says the padding is 12" is the difference between
/// a support call and a fix.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportPlan {
    /// What would change, in words.
    pub changes: Vec<Changed>,
    /// Keys in the file this build has never heard of. Reported, not refused:
    /// a configuration exported by a NEWER Magic Bill must still be usable, and
    /// dropping the two keys it gained is the honest outcome.
    pub unknown: Vec<String>,
    /// Why the file cannot be used. Non-empty means nothing will be written.
    pub problems: Vec<String>,
}

impl ImportPlan {
    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Read a configuration file into a plan. **Writes nothing.**
#[must_use]
pub fn plan_import(
    current: &ShopConfig,
    file: &std::collections::BTreeMap<String, serde_json::Value>,
) -> (ShopConfig, ImportPlan) {
    let mut plan = ImportPlan::default();
    let mut wanted = current.clone();

    let mut known: std::collections::BTreeSet<&str> =
        catalog::CATALOG.iter().map(|e| e.key).collect();

    for entry in catalog::CATALOG {
        let Some(raw) = file.get(entry.key) else {
            continue;
        };
        known.remove(entry.key);
        let value = match (entry.read)(current) {
            Value::Bool(_) => raw.as_bool().map(Value::Bool),
            Value::Int(_) => raw.as_i64().map(Value::Int),
            Value::Money(_) => raw.as_i64().map(|n| Value::Money(Money::from_paise(n))),
            Value::Text(_) => raw.as_str().map(|s| Value::Text(s.to_owned())),
        };
        let Some(value) = value else {
            plan.problems.push(format!(
                "\"{}\" ({}) is the wrong sort of value in this file.",
                entry.label, entry.key
            ));
            continue;
        };
        if let Err(e) = entry.kind.check(&value) {
            plan.problems.push(format!("\"{}\": {}", entry.label, e.message));
            continue;
        }
        let before = (entry.read)(&wanted);
        if let Err(e) = (entry.write)(&mut wanted, &value) {
            plan.problems.push(format!("\"{}\": {}", entry.label, e.message));
            continue;
        }
        if before != value {
            plan.changes.push(Changed {
                key: entry.key,
                label: entry.label,
                before: describe(&before),
                after: describe(&value),
            });
        }
    }

    for key in file.keys() {
        if !catalog::CATALOG.iter().any(|e| e.key == key.as_str()) {
            plan.unknown.push(key.clone());
        }
    }

    // A plan that cannot be used must not be able to be applied by accident.
    if !plan.is_usable() {
        wanted = current.clone();
    }
    (wanted, plan)
}

/// Every catalogue entry's default, as a fresh configuration would hold it.
#[must_use]
pub fn defaults() -> ShopConfig {
    ShopConfig::default()
}

/// Refuse a value, with the key attached.
pub fn check(entry: &catalog::Entry, value: &Value) -> Result<(), Invalid> {
    entry.kind.check(value).map_err(|e| e.about(entry.key))
}

#[cfg(test)]
mod tests;
