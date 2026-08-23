//! Every receipt toggle v1 had, and the ones it could not.
//!
//! Audit Part 3 is the complete list of what a shop can already change, and the
//! rule for this rebuild is that **none of it is lost** — a shop that spent
//! weeks tuning its receipt must not have to tune it again.
//!
//! This crate does not touch the database. P17 writes these through mb-db's
//! typed settings, P08 passes the struct in.
//!
//! Every default produces a sane bill on 80 mm paper with no configuration at
//! all, because that is what a shop sees on its first day, and a first bill
//! that looks broken is the worst first impression this product can make.

use serde::{Deserialize, Serialize};

use crate::doc::{Pattern, Style};

/// Where the logo goes.
///
/// v1 had two — nothing, or centred above the shop's name. The owner asked for
/// the other two on 2026-08-23:
///
/// > *"logo placement (left right, top, if left right means the hotel name and
/// > address will cover 70% of 3 inch or 4 inch paper, remaining 30% width for
/// > logo, also logo correctly fit with that on ful size)"*
///
/// `Left` and `Right` put the picture and the letterhead in one band of rows —
/// see [`crate::doc::Block::Band`], which had to be invented for them, because
/// a `Document` was a flat top-to-bottom list and nothing could sit beside
/// anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogoPosition {
    #[default]
    None,
    Top,
    /// The picture on the left, the shop's name and address centred in the rest.
    Left,
    /// The picture on the right, the same the other way round.
    Right,
}

impl LogoPosition {
    /// Is this one of the side-by-side placements?
    #[must_use]
    pub const fn is_beside(self) -> bool {
        matches!(self, LogoPosition::Left | LogoPosition::Right)
    }
}

/// Scope 8.2. Dynamic carries the amount, so the customer does not type it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrMode {
    #[default]
    None,
    Static,
    Dynamic,
}

/// v1's "Compact / Standard / Relaxed".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowHeight {
    Compact,
    #[default]
    Standard,
    Relaxed,
}

impl RowHeight {
    /// Blank lines after each ITEM. Cheap, and it is what "row height" means on
    /// a device that cannot vary leading.
    #[must_use]
    pub const fn gap(self) -> u8 {
        match self {
            RowHeight::Compact | RowHeight::Standard => 0,
            RowHeight::Relaxed => 1,
        }
    }

    /// Blank lines between one block of the receipt and the next.
    ///
    /// **Compact and Standard used to be the same value**, which made the
    /// setting a lie: a shop could choose Compact, watch nothing change, and
    /// reasonably conclude the product was broken. P17's T1 is the rule — *a
    /// toggle that changes nothing is a lie on a screen* — and this is the
    /// second number the three settings needed in order to be three settings.
    #[must_use]
    pub const fn section_gap(self) -> u8 {
        match self {
            RowHeight::Compact => 0,
            RowHeight::Standard | RowHeight::Relaxed => 1,
        }
    }
}

/// The six on/off content toggles from audit Part 3, plus the ones the new tax
/// engine needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Show {
    pub token: bool,
    pub gstin: bool,
    pub fssai: bool,
    pub address: bool,
    pub phone: bool,
    pub cashier: bool,
    /// Scope 2.5. Off by default: a shop below the turnover threshold does not
    /// need it and the column costs eight characters of a 32-column receipt.
    pub hsn: bool,
    /// Scope 2.7 — the rate-wise summary. On by default, because audit B11 is
    /// the reason this product can be filed from at all.
    pub tax_summary: bool,
    /// Scope 1.15 — "Cash 300 / UPI 200".
    pub payment_lines: bool,
    /// **P32 — "TAX INVOICE" or "BILL OF SUPPLY" at the head of the bill.**
    ///
    /// A GST document has to say what it is and none of them did. Printed only
    /// for a shop that has a GSTIN: putting "TAX INVOICE" on an unregistered
    /// shop's bill would be a worse fault than printing nothing.
    #[serde(default = "yes")]
    pub title: bool,
    /// **P32 — the time of day beside the date.** A restaurant bill without one
    /// is not much of a record, and no bill this product printed had one.
    #[serde(default = "yes")]
    pub time: bool,
    /// Scope 1.24 — how many people ate. Printed only when the order carries a
    /// cover count, which is honestly unknown far more often than not.
    #[serde(default = "yes")]
    pub covers: bool,
    /// Who took the order, as against who took the money.
    #[serde(default = "yes")]
    pub waiter: bool,
    /// Scope 2.4 — the state code the supply happened in. Off by default: it
    /// matters to a B2B customer and to nobody buying a dosa.
    #[serde(default)]
    pub place_of_supply: bool,
    /// Scope 2.9 — the grand total spelled out. Off by default: two lines of
    /// paper on every bill, wanted by an accounts department and by no walk-in.
    #[serde(default)]
    pub amount_in_words: bool,
}

