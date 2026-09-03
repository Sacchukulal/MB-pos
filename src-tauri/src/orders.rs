//! Applying a phone's intent.

use mb_auth::Permission;
use mb_auth::audit::{AuditEntry, action};
use mb_core::{AnyOrder, Money, OrderId, Qty, StaffId, Timestamp};
use mb_lan::intent::{Intent, LineView, Outcome, What};
use serde::Serialize;
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::state::{App, OUTLET};
use crate::words::{self, UiResult};

/// How old a queued intent may be before a person has to release it.
pub const HOLD_AFTER_HOURS: i64 = 12;

/// What the floor did to an order the cashier has open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct FloorChange {
    /// The whole sentence: "Ravi added 2 Masala Dosa from the floor.".
    pub says: String,
    pub item_id: String,
    pub name: String,
    pub qty: String,
    pub note: Option<String>,
}

/// The result of applying one intent, plus what has to happen after the transaction commits.
#[derive(Debug)]
pub struct Applied {
    pub outcome: Outcome,
    /// Set when the cashier has this order open and must be told rather than overwritten.
    pub tell_the_cashier: Option<FloorChange>,
    /// The kitchen ticket the caller queues once the outcome is on disk: the order as
    /// sent, and the delta the kitchen had not seen.
    pub kitchen_paper: Option<(AnyOrder, Vec<(mb_core::LineIdentity, Qty)>)>,
}

/// Apply one intent.
pub fn apply(
    app: &App,
    device_id: &str,
    staff: &StaffId,
    permissions: &mb_auth::PermissionSet,
    intent: &Intent,
) -> UiResult<Applied> {
    let at = now();
    let day = today(at);

    // The permission, server-side, before anything else.
    if let Some(need) = needs(&intent.what)
        && !permissions.has(need)
    {
        return Ok(Applied {
            outcome: Outcome::Refused {
                message: format!(
                    "You do not have permission to {}. Ask somebody at the counter.",
                    intent.what.name()
                ),
            },
            tell_the_cashier: None,
            kitchen_paper: None,
        });
    }

    // Too old to apply without asking a person.
    if is_stale(intent.at, intent.sent_at) {
        return Ok(Applied {
            outcome: Outcome::Held {
                message: format!(
                    "This was typed more than {HOLD_AFTER_HOURS} hours ago and is \
                     waiting for somebody at the counter to say whether it still \
                     applies."
                ),
                batch_id: intent.id.clone(),
            },
            tell_the_cashier: None,
            kitchen_paper: None,
        });
    }

    let cashier_has = app
        .with_cart(|state| Ok(state.order_id().map(str::to_owned)))
        .unwrap_or_default();
    let config = app.shop_config();

    let applied = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);

                // Idempotency, in the same transaction as the effect.
                if let Some(before) = repos.events().recall(&intent.id)? {
                    return Ok(Applied {
                        outcome: serde_json::from_str(&before).unwrap_or(Outcome::Refused {
                            message: "This was already done, and the counter could \
                                      not read back what it said the first time."
                                .to_owned(),
                        }),
                        tell_the_cashier: None,
                        kitchen_paper: None,
                    });
                }

                let applied = do_it(
                    &repos,
                    intent,
                    staff,
                    at,
                    day,
                    cashier_has.as_deref(),
                    app.terminal_id(),
                    &config,
                )?;

                let recorded = serde_json::to_string(&applied.outcome).unwrap_or_default();
                repos
                    .events()
                    .remember(OUTLET, &intent.id, device_id, &recorded, at)?;

                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(staff.clone()),
                        action::INTENT_APPLIED,
                        "order",
                    )
                    .about(intent.order_id.clone().unwrap_or_default())
                    .with_after(serde_json::json!({
                        "device": device_id,
                        "did": intent.what.name(),
                        "outcome": applied.outcome.message(),
                    })),
                )?;
                Ok(applied)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    // The paper, after the outcome is durably on disk — the same road as the counter's own
    // ticket (`flows::queue_kitchen_lines`): grouped by station, one KOT number per roll.
    // A shop that switched the kitchen ticket off still gets the event and the kitchen
    // screen; it just prints nothing.
    if let Some((order, delta)) = &applied.kitchen_paper
        && !config.billing.kitchen_ticket_off
    {
        let core = order.core();
        if let Err(e) = crate::flows::queue_kitchen_lines(
            app,
            mb_print::template::TicketKind::New,
            core.order_type(),
            core.table(),
            Some(order),
            crate::flows::ticket_lines(&core.cart, delta),
            false,
            "an order from a phone".to_owned(),
        ) {
            crate::log_warn!(
                "order={} the kitchen was told but the ticket did not queue: {e}",
                core.id.as_str()
            );
        }
    }

    // The bill, carried to the table: the same paper the counter's own "Print bill" makes,
    // queued once the outcome is on disk. A phone asked for it; the waiter is on the way.
    if matches!(intent.what, What::RequestBill)
        && let Outcome::Ok { order_id, .. } = &applied.outcome
        && let Err(e) = crate::flows::print_bill_from_floor(app, order_id, staff)
    {
        crate::log_warn!(
            "order={order_id} the bill was asked for from a phone but did not queue: {e}"
        );
    }

    Ok(applied)
}

