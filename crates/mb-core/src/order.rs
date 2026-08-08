//! The order, from the first item typed to the bill being voided.
//!
//! # Why each state is its own type
//!
//! **An illegal transition should not compile.** A single struct with a
//! `state` field would need a runtime check inside every method, and a runtime
//! check is something a future session can forget to write. Settling a draft,
//! cancelling a settled bill or voiding an open order are not errors to be
//! handled — they are sentences that cannot be spoken.
//!
//! ```compile_fail
//! # use mb_core::order::*;
//! # fn no(draft: DraftOrder) {
//! // A draft has no bill and no numbers. There is nothing to settle.
//! draft.settle();
//! # }
//! ```
//!
//! ```compile_fail
//! # use mb_core::order::*;
//! # fn no(settled: SettledOrder) {
//! // A settled bill is voided, not cancelled. Cancelling belongs to an open
//! // order that never became a bill.
//! settled.cancel();
//! # }
//! ```
//!
//! # What v1 could not do
//!
//! > **B5.** "There is no way to cancel a finalised bill. Once Complete Bill is
//! > pressed, that bill is in your sales and your GST forever."
//!
//! > **B6.** "There is no way to cancel an open (KOT'd) order from the counter.
//! > Only a phone can cancel. If a customer walks out, the order sits in the
//! > Processing list forever and the table stays busy."
//!
//! Both fixes demand a compulsory reason. So `reason` is a required parameter
//! here, never an `Option` checked later — that is how compulsory quietly
//! becomes optional.

use crate::bill::Bill;
use crate::businessday::BusinessDay;
use crate::cart::{Cart, LineIdentity};
use crate::ids::{OrderId, StaffId, TableId};
use crate::item::OrderType;
use crate::numbering::{Claimed, Numbering};
use crate::payment::Settlement;
use crate::qty::{Qty, QtyError};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// Something that stopped an order from moving on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OrderError {
    /// B5/B6: both fixes say "with a compulsory reason".
    #[error("please give a reason")]
    ReasonRequired,
    /// Audit 2.3: "A Table order must have a table number."
    #[error("a dine-in order needs a table")]
    TableRequired,
    #[error("this bill is not fully paid yet")]
    NotFullyPaid,
    #[error("that settlement is not valid: {0}")]
    Payment(#[from] crate::payment::PaymentError),
    #[error("a quantity on this order is out of range")]
    Qty(#[from] QtyError),
}

type Result<T> = std::result::Result<T, OrderError>;

/// What every order carries, in every state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderCore {
    pub id: OrderId,
    /// **Decision D5 — stamped once, at creation, and never re-derived.**
    /// Every report groups by this stored value, which is what stops the
    /// counter, the phone and the cloud from disagreeing about which day a
    /// late-night bill belongs to.
    pub business_day: BusinessDay,
    pub created_at: Timestamp,
    pub order_type: OrderType,
    /// Set for a dine-in order. P10 adds sub-tables (6A / 6B, scope 1.6) and
    /// P14 owns the floor plan; here it is only which table this order is on.
    pub table: Option<TableId>,
    pub cart: Cart,
    pub created_by: StaffId,
    pub note: Option<String>,
    pub kitchen: KitchenLedger,
}

/// An order still being typed. No numbers, nothing told to the kitchen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftOrder {
    pub core: OrderCore,
}

/// An order that has its numbers and is on the floor — v1's "Processing".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenOrder {
    pub core: OrderCore,
    pub token: Claimed,
    pub bill_number: Claimed,
}

/// A paid bill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettledOrder {
    pub core: OrderCore,
    pub token: Claimed,
    pub bill_number: Claimed,
    pub bill: Bill,
    pub settlement: Settlement,
    pub settled_at: Timestamp,
    pub settled_by: StaffId,
}

/// An open order that never became a bill — the customer walked out (B6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelledOrder {
    pub core: OrderCore,
    pub token: Claimed,
    pub bill_number: Claimed,
    pub reason: String,
    pub cancelled_at: Timestamp,
    pub cancelled_by: StaffId,
}

