/** The one way React talks to Rust. */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type { AppStatus } from './generated/AppStatus';
import type { FirstRunView } from './generated/FirstRunView';
import type { PrintJobView } from './generated/PrintJobView';
import type { PreviewDoc } from './generated/PreviewDoc';
import type { CartView } from './generated/CartView';
import type { MenuItemView } from './generated/MenuItemView';
import type { TableView } from './generated/TableView';
import type { Pushed } from './generated/Pushed';
import type { UiError } from './generated/UiError';
import type { LockState } from './generated/LockState';
import type { PersonView } from './generated/PersonView';
import type { RoleView } from './generated/RoleView';
import type { StaffEdit } from './generated/StaffEdit';
import type { AuditView } from './generated/AuditView';
import type { BillRowView } from './generated/BillRowView';
import type { DayTotalsView } from './generated/DayTotalsView';
import type { ReasonView } from './generated/ReasonView';
import type { MenuRowView } from './generated/MenuRowView';
import type { MenuEdit } from './generated/MenuEdit';
import type { CategoryView } from './generated/CategoryView';
import type { TaxClassView } from './generated/TaxClassView';
import type { ImportPlanView } from './generated/ImportPlanView';
import type { ItemComposition } from './generated/ItemComposition';
import type { ModifierGroupView } from './generated/ModifierGroupView';
import type { GroupEdit } from './generated/GroupEdit';
import type { ComboView } from './generated/ComboView';
import type { ComboEdit } from './generated/ComboEdit';
import type { FloorView } from './generated/FloorView';
import type { TableEdit } from './generated/TableEdit';
import type { SplitRequest } from './generated/SplitRequest';
import type { CustomerView } from './generated/CustomerView';
import type { CustomerEdit } from './generated/CustomerEdit';
import type { AccountView } from './generated/AccountView';
import type { HeadroomView } from './generated/HeadroomView';
import type { ExpensesView } from './generated/ExpensesView';
import type { InventoryView } from './generated/InventoryView';
import type { BuyingView } from './generated/BuyingView';
import type { SupplierAccountView } from './generated/SupplierAccountView';
import type { SupplierEdit } from './generated/SupplierEdit';
import type { PurchaseView } from './generated/PurchaseView';
import type { PurchaseEdit } from './generated/PurchaseEdit';
import type { PoEdit } from './generated/PoEdit';
import type { PhotoView } from './generated/PhotoView';
import type { StockCountView } from './generated/StockCountView';
import type { TerminalEdit } from './generated/TerminalEdit';
import type { TillsView } from './generated/TillsView';
import type { EmployeeView } from './generated/EmployeeView';
import type { EmployeeEdit } from './generated/EmployeeEdit';
import type { AttendanceView } from './generated/AttendanceView';
import type { LeaveView } from './generated/LeaveView';
import type { SalaryView } from './generated/SalaryView';
import type { SalaryEdit } from './generated/SalaryEdit';
import type { PayrollListView } from './generated/PayrollListView';
import type { PayrollView } from './generated/PayrollView';
import type { StaffCostView } from './generated/StaffCostView';
import type { DeliveryBoardView } from './generated/DeliveryBoardView';
import type { DeliveryEdit } from './generated/DeliveryEdit';
import type { PaymentsView } from './generated/PaymentsView';
import type { DevicesView } from './generated/DevicesView';
import type { DeviceTest } from './generated/DeviceTest';
import type { ScanOutcome } from './generated/ScanOutcome';
import type { CountEdit } from './generated/CountEdit';
import type { ShareView } from './generated/ShareView';
import type { Channel } from './generated/Channel';
import type { MaterialEdit } from './generated/MaterialEdit';
import type { MovementEdit } from './generated/MovementEdit';
import type { RecipeEdit } from './generated/RecipeEdit';
import type { RecipeView } from './generated/RecipeView';
import type { VarianceView } from './generated/VarianceView';
import type { ExpenseEdit } from './generated/ExpenseEdit';
import type { SettingsView } from './generated/SettingsView';
import type { SettingEdit } from './generated/SettingEdit';
import type { SavedView } from './generated/SavedView';
import type { PreviewView } from './generated/PreviewView';
import type { PrintersView } from './generated/PrintersView';
import type { LogoView } from './generated/LogoView';
import type { PickedFile } from './generated/PickedFile';
import type { PrinterEdit } from './generated/PrinterEdit';
import type { BackupView } from './generated/BackupView';
import type { VerifyView } from './generated/VerifyView';
import type { ConfigPlanView } from './generated/ConfigPlanView';
import type { NumberingView } from './generated/NumberingView';
import type { CountArg } from './generated/CountArg';
import type { DashboardView } from './generated/DashboardView';
import type { DayCloseView } from './generated/DayCloseView';
import type { LicenceView } from './generated/LicenceView';
import type { HealthView } from './generated/HealthView';
import type { BundlePlanView } from './generated/BundlePlanView';
import type { SetupView } from './generated/SetupView';
import type { KitchenView } from './generated/KitchenView';
import type { UpdateState } from './generated/UpdateState';
import type { NetworkView } from './generated/NetworkView';
import type { PeriodArg } from './generated/PeriodArg';
import type { ReportListView } from './generated/ReportListView';
import type { ReportView } from './generated/ReportView';
import type { SavedFileView } from './generated/SavedFileView';
import type { CounterEdit } from './generated/CounterEdit';
import type { NoticesView } from './generated/NoticesView';
import type { CloudRestoreView } from './generated/CloudRestoreView';

