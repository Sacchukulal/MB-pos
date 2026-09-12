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
    /// Take a paid bill back to the counter, fix it and bill again under the same number.
    BillRevert,
    /// Sign off a bill that was taken back and billed again.
    BillRevertApprove,
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
    /// Every permission, in the order the roles screen shows them: group by group, and inside
    /// a group from the everyday thing to the rare one.
    pub const ALL: &'static [Permission] = &[
        // Billing.
        Permission::BillCreate,
        Permission::BillDiscountLine,
        Permission::BillDiscountBill,
        Permission::BillReprint,
        Permission::BillVoid,
        Permission::BillRevert,
        Permission::BillRevertApprove,
        // Orders.
        Permission::OrderCancel,
        Permission::OrderItemVoid,
        // Cash and the day.
        Permission::DayClose,
        Permission::ExpensesManage,
        Permission::DrawerOpen,
        // Customers and credit.
        Permission::CustomersManage,
        Permission::CreditCollect,
        // Delivery.
        Permission::DeliveryDispatch,
        // Menu and tables.
        Permission::MenuManage,
        Permission::TablesManage,
        // Stock.
        Permission::InventoryView,
        Permission::InventoryManage,
        Permission::StockWaste,
        Permission::StockCount,
        Permission::StockAdjust,
        // Buying.
        Permission::PurchasesManage,
        Permission::SuppliersManage,
        // Staff.
        Permission::StaffManage,
        Permission::AttendanceMark,
        Permission::AttendanceCorrect,
        Permission::LeaveApprove,
        Permission::SalaryView,
        Permission::SalaryManage,
        // Reports and history.
        Permission::ReportsView,
        Permission::ReportsExport,
        Permission::AuditView,
        // Settings.
        Permission::SettingsStore,
        Permission::SettingsPrinter,
        Permission::SettingsTax,
        // Account, backup and phones.
        Permission::BackupRun,
        Permission::DevicesPair,
        Permission::LicenceManage,
    ];

    /// Which section of the roles screen this sits in.
    #[must_use]
    pub const fn group(self) -> PermissionGroup {
        match self {
            Permission::BillCreate
            | Permission::BillDiscountLine
            | Permission::BillDiscountBill
            | Permission::BillReprint
            | Permission::BillVoid
            | Permission::BillRevert
            | Permission::BillRevertApprove => PermissionGroup::Billing,
            Permission::OrderCancel | Permission::OrderItemVoid => PermissionGroup::Orders,
            Permission::DayClose | Permission::ExpensesManage | Permission::DrawerOpen => {
                PermissionGroup::Cash
            }
            Permission::CustomersManage | Permission::CreditCollect => PermissionGroup::Customers,
            Permission::DeliveryDispatch => PermissionGroup::Delivery,
            Permission::MenuManage | Permission::TablesManage => PermissionGroup::Menu,
            Permission::InventoryView
            | Permission::InventoryManage
            | Permission::StockWaste
            | Permission::StockCount
            | Permission::StockAdjust => PermissionGroup::Stock,
            Permission::PurchasesManage | Permission::SuppliersManage => PermissionGroup::Buying,
            Permission::StaffManage
            | Permission::AttendanceMark
            | Permission::AttendanceCorrect
            | Permission::LeaveApprove
            | Permission::SalaryView
            | Permission::SalaryManage => PermissionGroup::Staff,
            Permission::ReportsView | Permission::ReportsExport | Permission::AuditView => {
                PermissionGroup::Reports
            }
            Permission::SettingsStore | Permission::SettingsPrinter | Permission::SettingsTax => {
                PermissionGroup::Settings
            }
            Permission::BackupRun | Permission::DevicesPair | Permission::LicenceManage => {
                PermissionGroup::Shop
            }
        }
    }

    /// Whether the roles screen offers this box. A permission with nothing behind it yet stays
    /// in the vocabulary, so a role that holds it keeps it, but it is not offered.
    #[must_use]
    pub const fn shown(self) -> bool {
        // The drawer opens itself on a cash sale when the Printers page says so; there is no
        // separate "open the drawer" button for this to guard (owner, 2026-09-12).
        !matches!(self, Permission::DrawerOpen)
    }

    /// The short name beside the box on the roles screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Permission::BillCreate => "Take orders and settle bills",
            Permission::BillDiscountLine => "Discount one line",
            Permission::BillDiscountBill => "Discount the whole bill",
            Permission::BillVoid => "Void a settled bill",
            Permission::BillRevert => "Take a paid bill back",
            Permission::BillRevertApprove => "Approve a corrected bill",
            Permission::BillReprint => "Reprint a bill",
            Permission::OrderCancel => "Cancel an open order",
            Permission::OrderItemVoid => "Void a kitchen item",
            Permission::DrawerOpen => "Open the cash drawer",
            Permission::MenuManage => "Edit the menu",
            Permission::TablesManage => "Arrange tables",
            Permission::CustomersManage => "Manage customers",
            Permission::CreditCollect => "Take credit repayments",
            Permission::ExpensesManage => "Record expenses",
            Permission::ReportsView => "See reports",
            Permission::ReportsExport => "Export and share reports",
            Permission::DayClose => "Open and close the day",
            Permission::SettingsStore => "Shop details and bill design",
            Permission::SettingsPrinter => "Printers",
            Permission::SettingsTax => "Tax and numbering",
            Permission::StaffManage => "Manage staff and roles",
            Permission::AuditView => "Read the history",
            Permission::BackupRun => "Backups",
            Permission::DevicesPair => "Phones",
            Permission::LicenceManage => "Account and licence",
            Permission::InventoryView => "See stock",
            Permission::InventoryManage => "Edit materials and recipes",
            Permission::StockWaste => "Record wastage",
            Permission::StockAdjust => "Adjust stock by hand",
            Permission::SuppliersManage => "Manage suppliers",
            Permission::PurchasesManage => "Enter purchases",
            Permission::StockCount => "Count stock",
            Permission::AttendanceMark => "Mark attendance",
            Permission::AttendanceCorrect => "Correct clock times",
            Permission::LeaveApprove => "Approve leave",
            Permission::SalaryView => "See salaries",
            Permission::SalaryManage => "Set salaries and run payroll",
            Permission::DeliveryDispatch => "Send deliveries out",
        }
    }

    /// The explanation behind the info button: what the box really lets somebody do.
    #[must_use]
    pub const fn hint(self) -> &'static str {
        match self {
            Permission::BillCreate => "Take an order, send it to the kitchen, and settle the bill.",
            Permission::BillDiscountLine => {
                "Give a discount on one item of a bill, up to this role's limit."
            }
            Permission::BillDiscountBill => {
                "Give a discount on the whole bill, up to this role's limit."
            }
            Permission::BillVoid => {
                "Cancel a bill that was already paid. Needs a reason, and goes in the history."
            }
            Permission::BillRevert => {
                "Bring a paid bill back to the counter, fix it, and bill it again under the same \
                 number."
            }
            Permission::BillRevertApprove => "Sign off a bill that was taken back and billed again.",
            Permission::BillReprint => "Print a settled bill again. Every reprint is counted.",
            Permission::OrderCancel => "Cancel an order that has not been billed yet. Needs a reason.",
            Permission::OrderItemVoid => {
                "Take one item off after the kitchen ticket went out. Needs a reason."
            }
            Permission::DrawerOpen => "Open the cash drawer without a sale.",
            Permission::MenuManage => "Add, edit and price menu items and categories.",
            Permission::TablesManage => "Add, move and remove tables and sections on the floor.",
            Permission::CustomersManage => "Add customers, edit their details, and set credit limits.",
            Permission::CreditCollect => "Receive money against a customer's account.",
            Permission::ExpensesManage => "Record and edit money paid out of the till.",
            Permission::ReportsView => "Open the dashboard, the bill list, and the sales reports.",
            Permission::ReportsExport => "Save a report as CSV or PDF, or send it by WhatsApp or email.",
            Permission::DayClose => {
                "Close and lock the business day, count the drawer, and open the next one."
            }
            Permission::SettingsStore => "Change the shop's name, address, bill design, and tills.",
            Permission::SettingsPrinter => "Set up printers, the cash drawer, and the label printer.",
            Permission::SettingsTax => "Change tax rates and bill numbering.",
            Permission::StaffManage => {
                "Add people, set their roles and PINs, and change what each role may do."
            }
            Permission::AuditView => {
                "Read the audit trail: every void, discount, refund and reprint, and who did it."
            }
            Permission::BackupRun => "Take a backup, and restore one. Restoring replaces everything.",
            Permission::DevicesPair => "Let a phone onto this counter, and take it off again.",
            Permission::LicenceManage => {
                "Open the Account page: the plan, the licence, and updates. Move or sign out the \
                 licence."
            }
            Permission::InventoryView => "Read stock levels, recipes, and food cost.",
            Permission::InventoryManage => {
                "Add materials, set their cost, and change what a dish is made of."
            }
            Permission::StockWaste => {
                "Write down what was thrown away. Wastage is what shows theft, so a cook has to be \
                 able to record it."
            }
            Permission::StockAdjust => {
                "Change a stock figure with no bill and no bin behind it, and approve stock counts."
            }
            Permission::SuppliersManage => "Add suppliers, record what the shop owes them, and pay them.",
            Permission::PurchasesManage => "Enter deliveries, returns, and purchase orders.",
            Permission::StockCount => {
                "Walk the store and write down what is on the shelves. Approving the count needs \
                 Adjust stock by hand."
            }
            Permission::AttendanceMark => "Mark somebody present or absent, and set the roster.",
            Permission::AttendanceCorrect => {
                "Change a clock-in or clock-out after the event. Never your own."
            }
            Permission::LeaveApprove => "Approve or reject leave, and adjust a leave balance.",
            Permission::SalaryView => "See what people are paid, and the staff cost.",
            Permission::SalaryManage => "Set salaries, give advances, and approve a payroll run.",
            Permission::DeliveryDispatch => {
                "Assign riders, move deliveries along, and take the cash back off riders."
            }
        }
    }

    /// The stored form. This string is a database value: changing one is a migration, not a
    /// rename.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Permission::BillCreate => "bill.create",
            Permission::BillDiscountLine => "bill.discount.line",
            Permission::BillDiscountBill => "bill.discount.bill",
            Permission::BillVoid => "bill.void",
            Permission::BillRevert => "bill.revert",
            Permission::BillRevertApprove => "bill.revert.approve",
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
            Permission::BillRevert => "revert a bill",
            Permission::BillRevertApprove => "approve a corrected bill",
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