/// A bill that was settled and then reversed (B5).
///
/// It keeps its bill number and its original amounts. The number is never
/// reused and never reissued, and the bill stays readable, so a report can show
/// gross takings and takings net of voids side by side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoidedOrder {
    pub core: OrderCore,
    pub token: Claimed,
    pub bill_number: Claimed,
    pub bill: Bill,
    pub settlement: Settlement,
    pub settled_at: Timestamp,
    pub settled_by: StaffId,
    pub reason: String,
    pub voided_at: Timestamp,
    pub voided_by: StaffId,
}

/// Any order, whatever state it is in.
///
/// **For storage and iteration only.** P04 keeps them in one table and P09's
/// grid lists a mix of them. Transitions never go through here — they go
/// through the state types, where the compiler is watching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AnyOrder {
    Draft(DraftOrder),
    Open(OpenOrder),
    Settled(SettledOrder),
    Cancelled(CancelledOrder),
    Voided(VoidedOrder),
}

impl AnyOrder {
    #[must_use]
    pub fn core(&self) -> &OrderCore {
        match self {
            AnyOrder::Draft(o) => &o.core,
            AnyOrder::Open(o) => &o.core,
            AnyOrder::Settled(o) => &o.core,
            AnyOrder::Cancelled(o) => &o.core,
            AnyOrder::Voided(o) => &o.core,
        }
    }

    /// The bill number, once one has been claimed.
    #[must_use]
    pub fn bill_number(&self) -> Option<&Claimed> {
        match self {
            AnyOrder::Draft(_) => None,
            AnyOrder::Open(o) => Some(&o.bill_number),
            AnyOrder::Settled(o) => Some(&o.bill_number),
            AnyOrder::Cancelled(o) => Some(&o.bill_number),
            AnyOrder::Voided(o) => Some(&o.bill_number),
        }
    }
}

/// A reason that is blank is not a reason.
fn require_reason(reason: &str) -> Result<String> {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        return Err(OrderError::ReasonRequired);
    }
    Ok(trimmed.to_owned())
}

impl DraftOrder {
    /// Start an order. The business day is computed by the caller — through
    /// [`BusinessDay::of`](crate::businessday::BusinessDay::of), which is the
    /// only place that is allowed to — and stored here for good.
    #[must_use]
    pub fn new(
        id: OrderId,
        business_day: BusinessDay,
        created_at: Timestamp,
        order_type: OrderType,
        created_by: StaffId,
    ) -> Self {
        DraftOrder {
            core: OrderCore {
                id,
                business_day,
                created_at,
                order_type,
                table: None,
                cart: Cart::new(),
                created_by,
                note: None,
                kitchen: KitchenLedger::new(),
            },
        }
    }

    #[must_use]
    pub fn on_table(mut self, table: TableId) -> Self {
        self.core.table = Some(table);
        self
    }

    /// Put the order on the floor: claim its token and bill number.
    ///
    /// The numbers are claimed against the order's **own** business day, not
    /// against today. An order created at 00:15 belongs to yesterday and takes
    /// yesterday's series — B1 and B3 meeting.
    ///
    /// A dine-in order with no table is refused **here** rather than at
    /// creation. Audit 2.3 says "a Table order must have a table number", but a
    /// draft is allowed to be incomplete while the cashier is still typing.
    pub fn open(self, numbering: &mut Numbering) -> Result<OpenOrder> {
        if self.core.order_type == OrderType::DineIn && self.core.table.is_none() {
            return Err(OrderError::TableRequired);
        }
        let (token, bill_number) = numbering.claim_for_new_order(self.core.business_day);
        Ok(OpenOrder { core: self.core, token, bill_number })
    }
}

impl OpenOrder {
    /// Take payment.
    ///
    /// The order keeps its original creation time, business day, token and bill
    /// number — the audit's own words for what v1 got right here (2.3):
    /// "keeping its original time, bill number and token".
    pub fn settle(
        self,
        bill: Bill,
        settlement: Settlement,
        at: Timestamp,
        by: StaffId,
    ) -> Result<SettledOrder> {
        // P02's rules still hold one layer up: no change out of a card, and a
        // bill that is not covered is not settled.
        settlement.validate(bill.grand_total)?;
        if !settlement.is_settled(bill.grand_total)? {
            return Err(OrderError::NotFullyPaid);
        }

        Ok(SettledOrder {
            core: self.core,
            token: self.token,
            bill_number: self.bill_number,
            bill,
            settlement,
            settled_at: at,
            settled_by: by,
        })
    }