/// True when an intent has been sitting in a phone's pocket too long — measured on the
/// phone's own clock, typed-at against sent-at, so a phone that is hours wrong is still right
/// about how long it held the order. The counter's clock never judges a phone's: a tablet with
/// auto-time off once had every order it sent held as "last night's".
#[must_use]
pub fn is_stale(typed_at_ms: i64, sent_at_ms: Option<i64>) -> bool {
    // A phone that did not say when it sent is taken as sending now.
    let Some(sent_at_ms) = sent_at_ms else {
        return false;
    };
    let age_ms = sent_at_ms.saturating_sub(typed_at_ms);
    age_ms > HOLD_AFTER_HOURS.saturating_mul(60 * 60 * 1_000)
}

/// What each operation needs.
const fn needs(what: &What) -> Option<Permission> {
    match what {
        What::OpenOrder { .. }
        | What::AddItem { .. }
        | What::SetQty { .. }
        | What::SetOrderNote { .. }
        | What::SetCovers { .. }
        | What::SendToKitchen
        | What::RequestBill
        | What::RequestSettle { .. } => Some(Permission::BillCreate),
        // Taking something back is the permission the counter uses for the same act, and for
        // the same reason.
        What::VoidItem { .. } => Some(Permission::OrderItemVoid),
        What::CancelOrder { .. } => Some(Permission::OrderCancel),
        What::RequestDiscount { line: Some(_), .. } => Some(Permission::BillDiscountLine),
        What::RequestDiscount { line: None, .. } => Some(Permission::BillDiscountBill),
        What::SetCustomer { .. } => Some(Permission::CustomersManage),
        What::MoveTable { .. } => Some(Permission::BillCreate),
    }
}

fn refused(message: impl Into<String>) -> Applied {
    Applied {
        outcome: Outcome::Refused {
            message: message.into(),
        },
        tell_the_cashier: None,
        kitchen_paper: None,
    }
}

/// Which till this order is on, when there is more than one.
fn on_which_till(repos: &mb_db::Repos<'_>, order: &OrderId) -> Option<String> {
    if repos.terminals().count(OUTLET).ok()? < 2 {
        return None;
    }
    let id = repos.orders().terminal_of(order).ok()??;
    let name = repos.terminals().find(OUTLET, &id).ok()??.name;
    (!name.trim().is_empty()).then_some(name)
}

