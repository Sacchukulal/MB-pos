//! **Bills travelling between tills** — P27, scope 11.3, D136.
//!
//! # A settled bill is a FACT, and facts are copied rather than reconciled
//!
//! The draft of P27 called reconciliation *"the highest-risk code in the
//! product"*. It is right, which is why this file has none.
//!
//! A secondary settles into **its own database**, exactly as a single till
//! always has, and then sends the settled order to the master. The master
//! writes it. There is nothing to merge because:
//!
//! * the money was computed by the same `mb_core` the master runs;
//! * the number came from a series only that till issues (D135), so it cannot
//!   collide with anything already here;
//! * and a settled bill is **immutable** — D47 makes a correction a STATE, so
//!   there is no later version of it to conflict with.
//!
//! # Which makes the hard parts disappear
//!
//! * **Sending twice is the same as sending once.** `applied_events` (D82)
//!   keyed on the order's id, and `OrderRepo::save` is an upsert underneath.
//!   So a till that has been off for a week sends its whole queue on Monday and
//!   the master ends with exactly one copy of each bill.
//! * **Order does not matter.** Bills are independent facts; there is no "what
//!   if they arrive out of sequence" question to answer.
//! * **The master is never on the critical path of a sale.** Nothing here is
//!   called while a customer is standing there — B5 is untouched (D135).

use mb_core::{AnyOrder, StaffId};
use mb_lan::{Forwarded, Receipt};

use crate::billing::TERMINAL;
use crate::state::{App, OUTLET};
use crate::words::{self, UiResult};

/// **The master's side.** Store what another till sent, once each.
pub fn receive_on(app: &App, forwarded: &Forwarded) -> UiResult<Receipt> {
    let at = crate::flows::now();
    let sender = forwarded.terminal_id.clone();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut stored = Vec::new();
                let mut refused = Vec::new();

                for raw in &forwarded.orders {
                    let Ok(order) = serde_json::from_value::<AnyOrder>(raw.clone()) else {
                        refused.push((
                            id_of(raw),
                            "This till is running a different version of Magic Bill. \
                             Update both and it will send again."
                                .to_owned(),
                        ));
                        continue;
                    };
                    let id = order_id(&order);

                    // **D82, and it is the whole safety of this feature.** The
                    // id and the EFFECT go into ONE transaction, so a crash
                    // between them cannot exist. P20 built these two calls and
                    // this reuses them rather than inventing a second
                    // exactly-once scheme.
                    let event = format!("forward:{id}");
                    if repos.events().recall(&event)?.is_some() {
                        // Already here. **A repeat is a success** — that is what
                        // lets the sender retry for ever without keeping track.
                        stored.push((id, true));
                        continue;
                    }

                    // The sender's terminal, not this one: a bill belongs to the
                    // drawer that took the money, which is what makes the day
                    // close per till (D140) tie across the shop.
                    repos.orders().save(OUTLET, &sender, &order)?;
                    if let AnyOrder::Settled(settled) = &order {
                        // Idempotent on its own account (P25 claims
                        // `stock:<order>`), so a re-send cannot deduct twice.
                        repos.stock().deduct_for_bill(OUTLET, settled, at)?;
                    }
                    repos.events().remember(OUTLET, &event, "forward", "stored", at)?;
                    stored.push((id, true));
                }

                let says = if refused.is_empty() {
                    format!(
                        "{} stored.",
                        words::count(stored.len() as i64, "bill", "bills")
                    )
                } else {
                    format!(
                        "{} stored, {} could not be.",
                        words::count(stored.len() as i64, "bill", "bills"),
                        refused.len()
                    )
                };
                Ok(Receipt {
                    stored,
                    refused,
                    says,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// **The secondary's side.** Everything settled here that the master has not
/// acknowledged yet.
///
/// Read from `sync_outbox`, which P04 reserved and P19/P20 have been filling
/// since a bill was first written — so the queue this feature needs already
/// existed and already survives a restart.
pub fn waiting_on(app: &App) -> UiResult<Vec<serde_json::Value>> {
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut out = Vec::new();
                for row in repos.outbox().pending(200)? {
                    if row.table_name != "orders" {
                        continue;
                    }
                    // Only what is FINISHED travels. A draft is not a fact
                    // (D137): it lives on the till that is typing it, and if
                    // that till never comes back nobody was charged for it.
                    if let Some(order) =
                        repos.orders().find(&mb_core::OrderId::new(row.row_id))?
                        && matches!(
                            order,
                            AnyOrder::Settled(_) | AnyOrder::Voided(_) | AnyOrder::Cancelled(_)
                        )
                        && let Ok(value) = serde_json::to_value(&order)
                    {
                        out.push(value);
                    }
                }
                Ok(out)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// Mark what the master confirmed, so it is never sent again.
///
/// **Only a confirmed apply clears the queue.** Nothing else deletes from it —
/// not a timeout, not a restart, not a person — because a bill that left the
/// queue without reaching the master is a bill the shop has lost.
pub fn confirmed_on(app: &App, receipt: &Receipt) -> UiResult<()> {
    let at = crate::flows::now();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                // The outbox is keyed by table and row, and `entry_id` is the
                // one place that key is spelled — deriving it here a second
                // time is how a queue silently stops clearing.
                let ids: Vec<String> = receipt
                    .stored
                    .iter()
                    .filter(|(_, ok)| *ok)
                    .map(|(id, _)| mb_db::repo::OutboxRepo::entry_id("orders", id))
                    .collect();
                let borrowed: Vec<&str> = ids.iter().map(String::as_str).collect();
                repos.outbox().mark_synced(&borrowed, at)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// What this till is holding, as one sentence for its own screen.
///
/// **A shop must be able to see that the tills are apart** — the same reasoning
/// as audit D4's persistent print-queue indicator, which is the other thing
/// this product refuses to hide.
pub fn waiting_says(count: usize) -> String {
    match count {
        0 => String::new(),
        n => format!(
            "{} waiting to reach the main till. Nothing is lost — they go across \
             as soon as it is back.",
            words::count(n as i64, "bill is", "bills are")
        ),
    }
}

/// This till's own name in a forwarded batch.
#[must_use]
pub fn from_here(orders: Vec<serde_json::Value>) -> Forwarded {
    Forwarded {
        terminal_id: TERMINAL.to_owned(),
        orders,
    }
}

fn order_id(order: &AnyOrder) -> String {
    match order {
        AnyOrder::Draft(o) => o.core.id.to_string(),
        AnyOrder::Open(o) => o.core.id.to_string(),
        AnyOrder::Settled(o) => o.core.id.to_string(),
        AnyOrder::Voided(o) => o.core.id.to_string(),
        AnyOrder::Cancelled(o) => o.core.id.to_string(),
    }
}

/// The id out of a payload we could not parse, so the refusal can name it.
fn id_of(raw: &serde_json::Value) -> String {
    raw.get("core")
        .and_then(|c| c.get("id"))
        .and_then(|i| i.as_str())
        .unwrap_or("unknown")
        .to_owned()
}

/// Keeps the staff import honest about why it is here.
const _: Option<StaffId> = None;
