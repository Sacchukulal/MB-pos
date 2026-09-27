//! Financial state travels with an order, independently of the counter displaying it.
use crate::{DiscountEntry, OrderId, Placement, Settlement};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BillingAccount {
    pub discount: Option<DiscountEntry>,
    pub settlement: Settlement,
    /// Changes to an issued bill advance its revision; retries keep the same revision.
    pub revision: u32,
    /// The original orders included in this bill. Their serving locations remain independent.
    pub sources: Vec<OrderId>,
    pub source_labels: Vec<String>,
    pub serving: Vec<Placement>,
    /// A serving order whose charges now belong to another bill.
    pub billed_into: Option<OrderId>,
    /// The serving order before its bill included food from other tables.
    pub service_cart: Option<crate::Cart>,
    pub service_kitchen: Option<crate::KitchenLedger>,
    /// The token already printed on kitchen tickets before creating a serving-only order.
    pub service_token: Option<String>,
    /// Refund agreed at the counter, committed with the corrected bill.
    pub refund: Option<(String, crate::Money)>,
    /// Return allocations for each payment mode; legacy drafts may use refund above.
    pub refunds: Vec<(String, crate::Money)>,
}
