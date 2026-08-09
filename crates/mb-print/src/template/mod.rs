//! The templates: **the only place in the product that knows what a receipt
//! looks like.**
//!
//! Adding a field to a bill is a change to `bill.rs` and to nothing else. That
//! is the sentence audit D1 asked for, and if it ever stops being true this
//! crate has failed at the one job it was created to do.

pub mod bill;
/// P18's closing slip — audit B15's Z-report.
pub mod dayclose;
pub mod kitchen;

pub use bill::{BillContext, BillCustomer, Copy, EInvoice, Store, bill_document};
pub use dayclose::{CountedNote, DayCloseContext, SlipLine, day_close_document};
pub use kitchen::{
    KitchenContext, LabelContext, TicketKind, TicketLine, kitchen_document, label_document,
};
