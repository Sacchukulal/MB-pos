//! The table. Every setting in the product, exactly once.

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
    /// No scalar settings at all, and that is not an oversight: paper, connection and role
    /// belong to a PRINTER, and a shop has none, one or six of them.
    Printers,
    Numbering,
    Billing,
    Day,
    /// The stock book has exactly one scalar setting: the threshold a count variance has to
    /// pass before the screen asks why.
    Stock,
    Backup,
    Appearance,
    /// The scanner, the scale, the customer display and the label printer.
    Devices,
}

impl Group {
    /// In the order the sections appear, which is the order a shop is set up in: who you are,
    /// what you owe, what you print, how you bill, and what happens if the disk dies.
    pub const ALL: &'static [Group] = &[
        Group::Store,
        Group::Tax,
        Group::Receipt,
        Group::Kitchen,
        Group::Printers,
        Group::Numbering,
        Group::Billing,
        Group::Day,
        Group::Stock,
        Group::Backup,
        Group::Appearance,
        Group::Devices,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Group::Store => "Your shop",
            Group::Tax => "Tax",
            Group::Receipt => "The bill",
            Group::Kitchen => "The kitchen ticket",
            Group::Printers => "Printers",
            Group::Numbering => "Bill and token numbers",
            Group::Billing => "Billing",
            Group::Day => "The day",
            Group::Stock => "Stock",
            Group::Backup => "Backup",
            Group::Appearance => "How it looks",
            Group::Devices => "Devices",
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
            Group::Printers => "printers",
            Group::Numbering => "numbering",
            Group::Billing => "billing",
            Group::Day => "day",
            Group::Stock => "stock",
            Group::Backup => "backup",
            Group::Appearance => "appearance",
            Group::Devices => "devices",
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
    /// A sentence, from the cashier's side of the screen.
    pub help: &'static str,
    /// What a person might type looking for this.
    pub synonyms: &'static [&'static str],
    pub kind: Kind,
    pub read: fn(&ShopConfig) -> Value,
    pub write: fn(&mut ShopConfig, &Value) -> Result<(), Invalid>,
}

// The choices. Each list's stored values are the serde names of the enum it mirrors, so a row
// is readable in a SQLite browser and survives a relabelling.

/// An empty stored value, which for a choice means "not chosen yet".
const NOT_CHOSEN: &str = "";

/// What the number inside a scale's label means.
const LABEL_VALUES: &[Choice] = &[
    Choice {
        value: "quantity",
        label: "Weight or quantity",
    },
    Choice {
        value: "price",
        label: "Price",
    },
];

/// "Show me what it is sending" is a tool, not a fallback — it is how a dealer sets up a scale
/// nobody here has ever seen.
const SCALE_PROTOCOLS: &[Choice] = &[
    Choice {
        value: "status_then_weight",
        label: "Status, then the weight (commonest)",
    },
    Choice {
        value: "weight_only",
        label: "Just the weight",
    },
    Choice {
        value: "raw",
        label: "Show me what it is sending",
    },
];

const PATTERNS: &[Choice] = &[
    Choice {
        value: "dashed",
        label: "Dashed  - - -",
    },
    Choice {
        value: "dotted",
        label: "Dotted  . . .",
    },
    Choice {
        value: "solid",
        label: "Solid  ___",
    },
    Choice {
        value: "bold",
        label: "Bold  ===",
    },
    Choice {
        value: "double",
        label: "Double line",
    },
];

const ROW_HEIGHTS: &[Choice] = &[
    Choice {
        value: "compact",
        label: "Compact — least paper",
    },
    Choice {
        value: "standard",
        label: "Standard",
    },
    Choice {
        value: "relaxed",
        label: "Relaxed — easiest to read",
    },
];

/// Ten sizes, numbered 1 to 10.
pub(super) const SIZES: &[Choice] = &[
    Choice {
        value: "9",
        label: "1",
    },
    Choice {
        value: "11",
        label: "2",
    },
    Choice {
        value: "13",
        label: "3",
    },
    Choice {
        value: "15",
        label: "4",
    },
    Choice {
        value: "17",
        label: "5",
    },
    Choice {
        value: "19",
        label: "6",
    },
    Choice {
        value: "22",
        label: "7",
    },
    Choice {
        value: "26",
        label: "8",
    },
    Choice {
        value: "33",
        label: "9",
    },
    Choice {
        value: "41",
        label: "10",
    },
];

/// The typefaces.
pub(super) const FONTS: &[Choice] = &[
    Choice {
        value: "builtin",
        label: "Magic Bill's own (IBM Plex Mono)",
    },
    Choice {
        value: "consolas",
        label: "Consolas",
    },
    Choice {
        value: "consolas_bold",
        label: "Consolas Bold — darker on faint paper",
    },
    Choice {
        value: "courier",
        label: "Courier New",
    },
    Choice {
        value: "lucida",
        label: "Lucida Console",
    },
    Choice {
        value: "cascadia",
        label: "Cascadia Mono",
    },
    Choice {
        value: "times",
        label: "Times New Roman — a printed-book look",
    },
    Choice {
        value: "georgia",
        label: "Georgia — heavier serif, clear on faint paper",
    },
    Choice {
        value: "arial",
        label: "Arial",
    },
    Choice {
        value: "calibri",
        label: "Calibri — rounder, a little smaller",
    },
    Choice {
        value: "verdana",
        label: "Verdana — widest, easiest to read small",
    },
];

const LOGO_POSITIONS: &[Choice] = &[
    Choice {
        value: "none",
        label: "Do not print the logo",
    },
    Choice {
        value: "top",
        label: "Above the shop name",
    },
    Choice {
        value: "left",
        label: "Beside the shop name, on the left",
    },
    Choice {
        value: "right",
        label: "Beside the shop name, on the right",
    },
];

