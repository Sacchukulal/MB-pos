//! **The table.** Every setting in the product, exactly once.
//!
//! A line here is the whole of a setting: its key, the section it appears in,
//! what it is called on screen, the sentence that explains it, the words a
//! person might search for it by, what it may hold, where it is kept, and how
//! to read and write it on a [`ShopConfig`].
//!
//! Nothing else in the product may hard-code any of those facts. Audit **E6**
//! is a duplication bug — *"one giant command with 41 numbered slots… this has
//! already caused a 'reuse slot 39 for four columns' patch"* — and duplication
//! is what a second copy of "the maximum logo width is 100" would be.
//!
//! # The guard
//!
//! `the_catalogue_is_the_whole_of_the_configuration` serialises
//! `ShopConfig::default()`, walks every leaf of the resulting JSON, and asserts
//! that the set of leaf paths is **exactly** the set of keys here. Adding a
//! field to `ReceiptSettings` without adding a line to this file therefore
//! fails the build, and so does leaving a key here for a field that has gone.
//! That is D40's rule — *"the rules that erode are enforced by scripts, not by
//! agreement"* — applied to the one file most likely to erode.

use mb_core::Money;
use mb_print::doc::Pattern;
use mb_print::settings::{LogoPosition, QrMode, RowHeight};

use super::value::{Choice, Invalid, Kind, Shape, Value};
use super::{ShopConfig, Storage};

/// Which screen a setting appears on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Store,
    Tax,
    Receipt,
    Kitchen,
    Billing,
    Day,
    Backup,
}

impl Group {
    /// **In the order the sections appear**, which is the order a shop is set
    /// up in: who you are, what you owe, what you print, how you bill, and
    /// what happens if the disk dies.
    pub const ALL: &'static [Group] = &[
        Group::Store,
        Group::Tax,
        Group::Receipt,
        Group::Kitchen,
        Group::Billing,
        Group::Day,
        Group::Backup,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Group::Store => "Your shop",
            Group::Tax => "Tax",
            Group::Receipt => "The bill",
            Group::Kitchen => "The kitchen ticket",
            Group::Billing => "Billing",
            Group::Day => "The day",
            Group::Backup => "Backup",
        }
    }

    /// The stable name this group crosses the wire as.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Group::Store => "store",
            Group::Tax => "tax",
            Group::Receipt => "receipt",
            Group::Kitchen => "kitchen",
            Group::Billing => "billing",
            Group::Day => "day",
            Group::Backup => "backup",
        }
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Group> {
        Group::ALL.iter().copied().find(|g| g.code() == code)
    }
}

/// One setting.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub key: &'static str,
    pub group: Group,
    pub storage: Storage,
    pub label: &'static str,
    /// A sentence, from the cashier's side of the screen (UI_GUIDELINES §6).
    pub help: &'static str,
    /// What a person might type looking for this. Lower-case, always.
    pub synonyms: &'static [&'static str],
    pub kind: Kind,
    pub read: fn(&ShopConfig) -> Value,
    pub write: fn(&mut ShopConfig, &Value) -> Result<(), Invalid>,
}

// ---------------------------------------------------------------------------
// The choices. Each list's stored values are the serde names of the enum it
// mirrors, so a row is readable in a SQLite browser and survives a relabelling.
// ---------------------------------------------------------------------------

/// An empty stored value, which for a choice means "not chosen yet". Named,
/// because an unexplained `""` in a list of state codes reads like a mistake.
const NOT_CHOSEN: &str = "";

const PATTERNS: &[Choice] = &[
    Choice { value: "dashed", label: "Dashed  - - -" },
    Choice { value: "dotted", label: "Dotted  . . ." },
    Choice { value: "solid", label: "Solid  ___" },
    Choice { value: "bold", label: "Bold  ===" },
    Choice { value: "double", label: "Double line" },
];

// **There is no FONTS list, and there was one until T1 ran** — decision D71.
// The product embeds one face (D33), `layout` does not carry a family into
// `Laid`, and the raster sink draws with the face the queue loaded at start-up.
// Rendering the bill with each of the three choices produced three identical
// documents. See the note where `FontFamily` used to live, in mb-print's doc.rs.

const ROW_HEIGHTS: &[Choice] = &[
    Choice { value: "compact", label: "Compact — least paper" },
    Choice { value: "standard", label: "Standard" },
    Choice { value: "relaxed", label: "Relaxed — easiest to read" },
];

const SIZES: &[Choice] = &[
    Choice { value: "1", label: "Normal" },
    Choice { value: "2", label: "Large" },
    Choice { value: "3", label: "Extra large" },
];

const LOGO_POSITIONS: &[Choice] = &[
    Choice { value: "none", label: "Do not print the logo" },
    Choice { value: "top", label: "At the top" },
];

