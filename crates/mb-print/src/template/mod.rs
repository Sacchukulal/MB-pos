//! The templates: the only place in the product that knows what a receipt looks like.

pub mod bill;
pub mod dayclose;
pub mod delivery;
pub mod kitchen;
/// The payslip.
pub mod payslip;
/// The shop's recovery code, on paper.
pub mod recovery;

pub use bill::{BillContext, BillCustomer, Copy, EInvoice, Store, bill_document};
pub use dayclose::{CountedNote, DayCloseContext, SlipLine, day_close_document};
pub use delivery::{DeliveryContext, SlipLine as DeliverySlipLine, delivery_document};
pub use kitchen::{
    KitchenContext, LabelContext, TicketKind, TicketLine, kitchen_document, label_document,
};
pub use payslip::{PaySlipLine, PayslipContext, payslip_document};
pub use recovery::{RecoveryContext, recovery_document};