const QR_MODES: &[Choice] = &[
    Choice {
        value: "none",
        label: "No QR code",
    },
    Choice {
        value: "static",
        label: "Static — the customer types the amount",
    },
    Choice {
        value: "dynamic",
        label: "Dynamic — the amount is in the code",
    },
];

const MATCH_MODES: &[Choice] = &[
    Choice {
        value: "starts_with",
        label: "Starts with what I type",
    },
    Choice {
        value: "contains",
        label: "Contains what I type",
    },
];

const ROUNDING: &[Choice] = &[
    Choice {
        value: "none",
        label: "Print the paise",
    },
    Choice {
        value: "nearest_rupee",
        label: "To the nearest rupee",
    },
    Choice {
        value: "up",
        label: "Always up",
    },
    Choice {
        value: "down",
        label: "Always down",
    },
];

const ORDER_TYPES: &[Choice] = &[
    Choice {
        value: "dine_in",
        label: "Dine in",
    },
    Choice {
        value: "parcel",
        label: "Parcel",
    },
    Choice {
        value: "self_service",
        label: "Self service",
    },
    Choice {
        value: "delivery",
        label: "Delivery",
    },
];

/// What kind of taxpayer the shop is.
const REGISTRATIONS: &[Choice] = &[
    Choice {
        value: "unregistered",
        label: "Not registered",
    },
    Choice {
        value: "composition",
        label: "Composition scheme",
    },
    Choice {
        value: "regular",
        label: "Regular GST",
    },
];

const GST_RATES: &[Choice] = &[
    Choice {
        value: "0",
        label: "No GST",
    },
    Choice {
        value: "500",
        label: "5%",
    },
    Choice {
        value: "1200",
        label: "12%",
    },
    Choice {
        value: "1800",
        label: "18%",
    },
    Choice {
        value: "2800",
        label: "28%",
    },
];

/// The GST state codes, which are also the first two characters of every GSTIN.
const STATES: &[Choice] = &[
    Choice {
        value: NOT_CHOSEN,
        label: "Not chosen yet",
    },
    Choice {
        value: "01",
        label: "Jammu and Kashmir",
    },
    Choice {
        value: "02",
        label: "Himachal Pradesh",
    },
    Choice {
        value: "03",
        label: "Punjab",
    },
    Choice {
        value: "04",
        label: "Chandigarh",
    },
    Choice {
        value: "05",
        label: "Uttarakhand",
    },
    Choice {
        value: "06",
        label: "Haryana",
    },
    Choice {
        value: "07",
        label: "Delhi",
    },
    Choice {
        value: "08",
        label: "Rajasthan",
    },
    Choice {
        value: "09",
        label: "Uttar Pradesh",
    },
    Choice {
        value: "10",
        label: "Bihar",
    },
    Choice {
        value: "11",
        label: "Sikkim",
    },
    Choice {
        value: "12",
        label: "Arunachal Pradesh",
    },
    Choice {
        value: "13",
        label: "Nagaland",
    },
    Choice {
        value: "14",
        label: "Manipur",
    },
    Choice {
        value: "15",
        label: "Mizoram",
    },
    Choice {
        value: "16",
        label: "Tripura",
    },
    Choice {
        value: "17",
        label: "Meghalaya",
    },
    Choice {
        value: "18",
        label: "Assam",
    },
    Choice {
        value: "19",
        label: "West Bengal",
    },
    Choice {
        value: "20",
        label: "Jharkhand",
    },
    Choice {
        value: "21",
        label: "Odisha",
    },
    Choice {
        value: "22",
        label: "Chhattisgarh",
    },
    Choice {
        value: "23",
        label: "Madhya Pradesh",
    },
    Choice {
        value: "24",
        label: "Gujarat",
    },
    Choice {
        value: "26",
        label: "Dadra and Nagar Haveli and Daman and Diu",
    },
    Choice {
        value: "27",
        label: "Maharashtra",
    },
    Choice {
        value: "29",
        label: "Karnataka",
    },
    Choice {
        value: "30",
        label: "Goa",
    },
    Choice {
        value: "31",
        label: "Lakshadweep",
    },
    Choice {
        value: "32",
        label: "Kerala",
    },
    Choice {
        value: "33",
        label: "Tamil Nadu",
    },
    Choice {
        value: "34",
        label: "Puducherry",
    },
    Choice {
        value: "35",
        label: "Andaman and Nicobar Islands",
    },
    Choice {
        value: "36",
        label: "Telangana",
    },
    Choice {
        value: "37",
        label: "Andhra Pradesh",
    },
    Choice {
        value: "38",
        label: "Ladakh",
    },
    Choice {
        value: "97",
        label: "Other territory",
    },
];

/// The state's name for its GST code — "29" is Karnataka.
#[must_use]
pub fn state_label(code: &str) -> Option<&'static str> {
    if code.is_empty() {
        return None;
    }
    STATES.iter().find(|s| s.value == code).map(|s| s.label)
}

// The macros. One line per setting, and the shape of that line is the same for every setting of
// the same kind — which is what makes ninety of them reviewable.

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

/// Per-section text size, as the height of a capital letter.
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
            read: |c| Value::Text(c.$($field).+.size.to_string()),
            write: |c, v| {
                let cap: u16 = match v.as_text()?.parse::<u16>() {
                    // Every vocabulary this row has held, told apart by `Style::from_stored` —
                    // the one rule, shared with the config file's own deserialiser so a stored
                    // row and an exported file can never be read differently.
                    Ok(n) if crate::settings::is_a_size(mb_print::Style::from_stored(n)) => {
                        mb_print::Style::from_stored(n)
                    }
                    _ => return Err(Invalid::new(
                        "Pick a size from the list — 1 to 10.",
                    )),
                };
                c.$($field).+.size = cap;
                Ok(())
            },
        }
    };
}

