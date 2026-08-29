//! The reports, as one shape — so one screen renders all of them.

use mb_auth::Permission;
use mb_core::{BusinessDay, Money};
use mb_db::repo::reports::{Period, SalesBy, TaxBucket};
use mb_db::repo::settings::StoreProfile;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::guard;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// The one shape.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ReportView {
    pub id: String,
    pub title: String,
    /// The period, in words: "1 Aug.
    pub subtitle: String,
    pub columns: Vec<ReportColumn>,
    /// Already formatted. TypeScript does no arithmetic on money.
    pub rows: Vec<Vec<String>>,
    pub totals: Option<Vec<String>>,
    /// Against the previous equal period.
    pub compare: Option<CompareView>,
    /// Anything the report needs to admit: "4 items have no cost price".
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ReportColumn {
    pub header: String,
    /// Right-aligned and tabular. Money and counts, always (§3).
    pub numeric: bool,
}

/// What changed since the period before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CompareView {
    /// "1 Jul – 31 Jul", so a person can see WHAT it is being compared against rather than
    /// trusting a percentage.
    pub period: String,
    pub before: String,
    pub now: String,
    /// The whole sentence: "up 8% on the 31 days before".
    pub summary: String,
    /// `up`, `down` or `same`.
    pub direction: String,
}

/// One report a person can choose.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub id: &'static str,
    pub title: &'static str,
    pub group: &'static str,
    pub needs: Permission,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Sales(SalesBy),
    TaxByRate,
    TaxByHsn,
    /// The GST offline tool's B2CS sheet — counter sales, rate-wise.
    Gstr1B2cs,
    Control,
    MenuEngineering,
    StoppedSelling,
    KitchenSpeed(SpeedBy),
    Buying(BuyingBy),
    /// The rise is the finding: onion is up 46% on its three-month average.
    PriceTrend,
    /// Rows for a claiming shop; one honest sentence for a 5%-scheme one.
    InputCredit,
    /// Who the shop owes, oldest first.
    SupplierOutstanding,
    /// What is on the shelf, at what it actually cost.
    StockValuation,
    CountVariance,
    Profit,
    Tips,
    Handover,
}

/// How the buying report is grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuyingBy {
    Supplier,
    /// "Where does ₹40,000 a month of raw material go?" — the question the whole inventory
    /// module exists to answer.
    Material,
}

/// How the kitchen-speed report is grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedBy {
    Station,
    /// The hour of the day, in the shop's own time — which is how a shop finds out that seven
    /// o'clock is where it loses people.
    Hour,
}

/// The list, and it is the screen.
pub const CATALOGUE: &[Entry] = &[
    Entry {
        id: "sales_day",
        title: "Sales by day",
        group: "Sales",
        needs: Permission::ReportsView,
        kind: Kind::Sales(SalesBy::Day),
    },
    Entry {
        id: "sales_hour",
        title: "Sales by hour — your peak",
        group: "Sales",
        needs: Permission::ReportsView,
        kind: Kind::Sales(SalesBy::Hour),
    },
    Entry {
        id: "sales_type",
        title: "Sales by order type",
        group: "Sales",
        needs: Permission::ReportsView,
        kind: Kind::Sales(SalesBy::OrderType),
    },
    Entry {
        id: "sales_mode",
        title: "Sales by payment mode",
        group: "Sales",
        needs: Permission::ReportsView,
        kind: Kind::Sales(SalesBy::PaymentMode),
    },
    Entry {
        id: "sales_cashier",
        title: "Sales by cashier",
        group: "Sales",
        needs: Permission::ReportsView,
        kind: Kind::Sales(SalesBy::Cashier),
    },
    Entry {
        id: "sales_section",
        title: "Sales by section",
        group: "Sales",
        needs: Permission::ReportsView,
        kind: Kind::Sales(SalesBy::Section),
    },
    Entry {
        id: "sales_terminal",
        title: "Sales by till",
        group: "Sales",
        needs: Permission::ReportsView,
        kind: Kind::Sales(SalesBy::Terminal),
    },
    Entry {
        id: "items",
        title: "Item sales",
        group: "Items",
        needs: Permission::ReportsView,
        kind: Kind::Sales(SalesBy::Item),
    },
    Entry {
        id: "categories",
        title: "Category sales",
        group: "Items",
        needs: Permission::ReportsView,
        kind: Kind::Sales(SalesBy::Category),
    },
    Entry {
        id: "stopped",
        title: "Items that stopped selling",
        group: "Items",
        needs: Permission::ReportsView,
        kind: Kind::StoppedSelling,
    },
    // Cost and margin are behind `reports.view` too, but the MENU screen keeps cost behind the
    // same permission — so this is consistent rather than a new rule.
    Entry {
        id: "margin",
        title: "Menu engineering — volume against margin",
        group: "Items",
        needs: Permission::ReportsView,
        kind: Kind::MenuEngineering,
    },
    Entry {
        id: "tax_rate",
        title: "Tax, rate-wise",
        group: "Tax",
        needs: Permission::ReportsView,
        kind: Kind::TaxByRate,
    },
    Entry {
        id: "tax_hsn",
        title: "Tax, HSN-wise",
        group: "Tax",
        needs: Permission::ReportsView,
        kind: Kind::TaxByHsn,
    },
    Entry {
        id: "gstr1_b2cs",
        title: "GSTR-1 (B2CS) — for the GST offline tool",
        group: "Tax",
        needs: Permission::ReportsView,
        kind: Kind::Gstr1B2cs,
    },
    // Beside the other two, and that is not cosmetic.
    Entry {
        id: "input_credit",
        title: "Input tax credit on purchases",
        group: "Tax",
        needs: Permission::ReportsView,
        kind: Kind::InputCredit,
    },
    Entry {
        id: "control",
        title: "Voids, discounts, refunds and reprints",
        group: "Control",
        needs: Permission::AuditView,
        kind: Kind::Control,
    },
    Entry {
        id: "tips",
        title: "Tips, and who took them",
        group: "Control",
        needs: Permission::ReportsView,
        kind: Kind::Tips,
    },
    Entry {
        id: "handover",
        title: "Shift handovers — every drawer counted",
        group: "Control",
        needs: Permission::DayClose,
        kind: Kind::Handover,
    },
    Entry {
        id: "kitchen_station",
        title: "Kitchen speed, by station",
        group: "Kitchen",
        needs: Permission::ReportsView,
        kind: Kind::KitchenSpeed(SpeedBy::Station),
    },
    Entry {
        id: "kitchen_hour",
        title: "Kitchen speed, by hour",
        group: "Kitchen",
        needs: Permission::ReportsView,
        kind: Kind::KitchenSpeed(SpeedBy::Hour),
    },
    // Nine reports and not one `.tsx` file changed.
    Entry {
        id: "buying_supplier",
        title: "What you bought, by supplier",
        group: "Buying",
        needs: Permission::PurchasesManage,
        kind: Kind::Buying(BuyingBy::Supplier),
    },
    Entry {
        id: "buying_material",
        title: "What you bought, by material",
        group: "Buying",
        needs: Permission::InventoryView,
        kind: Kind::Buying(BuyingBy::Material),
    },
    Entry {
        id: "price_trend",
        title: "Price trend — what is going up",
        group: "Buying",
        needs: Permission::InventoryView,
        kind: Kind::PriceTrend,
    },
    Entry {
        id: "supplier_outstanding",
        title: "Who you owe",
        group: "Buying",
        needs: Permission::PurchasesManage,
        kind: Kind::SupplierOutstanding,
    },
    Entry {
        id: "stock_value",
        title: "Stock on hand, at cost",
        group: "Stock",
        needs: Permission::InventoryView,
        kind: Kind::StockValuation,
    },
    Entry {
        id: "count_variance",
        title: "Stock counts — what keeps going missing",
        group: "Stock",
        needs: Permission::InventoryView,
        kind: Kind::CountVariance,
    },
    // The one that makes the word "profit" mean something.
    Entry {
        id: "profit",
        title: "Profit — sales, food cost and running costs",
        group: "Money",
        needs: Permission::ReportsView,
        kind: Kind::Profit,
    },
];