/// "at the counter", or "on Counter 2".
fn where_words(till: Option<&str>) -> String {
    match till {
        Some(name) => format!("on {name}"),
        None => "at the counter".to_owned(),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "everything applying an intent needs, and threading it through a \
              struct would only move the list somewhere less obvious"
)]
/// This deliberately does NOT take `&App`.
fn do_it(
    repos: &mb_db::Repos<'_>,
    intent: &Intent,
    staff: &StaffId,
    at: Timestamp,
    day: mb_core::BusinessDay,
    cashier_has: Option<&str>,
    // Which till this order belongs to.
    till: &str,
    config: &crate::settings::ShopConfig,
) -> Result<Applied, mb_db::DbError> {
    // Opening an order is the one intent with no order to load.
    if let What::OpenOrder {
        order_type,
        table_id,
        covers,
    } = &intent.what
    {
        return open_order(
            repos,
            order_type,
            table_id.as_deref(),
            *covers,
            staff,
            at,
            day,
            till,
            config,
        );
    }

    let Some(order_id) = intent.order_id.as_deref() else {
        return Ok(refused(
            "The phone did not say which order this is about. Open the table again.",
        ));
    };
    let found = repos.orders().find(&OrderId::new(order_id))?;

    // Conflict (e): the counter has already finished with it.
    let mut open = match found {
        Some(AnyOrder::Open(open)) => open,
        Some(AnyOrder::Settled(_)) => {
            let till = on_which_till(repos, &OrderId::new(order_id));
            return Ok(refused(format!(
                "That bill has already been paid {}. Start a new order for \
                 anything else.",
                where_words(till.as_deref())
            )));
        }
        Some(AnyOrder::Voided(_)) => {
            let till = on_which_till(repos, &OrderId::new(order_id));
            return Ok(refused(format!(
                "That bill was paid and then cancelled {}. Start a new order.",
                where_words(till.as_deref())
            )));
        }
        Some(AnyOrder::Cancelled(_)) => {
            let till = on_which_till(repos, &OrderId::new(order_id));
            return Ok(refused(format!(
                "That order was cancelled {}. Start a new one.",
                where_words(till.as_deref())
            )));
        }
        Some(AnyOrder::Draft(_)) | None => {
            return Ok(refused(
                "That order is not on the counter any more. Open the table again.",
            ));
        }
    };

    let mut tell_the_cashier = None;
    let mut note = None;
    let mut kitchen_delta = None;

    match &intent.what {
        What::OpenOrder { .. } => unreachable!("handled above"),

        What::AddItem {
            item_id,
            qty,
            note: line_note,
            ..
        } => {
            let Some(item) = repos
                .menu()
                .find_item(&mb_core::ItemId::new(item_id.clone()))?
            else {
                return Ok(refused(
                    "That item is not on this shop's menu any more. Ask the counter.",
                ));
            };
            if !item.is_available {
                return Ok(refused(format!(
                    "{} has run out. The counter took it off the menu.",
                    item.name
                )));
            }
            let Ok(qty) = Qty::parse(qty) else {
                return Ok(refused("That quantity could not be read. Try 1, 2 or 0.5."));
            };
            // The counter's own price, frozen here.
            if open
                .core
                .cart
                .add(
                    item.snapshot(&config.tax)?,
                    qty,
                    line_note.clone(),
                    vec![],
                )
                .is_err()
            {
                return Ok(refused("That item could not be added to the order."));
            }
            if cashier_has == Some(order_id) {
                tell_the_cashier = Some(FloorChange {
                    says: format!("The floor added {qty} {} to this table.", item.name),
                    item_id: item_id.clone(),
                    name: item.name.clone(),
                    qty: qty.to_string(),
                    note: line_note.clone(),
                });
            }
        }

        What::SetQty { line, qty } => {
            let Ok(qty) = Qty::parse(qty) else {
                return Ok(refused("That quantity could not be read."));
            };
            // Conflict (c): the kitchen has already cooked some of it.
            if let Some(cart_line) = open.core.cart.lines().get(*line) {
                let told = open.core.kitchen.quantity_told(&cart_line.identity());
                if qty < told {
                    return Ok(refused(format!(
                        "The kitchen has already been told about {told} of these. \
                         Ask the counter to void it instead."
                    )));
                }
            }
            if open.core.cart.set_qty(*line, qty).is_err() {
                return Ok(refused("That line is not on the order any more."));
            }
        }

        What::VoidItem { line, reason } => {
            if reason.trim().is_empty() {
                return Ok(refused("A voided item needs a reason."));
            }
            let Some(cart_line) = open.core.cart.lines().get(*line).cloned() else {
                return Ok(refused("That line is not on the order any more."));
            };
            // Conflict (c) again, and this is the important half: voiding something the kitchen
            // COOKED is a decision with a cost, so it goes to the counter rather than being
            // done from the floor.
            if !open
                .core
                .kitchen
                .quantity_told(&cart_line.identity())
                .is_zero()
            {
                return Ok(refused(
                    "The kitchen has already made this. Ask somebody at the \
                     counter to take it off.",
                ));
            }
            if open.core.cart.remove(*line).is_err() {
                return Ok(refused("That line could not be removed."));
            }
        }

        What::SetOrderNote { note: order_note } => {
            open.core.note = order_note.clone().filter(|n| !n.trim().is_empty());
        }
        What::SetCovers { covers } => open.core.covers = *covers,
        What::SetCustomer { .. } => {
            return Ok(refused(
                "Putting a bill on somebody's account is done at the counter.",
            ));
        }

        What::RequestDiscount { .. } => {
            return Ok(refused(
                "Discounts are given at the counter. Ask the cashier.",
            ));
        }

        What::SendToKitchen => {
            // The counter decides the delta.
            let pending = open
                .core
                .kitchen
                .pending(&open.core.cart)
                .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
            if pending.is_empty() {
                note = Some("The kitchen already has everything on this order.".to_owned());
            } else {
                let how_many = pending.len();
                open.core
                    .kitchen
                    .mark_printed(&pending)
                    .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
                note = Some(format!(
                    "{} sent to the kitchen.",
                    words::count(i64::try_from(how_many).unwrap_or(0), "item", "items")
                ));
                kitchen_delta = Some(pending);
            }
        }

        What::MoveTable { table_id } => {
            // A table makes it dine-in, whatever it was.
            open.core.placement =
                mb_core::Placement::on_table(mb_core::TableId::new(table_id.clone()));
        }

        What::CancelOrder { reason } => {
            if reason.trim().is_empty() {
                return Ok(refused("A cancelled order needs a reason."));
            }
            let cancelled = open
                .clone()
                .cancel(reason, staff.clone(), at)
                .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
            repos
                .orders()
                .save(OUTLET, till, &AnyOrder::Cancelled(cancelled))?;
            return Ok(Applied {
                outcome: Outcome::Ok {
                    order_id: order_id.to_owned(),
                    total: Money::ZERO.to_plain_string(),
                    lines: vec![],
                    token: Some(open.token.formatted.clone()),
                    note: Some("The order was cancelled.".to_owned()),
                },
                tell_the_cashier: None,
                kitchen_paper: None,
            });
        }

        What::RequestBill => {
            if open.core.cart.is_empty() {
                return Ok(refused("There is nothing on this order to bill yet."));
            }
            // Written down, so the floor tile says "bill asked" until the table is settled.
            repos.events().record(
                order_id,
                at,
                day,
                mb_db::repo::events::BILL_ASKED,
                Some(staff),
                None,
            )?;
            let label = open.core.table().and_then(|t| {
                repos
                    .floor()
                    .list_tables(OUTLET)
                    .ok()?
                    .into_iter()
                    .find(|row| &row.id == t)
                    .map(|row| row.label)
            });
            note = Some(match label {
                Some(label) => format!("The bill for table {label} is printing at the counter."),
                None => "The bill is printing at the counter.".to_owned(),
            });
        }

        What::RequestSettle { payment } => {
            if open.core.cart.is_empty() {
                return Ok(refused("There is nothing on this order to settle yet."));
            }
            // Written down with what the waiter was handed, so the desk on the counter's
            // screen can offer that mode first. The cashier confirms; nothing is settled here.
            let payment = payment
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_lowercase);
            let detail = serde_json::json!({ "payment": payment }).to_string();
            repos.events().record(
                order_id,
                at,
                day,
                mb_db::repo::events::SETTLE_ASKED,
                Some(staff),
                Some(&detail),
            )?;
            let label = open.core.table().and_then(|t| {
                repos
                    .floor()
                    .list_tables(OUTLET)
                    .ok()?
                    .into_iter()
                    .find(|row| &row.id == t)
                    .map(|row| row.label)
            });
            note = Some(match label {
                Some(label) => format!(
                    "The counter has been asked to settle table {label}. Somebody there confirms it."
                ),
                None => "The counter has been asked to settle this bill. Somebody there confirms it."
                    .to_owned(),
            });
        }
    }

    repos
        .orders()
        .save(OUTLET, till, &AnyOrder::Open(open.clone()))?;

    // The kitchen is told in the SAME commit as the outcome — the event and the kitchen
    // screen, exactly as the counter's own ticket records them. The paper itself needs
    // `&App`, so `apply` queues it once this transaction is on disk.
    let kitchen_paper = match kitchen_delta {
        Some(delta) => {
            repos.events().record(
                order_id,
                at,
                day,
                mb_db::repo::events::KITCHEN_TICKET,
                None,
                None,
            )?;
            crate::kitchen::send_in(repos, config, order_id, None, at)?;
            Some((AnyOrder::Open(open.clone()), delta))
        }
        None => None,
    };

    Ok(Applied {
        outcome: view_of(&open, note, config),
        tell_the_cashier,
        kitchen_paper,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "everything opening an order needs, and a struct would only move \
              the list somewhere less obvious"
)]
fn open_order(
    repos: &mb_db::Repos<'_>,
    order_type: &str,
    table_id: Option<&str>,
    covers: Option<u32>,
    staff: &StaffId,
    at: Timestamp,
    day: mb_core::BusinessDay,
    till: &str,
    config: &crate::settings::ShopConfig,
) -> Result<Applied, mb_db::DbError> {
    let Ok(order_type) = mb_db::encode::order_type_from_sql(order_type) else {
        return Ok(refused("That is not an order type this counter knows."));
    };
    let placement = match mb_core::Placement::new(
        order_type,
        table_id.map(|t| mb_core::TableId::new(t.to_owned())),
        None,
    ) {
        Ok(placement) => placement,
        Err(mb_core::OrderError::TableRequired) => {
            return Ok(refused(
                "A dine-in order needs a table. Pick one and try again.",
            ));
        }
        Err(_) => {
            return Ok(refused("Only a dine-in order sits at a table."));
        }
    };

    // A table this shop does not have.
    if let Some(table) = table_id
        && !repos
            .floor()
            .list_tables(OUTLET)?
            .iter()
            .any(|t| t.id.as_str() == table)
    {
        return Ok(refused(
            "That table is not on this shop's floor any more. Pull down to \
             refresh, and open it again.",
        ));
    }

    // Conflict (a): two waiters open the same table at once.
    if let Some(table) = table_id {
        let existing = repos
            .orders()
            .list_open(OUTLET)?
            .into_iter()
            .find(|o| o.core().table().is_some_and(|t| t.as_str() == table));
        if let Some(AnyOrder::Open(open)) = existing {
            let till = on_which_till(repos, &open.core.id);
            let says = match till {
                Some(name) => format!(
                    "This table is already open on {name}. You are both on the \
                     same order."
                ),
                None => "Somebody had already opened this table. You are both on \
                         the same order."
                    .to_owned(),
            };
            return Ok(Applied {
                outcome: view_of(&open, Some(says), config),
                tell_the_cashier: None,
                kitchen_paper: None,
            });
        }
    }

    let mut draft = mb_core::DraftOrder::new(
        mb_core::OrderId::new(crate::newid::fresh_at("ord", at)),
        day,
        at,
        placement,
        staff.clone(),
    );
    if let Some(covers) = covers {
        draft = draft.with_covers(covers);
    }

    // The counter claims the numbers, atomically, in THIS transaction.
    let token = mb_db::numbering::claim(
        repos.tx(),
        OUTLET,
        till,
        mb_db::numbering::CounterKind::Token,
        day,
    )?;
    let bill_number = mb_db::numbering::claim(
        repos.tx(),
        OUTLET,
        till,
        mb_db::numbering::CounterKind::Bill,
        day,
    )?;
    let open = mb_core::OpenOrder {
        core: draft.core,
        token,
        bill_number,
    };
    repos
        .orders()
        .save(OUTLET, till, &AnyOrder::Open(open.clone()))?;

    Ok(Applied {
        outcome: view_of(&open, None, config),
        tell_the_cashier: None,
        kitchen_paper: None,
    })
}