/** Every command, with what it takes and what it gives back. */
export interface Commands {
  app_status: { args: void; returns: AppStatus };

  // 5, the first run.
  first_run: { args: void; returns: FirstRunView };
  create_shop: { args: { folder: string }; returns: FirstRunView };
  use_existing_shop: { args: { path: string }; returns: FirstRunView };
  /** Bring my shop from the cloud: the licence key, where the file goes, and whether to move a licence bound elsewhere. */
  restore_from_cloud: {
    args: { key: string; folder: string; moveHere: boolean };
    returns: CloudRestoreView;
  };
  reveal_logs: { args: void; returns: string };
  print_test_page: { args: { printerId: string }; returns: string };
  list_print_jobs: { args: void; returns: PrintJobView[] };
  preview_test_page: {
    args: { printerId: string | null };
    returns: PreviewDoc;
  };
  /** The REAL bill for the REAL order, before it prints. */
  preview_order: { args: { orderId: string | null }; returns: PreviewDoc };
  /** The kitchen ticket that would print right now — the delta, as sent. */
  retry_print_job: { args: { id: string }; returns: void };

  // The billing screen.
  current_cart: { args: void; returns: CartView };
  // The floor added lines to the order this cart has open.
  take_the_floors_items: { args: void; returns: CartView };
  dismiss_the_floors_items: { args: void; returns: CartView };
  cart_add: {
    args: { itemId: string; qty: string | null; note: string | null };
    returns: CartView;
  };
  cart_set_qty: { args: { index: number; qty: string }; returns: CartView };
  /** − and + on a cart line. */
  cart_step_qty: { args: { index: number; by: number }; returns: CartView };
  cart_remove: { args: { index: number }; returns: CartView };
  cart_clear: { args: { keepType: boolean }; returns: CartView };
  cart_set_order_type: { args: { orderType: string }; returns: CartView };
  cart_clear_payments: { args: void; returns: CartView };
  cart_cash_given: { args: { amount: string }; returns: CartView };
  /** Money off this bill. */
  cart_set_discount: {
    args: { kind: string; value: string; reason: string | null };
    returns: CartView;
  };
  cart_clear_discount: { args: void; returns: CartView };
  open_orders: { args: void; returns: TableView[] };
  menu_items: { args: void; returns: MenuItemView[] };
  /** Ranked search — the rule lives in Rust. */
  search_items: {
    args: { text: string; mode: 'starts_with' | 'contains' | null };
    returns: MenuItemView[];
  };
  open_table: { args: { tableId: string }; returns: CartView };
  open_order: { args: { orderId: string }; returns: CartView };
  join_table: { args: { tableId: string; seat: string | null }; returns: CartView };
  /** The delta only, from the order's own ledger. */
  print_kitchen_ticket: { args: void; returns: string };
  /** The cook lost the paper. */
  reprint_kitchen_ticket: { args: void; returns: string };
  /** settle() — one transaction — and THEN the print. */
  complete_bill: { args: { mode: string | null }; returns: string };
  /** The bill a waiter carries to the table, before anybody has paid. */
  print_open_bill: { args: { orderId: string }; returns: string };
  /** Development only — the command does not exist in a release build. */
  seed_demo_shop: { args: void; returns: string };
  dismiss_print_job: { args: { id: string }; returns: void };
  retry_parked_print_jobs: { args: void; returns: number };
  dismiss_all_print_jobs: { args: void; returns: number };