#[must_use]
pub fn find(id: &str) -> Option<&'static Entry> {
    CATALOGUE.iter().find(|e| e.id == id)
}

/// What the screen offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ReportListView {
    pub reports: Vec<ReportEntryView>,
    /// The buttons above the date boxes: Today, Yesterday, This month.
    pub periods: Vec<PeriodChoiceView>,
}

/// A one-press period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PeriodChoiceView {
    pub label: String,
    pub from: String,
    pub to: String,
}

/// The presets, from the shop's own idea of today.
fn choices(today: BusinessDay) -> Vec<PeriodChoiceView> {
    let one = |label: &str, from: BusinessDay, to: BusinessDay| PeriodChoiceView {
        label: label.to_owned(),
        from: from.to_string(),
        to: to.to_string(),
    };
    let back = |days: i32| BusinessDay::from_days_since_epoch(today.days_since_epoch() - days);
    let (year, month, _) = today.to_ymd();
    let month_start = BusinessDay::from_ymd(year, month, 1);
    let last_month_end = month_start.previous();
    let (ly, lm, _) = last_month_end.to_ymd();
    vec![
        one("Today", today, today),
        one("Yesterday", back(1), back(1)),
        one("Last 7 days", back(6), today),
        one("This month", month_start, today),
        one(
            "Last month",
            BusinessDay::from_ymd(ly, lm, 1),
            last_month_end,
        ),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ReportEntryView {
    pub id: String,
    pub title: String,
    pub group: String,
}

/// A period, as the screen asks for one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PeriodArg {
    pub from: String,
    pub to: String,
}

impl PeriodArg {
    fn parse(&self) -> UiResult<Period> {
        let bad = |which: &str, text: &str| {
            UiError::new(
                "report.period",
                format!("The {which} date could not be read. Pick it again."),
            )
            .with_detail(format!("{which} = {text:?}"))
        };
        let from: BusinessDay = self.from.parse().map_err(|_| bad("from", &self.from))?;
        let to: BusinessDay = self.to.parse().map_err(|_| bad("to", &self.to))?;
        if from.days_until(to) < 0 {
            // Swapping silently would be a screen guessing.
            return Err(UiError::new(
                "report.period",
                "The end date is before the start date.",
            ));
        }
        Ok(Period::new(from, to))
    }
}

// Building one.

fn column(header: &str, numeric: bool) -> ReportColumn {
    ReportColumn {
        header: header.to_owned(),
        numeric,
    }
}

fn period_words(period: Period) -> String {
    let days = period.days();
    if days == 1 {
        return period.from.to_string();
    }
    format!("{} to {} · {days} days", period.from, period.to)
}

/// The comparison sentence, built in Rust (§6).
fn compare(now: Money, before: Money, previous: Period) -> CompareView {
    let direction = if now > before {
        "up"
    } else if now < before {
        "down"
    } else {
        "same"
    };
    // Percent as an integer, computed on paise.
    let percent = if before.is_zero() {
        None
    } else {
        Some(
            (now.paise()
                .saturating_sub(before.paise())
                .saturating_mul(100))
            .saturating_div(before.paise().abs().max(1)),
        )
    };
    // "the day before" reads better than "the 1 days before" — which is what this said until
    // somebody looked at it.
    let span = if previous.days() == 1 {
        "the day before".to_owned()
    } else {
        format!("the {} days before", previous.days())
    };
    let summary = match (direction, percent) {
        (_, None) if before.is_zero() && now.is_zero() => {
            format!("Nothing {span} either.")
        }
        (_, None) => format!("Nothing at all {span}."),
        ("same", _) => format!("Exactly the same as {span}."),
        (dir, Some(p)) => format!(
            "{} {}% on {span} ({}).",
            if dir == "up" { "Up" } else { "Down" },
            p.abs(),
            before.to_plain_string()
        ),
    };
    CompareView {
        period: period_words(previous),
        before: before.to_plain_string(),
        now: now.to_plain_string(),
        summary,
        direction: direction.to_owned(),
    }
}

/// Build one report.
pub fn report_on(app: &App, id: String, period: PeriodArg) -> UiResult<ReportView> {
    let Some(entry) = find(&id) else {
        return Err(UiError::new(
            "report.unknown",
            "There is no such report. Refresh and try again.",
        )
        .with_detail(id));
    };
    guard::require(app, entry.needs)?;
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    let period = period.parse()?;

    // A reader, not the writer.
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| build(&mb_db::Repos::new(tx), entry, period))
            .map_err(|e| words::from_db(&e))
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "one match arm per report; splitting it would put each report's \
              shape further from the query it comes from"
)]
fn build(
    repos: &mb_db::Repos<'_>,
    entry: &Entry,
    period: Period,
) -> Result<ReportView, mb_db::DbError> {
    let reports = repos.reports();
    let mut notes = Vec::new();
    let (columns, rows, totals, compare_view) = match entry.kind {
        Kind::Sales(by) => {
            let buckets = reports.sales_by(OUTLET, period, by)?;
            let wants_qty = matches!(by, SalesBy::Item | SalesBy::Category);
            let mut columns = vec![column(label_for(by), false), column("Bills", true)];
            if wants_qty {
                columns.push(column("Quantity", true));
            }
            columns.push(column("Discount", true));
            columns.push(column("Tax", true));
            columns.push(column("Total", true));

            let mut gross = Money::ZERO;
            let mut discount = Money::ZERO;
            let mut tax = Money::ZERO;
            let mut qty = mb_core::Qty::ZERO;
            let mut bills = 0_i64;
            let rows = buckets
                .iter()
                .map(|b| {
                    gross = gross.add(b.gross).unwrap_or(gross);
                    discount = discount.add(b.discount).unwrap_or(discount);
                    tax = tax.add(b.tax).unwrap_or(tax);
                    if let Some(each) = b.qty {
                        qty = qty.add(each).unwrap_or(qty);
                    }
                    bills += b.bills;
                    let mut row = vec![b.label.clone(), b.bills.to_string()];
                    if wants_qty {
                        row.push(b.qty.map(|q| q.to_string()).unwrap_or_default());
                    }
                    row.push(b.discount.to_plain_string());
                    row.push(b.tax.to_plain_string());
                    row.push(b.gross.to_plain_string());
                    row
                })
                .collect();

            // Every column that has figures in it gets a total.
            let mut totals = vec!["Total".to_owned(), bills.to_string()];
            if wants_qty {
                totals.push(qty.to_string());
            }
            totals.extend([
                discount.to_plain_string(),
                tax.to_plain_string(),
                gross.to_plain_string(),
            ]);

            // The comparison, from the same query over the previous period.
            let previous = period.previous();
            let before = reports
                .sales_by(OUTLET, previous, by)?
                .iter()
                .try_fold(Money::ZERO, |sum, b| sum.add(b.gross))
                .unwrap_or(Money::ZERO);
            (
                columns,
                rows,
                Some(totals),
                Some(compare(gross, before, previous)),
            )
        }

        Kind::TaxByRate => {
            let buckets = reports.tax_by_rate(OUTLET, period)?;
            let columns = vec![
                column("Rate", false),
                column("Taxable", true),
                column("CGST", true),
                column("SGST", true),
                column("IGST", true),
                column("VAT", true),
            ];
            let mut taxable = Money::ZERO;
            let rows = buckets
                .iter()
                .map(|b| {
                    taxable = taxable.add(b.taxable).unwrap_or(taxable);
                    vec![
                        rate_words(b.rate_bp, &b.tax_kind),
                        b.taxable.to_plain_string(),
                        b.cgst.to_plain_string(),
                        b.sgst.to_plain_string(),
                        b.igst.to_plain_string(),
                        b.vat.to_plain_string(),
                    ]
                })
                .collect();
            notes.push(
                "These are the figures the bills printed, added up — not a \
                 second calculation. Liquor and exempt items are listed \
                 separately and are never inside a GST total."
                    .to_owned(),
            );
            (
                columns,
                rows,
                Some(vec![
                    "Total".to_owned(),
                    taxable.to_plain_string(),
                    String::new(),
                    String::new(),
                    String::new(),
                ]),
                None,
            )
        }

        // The offline tool's own columns, in its own order.
        Kind::Gstr1B2cs => {
            let profile = repos.settings().store_profile(OUTLET)?;
            let buckets = reports.tax_by_rate(OUTLET, period)?;
            let (rows, taxable, said) = b2cs(&buckets, profile.as_ref());
            notes.extend(said);
            let columns = vec![
                column("Type", false),
                column("Place Of Supply", false),
                column("Rate", true),
                column("Applicable % of Tax Rate", true),
                column("Taxable Value", true),
                column("Cess Amount", true),
                column("E-Commerce GSTIN", false),
            ];
            // No totals row. `csv_of` writes it into the file, and the offline tool reads it as
            // a malformed eighth invoice.
            if !rows.is_empty() {
                notes.push(format!(
                    "Total taxable value: {}",
                    taxable.to_plain_string()
                ));
            }
            (columns, rows, None, None)
        }

        Kind::TaxByHsn => {
            let buckets = reports.tax_by_hsn(OUTLET, period)?;
            let columns = vec![
                column("HSN", false),
                column("Quantity", true),
                column("Taxable", true),
                column("CGST", true),
                column("SGST", true),
                column("IGST", true),
            ];
            let missing = buckets.iter().filter(|b| b.hsn.is_empty()).count();
            if missing > 0 {
                notes.push(
                    "Some lines have no HSN code and are grouped under \
                     \"(none)\". A GST return above the turnover threshold \
                     needs one on every item — set them on the Menu screen."
                        .to_owned(),
                );
            }
            let rows = buckets
                .iter()
                .map(|b| {
                    vec![
                        if b.hsn.is_empty() {
                            "(none)".to_owned()
                        } else {
                            b.hsn.clone()
                        },
                        b.qty.to_string(),
                        b.taxable.to_plain_string(),
                        b.cgst.to_plain_string(),
                        b.sgst.to_plain_string(),
                        b.igst.to_plain_string(),
                    ]
                })
                .collect();
            (columns, rows, None, None)
        }

        Kind::Control => {
            let entries = reports.control_log(OUTLET, period)?;
            let columns = vec![
                column("Day", false),
                column("What", false),
                column("Bill", false),
                column("Who", false),
                column("Why", false),
                column("Amount", true),
            ];
            let rows = entries
                .iter()
                .map(|row| {
                    vec![
                        row.business_day.to_string(),
                        control_words(&row.kind).to_owned(),
                        row.reference.clone(),
                        row.who.clone(),
                        row.reason.clone(),
                        row.amount.to_plain_string(),
                    ]
                })
                .collect();
            (columns, rows, None, None)
        }

        Kind::Handover => {
            let rows_in = reports.handovers(OUTLET, period)?;
            let columns = vec![
                column("Day", false),
                column("Till", false),
                column("Shift", true),
                column("Closed by", false),
                column("At", false),
                column("Expected", true),
                column("Counted", true),
                column("Difference", true),
                column("Who was on", false),
                column("Reason given", false),
            ];
            let short = rows_in.iter().filter(|r| r.variance.is_negative()).count();
            let rows = rows_in
                .iter()
                .map(|row| {
                    vec![
                        row.day.to_string(),
                        row.till.clone(),
                        row.shift_no.to_string(),
                        row.closed_by.clone(),
                        crate::words::when(row.closed_at),
                        row.expected.to_plain_string(),
                        row.counted.to_plain_string(),
                        // The difference in words, never a bare minus sign.
                        crate::dayclose::variance_words(row.variance),
                        row.who_was_on.join(", "),
                        row.note.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            if short > 0 {
                notes.push(format!(
                    "{short} of these came up short. The reason beside each one \
                     is what somebody typed at the time — that is what it is \
                     for."
                ));
            }
            (columns, rows, None, None)
        }

        Kind::Tips => {
            let rows_in = reports.tips_by_staff(OUTLET, period)?;
            let columns = vec![
                column("Who settled", false),
                column("Bills", true),
                column("Cash — in the drawer", true),
                column("Card and UPI — owed", true),
                column("Total", true),
            ];
            let rows = rows_in
                .iter()
                .map(|row| {
                    vec![
                        row.who.clone(),
                        row.bills.to_string(),
                        row.cash.to_plain_string(),
                        row.other.to_plain_string(),
                        row.total.to_plain_string(),
                    ]
                })
                .collect();
            // The sentence matters more than the table here, because the one thing a shop can
            // get wrong about tips is thinking they are takings.
            notes.push(
                "A tip is not the shop's money. It is in no sales figure and \
                 no tax figure. Cash tips are already in the drawer, so they \
                 come out of it when they are handed over."
                    .to_owned(),
            );
            (columns, rows, None, None)
        }

        Kind::MenuEngineering => {
            let items = reports.menu_engineering(OUTLET, period)?;
            let columns = vec![
                column("Item", false),
                column("Quantity", true),
                column("Revenue", true),
                column("Cost", true),
                column("Margin", true),
            ];
            let uncosted = items.iter().filter(|i| i.cost.is_none()).count();
            if uncosted > 0 {
                notes.push(format!(
                    "{uncosted} item(s) have no cost price, so their margin is \
                     unknown rather than 100%. Set it on the Menu screen."
                ));
            }
            let rows = items
                .iter()
                .map(|item| {
                    // The cost of what was SOLD: the item's unit cost times the quantity.
                    let cost = item.cost.and_then(|unit| item.qty.extend(unit).ok());
                    let margin = cost.and_then(|c| item.revenue.sub(c).ok());
                    vec![
                        item.name.clone(),
                        item.qty.to_string(),
                        item.revenue.to_plain_string(),
                        cost.map_or_else(|| "not known".to_owned(), Money::to_plain_string),
                        margin.map_or_else(|| "—".to_owned(), Money::to_plain_string),
                    ]
                })
                .collect();
            (columns, rows, None, None)
        }

        Kind::StoppedSelling => {
            let items = reports.stopped_selling(OUTLET, period)?;
            let columns = vec![
                column("Item", false),
                column("Sold before", true),
                column("Was worth", true),
            ];
            notes.push(format!(
                "These sold in the {} days before {} and not since.",
                period.previous().days(),
                period.from
            ));
            let rows = items
                .iter()
                .map(|item| {
                    vec![
                        item.label.clone(),
                        item.qty.map(|q| q.to_string()).unwrap_or_default(),
                        item.gross.to_plain_string(),
                    ]
                })
                .collect();
            (columns, rows, None, None)
        }

        // Time from "the kitchen was told" to "it came off the pass", which is the only
        // definition that ties back to a bill.
        Kind::KitchenSpeed(by) => {
            // The STORED business day, like every other report here.
            let kitchen = repos.kitchen();
            let speed = match by {
                SpeedBy::Station => kitchen.speed_by_station(OUTLET, period.from, period.to)?,
                SpeedBy::Hour => kitchen.speed_by_hour(OUTLET, period.from, period.to)?,
            };
            let columns = vec![
                column(
                    if by == SpeedBy::Station {
                        "Station"
                    } else {
                        "Hour"
                    },
                    false,
                ),
                column("Tickets", true),
                column("Average", true),
                column("Slowest", true),
                column("Late", true),
            ];
            notes.push(
                "Measured from the moment the kitchen was told to the moment a \
                 cook pressed Done. \"Late\" counts tickets that took longer \
                 than the dishes on them were expected to."
                    .to_owned(),
            );
            if speed.is_empty() {
                notes.push(
                    "Nothing here yet. This fills in once the kitchen screen has \
                     been used for a service."
                        .to_owned(),
                );
            }
            let rows = speed
                .iter()
                .map(|row| {
                    vec![
                        if by == SpeedBy::Hour {
                            format!("{}:00", row.label)
                        } else {
                            row.label.clone()
                        },
                        row.tickets.to_string(),
                        minutes_and_seconds(row.average_ms),
                        minutes_and_seconds(row.slowest_ms),
                        row.late.to_string(),
                    ]
                })
                .collect();
            (columns, rows, None, None)
        }

        Kind::Buying(by) => {
            let buying = repos.buying();
            let list = match by {
                BuyingBy::Supplier => buying.by_supplier(OUTLET, period.from, period.to)?,
                BuyingBy::Material => buying.by_material(OUTLET, period.from, period.to)?,
            };
            let mut columns = vec![
                column(
                    if by == BuyingBy::Supplier {
                        "Supplier"
                    } else {
                        "Material"
                    },
                    false,
                ),
                column("Deliveries", true),
            ];
            if by == BuyingBy::Material {
                columns.push(column("Quantity", true));
            }
            columns.push(column("Value", true));
            if by == BuyingBy::Supplier {
                columns.push(column("GST on it", true));
            }
            let total = Money::try_sum(list.iter().map(|r| r.value)).unwrap_or(Money::ZERO);
            notes.push(
                "What you paid, all in — the rate less discounts, plus transport and \
                 any GST you cannot claim back. Cancelled invoices are left out; goods \
                 you sent back are subtracted."
                    .to_owned(),
            );
            // "275 kg", never "275000 g".
            let materials = repos.stock().materials(OUTLET, true)?;
            let rows = list
                .iter()
                .map(|row| {
                    let mut cells = vec![row.label.clone(), row.count.to_string()];
                    if by == BuyingBy::Material {
                        cells.push(match row.qty {
                            Some(qty) => materials
                                .iter()
                                .find(|m| m.id.as_str() == row.key)
                                .map_or_else(
                                    || format!("{qty} {}", row.unit),
                                    |m| m.units().say(qty),
                                ),
                            None => String::new(),
                        });
                    }
                    cells.push(row.value.to_plain_string());
                    if by == BuyingBy::Supplier {
                        cells.push(row.tax.to_plain_string());
                    }
                    cells
                })
                .collect();
            let mut totals = vec!["Total".to_owned(), String::new()];
            if by == BuyingBy::Material {
                totals.push(String::new());
            }
            totals.push(total.to_plain_string());
            if by == BuyingBy::Supplier {
                totals.push(String::new());
            }
            (columns, rows, Some(totals), None)
        }

        // The rise is the finding.
        Kind::PriceTrend => {
            let trend = repos.buying().price_trend(OUTLET, period.from, period.to)?;
            let columns = vec![
                column("Material", false),
                column("Deliveries", true),
                column("Cheapest", true),
                column("Dearest", true),
                column("Average", true),
                column("Last", true),
                column("Change", false),
            ];
            notes.push(
                "Per kilo, litre or piece, and it is what the food ACTUALLY cost — \
                 transport and non-claimable GST included. \"Change\" compares the last \
                 delivery with the average over this period."
                    .to_owned(),
            );
            let rows = trend
                .iter()
                .map(|row| {
                    vec![
                        row.name.clone(),
                        row.deliveries.to_string(),
                        Money::from_paise(row.cheapest.paise_per_thousand()).to_plain_string(),
                        Money::from_paise(row.dearest.paise_per_thousand()).to_plain_string(),
                        Money::from_paise(row.average.paise_per_thousand()).to_plain_string(),
                        Money::from_paise(row.latest.paise_per_thousand()).to_plain_string(),
                        change_words(row.change_bp()),
                    ]
                })
                .collect();
            (columns, rows, None, None)
        }

        // And the sentence IS the report for most shops.
        Kind::InputCredit => {
            // Only a regular taxpayer can take the credit back.
            let claims = app_registration(repos)?.charges_gst();
            let rows_in = repos
                .buying()
                .input_credit(OUTLET, period.from, period.to)?;
            let columns = vec![
                column("Rate", false),
                column("Invoices", true),
                column("Taxable value", true),
                column("GST paid", true),
            ];
            if claims {
                let claimable = repos
                    .buying()
                    .creditable_total(OUTLET, period.from, period.to)?;
                notes.push(format!(
                    "You can claim {} of this back. Set it against the GST you collected \
                     on sales (Tax, rate-wise).",
                    claimable.to_plain_string()
                ));
            } else {
                notes.push(
                    "You bill under the 5% scheme, so purchase GST is a cost and not a \
                     credit. It is already inside your food cost, and there is nothing \
                     to claim. Change this in Settings → Tax if it is wrong."
                        .to_owned(),
                );
            }
            let rows = rows_in
                .iter()
                .map(|row| {
                    vec![
                        row.label.clone(),
                        row.count.to_string(),
                        row.value.to_plain_string(),
                        row.tax.to_plain_string(),
                    ]
                })
                .collect();
            (columns, rows, None, None)
        }

        Kind::SupplierOutstanding => {
            let owing = repos.buying().outstanding(OUTLET, period.to)?;
            let columns = vec![
                column("Supplier", false),
                column("Owed", true),
                column("Not due yet", true),
                column("30 days", true),
                column("60 days", true),
                column("90 days +", true),
                column("Oldest", false),
            ];
            notes.push(
                "Aged from the day each invoice falls DUE, using the payment terms you \
                 gave that supplier — so \"not due yet\" means exactly that."
                    .to_owned(),
            );
            let total = Money::try_sum(owing.iter().map(|o| o.balance)).unwrap_or(Money::ZERO);
            let rows = owing
                .iter()
                .map(|o| {
                    vec![
                        o.supplier.name.clone(),
                        o.balance.to_plain_string(),
                        o.ageing.current.to_plain_string(),
                        o.ageing.days_30.to_plain_string(),
                        o.ageing.days_60.to_plain_string(),
                        o.ageing.days_90.to_plain_string(),
                        match o.ageing.oldest_days {
                            Some(n) if n > 0 => format!("{n} days overdue"),
                            Some(_) => "not due yet".to_owned(),
                            None => String::new(),
                        },
                    ]
                })
                .collect();
            (
                columns,
                rows,
                Some(vec![
                    "Total".to_owned(),
                    total.to_plain_string(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ]),
                None,
            )
        }

        Kind::StockValuation => {
            let held = repos.stock().on_hand(OUTLET, false)?;
            let columns = vec![
                column("Material", false),
                column("On hand", true),
                column("Cost", true),
                column("Value", true),
                column("Last counted", false),
            ];
            notes.push(
                "Valued at the weighted average of what actually came in, never at a \
                 price somebody typed. A material nobody has counted is what the software \
                 worked out, not what is on the shelf."
                    .to_owned(),
            );
            let total = Money::try_sum(held.iter().map(mb_db::repo::stock::OnHand::value))
                .unwrap_or(Money::ZERO);
            let rows = held
                .iter()
                .map(|row| {
                    let units = row.material.units();
                    vec![
                        row.material.name.clone(),
                        units.say(row.base_qty),
                        Money::from_paise(row.material.avg_cost.paise_per_thousand())
                            .to_plain_string(),
                        row.value().to_plain_string(),
                        // "never" is a real answer and this says it.
                        match row.material.last_counted_at {
                            Some(_) => "counted".to_owned(),
                            None => "never counted".to_owned(),
                        },
                    ]
                })
                .collect();
            (
                columns,
                rows,
                Some(vec![
                    "Total".to_owned(),
                    String::new(),
                    String::new(),
                    total.to_plain_string(),
                    String::new(),
                ]),
                None,
            )
        }

        Kind::CountVariance => {
            let history = repos
                .counts()
                .variance_history(OUTLET, period.from, period.to)?;
            let columns = vec![
                column("Material", false),
                column("Counts", true),
                column("Out by", true),
                column("Worth", true),
                column("Last counted", false),
            ];
            if history.is_empty() {
                notes.push(
                    "No count has been approved in this period. Until somebody walks the \
                     store with a clipboard, every stock figure is what the software \
                     worked out — Stock → Count."
                        .to_owned(),
                );
            } else {
                notes.push(
                    "Approved counts only. A material that is short month after month is \
                     the finding; one bad month is usually a recipe that needs correcting."
                        .to_owned(),
                );
            }
            let rows = history
                .iter()
                .map(|row| {
                    vec![
                        row.name.clone(),
                        row.counts.to_string(),
                        format!("{} {}", row.variance_qty, row.unit),
                        row.variance_value.to_plain_string(),
                        row.last_counted.to_string(),
                    ]
                })
                .collect();
            (columns, rows, None, None)
        }

        // Three blocks, and the double count is named out loud.
        Kind::Profit => {
            let profit = repos.reports().profit(OUTLET, period)?;
            let columns = vec![column("", false), column("Amount", true)];
            // The deductions are shown NEGATIVE, and the labels say "less".
            let mut rows = vec![
                vec![
                    "Sales, less the GST on them".to_owned(),
                    profit.net_sales.to_plain_string(),
                ],
                vec![
                    "less what the food cost".to_owned(),
                    profit.food_used.neg().to_plain_string(),
                ],
                vec![
                    "less wastage".to_owned(),
                    profit.wastage.neg().to_plain_string(),
                ],
                vec![
                    "less what a count found missing".to_owned(),
                    profit.shrinkage.neg().to_plain_string(),
                ],
                vec![
                    "Gross margin".to_owned(),
                    profit.gross_margin.to_plain_string(),
                ],
                vec![
                    "less running costs".to_owned(),
                    profit.running_costs.neg().to_plain_string(),
                ],
                vec!["What is left".to_owned(), profit.left.to_plain_string()],
            ];
            if let Some(bp) = profit.margin_bp() {
                rows.push(vec![
                    "Gross margin as a share of sales".to_owned(),
                    margin_words(bp),
                ]);
            }
            notes.push(
                // No markdown: a report note is printed as it is, and the first version showed
                // a shopkeeper literal asterisks.
                "Money you paid for stock is not a running cost. Rice bought on the \
                 3rd is a cost when it is eaten, not when it is paid for — which is the \
                 whole difference between this number and the one every till prints."
                    .to_owned(),
            );
            if profit.double_counted.is_positive() {
                notes.push(format!(
                    "{} of your running costs are in categories you also buy through \
                     Purchases. That money may be counted twice — open Spends and check.",
                    profit.double_counted.to_plain_string()
                ));
            }
            notes.push(
                "It excludes anything nobody recorded. Where a material has never been \
                 counted, the food cost is what your recipes say rather than what the \
                 shelf says."
                    .to_owned(),
            );
            (columns, rows, None, None)
        }
    };

    Ok(ReportView {
        id: entry.id.to_owned(),
        title: entry.title.to_owned(),
        subtitle: period_words(period),
        columns,
        rows,
        totals,
        compare: compare_view,
        notes,
    })
}

/// `8:20` — a duration a shopkeeper reads, not a number of milliseconds.
#[allow(
    clippy::integer_division,
    reason = "a clock, not money — the same note logging.rs carries"
)]
fn minutes_and_seconds(millis: i64) -> String {
    let total = millis.max(0) / 1_000;
    format!("{}:{:02}", total / 60, total % 60)
}

/// "64.8%" â a margin to one decimal place, from basis points and with no float in sight.
#[allow(
    clippy::integer_division,
    reason = "basis points to a percent and one decimal, by remainder; a float \n              here would be the only one in the money path"
)]
fn margin_words(bp: i64) -> String {
    format!("{}.{}%", bp / 100, (bp.abs() % 100) / 10)
}

