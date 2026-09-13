//! The ways a shop takes something back, and the bills list they hang off.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::{AnyOrder, Money, OrderId, Qty, StaffId};
use mb_db::repo::corrections::{Reason, Refund, RevertLine, RevertPayment, RevertRow};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::billing::{BillView, CartLineView, PaymentView};
use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};
use crate::{log_info, log_warn};

/// Above this, a void or a revert needs a second person.
const APPROVAL_KEY: &str = "bill.void.approval_above_paise";

// What the screens see.

/// One bill, as the Bills list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BillRowView {
    pub order_id: String,
    pub number: String,
    pub at: String,
    pub table: Option<String>,
    pub order_type: String,
    pub items: u32,
    pub total: MoneyView,
    /// "Cash", "UPI", "Cash + Card".
    pub paid_by: String,
    pub cashier: Option<String>,
    /// "settled", "voided", "cancelled".
    pub state: String,
    /// The same state in the word a person reads: "Paid", "Voided", "Cancelled". Rust owns
    /// the word, so the badge on the screen and the cell in a saved file never disagree.
    pub state_word: String,
    /// Present on a voided bill, and shown.
    pub void_reason: Option<String>,
    pub refunded: Option<MoneyView>,
    /// How many pieces of paper this bill has produced beyond the first.
    pub reprints: u32,
    /// The bill was taken back to the counter and billed again under this number.
    pub edited: bool,
    /// On an edited bill: "waiting" or "approved".
    pub approval: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ReasonView {
    pub id: String,
    pub text: String,
}

/// The three figures that must tie, for the screen's header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DayTotalsView {
    pub gross: MoneyView,
    pub voids: MoneyView,
    pub net: MoneyView,
    pub refunded: MoneyView,
    pub bills: i64,
    pub voided_bills: i64,
    pub cancelled_orders: i64,
}

/// What the Bills screen asks for. Everything optional; nothing means today, everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BillFilter {
    pub period: Option<crate::reports::PeriodArg>,
    /// Bill number, table, item, amount or name — matched anywhere.
    pub query: Option<String>,
    /// A staff id.
    pub cashier: Option<String>,
    /// "settled", "edited", "voided", "cancelled".
    pub state: Option<String>,
    /// A payment mode label: "Cash", "Card", "UPI", "Credit".
    pub mode: Option<String>,
}

/// The Bills screen, in one value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BillsView {
    pub rows: Vec<BillRowView>,
    /// The figures for what is listed.
    pub totals: DayTotalsView,
    pub periods: Vec<crate::reports::PeriodChoiceView>,
    /// Everyone who took a bill in the period, for the filter.
    pub cashiers: Vec<crate::lan::PersonPick>,
    /// Everyone whose PIN can approve a big void or revert.
    pub approvers: Vec<crate::lan::PersonPick>,
    /// Edits nobody has signed off yet, in the whole shop.
    pub waiting: u32,
    pub can_revert: bool,
    pub can_void: bool,
    pub can_reprint: bool,
    pub can_approve: bool,
}

/// One thing that happened to a bill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct HistoryView {
    pub when: String,
    pub who: String,
    pub what: String,
}

/// One line of a bill as it was before it was taken back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BeforeLineView {
    pub name: String,
    pub qty: String,
    pub unit_price: MoneyView,
    pub amount: MoneyView,
}

/// One time a bill was taken back to the counter, from the register.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RevertView {
    pub id: String,
    pub reason: String,
    pub by: String,
    pub at: String,
    /// The bill as it was: its lines, what it came to, how and when it was paid.
    pub before_lines: Vec<BeforeLineView>,
    pub before_total: MoneyView,
    pub before_paid_by: String,
    pub before_paid_at: String,
    /// What is different since, one line each.
    pub changes: Vec<String>,
    pub approved_by: Option<String>,
    pub approved_at: Option<String>,
}

/// One bill, opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BillDetailView {
    pub row: BillRowView,
    pub taken_by: Option<String>,
    pub covers: Option<u32>,
    pub note: Option<String>,
    pub lines: Vec<CartLineView>,
    /// The totals block; absent on a cancelled order, which has no bill.
    pub bill: Option<BillView>,
    pub payments: Vec<PaymentView>,
    pub tip: MoneyView,
    pub change: MoneyView,
    pub history: Vec<HistoryView>,
    /// Every time this bill was taken back, oldest first.
    pub edits: Vec<RevertView>,
    pub can_approve: bool,
}

// The list.

/// Today's bills, newest first — what a void or a refund hands back.
pub fn list_bills_on(app: &App) -> UiResult<Vec<BillRowView>> {
    Ok(bills_on(app, BillFilter::default())?.rows)
}

/// The Bills screen: the rows that match, and everything the toolbar needs.
pub fn bills_on(app: &App, filter: BillFilter) -> UiResult<BillsView> {
    let who = guard::require(app, Permission::ReportsView)?;
    let day = today(now());
    let period = match &filter.period {
        Some(arg) => arg.parse()?,
        None => mb_db::repo::reports::Period::one_day(day),
    };
    let query = filter
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(str::to_lowercase);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let staff = repos.people().list_staff(OUTLET)?;
                let names = Names::new(&staff);
                let tables = repos.floor().list_tables(OUTLET)?;
                let label_of = |id: &mb_core::TableId| {
                    tables.iter().find(|t| &t.id == id).map(|t| t.label.clone())
                };

                let mut rows = Vec::new();
                let mut cashiers: Vec<crate::lan::PersonPick> = Vec::new();
                for order in repos
                    .orders()
                    .list_between(OUTLET, period.from, period.to)?
                {
                    let Some(row) = bill_row(&order, &names, &label_of) else {
                        continue;
                    };
                    let id = OrderId::new(row.order_id.clone());
                    let row = with_the_register(&repos, &id, row)?;

                    if let Some(by) = taken_by(&order)
                        && !cashiers.iter().any(|c| c.id == by.as_str())
                    {
                        cashiers.push(crate::lan::PersonPick {
                            id: by.as_str().to_owned(),
                            name: names.of(by),
                        });
                    }
                    if !row_matches(&order, &row, &filter, query.as_deref()) {
                        continue;
                    }

                    let reprints = repos.corrections().reprint_count(&id)?;
                    let refunded = repos.corrections().refunded_so_far(&id)?;
                    rows.push(BillRowView {
                        reprints,
                        refunded: refunded.is_positive().then(|| MoneyView::from(refunded)),
                        ..row
                    });
                }
                // Newest first: the bill somebody wants is nearly always the one that just
                // printed.
                rows.reverse();
                cashiers.sort_by(|a, b| a.name.cmp(&b.name));

                let totals = totals_of(&rows);
                let waiting = repos.corrections().reverts_waiting(OUTLET)?;
                let approvers = staff
                    .iter()
                    .filter(|s| s.permissions.has(Permission::BillVoid))
                    .map(|s| crate::lan::PersonPick {
                        id: s.id.as_str().to_owned(),
                        name: s.name.clone(),
                    })
                    .collect();

                Ok(BillsView {
                    rows,
                    totals,
                    periods: crate::reports::choices(day),
                    cashiers,
                    approvers,
                    waiting: crate::ipc::count(waiting),
                    can_revert: who.must(Permission::BillRevert).is_ok(),
                    can_void: who.must(Permission::BillVoid).is_ok(),
                    can_reprint: who.must(Permission::BillReprint).is_ok(),
                    can_approve: who.must(Permission::BillRevertApprove).is_ok(),
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// Staff ids to names.
struct Names<'a>(&'a [mb_db::repo::people::StaffMember]);

impl Names<'_> {
    fn new(staff: &[mb_db::repo::people::StaffMember]) -> Names<'_> {
        Names(staff)
    }

    fn find(&self, id: &StaffId) -> Option<String> {
        self.0.iter().find(|s| s.id == *id).map(|s| s.name.clone())
    }

    /// A name, or the id when the person is gone from the list.
    fn of(&self, id: &StaffId) -> String {
        self.find(id).unwrap_or_else(|| id.as_str().to_owned())
    }

    fn maybe(&self, id: Option<&StaffId>) -> String {
        id.map_or_else(String::new, |id| self.of(id))
    }
}

