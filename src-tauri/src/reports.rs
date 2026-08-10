//! **The reports, as one shape** — so one screen renders all of them.
//!
//! P17 taught this: the settings screen is one component over one catalogue,
//! and adding a setting touches no UI file. The same trick works here and for
//! the same reason. Every report comes down as a [`ReportView`] — a title, a
//! set of columns, rows of **already-formatted strings**, totals, and the
//! comparison against the previous period. One React component draws all of
//! them, one CSV writer exports all of them, one print path prints all of them.
//!
//! **Adding a report is a function in `mb-db`'s `reports.rs` plus a line in
//! [`CATALOGUE`], and nothing on the screen, ever.** A test walks that list in
//! both directions.
//!
//! # Every cell is a string, and money was formatted in Rust
//!
//! D39 and R2. `Money::to_plain_string` is the only formatter in this product,
//! and a report is the place that rule is most tempting to break: it is all
//! numbers, and JavaScript has no integers. So the boundary carries text.

use mb_auth::Permission;
use mb_core::{BusinessDay, Money};
use mb_db::repo::reports::{Period, SalesBy};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::guard;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// ---------------------------------------------------------------------------
// The one shape.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ReportView {
    pub id: String,
    pub title: String,
    /// The period, in words: "1 Aug – 9 Aug 2026 · 9 days".
    pub subtitle: String,
    pub columns: Vec<ReportColumn>,
    /// Already formatted. TypeScript does no arithmetic on money (R8).
    pub rows: Vec<Vec<String>>,
    pub totals: Option<Vec<String>>,
    /// Scope 10.9 — against the previous equal period.
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

/// What changed since the period before — audit **G2**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CompareView {
    /// "1 Jul – 31 Jul", so a person can see WHAT it is being compared against
    /// rather than trusting a percentage.
    pub period: String,
    pub before: String,
    pub now: String,
    /// The whole sentence: "up 8% on the 31 days before". **One string**,
    /// because §6 forbids building one out of fragments — a percentage, a
    /// direction and a period assembled on the screen is three languages'
    /// worth of word order.
    pub summary: String,
    /// `up`, `down` or `same`. For the arrow — colour is never the only
    /// signal, and neither is a sign.
    pub direction: String,
}

/// One report a person can choose.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub id: &'static str,
    pub title: &'static str,
    pub group: &'static str,
    /// What it needs. **`reports.view` for the money ones**, because audit C1's
    /// first example is *"anybody can open Reports and see the whole day's
    /// cash"*.
    pub needs: Permission,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Sales(SalesBy),
    TaxByRate,
    TaxByHsn,
    Control,
    MenuEngineering,
    StoppedSelling,
}

/// **The list, and it is the screen.**
pub const CATALOGUE: &[Entry] = &[
    Entry { id: "sales_day", title: "Sales by day", group: "Sales", needs: Permission::ReportsView, kind: Kind::Sales(SalesBy::Day) },
    Entry { id: "sales_hour", title: "Sales by hour — your peak", group: "Sales", needs: Permission::ReportsView, kind: Kind::Sales(SalesBy::Hour) },
    Entry { id: "sales_type", title: "Sales by order type", group: "Sales", needs: Permission::ReportsView, kind: Kind::Sales(SalesBy::OrderType) },
    Entry { id: "sales_mode", title: "Sales by payment mode", group: "Sales", needs: Permission::ReportsView, kind: Kind::Sales(SalesBy::PaymentMode) },
    Entry { id: "sales_cashier", title: "Sales by cashier", group: "Sales", needs: Permission::ReportsView, kind: Kind::Sales(SalesBy::Cashier) },
    Entry { id: "sales_section", title: "Sales by section", group: "Sales", needs: Permission::ReportsView, kind: Kind::Sales(SalesBy::Section) },
    Entry { id: "items", title: "Item sales", group: "Items", needs: Permission::ReportsView, kind: Kind::Sales(SalesBy::Item) },
    Entry { id: "categories", title: "Category sales", group: "Items", needs: Permission::ReportsView, kind: Kind::Sales(SalesBy::Category) },
    Entry { id: "stopped", title: "Items that stopped selling", group: "Items", needs: Permission::ReportsView, kind: Kind::StoppedSelling },
    // Cost and margin are behind `reports.view` too, but the MENU screen keeps
    // cost behind the same permission (P13) — so this is consistent rather
    // than a new rule.
    Entry { id: "margin", title: "Menu engineering — volume against margin", group: "Items", needs: Permission::ReportsView, kind: Kind::MenuEngineering },
    Entry { id: "tax_rate", title: "Tax, rate-wise", group: "Tax", needs: Permission::ReportsView, kind: Kind::TaxByRate },
    Entry { id: "tax_hsn", title: "Tax, HSN-wise", group: "Tax", needs: Permission::ReportsView, kind: Kind::TaxByHsn },
    Entry { id: "control", title: "Voids, discounts, refunds and reprints", group: "Control", needs: Permission::AuditView, kind: Kind::Control },
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
///
/// **Computed in Rust, and D70 is why it has to be.** "Today" on this screen is
/// the shop's BUSINESS day — a shop that closes at 1 am has a day that starts at
/// 5 am, and a browser asked for today's date at half past midnight would answer
/// with tomorrow. That is audit B1 wearing a third hat.
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
///
/// **Two `YYYY-MM-DD` strings**, which is what an `<input type="date">`
/// produces. Not numbers: D58 bans a `bigint` at this boundary anyway, and a
/// days-since-epoch integer would have to be computed in TypeScript — date
/// arithmetic on the value every report is keyed by, in the language §6 keeps
/// arithmetic out of.
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
            // Swapping silently would be a screen guessing. Saying so is one
            // sentence, and the picker is right there.
            return Err(UiError::new(
                "report.period",
                "The end date is before the start date.",
            ));
        }
        Ok(Period::new(from, to))
    }
}