/// The sections of the roles screen, in the order they are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PermissionGroup {
    Billing,
    Orders,
    Cash,
    Customers,
    Delivery,
    Menu,
    Stock,
    Buying,
    Staff,
    Reports,
    Settings,
    Shop,
}

impl PermissionGroup {
    /// Every group, in screen order.
    pub const ALL: &'static [PermissionGroup] = &[
        PermissionGroup::Billing,
        PermissionGroup::Orders,
        PermissionGroup::Cash,
        PermissionGroup::Customers,
        PermissionGroup::Delivery,
        PermissionGroup::Menu,
        PermissionGroup::Stock,
        PermissionGroup::Buying,
        PermissionGroup::Staff,
        PermissionGroup::Reports,
        PermissionGroup::Settings,
        PermissionGroup::Shop,
    ];

    /// The section heading.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            PermissionGroup::Billing => "Billing",
            PermissionGroup::Orders => "Orders",
            PermissionGroup::Cash => "Cash and the day",
            PermissionGroup::Customers => "Customers and credit",
            PermissionGroup::Delivery => "Delivery",
            PermissionGroup::Menu => "Menu and tables",
            PermissionGroup::Stock => "Stock",
            PermissionGroup::Buying => "Buying",
            PermissionGroup::Staff => "Staff",
            PermissionGroup::Reports => "Reports and history",
            PermissionGroup::Settings => "Settings",
            PermissionGroup::Shop => "Account, backup and phones",
        }
    }

    /// The permissions in this section, in screen order.
    pub fn permissions(self) -> impl Iterator<Item = Permission> {
        Permission::ALL.iter().copied().filter(move |p| p.group() == self)
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
        assert_eq!(Permission::ALL.len(), 39);
        let codes: BTreeSet<&str> = Permission::ALL.iter().map(|p| p.code()).collect();
        assert_eq!(codes.len(), 39, "two permissions share a code");
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
    fn every_group_has_a_box_and_every_box_a_group() {
        let mut seen = 0;
        for group in PermissionGroup::ALL {
            let boxes: Vec<Permission> = group.permissions().collect();
            assert!(!boxes.is_empty(), "{group:?} has no permission in it");
            seen += boxes.len();
        }
        assert_eq!(seen, Permission::ALL.len(), "a permission sits in no group");
    }

    #[test]
    fn all_is_in_group_order() {
        // The screen draws ALL group by group; a permission out of place would jump sections.
        let groups: Vec<PermissionGroup> = Permission::ALL.iter().map(|p| p.group()).collect();
        let mut sorted = groups.clone();
        sorted.sort();
        assert_eq!(groups, sorted);
    }

    #[test]
    fn a_label_is_short_and_a_hint_is_a_sentence() {
        for p in Permission::ALL {
            let label = p.label();
            assert!(label.len() <= 32, "\"{label}\" is too long for a box");
            assert!(label.starts_with(|c: char| c.is_uppercase()), "\"{label}\"");
            assert!(!label.ends_with('.'), "\"{label}\" is a label, not a sentence");
            let hint = p.hint();
            assert!(hint.ends_with('.'), "\"{hint}\" must finish");
            assert_ne!(label, hint, "the tip must say more than the label");
        }
    }

    #[test]
    fn the_drawer_box_is_not_offered() {
        assert!(!Permission::DrawerOpen.shown());
        assert_eq!(Permission::ALL.iter().filter(|p| p.shown()).count(), 38);
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