/// Who took the money, or who cancelled.
fn taken_by(order: &AnyOrder) -> Option<&StaffId> {
    match order {
        AnyOrder::Settled(o) => Some(&o.settled_by),
        AnyOrder::Voided(o) => Some(&o.settled_by),
        AnyOrder::Cancelled(o) => Some(&o.cancelled_by),
        AnyOrder::Draft(_) | AnyOrder::Open(_) => None,
    }
}

/// "Cash", "Cash + UPI".
fn paid_by<'a>(modes: impl Iterator<Item = &'a str>) -> String {
    let mut labels: Vec<&str> = Vec::new();
    for label in modes {
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels.join(" + ")
}

fn bill_row(
    order: &AnyOrder,
    names: &Names<'_>,
    label_of: &impl Fn(&mb_core::TableId) -> Option<String>,
) -> Option<BillRowView> {
    let core = order.core();
    let (state, total, cashier, void_reason, paid) = match order {
        AnyOrder::Settled(o) => (
            "settled",
            o.bill.grand_total,
            names.find(&o.settled_by),
            None,
            paid_by(
                o.settlement
                    .payments()
                    .iter()
                    .map(|p| p.mode.report_label()),
            ),
        ),
        AnyOrder::Voided(o) => (
            "voided",
            o.bill.grand_total,
            names.find(&o.settled_by),
            Some(o.reason.clone()),
            paid_by(
                o.settlement
                    .payments()
                    .iter()
                    .map(|p| p.mode.report_label()),
            ),
        ),
        AnyOrder::Cancelled(o) => (
            "cancelled",
            Money::ZERO,
            names.find(&o.cancelled_by),
            Some(o.reason.clone()),
            String::new(),
        ),
        AnyOrder::Draft(_) | AnyOrder::Open(_) => return None,
    };

    Some(BillRowView {
        order_id: core.id.as_str().to_owned(),
        number: order
            .bill_number()
            .map(|n| n.formatted.clone())
            .unwrap_or_default(),
        at: words::when(core.created_at),
        table: core.table().map(|t| {
            let label = label_of(t).unwrap_or_else(|| t.as_str().to_owned());
            match core.seat() {
                Some(seat) => format!("Table {label}{}", seat.as_str()),
                None => format!("Table {label}"),
            }
        }),
        order_type: crate::billing::order_type_label(core.order_type()).to_owned(),
        items: crate::ipc::count(i64::try_from(core.cart.len()).unwrap_or(i64::MAX)),
        total: MoneyView::from(total),
        paid_by: paid,
        cashier,
        state_word: state_word(state).to_owned(),
        state: state.to_owned(),
        void_reason,
        refunded: None,
        reprints: 0,
        edited: false,
        approval: None,
    })
}

/// The word for a state. One list, because a badge and a saved file that disagree about what
/// happened to a bill are worse than either on its own.
fn state_word(state: &str) -> &str {
    match state {
        "settled" => "Paid",
        "voided" => "Voided",
        "cancelled" => "Cancelled",
        // No bill is in this state; it is one of the things the toolbar can ask for.
        "edited" => "Edited",
        other => other,
    }
}

/// What the register says about this bill: taken back, and whether that was signed off.
fn with_the_register(
    repos: &mb_db::Repos<'_>,
    id: &OrderId,
    mut row: BillRowView,
) -> Result<BillRowView, mb_db::DbError> {
    let reverts = repos.corrections().reverts_of(id)?;
    if !reverts.is_empty() {
        row.edited = true;
        row.approval = Some(if reverts.iter().all(|r| r.approved_at.is_some()) {
            "approved".to_owned()
        } else {
            "waiting".to_owned()
        });
    }
    Ok(row)
}

/// Does this bill pass the toolbar?
fn row_matches(
    order: &AnyOrder,
    row: &BillRowView,
    filter: &BillFilter,
    query: Option<&str>,
) -> bool {
    match filter.state.as_deref().filter(|s| !s.is_empty()) {
        Some("edited") if !(row.edited && row.state == "settled") => return false,
        Some(state) if state != "edited" && row.state != state => return false,
        _ => {}
    }
    if let Some(cashier) = filter.cashier.as_deref().filter(|c| !c.is_empty())
        && taken_by(order).is_none_or(|by| by.as_str() != cashier)
    {
        return false;
    }
    if let Some(mode) = filter.mode.as_deref().filter(|m| !m.is_empty()) {
        let settlement = match order {
            AnyOrder::Settled(o) => Some(&o.settlement),
            AnyOrder::Voided(o) => Some(&o.settlement),
            _ => None,
        };
        let paid_that_way = settlement.is_some_and(|s| {
            s.payments()
                .iter()
                .any(|p| p.mode.report_label().eq_ignore_ascii_case(mode))
        });
        if !paid_that_way {
            return false;
        }
    }
    if let Some(query) = query {
        let mut haystack = vec![
            row.number.to_lowercase(),
            row.table.clone().unwrap_or_default().to_lowercase(),
            row.order_type.to_lowercase(),
            row.total.text.to_lowercase(),
            row.cashier.clone().unwrap_or_default().to_lowercase(),
            row.paid_by.to_lowercase(),
        ];
        if let Some(token) = order.token() {
            haystack.push(token.formatted.to_lowercase());
        }
        for line in order.core().cart.lines() {
            haystack.push(line.snapshot.name.to_lowercase());
        }
        if !haystack.iter().any(|h| h.contains(query)) {
            return false;
        }
    }
    true
}

/// Gross, voids and net, from the rows on screen — so the header describes the list.
fn totals_of(rows: &[BillRowView]) -> DayTotalsView {
    let mut gross = 0_i64;
    let mut voids = 0_i64;
    let mut refunded = 0_i64;
    let mut bills = 0_i64;
    let mut voided_bills = 0_i64;
    let mut cancelled = 0_i64;
    for row in rows {
        match row.state.as_str() {
            "settled" => {
                gross = gross.saturating_add(row.total.paise);
                bills += 1;
            }
            "voided" => {
                gross = gross.saturating_add(row.total.paise);
                voids = voids.saturating_add(row.total.paise);
                bills += 1;
                voided_bills += 1;
            }
            _ => cancelled += 1,
        }
        if let Some(back) = &row.refunded {
            refunded = refunded.saturating_add(back.paise);
        }
    }
    DayTotalsView {
        gross: MoneyView::from(Money::from_paise(gross)),
        voids: MoneyView::from(Money::from_paise(voids)),
        net: MoneyView::from(Money::from_paise(gross.saturating_sub(voids))),
        refunded: MoneyView::from(Money::from_paise(refunded)),
        bills,
        voided_bills,
        cancelled_orders: cancelled,
    }
}

// One bill, opened.

pub fn bill_detail_on(app: &App, order_id: String) -> UiResult<BillDetailView> {
    let who = guard::require(app, Permission::ReportsView)?;
    let id = OrderId::new(order_id);
    let config = app.shop_config();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(order) = repos.orders().find(&id)? else {
                    return Err(mb_db::DbError::invariant("there is no such bill"));
                };
                let staff = repos.people().list_staff(OUTLET)?;
                let names = Names::new(&staff);
                let tables = repos.floor().list_tables(OUTLET)?;
                let label_of = |id: &mb_core::TableId| {
                    tables.iter().find(|t| &t.id == id).map(|t| t.label.clone())
                };
                let Some(row) = bill_row(&order, &names, &label_of) else {
                    return Err(mb_db::DbError::invariant(
                        "that order is still open, so it is on the floor and not in the bills",
                    ));
                };
                let row = with_the_register(&repos, &id, row)?;
                let reprints = repos.corrections().reprints_for(&id)?;
                let refunds = repos.corrections().refunds_for(&id)?;
                let refunded = refunds
                    .iter()
                    .fold(Money::ZERO, |sum, r| sum.add(r.amount).unwrap_or(sum));
                let row = BillRowView {
                    reprints: crate::ipc::count(i64::try_from(reprints.len()).unwrap_or(0)),
                    refunded: refunded.is_positive().then(|| MoneyView::from(refunded)),
                    ..row
                };

                let core = order.core();
                let (bill, settlement) = match &order {
                    AnyOrder::Settled(o) => (Some(&o.bill), Some(&o.settlement)),
                    AnyOrder::Voided(o) => (Some(&o.bill), Some(&o.settlement)),
                    _ => (None, None),
                };
                // A cancelled order never had a bill: its lines are priced now, only so the
                // screen can list them.
                let lines = match bill {
                    Some(bill) => crate::billing::bill_lines(bill),
                    None => crate::billing::bill_for(&core.cart, core.order_type(), None, &config)
                        .map(|b| crate::billing::bill_lines(&b))
                        .unwrap_or_default(),
                };
                let (tip, change) = settlement
                    .zip(bill)
                    .map(|(s, b)| (s.tip(), s.change_due(b.grand_total).unwrap_or(Money::ZERO)))
                    .unwrap_or((Money::ZERO, Money::ZERO));

                let mut history: Vec<(mb_core::Timestamp, HistoryView)> = Vec::new();
                let mut note = |at: mb_core::Timestamp, who: String, what: String| {
                    history.push((
                        at,
                        HistoryView {
                            when: words::when(at),
                            who,
                            what,
                        },
                    ));
                };
                match &order {
                    AnyOrder::Settled(o) => note(
                        o.settled_at,
                        names.of(&o.settled_by),
                        format!(
                            "Paid {} by {}",
                            o.bill.grand_total.to_plain_string(),
                            row.paid_by
                        ),
                    ),
                    AnyOrder::Voided(o) => {
                        note(
                            o.settled_at,
                            names.of(&o.settled_by),
                            format!(
                                "Paid {} by {}",
                                o.bill.grand_total.to_plain_string(),
                                row.paid_by
                            ),
                        );
                        note(
                            o.voided_at,
                            names.of(&o.voided_by),
                            format!("Voided: {}", o.reason),
                        );
                    }
                    AnyOrder::Cancelled(o) => note(
                        o.cancelled_at,
                        names.of(&o.cancelled_by),
                        format!("Cancelled: {}", o.reason),
                    ),
                    AnyOrder::Draft(_) | AnyOrder::Open(_) => {}
                }
                for refund in &refunds {
                    note(
                        refund.refunded_at,
                        names.maybe(refund.refunded_by.as_ref()),
                        format!(
                            "{} given back by {}: {}",
                            refund.amount.to_plain_string(),
                            refund.mode,
                            refund.reason
                        ),
                    );
                }
                for copy in &reprints {
                    note(
                        copy.printed_at,
                        names.maybe(copy.printed_by.as_ref()),
                        match &copy.reason {
                            Some(reason) => format!("Copy {} printed: {reason}", copy.copy),
                            None => format!("Copy {} printed", copy.copy),
                        },
                    );
                }

                let edits = edits_of(&repos, &order, &names, &config)?;
                for (edit, row) in edits.iter().zip(repos.corrections().reverts_of(&id)?) {
                    note(
                        row.before_settled_at,
                        names.maybe(row.before_settled_by.as_ref()),
                        format!("Paid {} by {}", edit.before_total.text, edit.before_paid_by),
                    );
                    note(
                        row.reverted_at,
                        edit.by.clone(),
                        format!("Taken back to the counter: {}", edit.reason),
                    );
                    if let (Some(at), Some(by)) = (row.approved_at, &row.approved_by) {
                        note(at, names.of(by), "Edit approved".to_owned());
                    }
                }

                history.sort_by_key(|(at, _)| at.millis());
                Ok(BillDetailView {
                    taken_by: Some(names.of(&core.created_by)),
                    covers: core.covers,
                    note: core.note.clone(),
                    lines,
                    bill: bill
                        .map(crate::billing::bill_view)
                        .transpose()
                        .map_err(|e| mb_db::DbError::invariant(e.message))?,
                    payments: settlement
                        .map(crate::billing::payment_views)
                        .unwrap_or_default(),
                    tip: tip.into(),
                    change: change.into(),
                    history: history.into_iter().map(|(_, h)| h).collect(),
                    can_approve: who.must(Permission::BillRevertApprove).is_ok()
                        && edits.iter().any(|e| e.approved_at.is_none()),
                    edits,
                    row,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// Every register entry for one bill, oldest first, each with what changed after it.
fn edits_of(
    repos: &mb_db::Repos<'_>,
    order: &AnyOrder,
    names: &Names<'_>,
    config: &crate::settings::ShopConfig,
) -> Result<Vec<RevertView>, mb_db::DbError> {
    let reverts = repos.corrections().reverts_of(&order.core().id)?;
    let mut befores: Vec<(Vec<RevertLine>, Vec<RevertPayment>)> = Vec::new();
    for revert in &reverts {
        befores.push((
            repos.corrections().revert_lines(&revert.id)?,
            repos.corrections().revert_payments(&revert.id)?,
        ));
    }

    // What the bill is now, for the last edit to compare against.
    let now_total = match order {
        AnyOrder::Settled(o) => Some(o.bill.grand_total),
        AnyOrder::Voided(o) => Some(o.bill.grand_total),
        AnyOrder::Cancelled(o) => {
            crate::billing::bill_for(&o.core.cart, o.core.order_type(), None, config)
                .ok()
                .map(|b| b.grand_total)
        }
        AnyOrder::Draft(_) | AnyOrder::Open(_) => None,
    };
    let now_lines: Vec<(String, Qty)> = tally(order.core().cart.lines().iter().map(|line| {
        (
            line_name(&line.snapshot.name, &line.modifiers, line.note.as_deref()),
            line.qty,
        )
    }));

    let mut out = Vec::new();
    for (index, revert) in reverts.iter().enumerate() {
        let (lines, payments) = &befores[index];
        let was = tally(lines.iter().map(|l| (l.name.clone(), l.qty)));
        // The next edit's "before" is this edit's "after"; the last edit's is the bill now.
        let (is, after_total, billed_again) = match befores.get(index + 1) {
            Some((next_lines, _)) => (
                tally(next_lines.iter().map(|l| (l.name.clone(), l.qty))),
                Some(reverts[index + 1].before_total),
                true,
            ),
            None => (
                now_lines.clone(),
                now_total,
                !matches!(order, AnyOrder::Open(_) | AnyOrder::Draft(_)),
            ),
        };
        let changes = if billed_again {
            changes_between(&was, &is, revert.before_total, after_total, order)
        } else {
            vec!["Not billed again yet".to_owned()]
        };
        out.push(RevertView {
            id: revert.id.clone(),
            reason: revert.reason.clone(),
            by: names.maybe(revert.reverted_by.as_ref()),
            at: words::when(revert.reverted_at),
            before_lines: lines
                .iter()
                .map(|l| BeforeLineView {
                    name: l.name.clone(),
                    qty: l.qty.to_string(),
                    unit_price: l.unit_price.into(),
                    amount: l.amount.into(),
                })
                .collect(),
            before_total: revert.before_total.into(),
            before_paid_by: paid_by(payments.iter().map(|p| p.mode.as_str())),
            before_paid_at: words::when(revert.before_settled_at),
            changes,
            approved_by: revert.approved_by.as_ref().map(|by| names.of(by)),
            approved_at: revert.approved_at.map(words::when),
        });
    }
    Ok(out)
}

/// What a line is called on the bill, with its extras and its note.
fn line_name(name: &str, modifiers: &[mb_core::Modifier], note: Option<&str>) -> String {
    let mut out = name.to_owned();
    let extras: Vec<&str> = modifiers.iter().map(|m| m.name.as_str()).collect();
    if !extras.is_empty() {
        out = format!("{out} ({})", extras.join(", "));
    }
    if let Some(note) = note.map(str::trim).filter(|n| !n.is_empty()) {
        out = format!("{out} \"{note}\"");
    }
    out
}

/// One entry per name, quantities added up.
fn tally(lines: impl Iterator<Item = (String, Qty)>) -> Vec<(String, Qty)> {
    let mut out: Vec<(String, Qty)> = Vec::new();
    for (name, qty) in lines {
        match out.iter_mut().find(|(known, _)| *known == name) {
            Some((_, known)) => *known = known.add(qty).unwrap_or(*known),
            None => out.push((name, qty)),
        }
    }
    out
}

/// The difference between a bill as it was and as it became, in words.
fn changes_between(
    was: &[(String, Qty)],
    is: &[(String, Qty)],
    before_total: Money,
    after_total: Option<Money>,
    order: &AnyOrder,
) -> Vec<String> {
    let mut out = Vec::new();
    for (name, qty) in was {
        match is.iter().find(|(known, _)| known == name) {
            None => out.push(format!("Removed {qty} {name}")),
            Some((_, now)) if now != qty => out.push(format!("{name}: {qty} → {now}")),
            Some(_) => {}
        }
    }
    for (name, qty) in is {
        if !was.iter().any(|(known, _)| known == name) {
            out.push(format!("Added {qty} {name}"));
        }
    }
    if let Some(after) = after_total
        && after != before_total
    {
        out.push(format!(
            "Total {} → {}",
            before_total.to_plain_string(),
            after.to_plain_string()
        ));
    }
    match order {
        AnyOrder::Cancelled(_) => out.push("Cancelled instead of billed again".to_owned()),
        AnyOrder::Voided(_) => out.push("Voided since".to_owned()),
        _ => {}
    }
    if out.is_empty() {
        out.push("Billed again unchanged".to_owned());
    }
    out
}

// The reasons.

/// The shop's own reasons for one flow.
pub fn reasons_on(app: &App, kind: String) -> UiResult<Vec<ReasonView>> {
    guard::require(app, Permission::BillCreate)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).corrections().reasons(OUTLET, &kind))
            .map_err(|e| words::from_db(&e))
            .map(|list| {
                list.into_iter()
                    .map(|r: Reason| ReasonView {
                        id: r.id,
                        text: r.text,
                    })
                    .collect()
            })
    })
}