const QR_MODES: &[Choice] = &[
    Choice { value: "none", label: "No QR code" },
    Choice { value: "static", label: "Static — the customer types the amount" },
    Choice { value: "dynamic", label: "Dynamic — the amount is in the code" },
];

const MATCH_MODES: &[Choice] = &[
    Choice { value: "starts_with", label: "Starts with what I type" },
    Choice { value: "contains", label: "Contains what I type" },
];

const ROUNDING: &[Choice] = &[
    Choice { value: "none", label: "Print the paise" },
    Choice { value: "nearest_rupee", label: "To the nearest rupee" },
    Choice { value: "up", label: "Always up" },
    Choice { value: "down", label: "Always down" },
];

const ORDER_TYPES: &[Choice] = &[
    Choice { value: "dine_in", label: "Dine in" },
    Choice { value: "parcel", label: "Parcel" },
    Choice { value: "self_service", label: "Self service" },
    Choice { value: "delivery", label: "Delivery" },
];

const PLACES_OF_SUPPLY: &[Choice] = &[
    Choice { value: "intra", label: "In my own state — CGST and SGST" },
    Choice { value: "inter", label: "Another state — IGST" },
];

const GST_RATES: &[Choice] = &[
    Choice { value: "0", label: "No GST" },
    Choice { value: "500", label: "5%" },
    Choice { value: "1200", label: "12%" },
    Choice { value: "1800", label: "18%" },
    Choice { value: "2800", label: "28%" },
];

/// The GST state codes, which are also the first two characters of every GSTIN.
///
/// The list is here rather than in a data file because it is a legal constant:
/// it changes when a state is created, which has happened twice in fifty years.
const STATES: &[Choice] = &[
    Choice { value: NOT_CHOSEN, label: "Not chosen yet" },
    Choice { value: "01", label: "Jammu and Kashmir" },
    Choice { value: "02", label: "Himachal Pradesh" },
    Choice { value: "03", label: "Punjab" },
    Choice { value: "04", label: "Chandigarh" },
    Choice { value: "05", label: "Uttarakhand" },
    Choice { value: "06", label: "Haryana" },
    Choice { value: "07", label: "Delhi" },
    Choice { value: "08", label: "Rajasthan" },
    Choice { value: "09", label: "Uttar Pradesh" },
    Choice { value: "10", label: "Bihar" },
    Choice { value: "11", label: "Sikkim" },
    Choice { value: "12", label: "Arunachal Pradesh" },
    Choice { value: "13", label: "Nagaland" },
    Choice { value: "14", label: "Manipur" },
    Choice { value: "15", label: "Mizoram" },
    Choice { value: "16", label: "Tripura" },
    Choice { value: "17", label: "Meghalaya" },
    Choice { value: "18", label: "Assam" },
    Choice { value: "19", label: "West Bengal" },
    Choice { value: "20", label: "Jharkhand" },
    Choice { value: "21", label: "Odisha" },
    Choice { value: "22", label: "Chhattisgarh" },
    Choice { value: "23", label: "Madhya Pradesh" },
    Choice { value: "24", label: "Gujarat" },
    Choice { value: "26", label: "Dadra and Nagar Haveli and Daman and Diu" },
    Choice { value: "27", label: "Maharashtra" },
    Choice { value: "29", label: "Karnataka" },
    Choice { value: "30", label: "Goa" },
    Choice { value: "31", label: "Lakshadweep" },
    Choice { value: "32", label: "Kerala" },
    Choice { value: "33", label: "Tamil Nadu" },
    Choice { value: "34", label: "Puducherry" },
    Choice { value: "35", label: "Andaman and Nicobar Islands" },
    Choice { value: "36", label: "Telangana" },
    Choice { value: "37", label: "Andhra Pradesh" },
    Choice { value: "38", label: "Ladakh" },
    Choice { value: "97", label: "Other territory" },
];

// ---------------------------------------------------------------------------
// The macros. One line per setting, and the shape of that line is the same for
// every setting of the same kind — which is what makes ninety of them
// reviewable.
// ---------------------------------------------------------------------------

macro_rules! flag {
    ($key:literal, $group:ident, $storage:ident, $label:literal, $help:literal,
     [$($syn:literal),* $(,)?], $($field:ident).+) => {
        Entry {
            key: $key,
            group: Group::$group,
            storage: Storage::$storage,
            label: $label,
            help: $help,
            synonyms: &[$($syn),*],
            kind: Kind::Bool,
            read: |c| Value::Bool(c.$($field).+),
            write: |c, v| { c.$($field).+ = v.as_bool()?; Ok(()) },
        }
    };
}

