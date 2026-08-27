//! Magic Bill — billing core.

#![deny(missing_debug_implementations)]

pub mod bill;
pub mod businessday;
pub mod cart;
pub mod charge;
pub mod combo;
pub mod credit;
/// What the machines plugged into a counter are saying — a scanner, a scale, and the barcodes a
/// scale prints.
pub mod devices;
pub mod discount;
pub mod employment;
pub mod expense;
pub mod ids;
pub mod item;
pub mod kitchen_delivery;
pub mod money;
pub mod numbering;
pub mod order;
pub mod payment;
/// What a phone number is, in the one place that decides it.
pub mod phone;
/// What a delivery actually cost — pure, and the free bag is a denominator.
pub mod provider;
pub mod purchase;
pub mod qty;
/// Recipes, and what a sale takes off the shelf — pure, and deliberately incapable of returning
/// an error.
pub mod recipe;
pub mod table;
pub mod tax;
pub mod taxclass;
pub mod time;
pub mod transfer;
/// Units, packs and the cost of a unit — where inventory actually fails.
pub mod units;

pub use bill::{Bill, BillError, BillInput, BillLine, compute_bill};
pub use businessday::{BusinessDay, DayRule};
pub use cart::{Cart, CartError, CartLine, LineIdentity};
pub use charge::{BillCharge, Charge, ChargeBasis, ChargeKind};
pub use combo::{Apportioned, ComboComponent, ComboError, apportion};
pub use discount::{Discount, DiscountEntry, DiscountOutcome, DiscountPolicy, DiscountPolicyError};
pub use ids::{CategoryId, CustomerId, ItemId, MaterialId, ModifierId, OrderId, StaffId, TableId};
pub use item::{ItemSnapshot, Modifier, OrderType};
pub use kitchen_delivery::{
    ACK_SECONDS, Action as DeliveryAction, Delivery, DeliveryError, State as DeliveryState,
};
pub use money::{Money, MoneyError, RoundingMode};
pub use numbering::{Claimed, Counter, Numbering};
pub use order::{
    AnyOrder, CancelledOrder, DraftOrder, KitchenLedger, OpenOrder, OrderCore, OrderError,
    SettledOrder, VoidedOrder,
};
pub use payment::{Payment, PaymentError, PaymentMode, Settlement};
pub use phone::{PHONE_DIGITS, Phone, PhoneError};
// Not `Costed` or `CostedLine` — `recipe` already exports both, and two types with one name in
// the prelude is how a caller ends up costing a delivery with a recipe's arithmetic.
pub use purchase::{CostedInvoice, Entry as PurchaseEntry, Invoice, PurchaseError, cost_invoice};
pub use qty::{Qty, QtyError};
pub use recipe::{
    Costed, CostedLine, Draw, Explosion, MAX_DEPTH, MaterialFacts, Problem, Production, Recipe,
    RecipeLine, RecipeOwner, Recipes, Sold, cost_of, explode,
};
pub use table::{SubTable, TableError, clashes_with, printed_name, printed_seat, same_table};
pub use tax::{
    GstAmounts, PlaceOfSupply, PriceBasis, RateSummaryRow, Registration, StateTax, TaxKind,
    TaxOutcome, TaxRate, TaxSpec, TaxSummary, Vat, VatSummaryRow,
};
pub use taxclass::{TaxClass, TaxClassId, starting_classes};
pub use time::{TimeError, Timestamp, UtcOffset};
pub use transfer::{Pick, Portion, TransferError, even_shares, merge_into, take_lines};
pub use units::{Dimension, Pack, UnitCost, UnitError, Units};