#[cfg(test)]
pub fn day_totals_on(app: &App) -> UiResult<DayTotalsView> {
    guard::require(app, Permission::ReportsView)?;
    let day = today(now());
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).corrections().day_totals(OUTLET, day))
            .map_err(|e| words::from_db(&e))
            .map(|t| DayTotalsView {
                gross: MoneyView::from(t.gross),
                voids: MoneyView::from(t.voids),
                net: MoneyView::from(t.net),
                refunded: MoneyView::from(t.refunded),
                bills: t.bills,
                voided_bills: t.voided_bills,
                cancelled_orders: t.cancelled_orders,
            })
    })
}

// Void a bill.

/// Reverse a settled bill.
pub fn void_bill_on(
    app: &App,
    order_id: String,
    reason: String,
    approver_staff_id: Option<String>,
    approver_pin: Option<String>,
) -> UiResult<Vec<BillRowView>> {
    let who = guard::require(app, Permission::BillVoid)?;
    let at = now();
    let day = today(at);
    let id = OrderId::new(order_id.clone());

    let found = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&id))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(AnyOrder::Settled(settled)) = found else {
        return Err(UiError::new(
            "void.not_settled",
            "Only a bill that has been paid can be voided. Check the bill and try again.",
        ));
    };

    if let Some(refusal) = crate::dayclose::day_refusal_on(
        app,
        settled.core.business_day,
        "void.day_closed",
        "void this bill",
    )? {
        return Err(refusal);
    }

    // The second person.
    let total = settled.bill.grand_total;
    approve_if_needed(app, total, approver_staff_id, approver_pin)?;

    let voided = settled
        .clone()
        .void(&reason, who.staff_id.clone(), at)
        .map_err(|e| {
            UiError::new("void.refused", "This bill could not be voided.")
                .with_detail(e.to_string())
        })?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.orders().save(
                    OUTLET,
                    app.terminal_id(),
                    &AnyOrder::Voided(voided.clone()),
                )?;
                repos.stock().reverse_for_bill(
                    OUTLET,
                    &voided.core.id,
                    at,
                    day,
                    Some(&who.staff_id),
                )?;
                // The same transaction as the thing it describes.
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::BILL_VOIDED,
                        "bill",
                    )
                    .about(voided.bill_number.formatted.clone())
                    .changed(
                        serde_json::json!({
                            "state": "settled",
                            "total_paise": total.paise(),
                        }),
                        serde_json::json!({
                            "state": "voided",
                            "total_paise": total.paise(),
                            "reason": reason,
                        }),
                    ),
                )?;
                repos.kitchen().close_order(voided.core.id.as_str())?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    crate::log_bill!(
        voided.core.id,
        "bill {} voided by {} — {reason}",
        voided.bill_number.formatted,
        who.name
    );
    list_bills_on(app)
}

