//! **Applying a phone's intent** — P20, decision D9.
//!
//! # The counter is the authority
//!
//! The phone asks; this file decides. Every number, every rupee, every
//! decision about what the kitchen has been told is made here, on the counter,
//! against the stored order — never on the phone and never from a figure a
//! phone sent.
//!
//! `mb-lan` carries the message and knows nothing about any of it (its
//! `Counter` trait is the seam). This is the other side.
//!
//! # Idempotency, and why the order of operations is load-bearing
//!
//! A waiter on a flaky connection retries. A phone that loses a reply cannot
//! know whether its intent landed. So every intent carries a client-generated
//! id, and:
//!
//! 1. the id is checked, the effect applied, the outcome recorded and the
//!    audit row written **inside ONE transaction**;
//! 2. a repeat returns the **original outcome**, byte for byte — not "already
//!    done", because the phone is going to show it to a waiter;
//! 3. the broadcast to other phones happens **after** the commit, because a
//!    broadcast inside the transaction holds the writer open for as long as
//!    the slowest socket, and the writer belongs to the till.
//!
//! Recording the id before the effect would swallow an intent on a crash;
//! recording it after would apply one twice. That is R11's rule — the row goes
//! in the same transaction as the thing it records — applied to the network.
//!
//! # And the cashier is never clobbered
//!
//! If a phone changes the order the cashier has open on screen, this file does
//! **not** touch the cashier's cart. It records what the floor did, and the
//! screen offers to take it in. v1 learned that the hard way; see
//! [`FloorChange`] and D83.

use mb_auth::audit::{AuditEntry, action};
use mb_auth::Permission;
use mb_core::{AnyOrder, Money, OrderId, Qty, StaffId, Timestamp};
use mb_lan::intent::{Intent, LineView, Outcome, What};
use serde::Serialize;
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::state::{App, OUTLET};
use crate::words::{self, UiResult};

/// How old a queued intent may be before a person has to release it.
///
/// **Twelve hours, and the reason is v1's 7 a.m. problem**: a phone that was
/// offline all evening reconnecting when the shop opens would silently print
/// yesterday's tickets into a kitchen that is making breakfast. Twelve hours
/// spans one service and not two.
pub const HOLD_AFTER_HOURS: i64 = 12;

/// What the floor did to an order the cashier has open.
///
/// Kept beside the cart rather than merged into it: **the cashier's unsaved
/// typing is theirs**, and a phone that adds a dosa must not silently rewrite
/// what somebody is halfway through settling. The screen shows this and offers
/// to take it in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct FloorChange {
    /// The whole sentence: "Ravi added 2 Masala Dosa from the floor."
    pub says: String,
    pub item_id: String,
    pub name: String,
    pub qty: String,
    pub note: Option<String>,
}

/// The result of applying one intent, plus what has to happen after the
/// transaction commits.
#[derive(Debug)]
pub struct Applied {
    pub outcome: Outcome,
    /// Set when the cashier has this order open and must be told rather than
    /// overwritten.
    pub tell_the_cashier: Option<FloorChange>,
}