  // Signing in, the people and the history.
  lock_state: { args: void; returns: LockState };
  login: { args: { staffId: string; pin: string }; returns: LockState };
  lock_now: { args: void; returns: LockState };
  /** Returns the NEW recovery code, to be shown once and printed. */
  recover_with_code: {
    args: { code: string; staffId: string; newPin: string };
    returns: string;
  };
  list_staff: { args: void; returns: PersonView[] };
  save_staff_member: { args: { staff: StaffEdit }; returns: PersonView[] };
  /** `null` clears the PIN. */
  set_staff_pin: {
    args: { staffId: string; pin: string | null };
    returns: string | null;
  };
  list_roles: { args: void; returns: RoleView[] };
  save_role: { args: { role: RoleView }; returns: RoleView[] };
  list_permissions: { args: void; returns: [string, string][] };
  audit_trail: {
    args: {
      staffId: string | null;
      actionCode: string | null;
      days: number | null;
    };
    returns: AuditView;
  };

  // The four ways a shop takes something back.
  list_bills: { args: void; returns: BillRowView[] };
  day_totals: { args: void; returns: DayTotalsView };
  reasons: { args: { kind: string }; returns: ReasonView[] };
  void_bill: {
    args: {
      orderId: string;
      reason: string;
      approverStaffId: string | null;
      approverPin: string | null;
    };
    returns: BillRowView[];
  };
  cancel_order: { args: { orderId: string; reason: string }; returns: void };
  void_line: { args: { index: number; reason: string }; returns: CartView };
  reprint_bill: { args: { orderId: string; reason: string }; returns: string };
  bill_pdf: { args: { orderId: string }; returns: SavedFileView };
  refund_bill: {
    args: {
      orderId: string;
      amountPaise: number;
      mode: string;
      reason: string;
    };
    returns: BillRowView[];
  };

  // The menu.
  menu_tax_classes: { args: void; returns: TaxClassView[] };
  menu_categories: { args: void; returns: CategoryView[] };
  menu_rows: { args: void; returns: MenuRowView[] };
  save_menu_item: { args: { edit: MenuEdit }; returns: MenuRowView[] };
  set_item_available: {
    args: { itemId: string; available: boolean };
    returns: MenuRowView[];
  };
  save_menu_category: {
    args: { id: string; name: string; isActive: boolean };
    returns: CategoryView[];
  };
  // The kind and the basis are the machine values Rust sent, never words read back off the
  // screen.
  save_tax_class: {
    args: {
      id: string;
      name: string;
      rate: string;
      kind: TaxClassView['kind'];
      basis: TaxClassView['basis'];
    };
    returns: string;
  };
  change_menu_prices: {
    args: { categoryId: string | null; percent: string };
    returns: string;
  };
  // The dry run writes nothing; the import then does exactly what it said.
  plan_menu_import: { args: { csv: string }; returns: ImportPlanView };
  run_menu_import: { args: { csv: string }; returns: string };
  export_menu: { args: void; returns: string };

  item_composition: { args: { itemId: string }; returns: ItemComposition };
  save_item_variant: {
    args: {
      itemId: string;
      variantId: string;
      name: string;
      price: string;
      isActive: boolean;
    };
    returns: ItemComposition;
  };
  list_modifier_groups: { args: void; returns: ModifierGroupView[] };
  save_modifier_group: { args: { group: GroupEdit }; returns: ModifierGroupView[] };
  attach_modifier_group: {
    args: { itemId: string; groupId: string; attach: boolean };
    returns: ItemComposition;
  };
  list_combos: { args: void; returns: ComboView[] };
  save_combo: { args: { combo: ComboEdit }; returns: ComboView[] };