    /// The customer walked out (B6). Reason and staff are compulsory.
    pub fn cancel(self, reason: &str, by: StaffId, at: Timestamp) -> Result<CancelledOrder> {
        Ok(CancelledOrder {
            core: self.core,
            token: self.token,
            bill_number: self.bill_number,
            reason: require_reason(reason)?,
            cancelled_at: at,
            cancelled_by: by,
        })
    }
}

impl SettledOrder {
    /// Reverse a bill that should not have been made (B5). Reason and staff are
    /// compulsory, and the bill number is kept, never released.
    pub fn void(self, reason: &str, by: StaffId, at: Timestamp) -> Result<VoidedOrder> {
        Ok(VoidedOrder {
            core: self.core,
            token: self.token,
            bill_number: self.bill_number,
            bill: self.bill,
            settlement: self.settlement,
            settled_at: self.settled_at,
            settled_by: self.settled_by,
            reason: require_reason(reason)?,
            voided_at: at,
            voided_by: by,
        })
    }
}

/// What the kitchen has already been told.
///
/// Audit crown jewel 2, kept word for word:
///
/// > "The delta KOT. Only what the kitchen has not seen gets printed, and what
/// > was printed is remembered **in the database**, not in the screen's
/// > memory."
///
/// So this lives on the order, is serialisable, and is keyed by
/// [`LineIdentity`] — the same identity the cart merges by, not a lookalike.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct KitchenLedger {
    told: Vec<(LineIdentity, Qty)>,
}

impl KitchenLedger {
    #[must_use]
    pub fn new() -> Self {
        KitchenLedger::default()
    }

    #[must_use]
    pub fn told(&self) -> &[(LineIdentity, Qty)] {
        &self.told
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.told.is_empty()
    }

    /// What still has to go to the kitchen.
    ///
    /// Positive deltas only, **in cart order** — a cook works down the ticket
    /// in the sequence the waiter called the items out.
    pub fn pending(&self, cart: &Cart) -> std::result::Result<Vec<(LineIdentity, Qty)>, QtyError> {
        let mut pending = Vec::new();
        for line in cart.lines() {
            let identity = line.identity();
            let already = self.quantity_told(&identity);
            let outstanding = line.qty.sub(already)?;
            if outstanding.is_positive() {
                pending.push((identity, outstanding));
            }
        }
        Ok(pending)
    }

    /// Record that a ticket was printed.
    pub fn mark_printed(
        &mut self,
        delta: &[(LineIdentity, Qty)],
    ) -> std::result::Result<(), QtyError> {
        for (identity, qty) in delta {
            match self.told.iter_mut().find(|(known, _)| known == identity) {
                Some((_, running)) => *running = running.add(*qty)?,
                None => self.told.push((identity.clone(), *qty)),
            }
        }
        Ok(())
    }

    /// What the kitchen was told about that is no longer on the order.
    ///
    /// If a line is reduced or removed after the ticket has gone, the kitchen
    /// is cooking something nobody is paying for. [`KitchenLedger::pending`]
    /// returns positive deltas only, so that case would vanish silently — this
    /// names it.
    ///
    /// **P03 only reports it.** Turning this into a cancellation slip is P12
    /// (scope 1.19, "void individual items, with kitchen cancellation slip").
    /// The seam is here so P12 does not build a second ledger to find it.
    pub fn over_told(&self, cart: &Cart) -> std::result::Result<Vec<(LineIdentity, Qty)>, QtyError> {
        let mut excess = Vec::new();
        for (identity, told) in &self.told {
            let ordered = cart
                .lines()
                .iter()
                .find(|line| &line.identity() == identity)
                .map_or(Qty::ZERO, |line| line.qty);
            let over = told.sub(ordered)?;
            if over.is_positive() {
                excess.push((identity.clone(), over));
            }
        }
        Ok(excess)
    }

