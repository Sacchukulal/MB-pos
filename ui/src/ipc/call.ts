/**
 * The one way React talks to Rust.
 *
 * Every command goes through here so that four things are true in one place
 * rather than in every screen:
 *
 * 1. **Errors arrive as `UiError`**, with a code, a sentence for the shopkeeper
 *    and the technical detail behind it — audit F8: *"errors show raw system
 *    text to a restaurant owner."* A screen never sees a Rust panic string.
 * 2. **A failure is never silent.** A screen may choose how to show an error;
 *    it may not choose not to have one.
 * 3. **The names are typed.** `call('app_status')` is checked against the list
 *    below, so a renamed command is a compile error rather than a runtime
 *    "command not found" that only fires on the screen nobody opened.
 * 4. **Nothing polls.** Subscriptions are events pushed from Rust
 *    ([`subscribe`]), which is budget M4 and `PERFORMANCE.md` §5 rule 6.
 *
 * # No argument is ever a `bigint`
 *
 * `invoke` serialises with `JSON.stringify`, and `JSON.stringify` **throws** on
 * a BigInt. An `i64` on the Rust side arrives here as a plain JSON number, so
 * anything that comes back and goes straight out again works by accident; a
 * screen that honestly builds a `1n` does not. P13 found this by saving a
 * modifier group and reading *"Do not know how to serialize a BigInt"*.
 *
 * The fix is on the Rust side — a count that crosses the wire is a `u32`, which
 * `ts-rs` renders as `number` — and there is a guard in `guards.test.ts` that
 * fails the build if a `bigint` appears in an argument type again.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type { AppStatus } from './generated/AppStatus';
import type { PrintJobView } from './generated/PrintJobView';
import type { PreviewDoc } from './generated/PreviewDoc';
import type { CartView } from './generated/CartView';
import type { MenuItemView } from './generated/MenuItemView';
import type { PrinterView } from './generated/PrinterView';
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
import type { EvenSplitView } from './generated/EvenSplitView';
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

/**
 * Every command, with what it takes and what it gives back.
 *
 * Hand-written on purpose, and it is the one hand-written thing at this
 * boundary: the *types* are generated from Rust (`ts-rs`), and this maps names
 * to them. If a command is renamed in Rust and not here, the screen that calls
 * it stops compiling — which is the failure we want, at the time we want it.
 */
export interface Commands {
  app_status: { args: void; returns: AppStatus };
  set_appearance: { args: { theme: string; textSize: string }; returns: void };
  reveal_logs: { args: void; returns: string };
  list_printers: { args: void; returns: PrinterView[] };
  print_test_page: { args: { printerId: string }; returns: string };
  nudge_print_offset: {
    args: { printerId: string; dxMm: number; dyMm: number };
    returns: PrinterView;
  };
  list_print_jobs: { args: void; returns: PrintJobView[] };
  preview_test_page: {
    args: { printerId: string | null };
    returns: PreviewDoc;
  };
  retry_print_job: { args: { id: string }; returns: void };

  // The billing screen (P09). Every cart command returns the WHOLE new view:
  // the bill is recomputed in Rust from the cart every time (D4, 14 us), so a
  // delta would only be a way of being stale.
  current_cart: { args: void; returns: CartView };
  // P20 — the floor added lines to the order this cart has open. The counter
  // already took them; these decide whether they join the bill on screen.
  take_the_floors_items: { args: void; returns: CartView };
  dismiss_the_floors_items: { args: void; returns: CartView };
  cart_add: {
    args: { itemId: string; qty: string | null; note: string | null };
    returns: CartView;
  };
  cart_set_qty: { args: { index: number; qty: string }; returns: CartView };
  cart_remove: { args: { index: number }; returns: CartView };
  cart_clear: { args: { keepType: boolean }; returns: CartView };
  cart_set_order_type: { args: { orderType: string }; returns: CartView };
  cart_add_payment: {
    args: { mode: string; amountPaise: number };
    returns: CartView;
  };
  cart_clear_payments: { args: void; returns: CartView };
  open_orders: { args: void; returns: TableView[] };
  menu_items: { args: void; returns: MenuItemView[] };
  /** Ranked search — the rule lives in Rust (P10, budget B2). */
  search_items: {
    args: { text: string; mode: 'starts_with' | 'contains' | null };
    returns: MenuItemView[];
  };
  /** Budget B7 — an existing table's order into the cart. */
  open_table: { args: { tableId: string }; returns: CartView };
  /** The delta only, from the order's own ledger (crown jewel 2). */
  print_kitchen_ticket: { args: void; returns: string };
  /** settle() — one transaction — and THEN the print (audit D4). */
  complete_bill: { args: void; returns: string };
  /** Development only — the command does not exist in a release build. */
  seed_demo_shop: { args: void; returns: string };
  dismiss_print_job: { args: { id: string }; returns: void };

