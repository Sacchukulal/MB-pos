//! **Did the money actually arrive?** — P29, scope 8.3 and 8.4.
//!
//! # Where the pieces live
//!
//! | | |
//! |---|---|
//! | the seam and the manual provider | [`mb_core::provider`] |
//! | the attempts ledger, the unconfirmed list | [`mb_db::repo::payments`] |
//! | this file | taking a payment through a provider, and the words |
//!
//! # What actually changes at the counter
//!
//! Pressing **Cash** is exactly what it was. Pressing **UPI** or **Card** now
//! asks whoever this shop's provider is, and there are three answers:
//!
//! | answer | what happens |
//! |---|---|
//! | approved | the payment is taken and marked confirmed |
//! | waiting | the payment is taken and marked **unconfirmed** |
//! | declined | **no payment is taken**, the bill stays unsettled, and the reason is shown and written down |
//!
//! The provider that ships answers `waiting` for everything electronic, because
//! it is a person looking at a phone and nothing in this product can check a
//! bank. That is not a small feature dressed up: it turns "we think that UPI
//! came through" into a list the shop reads before it closes.
//!
//! # The rule for this whole session applies here too
//!
//! **A payment machine that is absent, unplugged or slow must never stop a
//! sale.** A provider that cannot answer returns `waiting`; it does not hold
//! the counter, and the cashier can always take cash instead. The only thing
//! that stops a bill is an explicit *no* — which is not a failure of the
//! device, it is the bank saying the money is not there.

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
    /// Which provider this counter is asking. "Typed in by hand" today.
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

/// The shop's word for a mode tag (UI_GUIDELINES §6).
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

// ===========================================================================
// Asking a provider
// ===========================================================================

/// **Ask whoever this shop's provider is, and write down what they said.**
///
/// Called by `cart_add_payment` before a non-cash payment goes on the bill.
/// Returns the answer; the caller decides what to do with it, because only the
/// caller knows whether it is holding a cart.
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

    // **Cash is not written down as an attempt.** The manual provider approves
    // it without being asked anything, and a ledger with one row per cash sale
    // is a ledger nobody reads.
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
                    &format!("try_{}", at.millis()),
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
    // **A ledger that cannot be written must not stop a sale.** The answer is
    // still the answer; the shop loses a line of history and keeps its bill.
    if let Err(e) = write {
        crate::log_warn!("the payment attempt could not be written down ({e})");
    }
    Ok(answer)
}

// ===========================================================================
// The screen
// ===========================================================================

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

/// **Somebody says the money arrived.**
///
/// `BillCreate` and not `ReportsView`: this is the person at the counter with
/// the bank app open, and making it an owner-only action would mean nothing is
/// ever confirmed. It writes an audit row every time, which is what makes that
/// safe.
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

// ===========================================================================
// The commands
// ===========================================================================

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
