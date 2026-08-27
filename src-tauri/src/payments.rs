//! Did the money actually arrive?

use serde::Serialize;
use ts_rs::TS;

use mb_auth::Permission;
use mb_core::money::Money;
use mb_core::payment::PaymentMode;
use mb_core::provider::{Answer, Ask};
use mb_db::repo::payments::{Attempt, Unconfirmed};

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::state::{App, OUTLET};
use crate::words::{self, UiResult};

/// One payment nobody has confirmed, as the screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct UnconfirmedView {
    pub order_id: String,
    pub seq: i32,
    pub bill: String,
    pub mode: String,
    pub amount: MoneyView,
    pub reference: String,
    pub provider: String,
    /// "7:40 pm".
    pub when: String,
}

/// One ask, and what came back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct AttemptView {
    pub id: String,
    pub bill: String,
    pub provider: String,
    pub mode: String,
    pub amount: MoneyView,
    pub reference: String,
    /// `approved`, `declined` or `waiting`.
    pub answer: String,
    /// The provider's own words.
    pub because: String,
    pub when: String,
}

/// The payments screen: what is unconfirmed, and what the machines said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PaymentsView {
    pub day: String,
    /// Which provider this counter is asking.
    pub provider: String,
    pub unconfirmed: Vec<UnconfirmedView>,
    pub attempts: Vec<AttemptView>,
    /// The sum of everything unconfirmed.
    pub waiting: MoneyView,
    /// The headline sentence, in the shop's words.
    pub says: String,
}

fn when(at: mb_core::Timestamp) -> String {
    words::when(at)
}

fn view_of(row: &Unconfirmed) -> UnconfirmedView {
    UnconfirmedView {
        order_id: row.order_id.clone(),
        seq: i32::try_from(row.seq).unwrap_or(i32::MAX),
        bill: row
            .bill_number
            .clone()
            .unwrap_or_else(|| "No number".to_owned()),
        mode: mode_words(&row.mode),
        amount: MoneyView::from(row.amount),
        reference: row.reference.clone().unwrap_or_default(),
        provider: row.provider.clone().unwrap_or_else(|| "—".to_owned()),
        when: when(row.at),
    }
}

fn attempt_view_of(row: &Attempt) -> AttemptView {
    AttemptView {
        id: row.id.clone(),
        bill: row.order_id.clone().unwrap_or_else(|| "—".to_owned()),
        provider: row.provider.clone(),
        mode: mode_words(&row.mode),
        amount: MoneyView::from(row.amount),
        reference: row.reference.clone().unwrap_or_default(),
        answer: row.answer.clone(),
        because: row.because.clone().unwrap_or_default(),
        when: when(row.at),
    }
}

/// The shop's word for a mode tag.
fn mode_words(tag: &str) -> String {
    match tag {
        "cash" => "Cash",
        "card" => "Card",
        "upi" => "UPI",
        "credit" => "Credit",
        _ => "Other",
    }
    .to_owned()
}

// Asking a provider.

/// Ask whoever this shop's provider is, and write down what they said.
pub fn ask_about(
    app: &App,
    order_id: Option<&str>,
    mode: &PaymentMode,
    amount: Money,
    reference: Option<&str>,
) -> UiResult<Answer> {
    let provider = app.provider();
    let at = now();
    let day = today(at);
    let answer = provider.ask(&Ask {
        mode: mode.clone(),
        amount,
        reference: reference.map(str::to_owned),
    });

    // Cash is not written down as an attempt.
    if matches!(mode, PaymentMode::Cash | PaymentMode::Credit(_)) {
        return Ok(answer);
    }

    let who = app.sessions().current().map(|s| s.actor.staff_id);
    let tag = mb_db::encode::payment_mode_to_sql(mode).mode.to_owned();
    let write = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).payments().record_attempt(
                    OUTLET,
                    &crate::newid::fresh_at("try", at),
                    order_id,
                    provider.name(),
                    &tag,
                    amount,
                    reference,
                    &answer,
                    at,
                    day,
                    who.as_ref().map(mb_core::StaffId::as_str),
                )
            })
            .map_err(|e| words::from_db(&e))
    });
    // A ledger that cannot be written must not stop a sale.
    if let Err(e) = write {
        crate::log_warn!("the payment attempt could not be written down ({e})");
    }
    Ok(answer)
}

// The screen.

pub fn payments_on(app: &App) -> UiResult<PaymentsView> {
    guard::require(app, Permission::ReportsView)?;
    let at = now();
    let day = today(at);

    let (unconfirmed, attempts) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let p = repos.payments();
                Ok((p.unconfirmed_on(OUTLET, day)?, p.attempts_on(OUTLET, day)?))
            })
            .map_err(|e| words::from_db(&e))
    })?;

    let waiting = unconfirmed
        .iter()
        .try_fold(Money::ZERO, |sum, u| sum.add(u.amount))
        .unwrap_or(Money::ZERO);
    let refused = attempts.iter().filter(|a| a.answer == "declined").count();

    let says = match (unconfirmed.len(), refused) {
        (0, 0) => "Everything taken today is confirmed.".to_owned(),
        (0, n) => format!("{n} payment(s) were refused today."),
        (n, 0) => format!(
            "{n} payment(s) worth {} have not been confirmed.",
            waiting.to_indian_string()
        ),
        (n, r) => format!(
            "{n} payment(s) worth {} not confirmed, and {r} refused.",
            waiting.to_indian_string()
        ),
    };

    Ok(PaymentsView {
        day: day.to_string(),
        provider: app.provider().name().to_owned(),
        unconfirmed: unconfirmed.iter().map(view_of).collect(),
        attempts: attempts.iter().map(attempt_view_of).collect(),
        waiting: MoneyView::from(waiting),
        says,
    })
}

/// Somebody says the money arrived.
pub fn confirm_payment_on(
    app: &App,
    order_id: String,
    seq: i32,
    reference: String,
) -> UiResult<PaymentsView> {
    let who = guard::require(app, Permission::BillCreate)?;
    let at = now();
    let day = today(at);
    let reference = reference.trim().to_owned();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.payments().confirm(
                    OUTLET,
                    &order_id,
                    i64::from(seq),
                    if reference.is_empty() {
                        None
                    } else {
                        Some(reference.as_str())
                    },
                    at,
                    who.staff_id.as_str(),
                )?;
                repos.audit().append(
                    OUTLET,
                    &mb_auth::AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        mb_auth::audit::action::PAYMENT_CONFIRMED,
                        "order",
                    )
                    .about(order_id.clone())
                    .with_after(serde_json::json!({
                        "seq": seq,
                        "reference": reference,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    payments_on(app)
}

// The commands.

#[tauri::command]
pub fn payments(app: tauri::State<'_, App>) -> UiResult<PaymentsView> {
    payments_on(&app)
}

#[tauri::command]
pub fn confirm_payment(
    app: tauri::State<'_, App>,
    order_id: String,
    seq: i32,
    reference: String,
) -> UiResult<PaymentsView> {
    confirm_payment_on(&app, order_id, seq, reference)
}