  // The floor.
  floor_plan: { args: void; returns: FloorView };
  save_floor_section: {
    args: { id: string; name: string; sortOrder: number; isActive: boolean };
    returns: FloorView;
  };
  delete_floor_section: { args: { id: string }; returns: FloorView };
  save_dining_table: { args: { edit: TableEdit }; returns: FloorView };
  add_dining_tables: {
    args: {
      sectionId: string | null;
      prefix: string;
      from: number;
      to: number;
      seats: number;
    };
    returns: FloorView;
  };
  /** `null` for both takes the table off the plan and back to the section grid. */
  place_dining_table: {
    args: { tableId: string; x: number | null; y: number | null };
    returns: FloorView;
  };
  delete_dining_table: { args: { tableId: string }; returns: FloorView };
  /** The bulk pair — one transaction, all or nothing. */
  delete_dining_tables: { args: { tableIds: readonly string[] }; returns: FloorView };
  set_dining_tables_active: {
    args: { tableIds: readonly string[]; active: boolean };
    returns: FloorView;
  };
  save_floor_thresholds: { args: { warn: number; late: number }; returns: FloorView };
  move_order: { args: { orderId: string; toTable: string }; returns: FloorView };
  merge_orders: { args: { fromOrder: string; intoOrder: string }; returns: FloorView };
  split_order: { args: { request: SplitRequest }; returns: FloorView };
  /** Answers "what do we each owe?" — it does not create n bills. */

  // Customers and what they owe.
  customers: { args: void; returns: CustomerView[] };
  customer_account: { args: { customerId: string }; returns: AccountView };
  /** A duplicate phone comes back as an error carrying the existing id. */
  save_customer: { args: { edit: CustomerEdit }; returns: CustomerView[] };
  record_repayment: {
    args: { customerId: string; amount: string; mode: string; reference: string };
    returns: AccountView;
  };
  save_credit_adjustment: {
    args: { customerId: string; amount: string; increases: boolean; reason: string };
    returns: AccountView;
  };
  /** What this bill would do to the account — asked before it happens. */
  credit_headroom: { args: { customerId: string }; returns: HeadroomView };
  put_on_account: {
    args: { customerId: string; overrideLimit: boolean };
    returns: CartView;
  };

  // Money going out, and the drawer.
  expenses: { args: void; returns: ExpensesView };
  save_expense: { args: { edit: ExpenseEdit }; returns: ExpensesView };
  delete_expense: { args: { id: string }; returns: ExpensesView };
  /** The float, a top-up, a payout, a bank drop. */
  save_cash_movement: {
    args: { kind: string; amount: string; reason: string };
    returns: ExpensesView;
  };
  save_expense_category: {
    args: { id: string; name: string; isActive: boolean };
    returns: ExpensesView;
  };
  save_recurring_expense: {
    args: {
      id: string;
      description: string;
      amount: string;
      mode: string;
      every: string;
      categoryId: string | null;
    };
    returns: ExpensesView;
  };
  /** The only way a reminder becomes money. */
  confirm_recurring_expense: { args: { id: string }; returns: ExpensesView };
  export_expenses: { args: void; returns: string };

  // The employment side.
  employees: { args: void; returns: EmployeeView[] };
  save_employee: { args: { edit: EmployeeEdit }; returns: EmployeeView[] };
  attendance: {
    args: { staffId: string | null; from: string; to: string };
    returns: AttendanceView;
  };
  /** Needs nothing but being signed in. */
  clock_in: { args: { terminalId: string | null }; returns: AttendanceView };
  clock_out: { args: void; returns: AttendanceView };
  correct_attendance: {
    args: { id: string; started: string; ended: string; reason: string };
    returns: AttendanceView;
  };
  save_roster: {
    args: { staffId: string; day: string; patternId: string; note: string };
    returns: AttendanceView;
  };
  leave: { args: { staffId: string | null }; returns: LeaveView };
  request_leave: {
    args: {
      staffId: string;
      leaveTypeId: string;
      from: string;
      to: string;
      halfDays: number;
      reason: string;
    };
    returns: LeaveView;
  };
  decide_leave: {
    args: { id: string; approve: boolean; note: string };
    returns: LeaveView;
  };
  adjust_leave: {
    args: {
      staffId: string;
      leaveTypeId: string;
      halfDays: number;
      reason: string;
      accrual: boolean;
    };
    returns: LeaveView;
  };
  salary: { args: { staffId: string }; returns: SalaryView };
  save_salary: { args: { edit: SalaryEdit }; returns: SalaryView };
  give_advance: {
    args: { staffId: string; amount: string; instalments: number; reason: string };
    returns: SalaryView;
  };
  payroll_runs: { args: void; returns: PayrollListView };
  payroll: { args: { runId: string }; returns: PayrollView };
  /** Computes and stores a DRAFT. */
  compute_payroll: { args: { from: string; to: string }; returns: PayrollView };
  edit_payroll_line: {
    args: { runId: string; staffId: string; net: string; note: string };
    returns: PayrollView;
  };
  /** Where money leaves the shop. */
  approve_payroll: { args: { runId: string; paidBy: string }; returns: PayrollView };
  reverse_payroll: { args: { runId: string; reason: string }; returns: PayrollView };
  staff_cost: { args: { from: string; to: string }; returns: StaffCostView };
  print_payslip: { args: { runId: string; staffId: string }; returns: string };