macro_rules! number {
    ($key:literal, $group:ident, $storage:ident, $label:literal, $help:literal,
     [$($syn:literal),* $(,)?], $min:literal ..= $max:literal $unit:literal,
     $ty:ty, $($field:ident).+) => {
        Entry {
            key: $key,
            group: Group::$group,
            storage: Storage::$storage,
            label: $label,
            help: $help,
            synonyms: &[$($syn),*],
            kind: Kind::Int { min: $min, max: $max, unit: $unit },
            read: |c| Value::Int(i64::from(c.$($field).+)),
            write: |c, v| {
                c.$($field).+ = <$ty>::try_from(v.as_int()?)
                    .map_err(|_| Invalid::new("That number is outside what this can hold."))?;
                Ok(())
            },
        }
    };
}

macro_rules! cash {
    ($key:literal, $group:ident, $storage:ident, $label:literal, $help:literal,
     [$($syn:literal),* $(,)?], $min:literal ..= $max:literal, $($field:ident).+) => {
        Entry {
            key: $key,
            group: Group::$group,
            storage: Storage::$storage,
            label: $label,
            help: $help,
            synonyms: &[$($syn),*],
            kind: Kind::Money { min_paise: $min, max_paise: $max },
            read: |c| Value::Money(c.$($field).+),
            write: |c, v| { c.$($field).+ = v.as_money()?; Ok(()) },
        }
    };
}

macro_rules! words {
    ($key:literal, $group:ident, $storage:ident, $label:literal, $help:literal,
     [$($syn:literal),* $(,)?], $max:literal, $shape:ident, $($field:ident).+) => {
        Entry {
            key: $key,
            group: Group::$group,
            storage: Storage::$storage,
            label: $label,
            help: $help,
            synonyms: &[$($syn),*],
            kind: Kind::Text { max_len: $max, shape: Shape::$shape },
            read: |c| Value::Text(c.$($field).+.clone()),
            write: |c, v| { c.$($field).+ = v.as_text()?.to_owned(); Ok(()) },
        }
    };
}

/// A choice whose stored value IS the field — a plain `String`.
macro_rules! pick_text {
    ($key:literal, $group:ident, $storage:ident, $label:literal, $help:literal,
     [$($syn:literal),* $(,)?], $options:expr, $($field:ident).+) => {
        Entry {
            key: $key,
            group: Group::$group,
            storage: Storage::$storage,
            label: $label,
            help: $help,
            synonyms: &[$($syn),*],
            kind: Kind::Choice($options),
            read: |c| Value::Text(c.$($field).+.clone()),
            write: |c, v| { c.$($field).+ = v.as_text()?.to_owned(); Ok(()) },
        }
    };
}

/// A choice that mirrors an enum, with the two conversion functions beside it.
macro_rules! pick {
    ($key:literal, $group:ident, $storage:ident, $label:literal, $help:literal,
     [$($syn:literal),* $(,)?], $options:expr, $to:path, $from:path, $($field:ident).+) => {
        Entry {
            key: $key,
            group: Group::$group,
            storage: Storage::$storage,
            label: $label,
            help: $help,
            synonyms: &[$($syn),*],
            kind: Kind::Choice($options),
            read: |c| Value::Text($to(c.$($field).+).to_owned()),
            write: |c, v| {
                c.$($field).+ = $from(v.as_text()?)
                    .ok_or_else(|| Invalid::new("That is not one of the choices here."))?;
                Ok(())
            },
        }
    };
}

/// Per-section text size: 1, 2 or 3 — the ESC/POS multiplier, which is what
/// "Normal / Large / Extra large" means on a thermal printer.
macro_rules! size {
    ($key:literal, $group:ident, $label:literal, $help:literal,
     [$($syn:literal),* $(,)?], $($field:ident).+) => {
        Entry {
            key: $key,
            group: Group::$group,
            storage: Storage::Row,
            label: $label,
            help: $help,
            synonyms: &[$($syn),*],
            kind: Kind::Choice(SIZES),
            read: |c| Value::Text(c.$($field).+.scale().to_string()),
            write: |c, v| {
                c.$($field).+.scale = match v.as_text()? {
                    "1" => 1,
                    "2" => 2,
                    "3" => 3,
                    _ => return Err(Invalid::new("A size here is Normal, Large or Extra large.")),
                };
                Ok(())
            },
        }
    };
}

// ---------------------------------------------------------------------------
// The conversions the `pick!` entries use. Each pair is `to` and `from` for one
// enum, and the strings are that enum's own serde names — so a stored row and a
// serialised struct always agree.
// ---------------------------------------------------------------------------

const fn pattern_to(p: Pattern) -> &'static str {
    match p {
        Pattern::Dashed => "dashed",
        Pattern::Dotted => "dotted",
        Pattern::Solid => "solid",
        Pattern::Bold => "bold",
        Pattern::Double => "double",
    }
}