/// Does this void need a second person, and did it get one?
fn approve_if_needed(
    app: &App,
    total: Money,
    approver_staff_id: Option<String>,
    approver_pin: Option<String>,
) -> UiResult<()> {
    let threshold: Option<Money> = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).settings().get(OUTLET, APPROVAL_KEY))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(threshold) = threshold else {
        return Ok(()); // absent means never
    };
    if total.paise() < threshold.paise() {
        return Ok(());
    }

    let (Some(staff_id), Some(pin)) = (approver_staff_id, approver_pin) else {
        return Err(UiError::new(
            "void.needs_approval",
            format!(
                "A void of {} needs a manager. Ask somebody who can void bills to \
                 enter their PIN.",
                total.to_plain_string()
            ),
        ));
    };

    let pin = mb_auth::Pin::parse(&pin)
        .map_err(|e| UiError::new("auth.pin_shape", format!("{e}.")).with_detail(e.to_string()))?;

    let member = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).people().find_staff(OUTLET, &staff_id))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(member) = member else {
        return Err(UiError::new(
            "void.no_approver",
            "That person is not on this shop's staff list.",
        ));
    };
    // Approving is voiding. Somebody who may not void may not wave one through.
    if !member.permissions.has(Permission::BillVoid) {
        return Err(UiError::new(
            "void.approver_denied",
            format!("{} cannot void bills either.", member.name),
        ));
    }
    let stored = member
        .pin()
        .map_err(|e| words::from_db(&e))?
        .ok_or_else(|| {
            UiError::new(
                "void.approver_no_pin",
                format!("{} has no PIN, so they cannot approve this.", member.name),
            )
        })?;

    if !mb_auth::verify_pin(&pin, &stored) {
        return Err(UiError::new(
            "void.approver_wrong_pin",
            "That PIN is not right.",
        ));
    }
    Ok(())
}