/// "up 46%", "down 8%", or nothing at all when there is no average to compare with — a
/// percentage of nothing is not a number.
#[allow(
    clippy::integer_division,
    reason = "basis points to whole percent for a screen; the remainder is \
              noise a shopkeeper does not read"
)]
fn change_words(bp: Option<i64>) -> String {
    match bp {
        None => String::new(),
        Some(n) if n.abs() < 100 => "about the same".to_owned(),
        Some(n) if n > 0 => format!("up {}%", n / 100),
        Some(n) => format!("down {}%", -n / 100),
    }
}

/// The B2CS rows, the taxable total, and what had to be said.
#[allow(
    clippy::integer_division,
    reason = "basis points to whole percent; GST has no fractional-percent rate"
)]
/// `500` → `"5"`, `250` → `"2.5"`.
fn rate_percent(bp: i64) -> String {
    u32::try_from(bp)
        .ok()
        .and_then(mb_core::TaxRate::from_basis_points)
        .map_or_else(String::new, |r| r.label().trim_end_matches('%').to_owned())
}

fn b2cs(
    buckets: &[TaxBucket],
    profile: Option<&StoreProfile>,
) -> (Vec<Vec<String>>, Money, Vec<String>) {
    let mut notes = Vec::new();
    let registration = profile.map_or(mb_core::Registration::Regular, |p| {
        crate::settings::registration_from(&p.registration)
    });
    // Only a regular taxpayer files a GSTR-1. An empty table with a plain sentence is the right
    // answer for the other two.
    if !registration.charges_gst() {
        notes.push(
            if matches!(registration, mb_core::Registration::Composition) {
                "You are under the composition scheme, so you file CMP-08 every \
                 quarter — not GSTR-1. There is nothing to put in this table."
            } else {
                "You have no GST registration, so you file no GST return. There \
                 is nothing to put in this table."
            }
            .to_owned(),
        );
        return (Vec::new(), Money::ZERO, notes);
    }

    let code = profile.and_then(|p| p.state_code.as_deref()).unwrap_or("");
    let place = crate::settings::catalog::state_label(code)
        .map_or_else(String::new, |name| format!("{code}-{name}"));
    if place.is_empty() {
        notes.push(
            "Your state is not set, so the Place Of Supply column is empty and \
             the offline tool will reject the file. Set it in Settings → Shop."
                .to_owned(),
        );
    }
    if profile.is_none_or(|p| p.gstin.as_deref().unwrap_or("").trim().is_empty()) {
        notes.push(
            "Your GSTIN is empty. The offline tool needs it before it will take \
             this file. Set it in Settings → Shop."
                .to_owned(),
        );
    }

    let mut taxable = Money::ZERO;
    let rows = buckets
        .iter()
        .filter(|b| b.tax_kind == "gst" && b.rate_bp > 0)
        .map(|b| {
            taxable = taxable.add(b.taxable).unwrap_or(taxable);
            vec![
                "OE".to_owned(),
                place.clone(),
                // The rate as the tool wants it: a bare number, 2.5 included.
                rate_percent(b.rate_bp),
                String::new(),
                b.taxable.to_plain_string(),
                "0".to_owned(),
                String::new(),
            ]
        })
        .collect();

    notes.push(
        "Liquor is outside GST and is not in this table — it goes in your state \
         VAT return."
            .to_owned(),
    );
    if buckets.iter().any(|b| {
        matches!(b.tax_kind.as_str(), "exempt" | "untaxed")
            || (b.tax_kind == "gst" && b.rate_bp == 0)
    }) {
        notes.push(
            "Sales with no GST on them — exempt and nil-rated — are not in this \
             table either. B2CS is only for taxed sales."
                .to_owned(),
        );
    }
    (rows, taxable, notes)
}

