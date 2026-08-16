//! Magic Bill — billing core.
//!
//! Everything a bill is made of, with no screen, no database and no printer in
//! sight. This crate can be reasoned about, and tested, on its own.
//!
//! The rules it owns are recorded in `docs/DECISIONS.md`. The ones that shape
//! every line of code here:
//!
//! * **D2 — money is an integer count of paise.** No floating point exists in
//!   the money path.
//! * **D4 — the order of operations on a bill is fixed**: line gross, line
//!   discount, bill discount spread across lines, per-line tax, charges,
//!   totals, then round-off on the grand total only.
//! * **D7 — nothing may be silently lossy.** A value that cannot be
//!   represented, or a discount that could not take what it was asked for, is
//!   reported rather than quietly adjusted.
//! * **D12 — speed is a budget with a number.** `compute_bill` owns budget B4
//!   in `docs/PERFORMANCE.md`, asserted by `tests/perf.rs`.
//!
//! # The layers
//!
//! Each module depends only on the ones above it, and keeping that order is
//! what lets any piece be tested without the rest.
//!
//! | module | owns |
//! |---|---|
//! | [`money`] | rupees as integer paise, and the one place rounding happens |
//! | [`tax`] | GST per line: rate, treatment, CGST/SGST/IGST, the rate summary |
//! | [`qty`] | quantity in thousandths, and price × quantity |
//! | [`ids`] | the identity newtypes |
//! | [`item`] | the frozen item snapshot, modifiers, order type |
//! | [`discount`] | discounts, their policy, and the spread across lines |
//! | [`charge`] | service / packing / delivery, each with its own rate |
//! | [`cart`] | the lines as typed, and the rule that merges them |
//! | [`bill`] | the D4 pipeline, in one place, with the steps numbered |
//! | [`payment`] | split payment, change, tips — after the bill, not in it |
//! | [`time`] | instants, the local offset, and calendar arithmetic |
//! | [`businessday`] | the trading day, stamped once and stored (D5) |
//! | [`numbering`] | token and bill numbers, claimed atomically (D6) |
//! | [`order`] | the lifecycle, as types the compiler enforces |
//! | [`units`] | base units, the shop's own packs, and what a unit costs (D108) |
//! | [`recipe`] | what a dish is made of, and what a sale takes off the shelf |
//! | [`purchase`] | what a delivery actually cost, free bags and tempo included |

#![deny(missing_debug_implementations)]

pub mod bill;
pub mod businessday;
pub mod cart;
pub mod charge;
pub mod combo;
pub mod credit;
/// P29. **What the machines plugged into a counter are saying** — a scanner,
/// a scale, and the barcodes a scale prints. Pure, because none of those
/// devices exists on the machine this was written on.
pub mod devices;
pub mod discount;
pub mod employment;
pub mod expense;
pub mod ids;
pub mod item;
pub mod money;
pub mod numbering;
pub mod kitchen_delivery;
pub mod order;
pub mod payment;
/// P26. What a delivery actually cost — pure, and the free bag is a
/// denominator (D123).
/// P29, scope 8.3/8.4. **Did the money actually arrive?** — the seam a real
/// payment provider drops into, and the honest manual one that ships today.
pub mod provider;
pub mod purchase;
pub mod qty;
/// P25. Recipes, and what a sale takes off the shelf — pure, and deliberately
/// incapable of returning an error (D112).
pub mod recipe;
pub mod table;
pub mod tax;
pub mod taxclass;
pub mod time;
pub mod transfer;
/// P25. Units, packs and the cost of a unit — where inventory actually fails.
pub mod units;

pub use bill::{Bill, BillError, BillInput, BillLine, compute_bill};
pub use businessday::{BusinessDay, DayRule};
pub use cart::{Cart, CartError, CartLine, LineIdentity};
pub use combo::{Apportioned, ComboComponent, ComboError, apportion};
pub use charge::{BillCharge, Charge, ChargeBasis, ChargeKind};
pub use discount::{
    Discount, DiscountEntry, DiscountOutcome, DiscountPolicy, DiscountPolicyError,
};
pub use ids::{CategoryId, CustomerId, ItemId, MaterialId, ModifierId, OrderId, StaffId, TableId};
pub use item::{ItemSnapshot, Modifier, OrderType};
pub use money::{Money, MoneyError, RoundingMode};
pub use numbering::{Claimed, Counter, Numbering};
pub use kitchen_delivery::{
    ACK_SECONDS, Action as DeliveryAction, Delivery, DeliveryError, State as DeliveryState,
};
pub use order::{
    AnyOrder, CancelledOrder, DraftOrder, KitchenLedger, OpenOrder, OrderCore, OrderError,
    SettledOrder, VoidedOrder,
};
pub use payment::{Payment, PaymentError, PaymentMode, Settlement};
// **Not `Costed` or `CostedLine`** — `recipe` already exports both, and two
// types with one name in the prelude is how a caller ends up costing a delivery
// with a recipe's arithmetic. `mb_core::purchase::Costed` is spelled out.
pub use purchase::{
    CostedInvoice, Entry as PurchaseEntry, Invoice, PurchaseError, cost_invoice,
};
pub use qty::{Qty, QtyError};
pub use recipe::{
    Costed, CostedLine, Draw, Explosion, MAX_DEPTH, MaterialFacts, Problem, Production, Recipe,
    RecipeLine, RecipeOwner, Recipes, Sold, cost_of, explode,
};
pub use time::{Timestamp, TimeError, UtcOffset};
pub use units::{Dimension, Pack, UnitCost, UnitError, Units};
pub use taxclass::{OrderTypeRate, TaxClass, TaxClassId, starting_classes};
pub use table::{SubTable, TableError, clashes_with, printed_name, printed_seat, same_table};
pub use transfer::{Pick, Portion, TransferError, even_shares, merge_into, take_lines};
pub use tax::{
    PlaceOfSupply, RateSummaryRow, TaxAmounts, TaxOutcome, TaxRate, TaxSummary, TaxTreatment,
};
