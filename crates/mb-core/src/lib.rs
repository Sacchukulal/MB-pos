//! Magic Bill — billing core.
//!
//! Everything a bill is made of, with no screen, no database and no printer in
//! sight. This crate can be reasoned about, and tested, on its own.
//!
//! The rules it owns are recorded in `v2/docs/DECISIONS.md`. The two that
//! shape every line of code here:
//!
//! * **D2 — money is an integer count of paise.** No floating point exists in
//!   the money path.
//! * **D4 — the order of operations on a bill is fixed**: line gross, line
//!   discount, bill discount spread across lines, per-line tax, charges,
//!   totals, then round-off on the grand total only.

#![deny(missing_debug_implementations)]

pub mod money;
pub mod tax;

pub use money::{Money, MoneyError};
pub use tax::{
    PlaceOfSupply, RateSummaryRow, TaxAmounts, TaxOutcome, TaxRate, TaxSummary, TaxTreatment,
};
