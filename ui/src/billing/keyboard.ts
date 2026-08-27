/** The billing keyboard. */

import type { MenuItemView } from '../ipc/generated/MenuItemView';
import type { TableView } from '../ipc/generated/TableView';

// What the screen can be doing.

export type Mode =
  | { kind: 'searching' }
  /** The quantity popup. Everything else is inert while it is open. */
  | { kind: 'quantity'; item: MenuItemView; typed: string }
  /** Moving around the table grid. */
  | { kind: 'grid'; index: number }
  /** The chosen table is busy: merge, or take a sub-table letter (1.6). */
  | { kind: 'table-busy'; table: TableView; choice: number }
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
  | { kind: 'suggestions'; items: readonly MenuItemView[] }
  | { kind: 'tables'; tables: readonly TableView[] }
  | { kind: 'cart'; hasItems: boolean; kitchenUpToDate: boolean }
  | { kind: 'order-type'; value: string }
  | { kind: 'toggle-lock' }
  /** A tap. Touch reaches everything the keyboard does. */
  | { kind: 'tap-suggestion'; index: number }
  | { kind: 'tap-tile'; index: number }
  | { kind: 'click-empty'; textSelected: boolean; controlFocused: boolean };

/** What the screen must actually go and do. */
export type Command =
  | { do: 'search'; text: string }
  | { do: 'add-item'; itemId: string; qty: string }
  | { do: 'open-table'; tableId: string }
  | { do: 'print-kitchen' }
  | { do: 'complete-bill' }
  | { do: 'new-order' }
  | { do: 'confirm-new-order' }
  | { do: 'set-order-type'; value: string }
  | { do: 'focus-search' }
  | { do: 'merge-into'; tableId: string }
  | { do: 'sub-table'; tableId: string; letter: string };

/** B to H — seven second parties on one table is more than anybody needs. */
export const SUB_TABLE_LETTERS = ['B', 'C', 'D', 'E', 'F', 'G', 'H'] as const;