/// The counter's own view of the order — the same bill the counter would print.
fn view_of(
    open: &mb_core::OpenOrder,
    note: Option<String>,
    config: &crate::settings::ShopConfig,
) -> Outcome {
    let total = crate::billing::bill_for(&open.core.cart, open.core.order_type(), None, config)
        .map_or(Money::ZERO, |bill| bill.grand_total);

    Outcome::Ok {
        order_id: open.core.id.as_str().to_owned(),
        total: total.to_plain_string(),
        lines: open
            .core
            .cart
            .lines()
            .iter()
            .enumerate()
            .map(|(index, line)| LineView {
                line: index,
                name: line.snapshot.name.clone(),
                qty: line.qty.to_string(),
                amount: line
                    .qty
                    .extend(line.snapshot.unit_price)
                    .unwrap_or(Money::ZERO)
                    .to_plain_string(),
                note: line.note.clone(),
                sent_to_kitchen: !open.core.kitchen.quantity_told(&line.identity()).is_zero(),
            })
            .collect(),
        token: Some(open.token.formatted.clone()),
        note,
    }
}

/// The floor for the phones: which tables are taken, and every open order as a phone would be
/// told it — the same lines and total an intent's outcome carries. Sent as the `floor` push
/// after any change, and answered at `GET /v1/floor`. A phone shows only the orders it has
/// touched; an order missing from this list is one the counter has finished with.
pub fn floor_body(app: &App) -> UiResult<serde_json::Value> {
    let config = app.shop_config();
    let at = now();
    // The same two thresholds the counter's own tiles use, so a phone turns a table amber and
    // red at the same minute the counter does.
    let (warn_minutes, late_minutes) = crate::floor::thresholds(app)?;
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let tables = repos.floor().list_tables(OUTLET)?;
                let open = repos.orders().list_open(OUTLET)?;
                // Which orders asked for their bill, and who opened each — the phones show
                // both, and the words are the counter's.
                let asked = repos
                    .events()
                    .last_for_each(mb_db::repo::events::BILL_ASKED)?;
                let settles = repos.events().latest_of_two(
                    mb_db::repo::events::SETTLE_ASKED,
                    mb_db::repo::events::SETTLE_DECLINED,
                )?;
                let settle_asked = |order_id: &str| {
                    settles
                        .iter()
                        .any(|e| e.order_id == order_id && e.event == mb_db::repo::events::SETTLE_ASKED)
                };
                let people = repos.people().list_staff(OUTLET)?;
                let name_of = |id: &StaffId| {
                    people
                        .iter()
                        .find(|p| &p.id == id)
                        .map(|p| p.name.clone())
                };
                let label_of = |id: &mb_core::TableId| {
                    tables
                        .iter()
                        .find(|t| &t.id == id)
                        .map(|t| t.label.clone())
                };
                let on_table = |id: &mb_core::TableId| {
                    open.iter()
                        .find(|o| o.core().table() == Some(id))
                        .map(|o| o.core().id.as_str().to_owned())
                };
                let bill_asked = |order_id: &str| asked.iter().any(|(id, _)| id == order_id);
                let table_rows: Vec<serde_json::Value> = tables
                    .iter()
                    .map(|t| {
                        let order_id = on_table(&t.id);
                        let state = match &order_id {
                            Some(id) if bill_asked(id) => "bill_asked",
                            Some(_) => "taken",
                            None => "free",
                        };
                        serde_json::json!({
                            "id": t.id.as_str(),
                            "state": state,
                            "order_id": order_id,
                        })
                    })
                    .collect();
                let order_rows: Vec<serde_json::Value> = open
                    .iter()
                    .filter_map(|o| match o {
                        mb_core::AnyOrder::Open(open) => Some(open),
                        _ => None,
                    })
                    .map(|open| {
                        let table_id = open.core.table().map(|t| t.as_str().to_owned());
                        let table_label = open.core.table().and_then(label_of);
                        let order_type = serde_json::to_value(open.core.order_type())
                            .ok()
                            .and_then(|v| v.as_str().map(ToOwned::to_owned))
                            .unwrap_or_default();
                        match view_of(open, None, &config) {
                            Outcome::Ok {
                                order_id,
                                total,
                                lines,
                                token,
                                note,
                            } => serde_json::json!({
                                "order_id": order_id,
                                "table_id": table_id,
                                "table_label": table_label,
                                "order_type": order_type,
                                "total": total,
                                "token": token,
                                "note": note.or_else(|| open.core.note.clone()),
                                "lines": lines,
                                "bill_asked": bill_asked(open.core.id.as_str()),
                                "settle_asked": settle_asked(open.core.id.as_str()),
                                // How long the table has been sitting, by the counter's clock —
                                // the same number its own tile shows.
                                "minutes": (at.millis() - open.core.created_at.millis()).div_euclid(60_000).max(0),
                                "by": name_of(&open.core.created_by),
                                "by_id": open.core.created_by.as_str(),
                            }),
                            _ => serde_json::Value::Null,
                        }
                    })
                    .filter(|v| !v.is_null())
                    .collect();
                Ok(serde_json::json!({
                    "tables": table_rows,
                    "orders": order_rows,
                    "warn_minutes": warn_minutes,
                    "late_minutes": late_minutes,
                }))
            })
            .map_err(|e| words::from_db(&e))
    })
}