// Revert a bill: back to the counter under the same number, to be fixed and billed again.

pub fn revert_bill_on(
    app: &App,
    order_id: String,
    reason: String,
    approver_staff_id: Option<String>,
    approver_pin: Option<String>,
) -> UiResult<String> {
    // One counter action at a time — see `App::begin_action`.
    let _one_at_a_time = app.begin_action();
    let who = guard::require(app, Permission::BillRevert)?;
    let at = now();
    let day = today(at);
    let id = OrderId::new(order_id);
    let till = app.terminal_id().to_owned();
    let reason = reason.trim().to_owned();
    if reason.is_empty() {
        return Err(UiError::new("revert.reason", "Give a reason."));
    }

    // The lines come back onto this counter, so it has to be free.
    let busy = app.with_cart(|state| Ok(!state.cart.is_empty()))?;
    if busy {
        return Err(UiError::new(
            "revert.counter_busy",
            "There is a bill on the counter. Finish it or clear it first.",
        ));
    }

    let Some(AnyOrder::Settled(settled)) = crate::flows::find_order(app, &id)? else {
        return Err(UiError::new(
            "revert.not_settled",
            "Only a bill that has been paid can be reverted.",
        ));
    };
    if let Some(refusal) = crate::dayclose::day_refusal_on(
        app,
        settled.core.business_day,
        "revert.day_closed",
        "revert this bill",
    )? {
        return Err(refusal);
    }
    let total = settled.bill.grand_total;
    approve_if_needed(app, total, approver_staff_id, approver_pin)?;

    // The bill as it is, for the register.
    let before_lines: Vec<RevertLine> = settled
        .bill
        .lines
        .iter()
        .map(|line| RevertLine {
            name: line_name(&line.snapshot.name, &line.modifiers, line.note.as_deref()),
            qty: line.qty,
            unit_price: line.snapshot.unit_price,
            amount: line.net,
        })
        .collect();
    let before_payments: Vec<RevertPayment> = settled
        .settlement
        .payments()
        .iter()
        .map(|p| RevertPayment {
            mode: p.mode.report_label().to_owned(),
            amount: p.amount,
        })
        .collect();
    let number = settled.bill_number.formatted.clone();
    let before_settled_at = settled.settled_at;
    let before_settled_by = settled.settled_by.clone();
    let open = settled.reopen();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                // The stock the bill used goes back on the shelf; billing again takes it off.
                repos.stock().reverse_for_bill(
                    OUTLET,
                    &open.core.id,
                    at,
                    day,
                    Some(&who.staff_id),
                )?;
                repos
                    .orders()
                    .save(OUTLET, &till, &AnyOrder::Open(open.clone()))?;
                repos.corrections().record_revert(
                    OUTLET,
                    &RevertRow {
                        id: crate::newid::fresh_at("rvt", at),
                        order_id: open.core.id.clone(),
                        business_day: day,
                        reason: reason.clone(),
                        reverted_at: at,
                        reverted_by: Some(who.staff_id.clone()),
                        before_total: total,
                        before_settled_at,
                        before_settled_by: Some(before_settled_by.clone()),
                        approved_at: None,
                        approved_by: None,
                    },
                    &before_lines,
                    &before_payments,
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::BILL_REVERTED,
                        "bill",
                    )
                    .about(number.clone())
                    .changed(
                        serde_json::json!({
                            "state": "settled",
                            "total_paise": total.paise(),
                        }),
                        serde_json::json!({
                            "state": "open",
                            "reason": reason,
                        }),
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // Onto the counter, the way pressing its table would put it there.
    let label = open
        .core
        .table()
        .and_then(|t| crate::flows::table_name(app, t));
    app.with_cart_mut(|state| {
        *state = crate::billing::CartState::load(&AnyOrder::Open(open.clone()), label);
        Ok(())
    })?;

    crate::log_bill!(
        open.core.id,
        "bill {number} taken back to the counter by {} — {reason}",
        who.name
    );
    Ok(format!("Bill {number} is back on the counter."))
}

/// A manager signs an edit off.
pub fn approve_revert_on(app: &App, revert_id: String) -> UiResult<()> {
    let who = guard::require(app, Permission::BillRevertApprove)?;
    let at = now();
    let day = today(at);

    let number = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let row = repos
                    .corrections()
                    .approve_revert(&revert_id, &who.staff_id, at)?;
                let number = repos
                    .corrections()
                    .bill_number_of(&row.order_id)?
                    .unwrap_or_else(|| row.order_id.as_str().to_owned());
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::REVERT_APPROVED,
                        "bill",
                    )
                    .about(number.clone())
                    .with_after(serde_json::json!({
                        "revert": revert_id,
                        "reason": row.reason,
                    })),
                )?;
                Ok(number)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!("the edit of bill {number} was approved by {}", who.name);
    Ok(())
}