    /// **The other half of [`KitchenLedger::over_told`]** — record that the
    /// kitchen has been told to stop.
    ///
    /// P03 built `over_told` and said P12 would turn it into a cancellation
    /// slip. This is what makes that work more than once: without it,
    /// `over_told` keeps returning the same line, and every cancellation slip
    /// for the rest of that order repeats it — the kitchen gets told to stop
    /// cooking the same dosa four times.
    ///
    /// # The order of operations is load-bearing
    ///
    /// Remove the line, ask `over_told`, **print**, and only then call this —
    /// the same shape as `print_kitchen_ticket` (P10), and for the same reason:
    /// recording before the paper is durable loses the cancellation silently,
    /// and the kitchen carries on cooking.
    ///
    /// An entry that falls to zero is dropped rather than kept at zero. The
    /// ledger is a record of what the kitchen currently believes, not a
    /// history — `order_events` and `audit_log` are the history.
    pub fn mark_cancelled(
        &mut self,
        cancelled: &[(LineIdentity, Qty)],
    ) -> std::result::Result<(), QtyError> {
        for (identity, qty) in cancelled {
            if let Some((_, running)) = self.told.iter_mut().find(|(known, _)| known == identity) {
                // Saturating at zero rather than erroring: being asked to
                // cancel more than the kitchen was told is not a failure a
                // cashier can act on, and refusing it would leave the ledger
                // saying the kitchen is still cooking something it was told to
                // stop.
                *running = running.sub(*qty).unwrap_or(Qty::ZERO);
                if !running.is_positive() {
                    *running = Qty::ZERO;
                }
            }
        }
        self.told.retain(|(_, qty)| qty.is_positive());
        Ok(())
    }

    /// How much of one line the kitchen has been told about.
    ///
    /// Public since P14, because splitting an order has to ask — but `told`
    /// itself stays private. The difference matters: a reader can find out
    /// what the kitchen believes; only the methods here can change it, so
    /// every change goes past the conservation arguments written on them.
    #[must_use]
    pub fn quantity_told(&self, identity: &LineIdentity) -> Qty {
        self.told
            .iter()
            .find(|(known, _)| known == identity)
            .map_or(Qty::ZERO, |(_, qty)| *qty)
    }

    /// Set one line's told quantity outright — **the split half of P14.**
    ///
    /// Not a general-purpose setter: `mark_printed` and `mark_cancelled` are
    /// the ways the kitchen's belief changes during an order's life, and they
    /// stay that way. This exists because a split moves quantity between two
    /// ledgers, and the origin's new figure is neither an addition nor a
    /// cancellation — nothing was cooked and nothing was called off; the food
    /// simply belongs to a different bill now. `transfer::take_lines` carries
    /// the arithmetic and the reasoning.
    ///
    /// Zero drops the entry, for the reason `mark_cancelled` gives: the ledger
    /// records what the kitchen currently believes, not a history.
    pub fn set_told(&mut self, identity: &LineIdentity, qty: Qty) {
        match self.told.iter_mut().find(|(known, _)| known == identity) {
            Some((_, running)) => *running = qty,
            None if qty.is_positive() => self.told.push((identity.clone(), qty)),
            None => {}
        }
        self.told.retain(|(_, qty)| qty.is_positive());
    }

