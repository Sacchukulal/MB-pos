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
    /// **No scalar settings at all**, and that is not an oversight: paper,
    /// connection and role belong to a PRINTER, and a shop has none, one or
    /// six of them. The section exists so the screen has somewhere to put
    /// `settings::printers`, which owns the records.
    Printers,
    /// Also no scalar settings of its own: the counters are records, one per
    /// terminal per series, and P27 adds terminals.
    Numbering,
    Billing,
    Day,
    /// P26. The stock book has exactly one scalar setting: the threshold a
    /// count variance has to pass before the screen asks why. Everything else
    /// about stock is a record, not a preference.
    Stock,
    Backup,
    Appearance,
    /// P29. The scanner, the scale, the customer display and the label
    /// printer. **Every one of them optional**, and an empty port here is a
    /// finished configuration rather than a half-done one.
    Devices,
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

/// P29. What the number inside a scale's label means.
const LABEL_VALUES: &[Choice] = &[
    Choice { value: "quantity", label: "Weight or quantity" },
    Choice { value: "price", label: "Price" },
];

/// P29. **"Show me what it is sending" is a tool, not a fallback** — it is how
/// a dealer sets up a scale nobody here has ever seen.
const SCALE_PROTOCOLS: &[Choice] = &[
    Choice { value: "status_then_weight", label: "Status, then the weight (commonest)" },
    Choice { value: "weight_only", label: "Just the weight" },
    Choice { value: "raw", label: "Show me what it is sending" },
];

const PATTERNS: &[Choice] = &[
    Choice { value: "dashed", label: "Dashed  - - -" },
    Choice { value: "dotted", label: "Dotted  . . ." },
    Choice { value: "solid", label: "Solid  ___" },
    Choice { value: "bold", label: "Bold  ===" },
    Choice { value: "double", label: "Double line" },
];

// **The FONTS list came back at P31, and D71 is why it took this long.**
//
// P17 added a font list, T1 rendered the bill with each of its three choices,
// got three identical documents, and deleted it — correctly. The product
// embedded one face (D33) and the raster sink drew with whatever the queue
// loaded at start-up, so the setting was a control wired to nothing.
//
// What changed is not the list; it is the layer underneath it. `mb_print::queue`
// now asks a `Typefaces` for the face on each JOB, the job carries the key, and
// `typefaces.rs` loads the file. So the choice reaches the dots.
//
// It still does not reach `Laid` — a receipt is a character grid and every face
// produces the same 48 columns — which is why T1 exempts these two keys BY NAME
// and points at the three tests that cover them instead. If those go, D71
// stands again and so should the deletion. See FONTS below.

const ROW_HEIGHTS: &[Choice] = &[
    Choice { value: "compact", label: "Compact — least paper" },
    Choice { value: "standard", label: "Standard" },
    Choice { value: "relaxed", label: "Relaxed — easiest to read" },
];

/// **The text sizes, in px** — P31, and the owner asked for it by name:
/// *"sizes in px not Large/Normal/Small."*
///
/// # These are the only three, and that is the printer's doing rather than ours
///
/// A thermal printer's character cell comes from the paper: `paper.dots()`
/// divided by `paper.columns()`, which is **12 × 24 dots on both 58 mm and
/// 80 mm rolls**. Everything the hardware can draw is that cell times one, two
/// or three — the ESC/POS multiplier — so 24, 48 and 72 px are not three
/// options out of many, they are the complete list.
///
/// A free-text px box would be a box that lies. The character WIDTH has to stay
/// on the column grid or the text sink, the PDF sink, the raster sink and the
/// on-screen preview stop agreeing about where a line breaks, which is the
/// drift `mb-print` exists to prevent (D1, D29).
///
/// The old word stays in brackets, because a shop that learned "Large" on v1
/// should still be able to find it.
pub(super) const SIZES: &[Choice] = &[
    Choice { value: "1", label: "24 px (normal)" },
    Choice { value: "2", label: "48 px (large)" },
    Choice { value: "3", label: "72 px (extra large)" },
];

