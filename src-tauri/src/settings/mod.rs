//! The whole of a shop's configuration, as one value and one table.

pub mod backup;
pub mod catalog;
pub mod ipc;
pub mod numbering;
pub mod printers;
pub mod sample;
pub mod value;

use mb_core::{Money, Timestamp};
use mb_db::Repos;
use mb_db::error::DbError;
use mb_print::settings::{KitchenSettings, ReceiptSettings};
use serde::{Deserialize, Serialize};

use crate::search::MatchMode;
use value::{Invalid, Value};

/// What one ESC/POS multiplier step is worth, in dots.
pub const BASE_CELL_PX: u16 = 24;

/// Is this one of the sizes the screen offers?
#[must_use]
pub fn is_a_size(px: u16) -> bool {
    catalog::SIZES
        .iter()
        .any(|choice| choice.value.parse::<u16>() == Ok(px))
}

/// What a shop calls this height — "1" to "10", the numbers on the dropdown.
#[must_use]
pub fn size_label(px: u16) -> String {
    let mut best: Option<(u16, &str)> = None;
    for choice in catalog::SIZES {
        let Ok(dots) = choice.value.parse::<u16>() else {
            continue;
        };
        if dots <= px && best.is_none_or(|(had, _)| dots >= had) {
            best = Some((dots, choice.label));
        }
    }
    // Below the smallest on the list is still the smallest a person can ask for, so it is what
    // they should be told.
    best.map_or_else(
        || {
            catalog::SIZES
                .first()
                .map_or_else(|| px.to_string(), |c| c.label.to_owned())
        },
        |(_, label)| label.to_owned(),
    )
}

/// The shop, as a person fills it in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub name: String,
    pub address: String,
    pub phone: String,
    pub gstin: String,
    pub fssai: String,
    /// The two-digit GST state code.
    pub state_code: String,
    pub upi_id: String,
    pub upi_merchant_name: String,
    pub upi_reference: String,
    /// `unregistered`, `composition` or `regular` — the gate on the whole tax pipeline.
    pub registration: String,
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
            registration: "regular".to_owned(),
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
            registration: self.registration.clone(),
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
            registration: profile.registration.clone(),
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
            registration: self.registration(),
        }
    }

    /// What kind of taxpayer this shop is — the gate on the tax pipeline.
    #[must_use]
    pub fn registration(&self) -> mb_core::Registration {
        registration_from(&self.registration)
    }
}

/// The stored text as the rule.
#[must_use]
pub fn registration_from(text: &str) -> mb_core::Registration {
    match text {
        "unregistered" => mb_core::Registration::Unregistered,
        "composition" => mb_core::Registration::Composition,
        _ => mb_core::Registration::Regular,
    }
}

/// How the counter behaves while somebody is billing on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Billing {
    pub search_mode: MatchMode,
    pub rounding: mb_core::RoundingMode,
    pub lock_order_type: bool,
    pub locked_order_type: mb_core::OrderType,
    pub confirm_before_kitchen: bool,
    pub confirm_before_bill: bool,
    pub kitchen_ticket_off: bool,
    /// 0 means never. Read by `session::IDLE_LOCK`'s caller.
    pub idle_lock_minutes: u32,
    /// Basis points, so 5% is 500 — and 0 means the shop does not charge it, which is why there
    /// is no separate on/off tick beside it.
    pub service_charge_bp: u32,
    /// Its OWN rate, because a service charge is taxed at 18% on a bill of 5% food and that
    /// mixed case is the whole reason `Charge` carries a rate.
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
            // Every charge off by default.
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
    /// Minutes past midnight. 300 is 5 a.m., the default every Indian restaurant recognises.
    pub starts_at_minutes: u32,
    /// Above this difference between counted and expected cash, closing the day asks why — and
    /// the answer goes on the slip and in the history.
    pub variance_reason_above: mb_core::Money,
    /// Whether the drawer keeps change overnight.
    pub carry_float: bool,
    pub float_amount: mb_core::Money,
}

impl Default for Day {
    fn default() -> Self {
        Day {
            starts_at_minutes: u32::from(mb_core::DayRule::DEFAULT.starts_at_minutes()),
            // ₹20. Small enough that a real shortage is caught, large enough that a rounded-off
            // bill does not make a cashier fill in a form.
            variance_reason_above: mb_core::Money::from_paise(2_000),
            carry_float: false,
            float_amount: mb_core::Money::ZERO,
        }
    }
}