fn pattern_from(text: &str) -> Option<Pattern> {
    match text {
        "dashed" => Some(Pattern::Dashed),
        "dotted" => Some(Pattern::Dotted),
        "solid" => Some(Pattern::Solid),
        "bold" => Some(Pattern::Bold),
        "double" => Some(Pattern::Double),
        _ => None,
    }
}

const fn row_height_to(r: RowHeight) -> &'static str {
    match r {
        RowHeight::Compact => "compact",
        RowHeight::Standard => "standard",
        RowHeight::Relaxed => "relaxed",
    }
}

fn row_height_from(text: &str) -> Option<RowHeight> {
    match text {
        "compact" => Some(RowHeight::Compact),
        "standard" => Some(RowHeight::Standard),
        "relaxed" => Some(RowHeight::Relaxed),
        _ => None,
    }
}

const fn logo_to(l: LogoPosition) -> &'static str {
    match l {
        LogoPosition::None => "none",
        LogoPosition::Top => "top",
    }
}

fn logo_from(text: &str) -> Option<LogoPosition> {
    match text {
        "none" => Some(LogoPosition::None),
        "top" => Some(LogoPosition::Top),
        _ => None,
    }
}

const fn qr_to(q: QrMode) -> &'static str {
    match q {
        QrMode::None => "none",
        QrMode::Static => "static",
        QrMode::Dynamic => "dynamic",
    }
}

fn qr_from(text: &str) -> Option<QrMode> {
    match text {
        "none" => Some(QrMode::None),
        "static" => Some(QrMode::Static),
        "dynamic" => Some(QrMode::Dynamic),
        _ => None,
    }
}

const fn match_to(m: crate::search::MatchMode) -> &'static str {
    match m {
        crate::search::MatchMode::StartsWith => "starts_with",
        crate::search::MatchMode::Contains => "contains",
    }
}

fn match_from(text: &str) -> Option<crate::search::MatchMode> {
    match text {
        "starts_with" => Some(crate::search::MatchMode::StartsWith),
        "contains" => Some(crate::search::MatchMode::Contains),
        _ => None,
    }
}

const fn rounding_to(r: mb_core::RoundingMode) -> &'static str {
    match r {
        mb_core::RoundingMode::None => "none",
        mb_core::RoundingMode::NearestRupee => "nearest_rupee",
        mb_core::RoundingMode::Up => "up",
        mb_core::RoundingMode::Down => "down",
    }
}

fn rounding_from(text: &str) -> Option<mb_core::RoundingMode> {
    match text {
        "none" => Some(mb_core::RoundingMode::None),
        "nearest_rupee" => Some(mb_core::RoundingMode::NearestRupee),
        "up" => Some(mb_core::RoundingMode::Up),
        "down" => Some(mb_core::RoundingMode::Down),
        _ => None,
    }
}

const fn order_type_to(o: mb_core::OrderType) -> &'static str {
    match o {
        mb_core::OrderType::DineIn => "dine_in",
        mb_core::OrderType::Parcel => "parcel",
        mb_core::OrderType::SelfService => "self_service",
        mb_core::OrderType::Delivery => "delivery",
    }
}

fn order_type_from(text: &str) -> Option<mb_core::OrderType> {
    match text {
        "dine_in" => Some(mb_core::OrderType::DineIn),
        "parcel" => Some(mb_core::OrderType::Parcel),
        "self_service" => Some(mb_core::OrderType::SelfService),
        "delivery" => Some(mb_core::OrderType::Delivery),
        _ => None,
    }
}

/// A GST rate, stored as basis points in a choice so the five legal rates are
/// the only five that can be chosen.
fn rate_to(bp: u32) -> &'static str {
    match bp {
        500 => "500",
        1_200 => "1200",
        1_800 => "1800",
        2_800 => "2800",
        _ => "0",
    }
}