  delivery_board: { args: { day: string | null }; returns: DeliveryBoardView };
  save_delivery: { args: { edit: DeliveryEdit }; returns: DeliveryBoardView };
  record_handback: {
    args: { riderId: string; amount: string; note: string };
    returns: DeliveryBoardView;
  };
  set_rider: { args: { staffId: string; isRider: boolean }; returns: DeliveryBoardView };
  print_delivery_slip: { args: { orderId: string }; returns: string };

  payments: { args: void; returns: PaymentsView };
  confirm_payment: {
    args: { orderId: string; seq: number; reference: string };
    returns: PaymentsView;
  };

  device_manager: { args: void; returns: DevicesView };
  read_scale_once: { args: void; returns: DeviceTest };
  // The timing crosses the wire, the decision does not.
  scanned: { args: { text: string; gapsMs: number[] }; returns: ScanOutcome };
  show_customer_display: { args: { on: boolean }; returns: DevicesView };
  print_label: { args: { line: string; token: string }; returns: string };

  inventory: { args: { material: string | null }; returns: InventoryView };
  recipe: { args: { ownerKind: string; ownerId: string }; returns: RecipeView };
  save_material: { args: { edit: MaterialEdit }; returns: InventoryView };
  save_recipe: { args: { edit: RecipeEdit }; returns: RecipeView };
  delete_recipe: { args: { ownerKind: string; ownerId: string }; returns: RecipeView };
  record_stock_movement: { args: { edit: MovementEdit }; returns: InventoryView };
  /** Work the balances out again from the movements. */
  rebuild_stock_balances: { args: void; returns: InventoryView };
  resolve_stock_problem: { args: { id: string }; returns: InventoryView };
  stock_variance: { args: { from: string; to: string }; returns: VarianceView[] };
  /** The buy list as text a person can send. */
  buy_list_text: { args: void; returns: string };

  buying: { args: { supplier: string | null }; returns: BuyingView };
  supplier_account: { args: { id: string }; returns: SupplierAccountView };
  purchase: { args: { id: string }; returns: PurchaseView };
  save_supplier: { args: { edit: SupplierEdit }; returns: BuyingView };
  save_purchase: { args: { edit: PurchaseEdit }; returns: BuyingView };
  /** A purchase is never edited. */
  cancel_purchase: { args: { id: string; reason: string }; returns: BuyingView };
  record_supplier_payment: {
    args: { supplierId: string; amount: string; mode: string; reference: string };
    returns: SupplierAccountView;
  };
  save_supplier_adjustment: {
    args: { supplierId: string; amount: string; increases: boolean; reason: string };
    returns: SupplierAccountView;
  };
  save_purchase_order: { args: { edit: PoEdit }; returns: BuyingView };
  set_order_state: { args: { id: string; state: string }; returns: BuyingView };
  /** The photograph, already downscaled by this side (canvas, 1600 px, JPEG 0.7). */
  attach_photo: { args: { dataUrl: string }; returns: PhotoView };
  purchase_photo: { args: { id: string }; returns: PhotoView };

  /** The count freezes the book and approving posts a DELTA. */
  stock_count: { args: { id: string | null }; returns: StockCountView };
  open_stock_count: { args: { location: string }; returns: StockCountView };
  record_count_line: { args: { edit: CountEdit }; returns: StockCountView };
  explain_count_line: {
    args: { countId: string; materialId: string; reasonId: string | null; note: string };
    returns: StockCountView;
  };
  remove_count_line: { args: { countId: string; materialId: string }; returns: StockCountView };
  approve_stock_count: { args: { id: string }; returns: StockCountView };
  abandon_stock_count: { args: { id: string; reason: string }; returns: StockCountView };
  /** And the book quantity is deliberately not on it. */
  count_sheet: { args: { location: string }; returns: string };

