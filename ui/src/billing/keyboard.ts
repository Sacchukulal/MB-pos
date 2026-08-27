/** The billing keyboard. */

import type { MenuItemView } from '../ipc/generated/MenuItemView';
import type { TableView } from '../ipc/generated/TableView';

// What the screen can be doing.

export type Mode =
  | { kind: 'searching' }
  /** An item was chosen; how many of it is being typed. */
  | { kind: 'quantity'; item: MenuItemView; text: string }
  /** Moving through the processing orders. */
  | { kind: 'processing'; index: number }
  /** The shortcut sheet. */
  | { kind: 'help' };

export interface State {
  mode: Mode;
  /** What is in the search box. */
  text: string;
  /** Which suggestion is highlighted. */
  highlighted: number;
  suggestions: readonly MenuItemView[];
  /** The floor, so Enter on a typed table name can resolve it. */
  tables: readonly TableView[];
  /** The orders the kitchen has, in the order the panel shows them. */
  processing: readonly TableView[];
  /** And the LOCK is what stops a parcel counter re-picking it. */
  orderType: string;
  orderTypeLocked: boolean;
  /** Whether the cart has anything in it. */
  cartHasItems: boolean;
  /** Whether the kitchen has been told everything in the cart. */
  kitchenUpToDate: boolean;
}

/** Ten, and it is a design decision rather than a performance limit. */
export const MAX_SUGGESTIONS = 10;

export const ORDER_TYPES = [
  'Dine in',
  'Parcel',
  'Self service',
  'Delivery',
] as const;

// What happens to it.

export type Event =
  | { kind: 'key'; key: string; shift?: boolean }
  | { kind: 'typed'; text: string }
  /** What is in the quantity box. */
  | { kind: 'typed-qty'; text: string }
  | { kind: 'suggestions'; items: readonly MenuItemView[] }
  | { kind: 'tables'; tables: readonly TableView[] }
  | { kind: 'processing'; orders: readonly TableView[] }
  | { kind: 'cart'; hasItems: boolean; kitchenUpToDate: boolean }
  /** The order type, and whether the shop's settings lock it. */
  | { kind: 'order-type'; value: string; locked: boolean }
  /** A tap. Touch reaches everything the keyboard does. */
  | { kind: 'tap-suggestion'; index: number }
  | { kind: 'tap-tile'; index: number }
  | { kind: 'click-empty'; textSelected: boolean; controlFocused: boolean };

/** What the screen must actually go and do. */
export type Command =
  | { do: 'search'; text: string }
  | { do: 'add-item'; itemId: string; qty: string }
  | { do: 'open-table'; tableId: string }
  | { do: 'open-order'; orderId: string }
  | { do: 'print-kitchen' }
  | { do: 'complete-bill' }
  | { do: 'new-order' }
  | { do: 'set-order-type'; value: string }
  | { do: 'focus-search' };

export function initial(orderType = 'Dine in'): State {
  return {
    mode: { kind: 'searching' },
    text: '',
    highlighted: -1,
    suggestions: [],
    tables: [],
    processing: [],
    orderType,
    orderTypeLocked: false,
    cartHasItems: false,
    kitchenUpToDate: true,
  };
}

/** The whole keyboard, as one function. */
export function reduce(state: State, event: Event): [State, Command[]] {
  switch (event.kind) {
    case 'suggestions': {
      const items = event.items.slice(0, MAX_SUGGESTIONS);
      return [
        {
          ...state,
          suggestions: items,
          // The first is highlighted, so Enter is always one keystroke away.
          highlighted: items.length > 0 ? 0 : -1,
        },
        [],
      ];
    }

    case 'tables':
      return [{ ...state, tables: event.tables }, []];

    case 'processing': {
      // A highlight past the end of a shorter list moves onto its last row; an empty list
      // sends it back to the box.
      const count = event.orders.length;
      const mode =
        state.mode.kind !== 'processing' || state.mode.index < count
          ? state.mode
          : count > 0
            ? { kind: 'processing' as const, index: count - 1 }
            : { kind: 'searching' as const };
      return [{ ...state, processing: event.orders, mode }, []];
    }

    case 'cart':
      return [
        {
          ...state,
          cartHasItems: event.hasItems,
          kitchenUpToDate: event.kitchenUpToDate,
        },
        [],
      ];

    case 'order-type':
      return [{ ...state, orderType: event.value, orderTypeLocked: event.locked }, []];

    case 'typed': {
      return [
        { ...state, text: event.text, mode: { kind: 'searching' } },
        [{ do: 'search', text: event.text }],
      ];
    }

    case 'typed-qty':
      if (state.mode.kind !== 'quantity') return [state, []];
      return [{ ...state, mode: { ...state.mode, text: event.text } }, []];

    case 'tap-suggestion': {
      const item = state.suggestions[event.index];
      if (!item) return [state, []];
      return ask(state, item);
    }

    case 'tap-tile': {
      const table = state.tables[event.index];
      if (!table) return [state, []];
      return openTile(state, table);
    }

    case 'click-empty':
      if (event.textSelected || event.controlFocused) return [state, []];
      return [state, [{ do: 'focus-search' }]];

    case 'key':
      return key(state, event.key);
  }
}