/// A property of the shop.
fn app_registration(repos: &mb_db::Repos<'_>) -> Result<mb_core::Registration, mb_db::DbError> {
    Ok(repos
        .settings()
        .store_profile(OUTLET)?
        .map_or(mb_core::Registration::Regular, |p| {
            crate::settings::Store::from_profile(&p).registration()
        }))
}

const fn label_for(by: SalesBy) -> &'static str {
    match by {
        SalesBy::Day => "Day",
        SalesBy::Hour => "Hour",
        SalesBy::OrderType => "Order type",
        SalesBy::PaymentMode => "Paid by",
        SalesBy::Cashier => "Cashier",
        SalesBy::Section => "Section",
        SalesBy::Terminal => "Till",
        SalesBy::Item => "Item",
        SalesBy::Category => "Category",
    }
}

/// A rate, in the words the bill prints — including the two that are not rates.
fn rate_words(rate_bp: i64, tax_kind: &str) -> String {
    match tax_kind {
        "outside_gst" => "Outside GST (liquor)".to_owned(),
        "exempt" => "Exempt".to_owned(),
        // The same label the bill prints — 2.5% stays 2.5%, never "2%".
        _ => u32::try_from(rate_bp)
            .ok()
            .and_then(mb_core::TaxRate::from_basis_points)
            .map_or_else(|| format!("{rate_bp} bp"), |r| r.label()),
    }
}

