//! Money going out, and what is in the drawer — scope 10.6.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::expense::{self, Every};
use mb_core::{BusinessDay, Money, StaffId, TaxRate};
use mb_db::repo::money::{CashMovement, Expense, ExpenseCategory, Recurring};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::log_info;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// What the screen sees.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ExpenseRowView {
    pub id: String,
    pub category_id: Option<String>,
    pub category: String,
    pub description: String,
    pub amount: MoneyView,
    /// "Cash", "Bank", "UPI", "Card" — the word, not the tag.
    pub mode: String,
    pub mode_tag: String,
    pub paid_to: Option<String>,
    pub reference: Option<String>,
    /// "18% · 180.00", or absent.
    pub input_credit: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CategoryTotalView {
    pub id: Option<String>,
    pub name: String,
    pub total: MoneyView,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MovementRowView {
    pub id: String,
    /// "Opening float", "Top-up", "Payout", "Bank drop".
    pub kind: String,
    pub kind_tag: String,
    pub amount: MoneyView,
    pub reason: String,
    /// True when it takes money OUT of the drawer, so a screen shows the direction without
    /// knowing the vocabulary.
    pub takes_out: bool,
}

/// The drawer, in the order a shopkeeper counts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CashPositionView {
    pub opening_float: MoneyView,
    pub cash_sales: MoneyView,
    pub top_ups: MoneyView,
    pub cash_expenses: MoneyView,
    pub payouts: MoneyView,
    pub bank_drops: MoneyView,
    pub suppliers_paid: MoneyView,
    pub expected: MoneyView,
    /// The whole sum as one sentence, so the screen never assembles it: "2,000.00 float +
    /// 3,450.00 cash sales + 0.00 top-ups − 400.00 expenses − 300.00 payouts − 1,000.00 to the
    /// bank − 2,000.00 to suppliers".
    pub says: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DueView {
    pub id: String,
    pub description: String,
    pub amount: MoneyView,
    pub paid_to: Option<String>,
    /// "due today", "3 days late" — said in Rust.
    pub when: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ExpensesView {
    pub rows: Vec<ExpenseRowView>,
    pub categories: Vec<CategoryTotalView>,
    pub all_categories: Vec<CategoryTotalView>,
    pub movements: Vec<MovementRowView>,
    pub cash: CashPositionView,
    pub total: MoneyView,
    /// 6's "this month against last".
    pub this_month: MoneyView,
    pub last_month: MoneyView,
    /// Templates that are due and have not been confirmed.
    pub due: Vec<DueView>,
}

/// What the screen sends to record or edit one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ExpenseEdit {
    pub id: String,
    pub category_id: Option<String>,
    pub description: String,
    /// Typed by a person.
    pub amount: String,
    pub mode: String,
    pub paid_to: String,
    pub reference: String,
    /// "18", "5", or blank for no input credit.
    pub gst_percent: String,
    pub note: String,
}

fn mode_words(tag: &str) -> &'static str {
    match tag {
        "bank" => "Bank",
        "upi" => "UPI",
        "card" => "Card",
        _ => "Cash",
    }
}

fn kind_words(tag: &str) -> &'static str {
    match tag {
        "float" => "Opening float",
        "top_up" => "Top-up",
        "payout" => "Payout",
        _ => "Bank drop",
    }
}