// Cancel an open order.

/// The customer walked out.
pub fn cancel_order_on(app: &App, order_id: String, reason: String) -> UiResult<()> {
    let who = guard::require(app, Permission::OrderCancel)?;
    let at = now();
    let day = today(at);
    let id = OrderId::new(order_id.clone());

    let found = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&id))
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(AnyOrder::Open(open)) = found else {
        return Err(UiError::new(
            "cancel.not_open",
            "Only an order that is still open can be cancelled.",
        ));
    };

    // The kitchen first — while the order still says what it was told.
    let told: Vec<(mb_core::LineIdentity, mb_core::Qty)> = open.core.kitchen.told().to_vec();
    let table = open.core.table().cloned();

    let cancelled = open
        .clone()
        .cancel(&reason, who.staff_id.clone(), at)
        .map_err(|e| {
            UiError::new("cancel.refused", "This order could not be cancelled.")
                .with_detail(e.to_string())
        })?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.orders().save(
                    OUTLET,
                    app.terminal_id(),
                    &AnyOrder::Cancelled(cancelled.clone()),
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::ORDER_CANCELLED,
                        "order",
                    )
                    .about(order_id.clone())
                    .changed(
                        serde_json::json!({ "state": "open", "table": table.as_ref().map(mb_core::TableId::as_str) }),
                        serde_json::json!({ "state": "cancelled", "reason": reason }),
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // The order is on disk and the table is free.
    if !told.is_empty()
        && let Err(e) = print_cancellation(
            app,
            &open.core,
            crate::flows::ticket_lines(&open.core.cart, &told),
            table.as_ref(),
            Some(&mb_core::AnyOrder::Open(open.clone())),
        )
    {
        log_warn!("order {order_id} was cancelled but the kitchen slip failed: {e}");
    }

    // And this is the one thing on the kitchen screen that is allowed to interrupt.
    if let Err(e) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).kitchen().cancel_order(&order_id, at))
            .map_err(|e| words::from_db(&e))
    }) {
        log_warn!("order {order_id} was cancelled but the kitchen screen was not told: {e}");
    }

    log_info!("order {order_id} cancelled by {} — {reason}", who.name);
    Ok(())
}

/// Tell the kitchen to stop.
fn print_cancellation(
    app: &App,
    core: &mb_core::OrderCore,
    // Already in a cook's words, named from the cart that still had them.
    lines: Vec<(mb_core::ItemId, mb_print::template::TicketLine)>,
    table: Option<&mb_core::TableId>,
    // The order, when the caller has one — so the slip carries the same token and bill number
    // the ticket that started the cooking did.
    order: Option<&mb_core::AnyOrder>,
) -> UiResult<()> {
    // A cancellation slip is a kitchen ticket, so it obeys the shop's kitchen-ticket settings.
    crate::flows::queue_kitchen_lines(
        app,
        mb_print::template::TicketKind::Cancellation,
        core.order_type(),
        table,
        order,
        lines,
        false,
        "cancellation".to_owned(),
    )?;
    Ok(())
}

// Void one line.