export function initial(orderType = 'Dine in'): State {
  return {
    mode: { kind: 'searching' },
    text: '',
    highlighted: -1,
    suggestions: [],
    tables: [],
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
      return [{ ...state, orderType: event.value }, []];

    case 'toggle-lock':
      return [{ ...state, orderTypeLocked: !state.orderTypeLocked }, []];

    case 'typed': {
      if (state.mode.kind === 'quantity') {
        return [{ ...state, mode: { ...state.mode, typed: event.text } }, []];
      }
      return [
        { ...state, text: event.text, mode: { kind: 'searching' } },
        [{ do: 'search', text: event.text }],
      ];
    }

    case 'tap-suggestion': {
      const item = state.suggestions[event.index];
      if (!item) return [state, []];
      // A tap opens the same popup Enter does — one path, two ways in.
      return [
        { ...state, mode: { kind: 'quantity', item, typed: '' } },
        [],
      ];
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

function key(state: State, pressed: string): [State, Command[]] {
  // The popup owns everything while it is open.
  if (state.mode.kind === 'quantity') {
    const popup = state.mode;
    if (pressed === 'Escape') {
      return [{ ...state, mode: { kind: 'searching' } }, []];
    }
    if (pressed === 'Enter') {
      // Blank means one. Typing a quantity is optional, which is what makes "name, Enter,
      // Enter" two keystrokes.
      const qty = popup.typed.trim() === '' ? '1' : popup.typed.trim();
      return [
        { ...state, mode: { kind: 'searching' }, text: '', suggestions: [], highlighted: -1 },
        [{ do: 'add-item', itemId: popup.item.id, qty }],
      ];
    }
    return [state, []];
  }

  if (state.mode.kind === 'help') {
    if (pressed === 'Escape' || pressed === '?') {
      return [{ ...state, mode: { kind: 'searching' } }, []];
    }
    return [state, []];
  }

  // The busy-table chooser.
  if (state.mode.kind === 'table-busy') {
    const busy = state.mode;
    // 0 is "merge"; 1..7 are the letters B to H.
    const options = 1 + SUB_TABLE_LETTERS.length;
    if (pressed === 'Escape') {
      return [{ ...state, mode: { kind: 'searching' } }, []];
    }
    if (pressed === 'ArrowRight' || pressed === 'ArrowDown') {
      return [
        { ...state, mode: { ...busy, choice: (busy.choice + 1) % options } },
        [],
      ];
    }
    if (pressed === 'ArrowLeft' || pressed === 'ArrowUp') {
      return [
        {
          ...state,
          mode: { ...busy, choice: (busy.choice - 1 + options) % options },
        },
        [],
      ];
    }
    if (pressed === 'Enter') {
      if (busy.choice === 0) {
        return [
          { ...state, mode: { kind: 'searching' } },
          [{ do: 'merge-into', tableId: busy.table.id }],
        ];
      }
      const letter = SUB_TABLE_LETTERS[busy.choice - 1];
      if (!letter) return [state, []];
      return [
        { ...state, mode: { kind: 'searching' } },
        [{ do: 'sub-table', tableId: busy.table.id, letter }],
      ];
    }
    return [state, []];
  }

  // The grid.
  if (state.mode.kind === 'grid') {
    const grid = state.mode;
    if (pressed === 'Escape') {
      return [{ ...state, mode: { kind: 'searching' } }, [{ do: 'focus-search' }]];
    }
    const moved = moveInGrid(grid.index, pressed, state.tables.length);
    if (moved !== null) {
      return [{ ...state, mode: { kind: 'grid', index: moved } }, []];
    }
    if (pressed === 'Enter') {
      const table = state.tables[grid.index];
      if (!table) return [state, []];
      return openTile(state, table);
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
    // Unwinds ONE layer at a time.
    if (state.suggestions.length > 0 || state.text !== '') {
      return [
        { ...state, text: '', suggestions: [], highlighted: -1 },
        [{ do: 'search', text: '' }],
      ];
    }
    // Never destroys work silently: with a cart, it asks.
    return [state, [{ do: state.cartHasItems ? 'confirm-new-order' : 'new-order' }]];
  }

  const hasSuggestions = state.suggestions.length > 0;

  if (pressed === 'ArrowDown') {
    if (hasSuggestions) {
      return [
        { ...state, highlighted: (state.highlighted + 1) % state.suggestions.length },
        [],
      ];
    }
    // Down on an empty box with nothing suggested enters the grid — which is how a cashier gets
    // to the floor without the mouse.
    if (state.text === '' && state.tables.length > 0) {
      return [{ ...state, mode: { kind: 'grid', index: 0 } }, []];
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
    // A highlighted suggestion wins.
    if (hasSuggestions && state.highlighted >= 0) {
      const item = state.suggestions[state.highlighted];
      if (item) {
        return [{ ...state, mode: { kind: 'quantity', item, typed: '' } }, []];
      }
    }

    // Type a table name and press Enter — the trick, and it takes precedence over item search
    // because a cashier types "6" far more often than they mean an item called 6.
    if (state.text !== '') {
      const table = matchTable(state.tables, state.text);
      if (table) {
        return [
          { ...state, text: '', suggestions: [], highlighted: -1 },
          [{ do: 'open-table', tableId: table.id }],
        ];
      }
      // Not a table: fall through to search, which is already happening.
      return [state, []];
    }

    // ENTER ON AN EMPTY BOX.
    if (state.cartHasItems) {
      return [
        state,
        [{ do: state.kitchenUpToDate ? 'complete-bill' : 'print-kitchen' }],
      ];
    }
    const firstOpen = state.tables.find((t) => t.orderId !== null);
    if (firstOpen) {
      return [state, [{ do: 'open-table', tableId: firstOpen.id }]];
    }
    return [state, []];
  }

  return [state, []];
}

/** A tile was chosen, by key or by tap. */
function openTile(state: State, table: TableView): [State, Command[]] {
  // A busy table with a dine-in order in the cart is the sub-table question.
  if (table.orderId !== null && state.cartHasItems) {
    return [
      { ...state, mode: { kind: 'table-busy', table, choice: 0 } },
      [],
    ];
  }
  return [
    { ...state, mode: { kind: 'searching' }, text: '', suggestions: [] },
    [{ do: 'open-table', tableId: table.id }],
  ];
}

/** Two-dimensional movement over a grid that wraps. */
export const GRID_COLUMNS = 6;

function moveInGrid(index: number, pressed: string, count: number): number | null {
  if (count === 0) return null;
  switch (pressed) {
    case 'ArrowRight':
      return (index + 1) % count;
    case 'ArrowLeft':
      return (index - 1 + count) % count;
    case 'ArrowDown':
      return (index + GRID_COLUMNS) % count;
    case 'ArrowUp':
      return (index - GRID_COLUMNS + count * GRID_COLUMNS) % count;
    default:
      return null;
  }
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
  { group: 'Searching', keys: 'type', what: 'Search the menu' },
  { group: 'Searching', keys: '↑ ↓', what: 'Move through the suggestions' },
  { group: 'Searching', keys: 'Enter', what: 'Choose the highlighted item' },
  { group: 'Searching', keys: 'Esc', what: 'Clear the search' },
  { group: 'Quantity', keys: 'type', what: 'Type a quantity — 2, or 0.5' },
  { group: 'Quantity', keys: 'Enter', what: 'Add it (blank means one)' },
  { group: 'Quantity', keys: 'Esc', what: 'Cancel' },
  { group: 'The order', keys: 'Enter', what: 'On an empty box: print the kitchen ticket, then complete the bill' },
  { group: 'The order', keys: 'table number, Enter', what: "Open that table's order" },
  { group: 'The order', keys: '← →', what: 'Change the order type (unless it is locked)' },
  { group: 'The order', keys: 'Esc', what: 'Start a new order' },
  { group: 'The floor', keys: '↓', what: 'From an empty box, move into the table grid' },
  { group: 'The floor', keys: '← ↑ → ↓', what: 'Move around the grid' },
  { group: 'The floor', keys: 'Enter', what: 'Open the highlighted table' },
  { group: 'The floor', keys: 'Esc', what: 'Back to the search box' },
  { group: 'Help', keys: '?', what: 'Show this sheet' },
  // The key is handled by the shell rather than by this reducer — it must work on every screen,
  // not only on this one — but it is documented HERE, because the help sheet is generated from
  // this table and a shortcut written down somewhere else is a shortcut nobody learns.
  { group: 'The counter', keys: 'Ctrl + L', what: 'Lock the counter' },
];