  // P11 — signing in, the people and the history. Audit C1.
  //
  // The first four answer while the screen is LOCKED; everything else is
  // refused in Rust by `guard::require`, which is the control. Hiding a rail
  // item is only a courtesy.
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
  /** `null` clears the PIN. Returns the shop's recovery code the first time one is made. */
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

  // P12 — the four ways a shop takes something back (audit B5, B6, D7).
  // Every one of them refuses in Rust; the dialogs only collect the reason.
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
  refund_bill: {
    args: {
      orderId: string;
      amountPaise: number;
      mode: string;
      reason: string;
    };
    returns: BillRowView[];
  };

  // P13 — the menu. Audit B10/B11/B14: v1 had one tax rate for the whole
  // shop, so it could not bill a bar, an AC/non-AC outlet or anyone selling
  // packaged goods.
  //
  // `menu.manage` gates every one of these in Rust. `save_tax_class` needs
  // `settings.tax` instead, because a rate is what the shop owes the
  // government rather than what it charges — getting it wrong is a notice,
  // not a bad price.
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
  save_tax_class: {
    args: { id: string; name: string; rate: string; treatment: string };
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

  // What an item is made of — scope 6.1–6.3. A size, a group of choices and a
  // combo are all prices, so all three need `menu.manage`.
  //
  // Each of these returns the WHOLE new picture rather than the one thing that
  // changed, for the reason the cart does (D4): the second copy is the one that
  // goes stale.
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

  // P14 — the floor. Scope 14.1 the plan, 14.2 the timers, 14.3 occupancy,
  // and 1.21/1.22/1.23, the three things you do to an order that is already
  // on a table.
  //
  // Every one of these returns the WHOLE floor, for the reason the cart does
  // (D4): the second copy is the one that goes stale.
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
  set_dining_table_active: {
    args: { tableId: string; active: boolean };
    returns: FloorView;
  };
  delete_dining_table: { args: { tableId: string }; returns: FloorView };
  save_floor_thresholds: { args: { warn: number; late: number }; returns: FloorView };
  move_order: { args: { orderId: string; toTable: string }; returns: FloorView };
  merge_orders: { args: { fromOrder: string; intoOrder: string }; returns: FloorView };
  split_order: { args: { request: SplitRequest }; returns: FloorView };
  /** Answers "what do we each owe?" — it does not create n bills. */
  even_split: { args: { ways: number }; returns: EvenSplitView };
  set_covers: { args: { covers: number | null }; returns: void };

  // P15 — customers and what they owe. The owner renamed this from "khata"
  // on 2026-08-08.
  //
  // The balance is never sent as a number to add up: every one of these
  // returns money already formatted and ageing already bucketed, because a
  // screen that divides by thirty has a second answer.
  who_owes: { args: void; returns: CustomerView[] };
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

  // P16 — money going out, and the drawer. Audit A2 / ANDROID-D1: v1 never
  // sent expenses anywhere, so every owner's phone showed a profit that was
  // too high, every day.
  expenses: { args: void; returns: ExpensesView };
  save_expense: { args: { edit: ExpenseEdit }; returns: ExpensesView };
  delete_expense: { args: { id: string }; returns: ExpensesView };
  /** The float, a top-up, a payout, a bank drop. A purchase is NOT one. */
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

  // --- P25, the stock book --------------------------------------------------
  // MARKET_GAP_ANALYSIS calls inventory "the biggest single hole". Every
  // quantity below crosses as a SENTENCE — "1.712 bag", "−180 g" — because
  // units are where inventory actually fails (D108), and the only thing this
  // side ever sends back is what a person typed and which unit they typed it
  // in (D109).
  inventory: { args: { material: string | null }; returns: InventoryView };
  recipe: { args: { ownerKind: string; ownerId: string }; returns: RecipeView };
  save_material: { args: { edit: MaterialEdit }; returns: InventoryView };
  save_recipe: { args: { edit: RecipeEdit }; returns: RecipeView };
  delete_recipe: { args: { ownerKind: string; ownerId: string }; returns: RecipeView };
  /** An opening balance, a purchase before P26, a wastage entry, an adjustment. */
  record_stock_movement: { args: { edit: MovementEdit }; returns: InventoryView };
  /** D114 — work the balances out again from the movements. */
  rebuild_stock_balances: { args: void; returns: InventoryView };
  resolve_stock_problem: { args: { id: string }; returns: InventoryView };
  /** Scope 4.9, D115 — theoretical against actual, and what nobody has counted. */
  stock_variance: { args: { from: string; to: string }; returns: VarianceView[] };
  /** Scope 4.6 — the buy list as text a person can send. */
  buy_list_text: { args: void; returns: string };

  // --- P26, buying and the count --------------------------------------------
  // **One rupee, one row** (D120). Saving a delivery moves the shelf, the paper
  // and what the shop owes in one transaction, and writes no expense row — so
  // there is no second command here for "also record it as a spend", and there
  // must never be one.
  buying: { args: { supplier: string | null }; returns: BuyingView };
  supplier_account: { args: { id: string }; returns: SupplierAccountView };
  purchase: { args: { id: string }; returns: PurchaseView };
  save_supplier: { args: { edit: SupplierEdit }; returns: BuyingView };
  save_purchase: { args: { edit: PurchaseEdit }; returns: BuyingView };
  /** D125 — a purchase is never edited. This is the only correction path. */
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
  /**
   * D132 — the photograph, already downscaled by this side (canvas, 1600 px,
   * JPEG 0.7). Rust checks the size, hashes it and writes the file; there is no
   * image library on that side at all.
   */
  attach_photo: { args: { dataUrl: string }; returns: PhotoView };
  purchase_photo: { args: { id: string }; returns: PhotoView };

  /** D127 — the count freezes the book and approving posts a DELTA. */
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
  /** D128 — and the book quantity is deliberately not on it. */
  count_sheet: { args: { location: string }; returns: string };

  /** D134 — scope 10.13. The summary is composed in Rust; this side sends it. */
  share_report: {
    args: { id: string; period: PeriodArg; channel: Channel };
    returns: ShareView;
  };

  // --- P27, the tills -------------------------------------------------------
  // Every one of them answers with the whole roster, so the screen never has to
  // work out what changed — the same shape the settings and the floor use.
  tills: { args: void; returns: TillsView };
  /** The prefix is the field that matters: D135's one remaining risk. */
  save_till: { args: { edit: TerminalEdit }; returns: TillsView };
  /** D139 — a person chooses. There is no election. */
  make_master: { args: { id: string }; returns: TillsView };
  /**
   * **Waits for somebody at the other counter to press Allow**, so it can take
   * a minute or two — the screen shows a spinner and Tauri runs it off the UI
   * thread.
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

  // --- P17, the settings ----------------------------------------------------
  // Five commands for ninety settings, because the catalogue IS the screen:
  // every label, help sentence, limit and choice comes down inside
  // `SettingsView`, and one component renders all of it.
  settings_all: { args: void; returns: SettingsView };
  /** After a restore, and whenever a screen wants to be sure. */
  reload_settings: { args: void; returns: SettingsView };
  /** Returns the KEYS that match. The matching rule lives in Rust with the
   *  synonym list it reads — a second copy here would disagree. */
  search_settings: { args: { text: string }; returns: string[] };
  save_settings: { args: { edits: SettingEdit[] }; returns: SavedView };
  /** What "reset this section" WOULD set. It does not save — the screen shows
   *  them as unsaved edits, so a reset can be looked at and cancelled. */
  settings_defaults_for: { args: { group: string }; returns: SettingEdit[] };
  /** The sample bill or ticket, laid out with the settings as they are on
   *  screen RIGHT NOW — saved or not. It renders the real mb-print document
   *  (audit D1: a hand-drawn imitation is how the preview and the paper come
   *  to disagree), on the shop's own paper width. */
  preview_settings: {
    args: { group: string; edits: SettingEdit[] };
    returns: PreviewView;
  };

  // The printers (P17 part 4). A printer is a RECORD, not a scalar, so it has
  // its own commands rather than a place in the catalogue.
  printer_setup: { args: void; returns: PrintersView };
  save_printer: { args: { edit: PrinterEdit }; returns: PrintersView };
  delete_printer: { args: { id: string }; returns: PrintersView };
  /** Scope 3.1. An empty printerId means "the default kitchen printer". */
  route_category: {
    args: { categoryId: string; printerId: string };
    returns: PrintersView;
  };
  /** A whole sample BILL, not a slip — a slip cannot show whether a bill is
   *  centred, and that is what somebody at the printer is asking. */
  print_sample_bill: { args: { printerId: string }; returns: string };
  /** Scope 7.11 — print, look at the paper, nudge, print again. */
  nudge_printer: {
    args: { printerId: string; dxMm: number; dyMm: number };
    returns: PrintersView;
  };

  // Backup (audit group A). `request_restore` does NOT restore: D27 says a
  // restore runs before the database is opened, so it records the request and
  // start-up carries it out.
  backup_status: { args: void; returns: BackupView };
  back_up_now: { args: void; returns: BackupView };
  verify_backup: { args: { path: string }; returns: VerifyView };
  request_restore: { args: { path: string }; returns: BackupView };
  cancel_restore: { args: void; returns: BackupView };
  /** Audit A5 — the screen somebody opens when everything else is broken. */
  find_shops: { args: void; returns: string[] };

  // The whole configuration, out and in — a dealer sets up the second shop by
  // copying one file. The import is a DRY RUN first, the same shape P13's CSV
  // import uses and for the same reason.
  export_settings: { args: void; returns: string };
  plan_settings_import: { args: { text: string }; returns: ConfigPlanView };
  run_settings_import: { args: { text: string }; returns: SavedView };

  // The counters (audit Part 3's two numbering blocks). A bill number that
  // goes backwards is a GST return the department will reject, so Rust
  // refuses it and says why.
  numbering: { args: void; returns: NumberingView };
  save_counter: { args: { edit: CounterEdit }; returns: NumberingView };

  // P18 — thirteen reports behind four commands, because the report list is
  // the screen. A period crosses as two `YYYY-MM-DD` strings: TypeScript does
  // no date arithmetic on the value every report is keyed by.
  // Audit G1's answer: "how did today go, and what needs me" — the question
  // thirteen reports do not answer because you have to know to ask them.
  dashboard: { args: void; returns: DashboardView };
  report_list: { args: void; returns: ReportListView };
  report: { args: { id: string; period: PeriodArg }; returns: ReportView };
  report_csv: { args: { id: string; period: PeriodArg }; returns: SavedFileView };
  report_pdf: { args: { id: string; period: PeriodArg }; returns: SavedFileView };

  // P19 — the phones this counter serves. Reading the panel is reports.view;
  // every write is devices.pair, because letting a phone onto the shop's
  // network is its own decision.
  network: { args: void; returns: NetworkView };
  open_pairing: { args: void; returns: NetworkView };
  close_pairing: { args: void; returns: NetworkView };
  allow_device: { args: { requestId: string }; returns: NetworkView };
  refuse_device: { args: { requestId: string }; returns: NetworkView };
  revoke_device: { args: { deviceId: string }; returns: NetworkView };

  // P18's day close — requirement 9 of the ten. `count_cash` recomputes the
  // variance as somebody types: there is ONE variance calculation and it is in
  // Rust, so the preview cannot disagree with what gets saved.
  day_close: { args: void; returns: DayCloseView };
  count_cash: { args: { counts: CountArg[] }; returns: DayCloseView };
  close_day: {
    args: { counts: CountArg[]; reason: string; print: boolean };
    returns: DayCloseView;
  };
  reopen_day: { args: { reason: string }; returns: DayCloseView };

  // P21 — the licence. Reading the screen is reports.view (the plan and the
  // renewal date are shop information); every write is licence.manage, which
  // is audit C1's own last example of what anybody behind the counter could
  // reach. **Every one of these returns the whole view**, so the screen never
  // has to work out what changed.
  account: { args: void; returns: LicenceView };
  refresh_licence: { args: void; returns: LicenceView };
  activate: { args: { key: string; proof: string }; returns: LicenceView };
  start_trial: { args: { contact: string }; returns: LicenceView };
  deactivate: { args: void; returns: LicenceView };
  transfer_here: { args: { key: string; proof: string }; returns: LicenceView };
  use_emergency_code: { args: { code: string }; returns: LicenceView };

  // P22 — is this counter healthy, and what can we send to support. The plan
  // is separate from the write on purpose (D94): a person sees the manifest
  // before the zip exists.
  health: { args: void; returns: HealthView };
  // Public in Rust, deliberately: on a first run nobody has a PIN yet, and a
  // set-up list that will not draw until somebody does is a list nobody can
  // use to create one.
  setup_list: { args: void; returns: SetupView };

  // P24 — the kitchen screen. Every one of them returns the WHOLE view, so a
  // cook's tap and another waiter's new order can never leave the screen
  // showing half of each.
  kitchen: { args: { station: string | null }; returns: KitchenView };
  kitchen_shown: { args: { id: string }; returns: KitchenView };
  kitchen_bump: { args: { id: string }; returns: KitchenView };
  kitchen_bump_line: { args: { id: string; key: string }; returns: KitchenView };
  kitchen_recall: { args: { id: string }; returns: KitchenView };
  kitchen_acknowledge: { args: { id: string }; returns: KitchenView };
  kitchen_fire: { args: { orderId: string; course: string }; returns: KitchenView };
  look_for_an_update: { args: void; returns: UpdateState };
  dismiss_update: { args: void; returns: UpdateState };
  go_back_a_version: { args: void; returns: string };
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

/**
 * A failure that did not come from Rust — the webview lost the bridge, or a
 * command name is wrong. Given the same shape so a screen has one thing to
 * render, and a code that says where to look.
 */
function asUiError(cause: unknown): UiError {
  if (isUiError(cause)) return cause;
  return {
    code: 'ipc.failed',
    message:
      'Magic Bill could not reach its own engine. Close it and open it again — ' +
      'nothing has been lost.',
    detail: String(cause),
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

/**
 * What Rust pushes, when it pushes it.
 *
 * **React never polls.** `PERFORMANCE.md` §5 rule 6: *"a 250 ms poll loop is M4
 * gone before a single feature is written."* There is a test asserting no
 * `setInterval` outside the one shared clock.
 */
export function subscribe(onPush: (message: Pushed) => void): Promise<() => void> {
  return listen<Pushed>('mb://push', (event) => onPush(event.payload));
}

/** True when we are running inside Tauri rather than a browser test. */
export function inApp(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