/// Take one line off the order in the cart.
pub fn void_line_on(app: &App, index: usize, reason: String) -> UiResult<crate::billing::CartView> {
    // One counter action at a time — see `App::begin_action`.
    let _one_at_a_time = app.begin_action();
    let who = guard::require(app, Permission::OrderItemVoid)?;
    let at = now();
    let day = today(at);

    let (name, qty, order_id) = app.with_cart(|state| {
        let line = state.cart.lines().get(index).ok_or_else(|| {
            UiError::new("void_line.gone", "That line is not on this bill any more.")
        })?;
        Ok((
            line.snapshot.name.clone(),
            line.qty,
            state.order_id().map(str::to_owned),
        ))
    })?;

    // Off the order. The cook's words come from the cart as it was, since the line is
    // leaving it.
    let (cancel, slip, core) = app.with_cart_mut(|state| {
        let before = state.cart.clone();
        state.cart.remove(index).map_err(|e| {
            UiError::new("void_line.refused", "That line could not be removed.")
                .with_detail(e.to_string())
        })?;
        let cancel = state.kitchen.over_told(&state.cart).map_err(|e| {
            UiError::new(
                "void_line.kitchen",
                "The kitchen's list could not be worked out.",
            )
            .with_detail(e.to_string())
        })?;
        let slip = crate::flows::ticket_lines(&before, &cancel);
        Ok((
            cancel,
            slip,
            state.to_core(at, &who.staff_id, app.terminal_id()),
        ))
    })?;
    // The kitchen's slip does not need a table to exist yet, so a dine-in cart with no table is
    // not refused here — there is no paper without a ledger, and no ledger without a park.
    let core = core.ok();

    // The paper, before the ledger.
    if !cancel.is_empty() {
        let Some(core) = core.as_ref() else {
            return Err(UiError::new(
                "void_line.no_table",
                "This is a dine-in order with no table, so no kitchen slip can be printed.",
            ));
        };
        if let Err(e) = print_cancellation(app, core, slip, core.table(), None) {
            // Deliberately not fatal, and deliberately loud: the line IS off the bill, so the
            // customer is not charged.
            log_warn!("a line was voided but the kitchen slip failed: {e}");
            return Err(UiError::new(
                "void_line.no_slip",
                format!(
                    "{name} is off the bill, but the kitchen slip did not print. \
                     Tell the kitchen to stop it."
                ),
            )
            .with_detail(e.to_string()));
        }
        // And only now does the ledger believe it.
        app.with_cart_mut(|state| {
            state.kitchen.mark_cancelled(&cancel).map_err(|e| {
                UiError::new(
                    "void_line.record",
                    "The cancellation could not be recorded.",
                )
                .with_detail(e.to_string())
            })
        })?;
    }

    app.record(
        &AuditEntry::new(
            at,
            day,
            Some(who.staff_id.clone()),
            action::ITEM_VOIDED,
            "order",
        )
        .about(order_id.unwrap_or_else(|| "unsaved".to_owned()))
        .changed(
            serde_json::json!({ "item": name, "qty": qty.to_string() }),
            serde_json::json!({ "removed": true, "reason": reason }),
        ),
    );

    log_info!("{name} voided off the bill by {} — {reason}", who.name);
    app.with_cart(|state| crate::billing::cart_view(state, &app.shop_config()))
}

/// Print another copy, and say so on the paper.
pub fn reprint_bill_on(app: &App, order_id: String, reason: String) -> UiResult<String> {
    let who = guard::require(app, Permission::BillReprint)?;
    let at = now();
    let day = today(at);
    let id = OrderId::new(order_id.clone());

    let found = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).orders().find(&id))
            .map_err(|e| words::from_db(&e))
    })?;

    let (bill, order, voided_reason) = match found {
        Some(AnyOrder::Settled(o)) => (o.bill.clone(), AnyOrder::Settled(o), None),
        Some(AnyOrder::Voided(o)) => {
            let reason = o.reason.clone();
            (o.bill.clone(), AnyOrder::Voided(o), Some(reason))
        }
        _ => {
            return Err(UiError::new(
                "reprint.not_a_bill",
                "There is no bill to reprint for that order.",
            ));
        }
    };

    let copy = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let copy = repos.corrections().record_reprint(
                    OUTLET,
                    &id,
                    Some(&who.staff_id),
                    Some(&reason),
                    at,
                    day,
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::BILL_REPRINTED,
                        "bill",
                    )
                    .about(order_id.clone())
                    .with_after(serde_json::json!({ "copy": copy, "reason": reason })),
                )?;
                Ok(copy)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    let marking = match voided_reason {
        Some(reason) => mb_print::template::Copy::Voided { reason },
        None => mb_print::template::Copy::Duplicate { number: copy },
    };

    crate::flows::queue_bill(app, &order, &bill, &who.name, marking)?;

    log_info!("bill {order_id} reprinted as copy {copy} by {}", who.name);
    Ok(format!("Copy {copy} is printing."))
}

// Refund — 8.7.

