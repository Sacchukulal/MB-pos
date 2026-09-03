use serde::{Deserialize, Serialize};

use crate::doc::{Pattern, Style};

/// Where the logo goes.
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

/// Dynamic carries the amount, so the customer does not type it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrMode {
    #[default]
    None,
    Static,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowHeight {
    Compact,
    #[default]
    Standard,
    Relaxed,
}

impl RowHeight {
    /// Blank lines after each ITEM.
    #[must_use]
    pub const fn gap(self) -> u8 {
        match self {
            RowHeight::Compact | RowHeight::Standard => 0,
            RowHeight::Relaxed => 1,
        }
    }

    /// Blank lines between one block of the receipt and the next.
    #[must_use]
    pub const fn section_gap(self) -> u8 {
        match self {
            RowHeight::Compact => 0,
            RowHeight::Standard | RowHeight::Relaxed => 1,
        }
    }

    /// The air between one section and the next, in half body rows — the letterhead from the
    /// bill's details, the details from the items, the items from the sums. Compact packs
    /// them; Relaxed gives a whole row.
    #[must_use]
    pub const fn section_air(self) -> u8 {
        match self {
            RowHeight::Compact => 0,
            RowHeight::Standard => 1,
            RowHeight::Relaxed => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Show {
    pub token: bool,
    pub gstin: bool,
    pub fssai: bool,
    pub address: bool,
    pub phone: bool,
    pub cashier: bool,
    /// Off by default: a shop below the turnover threshold does not need it and the column
    /// costs eight characters of a 32-column receipt.
    pub hsn: bool,
    /// The rate-wise summary.
    pub tax_summary: bool,
    /// "Cash 300 / UPI 200".
    pub payment_lines: bool,
    /// "TAX INVOICE" or "BILL OF SUPPLY" at the head of the bill.
    #[serde(default = "yes")]
    pub title: bool,
    /// The time of day beside the date.
    #[serde(default = "yes")]
    pub time: bool,
    /// How many people ate.
    #[serde(default = "yes")]
    pub covers: bool,
    /// Who took the order, as against who took the money.
    #[serde(default = "yes")]
    pub waiter: bool,
    /// The state code the supply happened in.
    #[serde(default)]
    pub place_of_supply: bool,
    /// The grand total spelled out.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Separators {
    pub below_store_header: bool,
    pub below_meta: bool,
    pub below_token: bool,
    pub below_column_names: bool,
    pub below_items: bool,
    pub below_subtotals: bool,
    pub below_grand_total: bool,
    /// A rule the template drew with no setting in front of it.
    #[serde(default)]
    pub below_tax_summary: bool,
    /// The same, under the payment lines.
    #[serde(default)]
    pub below_payments: bool,
}

impl Default for Separators {
    /// Four rules on a bill, not nine.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sections {
    pub store_name: Style,
    pub meta: Style,
    pub items: Style,
    pub subtotals: Style,
    pub grand_total: Style,
    pub footer: Style,
    pub token: Style,
}

/// A shop's saved sizes carry their cap height explicitly.
const fn dots(px: u16, bold: bool) -> Style {
    Style::px(px, bold, 24)
}

impl Default for Sections {
    /// The body is rung 4 and a heading is rung 8.
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
    #[serde(default)]
    pub bill_barcode: bool,
    /// The typeface this is printed in, as a key from `crate::font::FAMILIES`.
    #[serde(default)]
    pub font: String,
    pub footer: String,
    /// Printed above the totals when the shop is a composition dealer — they may not collect
    /// GST and must say so.
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
            // Named rather than empty: "" is not one of the choices, and the settings catalogue
            // is right to refuse a value that is not on its own list.
            font: crate::font::DEFAULT_KEY.to_owned(),
            footer: "Thank you, visit again".to_owned(),
            composition_note: "Composition taxable person, not eligible to collect tax on supplies"
                .to_owned(),
        }
    }
}

/// The kitchen ticket's five separators.
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

/// The kitchen ticket's own toggles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KitchenSettings {
    pub show_title: bool,
    pub show_token: bool,
    pub show_bill_number: bool,
    pub show_order_type: bool,
    pub show_table: bool,
    pub show_time: bool,
    /// The eighth toggle, and it is here because the seventh separator asked for it.
    pub show_column_names: bool,
    /// The ticket's own running number, so a cook can say "KOT 14".
    #[serde(default = "yes")]
    pub show_kot_number: bool,
    pub two_column: bool,
    pub pattern: Pattern,
    pub separators: KitchenSeparators,
    pub row_height: RowHeight,
    pub title: Style,
    pub details: Style,
    pub items: Style,
    /// The typeface this is printed in, as a key from `crate::font::FAMILIES`.
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
            // Relaxed, and on purpose: the kitchen reads this at speed with wet hands, and a
            // line of air between dishes is worth the paper.
            row_height: RowHeight::Relaxed,
            // The kitchen reads this across a hot room at speed, so the defaults are
            // deliberately larger than the bill's — the food most of all.
            title: dots(Style::HEADING, true),
            details: dots(Style::BODY, true),
            items: dots(Style::LADDER[8], true),
            // Named rather than empty: "" is not one of the choices, and the settings catalogue
            // is right to refuse a value that is not on its own list.
            font: crate::font::DEFAULT_KEY.to_owned(),
        }
    }
}