/** An item was chosen: ask how many, with one already written in. */
function ask(state: State, item: MenuItemView): [State, Command[]] {
  return [{ ...state, mode: { kind: 'quantity', item, text: '1' } }, []];
}

/** "2", "0.5", "1.25" — or one, when nothing usable was typed. */
export function quantityOf(text: string): string {
  const n = Number(text.trim());
  return Number.isFinite(n) && n > 0 ? text.trim() : '1';
}

/** One more or one fewer, never below one whole. */
function stepQuantity(text: string, by: number): string {
  const n = Number(text.trim());
  const now = Number.isFinite(n) && n > 0 ? n : 1;
  const next = Math.round((now + by) * 1000) / 1000;
  return String(next >= 1 ? next : now);
}

/** Back to the box, with nothing typed in it — the counter is ready for the next thing. */
function cleared(state: State): State {
  return { ...state, mode: { kind: 'searching' }, text: '', suggestions: [], highlighted: -1 };
}

function key(state: State, pressed: string): [State, Command[]] {
  if (state.mode.kind === 'help') {
    if (pressed === 'Escape' || pressed === '?') {
      return [{ ...state, mode: { kind: 'searching' } }, [{ do: 'focus-search' }]];
    }
    return [state, []];
  }

  // How many of the chosen item.
  if (state.mode.kind === 'quantity') {
    const asking = state.mode;
    if (pressed === 'Escape') {
      // Nothing was added; the suggestions are still there to choose from.
      return [{ ...state, mode: { kind: 'searching' } }, [{ do: 'focus-search' }]];
    }
    if (pressed === 'Enter') {
      return [
        cleared(state),
        [
          { do: 'add-item', itemId: asking.item.id, qty: quantityOf(asking.text) },
          { do: 'focus-search' },
        ],
      ];
    }
    if (pressed === 'ArrowUp' || pressed === 'ArrowDown') {
      const text = stepQuantity(asking.text, pressed === 'ArrowUp' ? 1 : -1);
      return [{ ...state, mode: { ...asking, text } }, []];
    }
    return [state, []];
  }

  // The processing orders.
  if (state.mode.kind === 'processing') {
    const at = state.mode.index;
    const count = state.processing.length;
    if (pressed === 'Escape') {
      return [cleared(state), [{ do: 'new-order' }, { do: 'focus-search' }]];
    }
    if (pressed === 'ArrowDown' && count > 0) {
      return [{ ...state, mode: { kind: 'processing', index: (at + 1) % count } }, []];
    }
    if (pressed === 'ArrowUp' && count > 0) {
      return [{ ...state, mode: { kind: 'processing', index: (at - 1 + count) % count } }, []];
    }
    if (pressed === 'Enter') {
      const order = state.processing[at];
      if (!order) return [cleared(state), [{ do: 'focus-search' }]];
      // Already in the cart: this Enter completes it. Otherwise into the cart — and the
      // highlight stays on the row, so the arrows carry on from here.
      if (order.selected && state.cartHasItems) {
        return [state, [{ do: 'complete-bill' }]];
      }
      return [state, [openCommand(order), { do: 'focus-search' }]];
    }
    return [state, []];
  }

  // searching: the default, and where a cashier lives.
  if (pressed === '?') {
    // Only when the box is empty; otherwise "?" is a character somebody is typing into a
    // search.
    if (state.text === '') {
      return [{ ...state, mode: { kind: 'help' } }, []];
    }
    return [state, []];
  }

  if (pressed === 'Escape') {
    // From anywhere: a fresh order, and the box ready to type into. A typed cart is nothing
    // in the books yet, so it goes without a question; a parked order is cancelled with a
    // reason, from the actions.
    return [cleared(state), [{ do: 'new-order' }, { do: 'focus-search' }]];
  }

  const hasSuggestions = state.suggestions.length > 0;

  if (pressed === 'ArrowDown') {
    if (hasSuggestions) {
      return [
        { ...state, highlighted: (state.highlighted + 1) % state.suggestions.length },
        [],
      ];
    }
    // Down on an empty box with nothing suggested moves into the processing orders — which is
    // how a cashier reaches a bill to complete without the mouse. It starts on the order that
    // is in the cart, when one is, so a tap on a row is where the arrows carry on from.
    if (state.text === '' && state.processing.length > 0) {
      const on = state.processing.findIndex((order) => order.selected);
      return [{ ...state, mode: { kind: 'processing', index: on >= 0 ? on : 0 } }, []];
    }
    return [state, []];
  }

  if (pressed === 'ArrowUp') {
    if (hasSuggestions) {
      const next =
        (state.highlighted - 1 + state.suggestions.length) % state.suggestions.length;
      return [{ ...state, highlighted: next }, []];
    }
    return [state, []];
  }

  // Left/Right cycle the order type — UNLESS the lock is on, which is the whole point of the
  // lock.
  if (pressed === 'ArrowLeft' || pressed === 'ArrowRight') {
    if (hasSuggestions || state.orderTypeLocked) return [state, []];
    const at = ORDER_TYPES.indexOf(state.orderType as (typeof ORDER_TYPES)[number]);
    const step = pressed === 'ArrowRight' ? 1 : -1;
    const next =
      ORDER_TYPES[(at + step + ORDER_TYPES.length) % ORDER_TYPES.length] ??
      state.orderType;
    return [{ ...state, orderType: next }, [{ do: 'set-order-type', value: next }]];
  }

  if (pressed === 'Enter') {
    // Type a table name and press Enter — the trick, and it comes BEFORE the suggestions
    // because a cashier types "6" far more often than they mean an item with a 6 in its name.
    if (state.text !== '') {
      const table = matchTable(state.tables, state.text);
      if (table) {
        return [{ ...state, text: '', suggestions: [], highlighted: -1 }, [openCommand(table)]];
      }
    }

    // Then a highlighted suggestion.
    if (hasSuggestions && state.highlighted >= 0) {
      const item = state.suggestions[state.highlighted];
      if (item) return ask(state, item);
    }

    // Not a table and nothing suggested: the search is already happening.
    if (state.text !== '') return [state, []];

    // ENTER ON AN EMPTY BOX. The kitchen ticket first — the order then waits in the processing
    // orders — and the bill once the kitchen has everything.
    if (state.cartHasItems) {
      return [
        state,
        [{ do: state.kitchenUpToDate ? 'complete-bill' : 'print-kitchen' }],
      ];
    }
    const firstOpen = state.tables.find((t) => t.orderId !== null);
    if (firstOpen) {
      return [state, [openCommand(firstOpen)]];
    }
    return [state, []];
  }

  return [state, []];
}