/// **Apply one intent.**
///
/// # Errors
///
/// Only when the database itself will not answer. A business refusal is an
/// [`Outcome::Refused`] with a sentence, not an error — the phone shows it to
/// a waiter and does not retry.
pub fn apply(
    app: &App,
    device_id: &str,
    staff: &StaffId,
    permissions: &mb_auth::PermissionSet,
    intent: &Intent,
) -> UiResult<Applied> {
    let at = now();
    let day = today(at);

    // **The permission, server-side, before anything else.** D45 said it for
    // the counter's own commands and it is truer here: a phone that hides a
    // button is a courtesy, this is the control.
    //
    // Note what this means for §6's "the same answer every time": a permission
    // refusal is decided BEFORE the transaction and so is never written to the
    // idempotency ledger. That is deliberate and it is still honest — the
    // answer is a pure function of who is asking and what they asked, so a
    // retry gets the same sentence anyway — and it keeps the ledger from
    // filling up with rows for a phone that is repeatedly told no.
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
        });
    }

    // **Too old to apply without asking a person.** Checked before the
    // transaction because a held intent changes nothing at all.
    if is_stale(intent.at, at) {
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
        });
    }

    let cashier_has = app
        .with_cart(|state| Ok(state.order_id.clone()))
        .unwrap_or_default();

    let applied = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);

                // **Idempotency, in the same transaction as the effect.** A
                // repeat gets the ORIGINAL answer — the phone is going to show
                // it to a waiter, so "already applied" would be a different
                // sentence for the same event.
                if let Some(before) = repos.events().recall(&intent.id)? {
                    return Ok(Applied {
                        outcome: serde_json::from_str(&before).unwrap_or(Outcome::Refused {
                            message: "This was already done, and the counter could \
                                      not read back what it said the first time."
                                .to_owned(),
                        }),
                        tell_the_cashier: None,
                    });
                }

                let applied = do_it(&repos, intent, staff, at, day, cashier_has.as_deref())?;

                let recorded = serde_json::to_string(&applied.outcome).unwrap_or_default();
                repos
                    .events()
                    .remember(OUTLET, &intent.id, device_id, &recorded, at)?;

                // R11 — the audit row, in the same transaction, so "who
                // cancelled that item?" is answerable a month later. v1 kept
                // two days.
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(at, day, Some(staff.clone()), action::INTENT_APPLIED, "order")
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

    Ok(applied)
}

/// True when an intent has been sitting in a phone's pocket too long.
#[must_use]
pub fn is_stale(typed_at_ms: i64, now: Timestamp) -> bool {
    // A phone's clock can be ahead of the counter's; that is not staleness and
    // must not be treated as it. Only the past counts.
    let age_ms = now.millis().saturating_sub(typed_at_ms);
    age_ms > HOLD_AFTER_HOURS.saturating_mul(60 * 60 * 1_000)
}