// The conversions the `pick!` entries use.

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
        LogoPosition::Left => "left",
        LogoPosition::Right => "right",
    }
}

fn logo_from(text: &str) -> Option<LogoPosition> {
    match text {
        "none" => Some(LogoPosition::None),
        "top" => Some(LogoPosition::Top),
        "left" => Some(LogoPosition::Left),
        "right" => Some(LogoPosition::Right),
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

/// A GST rate, stored as basis points in a choice so the five legal rates are the only five
/// that can be chosen.
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

// THE TABLE.

pub const CATALOG: &[Entry] = &[
    // Your shop.
    words!(
        "store.name",
        Store,
        Store,
        "Shop name",
        "Printed at the top of every bill.",
        ["hotel", "restaurant", "title"],
        60,
        Free,
        store.name
    ),
    words!(
        "store.address",
        Store,
        Store,
        "Address",
        "Printed under the name. Long addresses wrap onto a second line.",
        ["location", "street"],
        200,
        Free,
        store.address
    ),
    words!(
        "store.phone",
        Store,
        Store,
        "Phone number",
        "Ten digits. Printed on the bill so a customer can call about an order.",
        ["mobile", "contact", "telephone"],
        10,
        Phone,
        store.phone
    ),
    // Neither of these is checked any more.
    words!(
        "store.gstin",
        Store,
        Store,
        "GST number",
        "Printed on the bill if you turn that on. Type it exactly as it is on \
         your certificate — nothing here checks it or changes it.",
        ["gst", "gstin", "tax number"],
        32,
        Gstin,
        store.gstin
    ),
    words!(
        "store.fssai",
        Store,
        Store,
        "FSSAI licence number",
        "Printed on the bill if you turn that on. Type it exactly as it is on \
         your certificate.",
        ["food licence", "fssai"],
        32,
        Fssai,
        store.fssai
    ),
    pick_text!(
        "store.state_code",
        Store,
        Store,
        "State",
        "Which state you are in. This decides whether a bill charges CGST and \
         SGST or IGST.",
        ["state", "karnataka", "igst", "cgst"],
        STATES,
        store.state_code
    ),
    words!(
        "store.upi_id",
        Store,
        Store,
        "UPI id",
        "Where the QR code on the bill sends money. Looks like name@bank.",
        ["upi", "qr", "gpay", "phonepe", "payment"],
        60,
        UpiId,
        store.upi_id
    ),
    words!(
        "store.upi_merchant_name",
        Store,
        Store,
        "UPI name",
        "The name the customer sees in their payment app. Your shop name is used \
         if this is blank.",
        ["upi", "merchant", "payee"],
        40,
        Free,
        store.upi_merchant_name
    ),
    words!(
        "store.upi_reference",
        Store,
        Store,
        "UPI reference",
        "Carried inside the QR code so your bank statement says which counter a \
         payment came from.",
        ["upi", "reference", "note", "statement"],
        40,
        Free,
        store.upi_reference
    ),
    pick_text!(
        "store.registration",
        Tax,
        Store,
        "Your GST registration",
        "It decides whether a bill may show GST at all, and what the bill is called.",
        [
            "gst",
            "composition",
            "registration",
            "unregistered",
            "regular",
            "scheme",
            "small dealer"
        ],
        REGISTRATIONS,
        store.registration
    ),
    // The bill.
    pick!(
        "receipt.pattern",
        Receipt,
        Row,
        "Separator line",
        "What the dividing lines on the bill look like.",
        ["line", "divider", "dashes", "dotted"],
        PATTERNS,
        pattern_to,
        pattern_from,
        receipt.pattern
    ),
    pick!(
        "receipt.row_height",
        Receipt,
        Row,
        "Row height",
        "How much air there is between items and around the total.",
        ["spacing", "compact", "relaxed", "air", "paper"],
        ROW_HEIGHTS,
        row_height_to,
        row_height_from,
        receipt.row_height
    ),
    flag!(
        "receipt.show.token",
        Receipt,
        Row,
        "Print the token number",
        "The big number the customer waits for.",
        ["token", "number"],
        receipt.show.token
    ),
    flag!(
        "receipt.show.gstin",
        Receipt,
        Row,
        "Print your GST number",
        "In the header, under the address.",
        ["gst", "gstin"],
        receipt.show.gstin
    ),
    flag!(
        "receipt.show.fssai",
        Receipt,
        Row,
        "Print your FSSAI number",
        "In the header.",
        ["fssai", "food licence"],
        receipt.show.fssai
    ),
    flag!(
        "receipt.show.address",
        Receipt,
        Row,
        "Print your address",
        "In the header, under the shop name.",
        ["address"],
        receipt.show.address
    ),
    flag!(
        "receipt.show.phone",
        Receipt,
        Row,
        "Print your phone number",
        "In the header.",
        ["phone", "mobile"],
        receipt.show.phone
    ),
    flag!(
        "receipt.show.cashier",
        Receipt,
        Row,
        "Print who billed it",
        "The name of whoever was signed in.",
        ["cashier", "staff", "who"],
        receipt.show.cashier
    ),
    flag!(
        "receipt.show.hsn",
        Receipt,
        Row,
        "Print the HSN code",
        "Needed above the turnover threshold. It costs six characters of a \
         narrow bill.",
        ["hsn", "sac", "code"],
        receipt.show.hsn
    ),
    flag!(
        "receipt.show.tax_summary",
        Receipt,
        Row,
        "Print the rate-wise tax summary",
        "The block a chartered accountant looks at first. Turning it off makes \
         a bill harder to file from.",
        ["gst", "summary", "cgst", "sgst", "rate wise"],
        receipt.show.tax_summary
    ),
    flag!(
        "receipt.show.payment_lines",
        Receipt,
        Row,
        "Print how it was paid",
        "\"Cash 300 / UPI 200\" under the total.",
        ["payment", "split", "cash", "card"],
        receipt.show.payment_lines
    ),
    flag!(
        "receipt.show.title",
        Receipt,
        Row,
        "Print \"TAX INVOICE\"",
        "A GST document has to say what it is. A composition dealer gets \
         \"BILL OF SUPPLY\"; a shop with no GST number gets neither.",
        ["tax invoice", "title", "heading", "gst", "bill of supply"],
        receipt.show.title
    ),
    flag!(
        "receipt.show.time",
        Receipt,
        Row,
        "Print the time",
        "Beside the date. A restaurant bill without a time is not much of a \
         record.",
        ["time", "clock", "hour"],
        receipt.show.time
    ),
    flag!(
        "receipt.show.covers",
        Receipt,
        Row,
        "Print how many people",
        "Only when the order says. Most do not.",
        ["covers", "persons", "pax", "guests"],
        receipt.show.covers
    ),
    flag!(
        "receipt.show.waiter",
        Receipt,
        Row,
        "Print who took the order",
        "As well as who took the money.",
        ["waiter", "steward", "server", "staff"],
        receipt.show.waiter
    ),
    flag!(
        "receipt.show.place_of_supply",
        Receipt,
        Row,
        "Print the place of supply",
        "The state code. A B2B customer's accounts department asks for it.",
        ["place of supply", "state", "gst"],
        receipt.show.place_of_supply
    ),
    flag!(
        "receipt.show.amount_in_words",
        Receipt,
        Row,
        "Print the total in words",
        "\"Rupees One Thousand Two Hundred only\" under the total. Two more \
         lines of paper on every bill.",
        ["words", "rupees", "amount in words"],
        receipt.show.amount_in_words
    ),
    flag!(
        "receipt.separators.below_store_header",
        Receipt,
        Row,
        "Line under the shop header",
        "",
        ["separator", "line", "header"],
        receipt.separators.below_store_header
    ),
    flag!(
        "receipt.separators.below_meta",
        Receipt,
        Row,
        "Line under the bill details",
        "",
        ["separator", "line", "details"],
        receipt.separators.below_meta
    ),
    flag!(
        "receipt.separators.below_token",
        Receipt,
        Row,
        "Line under the token",
        "",
        ["separator", "line", "token"],
        receipt.separators.below_token
    ),
    flag!(
        "receipt.separators.below_column_names",
        Receipt,
        Row,
        "Line under the column names",
        "",
        ["separator", "line", "columns"],
        receipt.separators.below_column_names
    ),
    flag!(
        "receipt.separators.below_items",
        Receipt,
        Row,
        "Line under the items",
        "",
        ["separator", "line", "items"],
        receipt.separators.below_items
    ),
    flag!(
        "receipt.separators.below_subtotals",
        Receipt,
        Row,
        "Line above the total",
        "",
        ["separator", "line", "subtotal"],
        receipt.separators.below_subtotals
    ),
    flag!(
        "receipt.separators.below_grand_total",
        Receipt,
        Row,
        "Line under the total",
        "",
        ["separator", "line", "total"],
        receipt.separators.below_grand_total
    ),
    // Two rules the template drew with no setting in front of them.
    flag!(
        "receipt.separators.below_tax_summary",
        Receipt,
        Row,
        "Line under the tax summary",
        "",
        ["separator", "line", "tax", "summary"],
        receipt.separators.below_tax_summary
    ),
    flag!(
        "receipt.separators.below_payments",
        Receipt,
        Row,
        "Line under the payment lines",
        "",
        ["separator", "line", "payment"],
        receipt.separators.below_payments
    ),
    pick_text!(
        "receipt.font",
        Receipt,
        Row,
        "Bill typeface",
        "The face your bills print in. All of these come with Windows — if one \
         is missing from this computer, Magic Bill quietly uses its own.",
        ["font", "typeface", "face", "typography", "letters", "print"],
        FONTS,
        receipt.font
    ),
    size!(
        "receipt.sections.store_name.scale",
        Receipt,
        "Shop name size",
        "",
        ["size", "big", "header", "name"],
        receipt.sections.store_name
    ),
    flag!(
        "receipt.sections.store_name.bold",
        Receipt,
        Row,
        "Shop name in bold",
        "",
        ["bold", "header", "name"],
        receipt.sections.store_name.bold
    ),
    size!(
        "receipt.sections.meta.scale",
        Receipt,
        "Bill details size",
        "",
        ["size", "details", "address"],
        receipt.sections.meta
    ),
    flag!(
        "receipt.sections.meta.bold",
        Receipt,
        Row,
        "Bill details in bold",
        "",
        ["bold", "details"],
        receipt.sections.meta.bold
    ),
    size!(
        "receipt.sections.items.scale",
        Receipt,
        "Item list size",
        "",
        ["size", "items"],
        receipt.sections.items
    ),
    flag!(
        "receipt.sections.items.bold",
        Receipt,
        Row,
        "Item list in bold",
        "",
        ["bold", "items"],
        receipt.sections.items.bold
    ),
    size!(
        "receipt.sections.subtotals.scale",
        Receipt,
        "Subtotal and GST size",
        "",
        ["size", "subtotal", "gst"],
        receipt.sections.subtotals
    ),
    flag!(
        "receipt.sections.subtotals.bold",
        Receipt,
        Row,
        "Subtotal and GST in bold",
        "",
        ["bold", "subtotal"],
        receipt.sections.subtotals.bold
    ),
    size!(
        "receipt.sections.grand_total.scale",
        Receipt,
        "Total size",
        "",
        ["size", "total", "grand"],
        receipt.sections.grand_total
    ),
    flag!(
        "receipt.sections.grand_total.bold",
        Receipt,
        Row,
        "Total in bold",
        "",
        ["bold", "total"],
        receipt.sections.grand_total.bold
    ),
    size!(
        "receipt.sections.footer.scale",
        Receipt,
        "Footer size",
        "",
        ["size", "footer", "thank you"],
        receipt.sections.footer
    ),
    flag!(
        "receipt.sections.footer.bold",
        Receipt,
        Row,
        "Footer in bold",
        "",
        ["bold", "footer"],
        receipt.sections.footer.bold
    ),
    size!(
        "receipt.sections.token.scale",
        Receipt,
        "Token size",
        "The customer reads this one across a room.",
        ["size", "token", "big"],
        receipt.sections.token
    ),
    flag!(
        "receipt.sections.token.bold",
        Receipt,
        Row,
        "Token in bold",
        "",
        ["bold", "token"],
        receipt.sections.token.bold
    ),
    pick!(
        "receipt.logo",
        Receipt,
        Row,
        "Logo",
        "Where your logo prints, if you have uploaded one.",
        ["logo", "image", "picture", "brand"],
        LOGO_POSITIONS,
        logo_to,
        logo_from,
        receipt.logo
    ),
    number!("receipt.logo_width_pct", Receipt, Row, "Logo width",
        "As a percentage of the paper width.", ["logo", "size", "width"],
        10..=100 "%", u8, receipt.logo_width_pct),
    pick!(
        "receipt.qr",
        Receipt,
        Row,
        "UPI QR code",
        "Dynamic carries the amount, so the customer does not type it.",
        ["qr", "upi", "scan", "payment"],
        QR_MODES,
        qr_to,
        qr_from,
        receipt.qr
    ),
    number!("receipt.qr_width_pct", Receipt, Row, "QR code width",
        "As a percentage of the paper width. Too small and a phone cannot read it.",
        ["qr", "size", "width"], 10..=100 "%", u8, receipt.qr_width_pct),
    words!(
        "receipt.footer",
        Receipt,
        Row,
        "Footer message",
        "The last line of the bill.",
        ["thank you", "footer", "message", "bottom"],
        120,
        Free,
        receipt.footer
    ),
    words!(
        "receipt.composition_note",
        Receipt,
        Row,
        "Composition declaration",
        "Printed instead of a tax breakdown when you are under the composition \
         scheme.",
        ["composition", "declaration", "note"],
        160,
        Free,
        receipt.composition_note
    ),
    // The kitchen ticket.
    flag!(
        "kitchen.show_title",
        Kitchen,
        Row,
        "Print the word KITCHEN",
        "",
        ["kot", "title", "heading"],
        kitchen.show_title
    ),
    flag!(
        "kitchen.show_token",
        Kitchen,
        Row,
        "Print the token number",
        "",
        ["kot", "token"],
        kitchen.show_token
    ),
    flag!(
        "kitchen.show_bill_number",
        Kitchen,
        Row,
        "Print the bill number",
        "",
        ["kot", "bill number"],
        kitchen.show_bill_number
    ),
    flag!(
        "kitchen.show_order_type",
        Kitchen,
        Row,
        "Print the order type",
        "Dine in, parcel, delivery.",
        ["kot", "type", "parcel"],
        kitchen.show_order_type
    ),
    flag!(
        "kitchen.show_table",
        Kitchen,
        Row,
        "Print the table",
        "",
        ["kot", "table"],
        kitchen.show_table
    ),
    flag!(
        "kitchen.show_time",
        Kitchen,
        Row,
        "Print the time",
        "",
        ["kot", "time", "clock"],
        kitchen.show_time
    ),
    // A cook could not say "KOT 14", because there was no such number.
    flag!(
        "kitchen.show_kot_number",
        Kitchen,
        Row,
        "Print the ticket number",
        "Its own running number, so the kitchen and the counter can talk about \n         one ticket.",
        ["kot", "number", "ticket"],
        kitchen.show_kot_number
    ),
    flag!(
        "kitchen.show_column_names",
        Kitchen,
        Row,
        "Print column names",
        "A \"Qty  Item\" heading above the food.",
        ["kot", "columns", "heading"],
        kitchen.show_column_names
    ),
    flag!(
        "kitchen.two_column",
        Kitchen,
        Row,
        "Two dishes per line",
        "Half the paper, harder to read. Notes are packed onto the dish's own \
         line.",
        ["kot", "two column", "packing", "short", "paper"],
        kitchen.two_column
    ),
    pick!(
        "kitchen.pattern",
        Kitchen,
        Row,
        "Separator line",
        "",
        ["kot", "line", "divider"],
        PATTERNS,
        pattern_to,
        pattern_from,
        kitchen.pattern
    ),
    pick!(
        "kitchen.row_height",
        Kitchen,
        Row,
        "Row height",
        "How much air there is between dishes. The kitchen reads this at speed.",
        ["kot", "spacing", "compact", "relaxed"],
        ROW_HEIGHTS,
        row_height_to,
        row_height_from,
        kitchen.row_height
    ),
    flag!(
        "kitchen.separators.below_title",
        Kitchen,
        Row,
        "Line under the title",
        "",
        ["kot", "separator", "title"],
        kitchen.separators.below_title
    ),
    flag!(
        "kitchen.separators.below_token",
        Kitchen,
        Row,
        "Line under the token",
        "",
        ["kot", "separator", "token"],
        kitchen.separators.below_token
    ),
    flag!(
        "kitchen.separators.below_details",
        Kitchen,
        Row,
        "Line under the details",
        "",
        ["kot", "separator", "details"],
        kitchen.separators.below_details
    ),
    flag!(
        "kitchen.separators.below_column_names",
        Kitchen,
        Row,
        "Line under the column names",
        "",
        ["kot", "separator", "columns"],
        kitchen.separators.below_column_names
    ),
    flag!(
        "kitchen.separators.below_items",
        Kitchen,
        Row,
        "Line under the food",
        "",
        ["kot", "separator", "items"],
        kitchen.separators.below_items
    ),
    pick_text!(
        "kitchen.font",
        Kitchen,
        Row,
        "Kitchen ticket typeface",
        "The face your kitchen tickets print in. It does not have to be the \
         same as the bill's.",
        [
            "font", "typeface", "face", "kot", "kitchen", "letters", "print"
        ],
        FONTS,
        kitchen.font
    ),
    size!(
        "kitchen.title.scale",
        Kitchen,
        "Title size",
        "",
        ["kot", "size", "title"],
        kitchen.title
    ),
    flag!(
        "kitchen.title.bold",
        Kitchen,
        Row,
        "Title in bold",
        "",
        ["kot", "bold", "title"],
        kitchen.title.bold
    ),
    size!(
        "kitchen.details.scale",
        Kitchen,
        "Details size",
        "",
        ["kot", "size", "details"],
        kitchen.details
    ),
    flag!(
        "kitchen.details.bold",
        Kitchen,
        Row,
        "Details in bold",
        "",
        ["kot", "bold", "details"],
        kitchen.details.bold
    ),
    size!(
        "kitchen.items.scale",
        Kitchen,
        "Food size",
        "The kitchen reads this across a hot room.",
        ["kot", "size", "items"],
        kitchen.items
    ),
    flag!(
        "kitchen.items.bold",
        Kitchen,
        Row,
        "Food in bold",
        "",
        ["kot", "bold", "items"],
        kitchen.items.bold
    ),
    // Billing behaviour.
    pick!(
        "billing.search_mode",
        Billing,
        Row,
        "How the item search matches",
        "A short menu wants \"starts with\"; a long one wants \"contains\".",
        ["search", "find", "starts with", "contains"],
        MATCH_MODES,
        match_to,
        match_from,
        billing.search_mode
    ),
    pick!(
        "billing.rounding",
        Billing,
        Row,
        "Round off the total",
        "What happens to the paise on the grand total.",
        ["round", "roundoff", "round off", "paise"],
        ROUNDING,
        rounding_to,
        rounding_from,
        billing.rounding
    ),
    flag!(
        "billing.lock_order_type",
        Billing,
        Row,
        "Always start on one order type",
        "For a counter that only does parcels, or only dine-in.",
        ["order type", "lock", "parcel", "default"],
        billing.lock_order_type
    ),
    pick!(
        "billing.locked_order_type",
        Billing,
        Row,
        "Which order type",
        "Used when the order type is locked.",
        ["order type", "parcel", "dine in"],
        ORDER_TYPES,
        order_type_to,
        order_type_from,
        billing.locked_order_type
    ),
    number!("billing.idle_lock_minutes", Billing, Row, "Lock the counter after",
        "Minutes of nobody touching it. 0 never locks by itself.",
        ["lock", "idle", "timeout", "screen"], 0..=240 "minutes", u32,
        billing.idle_lock_minutes),
    flag!(
        "billing.kitchen_ticket_off",
        Billing,
        Row,
        "This shop has no kitchen ticket",
        "For a counter where the food is already made.",
        ["kot", "disable", "off", "no kitchen"],
        billing.kitchen_ticket_off
    ),
    flag!(
        "billing.kitchen_screen",
        Billing,
        Row,
        "The kitchen has a screen",
        "Orders go to the Kitchen screen. If nobody sees one there in 20 seconds, it \
         prints instead.",
        ["kds", "screen", "display", "kitchen"],
        billing.kitchen_screen
    ),
    number!("billing.service_charge_bp", Billing, Row, "Service charge",
        "In hundredths of a percent, so 500 is 5%. 0 means you do not charge it. \
         Added to dine-in bills only.",
        ["service", "charge", "percent", "tip"], 0..=2500 "hundredths of a percent",
        u32, billing.service_charge_bp),
    pick!(
        "billing.service_charge_tax_bp",
        Billing,
        Row,
        "GST on the service charge",
        "A service charge is usually taxed at 18% even on a bill of 5% food.",
        ["service", "gst", "tax"],
        GST_RATES,
        rate_to,
        rate_from,
        billing.service_charge_tax_bp
    ),
    cash!(
        "billing.packing_charge",
        Billing,
        Row,
        "Packing charge",
        "Added to parcel and self-service bills. Zero means you do not charge it.",
        ["packing", "parcel", "box", "charge"],
        0..=100_000,
        billing.packing_charge
    ),
    pick!(
        "billing.packing_charge_tax_bp",
        Billing,
        Row,
        "GST on the packing charge",
        "",
        ["packing", "gst", "tax"],
        GST_RATES,
        rate_to,
        rate_from,
        billing.packing_charge_tax_bp
    ),
    cash!(
        "billing.delivery_charge",
        Billing,
        Row,
        "Delivery charge",
        "Added to delivery bills. Zero means you do not charge it.",
        ["delivery", "charge", "rider"],
        0..=100_000,
        billing.delivery_charge
    ),
    pick!(
        "billing.delivery_charge_tax_bp",
        Billing,
        Row,
        "GST on the delivery charge",
        "",
        ["delivery", "gst", "tax"],
        GST_RATES,
        rate_to,
        rate_from,
        billing.delivery_charge_tax_bp
    ),
    // The day.
    number!("day.starts_at_minutes", Day, Row, "Your day starts at",
        "Minutes past midnight. 300 is 5 in the morning, which means a bill \
         printed at 1 a.m. counts as yesterday's. This changes every report you \
         will ever run.",
        ["day", "business day", "5 am", "close", "midnight", "cutoff"],
        0..=1439 "minutes past midnight", u32, day.starts_at_minutes),
    cash!(
        "stock.count_reason_above",
        Stock,
        Row,
        "Ask why if a counted item is out by more than",
        "When a stock count finds a material short or over by more than this in \
         value, it asks for a reason before it can be approved. Zero asks every \
         time.",
        ["count", "variance", "short", "reason", "stock"],
        0..=1_000_000,
        stock.count_reason_above
    ),
    cash!(
        "day.variance_reason_above",
        Day,
        Row,
        "Ask for a reason if the drawer is out by more than",
        "When the counted cash differs from the expected cash by more than \
         this, closing the day asks why. Zero asks every time.",
        [
            "variance",
            "short",
            "over",
            "reason",
            "difference",
            "drawer"
        ],
        0..=1_000_000,
        day.variance_reason_above
    ),
    flag!(
        "day.carry_float",
        Day,
        Row,
        "Leave a float in the drawer overnight",
        "Tomorrow starts with the amount below already counted, instead of an \
         empty drawer.",
        ["float", "opening", "carry", "tomorrow", "change"],
        day.carry_float
    ),
    cash!(
        "day.float_amount",
        Day,
        Row,
        "How much to leave",
        "Only used when the float is carried forward.",
        ["float", "opening", "how much", "change", "tomorrow"],
        0..=10_000_000,
        day.float_amount
    ),
    words!(
        "backup.folder",
        Backup,
        Row,
        "Backup folder",
        "Where backups are written. Blank uses the folder beside your database.",
        ["backup", "folder", "where", "copy"],
        260,
        Folder,
        backup.folder
    ),
    words!(
        "backup.second_folder",
        Backup,
        Row,
        "Second backup folder",
        "A pen drive or a network share. One copy on one disk is not a backup.",
        ["backup", "second", "pen drive", "usb", "network"],
        260,
        Folder,
        backup.second_folder
    ),
    number!("backup.every_hours", Backup, Row, "Back up every",
        "Hours. 0 means only when you press the button.",
        ["backup", "schedule", "automatic", "how often"], 0..=168 "hours", u32,
        backup.every_hours),
    number!("backup.keep_count", Backup, Row, "Keep this many backups",
        "Older ones are deleted to save disk space.",
        ["backup", "keep", "retention", "delete", "old"], 1..=365 "backups", u32,
        backup.keep_count),
    flag!(
        "receipt.bill_barcode",
        Receipt,
        Row,
        "Print the bill number as a barcode",
        "Adds a barcode to the foot of every bill, so a scanner can bring that          bill back onto the screen. Leave it off if you have no scanner — it          is two more lines of paper on every bill.",
        ["barcode", "scan", "scanner", "recall", "bill"],
        receipt.bill_barcode
    ),
    // The things a counter is plugged into.
    number!("devices.scan_average_gap_ms", Devices, Row, "A scan types faster than (average)",
        "How quickly characters have to arrive, on average, to be a barcode \
         scanner rather than a person. Lower is safer: a fast typist read as a \
         scan loses what they typed.",
        ["scanner", "barcode", "scan", "speed", "gap"],
        1..=200 "ms between keys", u32, devices.scan_average_gap_ms),
    number!("devices.scan_single_gap_ms", Devices, Row, "A scan never pauses longer than",
        "One pause longer than this ends the burst, whatever the average — so \
         somebody typing, thinking, and typing again is never read as one scan.",
        ["scanner", "barcode", "pause", "gap"],
        1..=1000 "ms", u32, devices.scan_single_gap_ms),
    number!("devices.scan_min_length", Devices, Row, "A barcode is at least this long",
        "Anything shorter is somebody searching the menu. The shortest real \
         barcode in a shop is 8 characters.",
        ["scanner", "barcode", "length", "short"],
        1..=40 "characters", u32, devices.scan_min_length),
    words!(
        "devices.label_prefix",
        Devices,
        Row,
        "Scale labels start with",
        "The leading digits on the labels your weighing scale prints — often \
         21 or 22. Leave it empty unless your scale prints labels with the \
         weight inside the barcode.",
        ["scale", "label", "weight", "barcode", "prefix", "embedded"],
        4,
        Free,
        devices.label_prefix
    ),
    number!("devices.label_item_from", Devices, Row, "The item code starts at digit",
        "Counting from zero. Your scale's manual, or the dealer, will say.",
        ["scale", "label", "item", "position"],
        0..=12 "", u32, devices.label_item_from),
    number!("devices.label_item_len", Devices, Row, "The item code is this many digits",
        "How long the item code inside the label is.",
        ["scale", "label", "item", "length"],
        1..=12 "digits", u32, devices.label_item_len),
    number!("devices.label_value_from", Devices, Row, "The weight or price starts at digit",
        "Counting from zero.",
        ["scale", "label", "weight", "price", "position"],
        0..=12 "", u32, devices.label_value_from),
    number!("devices.label_value_len", Devices, Row, "The weight or price is this many digits",
        "How long the number inside the label is.",
        ["scale", "label", "weight", "price", "length"],
        1..=12 "digits", u32, devices.label_value_len),
    pick_text!(
        "devices.label_value_is",
        Devices,
        Row,
        "That number is a",
        "Some scales print the weight inside the barcode and some print the \
         price. **The shop's own price still bills**: a printed price is \
         compared against it, not trusted over it.",
        ["scale", "label", "weight", "price"],
        LABEL_VALUES,
        devices.label_value_is
    ),
    number!("devices.label_value_decimals", Devices, Row, "With this many decimal places",
        "450 grams printed as 00450 with 3 decimal places is 0.450 kg. ₹12.50 \
         printed as 01250 with 2 is twelve rupees fifty.",
        ["scale", "label", "decimal", "weight", "price"],
        0..=4 "places", u32, devices.label_value_decimals),
    words!(
        "devices.scale_port",
        Devices,
        Row,
        "The scale is on",
        "The COM port your weighing scale is plugged into — COM3, COM4. Leave \
         it empty if you have no scale, which is most shops.",
        ["scale", "weight", "com", "serial", "port"],
        16,
        Free,
        devices.scale_port
    ),
    number!("devices.scale_baud", Devices, Row, "The scale runs at this speed",
        "The scale's baud rate. 9600 is right for nearly every counter scale \
         sold in India.",
        ["scale", "baud", "speed", "serial"],
        300..=115200 "baud", u32, devices.scale_baud),
    pick_text!(
        "devices.scale_protocol",
        Devices,
        Row,
        "The scale talks like this",
        "Every brand differs. If neither shape works, choose \"Show me what it \
         is sending\" — the device screen then prints the raw bytes, which is \
         how an unknown scale gets set up without waiting for us.",
        ["scale", "protocol", "raw", "format"],
        SCALE_PROTOCOLS,
        devices.scale_protocol
    ),
    flag!(
        "devices.display_on",
        Devices,
        Row,
        "Show the customer their bill",
        "A second screen the customer can see, with the bill as it is typed \
         and the total at the end. It never takes the keyboard away from the \
         billing screen.",
        ["customer", "display", "second screen", "pole", "monitor"],
        devices.display_on
    ),
    words!(
        "devices.display_port",
        Devices,
        Row,
        "The pole display is on",
        "A serial pole display's COM port. Leave it empty to use a second \
         monitor instead, which is what most shops have.",
        ["customer", "display", "pole", "com", "serial", "port"],
        16,
        Free,
        devices.display_port
    ),
    words!(
        "devices.display_idle",
        Devices,
        Row,
        "The display says this when idle",
        "What the customer sees between bills. Empty shows the shop's name.",
        ["customer", "display", "idle", "welcome"],
        40,
        Free,
        devices.display_idle
    ),
    words!(
        "devices.label_printer",
        Devices,
        Row,
        "Parcel labels print on",
        "The printer id parcel labels go to. Leave it empty if you do not \
         print labels — the button is then not offered at all.",
        ["label", "parcel", "sticker", "printer"],
        40,
        Free,
        devices.label_printer
    ),
];

/// The entry for a key, or `None` if this build has never heard of it.
#[must_use]
pub fn find(key: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.key == key)
}

/// Sub-headings, by key prefix — and this is a bug found by looking at it.
const TOPICS: &[(&str, &str)] = &[
    ("store.upi_", "Taking money by UPI"),
    ("receipt.pattern", "Paper and spacing"),
    ("receipt.row_height", "Paper and spacing"),
    ("receipt.show.", "What goes on the bill"),
    ("receipt.separators.", "Dividing lines"),
    ("receipt.font", "Typeface and sizes"),
    ("receipt.sections.", "Typeface and sizes"),
    ("receipt.logo", "Your logo"),
    ("receipt.qr", "The UPI QR code"),
    ("receipt.footer", "The last words"),
    ("receipt.composition_note", "The last words"),
    ("kitchen.show_", "What goes on the ticket"),
    ("kitchen.two_column", "What goes on the ticket"),
    ("kitchen.pattern", "Paper and spacing"),
    ("kitchen.row_height", "Paper and spacing"),
    ("kitchen.separators.", "Dividing lines"),
    ("kitchen.font", "Typeface and sizes"),
    ("kitchen.title", "Typeface and sizes"),
    ("kitchen.details", "Typeface and sizes"),
    ("kitchen.items", "Typeface and sizes"),
    ("billing.search_mode", "At the counter"),
    ("billing.rounding", "At the counter"),
    ("billing.lock_order_type", "At the counter"),
    ("billing.locked_order_type", "At the counter"),
    ("billing.kitchen_ticket_off", "Before it prints"),
    ("billing.kitchen_screen", "Before it prints"),
    ("billing.idle_lock_minutes", "At the counter"),
    ("billing.service_charge", "Charges you add"),
    ("billing.packing_charge", "Charges you add"),
    ("billing.delivery_charge", "Charges you add"),
];

/// The heading a setting sits under.
#[must_use]
pub fn topic_for(entry: &Entry) -> &'static str {
    TOPICS
        .iter()
        .filter(|(prefix, _)| entry.key.starts_with(prefix))
        // Longest wins: `receipt.qr` and `receipt.qr_width_pct` are both the QR code, and
        // `kitchen.show_` must not swallow `kitchen.show_column_names` into something more
        // general.
        .max_by_key(|(prefix, _)| prefix.len())
        .map_or_else(|| entry.group.label(), |(_, heading)| *heading)
}

