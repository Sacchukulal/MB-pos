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

/// What the paper calls an order type — the bill and the kitchen ticket say the same word.
#[must_use]
pub const fn order_type_label(kind: mb_core::OrderType) -> &'static str {
    match kind {
        mb_core::OrderType::DineIn => "Dine In",
        mb_core::OrderType::Parcel => "Parcel",
        mb_core::OrderType::SelfService => "Self Service",
        mb_core::OrderType::Delivery => "Delivery",
    }
}

/// The row-height setting, applied: `gap` blank rows before every item but the first. Not
/// after the last — trailing air is what the closing spacer is for, and doubling it wastes
/// paper on every ticket of the day.
pub(crate) fn gap_before(rows: &mut Vec<Vec<String>>, index: usize, gap: usize, columns: usize) {
    if index == 0 {
        return;
    }
    for _ in 0..gap {
        rows.push(vec![String::new(); columns]);
    }
}
