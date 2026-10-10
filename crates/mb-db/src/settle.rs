//! The one function that settles a bill.

use mb_core::{Bill, OpenOrder, SettledOrder, Settlement, StaffId, Timestamp};

use crate::conn::Db;
use crate::error::DbError;
use crate::numbering::{self, CounterKind};
use crate::repo::Repos;

/// Which counter, in which shop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Till<'a> {
    pub outlet: &'a str,
    pub terminal: &'a str,
}

impl<'a> Till<'a> {
    #[must_use]
    pub const fn new(outlet: &'a str, terminal: &'a str) -> Self {
        Till { outlet, terminal }
    }
}

/// Settle an open order and write it, in one commit. An order that has not been billed yet
/// takes its bill number here, in the same transaction, so a settle that fails spends none.
pub fn settle(
    db: &Db,
    till: Till<'_>,
    order: OpenOrder,
    bill: Bill,
    settlement: Settlement,
    at: Timestamp,
    by: StaffId,
) -> Result<SettledOrder, DbError> {
    settle_checked(db, till, order, bill, settlement, at, by, |_| Ok(()))
}

/// The normal settlement transaction, with a final authorization checked and recorded
/// before any bill, money or stock write. An error rolls back the entire replacement.
#[allow(clippy::too_many_arguments, reason = "preserves the existing settlement inputs and adds authorization inside the same transaction")]
pub fn settle_checked(
    db: &Db,
    till: Till<'_>,
    order: OpenOrder,
    bill: Bill,
    settlement: Settlement,
    at: Timestamp,
    by: StaffId,
    authorize: impl FnOnce(&Repos<'_>) -> Result<(), DbError>,
) -> Result<SettledOrder, DbError> {
    let (outlet, terminal) = (till.outlet, till.terminal);
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        if let Some(existing) = repos.orders().find(&order.core.id)? {
            match existing {
                mb_core::AnyOrder::Settled(paid)
                    if paid.core.billing.revision >= order.core.billing.revision =>
                {
                    // A retried commit must not reverse its own stock then skip the already
                    // claimed deduction event. Return the same issued result without writes.
                    if paid.core == order.core && paid.bill == bill && paid.settlement == settlement
                    {
                        return Ok(paid);
                    }
                    return Err(DbError::invariant(
                        "This bill has already changed; open it again.",
                    ));
                }
                mb_core::AnyOrder::Voided(_) | mb_core::AnyOrder::Cancelled(_) => {
                    return Err(DbError::invariant(
                        "This order is no longer available to settle.",
                    ));
                }
                _ => {}
            }
        }
        authorize(&repos)?;
        let mut order = order;
        numbering::number_the_bill(tx, outlet, terminal, &mut order)?;
        let settled = order
            .settle(bill, settlement, at, by)
            .map_err(|e| DbError::invariant(format!("this bill cannot be settled: {e}")))?;

        for source in &settled.core.billing.sources {
            let Some(mut serving) = repos.orders().find_working(source)? else {
                return Err(DbError::invariant("One of the combined orders is missing."));
            };
            if let Some(mb_core::AnyOrder::Settled(_)) = repos.orders().find(source)? {
                repos.stock().reverse_for_bill(
                    outlet,
                    source,
                    at,
                    settled.core.business_day,
                    Some(&settled.settled_by),
                )?;
            }
            serving.core_mut().billing.billed_into = Some(settled.core.id.clone());
            serving.core_mut().billing.settlement = Settlement::new();
            repos.orders().save(outlet, terminal, &serving)?;
            repos.orders().finish_edit(source)?;
        }
        for (index, (mode, amount)) in settled
            .core
            .billing
            .refund
            .iter()
            .chain(&settled.core.billing.refunds)
            .enumerate()
        {
            let refund = crate::repo::corrections::Refund {
                id: format!(
                    "adjust_{}_v{}_{}",
                    settled.core.id, settled.core.billing.revision, index
                ),
                order_id: settled.core.id.clone(),
                amount: *amount,
                mode: mode.clone(),
                reason: "Returned the difference on a corrected bill".to_owned(),
                refunded_at: at,
                refunded_by: Some(settled.settled_by.clone()),
            };
            repos.corrections().record_adjustment_refund(
                outlet,
                &refund,
                settled.core.business_day,
            )?;
        }
        if settled.core.billing.revision > 0 {
            repos.stock().reverse_for_bill(
                outlet,
                &settled.core.id,
                at,
                settled.core.business_day,
                Some(&settled.settled_by),
            )?;
        }
        repos.orders().save(
            outlet,
            terminal,
            &mb_core::AnyOrder::Settled(settled.clone()),
        )?;
        repos.stock().deduct_for_bill(outlet, &settled, at)?;
        if !settled.core.billing.sources.is_empty() && settled.core.table().is_some() {
            let service_id = mb_core::OrderId::new(format!("{}_service", settled.core.id));
            let occupied = repos.floor().first_party_at(
                settled
                    .core
                    .table()
                    .ok_or_else(|| DbError::invariant("missing table"))?,
            )?;
            let existing = repos.orders().find(&service_id)?;
            if let Some(mb_core::AnyOrder::Open(mut service)) = existing.clone() {
                // The serving party may have moved since billing. Refresh only its food and
                // kitchen quantities; keep its location, serving token and released state.
                if let Some(cart) = &settled.core.billing.service_cart {
                    service.core.cart = cart.clone();
                }
                if let Some(kitchen) = &settled.core.billing.service_kitchen {
                    service.core.kitchen = kitchen.clone();
                }
                repos
                    .orders()
                    .save(outlet, terminal, &mb_core::AnyOrder::Open(service))?;
                repos
                    .kitchen()
                    .transfer_order(settled.core.id.as_str(), service_id.as_str())?;
            } else if existing.is_none() && occupied.is_none_or(|id| id == settled.core.id.as_str())
            {
                let mut core = settled.core.clone();
                core.id = service_id;
                core.cart = core
                    .billing
                    .service_cart
                    .clone()
                    .unwrap_or_else(|| core.cart.clone());
                core.kitchen = core
                    .billing
                    .service_kitchen
                    .clone()
                    .unwrap_or_else(|| core.kitchen.clone());
                core.billing = mb_core::BillingAccount {
                    billed_into: Some(settled.core.id.clone()),
                    service_token: Some(settled.token.formatted.clone()),
                    ..Default::default()
                };
                let token =
                    numbering::claim(tx, outlet, terminal, CounterKind::Token, core.business_day)?;
                let service = mb_core::AnyOrder::Open(mb_core::OpenOrder {
                    core,
                    token,
                    bill_number: None,
                });
                repos.orders().save(outlet, terminal, &service)?;
                repos
                    .kitchen()
                    .transfer_order(settled.core.id.as_str(), service.core().id.as_str())?;
                repos
                    .floor()
                    .record_merge(service.core().id.as_str(), settled.core.id.as_str())?;
            }
        }
        repos.orders().finish_edit(&settled.core.id)?;
        Ok(settled)
    })
}

/// Open a draft: claim its token, and write it. The bill number waits for the bill.
pub fn open_draft(
    db: &Db,
    till: Till<'_>,
    draft: mb_core::DraftOrder,
) -> Result<OpenOrder, DbError> {
    let (outlet, terminal) = (till.outlet, till.terminal);
    let day = draft.core.business_day;

    db.transaction(|tx| {
        let token = numbering::claim(tx, outlet, terminal, CounterKind::Token, day)?;
        let open = OpenOrder {
            core: draft.core.clone(),
            token,
            bill_number: None,
        };
        Repos::new(tx)
            .orders()
            .save(outlet, terminal, &mb_core::AnyOrder::Open(open.clone()))?;
        Ok(open)
    })
}
