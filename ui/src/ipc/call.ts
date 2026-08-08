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
  cart_add: {
    args: { itemId: string; qty: string | null; note: string | null };
    returns: CartView;
  };
  cart_set_qty: { args: { index: number; qty: string }; returns: CartView };
  cart_remove: { args: { index: number }; returns: CartView };
  cart_clear: { args: { keepType: boolean }; returns: CartView };
  cart_set_order_type: { args: { orderType: string }; returns: CartView };
  cart_add_payment: {
    args: { mode: string; amountPaise: bigint };
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
      amountPaise: bigint;
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
