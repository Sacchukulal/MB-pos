//! Bills travelling between tills.

use mb_core::AnyOrder;
use mb_lan::{Forwarded, Receipt};
use tauri::Manager as _;

use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

/// The master's side. Store what another till sent, once each.
pub fn receive_on(app: &App, forwarded: &Forwarded) -> UiResult<Receipt> {
    let at = crate::flows::now();
    let sender = forwarded.terminal_id.clone();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut stored = Vec::new();
                let mut refused = Vec::new();
                let mut touched = std::collections::BTreeSet::new();

                // The master learns about a till the first time it hears from one.
                let known = repos.terminals().find(OUTLET, &sender)?;
                if known.is_none()
                    || known
                        .as_ref()
                        .is_some_and(|t| t.series_prefix != forwarded.series_prefix)
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
                        // Everything in the batch is refused, and with the sentence naming the
                        // other till — because storing half of it under a colliding series is
                        // the worse answer.
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

                    let event = format!("forward:{id}");
                    if repos.events().recall(&event)?.is_some() {
                        // Already here. A repeat is a success — that is what lets the sender
                        // retry for ever without keeping track.
                        stored.push((id, true));
                        continue;
                    }

                    // The sender's terminal, not this one: a bill belongs to the drawer that
                    // took the money, which is what makes the day close per till tie across the
                    // shop.
                    repos.orders().save(OUTLET, &sender, &order)?;
                    if let AnyOrder::Settled(settled) = &order {
                        // Idempotent on its own account, so a re-send cannot deduct twice.
                        repos.stock().deduct_for_bill(OUTLET, settled, at)?;
                    }
                    repos
                        .events()
                        .remember(OUTLET, &event, "forward", "stored", at)?;
                    touched.insert(order.core().business_day);
                    stored.push((id, true));
                }

                // A bill that arrives after its day was closed is still that day's bill, and
                // refusing it would bounce it between two tills for ever. It is stored and the
                // day's frozen figures are made true again; the day stays closed.
                for day in touched {
                    repos.days().refreeze(OUTLET, day)?;
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

/// The secondary's side. Everything settled here that the master has not acknowledged yet.
pub fn waiting_on(app: &App) -> UiResult<Vec<serde_json::Value>> {
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut out = Vec::new();
                // Bills only, and asked for as bills.
                for row in repos.outbox().pending_in(Some("orders"), 200)? {
                    // Only what is FINISHED travels.
                    if let Some(order) = repos.orders().find(&mb_core::OrderId::new(row.row_id))?
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
pub fn confirmed_on(app: &App, receipt: &Receipt) -> UiResult<()> {
    let at = crate::flows::now();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                // The outbox is keyed by table and row, and `entry_id` is the one place that
                // key is spelled — deriving it here a second time is how a queue silently stops
                // clearing.
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
    // `App`'s id, not the file's: this must be the id the bills in this batch were actually
    // written under, and that is the one the process started with.
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

/// Send everything this till is holding, and forget only what was taken.
pub fn send_once(app: &App, master: &mb_lan::Master) -> UiResult<usize> {
    let orders = waiting_on(app)?;
    if orders.is_empty() {
        return Ok(0);
    }
    let batch = from_here(app, orders)?;
    let receipt = master.forward_blocking(&batch).map_err(|e| match e {
        // "The master is off" is the ordinary state this feature is built for, so it is not
        // dressed up as a failure.
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

// The sender.

/// How often a till with something to send tries again.
const EAGER: std::time::Duration = std::time::Duration::from_secs(2);

/// And how often a till with nothing to send looks anyway.
const QUIET: std::time::Duration = std::time::Duration::from_secs(20);

/// Start the thread that drains the queue.
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
        // Loud, because the failure is invisible otherwise: the till keeps billing perfectly
        // and its money never reaches the shop's book.
        crate::log_warn!("the thread that sends bills to the main till did not start: {e}");
    }
}

/// One turn of the sender.
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

    // Count again rather than subtracting: a bill settled while that call was in flight, and
    // the banner must say what is true now.
    let left = waiting_on(app).map_or(waiting.len(), |w| w.len());
    announce(handle, left);
    if sent > 0 {
        crate::log_info!("{sent} bills went across to the main till, {left} still here");
    }
    left > 0
}

/// Tell the window what this till is holding.
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