pub fn expenses_on(app: &App) -> UiResult<ExpensesView> {
    guard::require(app, Permission::ExpensesManage)?;
    let day = today(now());

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let categories = repos.money().list_expense_categories(OUTLET)?;
                let name_of = |id: &Option<String>| {
                    id.as_ref()
                        .and_then(|id| categories.iter().find(|c| &c.id == id))
                        .map_or_else(|| "No category".to_owned(), |c| c.name.clone())
                };

                let today_rows = repos.money().list_expenses(OUTLET, day)?;
                let rows: Vec<ExpenseRowView> = today_rows
                    .iter()
                    .map(|e| ExpenseRowView {
                        id: e.id.clone(),
                        category: name_of(&e.category_id),
                        category_id: e.category_id.clone(),
                        description: e.description.clone(),
                        amount: MoneyView::from(e.amount),
                        mode: mode_words(&e.mode).to_owned(),
                        mode_tag: e.mode.clone(),
                        paid_to: e.paid_to.clone(),
                        reference: e.reference.clone(),
                        input_credit: match (e.gst_rate_bp, e.gst_amount) {
                            (Some(bp), Some(amount)) => Some(format!(
                                "{} · {}",
                                TaxRate::from_basis_points(u32::try_from(bp).unwrap_or(0))
                                    .map_or_else(|| "?".to_owned(), |r| r.label()),
                                amount.to_plain_string(),
                            )),
                            _ => None,
                        },
                        note: e.note.clone(),
                    })
                    .collect();

                let total = Money::try_sum(today_rows.iter().map(|e| e.amount))
                    .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;

                // Category totals for the day.
                let mut by_category: Vec<CategoryTotalView> = Vec::new();
                for expense in &today_rows {
                    let name = name_of(&expense.category_id);
                    match by_category.iter_mut().find(|c| c.name == name) {
                        Some(found) => {
                            found.total = MoneyView::from(
                                Money::from_paise(found.total.paise)
                                    .add(expense.amount)
                                    .map_err(|e| mb_db::DbError::invariant(e.to_string()))?,
                            );
                            found.count += 1;
                        }
                        None => by_category.push(CategoryTotalView {
                            id: expense.category_id.clone(),
                            name,
                            total: MoneyView::from(expense.amount),
                            count: 1,
                        }),
                    }
                }

                let position = repos.money().cash_position(OUTLET, day)?;
                let movements: Vec<MovementRowView> = repos
                    .money()
                    .list_cash_movements(OUTLET, day)?
                    .iter()
                    .map(|m| MovementRowView {
                        id: m.id.clone(),
                        kind: kind_words(&m.kind).to_owned(),
                        kind_tag: m.kind.clone(),
                        amount: MoneyView::from(m.amount),
                        reason: m.reason.clone(),
                        takes_out: matches!(m.kind.as_str(), "payout" | "bank_drop"),
                    })
                    .collect();

                // This month against last, by business day.
                let (year, month, _) = day.to_ymd();
                let (prev_year, prev_month) =
                    if month == 1 { (year - 1, 12) } else { (year, month - 1) };
                let month_total = |y: i32, m: u32| -> Result<Money, mb_db::DbError> {
                    let from = BusinessDay::from_ymd(y, m, 1);
                    let to = BusinessDay::from_ymd(y, m, expense::last_day_of(y, m));
                    let rows = repos.money().expenses_between(OUTLET, from, to)?;
                    Money::try_sum(rows.iter().map(|e| e.amount))
                        .map_err(|e| mb_db::DbError::invariant(e.to_string()))
                };

                // Reminders. Nothing posts itself — this is a list, and a person confirms each
                // one.
                let due: Vec<DueView> = repos
                    .money()
                    .list_recurring(OUTLET)?
                    .iter()
                    .filter(|t| {
                        t.is_active && t.next_due.days_since_epoch() <= day.days_since_epoch()
                    })
                    .map(|t| DueView {
                        id: t.id.clone(),
                        description: t.description.clone(),
                        amount: MoneyView::from(t.amount),
                        paid_to: t.paid_to.clone(),
                        when: match day.days_since_epoch() - t.next_due.days_since_epoch() {
                            0 => "due today".to_owned(),
                            1 => "1 day late".to_owned(),
                            n => format!("{n} days late"),
                        },
                    })
                    .collect();

                Ok(ExpensesView {
                    rows,
                    categories: by_category,
                    all_categories: categories
                        .iter()
                        .filter(|c| c.is_active)
                        .map(|c| CategoryTotalView {
                            id: Some(c.id.clone()),
                            name: c.name.clone(),
                            total: MoneyView::from(Money::ZERO),
                            count: 0,
                        })
                        .collect(),
                    movements,
                    cash: CashPositionView {
                        opening_float: MoneyView::from(position.opening_float),
                        cash_sales: MoneyView::from(position.cash_sales),
                        top_ups: MoneyView::from(position.top_ups),
                        cash_expenses: MoneyView::from(position.cash_expenses),
                        payouts: MoneyView::from(position.payouts),
                        bank_drops: MoneyView::from(position.bank_drops),
                        suppliers_paid: MoneyView::from(position.suppliers_paid),
                        expected: MoneyView::from(position.expected),
                        says: format!(
                            "{} float + {} cash sales + {} top-ups − {} expenses − {} payouts − {} to the bank − {} to suppliers",
                            position.opening_float.to_plain_string(),
                            position.cash_sales.to_plain_string(),
                            position.top_ups.to_plain_string(),
                            position.cash_expenses.to_plain_string(),
                            position.payouts.to_plain_string(),
                            position.bank_drops.to_plain_string(),
                            position.suppliers_paid.to_plain_string(),
                        ),
                    },
                    total: MoneyView::from(total),
                    this_month: MoneyView::from(month_total(year, month)?),
                    last_month: MoneyView::from(month_total(prev_year, prev_month)?),
                    due,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

fn trimmed(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Record or edit an expense.
pub fn save_expense_on(app: &App, edit: ExpenseEdit) -> UiResult<ExpensesView> {
    let who = guard::require(app, Permission::ExpensesManage)?;
    let at = now();
    let day = today(at);

    if edit.description.trim().is_empty() {
        return Err(UiError::new("expense.what", "Say what the money went on."));
    }
    let amount = crate::menu::parse_money_public(&edit.amount)?;
    if !amount.is_positive() {
        return Err(UiError::new(
            "expense.amount",
            "An expense is more than nothing.",
        ));
    }
    if !matches!(edit.mode.as_str(), "cash" | "bank" | "upi" | "card") {
        return Err(UiError::new(
            "expense.mode",
            "Pay it in cash, by bank, UPI or card.",
        ));
    }

    // The input credit, extracted from what was paid rather than added to it
    // (mb_core::expense::input_credit says why).
    let (rate_bp, gst_amount) = if edit.gst_percent.trim().is_empty() {
        (None, None)
    } else {
        let bp = mb_auth::RoleShape::parse_percent(edit.gst_percent.trim())
            .map_err(|e| UiError::new("expense.gst", e.to_string()))?
            .unwrap_or(0);
        let rate = TaxRate::from_basis_points(bp)
            .ok_or_else(|| UiError::new("expense.gst", "A tax rate is between 0% and 100%."))?;
        let credit = expense::input_credit(amount, rate).map_err(|e| {
            UiError::new("expense.gst", "That input credit could not be worked out.")
                .with_detail(e.to_string())
        })?;
        (Some(i64::from(bp)), Some(credit))
    };

    let before = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Ok(mb_db::Repos::new(tx)
                    .money()
                    .list_expenses(OUTLET, day)?
                    .into_iter()
                    .find(|e| e.id == edit.id))
            })
            .map_err(|e| words::from_db(&e))
    })?;
    // An expense written into a closed day would change a figure that was frozen.
    let dated = before.as_ref().map_or(day, |b| b.business_day);
    if let Some(refusal) =
        crate::dayclose::day_refusal_on(app, dated, "expense.day_closed", "record this")?
    {
        return Err(refusal);
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.money().save_expense(
                    OUTLET,
                    &Expense {
                        id: edit.id.clone(),
                        category_id: edit.category_id.clone(),
                        description: edit.description.trim().to_owned(),
                        amount,
                        mode: edit.mode.clone(),
                        paid_to: trimmed(&edit.paid_to),
                        reference: trimmed(&edit.reference),
                        gst_rate_bp: rate_bp,
                        gst_amount,
                        paid_at: at,
                        paid_by: Some(who.staff_id.clone()),
                        // The BUSINESS day: an expense paid at 00:30 belongs to the evening
                        // still being worked, exactly like a bill.
                        business_day: before.as_ref().map_or(day, |b| b.business_day),
                        note: trimmed(&edit.note),
                    },
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::EXPENSE_SAVED,
                        "expense",
                    )
                    .about(edit.id.clone())
                    .changed(
                        before
                            .as_ref()
                            .map_or(serde_json::Value::Null, expense_json),
                        serde_json::json!({
                            "what": edit.description.trim(),
                            "amount_paise": amount.paise(),
                            "mode": edit.mode,
                            "paid_to": edit.paid_to.trim(),
                        }),
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!("{} recorded {}", who.name, amount.to_plain_string());
    expenses_on(app)
}

fn expense_json(expense: &Expense) -> serde_json::Value {
    serde_json::json!({
        "what": expense.description,
        "amount_paise": expense.amount.paise(),
        "mode": expense.mode,
        "paid_to": expense.paid_to,
    })
}

pub fn delete_expense_on(app: &App, id: String) -> UiResult<ExpensesView> {
    let who = guard::require(app, Permission::ExpensesManage)?;
    let at = now();
    let day = today(at);
    if let Some(refusal) =
        crate::dayclose::day_refusal_on(app, day, "expense.day_closed", "delete this")?
    {
        return Err(refusal);
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let before = repos
                    .money()
                    .list_expenses(OUTLET, day)?
                    .into_iter()
                    .find(|e| e.id == id);
                repos.money().delete_expense(OUTLET, &id, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::EXPENSE_DELETED,
                        "expense",
                    )
                    .about(id.clone())
                    .changed(
                        before
                            .as_ref()
                            .map_or(serde_json::Value::Null, expense_json),
                        serde_json::Value::Null,
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    expenses_on(app)
}

/// Money in or out of the drawer that is not a sale and not an expense.
pub fn save_movement_on(
    app: &App,
    kind: String,
    amount: String,
    reason: String,
) -> UiResult<ExpensesView> {
    let who = guard::require(app, Permission::ExpensesManage)?;
    let at = now();
    let day = today(at);
    let amount = crate::menu::parse_money_public(&amount)?;

    if !matches!(kind.as_str(), "float" | "top_up" | "payout" | "bank_drop") {
        return Err(UiError::new(
            "cash.kind",
            "That is not something the drawer does.",
        ));
    }
    if reason.trim().is_empty() {
        return Err(UiError::new(
            "cash.reason",
            "Say why. Money leaving a drawer without a reason is how a shortfall becomes an argument.",
        ));
    }
    if let Some(refusal) =
        crate::dayclose::day_refusal_on(app, day, "cash.day_closed", "move this")?
    {
        return Err(refusal);
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.money().save_cash_movement(
                    OUTLET,
                    &CashMovement {
                        id: crate::newid::fresh_at("cm", at),
                        kind: kind.clone(),
                        amount,
                        reason: reason.trim().to_owned(),
                        at,
                        business_day: day,
                        moved_by: Some(who.staff_id.clone()),
                    },
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::CASH_MOVED,
                        "drawer",
                    )
                    .with_after(serde_json::json!({
                        "kind": kind,
                        "amount_paise": amount.paise(),
                        "reason": reason.trim(),
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    expenses_on(app)
}

pub fn save_category_on(
    app: &App,
    id: String,
    name: String,
    is_active: bool,
) -> UiResult<ExpensesView> {
    guard::require(app, Permission::ExpensesManage)?;
    let at = now();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).money().save_expense_category(
                    OUTLET,
                    &ExpenseCategory {
                        id: id.clone(),
                        name: name.clone(),
                        sort_order: 0,
                        is_active,
                    },
                    at,
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    expenses_on(app)
}

/// A template — rent, salary, the internet bill.
pub fn save_recurring_on(
    app: &App,
    id: String,
    description: String,
    amount: String,
    mode: String,
    every: String,
    category_id: Option<String>,
) -> UiResult<ExpensesView> {
    guard::require(app, Permission::ExpensesManage)?;
    let at = now();
    let amount = crate::menu::parse_money_public(&amount)?;
    if description.trim().is_empty() {
        return Err(UiError::new("expense.what", "Say what it is for."));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).money().save_recurring(
                    OUTLET,
                    &Recurring {
                        id: id.clone(),
                        category_id: category_id.clone(),
                        description: description.trim().to_owned(),
                        amount,
                        mode: mode.clone(),
                        paid_to: None,
                        every: if every == "week" {
                            Every::Week
                        } else {
                            Every::Month
                        },
                        next_due: today(at),
                        is_active: true,
                    },
                    at,
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    expenses_on(app)
}

/// Confirm a reminder — the only way a template becomes money.
pub fn confirm_due_on(app: &App, id: String) -> UiResult<ExpensesView> {
    let who = guard::require(app, Permission::ExpensesManage)?;
    let at = now();
    let day = today(at);

    let template = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Ok(mb_db::Repos::new(tx)
                    .money()
                    .list_recurring(OUTLET)?
                    .into_iter()
                    .find(|t| t.id == id))
            })
            .map_err(|e| words::from_db(&e))
    })?;

    let Some(template) = template else {
        return Err(UiError::new(
            "expense.no_template",
            "That reminder is not here any more.",
        ));
    };
    if template.next_due.days_since_epoch() > day.days_since_epoch() {
        return Err(UiError::new(
            "expense.not_due",
            "That one is not due yet — it has already been recorded for this period.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.money().save_expense(
                    OUTLET,
                    &Expense {
                        id: crate::newid::fresh_at("exp", at),
                        category_id: template.category_id.clone(),
                        description: template.description.clone(),
                        amount: template.amount,
                        mode: template.mode.clone(),
                        paid_to: template.paid_to.clone(),
                        reference: None,
                        gst_rate_bp: None,
                        gst_amount: None,
                        paid_at: at,
                        paid_by: Some(who.staff_id.clone()),
                        business_day: day,
                        note: Some("from a reminder".to_owned()),
                    },
                )?;
                // Advanced only now, and only because a person said so.
                let mut advanced = template.clone();
                advanced.next_due = expense::next_due(template.next_due, template.every);
                repos.money().save_recurring(OUTLET, &advanced, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::EXPENSE_SAVED,
                        "expense",
                    )
                    .about(template.id.clone())
                    .with_after(serde_json::json!({
                        "what": template.description,
                        "amount_paise": template.amount.paise(),
                        "from": "a confirmed reminder",
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    expenses_on(app)
}

pub fn export_expenses_on(app: &App) -> UiResult<String> {
    guard::require(app, Permission::ExpensesManage)?;
    let view = expenses_on(app)?;

    let mut out = String::from("date,category,description,amount,mode,paid_to,reference,note\n");
    let day = today(now());
    let (year, month, d) = day.to_ymd();
    for row in &view.rows {
        let cells: [String; 8] = [
            format!("{year:04}-{month:02}-{d:02}"),
            row.category.clone(),
            row.description.clone(),
            row.amount.text.clone(),
            row.mode.clone(),
            row.paid_to.clone().unwrap_or_default(),
            row.reference.clone().unwrap_or_default(),
            row.note.clone().unwrap_or_default(),
        ];
        mb_db::export::write_row(&mut out, cells.iter().map(|c| Some(c.as_str())));
    }
    Ok(out)
}

// The seats.

#[tauri::command]
pub fn expenses(app: tauri::State<'_, App>) -> UiResult<ExpensesView> {
    expenses_on(&app)
}

#[tauri::command]
pub fn save_expense(app: tauri::State<'_, App>, edit: ExpenseEdit) -> UiResult<ExpensesView> {
    save_expense_on(&app, edit)
}

#[tauri::command]
pub fn delete_expense(app: tauri::State<'_, App>, id: String) -> UiResult<ExpensesView> {
    delete_expense_on(&app, id)
}

#[tauri::command]
pub fn save_cash_movement(
    app: tauri::State<'_, App>,
    kind: String,
    amount: String,
    reason: String,
) -> UiResult<ExpensesView> {
    save_movement_on(&app, kind, amount, reason)
}

#[tauri::command]
pub fn save_expense_category(
    app: tauri::State<'_, App>,
    id: String,
    name: String,
    is_active: bool,
) -> UiResult<ExpensesView> {
    save_category_on(&app, id, name, is_active)
}

#[tauri::command]
pub fn save_recurring_expense(
    app: tauri::State<'_, App>,
    id: String,
    description: String,
    amount: String,
    mode: String,
    every: String,
    category_id: Option<String>,
) -> UiResult<ExpensesView> {
    save_recurring_on(&app, id, description, amount, mode, every, category_id)
}

#[tauri::command]
pub fn confirm_recurring_expense(app: tauri::State<'_, App>, id: String) -> UiResult<ExpensesView> {
    confirm_due_on(&app, id)
}

#[tauri::command]
pub fn export_expenses(app: tauri::State<'_, App>) -> UiResult<String> {
    export_expenses_on(&app)
}

#[allow(dead_code, reason = "P18 reads the drawer's staff from here")]
fn counted_by(who: &StaffId) -> String {
    who.as_str().to_owned()
}