    /// Add another order's ledger into this one — **the merge half.**
    ///
    /// Per identity, so two tables that were each told about dosas produce one
    /// order that was told about all of them. Adding is the only correct
    /// operation here: taking the larger, or replacing, would leave the
    /// kitchen either cooking a duplicate or never hearing about the
    /// difference.
    pub fn merge_from(&mut self, other: &KitchenLedger) -> std::result::Result<(), QtyError> {
        self.mark_printed(&other.told)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bill::{BillInput, compute_bill};
    use crate::ids::ItemId;
    use crate::item::ItemSnapshot;
    use crate::money::{Money, RoundingMode};
    use crate::payment::{Payment, PaymentMode};
    use crate::tax::TaxRate;

    fn day() -> BusinessDay {
        BusinessDay::from_ymd(2026, 8, 1)
    }

    fn at(millis: i64) -> Timestamp {
        Timestamp::from_millis(millis)
    }

    fn staff() -> StaffId {
        StaffId::new("stf_ravi")
    }

    fn item(id: &str, paise: i64) -> ItemSnapshot {
        ItemSnapshot::new(ItemId::new(id), id, Money::from_paise(paise), TaxRate::GST_5)
    }

    fn draft(order_type: OrderType) -> DraftOrder {
        DraftOrder::new(OrderId::new("ord_1"), day(), at(1_000), order_type, staff())
    }

    fn open_parcel() -> (OpenOrder, Numbering) {
        let mut numbering = Numbering::new();
        let order = draft(OrderType::Parcel).open(&mut numbering).expect("opens");
        (order, numbering)
    }

    #[test]
    fn a_dine_in_order_cannot_open_without_a_table() {
        // Audit 2.3. But a DRAFT may sit there without one while the cashier
        // is still typing.
        let mut numbering = Numbering::new();
        let no_table = draft(OrderType::DineIn);
        assert!(no_table.core.table.is_none(), "a draft may be incomplete");
        assert_eq!(
            draft(OrderType::DineIn).open(&mut numbering).err(),
            Some(OrderError::TableRequired)
        );

        // With a table it opens.
        let opened = draft(OrderType::DineIn)
            .on_table(TableId::new("tbl_6"))
            .open(&mut numbering)
            .expect("opens");
        assert_eq!(opened.core.table, Some(TableId::new("tbl_6")));

        // Parcel and self-service need no table at all.
        for order_type in [OrderType::Parcel, OrderType::SelfService, OrderType::Delivery] {
            assert!(draft(order_type).open(&mut numbering).is_ok());
        }
    }

    #[test]
    fn an_order_takes_its_numbers_from_its_own_business_day() {
        // Not from "today" — D5 and D6 meeting. An order created at 00:15
        // belongs to yesterday and takes yesterday's series.
        let mut numbering = Numbering::new();
        let order = DraftOrder::new(
            OrderId::new("ord_late"),
            BusinessDay::from_ymd(2026, 8, 1),
            at(0),
            OrderType::Parcel,
            staff(),
        )
        .open(&mut numbering)
        .expect("opens");

        assert_eq!(order.token.business_day, BusinessDay::from_ymd(2026, 8, 1));
        assert_eq!(order.bill_number.business_day, BusinessDay::from_ymd(2026, 8, 1));
        assert_eq!(order.core.business_day, BusinessDay::from_ymd(2026, 8, 1));
    }

    #[test]
    fn cancelling_and_voiding_both_demand_a_reason() {
        // B5 and B6 both say "compulsory reason". Blank is not a reason.
        let (order, _) = open_parcel();
        assert_eq!(
            order.clone().cancel("", staff(), at(2_000)).err(),
            Some(OrderError::ReasonRequired)
        );
        assert_eq!(
            order.clone().cancel("   \t ", staff(), at(2_000)).err(),
            Some(OrderError::ReasonRequired)
        );
        let cancelled = order
            .cancel("  customer walked out  ", staff(), at(2_000))
            .expect("cancels");
        assert_eq!(cancelled.reason, "customer walked out", "stored trimmed");
        assert_eq!(cancelled.cancelled_by, staff());

        let settled = settled_order();
        assert_eq!(
            settled.clone().void(" ", staff(), at(3_000)).err(),
            Some(OrderError::ReasonRequired)
        );
        assert_eq!(
            settled.void("billed twice", staff(), at(3_000)).expect("voids").reason,
            "billed twice"
        );
    }

    /// A ₹100 parcel order, paid in cash.
    fn settled_order() -> SettledOrder {
        let (mut order, _) = open_parcel();
        order
            .core
            .cart
            .add(item("dosa", 10_000), Qty::ONE, None, vec![])
            .expect("adds");

        let bill = compute_bill(BillInput::new(&order.core.cart).with_rounding(RoundingMode::None))
            .expect("computes");
        let mut settlement = Settlement::new();
        settlement
            .add(Payment::new(PaymentMode::Cash, bill.grand_total).expect("valid"))
            .expect("adds");

        order.settle(bill, settlement, at(2_500), staff()).expect("settles")
    }

    #[test]
    fn a_bill_that_is_not_covered_cannot_be_settled() {
        let (mut order, _) = open_parcel();
        order
            .core
            .cart
            .add(item("dosa", 10_000), Qty::ONE, None, vec![])
            .expect("adds");
        let bill = compute_bill(BillInput::new(&order.core.cart).with_rounding(RoundingMode::None))
            .expect("computes");

        // Underpaid.
        let mut short = Settlement::new();
        short
            .add(Payment::new(PaymentMode::Cash, Money::from_paise(5_000)).expect("valid"))
            .expect("adds");
        assert_eq!(
            order.clone().settle(bill.clone(), short, at(2_500), staff()).err(),
            Some(OrderError::NotFullyPaid)
        );

        // Overpaid on a card — P02's rule, still true one layer up.
        let mut card = Settlement::new();
        card.add(
            Payment::new(PaymentMode::Card, bill.grand_total.add(Money::from_paise(10_000)).expect("adds"))
                .expect("valid"),
        )
        .expect("adds");
        assert!(matches!(
            order.clone().settle(bill.clone(), card, at(2_500), staff()),
            Err(OrderError::Payment(_))
        ));

        // Overpaid in cash is fine — the difference is change.
        let mut cash = Settlement::new();
        cash.add(
            Payment::new(PaymentMode::Cash, bill.grand_total.add(Money::from_paise(10_000)).expect("adds"))
                .expect("valid"),
        )
        .expect("adds");
        assert!(order.settle(bill, cash, at(2_500), staff()).is_ok());
    }

    #[test]
    fn a_settled_order_keeps_its_original_time_and_numbers() {
        // The audit's exact words (2.3): "keeping its original time, bill
        // number and token".
        let settled = settled_order();
        assert_eq!(settled.core.created_at, at(1_000), "the original creation time");
        assert_eq!(settled.settled_at, at(2_500));
        assert_eq!(settled.token.value, 1);
        assert_eq!(settled.bill_number.value, 1);
        assert_eq!(settled.core.business_day, day());
    }

    #[test]
    fn a_voided_bill_keeps_its_number_and_its_amounts_forever() {
        // B5's fix needs a voided-bills report, which means the amounts have
        // to survive the void.
        let settled = settled_order();
        let original_total = settled.bill.grand_total;
        let number = settled.bill_number.clone();

        let voided = settled.void("wrong table", staff(), at(4_000)).expect("voids");
        assert_eq!(voided.bill_number, number, "the number does not change");
        assert_eq!(voided.bill.grand_total, original_total, "gross takings stay readable");
        assert_eq!(voided.settled_at, at(2_500), "when it was taken");
        assert_eq!(voided.voided_at, at(4_000), "and when it was reversed");

        // And the number is never handed out again.
        let mut numbering = Numbering::new();
        numbering.claim_for_new_order(day());
        let (_, next_bill) = numbering.claim_for_new_order(day());
        assert_ne!(next_bill.value, number.value);
        assert_eq!(next_bill.value, 2);
    }

    #[test]
    fn the_kitchen_only_hears_about_what_it_has_not_heard() {
        // Crown jewel 2. Order 2 dosa, print, add 1 dosa and 1 idli.
        let (mut order, _) = open_parcel();
        let cart = &mut order.core.cart;
        cart.add(item("dosa", 8_000), Qty::from_whole(2).expect("in range"), None, vec![])
            .expect("adds");

        let first = order.core.kitchen.pending(&order.core.cart).expect("computes");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].1, Qty::from_whole(2).expect("in range"));
        order.core.kitchen.mark_printed(&first).expect("marks");