/// Serde default for a toggle added after shops had saved settings.
const fn yes() -> bool {
    true
}

impl Default for Show {
    fn default() -> Self {
        Show {
            token: true,
            gstin: true,
            fssai: true,
            address: true,
            phone: true,
            cashier: true,
            hsn: false,
            tax_summary: true,
            payment_lines: true,
            title: true,
            time: true,
            covers: true,
            waiter: true,
            place_of_supply: false,
            amount_in_words: false,
        }
    }
}

/// The seven separator toggles, by name rather than by position.
///
/// Named fields, because audit E6 is what positional settings look like after
/// two years: *"one giant command with 41 numbered slots… this has already
/// caused a 'reuse slot 39 for four columns' patch."*
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Separators {
    pub below_store_header: bool,
    pub below_meta: bool,
    pub below_token: bool,
    pub below_column_names: bool,
    pub below_items: bool,
    pub below_subtotals: bool,
    pub below_grand_total: bool,
    /// **P32 — a rule the template drew with no setting in front of it.**
    ///
    /// `tax_summary` ended with a bare `doc.separator`, so a shop could turn
    /// every separator off and still get this one. A rule nobody can switch off
    /// is the same fault as a setting nobody reads (round 2's lesson), and it
    /// was one of the nine rules that made a quarter of the owner's bill.
    #[serde(default)]
    pub below_tax_summary: bool,
    /// The same, under the payment lines.
    #[serde(default)]
    pub below_payments: bool,
}

impl Default for Separators {
    /// **Four rules on a bill, not nine** — P32.
    ///
    /// Measured on the owner's own paper on 2026-08-23: nine rules cost 243
    /// dots, 30 mm, **26 % of the whole bill**. A rule earns its place where a
    /// reader's eye needs to stop — under the letterhead, around the table, and
    /// around the total — and nowhere else. Every one of the nine is still a
    /// setting; four of them start on.
    fn default() -> Self {
        Separators {
            below_store_header: true,
            below_meta: false,
            below_token: false,
            below_column_names: true,
            below_items: true,
            below_subtotals: false,
            below_grand_total: true,
            below_tax_summary: false,
            below_payments: false,
        }
    }
}

/// Per-section size and bold, exactly as v1 had it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sections {
    pub store_name: Style,
    pub meta: Style,
    pub items: Style,
    pub subtotals: Style,
    pub grand_total: Style,
    pub footer: Style,
    /// v1's "token print size: Normal / Large / Extra Large".
    pub token: Style,
}

/// **A shop's saved sizes carry their cap height explicitly.**
///
/// A style built by `Style::new(2, true)` would be the same size and a
/// different number on the wire, so writing the configuration out and reading
/// it back would not round-trip — the export/import test said so the moment
/// sizes became numbers.
///
/// So the defaults say the number, and the number is a rung of
/// [`Style::LADDER`] — never an arbitrary height, because a setting screen that
/// offers ten choices must not be able to read back an eleventh.
const fn dots(px: u16, bold: bool) -> Style {
    Style::px(px, bold, 24)
}