/// What each operation needs. `None` means "being paired is enough".
const fn needs(what: &What) -> Option<Permission> {
    match what {
        What::OpenOrder { .. }
        | What::AddItem { .. }
        | What::SetQty { .. }
        | What::SetOrderNote { .. }
        | What::SetCovers { .. }
        | What::SendToKitchen
        | What::RequestBill => Some(Permission::BillCreate),
        // Taking something back is the permission the counter uses for the
        // same act, and for the same reason (audit B5/B6).
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
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "everything applying an intent needs, and threading it through a \
              struct would only move the list somewhere less obvious"
)]
/// **This deliberately does NOT take `&App`.**
///
/// It runs inside `App::with_shop`, so reaching back through `App` would take
/// the shop mutex a second time on the same thread — which is not a slow path,
/// it is a hang. The first version did exactly that to look a menu item up and
/// the test suite stopped dead. Everything it needs comes through `repos`.
fn do_it(
    repos: &mb_db::Repos<'_>,
    intent: &Intent,
    staff: &StaffId,
    at: Timestamp,
    day: mb_core::BusinessDay,
    cashier_has: Option<&str>,
) -> Result<Applied, mb_db::DbError> {
    // Opening an order is the one intent with no order to load.
    if let What::OpenOrder {
        order_type,
        table_id,
        covers,
    } = &intent.what
    {
        return open_order(repos, order_type, table_id.as_deref(), *covers, staff, at, day);
    }

    let Some(order_id) = intent.order_id.as_deref() else {
        return Ok(refused(
            "The phone did not say which order this is about. Open the table again.",
        ));
    };
    let found = repos.orders().find(&OrderId::new(order_id))?;

    // **Conflict (e): the counter has already finished with it.** Said in the
    // words a waiter needs — whether to write it down or not.
    let mut open = match found {
        Some(AnyOrder::Open(open)) => open,
        Some(AnyOrder::Settled(_)) => {
            return Ok(refused(
                "That bill has already been paid at the counter. Start a new \
                 order for anything else.",
            ));
        }
        Some(AnyOrder::Voided(_)) => {
            return Ok(refused(
                "That bill was paid and then cancelled at the counter. Start a \
                 new order.",
            ));
        }
        Some(AnyOrder::Cancelled(_)) => {
            return Ok(refused(
                "That order was cancelled at the counter. Start a new one.",
            ));
        }
        Some(AnyOrder::Draft(_)) | None => {
            return Ok(refused(
                "That order is not on the counter any more. Open the table again.",
            ));
        }
    };

    let mut tell_the_cashier = None;
    let mut note = None;

    match &intent.what {
        What::OpenOrder { .. } => unreachable!("handled above"),

        What::AddItem {
            item_id,
            qty,
            note: line_note,
            ..
        } => {
            // **Read through THIS transaction, not through `App`.**
            //
            // `App::find_menu_item` opens its own `with_shop`, and this code
            // already runs inside one — which takes the shop's mutex a second
            // time on the same thread and hangs the counter. That is the exact
            // deadlock P18 found in `closed_words`, and it hung the test suite
            // here too. Reading through `repos` also means the item is the one
            // this transaction sees, which is the correct answer anyway.
            let Some(item) = repos
                .menu()
                .list_items(OUTLET, true)?
                .into_iter()
                .find(|i| i.id.as_str() == item_id)
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
            // **The counter's own price, frozen here.** A phone holding a
            // stale catalogue cannot sell yesterday's price (P13's
            // `ItemSnapshot`), and the phone never sent one anyway.
            if open
                .core
                .cart
                .add(
                    crate::billing::snapshot_for(&item),
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
            // **Conflict (c): the kitchen has already cooked some of it.**
            // The ledger is the authority and it says how much was told.
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
            // Conflict (c) again, and this is the important half: voiding
            // something the kitchen COOKED is a decision with a cost, so it
            // goes to the counter rather than being done from the floor.
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
            // The seam is named rather than half-built: a discount is money,
            // the counter owns money, and the staff limit lives in P11's role.
            // Until a phone genuinely needs it this refuses in words instead of
            // shipping a second discount path.
            return Ok(refused(
                "Discounts are given at the counter. Ask the cashier.",
            ));
        }

        What::SendToKitchen => {
            // **The counter decides the delta.** `pending` is the only thing
            // allowed to, and a phone that computed its own would double-print
            // on a retry — which is the exact failure crown jewel 2 exists to
            // prevent.
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
            }
        }

        What::MoveTable { table_id } => {
            // **Conflict (d): the cashier moved it first.** The counter's
            // table is the truth; the phone is told where it went.
            open.core.table = Some(mb_core::TableId::new(table_id.clone()));
        }

        What::CancelOrder { reason } => {
            if reason.trim().is_empty() {
                return Ok(refused("A cancelled order needs a reason."));
            }
            let cancelled = open
                .clone()
                .cancel(reason, staff.clone(), at)
                .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
            repos.orders().save(
                OUTLET,
                crate::billing::TERMINAL,
                &AnyOrder::Cancelled(cancelled),
            )?;
            return Ok(Applied {
                outcome: Outcome::Ok {
                    order_id: order_id.to_owned(),
                    total: Money::ZERO.to_plain_string(),
                    lines: vec![],
                    token: Some(open.token.formatted.clone()),
                    note: Some("The order was cancelled.".to_owned()),
                },
                tell_the_cashier: None,
            });
        }

        What::RequestBill => {
            note = Some("The counter has the bill. Send the customer over.".to_owned());
        }
    }

    repos
        .orders()
        .save(OUTLET, crate::billing::TERMINAL, &AnyOrder::Open(open.clone()))?;

    Ok(Applied {
        outcome: view_of(&open, note),
        tell_the_cashier,
    })
}