const fn control_words(kind: &str) -> &str {
    match kind.as_bytes() {
        b"void" => "Bill voided",
        b"cancel" => "Order cancelled",
        b"refund" => "Refunded",
        b"reprint" => "Reprinted",
        b"discount" => "Discount given",
        _ => "Correction",
    }
}

/// The list, filtered to what this person may open.
pub fn list_on(app: &App) -> UiResult<ReportListView> {
    let who = guard::require(app, Permission::ReportsView)?;
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    Ok(ReportListView {
        periods: choices(crate::flows::today(crate::flows::now())),
        reports: CATALOGUE
            .iter()
            .filter(|entry| who.must(entry.needs).is_ok())
            .map(|entry| ReportEntryView {
                id: entry.id.to_owned(),
                title: entry.title.to_owned(),
                group: entry.group.to_owned(),
            })
            .collect(),
    })
}

/// One CSV writer.
#[must_use]
pub fn csv_of(report: &ReportView) -> String {
    let mut out = String::new();
    mb_db::export::write_row(
        &mut out,
        report.columns.iter().map(|c| Some(c.header.as_str())),
    );
    for row in &report.rows {
        mb_db::export::write_row(&mut out, row.iter().map(|cell| Some(cell.as_str())));
    }
    if let Some(totals) = &report.totals {
        mb_db::export::write_row(&mut out, totals.iter().map(|cell| Some(cell.as_str())));
    }
    out
}