// Offline: a batch of intents a phone queued while it could not reach us.

/// Apply a whole batch, in order.
pub fn apply_batch(
    app: &App,
    device_id: &str,
    staff: &StaffId,
    permissions: &mb_auth::PermissionSet,
    batch: &mb_lan::Batch,
) -> UiResult<mb_lan::BatchResult> {
    let mut outcomes = Vec::with_capacity(batch.intents.len());
    let (mut ok, mut refused_count, mut held) = (0_i64, 0_i64, 0_i64);

    // A whole order in ONE batch: the phone cannot know the id of an order the same batch is
    // about to open, so an intent with no order id applies to the order this batch opened.
    // A replayed batch re-runs the open with the same intent id, gets the ORIGINAL outcome and
    // so the same order id (§6) — the adds land on the same order, never a second one.
    let mut opened: Option<String> = None;

    for intent in &batch.intents {
        let borrowed;
        let intent = if intent.order_id.is_none()
            && !matches!(intent.what, mb_lan::intent::What::OpenOrder { .. })
            && opened.is_some()
        {
            borrowed = mb_lan::Intent {
                id: intent.id.clone(),
                order_id: opened.clone(),
                at: intent.at,
                sent_at: intent.sent_at,
                what: intent.what.clone(),
            };
            &borrowed
        } else {
            intent
        };
        let applied = apply(app, device_id, staff, permissions, intent)?;
        if matches!(intent.what, mb_lan::intent::What::OpenOrder { .. })
            && let Outcome::Ok { order_id, .. } = &applied.outcome
        {
            opened = Some(order_id.clone());
        }
        match &applied.outcome {
            Outcome::Ok { .. } => ok += 1,
            Outcome::Refused { .. } => refused_count += 1,
            Outcome::Held { .. } => held += 1,
        }
        if let Some(change) = applied.tell_the_cashier {
            app.note_floor_change(change);
        }
        outcomes.push((intent.id.clone(), applied.outcome));
    }

    // The whole thing in one sentence, written here (§6) because the phone shows it and must
    // not assemble it. A waiter's batch is one order: what they hear is what the kitchen was
    // told — "6 items sent to the kitchen." — never a count of "order changes".
    let _ = ok;
    let mut says = outcomes
        .iter()
        .rev()
        .find_map(|(_, outcome)| match outcome {
            Outcome::Ok { note: Some(note), .. } if !note.trim().is_empty() => {
                Some(note.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            if refused_count + held == 0 {
                "Done.".to_owned()
            } else {
                String::new()
            }
        });
    if held > 0 {
        if !says.is_empty() {
            says.push(' ');
        }
        says.push_str(&format!(
            "{} waiting for somebody at the counter to say whether they still apply.",
            words::count(held, "change is", "changes are")
        ));
    }
    if refused_count > 0 {
        if !says.is_empty() {
            says.push(' ');
        }
        says.push_str(&format!(
            "{} could not be done — open the queue to see why.",
            words::count(refused_count, "change", "changes")
        ));
    }

    Ok(mb_lan::BatchResult { outcomes, says })
}

// The catalogue.

/// What a phone needs to take an order, with a version.
pub fn catalogue(app: &App) -> UiResult<mb_lan::Catalogue> {
    let (items, categories, sections, tables) = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                Ok((
                    // Every item, including the ones that ran out — `false` and not `true`.
                    repos.menu().list_items(OUTLET, false)?,
                    repos.menu().list_categories(OUTLET)?,
                    repos.floor().list_sections(OUTLET)?,
                    repos.floor().list_tables(OUTLET)?,
                ))
            })
            .map_err(|e| words::from_db(&e))
    })?;

    let mut fingerprint = String::new();
    let list: Vec<mb_lan::intent::CatalogueItem> = items
        .iter()
        .map(|item| {
            let one = mb_lan::intent::CatalogueItem {
                id: item.id.as_str().to_owned(),
                name: item.name.clone(),
                // The category's NAME — a waiter reads "Tiffin", never an id. A dish whose
                // category is gone or inactive simply has none.
                category: item
                    .category_id
                    .as_ref()
                    .and_then(|c| categories.iter().find(|cat| cat.id.as_str() == c.as_str()))
                    .map(|cat| cat.name.clone())
                    .unwrap_or_default(),
                price: item.unit_price.to_plain_string(),
                is_available: item.is_available,
            };
            fingerprint.push_str(&one.id);
            fingerprint.push_str(&one.name);
            // The category is part of what a phone can SEE, so renaming one must change the
            // version — or every paired phone keeps showing the old grouping for ever.
            fingerprint.push_str(&one.category);
            fingerprint.push_str(&one.price);
            fingerprint.push(if one.is_available { 'y' } else { 'n' });
            one
        })
        .collect();

    let mut rooms = Vec::new();
    for table in &tables {
        fingerprint.push_str(table.id.as_str());
        fingerprint.push_str(&table.label);
        rooms.push(mb_lan::intent::CatalogueTable {
            id: table.id.as_str().to_owned(),
            label: table.label.clone(),
            section: sections
                .iter()
                .find(|s| Some(&s.id) == table.section_id.as_ref())
                .map(|s| s.name.clone())
                .unwrap_or_default(),
            seats: u32::try_from(table.seats).unwrap_or(0),
            // The floor screen owns what a table IS doing; the catalogue only says what tables
            // EXIST.
            state: "free".to_owned(),
        });
    }

    let digest = mb_auth::sha256(fingerprint.as_bytes());
    let version: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();

    Ok(mb_lan::Catalogue {
        version,
        items: list,
        tables: rooms,
    })
}

