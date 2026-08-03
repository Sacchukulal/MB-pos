//! The one function that settles a bill.
//!
//! This is the only place in the product where the money path touches the disk,
//! and three rules meet in it.
//!
//! **One transaction is one fsync.** The numbering claim, the order header,
//! every line, the computed bill, the payments, the kitchen ledger and the
//! outbox row all commit together. D23 puts the writer at
//! `synchronous = FULL`, so a settle costs one flush and not four —
//! `PERFORMANCE.md` §5 rule 1 and budget **B5** (150 ms, ceiling 400 ms) are
//! the same statement seen from two sides. §1 of that document spells out the
//! alternative: *"a bill that settles with four separate durable writes is
//! 40 ms of pure disk wait before anything else happens."*
//!
//! **A settle that fails must not burn a bill number.** The claim happens
//! inside the transaction, so a rollback returns the number to the series. That
//! is why [`crate::numbering::claim`] takes a `&Transaction` and not a `&Db`,
//! and `t13` proves the number comes back.
//!
//! **Nothing metered happens here** (scope 17.1). No cloud call, no licence
//! check, no network of any kind. The outbox row is a local INSERT. If a later
//! session adds anything here that can block on a network it has broken
//! requirement 3 of the ten — *billing never stops* — and P30 will find it.

use mb_core::{Bill, OpenOrder, SettledOrder, Settlement, StaffId, Timestamp};

use crate::conn::Db;
use crate::error::DbError;
use crate::numbering::{self, CounterKind};
use crate::repo::Repos;

/// Which counter, in which shop.
///
/// Every write needs both — the outlet because scope 11.4 puts it on every root
/// table, the terminal because 11.1 puts two billing PCs in one shop and 11.2
/// gives each its own number series. Carrying them together stops one of them
/// being forgotten at a call site, which is the mistake that would only show up
/// on the second terminal a year from now.
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
///
/// The arithmetic is not re-checked here: `OpenOrder::settle` in mb-core
/// already refuses a settlement that does not settle the bill, and doing it
/// twice is how two versions of a rule appear.
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
    db.transaction(|tx| Repos::new(tx).orders().save(outlet, terminal, &any))?;
    Ok(settled)
}

/// Open a draft: claim its token and bill number, and write it.
///
/// The claim is inside the same transaction as the write for the same reason
/// the settle's is — a failure here must not consume a number. The numbers are
/// claimed for the order's **own** business day, not for today: an order
/// created at 00:15 under a 05:00 day rule belongs to yesterday and takes
/// yesterday's series (D5, and it is audit B1 and B3 meeting).
pub fn open_draft(
    db: &Db,
    till: Till<'_>,
    draft: mb_core::DraftOrder,
) -> Result<OpenOrder, DbError> {
    let (outlet, terminal) = (till.outlet, till.terminal);
    let day = draft.core.business_day;

    // mb-core refuses a dine-in order with no table at exactly this moment —
    // audit 2.3 — and a draft is allowed to be incomplete until it.
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
        Repos::new(tx).orders().save(
            outlet,
            terminal,
            &mb_core::AnyOrder::Open(open.clone()),
        )?;
        Ok(open)
    })
}