fn open_order(
    repos: &mb_db::Repos<'_>,
    order_type: &str,
    table_id: Option<&str>,
    covers: Option<u32>,
    staff: &StaffId,
    at: Timestamp,
    day: mb_core::BusinessDay,
) -> Result<Applied, mb_db::DbError> {
    let Ok(order_type) = mb_db::encode::order_type_from_sql(order_type) else {
        return Ok(refused("That is not an order type this counter knows."));
    };

    // **A table this shop does not have.**
    //
    // Checked here rather than left to the foreign key, because a constraint
    // violation reaches the waiter as *"The shop's data could not be read"* —
    // audit F8, on a phone, with a customer waiting. A phone holding a stale
    // catalogue after the counter deleted a table is an ordinary Tuesday.
    // Found by a test that opened `tbl_7` on a shop with no tables.
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

    // **Conflict (a): two waiters open the same table at once.** The FIRST one
    // wins and the second joins it — which is what a shop wants: two waiters at
    // one table are serving one party, and two orders on one table is a bill
    // that gets split by accident.
    if let Some(table) = table_id {
        let existing = repos.orders().list_open(OUTLET)?.into_iter().find(|o| {
            o.core()
                .table
                .as_ref()
                .is_some_and(|t| t.as_str() == table)
        });
        if let Some(AnyOrder::Open(open)) = existing {
            return Ok(Applied {
                outcome: view_of(
                    &open,
                    Some(
                        "Somebody had already opened this table. You are both on \
                         the same order."
                            .to_owned(),
                    ),
                ),
                tell_the_cashier: None,
            });
        }
    }

    let mut draft = mb_core::DraftOrder::new(
        mb_core::OrderId::new(format!("ord_{}", mb_auth::random_token(12))),
        day,
        at,
        order_type,
        staff.clone(),
    );
    if let Some(table) = table_id {
        draft = draft.on_table(mb_core::TableId::new(table.to_owned()));
    }
    if let Some(covers) = covers {
        draft = draft.with_covers(covers);
    }

    // **The counter claims the numbers, atomically, in THIS transaction** (D6,
    // audit B4: v1 read the number and increased it in a separate command, so
    // a phone order arriving at the moment the cashier pressed Complete Bill
    // could get the same one).
    let token = mb_db::numbering::claim(
        repos.tx(),
        OUTLET,
        crate::billing::TERMINAL,
        mb_db::numbering::CounterKind::Token,
        day,
    )?;
    let bill_number = mb_db::numbering::claim(
        repos.tx(),
        OUTLET,
        crate::billing::TERMINAL,
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
        .save(OUTLET, crate::billing::TERMINAL, &AnyOrder::Open(open.clone()))?;

    Ok(Applied {
        outcome: view_of(&open, None),
        tell_the_cashier: None,
    })
}

/// The counter's own view of the order, which is the only view there is.
fn view_of(open: &mb_core::OpenOrder, note: Option<String>) -> Outcome {
    let total = open
        .core
        .cart
        .lines()
        .iter()
        .filter_map(|line| line.qty.extend(line.snapshot.unit_price).ok())
        .try_fold(Money::ZERO, |sum, amount| sum.add(amount))
        .unwrap_or(Money::ZERO);

    Outcome::Ok {
        order_id: open.core.id.as_str().to_owned(),
        // **Formatted here.** R8 and D39: the phone shows this string and
        // never does arithmetic on money. It is the running total of the
        // lines, not the bill — the BILL is computed by `compute_bill` at the
        // counter when it is settled, and there is only ever one of those.
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

// ---------------------------------------------------------------------------
// Offline: a batch of intents a phone queued while it could not reach us.
// ---------------------------------------------------------------------------

/// Apply a whole batch, **in order**.
///
/// Idempotency is applied across the batch rather than per message, which
/// falls out of doing it per intent inside its own transaction: a phone that
/// retries a batch after getting half an answer gets the original outcome for
/// the half that landed and a fresh one for the rest.
///
/// **The report is per intent.** A batch that reports one overall status is a
/// batch whose failures are invisible — the waiter would see "sent" and never
/// learn that four of forty were refused.
///
/// # Errors
///
/// Only when the database itself will not answer.
pub fn apply_batch(
    app: &App,
    device_id: &str,
    staff: &StaffId,
    permissions: &mb_auth::PermissionSet,
    batch: &mb_lan::Batch,
) -> UiResult<mb_lan::BatchResult> {
    let mut outcomes = Vec::with_capacity(batch.intents.len());
    let (mut ok, mut refused_count, mut held) = (0_i64, 0_i64, 0_i64);

    for intent in &batch.intents {
        let applied = apply(app, device_id, staff, permissions, intent)?;
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

    // The whole thing in one sentence, written here (§6) because the phone
    // shows it and must not assemble it.
    let total = ok + refused_count + held;
    let mut says = format!(
        "{} of {} went through.",
        words::count(ok, "order change", "order changes"),
        total
    );
    if held > 0 {
        says.push_str(&format!(
            " {} waiting for somebody at the counter to say whether they still apply.",
            words::count(held, "is", "are")
        ));
    }
    if refused_count > 0 {
        says.push_str(&format!(
            " {} could not be done — open each one to see why.",
            words::count(refused_count, "change", "changes")
        ));
    }

    Ok(mb_lan::BatchResult { outcomes, says })
}

// ---------------------------------------------------------------------------
// The catalogue.
// ---------------------------------------------------------------------------

/// What a phone needs to take an order, with a version.
///
/// **The version is the whole point.** A phone asks with what it holds and is
/// told "unchanged" rather than being sent the menu again: 400 items to fifteen
/// phones on every reconnect is a shop whose WiFi is the bottleneck.
///
/// The version is a hash of what a phone would actually SEE — names, prices and
/// availability — so a shop that edits a cost price (which no phone shows)
/// does not push 400 items to the floor.
///
/// # Errors
///
/// When the shop's menu cannot be read.
pub fn catalogue(app: &App) -> UiResult<mb_lan::Catalogue> {
    let (items, sections, tables) = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                Ok((
                    // **Every item, including the ones that ran out** —
                    // `false` and not `true`. A phone that is simply not sent
                    // a sold-out dish shows a menu with a hole in it and a
                    // waiter who cannot tell "gone" from "never existed".
                    // Scope 3.9: it arrives marked unavailable and the phone
                    // greys it out. Found by a test that sold out a dosa and
                    // watched it vanish.
                    repos.menu().list_items(OUTLET, false)?,
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
                category: item
                    .category_id
                    .as_ref()
                    .map(|c| c.as_str().to_owned())
                    .unwrap_or_default(),
                price: item.unit_price.to_plain_string(),
                is_available: item.is_available,
            };
            fingerprint.push_str(&one.id);
            fingerprint.push_str(&one.name);
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
            // The floor screen owns what a table IS doing; the catalogue only
            // says what tables EXIST. A phone learns the state from the live
            // stream, which is the thing that changes every minute.
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

// ---------------------------------------------------------------------------
// The cashier's side of a floor change.
// ---------------------------------------------------------------------------

/// **Take the floor's items into the cart the cashier is typing.**
///
/// This is the "merge", and it is deliberately over LINES only. A phone can
/// add or change lines and nothing else — it cannot take a payment, it cannot
/// give a discount — so the cashier's `settlement` and `bill_discount` are not
/// part of the merge and are never touched. Writing that down is the point:
/// "merge the order" would be an invitation to overwrite the half-typed
/// payment somebody is standing there counting out.
///
/// # Errors
///
/// When an item has since left the menu.
pub fn take_the_floors_items_on(app: &App) -> UiResult<crate::billing::CartView> {
    guard::require(app, Permission::BillCreate)?;
    let changes = app.with_cart(|state| Ok(state.from_the_floor.clone()))?;

    // Every menu lookup FIRST, outside the cart lock — `find_menu_item` takes
    // the shop lock, and taking two locks in two orders in one product is how
    // a till freezes at eight o'clock on a Saturday.
    let mut resolved = Vec::new();
    for change in &changes {
        let item = app.find_menu_item(&change.item_id)?;
        let qty = Qty::parse(&change.qty).unwrap_or(Qty::ZERO);
        resolved.push((item, qty, change.note.clone()));
    }

    app.with_cart_mut(|state| {
        for (item, qty, note) in resolved {
            state
                .cart
                .add(crate::billing::snapshot_for(&item), qty, note, vec![])
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
///
/// **The order on disk still has them** — the counter took the phone's change
/// because the counter is the authority. This only clears the note on the
/// screen, and the sentence on the button says so.
pub fn dismiss_the_floors_items_on(app: &App) -> UiResult<crate::billing::CartView> {
    guard::require(app, Permission::BillCreate)?;
    app.with_cart_mut(|state| {
        state.from_the_floor.clear();
        crate::billing::cart_view(state, &app.shop_config())
    })
}

#[tauri::command]
pub fn take_the_floors_items(
    app: tauri::State<'_, App>,
) -> UiResult<crate::billing::CartView> {
    take_the_floors_items_on(&app)
}

#[tauri::command]
pub fn dismiss_the_floors_items(
    app: tauri::State<'_, App>,
) -> UiResult<crate::billing::CartView> {
    dismiss_the_floors_items_on(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **T7's first half.** A phone that was in somebody's apron all evening
    /// must not print yesterday's tickets when the shop opens.
    #[test]
    fn an_intent_from_yesterday_is_held_and_one_from_now_is_not() {
        let now = Timestamp::from_millis(50 * 60 * 60 * 1_000);
        let hour = 60 * 60 * 1_000;

        assert!(!is_stale(now.millis(), now), "an intent typed now is stale");
        assert!(!is_stale(now.millis() - 11 * hour, now));
        assert!(is_stale(now.millis() - 13 * hour, now));

        // **A phone whose clock is AHEAD is not stale.** Two devices on a shop
        // WiFi disagree about the time by minutes as a matter of routine, and
        // treating that as staleness would hold a waiter's live order.
        assert!(!is_stale(now.millis() + 5 * hour, now));
    }

    /// Every operation has had its permission decided — the same rule
    /// `guard::COMMAND_ACCESS` enforces for the counter's own commands (D45).
    #[test]
    fn every_operation_has_a_permission_decided() {
        let all = [
            What::OpenOrder { order_type: "dine_in".to_owned(), table_id: None, covers: None },
            What::AddItem { item_id: "i".to_owned(), qty: "1".to_owned(), note: None, modifiers: vec![] },
            What::SetQty { line: 0, qty: "1".to_owned() },
            What::VoidItem { line: 0, reason: "r".to_owned() },
            What::SetOrderNote { note: None },
            What::SetCustomer { customer_id: None },
            What::SetCovers { covers: None },
            What::RequestDiscount { line: Some(0), percent_bp: 1000, reason: "r".to_owned() },
            What::RequestDiscount { line: None, percent_bp: 1000, reason: "r".to_owned() },
            What::SendToKitchen,
            What::MoveTable { table_id: "t".to_owned() },
            What::CancelOrder { reason: "r".to_owned() },
            What::RequestBill,
        ];
        for what in &all {
            assert!(
                needs(what).is_some(),
                "{} has no permission decided, so a paired phone could do it",
                what.name()
            );
        }
        // And taking something back needs the same permission the counter's
        // own screen asks for.
        assert_eq!(
            needs(&What::VoidItem { line: 0, reason: String::new() }),
            Some(Permission::OrderItemVoid)
        );
        assert_eq!(
            needs(&What::CancelOrder { reason: String::new() }),
            Some(Permission::OrderCancel)
        );
    }
}
