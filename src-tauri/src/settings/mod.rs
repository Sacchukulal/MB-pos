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
//!
//! # Per-terminal settings (P27, scope 11.1) — and why there is no third scope
//!
//! P27 asked for a till to have its own printers, its own drawer, its own
//! default order type and its own numbering prefix. Every one of those is
//! already true, and **not one of them needed a scope on a key**:
//!
//! * **printers** and **the drawer** are rows in `printers`, and a secondary
//!   till has its own database — so they are its own by construction. Two tills
//!   have never been able to share a printer row.
//! * **the default order type** is `billing.lock_order_type` /
//!   `billing.locked_order_type`, and the same argument applies: they are read
//!   out of the database this machine opened. The parcel window opens on Parcel
//!   because *that machine* was set that way.
//! * **the numbering prefix** is on `counters`, which has been keyed on
//!   `(outlet, terminal, kind)` since P04, and D135 is what made it matter.
//!
//! So there is no `till:<id>:<key>` scope, and adding one would have been a
//! third way of saying where a value lives, to hold nothing. The thing a shop
//! actually notices — that the two tills' settings can drift apart — is honest
//! and expected until P33 gives them a way to be told about each other; a
//! secondary that silently inherited the master's printer would be a till
//! printing to a machine in another room.

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

/// **What one ESC/POS multiplier step is worth, in dots.**
///
/// A thermal printer's own Font A cell is 12 × 24, so `scale: 1` is 24 dots
/// tall. Every px size a shop chooses is converted against this — see
/// `mb_print::doc::Style::px` — and it is the printer's fact rather than the
/// paper's: all three roll widths use the same cell height.
pub const BASE_CELL_PX: u16 = 24;

/// Is this one of the sizes the screen offers?
///
/// The catalogue's list is the authority and this reads it, so adding a size
/// there is the whole of adding a size — there is no second list to keep in
/// step, which is how the three sizes and the three labels drifted before.
#[must_use]
pub fn is_a_size(px: u16) -> bool {
    catalog::SIZES
        .iter()
        .any(|choice| choice.value.parse::<u16>() == Ok(px))
}

/// **What a shop calls this height** — "1" to "10", the numbers on the
/// dropdown.
///
/// A height that is not exactly on the list (the layout caps to whatever fits,
/// which is rarely one of ten round numbers) reports as the nearest one below
/// it, because that is the size a person would have had to pick to get this.
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
    // Below the smallest on the list is still the smallest a person can ask
    // for, so it is what they should be told.
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
    /// **May we be told when the counter stops unexpectedly?** (P22, audit E8.)
    ///
    /// Off until the shop says otherwise, and it is genuinely their decision.
    /// **The report is written either way** — D95: whether a crash report
    /// exists on this computer is not a question about telemetry, it is what
    /// makes the shop's own support call solvable. This flag is only about
    /// sending it, and there is nowhere to send it until Phase 8.
    pub send_crash_reports: bool,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        BackupPolicy {
            folder: String::new(),
            second_folder: String::new(),
            every_hours: 24,
            keep_count: 30,
            // **Off.** Consent is given, not assumed — and the report exists
            // on this computer whichever way this is set (D95).
            send_crash_reports: false,
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
    /// P26. One setting, and D72's test walks every leaf of this JSON both ways
    /// — so a field here with no catalogue entry fails the build, and so does a
    /// catalogue entry with no field.
    pub stock: Stock,
    pub backup: BackupPolicy,
    pub appearance: Appearance,
    /// P29. The scanner, the scale, the customer display and the label
    /// printer — every one of them optional.
    pub devices: Devices,
}

/// **P29 — the things a counter is plugged into** (scope 7.6–7.9).
///
/// Every one of them is optional, and every one of them can be absent,
/// unplugged, broken or slow. **Not one of them may ever block a sale**, which
/// is why an empty port here is a normal, complete configuration and not a
/// half-finished one.
///
/// The scanner needs no port at all: it is a keyboard, and the only setting it
/// has is how to tell it apart from a person typing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Devices {
    /// The longest AVERAGE gap between keystrokes that still counts as a
    /// machine. See [`mb_core::devices::ScanRule`] for why this is the shape
    /// of the question.
    pub scan_average_gap_ms: u32,
    /// One gap longer than this ends the burst, whatever the average.
    pub scan_single_gap_ms: u32,
    /// Shorter than this and it is not a barcode at all.
    pub scan_min_length: u32,

    /// The leading digits that mark a weight-encoded label. Empty means this
    /// shop does not use them, which is most shops.
    pub label_prefix: String,
    pub label_item_from: u32,
    pub label_item_len: u32,
    pub label_value_from: u32,
    pub label_value_len: u32,
    /// `quantity` or `price` — what the number inside the label means.
    pub label_value_is: String,
    /// How many of the value's digits are after the decimal point.
    pub label_value_decimals: u32,

    /// `COM3`, or empty for a shop with no scale — which is nearly all of
    /// them.
    pub scale_port: String,
    pub scale_baud: u32,
    /// `status_then_weight`, `weight_only` or `raw`.
    ///
    /// **`raw` is a tool and not a fallback**: it shows exactly what the scale
    /// is sending, which is how a dealer configures a brand nobody here has
    /// ever seen. It is the difference between "we support your scale" and
    /// "we support scales".
    pub scale_protocol: String,

    /// Whether a second screen shows the customer their bill as it is typed.
    pub display_on: bool,
    /// A serial pole display's port. Empty means the second WINDOW, which is
    /// what a shop with a spare monitor uses.
    pub display_port: String,
    /// What the display says when there is no bill on the screen.
    pub display_idle: String,

    /// Which printer parcel labels go to. Empty means the shop prints none,
    /// and the label button is not offered.
    pub label_printer: String,
}

