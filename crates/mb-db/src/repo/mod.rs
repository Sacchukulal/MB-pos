//! The repositories: the only way anything above this crate touches a row.
//!
//! ```ignore
//! db.transaction(|tx| {
//!     let repos = Repos::new(tx);
//!     repos.menu().save_item(&item)?;
//!     repos.orders().save(&order)
//! })
//! ```

use rusqlite::Transaction;

pub mod audit;
/// Suppliers, the paper, the supplier ledger and purchase orders — one rupee, one row.
pub mod buying;
pub mod composition;
pub mod corrections;
/// The physical stock count, which freezes the book and posts a delta.
pub mod counts;
/// The business day: its lock, its kind and what it came to.
pub mod days;
pub mod delivery;
pub mod devices;
pub mod employment;
pub mod events;
pub mod floor;
pub mod kitchen;
pub mod menu;
pub mod menucsv;
pub mod money;
pub mod notices;
pub mod order;
pub mod outbox;
pub mod payments;
pub mod people;
pub mod print_jobs;
/// Every report, and every one of them groups by the STORED business day.
pub mod reports;
pub mod settings;
/// Materials, recipes and the append-only stock ledger — and the one function in this crate
/// that is deliberately incapable of refusing a bill.
pub mod stock;
pub mod taxclass;
/// The tills â every terminal has its own series, and moving the master is a decision a
/// person makes.
pub mod terminals;
pub mod wire;

pub use audit::{AuditFilter, AuditRepo};
pub use buying::{
    Attachment, BuyingRepo, OrderLine, OrderState, Outstanding, Purchase, PurchaseKind,
    PurchaseLine, PurchaseOrder, Supplier, SupplierAdjustment, SupplierMaterial, SupplierPayment,
};
pub use composition::{Combo, ComboPart, CompositionRepo, Modifier, ModifierGroup, Variant};
pub use corrections::{CorrectionsRepo, DayTotals, Reason, Refund, ReprintRow};
pub use counts::{CountLine, CountRepo, CountState, StockCount, Written};
pub use days::{DayFigures, DayKind, DayRow, DaysRepo};
pub use floor::FloorRepo;
pub use menu::MenuRepo;
pub use menucsv::{ImportPlan, MenuCsvRepo};
pub use money::MoneyRepo;
pub use order::OrderRepo;
pub use outbox::{Op, OutboxRepo, OutboxRow};
pub use people::{PeopleRepo, StaffMember};
pub use print_jobs::{PrintJobRepo, PrintJobRow};
pub use settings::{SettingValue, SettingsRepo};
pub use stock::{
    ConsumptionRow, Material, Movement, MovementKind, MovementRow, OnHand, ProblemRow, StockRepo,
};
pub use taxclass::TaxClassRepo;
pub use terminals::{Terminal, TerminalRepo};