fn rate_from(text: &str) -> Option<u32> {
    match text {
        "0" => Some(0),
        "500" => Some(500),
        "1200" => Some(1_200),
        "1800" => Some(1_800),
        "2800" => Some(2_800),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// THE TABLE.
// ---------------------------------------------------------------------------

pub const CATALOG: &[Entry] = &[
    // --- your shop (audit Part 3, "Store Information" and "UPI Payment") ----
    words!("store.name", Store, Store, "Shop name",
        "Printed at the top of every bill.", ["hotel", "restaurant", "title"], 60, Free, store.name),
    words!("store.address", Store, Store, "Address",
        "Printed under the name. Long addresses wrap onto a second line.",
        ["location", "street"], 200, Free, store.address),
    words!("store.phone", Store, Store, "Phone number",
        "Ten digits. Printed on the bill so a customer can call about an order.",
        ["mobile", "contact", "telephone"], 10, Phone, store.phone),
    words!("store.gstin", Store, Store, "GST number",
        "Fifteen characters. It is checked against its own checksum and against \
         your state, because a GST number that does not match its state is the \
         commonest cause of a rejected return.",
        ["gst", "gstin", "tax number"], 15, Gstin, store.gstin),
    words!("store.fssai", Store, Store, "FSSAI licence number",
        "Fourteen digits. Every food business must print it.",
        ["food licence", "fssai"], 14, Fssai, store.fssai),
    pick_text!("store.state_code", Store, Store, "State",
        "Which state you are in. This decides whether a bill charges CGST and \
         SGST or IGST.",
        ["state", "karnataka", "igst", "cgst"], STATES, store.state_code),
    words!("store.upi_id", Store, Store, "UPI id",
        "Where the QR code on the bill sends money. Looks like name@bank.",
        ["upi", "qr", "gpay", "phonepe", "payment"], 60, UpiId, store.upi_id),
    words!("store.upi_merchant_name", Store, Store, "UPI name",
        "The name the customer sees in their payment app. Your shop name is used \
         if this is blank.",
        ["upi", "merchant", "payee"], 40, Free, store.upi_merchant_name),
    words!("store.upi_reference", Store, Store, "UPI reference",
        "Carried inside the QR code so your bank statement says which counter a \
         payment came from.",
        ["upi", "reference", "note", "statement"], 40, Free, store.upi_reference),

    // --- tax ---------------------------------------------------------------
    // is_composition and the place of supply are STORED on the profile and
    // BELONG on the tax screen. That is why group and storage are two things.
    flag!("store.is_composition", Tax, Store, "I am under the composition scheme",
        "A composition dealer may not collect GST. The bill prints a declaration \
         saying so instead of a tax breakdown.",
        ["composition", "scheme", "small dealer"], store.is_composition),
    pick_text!("store.default_place_of_supply", Tax, Store, "Where you usually supply",
        "In your own state a bill charges CGST and SGST; outside it, IGST. A \
         single bill can still be changed.",
        ["igst", "cgst", "sgst", "inter state", "place of supply"],
        PLACES_OF_SUPPLY, store.default_place_of_supply),
    words!("tax.default_class_id", Tax, Row, "Tax rate for a new item",
        "Which tax class the Menu screen offers first when you add an item.",
        ["default", "rate", "class", "new item"], 40, Free, tax.default_class_id),
    flag!("tax.prices_include_tax", Tax, Row, "Menu prices already include tax",
        "On: a 100 rupee dosa is 100 rupees on the bill and the tax is worked \
         back out of it. Off: tax is added on top.",
        ["inclusive", "exclusive", "included", "on top"], tax.prices_include_tax),

    // --- the bill (audit Part 3, "Bill Settings" — every row) --------------
    // Part 3's "Font" row is NOT here. See D71 and the note in mb-print's
    // doc.rs: there is one embedded face, so the choice changed nothing. It
    // returns at P23, which ships a second face for Kannada anyway.
    pick!("receipt.pattern", Receipt, Row, "Separator line",
        "What the dividing lines on the bill look like.",
        ["line", "divider", "dashes", "dotted"], PATTERNS, pattern_to, pattern_from,
        receipt.pattern),
    pick!("receipt.row_height", Receipt, Row, "Row height",
        "How much air there is between items and around the total.",
        ["spacing", "compact", "relaxed", "air", "paper"], ROW_HEIGHTS,
        row_height_to, row_height_from, receipt.row_height),

    flag!("receipt.show.token", Receipt, Row, "Print the token number",
        "The big number the customer waits for.", ["token", "number"], receipt.show.token),
    flag!("receipt.show.gstin", Receipt, Row, "Print your GST number",
        "In the header, under the address.", ["gst", "gstin"], receipt.show.gstin),
    flag!("receipt.show.fssai", Receipt, Row, "Print your FSSAI number",
        "In the header.", ["fssai", "food licence"], receipt.show.fssai),
    flag!("receipt.show.address", Receipt, Row, "Print your address",
        "In the header, under the shop name.", ["address"], receipt.show.address),
    flag!("receipt.show.phone", Receipt, Row, "Print your phone number",
        "In the header.", ["phone", "mobile"], receipt.show.phone),
    flag!("receipt.show.cashier", Receipt, Row, "Print who billed it",
        "The name of whoever was signed in.", ["cashier", "staff", "who"],
        receipt.show.cashier),
    flag!("receipt.show.hsn", Receipt, Row, "Print the HSN code",
        "Needed above the turnover threshold. It costs six characters of a \
         narrow bill.", ["hsn", "sac", "code"], receipt.show.hsn),
    flag!("receipt.show.tax_summary", Receipt, Row, "Print the rate-wise tax summary",
        "The block a chartered accountant looks at first. Turning it off makes \
         a bill harder to file from.",
        ["gst", "summary", "cgst", "sgst", "rate wise"], receipt.show.tax_summary),
    flag!("receipt.show.payment_lines", Receipt, Row, "Print how it was paid",
        "\"Cash 300 / UPI 200\" under the total.",
        ["payment", "split", "cash", "card"], receipt.show.payment_lines),

    flag!("receipt.separators.below_store_header", Receipt, Row, "Line under the shop header",
        "", ["separator", "line", "header"], receipt.separators.below_store_header),
    flag!("receipt.separators.below_meta", Receipt, Row, "Line under the bill details",
        "", ["separator", "line", "details"], receipt.separators.below_meta),
    flag!("receipt.separators.below_token", Receipt, Row, "Line under the token",
        "", ["separator", "line", "token"], receipt.separators.below_token),
    flag!("receipt.separators.below_column_names", Receipt, Row, "Line under the column names",
        "", ["separator", "line", "columns"], receipt.separators.below_column_names),
    flag!("receipt.separators.below_items", Receipt, Row, "Line under the items",
        "", ["separator", "line", "items"], receipt.separators.below_items),
    flag!("receipt.separators.below_subtotals", Receipt, Row, "Line above the total",
        "", ["separator", "line", "subtotal"], receipt.separators.below_subtotals),
    flag!("receipt.separators.below_grand_total", Receipt, Row, "Line under the total",
        "", ["separator", "line", "total"], receipt.separators.below_grand_total),

    size!("receipt.sections.store_name.scale", Receipt, "Shop name size",
        "", ["size", "big", "header", "name"], receipt.sections.store_name),
    flag!("receipt.sections.store_name.bold", Receipt, Row, "Shop name in bold",
        "", ["bold", "header", "name"], receipt.sections.store_name.bold),
    size!("receipt.sections.meta.scale", Receipt, "Bill details size",
        "", ["size", "details", "address"], receipt.sections.meta),
    flag!("receipt.sections.meta.bold", Receipt, Row, "Bill details in bold",
        "", ["bold", "details"], receipt.sections.meta.bold),
    size!("receipt.sections.items.scale", Receipt, "Item list size",
        "", ["size", "items"], receipt.sections.items),
    flag!("receipt.sections.items.bold", Receipt, Row, "Item list in bold",
        "", ["bold", "items"], receipt.sections.items.bold),
    size!("receipt.sections.subtotals.scale", Receipt, "Subtotal and GST size",
        "", ["size", "subtotal", "gst"], receipt.sections.subtotals),
    flag!("receipt.sections.subtotals.bold", Receipt, Row, "Subtotal and GST in bold",
        "", ["bold", "subtotal"], receipt.sections.subtotals.bold),
    size!("receipt.sections.grand_total.scale", Receipt, "Total size",
        "", ["size", "total", "grand"], receipt.sections.grand_total),
    flag!("receipt.sections.grand_total.bold", Receipt, Row, "Total in bold",
        "", ["bold", "total"], receipt.sections.grand_total.bold),
    size!("receipt.sections.footer.scale", Receipt, "Footer size",
        "", ["size", "footer", "thank you"], receipt.sections.footer),
    flag!("receipt.sections.footer.bold", Receipt, Row, "Footer in bold",
        "", ["bold", "footer"], receipt.sections.footer.bold),
    size!("receipt.sections.token.scale", Receipt, "Token size",
        "The customer reads this one across a room.",
        ["size", "token", "big"], receipt.sections.token),
    flag!("receipt.sections.token.bold", Receipt, Row, "Token in bold",
        "", ["bold", "token"], receipt.sections.token.bold),

    pick!("receipt.logo", Receipt, Row, "Logo",
        "Where your logo prints, if you have uploaded one.",
        ["logo", "image", "picture", "brand"], LOGO_POSITIONS, logo_to, logo_from,
        receipt.logo),
    number!("receipt.logo_width_pct", Receipt, Row, "Logo width",
        "As a percentage of the paper width.", ["logo", "size", "width"],
        10..=100 "%", u8, receipt.logo_width_pct),
    pick!("receipt.qr", Receipt, Row, "UPI QR code",
        "Dynamic carries the amount, so the customer does not type it.",
        ["qr", "upi", "scan", "payment"], QR_MODES, qr_to, qr_from, receipt.qr),
    number!("receipt.qr_width_pct", Receipt, Row, "QR code width",
        "As a percentage of the paper width. Too small and a phone cannot read it.",
        ["qr", "size", "width"], 10..=100 "%", u8, receipt.qr_width_pct),
    words!("receipt.footer", Receipt, Row, "Footer message",
        "The last line of the bill.", ["thank you", "footer", "message", "bottom"],
        120, Free, receipt.footer),
    words!("receipt.composition_note", Receipt, Row, "Composition declaration",
        "Printed instead of a tax breakdown when you are under the composition \
         scheme.", ["composition", "declaration", "note"], 160, Free,
        receipt.composition_note),

    // --- the kitchen ticket (audit Part 3, "KOT visibility" / "KOT sections")
    flag!("kitchen.show_title", Kitchen, Row, "Print the word KITCHEN",
        "", ["kot", "title", "heading"], kitchen.show_title),
    flag!("kitchen.show_token", Kitchen, Row, "Print the token number",
        "", ["kot", "token"], kitchen.show_token),
    flag!("kitchen.show_bill_number", Kitchen, Row, "Print the bill number",
        "", ["kot", "bill number"], kitchen.show_bill_number),
    flag!("kitchen.show_order_type", Kitchen, Row, "Print the order type",
        "Dine in, parcel, delivery.", ["kot", "type", "parcel"],
        kitchen.show_order_type),
    flag!("kitchen.show_table", Kitchen, Row, "Print the table",
        "", ["kot", "table"], kitchen.show_table),
    flag!("kitchen.show_time", Kitchen, Row, "Print the time",
        "", ["kot", "time", "clock"], kitchen.show_time),
    flag!("kitchen.show_column_names", Kitchen, Row, "Print column names",
        "A \"Qty  Item\" heading above the food.", ["kot", "columns", "heading"],
        kitchen.show_column_names),
    flag!("kitchen.two_column", Kitchen, Row, "Two dishes per line",
        "Half the paper, harder to read. Notes are packed onto the dish's own \
         line.", ["kot", "two column", "packing", "short", "paper"],
        kitchen.two_column),
    pick!("kitchen.pattern", Kitchen, Row, "Separator line",
        "", ["kot", "line", "divider"], PATTERNS, pattern_to, pattern_from,
        kitchen.pattern),
    flag!("kitchen.separators.below_title", Kitchen, Row, "Line under the title",
        "", ["kot", "separator", "title"], kitchen.separators.below_title),
    flag!("kitchen.separators.below_token", Kitchen, Row, "Line under the token",
        "", ["kot", "separator", "token"], kitchen.separators.below_token),
    flag!("kitchen.separators.below_details", Kitchen, Row, "Line under the details",
        "", ["kot", "separator", "details"], kitchen.separators.below_details),
    flag!("kitchen.separators.below_column_names", Kitchen, Row, "Line under the column names",
        "", ["kot", "separator", "columns"], kitchen.separators.below_column_names),
    flag!("kitchen.separators.below_items", Kitchen, Row, "Line under the food",
        "", ["kot", "separator", "items"], kitchen.separators.below_items),
    pick!("kitchen.row_height", Kitchen, Row, "Row height",
        "How much air there is between dishes. The kitchen reads this at speed.",
        ["kot", "spacing", "compact", "relaxed"], ROW_HEIGHTS, row_height_to,
        row_height_from, kitchen.row_height),
    size!("kitchen.title.scale", Kitchen, "Title size",
        "", ["kot", "size", "title"], kitchen.title),
    flag!("kitchen.title.bold", Kitchen, Row, "Title in bold",
        "", ["kot", "bold", "title"], kitchen.title.bold),
    size!("kitchen.details.scale", Kitchen, "Details size",
        "", ["kot", "size", "details"], kitchen.details),
    flag!("kitchen.details.bold", Kitchen, Row, "Details in bold",
        "", ["kot", "bold", "details"], kitchen.details.bold),
    size!("kitchen.items.scale", Kitchen, "Food size",
        "The kitchen reads this across a hot room.", ["kot", "size", "items"],
        kitchen.items),
    flag!("kitchen.items.bold", Kitchen, Row, "Food in bold",
        "", ["kot", "bold", "items"], kitchen.items.bold),

    // --- billing behaviour --------------------------------------------------
    pick!("billing.search_mode", Billing, Row, "How the item search matches",
        "A short menu wants \"starts with\"; a long one wants \"contains\".",
        ["search", "find", "starts with", "contains"], MATCH_MODES, match_to,
        match_from, billing.search_mode),
    pick!("billing.rounding", Billing, Row, "Round off the total",
        "What happens to the paise on the grand total.",
        ["round", "roundoff", "round off", "paise"], ROUNDING, rounding_to,
        rounding_from, billing.rounding),
    flag!("billing.lock_order_type", Billing, Row, "Always start on one order type",
        "For a counter that only does parcels, or only dine-in.",
        ["order type", "lock", "parcel", "default"], billing.lock_order_type),
    pick!("billing.locked_order_type", Billing, Row, "Which order type",
        "Used when the order type is locked.", ["order type", "parcel", "dine in"],
        ORDER_TYPES, order_type_to, order_type_from, billing.locked_order_type),
    flag!("billing.confirm_before_kitchen", Billing, Row, "Ask before printing a kitchen ticket",
        "", ["confirm", "kot", "ask"], billing.confirm_before_kitchen),
    flag!("billing.confirm_before_bill", Billing, Row, "Ask before printing a bill",
        "", ["confirm", "bill", "ask"], billing.confirm_before_bill),
    flag!("billing.kitchen_ticket_off", Billing, Row, "This shop has no kitchen ticket",
        "For a counter where the food is already made.",
        ["kot", "disable", "off", "no kitchen"], billing.kitchen_ticket_off),
    number!("billing.idle_lock_minutes", Billing, Row, "Lock the counter after",
        "Minutes of nobody touching it. 0 never locks by itself.",
        ["lock", "idle", "timeout", "screen"], 0..=240 "minutes", u32,
        billing.idle_lock_minutes),
    number!("billing.service_charge_bp", Billing, Row, "Service charge",
        "In hundredths of a percent, so 500 is 5%. 0 means you do not charge it. \
         Added to dine-in bills only.",
        ["service", "charge", "percent", "tip"], 0..=2500 "hundredths of a percent",
        u32, billing.service_charge_bp),
    pick!("billing.service_charge_tax_bp", Billing, Row, "GST on the service charge",
        "A service charge is usually taxed at 18% even on a bill of 5% food.",
        ["service", "gst", "tax"], GST_RATES, rate_to, rate_from,
        billing.service_charge_tax_bp),
    cash!("billing.packing_charge", Billing, Row, "Packing charge",
        "Added to parcel and self-service bills. Zero means you do not charge it.",
        ["packing", "parcel", "box", "charge"], 0..=100_000, billing.packing_charge),
    pick!("billing.packing_charge_tax_bp", Billing, Row, "GST on the packing charge",
        "", ["packing", "gst", "tax"], GST_RATES, rate_to, rate_from,
        billing.packing_charge_tax_bp),
    cash!("billing.delivery_charge", Billing, Row, "Delivery charge",
        "Added to delivery bills. Zero means you do not charge it.",
        ["delivery", "charge", "rider"], 0..=100_000, billing.delivery_charge),
    pick!("billing.delivery_charge_tax_bp", Billing, Row, "GST on the delivery charge",
        "", ["delivery", "gst", "tax"], GST_RATES, rate_to, rate_from,
        billing.delivery_charge_tax_bp),

    // --- the day ------------------------------------------------------------
    number!("day.starts_at_minutes", Day, Row, "Your day starts at",
        "Minutes past midnight. 300 is 5 in the morning, which means a bill \
         printed at 1 a.m. counts as yesterday's. This changes every report you \
         will ever run.",
        ["day", "business day", "5 am", "close", "midnight", "cutoff"],
        0..=1439 "minutes past midnight", u32, day.starts_at_minutes),

    // --- backup -------------------------------------------------------------
    words!("backup.folder", Backup, Row, "Backup folder",
        "Where backups are written. Blank uses the folder beside your database.",
        ["backup", "folder", "where", "copy"], 260, Folder, backup.folder),
    words!("backup.second_folder", Backup, Row, "Second backup folder",
        "A pen drive or a network share. One copy on one disk is not a backup.",
        ["backup", "second", "pen drive", "usb", "network"], 260, Folder,
        backup.second_folder),
    number!("backup.every_hours", Backup, Row, "Back up every",
        "Hours. 0 means only when you press the button.",
        ["backup", "schedule", "automatic", "how often"], 0..=168 "hours", u32,
        backup.every_hours),
    number!("backup.keep_count", Backup, Row, "Keep this many backups",
        "Older ones are deleted to save disk space.",
        ["backup", "keep", "retention", "delete", "old"], 1..=365 "backups", u32,
        backup.keep_count),
];

/// The entry for a key, or `None` if this build has never heard of it.
#[must_use]
pub fn find(key: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.key == key)
}

/// **The GSTIN's state code must be the shop's own state.**
///
/// Checked here rather than in `value.rs` because only here are both halves
/// known. It is the check nobody does and everybody needs: a Karnataka shop
/// that typed a Kerala GSTIN files a return that is rejected months later.
pub fn check_gstin_against_state(config: &ShopConfig) -> Result<(), Invalid> {
    let (gstin, state) = (&config.store.gstin, &config.store.state_code);
    if gstin.is_empty() || state.is_empty() {
        return Ok(());
    }
    if gstin.starts_with(state.as_str()) {
        return Ok(());
    }
    let named = STATES
        .iter()
        .find(|c| c.value == &gstin[..gstin.len().min(2)])
        .map_or("another state", |c| c.label);
    Err(Invalid::new(format!(
        "That GST number starts with {}, which is {named} — but you have chosen \
         a different state. One of the two is wrong.",
        &gstin[..gstin.len().min(2)]
    ))
    .about("store.gstin"))
}

/// Money crosses this file only as a type; the import is honest about it.
const _: Option<Money> = None;
