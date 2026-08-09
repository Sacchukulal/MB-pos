//! **The vocabulary of "no" — twenty-two words, and there are no others.**
//!
//! > BACKEND-**G7**: *"The staff permission map is free-form. Any key can be
//! > written; a typo in a permission name silently means 'denied'. There is no
//! > list of valid permissions anywhere in the database."*
//!
//! P04 fixed half of that with a `permissions` table and a foreign key, so a
//! typo is a constraint violation instead of a silent refusal. This enum is the
//! other half: the codes exist **once**, in Rust, and
//! `mb-db`'s `permission_enum_matches_the_seeded_table` test asserts the enum
//! and the seeded rows are the same set **in both directions**.
//!
//! Both directions, because the two failures are different and neither is
//! visible by reading one file:
//!
//! * a variant with no row is a permission that can never be granted;
//! * a row with no variant is a permission that nothing ever checks — which
//!   looks like security and is not.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::error::AuthError;

/// Everything a person can be allowed to do.
///
/// **Do not add a variant without adding its row to migration 0001 in the same
/// commit.** The test will not let you, which is the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
    BillCreate,
    BillDiscountLine,
    BillDiscountBill,
    BillVoid,
    BillReprint,
    OrderCancel,
    OrderItemVoid,
    DrawerOpen,
    MenuManage,
    TablesManage,
    CustomersManage,
    CreditCollect,
    ExpensesManage,
    ReportsView,
    ReportsExport,
    DayClose,
    SettingsStore,
    SettingsPrinter,
    SettingsTax,
    StaffManage,
    AuditView,
    BackupRun,
    /// P19. Letting a phone onto the counter is its own decision: the person
    /// who may take an order is not automatically the person who may add a
    /// device to the shop's network.
    DevicesPair,
}

impl Permission {
    /// Every permission, in the order the roles screen shows them.
    pub const ALL: &'static [Permission] = &[
        Permission::BillCreate,
        Permission::BillDiscountLine,
        Permission::BillDiscountBill,
        Permission::BillVoid,
        Permission::BillReprint,
        Permission::OrderCancel,
        Permission::OrderItemVoid,
        Permission::DrawerOpen,
        Permission::MenuManage,
        Permission::TablesManage,
        Permission::CustomersManage,
        Permission::CreditCollect,
        Permission::ExpensesManage,
        Permission::ReportsView,
        Permission::ReportsExport,
        Permission::DayClose,
        Permission::SettingsStore,
        Permission::SettingsPrinter,
        Permission::SettingsTax,
        Permission::StaffManage,
        Permission::AuditView,
        Permission::BackupRun,
        Permission::DevicesPair,
    ];

    /// The stored form. **This string is a database value**: changing one is a
    /// migration, not a rename.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Permission::BillCreate => "bill.create",
            Permission::BillDiscountLine => "bill.discount.line",
            Permission::BillDiscountBill => "bill.discount.bill",
            Permission::BillVoid => "bill.void",
            Permission::BillReprint => "bill.reprint",
            Permission::OrderCancel => "order.cancel",
            Permission::OrderItemVoid => "order.item.void",
            Permission::DrawerOpen => "drawer.open",
            Permission::MenuManage => "menu.manage",
            Permission::TablesManage => "tables.manage",
            Permission::CustomersManage => "customers.manage",
            Permission::CreditCollect => "credit.collect",
            Permission::ExpensesManage => "expenses.manage",
            Permission::ReportsView => "reports.view",
            Permission::ReportsExport => "reports.export",
            Permission::DayClose => "day.close",
            Permission::SettingsStore => "settings.store",
            Permission::SettingsPrinter => "settings.printer",
            Permission::SettingsTax => "settings.tax",
            Permission::StaffManage => "staff.manage",
            Permission::AuditView => "audit.view",
            Permission::BackupRun => "backup.run",
            Permission::DevicesPair => "devices.pair",
        }
    }

    /// What a refusal says out loud — the tail of *"you do not have permission
    /// to …"*. Written from the cashier's side of the screen (§6), so it is
    /// "void a bill" rather than "BillVoid" or "bill.void".
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Permission::BillCreate => "take an order",
            Permission::BillDiscountLine => "discount a line",
            Permission::BillDiscountBill => "discount a bill",
            Permission::BillVoid => "void a bill",
            Permission::BillReprint => "reprint a bill",
            Permission::OrderCancel => "cancel an order",
            Permission::OrderItemVoid => "void an item",
            Permission::DrawerOpen => "open the cash drawer",
            Permission::MenuManage => "change the menu",
            Permission::TablesManage => "change the tables",
            Permission::CustomersManage => "manage customers",
            Permission::CreditCollect => "take a credit payment",
            Permission::ExpensesManage => "record expenses",
            Permission::ReportsView => "see reports",
            Permission::ReportsExport => "export reports",
            Permission::DayClose => "close the day",
            Permission::SettingsStore => "change the shop's details",
            Permission::SettingsPrinter => "change the printer setup",
            Permission::SettingsTax => "change tax and numbering",
            Permission::StaffManage => "manage staff",
            Permission::AuditView => "read the history",
            Permission::BackupRun => "take or restore a backup",
            Permission::DevicesPair => "let a phone onto this counter",
        }
    }

    /// Read one back.
    ///
    /// **An unknown code is an error and never a quiet "denied".** A silent
    /// deny is indistinguishable from a correct refusal, which is exactly why
    /// v1's typo could not be found from behind the counter.
    pub fn from_code(code: &str) -> Result<Permission, AuthError> {
        Permission::ALL
            .iter()
            .copied()
            .find(|p| p.code() == code)
            .ok_or_else(|| AuthError::UnknownPermission {
                code: code.to_owned(),
            })
    }
}