/// Every repository, over one transaction.
#[derive(Debug)]
pub struct Repos<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> Repos<'a> {
    #[must_use]
    pub fn new(tx: &'a Transaction<'a>) -> Self {
        Repos { tx }
    }

    #[must_use]
    pub fn menu(&self) -> MenuRepo<'a> {
        MenuRepo::new(self.tx)
    }

    #[must_use]
    pub fn floor(&self) -> FloorRepo<'a> {
        FloorRepo::new(self.tx)
    }

    #[must_use]
    pub fn orders(&self) -> OrderRepo<'a> {
        OrderRepo::new(self.tx)
    }

    #[must_use]
    pub fn people(&self) -> PeopleRepo<'a> {
        PeopleRepo::new(self.tx)
    }

    #[must_use]
    pub fn audit(&self) -> AuditRepo<'a> {
        AuditRepo::new(self.tx)
    }

    #[must_use]
    pub fn money(&self) -> MoneyRepo<'a> {
        MoneyRepo::new(self.tx)
    }

    /// The employment side: shifts, attendance, leave, salary and payroll.
    #[must_use]
    pub fn employment(&self) -> employment::EmploymentRepo<'a> {
        employment::EmploymentRepo::new(self.tx)
    }

    /// Orders that leave on a bike, and the cash a rider is carrying — which is the half of
    /// delivery where money actually goes.
    #[must_use]
    pub fn delivery(&self) -> delivery::DeliveryRepo<'a> {
        delivery::DeliveryRepo::new(self.tx)
    }

    #[must_use]
    pub fn settings(&self) -> SettingsRepo<'a> {
        SettingsRepo::new(self.tx)
    }

    #[must_use]
    pub fn notices(&self) -> notices::NoticesRepo<'a> {
        notices::NoticesRepo::new(self.tx)
    }

    #[must_use]
    pub fn wire(&self) -> wire::WireRepo<'a> {
        wire::WireRepo::new(self.tx)
    }

    #[must_use]
    pub fn outbox(&self) -> OutboxRepo<'a> {
        OutboxRepo::new(self.tx)
    }

    /// The menu in and out of a spreadsheet — a shop with 400 items will not type them in.
    #[must_use]
    pub fn menu_csv(&self) -> MenuCsvRepo<'a> {
        MenuCsvRepo::new(self.tx)
    }

    /// Variants, modifiers and combos — the three ways one menu row becomes several things a
    /// customer can order.
    #[must_use]
    pub fn composition(&self) -> CompositionRepo<'a> {
        CompositionRepo::new(self.tx)
    }

    /// The shop's tax classes — and the one operation that matters: editing a class rewrites
    /// the live menu and cannot reach a bill.
    #[must_use]
    pub fn tax_classes(&self) -> TaxClassRepo<'a> {
        TaxClassRepo::new(self.tx)
    }

    /// Reprints, refunds, reasons and the day reconciliation.
    #[must_use]
    pub fn corrections(&self) -> CorrectionsRepo<'a> {
        CorrectionsRepo::new(self.tx)
    }

    /// What a payment provider said, and every payment nobody has confirmed.
    #[must_use]
    pub fn payments(&self) -> payments::PaymentsRepo<'a> {
        payments::PaymentsRepo::new(self.tx)
    }

    #[must_use]
    pub fn devices(&self) -> devices::DevicesRepo<'a> {
        devices::DevicesRepo::new(self.tx)
    }

    /// What the kitchen was told, and what became of it.
    #[must_use]
    pub fn kitchen(&self) -> kitchen::KitchenRepo<'a> {
        kitchen::KitchenRepo::new(self.tx)
    }

    /// Materials, recipes and the stock ledger.
    #[must_use]
    pub fn stock(&self) -> StockRepo<'a> {
        StockRepo::new(self.tx)
    }

    /// Suppliers, the paper and what the shop owes.
    #[must_use]
    pub fn buying(&self) -> BuyingRepo<'a> {
        BuyingRepo::new(self.tx)
    }

    /// The physical stock count.
    #[must_use]
    pub fn counts(&self) -> CountRepo<'a> {
        CountRepo::new(self.tx)
    }

    /// The business day: the lock every money path checks, and what the day came to.
    #[must_use]
    pub fn days(&self) -> DaysRepo<'a> {
        DaysRepo::new(self.tx)
    }

    /// The tills.
    #[must_use]
    pub fn terminals(&self) -> TerminalRepo<'a> {
        TerminalRepo::new(self.tx)
    }

    /// Every report, grouped by the stored business day.
    #[must_use]
    pub fn reports(&self) -> crate::repo::reports::ReportsRepo<'a> {
        crate::repo::reports::ReportsRepo::new(self.tx)
    }

    /// What happened to an order and when.
    #[must_use]
    pub fn events(&self) -> crate::repo::events::EventsRepo<'a> {
        crate::repo::events::EventsRepo::new(self.tx)
    }

    /// The print spool.
    #[must_use]
    pub fn print_jobs(&self) -> PrintJobRepo<'a> {
        PrintJobRepo::new(self.tx)
    }

    /// Escape hatch for the perf harness and for `settle`, which needs the same transaction to
    /// claim a number through `crate::numbering`.
    #[must_use]
    pub fn tx(&self) -> &'a Transaction<'a> {
        self.tx
    }
}