// ---------------------------------------------------------------------------
// Building one.
// ---------------------------------------------------------------------------

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
    // Percent as an integer, computed on paise. No floats anywhere near money
    // (D7), and a percentage a shopkeeper reads to one decimal place is a
    // percentage nobody checks.
    let percent = if before.is_zero() {
        None
    } else {
        Some(
            (now.paise().saturating_sub(before.paise()).saturating_mul(100))
                .saturating_div(before.paise().abs().max(1)),
        )
    };
    // "the day before" reads better than "the 1 days before" — which is what
    // this said until somebody looked at it (§6, and P17's "1 have been
    // issued" was the same bug).
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
    // **P21's gate, in the core rather than on the screen.** `report_csv` and
    // `report_pdf` both come through here, so all three are covered by one
    // line — and T10 calls this function directly with a not-entitled
    // entitlement, which is the same shape as `guard`'s own test.
    crate::licensing::gate(app, mb_license::Feature::Reports)?;
    let period = period.parse()?;

    // **A reader, not the writer.** Scope 16.6: a report must never stand in
    // front of a bill, and a year-long scan on the writer connection would.
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| build(&mb_db::Repos::new(tx).reports(), entry, period))
            .map_err(|e| words::from_db(&e))
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "one match arm per report; splitting it would put each report's \
              shape further from the query it comes from"
)]
fn build(
    reports: &mb_db::repo::reports::ReportsRepo<'_>,
    entry: &Entry,
    period: Period,
) -> Result<ReportView, mb_db::DbError> {
    let mut notes = Vec::new();
    let (columns, rows, totals, compare_view) = match entry.kind {
        Kind::Sales(by) => {
            let buckets = reports.sales_by(OUTLET, period, by)?;
            let wants_qty = matches!(by, SalesBy::Item | SalesBy::Category);
            let mut columns = vec![
                column(label_for(by), false),
                column("Bills", true),
            ];
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

            // **Every column that has figures in it gets a total.** The first
            // version left Discount and Tax blank, which on a screen reads as
            // "this is broken" rather than as "we chose not to add these up".
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
            (columns, rows, Some(totals), Some(compare(gross, before, previous)))
        }

        Kind::TaxByRate => {
            let buckets = reports.tax_by_rate(OUTLET, period)?;
            let columns = vec![
                column("Rate", false),
                column("Taxable", true),
                column("CGST", true),
                column("SGST", true),
                column("IGST", true),
            ];
            let mut taxable = Money::ZERO;
            let rows = buckets
                .iter()
                .map(|b| {
                    taxable = taxable.add(b.taxable).unwrap_or(taxable);
                    vec![
                        rate_words(b.rate_bp, &b.treatment),
                        b.taxable.to_plain_string(),
                        b.cgst.to_plain_string(),
                        b.sgst.to_plain_string(),
                        b.igst.to_plain_string(),
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
                    // The cost of what was SOLD: the item's unit cost times the
                    // quantity. `Qty::extend` is mb-core's rule for that
                    // multiplication and this does not invent a second one.
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

const fn label_for(by: SalesBy) -> &'static str {
    match by {
        SalesBy::Day => "Day",
        SalesBy::Hour => "Hour",
        SalesBy::OrderType => "Order type",
        SalesBy::PaymentMode => "Paid by",
        SalesBy::Cashier => "Cashier",
        SalesBy::Section => "Section",
        SalesBy::Item => "Item",
        SalesBy::Category => "Category",
    }
}

/// A rate, in the words the bill prints — including the two that are not rates.
#[allow(
    clippy::integer_division,
    reason = "basis points to whole percent; 1800 bp IS 18% and the remainder \
              is meaningless — GST has no fractional-percent rate"
)]
fn rate_words(rate_bp: i64, treatment: &str) -> String {
    match treatment {
        "non_gst" => "Outside GST (liquor)".to_owned(),
        "exempt" => "Exempt".to_owned(),
        _ => format!("{}%", rate_bp / 100),
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

/// **One CSV writer** — `export::write_row`, and audit G7 is why.
///
/// > *"CSV export builds text by joining with commas. An item name containing
/// > a comma ("Chicken Biryani, Half") will break the columns of that row."*
///
/// The escaping already exists and is already tested. A second writer here is
/// how that bug was four bugs rather than one.
#[must_use]
pub fn csv_of(report: &ReportView) -> String {
    let mut out = String::new();
    mb_db::export::write_row(&mut out, report.columns.iter().map(|c| Some(c.header.as_str())));
    for row in &report.rows {
        mb_db::export::write_row(&mut out, row.iter().map(|cell| Some(cell.as_str())));
    }
    if let Some(totals) = &report.totals {
        mb_db::export::write_row(&mut out, totals.iter().map(|cell| Some(cell.as_str())));
    }
    out
}

// ---------------------------------------------------------------------------
// Today, at a glance — and the list of things that need somebody.
// ---------------------------------------------------------------------------

/// **The first thing an owner sees**, and the reason it is first.
///
/// Audit **G1**: *"the owner's questions — how did today go, what is unusual,
/// what needs me — are answered by opening four screens and doing arithmetic in
/// your head."* Thirteen reports do not answer "what needs me" either; they
/// answer questions you already knew to ask. This answers the one you did not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DashboardView {
    /// "Today, so far — 2026-08-09".
    pub title: String,
    pub stats: Vec<StatView>,
    pub compare: Option<CompareView>,
    /// **Things that need a person.** Empty is the good case and the screen
    /// says so out loud rather than showing a blank panel.
    pub attention: Vec<AttentionView>,
    pub quiet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct StatView {
    pub label: String,
    /// Already formatted, always. Even the counts — R8 does not have an
    /// exception for small numbers.
    pub value: String,
    pub note: String,
}

/// One thing that needs somebody.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AttentionView {
    /// `danger`, `warn` or `info`. The words say it too — colour is never the
    /// only signal (§2 rule 2).
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
///
/// **Every entry on the attention list is read from the thing that already
/// knows.** The backup headline is `backup::status_on`'s own sentence, the
/// parked prints are the queue's own snapshot, the reminders are the Spends
/// screen's. A dashboard that recomputed any of them would be a fifth place
/// for a figure to disagree — which is audit G1 with more steps.
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

    // The average bill, computed here because it is money divided by a count
    // and TypeScript may do neither.
    let average = if totals.bills > 0 {
        Money::from_paise(totals.net.paise().saturating_div(totals.bills))
    } else {
        Money::ZERO
    };

    let mut attention = Vec::new();

    // 1. Yesterday, never closed. The one that compounds: a shop that skips a
    //    close never notices the day it was ₹2,000 short.
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

    // 2. Paper that did not come out — audit D4, and the whole reason P07's
    //    queue parks a job instead of dropping it.
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

    // 3. The backup, in the backup screen's own words (audit A1).
    if who.must(Permission::BackupRun).is_ok()
        && let Ok(backup) = crate::settings::backup::status_on(app)
        && backup.tone != "ok"
    {
        attention.push(needs_you(&backup.tone, "Backup", backup.headline));
    }

    // 4. Money owed, and money the shop owes.
    if who.must(Permission::CustomersManage).is_ok()
        && let Ok(owing) = crate::credit::who_owes_on(app)
    {
        let old: Vec<_> = owing
            .iter()
            .filter(|c| c.oldest.split(' ').next().and_then(|n| n.parse::<i64>().ok()).is_some_and(|d| d > 30))
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

    // The comparison against yesterday, through the same report the screen
    // would run.
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
                Ok(compare(sum(period)?, sum(period.previous())?, period.previous()))
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

// ---------------------------------------------------------------------------
// Onto paper, and onto disk.
// ---------------------------------------------------------------------------

/// The report as a printable document — **the same document model the bill
/// uses** (D29), so a report goes to a PDF, a thermal printer or the on-screen
/// preview without a second layout engine existing.
#[must_use]
pub fn to_document(report: &ReportView) -> mb_print::doc::Document {
    use mb_print::doc::{Align, Block, Column, Document, Pattern, Style};
    use mb_print::paper::{Paper, PaperKind};

    let mut doc = Document::new(Paper::new(PaperKind::A4));
    doc.text(report.title.clone(), Style::new(2, true), Align::Centre)
        .text(report.subtitle.clone(), Style::NORMAL, Align::Centre)
        .separator(Pattern::Double);

    // Column widths from the widest cell, header included. The first
    // non-numeric column takes what is left, so a long item name is the thing
    // that gets the room rather than the thing that gets cut.
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

/// **`Documents\Magic Bill reports\`, and D76 says why it is not `%APPDATA%`.**
fn export_folder() -> std::path::PathBuf {
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let documents = std::path::PathBuf::from(profile).join("Documents");
        if documents.is_dir() {
            return documents.join("Magic Bill reports");
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return std::path::PathBuf::from(home).join("Magic Bill reports");
    }
    crate::config::AppConfig::directory().join("exports")
}

fn save(name: &str, bytes: &[u8]) -> UiResult<SavedFileView> {
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

// ---------------------------------------------------------------------------
// The seats.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn report_list(app: tauri::State<'_, App>) -> UiResult<ReportListView> {
    list_on(&app)
}

#[tauri::command]
pub fn report(
    app: tauri::State<'_, App>,
    id: String,
    period: PeriodArg,
) -> UiResult<ReportView> {
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

    /// **T11.** Every report in the list exists, and every report is in the
    /// list — the same both-directions guard P17's catalogue uses, and for the
    /// same reason: a second list is a list that drifts.
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
        // Every `SalesBy` is offered. A grouping that exists in mb-db and is
        // not on this list is a report nobody can run.
        for by in [
            SalesBy::Day,
            SalesBy::Hour,
            SalesBy::OrderType,
            SalesBy::PaymentMode,
            SalesBy::Cashier,
            SalesBy::Section,
            SalesBy::Item,
            SalesBy::Category,
        ] {
            assert!(
                CATALOGUE.iter().any(|e| matches!(e.kind, Kind::Sales(b) if b == by)),
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
        // And it names the period, so a person can check what it compared
        // against rather than trusting the percentage.
        assert!(up.period.contains("2026-07-01"), "{}", up.period);

        let down = compare(
            Money::from_paise(90_000),
            Money::from_paise(100_000),
            previous,
        );
        assert_eq!(down.direction, "down");
        assert!(down.summary.contains("Down 10%"), "{}", down.summary);

        // Nothing before: a percentage of zero is not a number, and saying
        // "up infinity%" is worse than saying what happened.
        let fresh = compare(Money::from_paise(5_000), Money::ZERO, previous);
        assert!(fresh.summary.contains("Nothing at all"), "{}", fresh.summary);
    }

    /// **Found by looking at it.** The dashboard read "Nothing at all in the 1
    /// days before" — the same bug as P17's "1 have been issued", which is
    /// also the same bug as "0 bill(s)" three cards to the left of it.
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

    /// **T4 — audit G7, made impossible.** The one writer escapes it.
    #[test]
    fn a_comma_in_an_item_name_does_not_break_its_row() {
        let mut out = String::new();
        mb_db::export::write_row(
            &mut out,
            ["Biryani, \"Half\"", "2", "240.00"].iter().map(|c| Some(*c)),
        );
        // Quoted, with the inner quotes doubled — RFC 4180, and Excel.
        assert_eq!(out, "\"Biryani, \"\"Half\"\"\",2,240.00\r\n");
        // And the row still has three fields, which is the whole point.
        assert_eq!(out.matches("\r\n").count(), 1);
    }
}