// The cashier's side of a floor change.

/// Take the floor's items into the cart the cashier is typing.
pub fn take_the_floors_items_on(app: &App) -> UiResult<crate::billing::CartView> {
    // One counter action at a time — see `App::begin_action`.
    let _one_at_a_time = app.begin_action();
    guard::require(app, Permission::BillCreate)?;
    let changes = app.with_cart(|state| Ok(state.from_the_floor.clone()))?;

    // Every menu lookup FIRST, outside the cart lock — `find_menu_item` takes the shop lock,
    // and taking two locks in two orders in one product is how a till freezes at eight o'clock
    // on a Saturday.
    let config = app.shop_config();
    let mut resolved = Vec::new();
    for change in &changes {
        let item = app.find_menu_item(&change.item_id)?;
        let qty = Qty::parse(&change.qty).unwrap_or(Qty::ZERO);
        resolved.push((
            crate::billing::snapshot_for(&item, &config.tax)?,
            qty,
            change.note.clone(),
        ));
    }

    app.with_cart_mut(|state| {
        for (snapshot, qty, note) in resolved {
            state
                .cart
                .add(snapshot, qty, note, vec![])
                .map_err(|e| {
                    crate::words::UiError::new(
                        "cart.add",
                        "The floor's items could not be added to this bill.",
                    )
                    .with_detail(e.to_string())
                })?;
        }
        state.from_the_floor.clear();
        crate::billing::cart_view(state, &app.shop_config())
    })
}