/// **The typefaces** — the owner's *"5-6 choices"*.
///
/// Mirrors [`mb_print::font::FAMILIES`], which is the list the printer actually
/// resolves against. Written out rather than generated because `Choice` holds
/// `&'static str` and this is a `const` table read at start-up — and there is a
/// test below that fails the build if the two ever disagree, which is the part
/// that matters.
pub(super) const FONTS: &[Choice] = &[
    Choice { value: "builtin", label: "Magic Bill's own (IBM Plex Mono)" },
    Choice { value: "consolas", label: "Consolas" },
    Choice { value: "consolas_bold", label: "Consolas Bold — darker on faint paper" },
    Choice { value: "courier", label: "Courier New" },
    Choice { value: "lucida", label: "Lucida Console" },
    Choice { value: "cascadia", label: "Cascadia Mono" },
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

const LANGUAGES: &[Choice] = &[
    Choice { value: "en", label: "English" },
    Choice { value: "hi", label: "Hindi — not installed yet (P23)" },
    Choice { value: "kn", label: "Kannada — not installed yet (P23)" },
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
    // **Neither of these is checked any more** — the owner, 2026-08-16. The
    // length is generous rather than exact for the same reason: it is a string
    // off a certificate, and the certificate is right.
    words!("store.gstin", Store, Store, "GST number",
        "Printed on the bill if you turn that on. Type it exactly as it is on \
         your certificate — nothing here checks it or changes it.",
        ["gst", "gstin", "tax number"], 32, Gstin, store.gstin),
    words!("store.fssai", Store, Store, "FSSAI licence number",
        "Printed on the bill if you turn that on. Type it exactly as it is on \
         your certificate.",
        ["food licence", "fssai"], 32, Fssai, store.fssai),
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
    // Part 3's "Font" row IS here now — `receipt.font`, a few lines below.
    // It was deleted at P17 (D71) because there was one embedded face and the
    // choice changed nothing; P31 gave the queue a face per job and the
    // choice something to change. See the note above FONTS.
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

    // P31, the owner's fourth item. First among the look settings, because it
    // is a decision about the whole piece of paper rather than about one line.
    pick_text!("receipt.font", Receipt, Row, "Bill typeface",
        "The face your bills print in. All of these come with Windows — if one \
         is missing from this computer, Magic Bill quietly uses its own.",
        ["font", "typeface", "face", "typography", "letters", "print"],
        FONTS, receipt.font),

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
    pick!("kitchen.row_height", Kitchen, Row, "Row height",
        "How much air there is between dishes. The kitchen reads this at speed.",
        ["kot", "spacing", "compact", "relaxed"], ROW_HEIGHTS, row_height_to,
        row_height_from, kitchen.row_height),
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
    // Its own face, separate from the bill's, because the owner asked for both
    // — and because they are read differently: a ticket across a hot room at
    // speed, a bill at arm's length.
    pick_text!("kitchen.font", Kitchen, Row, "Kitchen ticket typeface",
        "The face your kitchen tickets print in. It does not have to be the \
         same as the bill's.",
        ["font", "typeface", "face", "kot", "kitchen", "letters", "print"],
        FONTS, kitchen.font),

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
    number!("billing.idle_lock_minutes", Billing, Row, "Lock the counter after",
        "Minutes of nobody touching it. 0 never locks by itself.",
        ["lock", "idle", "timeout", "screen"], 0..=240 "minutes", u32,
        billing.idle_lock_minutes),
    flag!("billing.confirm_before_kitchen", Billing, Row, "Ask before printing a kitchen ticket",
        "", ["confirm", "kot", "ask"], billing.confirm_before_kitchen),
    flag!("billing.confirm_before_bill", Billing, Row, "Ask before printing a bill",
        "", ["confirm", "bill", "ask"], billing.confirm_before_bill),
    flag!("billing.kitchen_ticket_off", Billing, Row, "This shop has no kitchen ticket",
        "For a counter where the food is already made.",
        ["kot", "disable", "off", "no kitchen"], billing.kitchen_ticket_off),
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
    // P18's day close. The threshold is the shop's own tolerance: a stall that
    // deals in tens does not want a form every time it is ₹2 out, and a
    // restaurant taking ₹80,000 a day wants one at ₹100.
    // P26, scope 4.8. The sibling of the drawer's threshold below, and worded
    // the same way on purpose: a shop that has learnt what one means has
    // learnt what the other means.
    cash!("stock.count_reason_above", Stock, Row, "Ask why if a counted item is out by more than",
        "When a stock count finds a material short or over by more than this in \
         value, it asks for a reason before it can be approved. Zero asks every \
         time.",
        ["count", "variance", "short", "reason", "stock"],
        0..=1_000_000, stock.count_reason_above),

    cash!("day.variance_reason_above", Day, Row, "Ask for a reason if the drawer is out by more than",
        "When the counted cash differs from the expected cash by more than \
         this, closing the day asks why. Zero asks every time.",
        ["variance", "short", "over", "reason", "difference", "drawer"],
        0..=1_000_000, day.variance_reason_above),
    flag!("day.carry_float", Day, Row, "Leave a float in the drawer overnight",
        "Tomorrow starts with the amount below already counted, instead of an \
         empty drawer.",
        ["float", "opening", "carry", "tomorrow", "change"], day.carry_float),
    cash!("day.float_amount", Day, Row, "How much to leave",
        "Only used when the float is carried forward.",
        ["float", "opening", "how much", "change", "tomorrow"],
        0..=10_000_000, day.float_amount),

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
    // **P22, audit E8 — and it is off until the shop says otherwise.**
    //
    // The help text is the consent: it says what is sent and what is not, in
    // the words a shopkeeper would use, because a switch whose meaning has to
    // be guessed is not consent. **The report is written on this computer
    // either way** (D95) — this is only about sending it.
    flag!("backup.send_crash_reports", Backup, Row, "Tell us if the counter stops unexpectedly",
        "Sends us what went wrong — the version, what the counter was doing, and \
         nothing else. Never your bills, your customers or your licence key. \
         A record is kept on this computer whichever way you set this, so you \
         can always send it yourself with Copy diagnostics.",
        ["crash", "report", "telemetry", "send", "error", "diagnostics", "privacy"],
        backup.send_crash_reports),

    // --- how it looks (scope 13.11, 13.12) ----------------------------------
    // The THEME and the TEXT SIZE are deliberately NOT here. They live in
    // `AppConfig`, on the machine: they are applied before the first paint and
    // they have to work when the database will not open. A shop's LANGUAGE is
    // the shop's — it is on the receipt — so it is a setting like any other.
    pick_text!("appearance.language", Appearance, Row, "Language",
        "English is the only one installed today. Hindi and Kannada need a \
         second typeface and a text shaper, which is P23.",
        ["language", "hindi", "kannada", "english", "bhasha"], LANGUAGES,
        appearance.language),

    flag!("receipt.bill_barcode", Receipt, Row, "Print the bill number as a barcode",
        "Adds a barcode to the foot of every bill, so a scanner can bring that          bill back onto the screen. Leave it off if you have no scanner — it          is two more lines of paper on every bill.",
        ["barcode", "scan", "scanner", "recall", "bill"],
        receipt.bill_barcode),

    // -----------------------------------------------------------------------
    // P29 — the things a counter is plugged into (scope 7.6–7.9).
    //
    // **Every one of them is optional and every one of them can be absent.**
    // An empty port here is a finished configuration, not a half-done one, and
    // nothing in this section may ever be able to stop a bill.
    //
    // The scanner has no port at all: it IS a keyboard. Its only settings are
    // how to tell it apart from a person typing, and getting that wrong in the
    // wrong direction throws away what a cashier typed — so the defaults are
    // deliberately cautious.
    // -----------------------------------------------------------------------
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

    words!("devices.label_prefix", Devices, Row, "Scale labels start with",
        "The leading digits on the labels your weighing scale prints — often \
         21 or 22. Leave it empty unless your scale prints labels with the \
         weight inside the barcode.",
        ["scale", "label", "weight", "barcode", "prefix", "embedded"],
        4, Free, devices.label_prefix),
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
    pick_text!("devices.label_value_is", Devices, Row, "That number is a",
        "Some scales print the weight inside the barcode and some print the \
         price. **The shop's own price still bills**: a printed price is \
         compared against it, not trusted over it.",
        ["scale", "label", "weight", "price"],
        LABEL_VALUES, devices.label_value_is),
    number!("devices.label_value_decimals", Devices, Row, "With this many decimal places",
        "450 grams printed as 00450 with 3 decimal places is 0.450 kg. ₹12.50 \
         printed as 01250 with 2 is twelve rupees fifty.",
        ["scale", "label", "decimal", "weight", "price"],
        0..=4 "places", u32, devices.label_value_decimals),

    words!("devices.scale_port", Devices, Row, "The scale is on",
        "The COM port your weighing scale is plugged into — COM3, COM4. Leave \
         it empty if you have no scale, which is most shops.",
        ["scale", "weight", "com", "serial", "port"],
        16, Free, devices.scale_port),
    number!("devices.scale_baud", Devices, Row, "The scale runs at this speed",
        "The scale's baud rate. 9600 is right for nearly every counter scale \
         sold in India.",
        ["scale", "baud", "speed", "serial"],
        300..=115200 "baud", u32, devices.scale_baud),
    pick_text!("devices.scale_protocol", Devices, Row, "The scale talks like this",
        "Every brand differs. If neither shape works, choose \"Show me what it \
         is sending\" — the device screen then prints the raw bytes, which is \
         how an unknown scale gets set up without waiting for us.",
        ["scale", "protocol", "raw", "format"],
        SCALE_PROTOCOLS, devices.scale_protocol),

    flag!("devices.display_on", Devices, Row, "Show the customer their bill",
        "A second screen the customer can see, with the bill as it is typed \
         and the total at the end. It never takes the keyboard away from the \
         billing screen.",
        ["customer", "display", "second screen", "pole", "monitor"],
        devices.display_on),
    words!("devices.display_port", Devices, Row, "The pole display is on",
        "A serial pole display's COM port. Leave it empty to use a second \
         monitor instead, which is what most shops have.",
        ["customer", "display", "pole", "com", "serial", "port"],
        16, Free, devices.display_port),
    words!("devices.display_idle", Devices, Row, "The display says this when idle",
        "What the customer sees between bills. Empty shows the shop's name.",
        ["customer", "display", "idle", "welcome"],
        40, Free, devices.display_idle),

    words!("devices.label_printer", Devices, Row, "Parcel labels print on",
        "The printer id parcel labels go to. Leave it empty if you do not \
         print labels — the button is then not offered at all.",
        ["label", "parcel", "sticker", "printer"],
        40, Free, devices.label_printer),
];

/// The entry for a key, or `None` if this build has never heard of it.
#[must_use]
pub fn find(key: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.key == key)
}

/// **Sub-headings, by key prefix** — and this is a bug found by looking at it.
///
/// The bill section is thirty-nine settings, and the first version drew them as
/// one undifferentiated grid: a shopkeeper wanting "Total size" scrolled past
/// twenty checkboxes with no landmark to steer by. Audit Part 3 groups them —
/// visibility, separators, sizes — and the screen did not.
///
/// It is a prefix table rather than a field on all ninety lines because **the
/// keys already say what they are**: everything under `receipt.separators.` is
/// a separator. The longest matching prefix wins, so `receipt.qr_width_pct`
/// lands under the QR code rather than under the bill in general.
///
/// A key with no prefix here falls back to its group's own label, and there is
/// a test that every entry gets a heading.
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
    ("billing.confirm_", "Before it prints"),
    ("billing.kitchen_ticket_off", "Before it prints"),
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
        // Longest wins: `receipt.qr` and `receipt.qr_width_pct` are both the QR
        // code, and `kitchen.show_` must not swallow `kitchen.show_column_names`
        // into something more general.
        .max_by_key(|(prefix, _)| prefix.len())
        .map_or_else(|| entry.group.label(), |(_, heading)| *heading)
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