// Today, at a glance — and the list of things that need somebody.

/// The first thing an owner sees, and the reason it is first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DashboardView {
    /// "Today, so far.
    pub title: String,
    pub stats: Vec<StatView>,
    pub compare: Option<CompareView>,
    /// Things that need a person.
    pub attention: Vec<AttentionView>,
    pub quiet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct StatView {
    pub label: String,
    /// Already formatted, always. Even the counts.
    pub value: String,
    pub note: String,
}

/// One thing that needs somebody.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AttentionView {
    /// `danger`, `warn` or `info`.
    pub tone: String,
    pub title: String,
    /// A whole sentence saying what to do about it.
    pub detail: String,
}

fn needs_you(tone: &str, title: &str, detail: String) -> AttentionView {
    AttentionView {
        tone: tone.to_owned(),
        title: title.to_owned(),
        detail,
    }
}

/// Today's figures, and everything that is waiting for a person.
pub fn dashboard_on(app: &App) -> UiResult<DashboardView> {
    let who = guard::require(app, Permission::ReportsView)?;
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    let day = crate::flows::today(crate::flows::now());
    let period = Period::one_day(day);
    let yesterday = day.previous();

    let (totals, position, closed_yesterday) = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                Ok((
                    repos.corrections().day_totals(OUTLET, day)?,
                    repos.money().cash_position(OUTLET, day)?,
                    repos.corrections().day_is_locked(OUTLET, yesterday)?,
                ))
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // The average bill, computed here because it is money divided by a count and TypeScript may
    // do neither.
    let average = if totals.bills > 0 {
        Money::from_paise(totals.net.paise().saturating_div(totals.bills))
    } else {
        Money::ZERO
    };

    let mut attention = Vec::new();

    // Yesterday, never closed. The one that compounds: a shop that skips a close never notices
    // the day it was ₹2,000 short.
    if !closed_yesterday {
        attention.push(needs_you(
            "warn",
            "Yesterday was never closed",
            format!(
                "{yesterday} has no closing count. Closing it records what was \
                 in the drawer, and locks the day so its bills cannot change."
            ),
        ));
    }

    // Paper that did not come out.
    let parked = app.with_shop(|shop| {
        Ok(shop
            .queue
            .snapshot()
            .iter()
            .filter(|job| job.state == mb_print::queue::JobState::Parked)
            .count())
    })?;
    if parked > 0 {
        attention.push(needs_you(
            "danger",
            "Something did not print",
            format!(
                "{} gave up after retrying. Open the print queue from the bar \
                 at the top and either try again or dismiss them.",
                words::count(i64::try_from(parked).unwrap_or(i64::MAX), "job", "jobs")
            ),
        ));
    }

    // The backup, in the backup screen's own words.
    if who.must(Permission::BackupRun).is_ok()
        && let Ok(backup) = crate::settings::backup::status_on(app)
        && backup.tone != "ok"
    {
        attention.push(needs_you(&backup.tone, "Backup", backup.headline));
    }

    // Money owed, and money the shop owes.
    if who.must(Permission::CustomersManage).is_ok()
        && let Ok(owing) = crate::credit::who_owes_on(app)
    {
        let old: Vec<_> = owing
            .iter()
            .filter(|c| {
                c.oldest
                    .split(' ')
                    .next()
                    .and_then(|n| n.parse::<i64>().ok())
                    .is_some_and(|d| d > 30)
            })
            .collect();
        if !old.is_empty() {
            attention.push(needs_you(
                "warn",
                "Somebody has owed you for over a month",
                format!(
                    "{}, the oldest {}. Open Credit to see who.",
                    words::count(
                        i64::try_from(old.len()).unwrap_or(i64::MAX),
                        "customer",
                        "customers"
                    ),
                    old.iter().map(|c| c.oldest.as_str()).max().unwrap_or("—")
                ),
            ));
        }
    }
    if who.must(Permission::ExpensesManage).is_ok()
        && let Ok(spends) = crate::expenses::expenses_on(app)
        && !spends.due.is_empty()
    {
        attention.push(needs_you(
            "info",
            "Regular payments are due",
            format!(
                "{} waiting — rent, salary, the internet bill. Nothing posts \
                 itself; open Spends to confirm them.",
                words::count(
                    i64::try_from(spends.due.len()).unwrap_or(i64::MAX),
                    "reminder",
                    "reminders"
                )
            ),
        ));
    }

    // Suppliers who have been waiting, and a store nobody has ever counted.
    if who.must(Permission::PurchasesManage).is_ok()
        && let Ok(buying) = crate::buying::buying_on(app, None)
    {
        for line in buying.attention {
            attention.push(needs_you("warn", "Buying", line));
        }
    }

    let margin = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                if !repos.stock().has_any_recipe(OUTLET)? {
                    return Ok(None);
                }
                Ok(Some(repos.reports().profit(OUTLET, period)?))
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // The comparison against yesterday, through the same report the screen would run.
    let compare_view = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let reports = mb_db::Repos::new(tx).reports();
                let sum = |p: Period| -> Result<Money, mb_db::DbError> {
                    Ok(reports
                        .sales_by(OUTLET, p, SalesBy::Day)?
                        .iter()
                        .try_fold(Money::ZERO, |acc, b| acc.add(b.gross))
                        .unwrap_or(Money::ZERO))
                };
                Ok(compare(
                    sum(period)?,
                    sum(period.previous())?,
                    period.previous(),
                ))
            })
            .map_err(|e| words::from_db(&e))
    })?;

    Ok(DashboardView {
        title: format!("Today, so far — {day}"),
        stats: vec![
            StatView {
                label: "Takings".to_owned(),
                value: totals.net.to_plain_string(),
                note: words::count(totals.bills, "bill", "bills"),
            },
            StatView {
                label: "Average bill".to_owned(),
                value: average.to_plain_string(),
                note: if totals.bills > 0 {
                    String::new()
                } else {
                    "Nothing sold yet today.".to_owned()
                },
            },
            StatView {
                label: "In the drawer".to_owned(),
                value: position.expected.to_plain_string(),
                note: "What the till expects, before counting.".to_owned(),
            },
            StatView {
                label: "Voided".to_owned(),
                value: totals.voids.to_plain_string(),
                note: words::count(totals.voided_bills, "bill", "bills"),
            },
            StatView {
                label: "Gross margin".to_owned(),
                value: match margin {
                    Some(profit) => profit.gross_margin.to_plain_string(),
                    None => "—".to_owned(),
                },
                note: match margin.and_then(|p| p.margin_bp().map(|bp| (p, bp))) {
                    Some((_, bp)) => format!("{} of what you sold", margin_words(bp)),
                    None => "Add recipes to your dishes and this fills in.".to_owned(),
                },
            },
        ],
        compare: Some(compare_view),
        quiet: if attention.is_empty() {
            "Nothing needs you. The backup is current, everything printed, and \
             yesterday is closed."
                .to_owned()
        } else {
            String::new()
        },
        attention,
    })
}