/// The cashier looked and decided not to take them.
pub fn dismiss_the_floors_items_on(app: &App) -> UiResult<crate::billing::CartView> {
    guard::require(app, Permission::BillCreate)?;
    app.with_cart_mut(|state| {
        state.from_the_floor.clear();
        crate::billing::cart_view(state, &app.shop_config())
    })
}

#[tauri::command]
pub fn take_the_floors_items(app: tauri::State<'_, App>) -> UiResult<crate::billing::CartView> {
    take_the_floors_items_on(&app)
}

#[tauri::command]
pub fn dismiss_the_floors_items(app: tauri::State<'_, App>) -> UiResult<crate::billing::CartView> {
    dismiss_the_floors_items_on(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_intent_from_yesterday_is_held_and_one_from_now_is_not() {
        let sent = 50 * 60 * 60 * 1_000;
        let hour = 60 * 60 * 1_000;

        assert!(!is_stale(sent, Some(sent)), "an intent typed now is stale");
        assert!(!is_stale(sent - 11 * hour, Some(sent)));
        assert!(is_stale(sent - 13 * hour, Some(sent)));

        // The phone's clock is all that is read: only the gap between typing and sending.
        assert!(!is_stale(sent + 5 * hour, Some(sent)));
        assert!(!is_stale(sent - 13 * hour, None));
    }

    /// Every operation has had its permission decided — the same rule `guard::COMMAND_ACCESS`
    /// enforces for the counter's own commands.
    #[test]
    fn every_operation_has_a_permission_decided() {
        let all = [
            What::OpenOrder {
                order_type: "dine_in".to_owned(),
                table_id: None,
                covers: None,
            },
            What::AddItem {
                item_id: "i".to_owned(),
                qty: "1".to_owned(),
                note: None,
                modifiers: vec![],
            },
            What::SetQty {
                line: 0,
                qty: "1".to_owned(),
            },
            What::VoidItem {
                line: 0,
                reason: "r".to_owned(),
            },
            What::SetOrderNote { note: None },
            What::SetCustomer { customer_id: None },
            What::SetCovers { covers: None },
            What::RequestDiscount {
                line: Some(0),
                percent_bp: 1000,
                reason: "r".to_owned(),
            },
            What::RequestDiscount {
                line: None,
                percent_bp: 1000,
                reason: "r".to_owned(),
            },
            What::SendToKitchen,
            What::MoveTable {
                table_id: "t".to_owned(),
            },
            What::CancelOrder {
                reason: "r".to_owned(),
            },
            What::RequestBill,
            What::RequestSettle { payment: None },
        ];
        for what in &all {
            assert!(
                needs(what).is_some(),
                "{} has no permission decided, so a paired phone could do it",
                what.name()
            );
        }
        // And taking something back needs the same permission the counter's own screen asks
        // for.
        assert_eq!(
            needs(&What::VoidItem {
                line: 0,
                reason: String::new()
            }),
            Some(Permission::OrderItemVoid)
        );
        assert_eq!(
            needs(&What::CancelOrder {
                reason: String::new()
            }),
            Some(Permission::OrderCancel)
        );
    }
}

// The settle desk: what the phones asked the counter to settle, and the cashier's answer.

/// One request on the desk — a waiter asked, from a phone, for this bill to be settled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SettleRequestView {
    pub order_id: String,
    /// "Table 4", "Parcel", "Self service".
    pub place: String,
    pub token: Option<String>,
    /// The running total, formatted.
    pub total: String,
    /// Who asked, by name.
    pub waiter: String,
    /// The counter's own mode word — "Cash", "Card", "UPI" — when the waiter said what they
    /// were handed; `None` when they did not.
    pub payment: Option<String>,
    /// Minutes since they asked.
    pub minutes: u32,
    /// The whole sentence: "Ravi asks to settle table 4 — 840.00, paid by cash."
    pub says: String,
}

