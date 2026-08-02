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
//! | [`discount`] | discounts, and the proportional spread across lines |
//! | [`cart`] | the lines as typed, and the rule that merges them |
//! | [`bill`] | the D4 pipeline, in one place, with the steps numbered |

#![deny(missing_debug_implementations)]

pub mod bill;
pub mod cart;
pub mod discount;
pub mod ids;
pub mod item;
pub mod money;
pub mod qty;
pub mod tax;

pub use bill::{Bill, BillError, BillInput, BillLine, compute_bill};
pub use cart::{Cart, CartError, CartLine};
pub use discount::{Discount, DiscountOutcome};
pub use ids::{CategoryId, ItemId, ModifierId};
pub use item::{ItemSnapshot, Modifier, OrderType};
pub use money::{Money, MoneyError};
pub use qty::{Qty, QtyError};
pub use tax::{
    PlaceOfSupply, RateSummaryRow, TaxAmounts, TaxOutcome, TaxRate, TaxSummary, TaxTreatment,
};