/// Record money going back to a customer.
pub fn refund_on(
    app: &App,
    order_id: String,
    amount_paise: i64,
    mode: String,
    reason: String,
) -> UiResult<Vec<BillRowView>> {
    let who = guard::require(app, Permission::BillVoid)?;
    let at = now();
    let day = today(at);

    if amount_paise <= 0 {
        return Err(UiError::new(
            "refund.amount",
            "Type how much is going back to the customer.",
        ));
    }
    // A refund is money out of today's drawer, and a closed day takes none.
    if let Some(refusal) =
        crate::dayclose::day_refusal_on(app, day, "refund.day_closed", "refund this")?
    {
        return Err(refusal);
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let refund = Refund {
                    id: format!("{}_{order_id}", crate::newid::fresh_at("ref", at)),
                    order_id: OrderId::new(order_id.clone()),
                    amount: Money::from_paise(amount_paise),
                    mode: mode.clone(),
                    reason: reason.clone(),
                    refunded_at: at,
                    refunded_by: Some(who.staff_id.clone()),
                };
                repos.corrections().record_refund(OUTLET, &refund, day)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::BILL_VOIDED,
                        "refund",
                    )
                    .about(order_id.clone())
                    .with_after(serde_json::json!({
                        "amount_paise": amount_paise,
                        "mode": mode,
                        "reason": reason,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!(
        "{} refunded on {order_id} by {}",
        Money::from_paise(amount_paise).to_plain_string(),
        who.name
    );
    list_bills_on(app)
}

// The list, as a file.

/// The State column of a saved list: the word for the state, and anything else that happened
/// to the bill. The badges on the screen say the same things.
fn state_cell(row: &BillRowView) -> String {
    let mut out = row.state_word.clone();
    if row.edited {
        out.push_str(", edited");
    }
    if row.approval.as_deref() == Some("waiting") {
        out.push_str(", to approve");
    }
    out
}

/// What the file is of: the list, and the filters the person set — so two exports of the same
/// day under different filters are two files rather than one overwriting the other.
fn bills_title(view: &BillsView, filter: &BillFilter) -> String {
    let mut out = String::from("Bills");
    let mut add = |word: &str| {
        out.push(' ');
        out.push_str(word);
    };
    if let Some(id) = filter.cashier.as_deref().filter(|id| !id.is_empty()) {
        let name = view
            .cashiers
            .iter()
            .find(|c| c.id == id)
            .map_or(id, |c| c.name.as_str());
        add(name);
    }
    if let Some(state) = filter.state.as_deref().filter(|s| !s.is_empty()) {
        add(state_word(state));
    }
    if let Some(mode) = filter.mode.as_deref().filter(|m| !m.is_empty()) {
        add(mode);
    }
    if let Some(query) = filter
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        add(query);
    }
    out
}

/// The Bills screen as a report — the same shape every other report has, so the one CSV
/// writer and the one page layout serve this list too, filters and all.
pub(crate) fn bills_report(app: &App, filter: BillFilter) -> UiResult<crate::reports::ReportView> {
    let period = match &filter.period {
        Some(arg) => arg.parse()?,
        None => mb_db::repo::reports::Period::one_day(today(now())),
    };
    let view = bills_on(app, filter.clone())?;
    let totals = &view.totals;

    // The money last, because that is where a report keeps its figure — on a narrow roll it
    // is the one that stands beside the bill number.
    let columns = vec![
        crate::reports::column("Bill", false),
        crate::reports::column("When", false),
        crate::reports::column("Table", false),
        crate::reports::column("Items", true),
        crate::reports::column("Paid by", false),
        crate::reports::column("Taken by", false),
        crate::reports::column("State", false),
        crate::reports::column("Total", true),
    ];
    let rows: Vec<Vec<String>> = view
        .rows
        .iter()
        .map(|row| {
            vec![
                row.number.clone(),
                row.at.clone(),
                row.table.clone().unwrap_or_else(|| row.order_type.clone()),
                row.items.to_string(),
                row.paid_by.clone(),
                row.cashier.clone().unwrap_or_default(),
                state_cell(row),
                row.total.text.clone(),
            ]
        })
        .collect();
    // The Total column adds up to what was taken, voided bills included — the same figure the
    // screen's header calls "Taken".
    let mut total_row = vec![String::new(); columns.len()];
    total_row[0] = "Total".to_owned();
    total_row[columns.len() - 1] = totals.gross.text.clone();

    // What the column cannot say on its own.
    let mut notes = vec![format!("Net {}", totals.net.text)];
    if totals.voids.paise > 0 {
        notes.push(format!(
            "Voided {} on {} bills",
            totals.voids.text, totals.voided_bills
        ));
    }
    if totals.refunded.paise > 0 {
        notes.push(format!("Given back {}", totals.refunded.text));
    }
    if totals.cancelled_orders > 0 {
        notes.push(format!(
            "{} cancelled before they were billed",
            totals.cancelled_orders
        ));
    }

    Ok(crate::reports::ReportView {
        id: "bills".to_owned(),
        title: bills_title(&view, &filter),
        subtitle: crate::reports::period_words(period),
        columns,
        rows,
        totals: Some(total_row),
        compare: None,
        notes,
    })
}

// The command seats.

#[tauri::command]
pub fn bills(app: tauri::State<'_, App>, filter: BillFilter) -> UiResult<BillsView> {
    bills_on(&app, filter)
}

/// The list on screen, as a spreadsheet in the Downloads folder.
#[tauri::command]
pub fn bills_csv(
    app: tauri::State<'_, App>,
    filter: BillFilter,
) -> UiResult<crate::reports::SavedFileView> {
    // Taking the list out of the building is its own permission, on top of reading it.
    guard::require(&app, Permission::ReportsExport)?;
    let report = bills_report(&app, filter)?;
    let name = crate::reports::file_name(&report, "csv");
    crate::reports::save_and_show(&app, &name, crate::reports::csv_of(&report).as_bytes())
}

/// The same list, as a sheet of A4.
#[tauri::command]
pub fn bills_pdf(
    app: tauri::State<'_, App>,
    filter: BillFilter,
) -> UiResult<crate::reports::SavedFileView> {
    guard::require(&app, Permission::ReportsExport)?;
    let report = bills_report(&app, filter)?;
    let bytes = crate::reports::pdf_of(&app, &report)?;
    let name = crate::reports::file_name(&report, "pdf");
    crate::reports::save_and_show(&app, &name, &bytes)
}

#[tauri::command]
pub fn bill_detail(app: tauri::State<'_, App>, order_id: String) -> UiResult<BillDetailView> {
    bill_detail_on(&app, order_id)
}

#[tauri::command]
pub fn reasons(app: tauri::State<'_, App>, kind: String) -> UiResult<Vec<ReasonView>> {
    reasons_on(&app, kind)
}

#[tauri::command]
pub fn void_bill(
    app: tauri::State<'_, App>,
    order_id: String,
    reason: String,
    approver_staff_id: Option<String>,
    approver_pin: Option<String>,
) -> UiResult<Vec<BillRowView>> {
    void_bill_on(&app, order_id, reason, approver_staff_id, approver_pin)
}

#[tauri::command]
pub fn revert_bill(
    app: tauri::State<'_, App>,
    order_id: String,
    reason: String,
    approver_staff_id: Option<String>,
    approver_pin: Option<String>,
) -> UiResult<String> {
    revert_bill_on(&app, order_id, reason, approver_staff_id, approver_pin)
}

#[tauri::command]
pub fn approve_revert(app: tauri::State<'_, App>, revert_id: String) -> UiResult<()> {
    approve_revert_on(&app, revert_id)
}

#[tauri::command]
pub fn cancel_order(app: tauri::State<'_, App>, order_id: String, reason: String) -> UiResult<()> {
    cancel_order_on(&app, order_id, reason)
}

#[tauri::command]
pub fn void_line(
    app: tauri::State<'_, App>,
    index: usize,
    reason: String,
) -> UiResult<crate::billing::CartView> {
    void_line_on(&app, index, reason)
}

#[tauri::command]
pub fn reprint_bill(
    app: tauri::State<'_, App>,
    order_id: String,
    reason: String,
) -> UiResult<String> {
    reprint_bill_on(&app, order_id, reason)
}

#[tauri::command]
pub fn refund_bill(
    app: tauri::State<'_, App>,
    order_id: String,
    amount_paise: i64,
    mode: String,
    reason: String,
) -> UiResult<Vec<BillRowView>> {
    refund_on(&app, order_id, amount_paise, mode, reason)
}