/// What one role grants. Ordered, so two equal sets compare equal and a
/// permission grid renders the same way twice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSet(BTreeSet<Permission>);

impl PermissionSet {
    #[must_use]
    pub fn new() -> PermissionSet {
        PermissionSet(BTreeSet::new())
    }

    /// Everything. What the Owner preset holds, and what the stand-in counter
    /// user holds on a shop's first day (see `magic-bill`'s `session`).
    #[must_use]
    pub fn everything() -> PermissionSet {
        PermissionSet(Permission::ALL.iter().copied().collect())
    }

    #[must_use]
    pub fn has(&self, permission: Permission) -> bool {
        self.0.contains(&permission)
    }

    pub fn insert(&mut self, permission: Permission) {
        self.0.insert(permission);
    }

    pub fn remove(&mut self, permission: Permission) {
        self.0.remove(&permission);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = Permission> + '_ {
        self.0.iter().copied()
    }

    /// The stored form, sorted, for a row-per-permission write.
    #[must_use]
    pub fn codes(&self) -> Vec<&'static str> {
        self.0.iter().map(|p| p.code()).collect()
    }

    /// Read a whole set back from the database.
    pub fn from_codes<I, S>(codes: I) -> Result<PermissionSet, AuthError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut set = BTreeSet::new();
        for code in codes {
            set.insert(Permission::from_code(code.as_ref())?);
        }
        Ok(PermissionSet(set))
    }
}

impl FromIterator<Permission> for PermissionSet {
    fn from_iter<I: IntoIterator<Item = Permission>>(iter: I) -> PermissionSet {
        PermissionSet(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_is_in_all() {
        // ALL is written by hand, so it is the thing that rots. There is no way
        // to iterate an enum in Rust without a dependency, and this test is
        // cheaper than one: a variant added below ALL has no row, no screen and
        // no check, and nothing else would notice.
        assert_eq!(Permission::ALL.len(), 23);
        let codes: BTreeSet<&str> = Permission::ALL.iter().map(|p| p.code()).collect();
        assert_eq!(codes.len(), 23, "two permissions share a code");
    }

    #[test]
    fn a_code_round_trips() {
        for p in Permission::ALL {
            assert_eq!(Permission::from_code(p.code()), Ok(*p));
        }
    }

    #[test]
    fn an_unknown_code_is_an_error_not_a_denial() {
        // BACKEND-G7. The whole finding is that this used to return "denied".
        let wrong = Permission::from_code("bill.crate");
        assert_eq!(
            wrong,
            Err(AuthError::UnknownPermission {
                code: "bill.crate".to_owned()
            })
        );
    }

    #[test]
    fn a_set_refuses_a_typo_rather_than_dropping_it() {
        let set = PermissionSet::from_codes(["bill.create", "bill.reprnt"]);
        assert!(set.is_err(), "a typo must not silently shrink the set");
    }

    #[test]
    fn everything_is_everything() {
        let all = PermissionSet::everything();
        assert_eq!(all.len(), Permission::ALL.len());
        assert!(all.has(Permission::StaffManage));
    }

    #[test]
    fn a_refusal_reads_like_a_sentence() {
        // UI_GUIDELINES §6: written from the cashier's side of the screen.
        // Checked crudely on purpose — a test demanding exact phrasing is a
        // test that stops anybody improving the words (words.rs says the same).
        for p in Permission::ALL {
            let what = p.what();
            assert!(!what.contains('_'), "\"{what}\" reads like a tag");
            assert!(!what.contains('.'), "\"{what}\" reads like a code");
            assert!(
                what.starts_with(|c: char| c.is_lowercase()),
                "\"{what}\" must finish the sentence \"you do not have permission to …\""
            );
        }
    }
}