impl Day {
    /// Clamped, and this is the one place coercion is right.
    #[must_use]
    pub fn rule(self) -> mb_core::DayRule {
        u16::try_from(self.starts_at_minutes)
            .ok()
            .and_then(mb_core::DayRule::new)
            .unwrap_or(mb_core::DayRule::DEFAULT)
    }
}

/// 11 and 13.12 — the look and the language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// `en`, `hi` or `kn`.
    pub language: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            language: "en".to_owned(),
        }
    }
}

/// Where backups go and how many are kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupPolicy {
    /// Empty means the usual place beside the database.
    pub folder: String,
    pub second_folder: String,
    /// 0 means only when somebody presses the button.
    pub every_hours: u32,
    pub keep_count: u32,
    /// May we be told when the counter stops unexpectedly?
    pub send_crash_reports: bool,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        BackupPolicy {
            folder: String::new(),
            second_folder: String::new(),
            every_hours: 24,
            keep_count: 30,
            // Off. Consent is given, not assumed — and the report exists on this computer
            // whichever way this is set.
            send_crash_reports: false,
        }
    }
}

/// The whole configuration, as one value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ShopConfig {
    pub store: Store,
    pub receipt: ReceiptSettings,
    pub kitchen: KitchenSettings,
    pub billing: Billing,
    pub day: Day,
    pub stock: Stock,
    pub backup: BackupPolicy,
    pub appearance: Appearance,
    /// The scanner, the scale, the customer display and the label printer — every one of them
    /// optional.
    pub devices: Devices,
}

/// The things a counter is plugged into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Devices {
    /// The longest AVERAGE gap between keystrokes that still counts as a machine.
    pub scan_average_gap_ms: u32,
    /// One gap longer than this ends the burst, whatever the average.
    pub scan_single_gap_ms: u32,
    /// Shorter than this and it is not a barcode at all.
    pub scan_min_length: u32,

    /// The leading digits that mark a weight-encoded label.
    pub label_prefix: String,
    pub label_item_from: u32,
    pub label_item_len: u32,
    pub label_value_from: u32,
    pub label_value_len: u32,
    /// `quantity` or `price` — what the number inside the label means.
    pub label_value_is: String,
    /// How many of the value's digits are after the decimal point.
    pub label_value_decimals: u32,

    /// `COM3`, or empty for a shop with no scale — which is nearly all of them.
    pub scale_port: String,
    pub scale_baud: u32,
    /// `status_then_weight`, `weight_only` or `raw`.
    pub scale_protocol: String,

    /// Whether a second screen shows the customer their bill as it is typed.
    pub display_on: bool,
    /// A serial pole display's port.
    pub display_port: String,
    /// What the display says when there is no bill on the screen.
    pub display_idle: String,

    /// Which printer parcel labels go to.
    pub label_printer: String,
}

impl Default for Devices {
    fn default() -> Self {
        let scan = mb_core::devices::ScanRule::default();
        Devices {
            scan_average_gap_ms: scan.max_average_gap_ms,
            scan_single_gap_ms: scan.max_single_gap_ms,
            scan_min_length: u32::try_from(scan.min_length).unwrap_or(8),
            // The commonest Indian convention, and still off until a shop types a prefix:
            // guessing here would turn ordinary EAN-13 barcodes into weights.
            label_prefix: String::new(),
            label_item_from: 1,
            label_item_len: 6,
            label_value_from: 7,
            label_value_len: 5,
            label_value_is: "quantity".to_owned(),
            label_value_decimals: 3,
            scale_port: String::new(),
            scale_baud: 9_600,
            scale_protocol: "status_then_weight".to_owned(),
            display_on: false,
            display_port: String::new(),
            display_idle: String::new(),
            label_printer: String::new(),
        }
    }
}

impl Devices {
    /// The scan rule, as mb-core wants it.
    #[must_use]
    pub fn scan_rule(&self) -> mb_core::devices::ScanRule {
        mb_core::devices::ScanRule {
            max_average_gap_ms: self.scan_average_gap_ms,
            max_single_gap_ms: self.scan_single_gap_ms,
            min_length: usize::try_from(self.scan_min_length).unwrap_or(8),
        }
    }