/// The counter's word for what a phone said it was handed.
fn payment_word(detail: Option<&str>) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(detail?).ok()?;
    match parsed.get("payment")?.as_str()? {
        "cash" => Some("Cash".to_owned()),
        "card" => Some("Card".to_owned()),
        "upi" => Some("UPI".to_owned()),
        _ => None,
    }
}

/// Every settle request still waiting, oldest first.
pub fn settle_requests_on(app: &App) -> UiResult<Vec<SettleRequestView>> {
    guard::require(app, Permission::BillCreate)?;
    let at = now();
    let config = app.shop_config();
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut asked: Vec<_> = repos
                    .events()
                    .latest_of_two(
                        mb_db::repo::events::SETTLE_ASKED,
                        mb_db::repo::events::SETTLE_DECLINED,
                    )?
                    .into_iter()
                    .filter(|e| e.event == mb_db::repo::events::SETTLE_ASKED)
                    .collect();
                asked.sort_by_key(|e| e.at.millis());
                if asked.is_empty() {
                    return Ok(Vec::new());
                }
                let open = repos.orders().list_open(OUTLET)?;
                let tables = repos.floor().list_tables(OUTLET)?;
                let people = repos.people().list_staff(OUTLET)?;
                let mut out = Vec::new();
                for e in asked {
                    let Some(order) = open.iter().find(|o| o.core().id.as_str() == e.order_id) else {
                        // Settled or gone since: nothing left to ask.
                        continue;
                    };
                    let AnyOrder::Open(open_order) = order else { continue };
                    let place = match order.core().table() {
                        Some(t) => format!(
                            "Table {}",
                            tables
                                .iter()
                                .find(|row| &row.id == t)
                                .map_or_else(|| t.as_str().to_owned(), |row| row.label.clone())
                        ),
                        None => crate::billing::order_type_label(order.core().order_type()).to_owned(),
                    };
                    let waiter = e
                        .staff_id
                        .as_deref()
                        .and_then(|id| people.iter().find(|p| p.id.as_str() == id))
                        .map_or_else(|| "The floor".to_owned(), |p| p.name.clone());
                    let (total, token) = match view_of(open_order, None, &config) {
                        Outcome::Ok { total, token, .. } => (total, token),
                        _ => continue,
                    };
                    let payment = payment_word(e.detail.as_deref());
                    let paid = payment
                        .as_deref()
                        .map(|p| format!(", paid by {}", p.to_lowercase()))
                        .unwrap_or_default();
                    let says = format!("{waiter} asks to settle {} — {total}{paid}.", place.to_lowercase());
                    out.push(SettleRequestView {
                        order_id: e.order_id.clone(),
                        place,
                        token,
                        total,
                        waiter,
                        payment,
                        minutes: crate::ipc::count((at.millis() - e.at.millis()).div_euclid(60_000).max(0)),
                        says,
                    });
                }
                Ok(out)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// The cashier confirmed a request: the bill completes in `mode`, exactly as if they had
/// opened the order and pressed "Complete bill". Whatever the cashier was typing is kept —
/// parked first, brought back after — so a request never costs them their own bill.
pub fn settle_from_floor_on(app: &App, order_id: String, mode: String) -> UiResult<String> {
    guard::require(app, Permission::BillCreate)?;
    let (loaded, has_lines) = app.with_cart(|state| {
        Ok((state.order_id().map(str::to_owned), !state.cart.is_empty()))
    })?;
    let bring_back = if loaded.as_deref() == Some(order_id.as_str()) {
        None
    } else if has_lines {
        Some(crate::flows::park_open_order(app)?.core.id.as_str().to_owned())
    } else {
        loaded
    };
    crate::ipc::open_order_on(app, order_id.clone())?;
    let number = crate::flows::complete_bill_on(app, Some(mode))?;
    crate::log_info!("order={order_id} settled from the floor's request as bill {number}");
    if let Some(id) = bring_back
        && id != order_id
        && let Err(e) = crate::ipc::open_order_on(app, id.clone())
    {
        // Their bill is on disk as an open order; the Processing list has it.
        crate::log_warn!("order={id} could not be brought back to the cart after a settle: {}", e.message);
    }
    Ok(number)
}

/// The cashier said no: the request leaves the desk and the tile goes quiet. The waiter sees
/// the tile change; a word to them is the cashier's to say.
pub fn decline_settle_on(app: &App, order_id: String) -> UiResult<()> {
    let who = guard::require(app, Permission::BillCreate)?;
    let at = now();
    let day = today(at);
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.events().record(
                    &order_id,
                    at,
                    day,
                    mb_db::repo::events::SETTLE_DECLINED,
                    Some(&who.staff_id),
                    None,
                )
            })
            .map_err(|e| words::from_db(&e))
    })
}

#[tauri::command]
pub fn settle_requests(app: tauri::State<'_, App>) -> UiResult<Vec<SettleRequestView>> {
    settle_requests_on(&app)
}

#[tauri::command]
pub fn settle_from_floor(app: tauri::State<'_, App>, order_id: String, mode: String) -> UiResult<String> {
    settle_from_floor_on(&app, order_id, mode)
}

#[tauri::command]
pub fn decline_settle(app: tauri::State<'_, App>, order_id: String) -> UiResult<()> {
    decline_settle_on(&app, order_id)
}