impl Default for Devices {
    fn default() -> Self {
        let scan = mb_core::devices::ScanRule::default();
        Devices {
            scan_average_gap_ms: scan.max_average_gap_ms,
            scan_single_gap_ms: scan.max_single_gap_ms,
            scan_min_length: u32::try_from(scan.min_length).unwrap_or(8),
            // The commonest Indian convention, and still off until a shop
            // types a prefix: guessing here would turn ordinary EAN-13
            // barcodes into weights.
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

    /// The label rule, or `None` when this shop does not use weight-encoded
    /// labels — which is the ordinary case and not a misconfiguration.
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
    /// **How big a count variance has to be before the screen asks why.**
    ///
    /// In rupees and not in a percentage, because a shop thinks in rupees and
    /// because 5% of a cheap material is noise while ₹500 of paneer is a
    /// question. Zero asks every time.
    pub count_reason_above: Money,
}

impl Default for Stock {
    fn default() -> Self {
        Stock {
            count_reason_above: Money::from_paise(50_000),
        }
    }
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
        // **What an older build wrote, in the words this one uses.**
        let stored = modernise(entry, stored);
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

/// **A value an older build wrote, in the words this one uses.**
///
/// # Why this exists at all
///
/// A text size used to be the ESC/POS multiplier — `1`, `2` or `3` — and is a
/// height in dots since 2026-08-17. Every shop that has ever changed a size has
/// one of the old numbers on disk, and `Kind::check` runs BEFORE `write`, so
/// the compatibility built into the `size!` macro was never reached: the
/// counter opened, refused its own settings row, and fell back to the standard
/// receipt with a line in the log. Found by opening a shop that had a size set.
///
/// # Why here and not in the macro
///
/// Because the check is the thing that rejected it, and the check reads the
/// catalogue's list. This is the one place that knows a stored value has a
/// history, it is named, and it is keyed on the exact list that changed — so
/// when there is nothing left on disk from before that day, deleting this
/// function is the whole of removing it.
fn modernise(entry: &catalog::Entry, stored: Value) -> Value {
    let Value::Text(text) = &stored else {
        return stored;
    };
    // Sizes and nothing else. `ptr::eq` on the choice list rather than a match
    // on the key, so a size added to another section is covered without
    // anybody remembering to add it here.
    let value::Kind::Choice(choices) = entry.kind else {
        return stored;
    };
    if !std::ptr::eq(choices, catalog::SIZES) {
        return stored;
    }
    let Ok(stored_dots) = text.parse::<u16>() else {
        return stored;
    };
    // **Every vocabulary this row has held, told apart exactly** — P32.
    //
    // 1/2/3 was the multiplier, 16…72 was a nominal row height, 9…41 is a cap
    // height. The three sets are deliberately disjoint (see `Style::LADDER`),
    // so this is a lookup and not a guess — and it is the SAME function the
    // catalogue's `size!` writer and `Style`'s own deserialiser use, so a
    // database row, a config file and an export cannot be read three ways.
    let dots = mb_print::Style::from_stored(stored_dots);

    // **A height that is no longer on the list becomes the nearest one that
    // is.**
    //
    // The list has changed three times in one day — three multipliers, then
    // twenty-two `px` values, then ten plain numbers — and each time a shop had
    // rows on disk holding a value the new list did not contain. `Kind::check`
    // refuses those, so the counter opened, refused its own settings row, and
    // printed the STANDARD receipt while the screen still said what the shop
    // had chosen. The owner hit it twice.
    //
    // Snapping is the honest answer: a shop that chose 46 dots wanted "a bit
    // bigger than normal", and the nearest thing this build can print is what
    // they should get — not the default, and not a counter that refuses to
    // read its own configuration.
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