#[tauri::command]
pub fn dashboard(app: tauri::State<'_, App>) -> UiResult<DashboardView> {
    dashboard_on(&app)
}

// Onto paper, and onto disk.

/// The report as a printable document — the same document model the bill uses, so a report goes
/// to a PDF, a thermal printer or the on-screen preview without a second layout engine
/// existing.
#[must_use]
pub fn to_document(report: &ReportView) -> mb_print::doc::Document {
    use mb_print::doc::{Align, Block, Column, Document, Pattern, Style};
    use mb_print::paper::{Paper, PaperKind};

    let mut doc = Document::new(Paper::new(PaperKind::A4));
    doc.text(report.title.clone(), Style::new(2, true), Align::Centre)
        .text(report.subtitle.clone(), Style::NORMAL, Align::Centre)
        .separator(Pattern::Double);

    // Column widths from the widest cell, header included.
    let widest = |index: usize| {
        report
            .rows
            .iter()
            .chain(report.totals.iter())
            .filter_map(|row| row.get(index))
            .map(|cell| cell.chars().count())
            .chain(std::iter::once(
                report.columns[index].header.chars().count(),
            ))
            .max()
            .unwrap_or(1)
    };
    let mut filled = false;
    let columns: Vec<Column> = report
        .columns
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            if spec.numeric {
                Column::fixed(widest(index) + 2, Align::Right)
            } else if filled {
                Column::fixed(widest(index) + 2, Align::Left)
            } else {
                filled = true;
                Column::fill(Align::Left)
            }
        })
        .collect();

    doc.push(Block::Columns {
        columns: columns.clone(),
        rows: vec![report.columns.iter().map(|c| c.header.clone()).collect()],
        style: Style::new(1, true),
    })
    .separator(Pattern::Solid)
    .push(Block::Columns {
        columns: columns.clone(),
        rows: report.rows.clone(),
        style: Style::NORMAL,
    });

    if let Some(totals) = &report.totals {
        doc.separator(Pattern::Solid).push(Block::Columns {
            columns,
            rows: vec![totals.clone()],
            style: Style::new(1, true),
        });
    }
    for note in &report.notes {
        doc.spacer(1).line(note.clone());
    }
    doc
}

/// Where an export lands, and what to tell the person who asked for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SavedFileView {
    pub path: String,
    /// The whole sentence, assembled here (§6).
    pub message: String,
}

pub(crate) fn documents_folder(under: &str) -> std::path::PathBuf {
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let documents = std::path::PathBuf::from(profile).join("Documents");
        if documents.is_dir() {
            return documents.join(under);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return std::path::PathBuf::from(home).join(under);
    }
    crate::config::AppConfig::directory().join("exports")
}

pub(crate) fn export_folder() -> std::path::PathBuf {
    documents_folder("Magic Bill reports")
}

/// Write a file into the shop's exports folder and say where it went.
pub(crate) fn save(name: &str, bytes: &[u8]) -> UiResult<SavedFileView> {
    let folder = export_folder();
    std::fs::create_dir_all(&folder).map_err(|e| {
        UiError::new(
            "report.save",
            "The reports folder could not be created. Check the disk has room.",
        )
        .with_detail(e.to_string())
    })?;
    let path = folder.join(name);
    std::fs::write(&path, bytes).map_err(|e| {
        UiError::new(
            "report.save",
            "The file could not be written. Check the disk has room.",
        )
        .with_detail(e.to_string())
    })?;
    Ok(SavedFileView {
        message: format!("Saved as {name}, in your Documents folder under \"Magic Bill reports\"."),
        path: path.display().to_string(),
    })
}