        // Nothing new yet.
        assert!(
            order.core.kitchen.pending(&order.core.cart).expect("computes").is_empty(),
            "a second print with no changes must send nothing"
        );

        // One more dosa and an idli.
        order
            .core
            .cart
            .add(item("dosa", 8_000), Qty::ONE, None, vec![])
            .expect("adds");
        order
            .core
            .cart
            .add(item("idli", 4_000), Qty::ONE, None, vec![])
            .expect("adds");

        let second = order.core.kitchen.pending(&order.core.cart).expect("computes");
        assert_eq!(second.len(), 2, "exactly one dosa and one idli, not three and one");
        assert_eq!(second[0].0.item_id, ItemId::new("dosa"));
        assert_eq!(second[0].1, Qty::ONE);
        assert_eq!(second[1].0.item_id, ItemId::new("idli"));
        assert_eq!(second[1].1, Qty::ONE);

        order.core.kitchen.mark_printed(&second).expect("marks");
        assert!(order.core.kitchen.pending(&order.core.cart).expect("computes").is_empty());
    }

    #[test]
    fn the_kitchen_ticket_reads_in_the_order_the_waiter_called_it() {
        let (mut order, _) = open_parcel();
        for name in ["soup", "starter", "main"] {
            order
                .core
                .cart
                .add(item(name, 5_000), Qty::ONE, None, vec![])
                .expect("adds");
        }
        let pending = order.core.kitchen.pending(&order.core.cart).expect("computes");
        let names: Vec<&str> = pending.iter().map(|(id, _)| id.item_id.as_str()).collect();
        assert_eq!(names, ["soup", "starter", "main"]);
    }

    #[test]
    fn food_the_kitchen_no_longer_has_to_cook_is_reported() {
        // The case `pending` alone would drop silently. P12 turns this into a
        // cancellation slip.
        let (mut order, _) = open_parcel();
        order
            .core
            .cart
            .add(item("dosa", 8_000), Qty::from_whole(3).expect("in range"), None, vec![])
            .expect("adds");

        let ticket = order.core.kitchen.pending(&order.core.cart).expect("computes");
        order.core.kitchen.mark_printed(&ticket).expect("marks");

        // Cut it down to one.
        order.core.cart.set_qty(0, Qty::ONE).expect("sets");
        assert!(
            order.core.kitchen.pending(&order.core.cart).expect("computes").is_empty(),
            "nothing new to cook"
        );
        let over = order.core.kitchen.over_told(&order.core.cart).expect("computes");
        assert_eq!(over.len(), 1);
        assert_eq!(over[0].1, Qty::from_whole(2).expect("in range"), "two dosa to cancel");

        // Remove the line entirely and all three are surplus.
        order.core.cart.remove(0).expect("removes");
        let over = order.core.kitchen.over_told(&order.core.cart).expect("computes");
        assert_eq!(over[0].1, Qty::from_whole(3).expect("in range"));
    }

    /// **P12's half of the seam.** Without it, the same line comes back on
    /// every cancellation slip for the rest of the order.
    #[test]
    fn telling_the_kitchen_to_stop_is_remembered() {
        let (mut order, _) = open_parcel();
        order
            .core
            .cart
            .add(item("dosa", 8_000), Qty::from_whole(3).expect("in range"), None, vec![])
            .expect("adds");
        let ticket = order.core.kitchen.pending(&order.core.cart).expect("computes");
        order.core.kitchen.mark_printed(&ticket).expect("marks");

        // Void one of the three.
        order.core.cart.set_qty(0, Qty::from_whole(2).expect("in range")).expect("sets");
        let cancel = order.core.kitchen.over_told(&order.core.cart).expect("computes");
        assert_eq!(cancel[0].1, Qty::ONE, "one dosa to stop");

        order.core.kitchen.mark_cancelled(&cancel).expect("marks");

        // **The slip does not print again.** This is the whole point.
        assert!(
            order.core.kitchen.over_told(&order.core.cart).expect("computes").is_empty(),
            "the kitchen would have been told to stop the same dosa twice"
        );
        // And the kitchen still believes in the two that are left, so nothing
        // is re-sent either.
        assert!(
            order.core.kitchen.pending(&order.core.cart).expect("computes").is_empty(),
            "a cancelled line came back as something new to cook"
        );
    }

    /// Cancelling the whole line drops it from the ledger rather than leaving a
    /// zero behind — the ledger is what the kitchen believes now, not history.
    #[test]
    fn cancelling_everything_empties_the_ledger() {
        let (mut order, _) = open_parcel();
        order
            .core
            .cart
            .add(item("dosa", 8_000), Qty::from_whole(2).expect("in range"), None, vec![])
            .expect("adds");
        let ticket = order.core.kitchen.pending(&order.core.cart).expect("computes");
        order.core.kitchen.mark_printed(&ticket).expect("marks");

        order.core.cart.remove(0).expect("removes");
        let cancel = order.core.kitchen.over_told(&order.core.cart).expect("computes");
        order.core.kitchen.mark_cancelled(&cancel).expect("marks");

        assert!(order.core.kitchen.is_empty(), "a zero row was left behind");
    }

    /// Being asked to stop more than the kitchen was told is not a failure a
    /// cashier can do anything about, and refusing it would leave the ledger
    /// insisting the kitchen is still cooking something it was told to stop.
    #[test]
    fn cancelling_more_than_was_ever_told_is_not_an_error() {
        let (mut order, _) = open_parcel();
        order
            .core
            .cart
            .add(item("dosa", 8_000), Qty::ONE, None, vec![])
            .expect("adds");
        let ticket = order.core.kitchen.pending(&order.core.cart).expect("computes");
        order.core.kitchen.mark_printed(&ticket).expect("marks");

        let identity = ticket[0].0.clone();
        order
            .core
            .kitchen
            .mark_cancelled(&[(identity, Qty::from_whole(9).expect("in range"))])
            .expect("does not refuse");
        assert!(order.core.kitchen.is_empty());
    }

    /// A line the kitchen never heard of is not its problem.
    #[test]
    fn cancelling_a_line_the_kitchen_never_heard_of_changes_nothing() {
        let (mut order, _) = open_parcel();
        order
            .core
            .cart
            .add(item("dosa", 8_000), Qty::ONE, None, vec![])
            .expect("adds");
        let ticket = order.core.kitchen.pending(&order.core.cart).expect("computes");
        order.core.kitchen.mark_printed(&ticket).expect("marks");

        let stranger = LineIdentity {
            item_id: ItemId::new("itm_never"),
            note: None,
            modifier_ids: vec![],
        };
        order
            .core
            .kitchen
            .mark_cancelled(&[(stranger, Qty::ONE)])
            .expect("does not refuse");
        assert_eq!(order.core.kitchen.told().len(), 1, "the real line was touched");
    }

    #[test]
    fn a_note_makes_the_kitchen_delta_a_different_line() {
        // The identity the ledger keys on is the same one the cart merges by,
        // so "extra spicy" is genuinely a separate thing to cook.
        let (mut order, _) = open_parcel();
        order
            .core
            .cart
            .add(item("dosa", 8_000), Qty::ONE, None, vec![])
            .expect("adds");
        order
            .core
            .cart
            .add(item("dosa", 8_000), Qty::ONE, Some("extra spicy".to_owned()), vec![])
            .expect("adds");

        let pending = order.core.kitchen.pending(&order.core.cart).expect("computes");
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].0.note, None);
        assert_eq!(pending[1].0.note, Some("extra spicy".to_owned()));
    }

    #[test]
    fn the_ledger_survives_a_round_trip_through_storage() {
        // It lives in the database at P04, and the identity is the key.
        let (mut order, _) = open_parcel();
        order
            .core
            .cart
            .add(item("dosa", 8_000), Qty::from_thousandths(1_500), Some("crispy".to_owned()), vec![])
            .expect("adds");
        let ticket = order.core.kitchen.pending(&order.core.cart).expect("computes");
        order.core.kitchen.mark_printed(&ticket).expect("marks");

        let json = serde_json::to_string(&order.core.kitchen).expect("serialises");
        let restored: KitchenLedger = serde_json::from_str(&json).expect("reads");
        assert_eq!(restored, order.core.kitchen);
        assert!(
            restored.pending(&order.core.cart).expect("computes").is_empty(),
            "a reloaded order must not re-send the whole ticket"
        );
    }

    #[test]
    fn any_order_can_hold_every_state_for_storage() {
        let (open, _) = open_parcel();
        let settled = settled_order();
        let cancelled = open
            .clone()
            .cancel("walked out", staff(), at(9_000))
            .expect("cancels");
        let voided = settled.clone().void("wrong bill", staff(), at(9_500)).expect("voids");

        let all = vec![
            AnyOrder::Draft(draft(OrderType::Parcel)),
            AnyOrder::Open(open),
            AnyOrder::Settled(settled),
            AnyOrder::Cancelled(cancelled),
            AnyOrder::Voided(voided),
        ];

        assert_eq!(all[0].bill_number(), None, "a draft has no number yet");
        for order in all.iter().skip(1) {
            assert!(order.bill_number().is_some());
        }
        for order in &all {
            assert_eq!(order.core().business_day, day());
        }

        let json = serde_json::to_string(&all).expect("serialises");
        let restored: Vec<AnyOrder> = serde_json::from_str(&json).expect("reads");
        assert_eq!(restored, all);
    }
}