  /** The summary is composed in Rust; this side sends it. */
  share_report: {
    args: { id: string; period: PeriodArg; channel: Channel };
    returns: ShareView;
  };

  tills: { args: void; returns: TillsView };
  save_till: { args: { edit: TerminalEdit }; returns: TillsView };
  /** A person chooses. */
  make_master: { args: { id: string }; returns: TillsView };
  /**
   * Waits for somebody at the other counter to press Allow, so it can take a minute or two —
   * the screen shows a spinner and Tauri runs it off the UI thread.
   */
  join_master: {
    args: {
      address: string;
      fingerprint: string;
      token: string;
      name: string;
      prefix: string;
    };
    returns: TillsView;
  };
  /** The same call the background sender makes, so pressing it is only early. */
  send_waiting_bills: { args: void; returns: TillsView };

  settings_all: { args: void; returns: SettingsView };
  /** After a restore, and whenever a screen wants to be sure. */
  reload_settings: { args: void; returns: SettingsView };
  /** Returns the KEYS that match. */
  search_settings: { args: { text: string }; returns: string[] };
  save_settings: { args: { edits: SettingEdit[] }; returns: SavedView };
  /** What "reset this section" WOULD set. */
  settings_defaults_for: { args: { group: string }; returns: SettingEdit[] };
  /**
   * The sample bill or ticket, laid out with the settings as they are on screen RIGHT NOW —
   * saved or not.
   */
  preview_settings: {
    args: { group: string; edits: SettingEdit[] };
    returns: PreviewView;
  };

  // The logo, and the two Browse buttons.
  logo: { args: void; returns: LogoView };
  /** Opens the operating system's picker. */
  pick_a_logo: { args: void; returns: PickedFile | null };
  /**
   * `MB1` dots, base64. Rust decodes them before writing, so a picture that would silently fail
   * to print is refused while the person is still looking at it.
   */
  save_logo: { args: { encoded: string }; returns: LogoView };
  remove_logo: { args: void; returns: LogoView };
  /** The first run's Browse. */
  pick_a_folder: { args: { start: string | null }; returns: string | null };

  // The printers.
  printer_setup: { args: void; returns: PrintersView };
  save_printer: { args: { edit: PrinterEdit }; returns: PrintersView };
  delete_printer: { args: { id: string }; returns: PrintersView };
  /** An empty printerId means "the default kitchen printer". */
  route_category: {
    args: { categoryId: string; printerId: string };
    returns: PrintersView;
  };
  /**
   * A whole sample BILL, not a slip — a slip cannot show whether a bill is centred, and that is
   * what somebody at the printer is asking.
   */
  print_sample_bill: { args: { printerId: string }; returns: string };
  /**
   * Where bills print. One dropdown on the Printers screen, and the command that makes it mean
   * something — before this, the only way to change the default was a checkbox at the bottom of
   * the add-a-printer dialog, so shops kept printing to the stand-in that prints nothing.
   */
  set_default_printer: { args: { printerId: string }; returns: PrintersView };
  /** How wide the roll is — 58 (2 inch), 80 (3 inch) or 100 (4 inch). */
  set_paper_size: { args: { mm: number }; returns: PrintersView };
  /** Print, look at the paper, nudge, print again. */
  nudge_printer: {
    args: { printerId: string; dxMm: number; dyMm: number };
    returns: PrintersView;
  };

  backup_status: { args: void; returns: BackupView };
  back_up_now: { args: void; returns: BackupView };
  verify_backup: { args: { path: string }; returns: VerifyView };
  request_restore: { args: { path: string }; returns: BackupView };
  cancel_restore: { args: void; returns: BackupView };
  /** The screen somebody opens when everything else is broken. */
  find_shops: { args: void; returns: string[] };

  // The whole configuration, out and in — a dealer sets up the second shop by copying one file.
  export_settings: { args: void; returns: string };
  plan_settings_import: { args: { text: string }; returns: ConfigPlanView };
  run_settings_import: { args: { text: string }; returns: SavedView };

  // The counters.
  numbering: { args: void; returns: NumberingView };
  save_counter: { args: { edit: CounterEdit }; returns: NumberingView };