    /// The label rule, or `None` when this shop does not use weight-encoded labels — which is
    /// the ordinary case and not a misconfiguration.
    #[must_use]
    pub fn label_rule(&self) -> Option<mb_core::devices::EmbeddedRule> {
        if self.label_prefix.trim().is_empty() {
            return None;
        }
        Some(mb_core::devices::EmbeddedRule {
            prefix: self.label_prefix.trim().to_owned(),
            item_from: usize::try_from(self.label_item_from).unwrap_or(0),
            item_len: usize::try_from(self.label_item_len).unwrap_or(0),
            value_from: usize::try_from(self.label_value_from).unwrap_or(0),
            value_len: usize::try_from(self.label_value_len).unwrap_or(0),
            value_is_price: self.label_value_is == "price",
            value_decimals: self.label_value_decimals,
        })
    }

    #[must_use]
    pub fn protocol(&self) -> mb_core::devices::ScaleProtocol {
        match self.scale_protocol.as_str() {
            "weight_only" => mb_core::devices::ScaleProtocol::WeightOnly,
            "raw" => mb_core::devices::ScaleProtocol::Raw,
            _ => mb_core::devices::ScaleProtocol::StatusThenWeight,
        }
    }
}

/// The stock book's only preference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stock {
    /// How big a count variance has to be before the screen asks why.
    pub count_reason_above: Money,
}

impl Default for Stock {
    fn default() -> Self {
        Stock {
            count_reason_above: Money::from_paise(50_000),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub key: &'static str,
    pub label: &'static str,
    pub before: String,
    pub after: String,
}

/// Read the shop's configuration.
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
        // What an older build wrote, in the words this one uses.
        let stored = modernise(entry, stored);
        // A stored value is checked on the way IN, not only on the way out.
        entry.kind.check(&stored).map_err(|e| {
            DbError::invariant(format!(
                "the setting \"{}\" is wrong: {}",
                entry.key, e.message
            ))
        })?;
        (entry.write)(&mut config, &stored).map_err(|e| {
            DbError::invariant(format!(
                "the setting \"{}\" is wrong: {}",
                entry.key, e.message
            ))
        })?;
    }

    Ok(config)
}

/// A value an older build wrote, in the words this one uses.
fn modernise(entry: &catalog::Entry, stored: Value) -> Value {
    let Value::Text(text) = &stored else {
        return stored;
    };
    // Sizes and nothing else.
    let value::Kind::Choice(choices) = entry.kind else {
        return stored;
    };
    if !std::ptr::eq(choices, catalog::SIZES) {
        return stored;
    }
    let Ok(stored_dots) = text.parse::<u16>() else {
        return stored;
    };
    // Every vocabulary this row has held, told apart exactly.
    let dots = mb_print::Style::from_stored(stored_dots);

    // A height that is no longer on the list becomes the nearest one that is.
    if is_a_size(dots) {
        return Value::Text(dots.to_string());
    }
    let nearest = catalog::SIZES
        .iter()
        .filter_map(|choice| choice.value.parse::<u16>().ok())
        .min_by_key(|offered| offered.abs_diff(dots));
    match nearest {
        Some(dots) => Value::Text(dots.to_string()),
        None => stored,
    }
}

/// Write only what changed.
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

    // One row, one write — but only if something in it actually moved, or a shop editing its
    // footer message would restamp its own GSTIN.
    if store_moved {
        settings.save_store_profile(outlet, &new.store.to_profile(), at)?;
    }

    Ok(changed)
}

/// A value, in the words the history screen shows.
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
#[must_use]
pub fn reset_group(config: &ShopConfig, group: catalog::Group) -> ShopConfig {
    let defaults = ShopConfig::default();
    let mut out = config.clone();
    for entry in catalog::CATALOG {
        if entry.group != group {
            continue;
        }
        // A default cannot fail its own validation, and if one ever did the catalogue is wrong
        // — which `every_default_is_valid` fails the build over.
        let _ = (entry.write)(&mut out, &(entry.read)(&defaults));
    }
    out
}

/// Where a setting is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    /// A row in `settings`, by key.
    Row,
    /// A column of `store_profile`.
    Store,
}

/// Search every setting, by label, by section, or by a word a person would actually type.
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

/// What an import found, before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportPlan {
    /// What would change, in words.
    pub changes: Vec<Changed>,
    /// Keys in the file this build has never heard of.
    pub unknown: Vec<String>,
    /// Why the file cannot be used.
    pub problems: Vec<String>,
}

impl ImportPlan {
    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Read a configuration file into a plan.
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
            plan.problems
                .push(format!("\"{}\": {}", entry.label, e.message));
            continue;
        }
        let before = (entry.read)(&wanted);
        if let Err(e) = (entry.write)(&mut wanted, &value) {
            plan.problems
                .push(format!("\"{}\": {}", entry.label, e.message));
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
