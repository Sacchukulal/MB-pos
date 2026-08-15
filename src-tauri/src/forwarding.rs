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

use mb_core::AnyOrder;
use mb_lan::{Forwarded, Receipt};
use tauri::Manager as _;

use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

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

                // **The master learns about a till the first time it hears from
                // one.** Pairing made a DEVICE row; a forwarded bill points at a
                // TERMINAL, and the day close per drawer (D140) needs that row
                // to exist here.
                //
                // It is also the only place D135's guarantee can be checked:
                // the uniqueness that stops two tills sharing a bill number is
                // shop-wide, and only the master sees the whole shop. A clash
                // is refused here, so the sender is told rather than the two
                // series quietly overlapping.
                let known = repos.terminals().find(OUTLET, &sender)?;
                if known.is_none()
                    || known.as_ref().is_some_and(|t| {
                        t.series_prefix != forwarded.series_prefix
                    })
                {
                    let mut row = known.unwrap_or_else(|| {
                        mb_db::repo::terminals::Terminal::new(
                            sender.clone(),
                            forwarded.terminal_name.clone(),
                            at,
                        )
                    });
                    row.name.clone_from(&forwarded.terminal_name);
                    row.series_prefix.clone_from(&forwarded.series_prefix);
                    if let Err(clash) = repos.terminals().save(OUTLET, &row, at) {
                        // Everything in the batch is refused, and with the
                        // sentence naming the other till — because storing half
                        // of it under a colliding series is the worse answer.
                        let says = clash.to_string();
                        return Ok(Receipt {
                            stored: Vec::new(),
                            refused: forwarded
                                .orders
                                .iter()
                                .map(|o| (id_of(o), says.clone()))
                                .collect(),
                            says,
                        });
                    }
                }

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
                // **Bills only, and asked for as bills.** Reading "the oldest
                // two hundred rows" and filtering here would eventually read two
                // hundred menu edits — nothing clears those until P33 — and the
                // shop's money would silently stop travelling.
                for row in repos.outbox().pending_in(Some("orders"), 200)? {
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

/// This till, describing itself, with what it is carrying.
pub fn from_here(app: &App, orders: Vec<serde_json::Value>) -> UiResult<Forwarded> {
    // `App`'s id, not the file's: this must be the id the bills in this batch
    // were actually written under, and that is the one the process started with.
    let id = app.terminal_id().to_owned();
    let mine = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| mb_db::Repos::new(tx).terminals().find(OUTLET, &id))
            .map_err(|e| words::from_db(&e))
    })?;
    Ok(Forwarded {
        terminal_id: id,
        terminal_name: mine
            .as_ref()
            .map_or_else(|| "This till".to_owned(), |t| t.name.clone()),
        series_prefix: mine.map(|t| t.series_prefix).unwrap_or_default(),
        orders,
    })
}

/// **Send everything this till is holding, and forget only what was taken.**
///
/// Retryable for ever and safe to call at any moment: the master is idempotent
/// on each order's id, so a batch that half-arrived and a batch sent twice end
/// in exactly the same place.
pub fn send_once(app: &App, master: &mb_lan::Master) -> UiResult<usize> {
    let orders = waiting_on(app)?;
    if orders.is_empty() {
        return Ok(0);
    }
    let batch = from_here(app, orders)?;
    let receipt = master.forward_blocking(&batch).map_err(|e| match e {
        // **"The master is off" is the ordinary state this feature is built
        // for** (D138), so it is not dressed up as a failure. The bills stay in
        // the queue, which is exactly where they should be.
        mb_lan::ClientError::Unreachable(_) => UiError::new(
            "forward.away",
            "The main till is not answering. Nothing was lost — these go across \
             as soon as it is back.",
        ),
        other => UiError::new("forward.refused", other.to_string()),
    })?;
    confirmed_on(app, &receipt)?;
    Ok(receipt.stored.iter().filter(|(_, ok)| *ok).count())
}

// ---------------------------------------------------------------------------
// The sender.
// ---------------------------------------------------------------------------

/// How often a till with something to send tries again.
///
/// **R7 is 2 s target, 10 s ceiling**, so this is two. It is a budget it is
/// allowed to have because forwarding is not on the billing path — D135 is what
/// bought that, and T11 is the proof.
const EAGER: std::time::Duration = std::time::Duration::from_secs(2);

/// And how often a till with nothing to send looks anyway.
///
/// Budget **M4** — idle CPU under 1 %. An idle secondary must not spend its
/// afternoon opening connections to a master that has nothing to take, so a
/// quiet tick is a read of the queue and no network at all — and on a master it
/// is one small file read, because it stops before even that.
const QUIET: std::time::Duration = std::time::Duration::from_secs(20);

/// **Start the thread that drains the queue.**
///
/// One thread, started once, that ends when the app does. It does nothing at
/// all on a master — `Me::master` is `None` there — so the shop that has one
/// till pays a `COUNT(*)` every twenty seconds and nothing else.
///
/// **Nothing here touches the settle path.** The queue is written by the settle
/// itself (the outbox row is inside its transaction), and this thread only
/// reads. A cashier can unplug the network mid-bill and the bill is unaffected.
pub fn start_sender(handle: &tauri::AppHandle) {
    let handle = handle.clone();
    let started = std::thread::Builder::new()
        .name("mb-forward".to_owned())
        .spawn(move || {
            loop {
                let Some(app) = handle.try_state::<crate::state::App>() else {
                    return; // Shutting down.
                };
                let waiting = sweep(&app, &handle);
                std::thread::sleep(if waiting { EAGER } else { QUIET });
            }
        });
    if let Err(e) = started {
        // Loud, because the failure is invisible otherwise: the till keeps
        // billing perfectly and its money never reaches the shop's book.
        crate::log_warn!("the thread that sends bills to the main till did not start: {e}");
    }
}

/// One turn of the sender. Returns whether anything is still waiting.
fn sweep(app: &crate::state::App, handle: &tauri::AppHandle) -> bool {
    let mine = crate::terminals::me(&crate::config::AppConfig::directory());
    let Some(link) = mine.master else {
        return false; // This is the master. Its bills are already here.
    };
    let waiting = waiting_on(app).unwrap_or_default();
    if waiting.is_empty() {
        announce(handle, 0);
        return false;
    }

    let sent = mb_lan::Master::pinned(&link.base, &link.certificate_pem)
        .map(|m| {
            m.as_device(mb_lan::Credential {
                device_id: link.device_id,
                secret: link.secret,
            })
        })
        .map_or(0, |master| send_once(app, &master).unwrap_or(0));

    // Count again rather than subtracting: a bill settled while that call was
    // in flight, and the banner must say what is true now.
    let left = waiting_on(app).map_or(waiting.len(), |w| w.len());
    announce(handle, left);
    if sent > 0 {
        crate::log_info!("{sent} bills went across to the main till, {left} still here");
    }
    left > 0
}

/// Tell the window what this till is holding (D138 — *a shop must be able to
/// see that the tills are apart*).
fn announce(handle: &tauri::AppHandle, waiting: usize) {
    use tauri::Emitter as _;
    let _ = handle.emit(
        crate::push::CHANNEL,
        crate::state::Pushed::Tills {
            waiting: crate::ipc::count(waiting as i64),
            says: waiting_says(waiting),
        },
    );
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

