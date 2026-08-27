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

/// Settle an open order and write it, in one commit.
pub fn settle(
    db: &Db,
    till: Till<'_>,
    order: OpenOrder,
    bill: Bill,
    settlement: Settlement,
    at: Timestamp,
    by: StaffId,
) -> Result<SettledOrder, DbError> {
    let (outlet, terminal) = (till.outlet, till.terminal);
    let settled = order
        .settle(bill, settlement, at, by)
        .map_err(|e| DbError::invariant(format!("this bill cannot be settled: {e}")))?;

    let any = mb_core::AnyOrder::Settled(settled.clone());
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.orders().save(outlet, terminal, &any)?;
        repos.stock().deduct_for_bill(outlet, &settled, at)
    })?;
    Ok(settled)
}

/// Open a draft: claim its token and bill number, and write it.
pub fn open_draft(
    db: &Db,
    till: Till<'_>,
    draft: mb_core::DraftOrder,
) -> Result<OpenOrder, DbError> {
    let (outlet, terminal) = (till.outlet, till.terminal);
    let day = draft.core.business_day;

    if draft.core.order_type == mb_core::OrderType::DineIn && draft.core.table.is_none() {
        return Err(DbError::invariant(
            "a dine-in order needs a table before it can be opened",
        ));
    }

    db.transaction(|tx| {
        let token = numbering::claim(tx, outlet, terminal, CounterKind::Token, day)?;
        let bill_number = numbering::claim(tx, outlet, terminal, CounterKind::Bill, day)?;
        let open = OpenOrder {
            core: draft.core.clone(),
            token,
            bill_number,
        };
        Repos::new(tx)
            .orders()
            .save(outlet, terminal, &mb_core::AnyOrder::Open(open.clone()))?;
        Ok(open)
    })
}
