//! The repositories: the **only** way anything above this crate touches a row.
//!
//! Each one takes and returns mb-core domain types, never raw rows. Every
//! coercion — integer to `bool`, paise to [`Money`](mb_core::Money), tag to
//! enum — happens here and nowhere else, through [`crate::encode`].
//!
//! v1 had roughly sixty inline coercions scattered across screen files, which
//! is why nobody could answer "what exactly happens when a bill is settled?"
//! without reading four files at once (audit E3). A screen that writes SQL is a
//! screen that has to be found again at P30.
//!
//! # The transaction belongs to the caller
//!
//! [`Repos`] borrows a [`Transaction`]; it never owns a connection. Only the
//! caller knows whether this is one step of a settle or a standalone edit, and
//! a repository that opened its own transaction would make
//! [`crate::settle`] — which must be exactly one commit and therefore exactly
//! one fsync (D23, budget B5) — impossible to write.
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
/// P26. Suppliers, the paper, the supplier ledger and purchase orders —
/// **one rupee, one row** (D120).
pub mod buying;
pub mod composition;
/// P26. The physical stock count, which **freezes the book and posts a delta**
/// (D127).
pub mod counts;
pub mod corrections;
pub mod devices;
pub mod kitchen;
pub mod employment;
pub mod events;
pub mod floor;
pub mod menu;
pub mod menucsv;
pub mod money;
pub mod order;
pub mod outbox;
pub mod people;
pub mod print_jobs;
/// P18. Every report, and every one of them groups by the STORED business day
/// (D5) — which is audit B1, the bill that appeared on two different days on
/// two different screens.
pub mod reports;
pub mod settings;
/// P25. Materials, recipes and the append-only stock ledger — and the one
/// function in this crate that is deliberately incapable of refusing a bill
/// (D112).
pub mod stock;
/// P27. The tills â **every terminal has its own series** (D135), and moving
/// the master is a decision a person makes (D139).
pub mod terminals;
pub mod taxclass;

pub use audit::{AuditFilter, AuditRepo};
pub use buying::{
    Attachment, BuyingRepo, OrderLine, OrderState, Outstanding, Purchase, PurchaseKind,
    PurchaseLine, PurchaseOrder, Supplier, SupplierAdjustment, SupplierMaterial, SupplierPayment,
};
pub use counts::{CountLine, CountRepo, CountState, StockCount, Written};
pub use composition::{Combo, ComboPart, CompositionRepo, Modifier, ModifierGroup, Variant};
pub use corrections::{CorrectionsRepo, DayTotals, Reason, Refund, ReprintRow};
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

    /// The audit trail (P11). Like the print spool it raises **no outbox row**,
    /// and unlike the print spool that is not about size — it is D16: nothing
    /// on the phone reads it, so nothing pays to send it.
    #[must_use]
    pub fn audit(&self) -> AuditRepo<'a> {
        AuditRepo::new(self.tx)
    }

    #[must_use]
    pub fn money(&self) -> MoneyRepo<'a> {
        MoneyRepo::new(self.tx)
    }

    /// **The employment side** (P28): shifts, attendance, leave, salary and
    /// payroll.
    ///
    /// `people()` above is the IDENTITY side — who this is and what they may
    /// do — and the two are deliberately separate, because a shift supervisor
    /// reads one of them and the owner reads the other.
    #[must_use]
    pub fn employment(&self) -> employment::EmploymentRepo<'a> {
        employment::EmploymentRepo::new(self.tx)
    }

    #[must_use]
    pub fn settings(&self) -> SettingsRepo<'a> {
        SettingsRepo::new(self.tx)
    }

    #[must_use]
    pub fn outbox(&self) -> OutboxRepo<'a> {
        OutboxRepo::new(self.tx)
    }

    /// The menu in and out of a spreadsheet (P13) — a shop with 400 items
    /// will not type them in.
    #[must_use]
    pub fn menu_csv(&self) -> MenuCsvRepo<'a> {
        MenuCsvRepo::new(self.tx)
    }

    /// Variants, modifiers and combos (P13) — the three ways one menu row
    /// becomes several things a customer can order.
    #[must_use]
    pub fn composition(&self) -> CompositionRepo<'a> {
        CompositionRepo::new(self.tx)
    }

    /// The shop's tax classes (P13) — and the one operation that matters:
    /// editing a class rewrites the live menu and cannot reach a bill.
    #[must_use]
    pub fn tax_classes(&self) -> TaxClassRepo<'a> {
        TaxClassRepo::new(self.tx)
    }

    /// Reprints, refunds, reasons and the day reconciliation (P12).
    #[must_use]
    pub fn corrections(&self) -> CorrectionsRepo<'a> {
        CorrectionsRepo::new(self.tx)
    }

    /// P19 devices — the phones this counter serves.
    #[must_use]
    pub fn devices(&self) -> devices::DevicesRepo<'a> {
        devices::DevicesRepo::new(self.tx)
    }

    /// P24 — what the kitchen was told, and what became of it.
    #[must_use]
    pub fn kitchen(&self) -> kitchen::KitchenRepo<'a> {
        kitchen::KitchenRepo::new(self.tx)
    }

    /// P25 — materials, recipes and the stock ledger. **The one repository
    /// whose main function cannot refuse a bill** (D112).
    #[must_use]
    pub fn stock(&self) -> StockRepo<'a> {
        StockRepo::new(self.tx)
    }

    /// P26 — suppliers, the paper and what the shop owes. **Writes the shelf,
    /// the paper and the ledger in one transaction, and no `expenses` row**
    /// (D120).
    #[must_use]
    pub fn buying(&self) -> BuyingRepo<'a> {
        BuyingRepo::new(self.tx)
    }

    /// P26 — the physical stock count (D127).
    #[must_use]
    pub fn counts(&self) -> CountRepo<'a> {
        CountRepo::new(self.tx)
    }

    /// P27 — the tills. **The billing path never asks this repository for a
    /// number** (D135): a counter row is claimed inside the settle transaction,
    /// exactly as it has been since P05.
    #[must_use]
    pub fn terminals(&self) -> TerminalRepo<'a> {
        TerminalRepo::new(self.tx)
    }

    /// Every report (P18), grouped by the stored business day.
    #[must_use]
    pub fn reports(&self) -> crate::repo::reports::ReportsRepo<'a> {
        crate::repo::reports::ReportsRepo::new(self.tx)
    }

    /// What happened to an order and when (P14) — the first writer of
    /// `order_events`, which P04 modelled and nothing had used.
    #[must_use]
    pub fn events(&self) -> crate::repo::events::EventsRepo<'a> {
        crate::repo::events::EventsRepo::new(self.tx)
    }

    /// The print spool (P07). The one repository that deliberately does **not**
    /// enqueue an outbox row — see its module documentation.
    #[must_use]
    pub fn print_jobs(&self) -> PrintJobRepo<'a> {
        PrintJobRepo::new(self.tx)
    }

    /// Escape hatch for the perf harness and for `settle`, which needs the same
    /// transaction to claim a number through [`crate::numbering`].
    #[must_use]
    pub fn tx(&self) -> &'a Transaction<'a> {
        self.tx
    }
}