impl Default for Sections {
    /// **The body is rung 4 and a heading is rung 8** — P32.
    ///
    /// The body was 24 nominal dots, which drew a 13-dot capital: the setting
    /// and the paper disagreed by half. Rung 4 is a 15-dot capital, drawn
    /// honestly, in a row that costs 21 dots against the old 27. Bigger text,
    /// less paper, and the number on the screen is the number on the roll.
    fn default() -> Self {
        Sections {
            store_name: dots(Style::HEADING, true),
            meta: dots(Style::BODY, false),
            items: dots(Style::BODY, false),
            subtotals: dots(Style::BODY, false),
            grand_total: dots(Style::HEADING, true),
            footer: dots(Style::BODY, false),
            token: dots(Style::HEADING, true),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptSettings {
    pub pattern: Pattern,
    pub row_height: RowHeight,
    pub show: Show,
    pub separators: Separators,
    pub sections: Sections,
    pub logo: LogoPosition,
    pub logo_width_pct: u8,
    pub qr: QrMode,
    pub qr_width_pct: u8,
    /// **P29, scope 7.6 — print the bill number as a barcode.**
    ///
    /// Off by default: a shop with no scanner gets nothing out of two extra
    /// lines of paper on every bill. On, it is what makes "scan the bill to
    /// bring it back" possible at all — which is the one thing a scanner does
    /// that nothing else in this product can do.
    #[serde(default)]
    pub bill_barcode: bool,
    /// **P31. The typeface this is printed in**, as a key from
    /// [`crate::font::FAMILIES`].
    ///
    /// A string rather than an enum, and deliberately: the list of faces is a
    /// fact about the MACHINE, not about this format, and a shop whose Windows
    /// has one we have never heard of should be reachable by adding a row to
    /// that table rather than a variant here. An unknown value prints in the
    /// built-in face and says so in the log — never a bill that does not come
    /// out (requirement 3).
    ///
    /// `#[serde(default)]` because shops saved their settings before this
    /// existed, and an empty string means the built-in one for exactly them.
    #[serde(default)]
    pub font: String,
    pub footer: String,
    /// Printed above the totals when the shop is a composition dealer
    /// (scope 2.10) — they may not collect GST and must say so.
    pub composition_note: String,
}

impl Default for ReceiptSettings {
    fn default() -> Self {
        ReceiptSettings {
            pattern: Pattern::Dashed,
            row_height: RowHeight::Standard,
            show: Show::default(),
            separators: Separators::default(),
            sections: Sections::default(),
            logo: LogoPosition::None,
            logo_width_pct: 40,
            qr: QrMode::None,
            qr_width_pct: 40,
            bill_barcode: false,
            // Named rather than empty: "" is not one of the choices, and the
            // settings catalogue is right to refuse a value that is not on its
            // own list.
            font: "builtin".to_owned(),
            footer: "Thank you, visit again".to_owned(),
            composition_note:
                "Composition taxable person, not eligible to collect tax on supplies".to_owned(),
        }
    }
}

/// The kitchen ticket's five separators (audit Part 3, "KOT separators").
///
/// By name, for the reason [`Separators`] gives. Five rather than the bill's
/// seven because a ticket has no totals and no grand total to draw a line under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KitchenSeparators {
    pub below_title: bool,
    pub below_token: bool,
    pub below_details: bool,
    pub below_column_names: bool,
    pub below_items: bool,
}

impl Default for KitchenSeparators {
    fn default() -> Self {
        KitchenSeparators {
            below_title: true,
            below_token: false,
            below_details: true,
            below_column_names: false,
            below_items: true,
        }
    }
}

/// The kitchen ticket's own toggles (audit Part 3, "KOT visibility").
// **Not `Copy` since P31.** It carries a typeface name now, and a `String`
// cannot be. Every caller took it by value out of habit rather than need; the
// ones that did are `&` now, which is what they meant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KitchenSettings {
    pub show_title: bool,
    pub show_token: bool,
    pub show_bill_number: bool,
    pub show_order_type: bool,
    pub show_table: bool,
    pub show_time: bool,
    /// **The eighth toggle, and it is here because the seventh separator asked
    /// for it.** Audit Part 3 lists a KOT separator "below column names", which
    /// v1 could only have drawn under a column header — so the header is a
    /// setting too, rather than a rule under nothing (the mistake
    /// `narrow_items` already documents on the bill side).
    pub show_column_names: bool,
    /// **P32 — the ticket's own running number**, so a cook can say "KOT 14".
    #[serde(default = "yes")]
    pub show_kot_number: bool,
    /// v1's "2-column packing".
    pub two_column: bool,
    pub pattern: Pattern,
    pub separators: KitchenSeparators,
    /// Part 3's "KOT row height", which the bill has had since P06.
    pub row_height: RowHeight,
    pub title: Style,
    pub details: Style,
    pub items: Style,
    /// **P31. The typeface this is printed in**, as a key from
    /// [`crate::font::FAMILIES`].
    ///
    /// A string rather than an enum, and deliberately: the list of faces is a
    /// fact about the MACHINE, not about this format, and a shop whose Windows
    /// has one we have never heard of should be reachable by adding a row to
    /// that table rather than a variant here. An unknown value prints in the
    /// built-in face and says so in the log — never a bill that does not come
    /// out (requirement 3).
    ///
    /// `#[serde(default)]` because shops saved their settings before this
    /// existed, and "empty means the built-in one" is exactly right for them.
    #[serde(default)]
    pub font: String,
}

impl Default for KitchenSettings {
    fn default() -> Self {
        KitchenSettings {
            show_title: true,
            show_token: true,
            show_bill_number: true,
            show_order_type: true,
            show_table: true,
            show_time: true,
            show_column_names: false,
            show_kot_number: true,
            two_column: false,
            pattern: Pattern::Dashed,
            separators: KitchenSeparators::default(),
            // Relaxed, and on purpose: the kitchen reads this at speed with wet
            // hands, and a line of air between dishes is worth the paper.
            row_height: RowHeight::Relaxed,
            // The kitchen reads this across a hot room at speed, so the
            // defaults are deliberately larger than the bill's — the food most
            // of all. Rungs of `Style::LADDER`, for the reason `dots` gives.
            title: dots(Style::HEADING, true),
            details: dots(Style::BODY, true),
            items: dots(Style::LADDER[8], true),
            // Named rather than empty: "" is not one of the choices, and the
            // settings catalogue is right to refuse a value that is not on its
            // own list.
            font: "builtin".to_owned(),
        }
    }
}