/** A tile was chosen, by key or by tap. Typed items go with the cashier — Rust sees to that. */
function openTile(state: State, table: TableView): [State, Command[]] {
  return [
    { ...state, mode: { kind: 'searching' }, text: '', suggestions: [], highlighted: -1 },
    [openCommand(table)],
  ];
}

/** A tile with a table opens the table; a parcel, a self-service order or a second party IS its order. */
function openCommand(table: TableView): Command {
  if (table.orderId !== null && table.id === table.orderId) {
    return { do: 'open-order', orderId: table.orderId };
  }
  return { do: 'open-table', tableId: table.id };
}

/** Does this text name a table? */
function matchTable(
  tables: readonly TableView[],
  text: string,
): TableView | undefined {
  const wanted = text.trim().toLowerCase();
  if (wanted === '') return undefined;
  return tables.find((t) => t.label.toLowerCase() === wanted);
}

/** Every shortcut, in one table — and the help overlay is generated from it. */
export const SHORTCUTS: readonly {
  keys: string;
  what: string;
  group: string;
}[] = [
  { group: 'Searching', keys: 'type', what: 'Search the menu — from anywhere on the screen' },
  { group: 'Searching', keys: '↑ ↓', what: 'Move through the suggestions' },
  { group: 'Searching', keys: 'Enter', what: 'Choose the highlighted item, then say how many' },
  { group: 'Searching', keys: '↑ ↓ / Enter / Esc', what: 'How many: one more or fewer, add it, or leave it' },
  { group: 'The order', keys: 'press the quantity', what: 'Change how many, on the line' },
  { group: 'The order', keys: 'Enter', what: 'On an empty box: print the kitchen ticket — the order waits under Processing orders' },
  { group: 'The order', keys: 'Enter', what: 'On an order from Processing orders: complete the bill' },
  { group: 'The order', keys: 'table number, Enter', what: "Open that table's order — typed items go with you" },
  { group: 'The order', keys: '← →', what: 'Change the order type (unless the shop locks it)' },
  { group: 'The order', keys: 'Esc', what: 'New order, from anywhere' },
  { group: 'Processing orders', keys: '↓', what: 'From an empty box, into the processing orders' },
  { group: 'Processing orders', keys: '↑ ↓', what: 'Move through them' },
  { group: 'Processing orders', keys: 'Enter', what: 'Open it in the cart; Enter again completes the bill' },
  { group: 'Help', keys: '?', what: 'Show this sheet' },
  // The key is handled by the shell rather than by this reducer — it must work on every screen,
  // not only on this one — but it is documented HERE, because the help sheet is generated from
  // this table and a shortcut written down somewhere else is a shortcut nobody learns.
  { group: 'The counter', keys: 'Ctrl + L', what: 'Lock the counter' },
];
