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
pub mod composition;
pub mod corrections;
pub mod floor;
pub mod menu;
pub mod money;
pub mod order;
pub mod outbox;
pub mod people;
pub mod print_jobs;
pub mod settings;
pub mod taxclass;

pub use audit::{AuditFilter, AuditRepo};
pub use composition::{Combo, ComboPart, CompositionRepo, Modifier, ModifierGroup, Variant};
pub use corrections::{CorrectionsRepo, DayTotals, Reason, Refund, ReprintRow};
pub use floor::FloorRepo;
pub use menu::MenuRepo;
pub use money::MoneyRepo;
pub use order::OrderRepo;
pub use outbox::{Op, OutboxRepo, OutboxRow};
pub use people::{PeopleRepo, StaffMember};
pub use print_jobs::{PrintJobRepo, PrintJobRow};
pub use settings::{SettingValue, SettingsRepo};
pub use taxclass::TaxClassRepo;

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

    #[must_use]
    pub fn settings(&self) -> SettingsRepo<'a> {
        SettingsRepo::new(self.tx)
    }

    #[must_use]
    pub fn outbox(&self) -> OutboxRepo<'a> {
        OutboxRepo::new(self.tx)
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