/// Two settings that belong on one line, by key prefix.
const ROWS: &[(&str, &str)] = &[
    ("receipt.font", "Bill typeface"),
    ("kitchen.font", "Ticket typeface"),
    ("receipt.sections.store_name.", "Shop name"),
    ("receipt.sections.meta.", "Bill details"),
    ("receipt.sections.items.", "Item list"),
    ("receipt.sections.subtotals.", "Subtotal and GST"),
    ("receipt.sections.grand_total.", "Total"),
    ("receipt.sections.footer.", "Footer"),
    ("receipt.sections.token.", "Token"),
    ("kitchen.title.", "Title"),
    ("kitchen.details.", "Ticket details"),
    ("kitchen.items.", "Item list"),
];

/// The line a setting shares, or `""` when it stands on its own.
#[must_use]
pub fn row_for(entry: &Entry) -> &'static str {
    ROWS.iter()
        .filter(|(prefix, _)| entry.key.starts_with(prefix))
        // Longest wins, the same rule `topic_for` uses — so a short prefix added later cannot
        // quietly swallow a longer one already here.
        .max_by_key(|(prefix, _)| prefix.len())
        .map_or("", |(_, line)| *line)
}

/// The word this control wears inside a shared line.
#[must_use]
pub fn short_for(entry: &Entry) -> &'static str {
    match entry.key.rsplit('.').next() {
        Some("scale") => "Size",
        Some("bold") => "Bold",
        _ => "",
    }
}

/// The GSTIN's state code must be the shop's own state.
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