/// A file name a person can find again: what it is, and what it covers.
fn file_name(report: &ReportView, extension: &str) -> String {
    let clean: String = report
        .title
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let period: String = report
        .subtitle
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    format!(
        "{}-{}.{extension}",
        clean.trim_matches('-'),
        period.trim_matches('-')
    )
}

// The seats.

#[tauri::command]
pub fn report_list(app: tauri::State<'_, App>) -> UiResult<ReportListView> {
    list_on(&app)
}

#[tauri::command]
pub fn report(app: tauri::State<'_, App>, id: String, period: PeriodArg) -> UiResult<ReportView> {
    report_on(&app, id, period)
}

#[tauri::command]
pub fn report_csv(
    app: tauri::State<'_, App>,
    id: String,
    period: PeriodArg,
) -> UiResult<SavedFileView> {
    let report = report_on(&app, id, period)?;
    let name = file_name(&report, "csv");
    save(&name, csv_of(&report).as_bytes())
}

#[tauri::command]
pub fn report_pdf(
    app: tauri::State<'_, App>,
    id: String,
    period: PeriodArg,
) -> UiResult<SavedFileView> {
    let report = report_on(&app, id, period)?;
    let laid = mb_print::layout::layout(&to_document(&report)).map_err(|e| {
        UiError::new(
            "report.layout",
            "The report could not be laid out for printing.",
        )
        .with_detail(e.to_string())
    })?;
    let name = file_name(&report, "pdf");
    save(&name, &mb_print::pdf::to_pdf(&laid))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every report in the list exists, and every report is in the list.
    #[test]
    fn the_catalogue_has_no_gaps_and_no_ghosts() {
        let mut seen = std::collections::BTreeSet::new();
        for entry in CATALOGUE {
            assert!(seen.insert(entry.id), "{} is listed twice", entry.id);
            assert!(find(entry.id).is_some());
            assert!(!entry.title.is_empty());
            assert!(
                entry.title.chars().next().is_some_and(char::is_uppercase),
                "{}'s title should read like a sentence",
                entry.id
            );
        }
        // Every `SalesBy` is offered.
        for by in [
            SalesBy::Day,
            SalesBy::Hour,
            SalesBy::OrderType,
            SalesBy::PaymentMode,
            SalesBy::Cashier,
            SalesBy::Section,
            SalesBy::Terminal,
            SalesBy::Item,
            SalesBy::Category,
        ] {
            assert!(
                CATALOGUE
                    .iter()
                    .any(|e| matches!(e.kind, Kind::Sales(b) if b == by)),
                "{by:?} is not on the report list"
            );
        }
    }

    #[test]
    fn the_comparison_is_one_sentence_and_says_what_it_compared_against() {
        let previous = Period::new(
            BusinessDay::from_ymd(2026, 7, 1),
            BusinessDay::from_ymd(2026, 7, 31),
        );
        let up = compare(
            Money::from_paise(108_000),
            Money::from_paise(100_000),
            previous,
        );
        assert_eq!(up.direction, "up");
        assert!(up.summary.contains("Up 8%"), "{}", up.summary);
        assert!(up.summary.contains("31 days"), "{}", up.summary);
        // And it names the period, so a person can check what it compared against rather than
        // trusting the percentage.
        assert!(up.period.contains("2026-07-01"), "{}", up.period);

        let down = compare(
            Money::from_paise(90_000),
            Money::from_paise(100_000),
            previous,
        );
        assert_eq!(down.direction, "down");
        assert!(down.summary.contains("Down 10%"), "{}", down.summary);

        // Nothing before: a percentage of zero is not a number, and saying "up infinity%" is
        // worse than saying what happened.
        let fresh = compare(Money::from_paise(5_000), Money::ZERO, previous);
        assert!(
            fresh.summary.contains("Nothing at all"),
            "{}",
            fresh.summary
        );
    }

    /// Found by looking at it.
    #[test]
    fn one_day_is_not_one_days() {
        let yesterday = Period::one_day(BusinessDay::from_ymd(2026, 8, 8));
        let fresh = compare(Money::from_paise(12_600), Money::ZERO, yesterday);
        assert_eq!(fresh.summary, "Nothing at all the day before.");

        let up = compare(
            Money::from_paise(10_800),
            Money::from_paise(10_000),
            yesterday,
        );
        assert_eq!(up.summary, "Up 8% on the day before (100.00).");
        assert!(!up.summary.contains("1 days"), "{}", up.summary);
    }

    /// B2CS is the taxable-supply table, so liquor must never reach it — its VAT is a state
    /// return, not GSTR-1.
    #[test]
    fn b2cs_takes_only_taxed_gst_and_a_composition_shop_gets_none() {
        let bucket = |rate_bp, kind: &str, paise| TaxBucket {
            rate_bp,
            tax_kind: kind.to_owned(),
            taxable: Money::from_paise(paise),
            cgst: Money::ZERO,
            sgst: Money::ZERO,
            igst: Money::ZERO,
            vat: Money::ZERO,
        };
        let shop = |registration: &str| StoreProfile {
            name: "Shop".to_owned(),
            address: String::new(),
            phone: None,
            gstin: Some("29ABCDE1234F1Z5".to_owned()),
            fssai: None,
            state_code: Some("29".to_owned()),
            upi_id: None,
            upi_merchant_name: None,
            upi_reference: None,
            registration: registration.to_owned(),
            price_basis: mb_core::PriceBasis::Exclusive,
        };
        let buckets = [
            // A half-percent rate, because integer division printed it as "2".
            bucket(250, "gst", 40_000),
            bucket(500, "gst", 100_000),
            // Liquor, at the state VAT rate it really carries.
            bucket(2000, "outside_gst", 250_000),
        ];

        let (rows, taxable, _) = b2cs(&buckets, Some(&shop("regular")));
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(
            rows[0],
            vec!["OE", "29-Karnataka", "2.5", "", "400.00", "0", ""]
        );
        assert_eq!(
            rows[1],
            vec!["OE", "29-Karnataka", "5", "", "1000.00", "0", ""]
        );
        assert_eq!(taxable, Money::from_paise(140_000));
        // The liquor never reaches the sheet — neither its word nor its figure.
        let cells = rows.concat().join(" ").to_lowercase();
        assert!(
            !cells.contains("liquor") && !cells.contains("2500"),
            "{cells}"
        );

        let (none, _, notes) = b2cs(&buckets, Some(&shop("composition")));
        assert!(none.is_empty());
        assert!(notes.iter().any(|n| n.contains("CMP-08")), "{notes:?}");
    }

    #[test]
    fn a_comma_in_an_item_name_does_not_break_its_row() {
        let mut out = String::new();
        mb_db::export::write_row(
            &mut out,
            ["Biryani, \"Half\"", "2", "240.00"]
                .iter()
                .map(|c| Some(*c)),
        );
        // Quoted, with the inner quotes doubled — RFC 4180, and Excel.
        assert_eq!(out, "\"Biryani, \"\"Half\"\"\",2,240.00\r\n");
        // And the row still has three fields, which is the whole point.
        assert_eq!(out.matches("\r\n").count(), 1);
    }
}