  // Thirteen reports behind four commands, because the report list is the screen.
  dashboard: { args: void; returns: DashboardView };
  report_list: { args: void; returns: ReportListView };
  report: { args: { id: string; period: PeriodArg }; returns: ReportView };
  report_csv: { args: { id: string; period: PeriodArg }; returns: SavedFileView };
  report_pdf: { args: { id: string; period: PeriodArg }; returns: SavedFileView };

  // The phones this counter serves.
  network: { args: void; returns: NetworkView };
  open_pairing: { args: void; returns: NetworkView };
  close_pairing: { args: void; returns: NetworkView };
  allow_device: { args: { requestId: string }; returns: NetworkView };
  refuse_device: { args: { requestId: string }; returns: NetworkView };
  revoke_device: { args: { deviceId: string }; returns: NetworkView };

  day_close: { args: void; returns: DayCloseView };
  count_cash: { args: { counts: CountArg[] }; returns: DayCloseView };
  close_day: {
    args: { counts: CountArg[]; reason: string; print: boolean };
    returns: DayCloseView;
  };
  reopen_day: { args: { reason: string }; returns: DayCloseView };

  // The licence.
  account: { args: void; returns: LicenceView };
  refresh_licence: { args: void; returns: LicenceView };
  activate: { args: { key: string }; returns: LicenceView };
  deactivate: { args: void; returns: LicenceView };
  transfer_here: { args: { key: string }; returns: LicenceView };
  use_emergency_code: { args: { code: string }; returns: LicenceView };

  // Is this counter healthy, and what can we send to support.
  health: { args: void; returns: HealthView };
  // Public in Rust, deliberately: on a first run nobody has a PIN yet, and a set-up list that
  // will not draw until somebody does is a list nobody can use to create one.
  setup_list: { args: void; returns: SetupView };

  // The kitchen screen.
  kitchen: { args: { station: string | null }; returns: KitchenView };
  kitchen_shown: { args: { id: string }; returns: KitchenView };
  kitchen_bump: { args: { id: string }; returns: KitchenView };
  kitchen_bump_line: { args: { id: string; key: string }; returns: KitchenView };
  kitchen_recall: { args: { id: string }; returns: KitchenView };
  kitchen_acknowledge: { args: { id: string }; returns: KitchenView };
  kitchen_fire: { args: { orderId: string; course: string }; returns: KitchenView };
  look_for_an_update: { args: void; returns: UpdateState };
  go_back_a_version: { args: void; returns: string };
  install_update: { args: void; returns: string };
  // The cloud copy, and what comes back down it.
  notices: { args: void; returns: NoticesView };
  notices_seen: { args: void; returns: NoticesView };
  pull_from_cloud: { args: void; returns: NoticesView };
  diagnostics_plan: { args: void; returns: BundlePlanView };
  write_diagnostics: { args: void; returns: string };
}

export type CommandName = keyof Commands;

/** Is this thing something Rust sent us, rather than something that broke? */
export function isUiError(value: unknown): value is UiError {
  return (
    typeof value === 'object' &&
    value !== null &&
    'code' in value &&
    'message' in value
  );
}

/** Is this the licence saying no, rather than something going wrong? */
export function isLicenceRefusal(cause: unknown): cause is UiError {
  return isUiError(cause) && cause.code.startsWith('licence.');
}

/**
 * A failure that did not come from Rust — the webview lost the bridge, or a command name is
 * wrong.
 */
function asUiError(cause: unknown): UiError {
  if (isUiError(cause)) return cause;
  return {
    code: 'ipc.failed',
    message:
      'Magic Bill could not reach its own engine. Close it and open it again — ' +
      'nothing has been lost.',
    detail: String(cause),
    // The bridge is down: that is a problem, not a notice.
    tone: 'problem',
  };
}

export async function call<K extends CommandName>(
  ...[name, args]: Commands[K]['args'] extends void
    ? [K]
    : [K, Commands[K]['args']]
): Promise<Commands[K]['returns']> {
  try {
    return (await invoke(
      name,
      args as Record<string, unknown> | undefined,
    )) as Commands[K]['returns'];
  } catch (cause) {
    throw asUiError(cause);
  }
}

/** What Rust pushes, when it pushes it. */
export function subscribe(onPush: (message: Pushed) => void): Promise<() => void> {
  return listen<Pushed>('mb://push', (event) => onPush(event.payload));
}

/** True when we are running inside Tauri rather than a browser test. */
export function inApp(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
