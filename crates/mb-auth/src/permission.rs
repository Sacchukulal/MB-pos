//! The vocabulary of "no", and there are no other words.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::error::AuthError;

/// Everything a person can be allowed to do.
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
    /// Letting a phone onto the counter is its own decision: the person who may take an order
    /// is not automatically the person who may add a device to the shop's network.
    DevicesPair,
    LicenceManage,
    /// Reading the stock book: what is on the shelf, what a dish costs, what went in the bin.
    InventoryView,
    /// Changing what a dish is MADE OF, and what a material costs.
    InventoryManage,
    /// Recording wastage — the report that catches theft, and therefore the one a cook has to
    /// be able to feed.
    StockWaste,
    /// Changing a stock figure with no bill and no bin behind it.
    StockAdjust,
    /// Who the shop buys from, what it owes them, and paying it.
    SuppliersManage,
    /// Entering deliveries, returns and purchase orders — a daily job.
    PurchasesManage,
    /// Walking the store with a clipboard and writing down what is there.
    StockCount,
    /// Marking somebody present or absent, and setting the roster.
    AttendanceMark,
    /// Changing a clock-in or a clock-out after the event.
    AttendanceCorrect,
    /// Approving or rejecting leave, and adjusting a leave balance.
    LeaveApprove,
    /// Seeing what people are paid, and the staff cost.
    SalaryView,
    /// Setting salaries, giving advances, and approving a payroll run.
    SalaryManage,
    DeliveryDispatch,
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
        Permission::LicenceManage,
        Permission::InventoryView,
        Permission::InventoryManage,
        Permission::StockWaste,
        Permission::StockAdjust,
        Permission::SuppliersManage,
        Permission::PurchasesManage,
        Permission::StockCount,
        Permission::AttendanceMark,
        Permission::AttendanceCorrect,
        Permission::LeaveApprove,
        Permission::SalaryView,
        Permission::SalaryManage,
        Permission::DeliveryDispatch,
    ];

    /// The stored form. This string is a database value: changing one is a migration, not a
    /// rename.
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
            Permission::LicenceManage => "licence.manage",
            Permission::InventoryView => "inventory.view",
            Permission::InventoryManage => "inventory.manage",
            Permission::StockWaste => "stock.waste",
            Permission::StockAdjust => "stock.adjust",
            Permission::SuppliersManage => "suppliers.manage",
            Permission::PurchasesManage => "purchases.manage",
            Permission::StockCount => "stock.count",
            Permission::AttendanceMark => "attendance.mark",
            Permission::AttendanceCorrect => "attendance.correct",
            Permission::LeaveApprove => "leave.approve",
            Permission::SalaryView => "salary.view",
            Permission::SalaryManage => "salary.manage",
            Permission::DeliveryDispatch => "delivery.dispatch",
        }
    }

    /// What a refusal says out loud — the tail of "you do not have permission to …".
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
            Permission::LicenceManage => "change this shop's licence",
            Permission::InventoryView => "see stock and food cost",
            Permission::InventoryManage => "change materials and recipes",
            Permission::StockWaste => "record wastage",
            Permission::StockAdjust => "adjust stock by hand",
            Permission::SuppliersManage => "manage suppliers and pay them",
            Permission::PurchasesManage => "enter deliveries and returns",
            Permission::StockCount => "count what is on the shelves",
            Permission::AttendanceMark => "mark somebody present or absent",
            Permission::AttendanceCorrect => "change a clock-in or clock-out",
            Permission::LeaveApprove => "approve leave",
            Permission::SalaryView => "see what people are paid",
            Permission::SalaryManage => "set salaries and approve payroll",
            Permission::DeliveryDispatch => "send deliveries out and take the money back",
        }
    }

    /// Read one back.
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

/// What one role grants.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSet(BTreeSet<Permission>);

impl PermissionSet {
    #[must_use]
    pub fn new() -> PermissionSet {
        PermissionSet(BTreeSet::new())
    }

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
        // ALL is written by hand, so it is the thing that rots.
        assert_eq!(Permission::ALL.len(), 37);
        let codes: BTreeSet<&str> = Permission::ALL.iter().map(|p| p.code()).collect();
        assert_eq!(codes.len(), 37, "two permissions share a code");
    }

    #[test]
    fn a_code_round_trips() {
        for p in Permission::ALL {
            assert_eq!(Permission::from_code(p.code()), Ok(*p));
        }
    }

    #[test]
    fn an_unknown_code_is_an_error_not_a_denial() {
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
