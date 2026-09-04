//! Customers and what they owe.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::credit::{self, LimitVerdict, MovementKind};
use mb_core::{BusinessDay, CustomerId, Money, StaffId};
use mb_db::repo::money::{CreditAdjustment, CreditPayment, Customer};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::log_info;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// What the screens see.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CustomerEdit {
    pub id: String,
    pub name: String,
    pub phone: String,
    pub gstin: String,
    pub address: String,
    /// Typed by a person, and blank means no limit — which is not a limit of zero.
    pub credit_limit: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CustomerView {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub gstin: Option<String>,
    pub address: Option<String>,
    pub credit_limit: Option<MoneyView>,
    pub is_active: bool,
    pub balance: MoneyView,
    /// "74 days" or "—".
    pub oldest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AgeingView {
    pub current: MoneyView,
    pub days_30: MoneyView,
    pub days_60: MoneyView,
    pub days_90: MoneyView,
    pub oldest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MovementView {
    /// "12 Aug 2026", written by Rust.
    pub date: String,
    /// "Bill", "Repayment", "Opening balance", "Adjustment".
    pub kind: String,
    pub note: String,
    /// What it did to the account, with its sign shown as a direction: `debit` adds to what is
    /// owed, `credit` reduces it.
    pub amount: MoneyView,
    pub adds: bool,
    /// The balance after this movement — what a statement column shows.
    pub running: MoneyView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub customer: CustomerView,
    pub ageing: AgeingView,
    pub movements: Vec<MovementView>,
    /// The whole statement as text, ready for the printer or the clipboard.
    pub statement: String,
}

fn to_view(customer: &Customer, balance: Money, oldest: Option<i32>) -> CustomerView {
    CustomerView {
        id: customer.id.as_str().to_owned(),
        name: customer.name.clone(),
        phone: customer.phone.clone(),
        gstin: customer.gstin.clone(),
        address: customer.address.clone(),
        credit_limit: customer.credit_limit.map(MoneyView::from),
        is_active: customer.is_active,
        balance: MoneyView::from(balance),
        oldest: days_words(oldest),
    }
}

fn day_words(day: BusinessDay) -> String {
    let (year, month, d) = day.to_ymd();
    format!("{year:04}-{month:02}-{d:02}")
}

/// "74 days", "1 day", "—".
fn days_words(days: Option<i32>) -> String {
    match days {
        None => "—".to_owned(),
        Some(0) => "today".to_owned(),
        Some(1) => "1 day".to_owned(),
        Some(n) => format!("{n} days"),
    }
}

fn kind_words(kind: MovementKind) -> &'static str {
    match kind {
        MovementKind::Sale => "Bill",
        MovementKind::Repayment => "Repayment",
        MovementKind::Opening => "Opening balance",
        MovementKind::Adjustment { increases: true } => "Added",
        MovementKind::Adjustment { increases: false } => "Written off",
    }
}

/// Who owes me money, oldest first.
pub fn who_owes_on(app: &App) -> UiResult<Vec<CustomerView>> {
    guard::require(app, Permission::CustomersManage)?;
    let day = today(now());

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Ok(mb_db::Repos::new(tx)
                    .money()
                    .who_owes(OUTLET, day)?
                    .iter()
                    .map(|owing| to_view(&owing.customer, owing.balance, owing.ageing.oldest_days))
                    .collect())
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// Everybody, whether they owe anything or not — the picker on the billing screen needs this
/// one.
pub fn customers_on(app: &App) -> UiResult<Vec<CustomerView>> {
    guard::require(app, Permission::CustomersManage)?;
    let day = today(now());

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut out = Vec::new();
                for customer in repos.money().list_customers(OUTLET)? {
                    let movements = repos.money().credit_movements(&customer.id)?;
                    let balance = credit::balance(&movements)
                        .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
                    let ageing = credit::ageing(&movements, day)
                        .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
                    out.push(to_view(&customer, balance, ageing.oldest_days));
                }
                Ok(out)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// One account in full: the balance, the ageing, every movement, and the statement text.
pub fn account_on(app: &App, customer_id: String) -> UiResult<AccountView> {
    guard::require(app, Permission::CustomersManage)?;
    let day = today(now());
    let id = CustomerId::new(customer_id);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let customer = repos
                    .money()
                    .list_customers(OUTLET)?
                    .into_iter()
                    .find(|c| c.id == id)
                    .ok_or_else(|| mb_db::DbError::invariant("that customer is not here"))?;

                let movements = repos.money().credit_movements(&id)?;
                let balance = credit::balance(&movements)
                    .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
                let ageing = credit::ageing(&movements, day)
                    .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;

                // The running balance, computed once here rather than in the screen: a
                // statement whose column does not add up is the one thing a statement may not
                // do.
                let mut running = Money::ZERO;
                let mut rows = Vec::with_capacity(movements.len());
                for movement in &movements {
                    running = if movement.kind.adds() {
                        running.add(movement.amount)
                    } else {
                        running.sub(movement.amount)
                    }
                    .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
                    rows.push(MovementView {
                        // The date as a person writes it, from mb-core — the screen never
                        // formats one.
                        date: day_words(movement.day),
                        kind: kind_words(movement.kind).to_owned(),
                        note: movement.note.clone(),
                        amount: MoneyView::from(movement.amount),
                        adds: movement.kind.adds(),
                        running: MoneyView::from(running),
                    });
                }

                let statement = statement_text(&customer, &rows, balance, &ageing);

                Ok(AccountView {
                    customer: to_view(&customer, balance, ageing.oldest_days),
                    ageing: AgeingView {
                        current: MoneyView::from(ageing.current),
                        days_30: MoneyView::from(ageing.days_30),
                        days_60: MoneyView::from(ageing.days_60),
                        days_90: MoneyView::from(ageing.days_90),
                        oldest: days_words(ageing.oldest_days),
                    },
                    movements: rows,
                    statement,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// The statement, as text.
fn statement_text(
    customer: &Customer,
    rows: &[MovementView],
    balance: Money,
    ageing: &mb_core::credit::Ageing,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("Statement — {}\n", customer.name));
    if let Some(phone) = &customer.phone {
        out.push_str(&format!("{phone}\n"));
    }
    out.push('\n');
    for row in rows {
        out.push_str(&format!(
            "{:<12} {:<16} {:>10} {:>10}\n",
            row.date,
            row.kind,
            if row.adds {
                row.amount.text.clone()
            } else {
                format!("-{}", row.amount.text)
            },
            row.running.text,
        ));
    }
    out.push_str(&format!("\nOutstanding: {}\n", balance.to_plain_string()));
    out.push_str(&format!(
        "Up to 30 days {} · 30-60 {} · 60-90 {} · over 90 {}\n",
        ageing.current.to_plain_string(),
        ageing.days_30.to_plain_string(),
        ageing.days_60.to_plain_string(),
        ageing.days_90.to_plain_string(),
    ));
    if let Some(days) = ageing.oldest_days {
        out.push_str(&format!("Oldest unpaid: {days} days\n"));
    }
    out
}

/// Add or edit a customer.
pub fn save_customer_on(app: &App, edit: CustomerEdit) -> UiResult<Vec<CustomerView>> {
    let who = guard::require(app, Permission::CustomersManage)?;
    let at = now();

    if edit.name.trim().is_empty() {
        return Err(UiError::new("credit.name", "A customer needs a name."));
    }
    // `Phone::parse`, and what it returns is what is stored.
    let phone = mb_core::Phone::parse_optional(&edit.phone)
        .map_err(|e| UiError::new("credit.phone", e.to_string()))?
        .map_or_else(String::new, |p| p.as_str().to_owned());
    let limit = if edit.credit_limit.trim().is_empty() {
        // Blank is NO LIMIT, not a limit of zero.
        None
    } else {
        Some(crate::menu::parse_money_public(&edit.credit_limit)?)
    };

    // The duplicate check, before the write, with words of its own — an Invariant from the
    // index would reach the shopkeeper as "the shop's data could not be read" (words.rs says
    // why).
    if !phone.is_empty() {
        let existing = app.with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    mb_db::Repos::new(tx)
                        .money()
                        .customer_by_phone(OUTLET, &phone)
                })
                .map_err(|e| words::from_db(&e))
        })?;
        if let Some(found) = existing
            && found.id.as_str() != edit.id
        {
            return Err(UiError::new(
                "credit.duplicate_phone",
                format!(
                    "That number is already {}'s. Open them instead?",
                    found.name
                ),
            )
            // The id rides in the detail so the screen can OPEN them rather than just refusing
            // — which is the whole point.
            .with_detail(found.id.as_str().to_owned()));
        }
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).money().save_customer(
                    OUTLET,
                    &Customer {
                        id: CustomerId::new(edit.id.clone()),
                        name: edit.name.trim().to_owned(),
                        phone: (!phone.is_empty()).then(|| phone.clone()),
                        gstin: some_trimmed(&edit.gstin),
                        address: some_trimmed(&edit.address),
                        credit_limit: limit,
                        is_active: edit.is_active,
                    },
                    at,
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!("{} saved the customer {}", who.name, edit.name);
    customers_on(app)
}

fn some_trimmed(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Money handed over against the account.
pub fn record_repayment_on(
    app: &App,
    customer_id: String,
    amount: String,
    mode: String,
    reference: String,
) -> UiResult<AccountView> {
    let who = guard::require(app, Permission::CreditCollect)?;
    let at = now();
    let day = today(at);
    let amount = crate::menu::parse_money_public(&amount)?;
    if !amount.is_positive() {
        return Err(UiError::new(
            "credit.amount",
            "A repayment is more than nothing.",
        ));
    }
    let mode = match mode.as_str() {
        "cash" | "card" | "upi" => mode,
        _ => {
            return Err(UiError::new(
                "credit.mode",
                "Take it as cash, card or UPI — a repayment is real money arriving.",
            ));
        }
    };
    // Money arriving is money in a day, and a closed day takes none of it.
    if let Some(refusal) =
        crate::dayclose::day_refusal_on(app, day, "credit.day_closed", "take this money")?
    {
        return Err(refusal);
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.money().record_credit_payment(
                    OUTLET,
                    &CreditPayment {
                        id: crate::newid::fresh_at("crp", at),
                        customer_id: CustomerId::new(customer_id.clone()),
                        amount,
                        mode: mode.clone(),
                        reference: some_trimmed(&reference),
                        received_at: at,
                        received_by: Some(who.staff_id.clone()),
                        business_day: day,
                        terminal: Some(app.terminal_id().to_owned()),
                    },
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::CREDIT_TAKEN,
                        "customer",
                    )
                    .about(customer_id.clone())
                    .with_after(serde_json::json!({
                        "amount_paise": amount.paise(),
                        "mode": mode,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!(
        "{} took a credit repayment of {}",
        who.name,
        amount.to_plain_string()
    );
    account_on(app, customer_id)
}

/// An opening balance, a write-off, or a correction.
pub fn save_adjustment_on(
    app: &App,
    customer_id: String,
    amount: String,
    increases: bool,
    reason: String,
) -> UiResult<AccountView> {
    let who = guard::require(app, Permission::CustomersManage)?;
    let at = now();
    let day = today(at);
    let amount = crate::menu::parse_money_public(&amount)?;
    if reason.trim().is_empty() {
        return Err(UiError::new(
            "credit.reason",
            "Say why. An adjustment without a reason is a mistake with paperwork.",
        ));
    }
    if let Some(refusal) =
        crate::dayclose::day_refusal_on(app, day, "credit.day_closed", "adjust this account")?
    {
        return Err(refusal);
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.money().save_credit_adjustment(
                    OUTLET,
                    &CreditAdjustment {
                        id: crate::newid::fresh_at("adj", at),
                        customer_id: CustomerId::new(customer_id.clone()),
                        amount,
                        increases,
                        reason: reason.trim().to_owned(),
                        at,
                        business_day: day,
                        made_by: Some(who.staff_id.clone()),
                    },
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::CREDIT_ADJUSTED,
                        "customer",
                    )
                    .about(customer_id.clone())
                    .with_after(serde_json::json!({
                        "amount_paise": amount.paise(),
                        "increases": increases,
                        "reason": reason.trim(),
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    account_on(app, customer_id)
}

// Putting a bill on the account.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct HeadroomView {
    pub customer: String,
    pub balance: MoneyView,
    pub after: MoneyView,
    pub limit: Option<MoneyView>,
    /// `fine`, `close` or `over` — decided in Rust.
    pub verdict: String,
    /// The whole sentence: "Rekha owes 4,200.00 and her limit is 5,000.00. This bill takes her
    /// to 5,340.00.".
    pub says: String,
}

/// What this bill would do to the account, before it is put there.
pub fn headroom_on(app: &App, customer_id: String) -> UiResult<HeadroomView> {
    guard::require(app, Permission::BillCreate)?;
    let bill = app.with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))?;
    let id = CustomerId::new(customer_id);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let customer = repos
                    .money()
                    .list_customers(OUTLET)?
                    .into_iter()
                    .find(|c| c.id == id)
                    .ok_or_else(|| mb_db::DbError::invariant("that customer is not here"))?;
                let movements = repos.money().credit_movements(&id)?;
                let balance = credit::balance(&movements)
                    .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
                let room = credit::headroom(balance, customer.credit_limit, bill)
                    .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;

                let says = match (room.verdict, customer.credit_limit) {
                    (LimitVerdict::Fine, None) => {
                        format!(
                            "{} owes {} and has no limit set.",
                            customer.name,
                            balance.to_plain_string()
                        )
                    }
                    (_, Some(limit)) => format!(
                        "{} owes {} and the limit is {}. This bill takes them to {}.",
                        customer.name,
                        balance.to_plain_string(),
                        limit.to_plain_string(),
                        room.after.to_plain_string(),
                    ),
                    (_, None) => format!("{} owes {}.", customer.name, balance.to_plain_string()),
                };

                Ok(HeadroomView {
                    customer: customer.name,
                    balance: MoneyView::from(room.balance),
                    after: MoneyView::from(room.after),
                    limit: room.limit.map(MoneyView::from),
                    verdict: match room.verdict {
                        LimitVerdict::Fine => "fine",
                        LimitVerdict::Close => "close",
                        LimitVerdict::Over => "over",
                    }
                    .to_owned(),
                    says,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// Put the balance of this bill on a customer's account.
pub fn put_on_account_on(
    app: &App,
    customer_id: String,
    override_limit: bool,
) -> UiResult<crate::billing::CartView> {
    // One counter action at a time — see `App::begin_action`.
    let _one_at_a_time = app.begin_action();
    let who = guard::require(app, Permission::BillCreate)?;
    let at = now();
    let room = headroom_on(app, customer_id.clone())?;

    if room.verdict == "over" {
        if !override_limit {
            return Err(UiError::new(
                "credit.over_limit",
                format!("{} Ask somebody who can approve it.", room.says),
            ));
        }
        // An override is a decision somebody made, so it has a name on it.
        guard::require(app, Permission::CustomersManage)?;
        app.with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    mb_db::Repos::new(tx).audit().append(
                        OUTLET,
                        &AuditEntry::new(
                            at,
                            today(at),
                            Some(who.staff_id.clone()),
                            action::CREDIT_LIMIT_OVERRIDDEN,
                            "customer",
                        )
                        .about(customer_id.clone())
                        .with_after(serde_json::json!({
                            "balance_paise": room.balance.paise,
                            "after_paise": room.after.paise,
                        })),
                    )
                })
                .map_err(|e| words::from_db(&e))
        })?;
        log_info!(
            "{} put a bill over the credit limit on an account",
            who.name
        );
    }

    // What is LEFT after any cash or card already taken — a split where the rest goes on the
    // account is an ordinary thing for a regular to ask for.
    let balance = app.with_cart(|state| {
        let bill = state.bill(&app.shop_config())?;
        state.settlement.balance(bill.grand_total).map_err(|e| {
            UiError::new("bill.compute", "This bill could not be worked out.")
                .with_detail(e.to_string())
        })
    })?;

    if !balance.is_positive() {
        return Err(UiError::new(
            "credit.nothing_owed",
            "This bill is already paid — there is nothing to put on the account.",
        ));
    }

    app.with_cart_mut(|state| {
        let payment = mb_core::Payment::new(
            mb_core::PaymentMode::Credit(CustomerId::new(customer_id.clone())),
            balance,
        )
        .map_err(|e| {
            UiError::new("credit.payment", "That could not go on the account.")
                .with_detail(e.to_string())
        })?;
        state.settlement.add(payment).map_err(|e| {
            UiError::new("credit.payment", "That could not go on the account.")
                .with_detail(e.to_string())
        })?;
        state.customer = Some(customer_id.clone());
        Ok(())
    })?;

    app.with_cart(|state| crate::billing::cart_view(state, &app.shop_config()))
}

// The seats.

#[tauri::command]
pub fn customers(app: tauri::State<'_, App>) -> UiResult<Vec<CustomerView>> {
    customers_on(&app)
}

#[tauri::command]
pub fn customer_account(app: tauri::State<'_, App>, customer_id: String) -> UiResult<AccountView> {
    account_on(&app, customer_id)
}

#[tauri::command]
pub fn save_customer(
    app: tauri::State<'_, App>,
    edit: CustomerEdit,
) -> UiResult<Vec<CustomerView>> {
    save_customer_on(&app, edit)
}

#[tauri::command]
pub fn record_repayment(
    app: tauri::State<'_, App>,
    customer_id: String,
    amount: String,
    mode: String,
    reference: String,
) -> UiResult<AccountView> {
    record_repayment_on(&app, customer_id, amount, mode, reference)
}

#[tauri::command]
pub fn save_credit_adjustment(
    app: tauri::State<'_, App>,
    customer_id: String,
    amount: String,
    increases: bool,
    reason: String,
) -> UiResult<AccountView> {
    save_adjustment_on(&app, customer_id, amount, increases, reason)
}

#[tauri::command]
pub fn credit_headroom(app: tauri::State<'_, App>, customer_id: String) -> UiResult<HeadroomView> {
    headroom_on(&app, customer_id)
}

#[tauri::command]
pub fn put_on_account(
    app: tauri::State<'_, App>,
    customer_id: String,
    override_limit: bool,
) -> UiResult<crate::billing::CartView> {
    put_on_account_on(&app, customer_id, override_limit)
}

#[allow(dead_code, reason = "P18 reads the ageing day from here")]
fn ageing_day() -> BusinessDay {
    today(now())
}

#[allow(dead_code, reason = "P18's report wants the collector")]
fn collector(who: &StaffId) -> String {
    who.as_str().to_owned()
}
