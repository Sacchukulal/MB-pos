/** The keyboard state machine. */

import { describe, expect, it } from 'vitest';

import {
  MAX_SUGGESTIONS,
  ORDER_TYPES,
  SHORTCUTS,
  initial,
  quantityOf,
  reduce,
  type Command,
  type Event,
  type State,
} from '../src/billing/keyboard';
import type { MenuItemView } from '../src/ipc/generated/MenuItemView';
import type { TableView } from '../src/ipc/generated/TableView';

function item(id: string, name: string): MenuItemView {
  return {
    id,
    name,
    price: { paise: 12_000n, text: '120.00' },
    rateLabel: '5%',
    category: null,
  };
}

function table(label: string, busy = false): TableView {
  return {
    id: `tbl_${label}`,
    label,
    section: 'Main Hall',
    seats: 4,
    state: busy ? 'occupied' : 'free',
    total: busy ? { paise: 64_600n, text: '646.00' } : null,
    minutes: busy ? 12 : null,
    kitchenTold: true,
    billNumber: null,
    kitchenMinutes: null,
    orderId: busy ? `ord_${label}` : null,
    selected: false,
  };
}

/** Drive a sequence of events and return where it ended up. */
function run(start: State, ...events: Event[]): [State, Command[]] {
  let state = start;
  let commands: Command[] = [];
  for (const event of events) {
    const [next, issued] = reduce(state, event);
    state = next;
    commands = issued;
  }
  return [state, commands];
}

const press = (key: string): Event => ({ kind: 'key', key });
const type = (text: string): Event => ({ kind: 'typed', text });
const suggest = (...items: MenuItemView[]): Event => ({
  kind: 'suggestions',
  items,
});
const floor = (...tables: TableView[]): Event => ({ kind: 'tables', tables });
const cooking = (...orders: TableView[]): Event => ({ kind: 'processing', orders });
const cart = (hasItems: boolean, kitchenUpToDate = true): Event => ({
  kind: 'cart',
  hasItems,
  kitchenUpToDate,
});

/** Typed, suggested, chosen: the state a cashier is in the moment the how-many box opens. */
const chosen = (): State =>
  run(initial(), type('dos'), suggest(item('itm_dosa', 'Masala Dosa')), press('Enter'))[0];

describe('searching', () => {
  it('typing searches, and the first result is highlighted', () => {
    // Highlighted immediately, so Enter is always one keystroke away.
    const [state, commands] = run(
      initial(),
      type('dos'),
      suggest(item('itm_dosa', 'Masala Dosa')),
    );
    expect(state.highlighted).toBe(0);
    expect(commands).toEqual([]);

    const [, searched] = run(initial(), type('dos'));
    expect(searched).toEqual([{ do: 'search', text: 'dos' }]);
  });

  it('never offers more than ten suggestions', () => {
    const many = Array.from({ length: 40 }, (_, n) => item(`i${n}`, `Item ${n}`));
    const [state] = run(initial(), suggest(...many));
    expect(state.suggestions).toHaveLength(MAX_SUGGESTIONS);
  });

  it('arrows move the highlight and wrap', () => {
    const three = [item('a', 'A'), item('b', 'B'), item('c', 'C')];
    let [state] = run(initial(), type('x'), suggest(...three));
    [state] = run(state, press('ArrowDown'));
    expect(state.highlighted).toBe(1);
    [state] = run(state, press('ArrowUp'), press('ArrowUp'));
    expect(state.highlighted).toBe(2);
  });

  it('Enter on a suggestion asks how many, with one written in, and adds nothing yet', () => {
    const [state, commands] = run(
      initial(),
      type('dos'),
      suggest(item('itm_dosa', 'Masala Dosa')),
      press('Enter'),
    );
    expect(commands).toEqual([]);
    expect(state.mode).toEqual({ kind: 'quantity', item: item('itm_dosa', 'Masala Dosa'), text: '1' });
    // The search is still there behind it, for Esc to come back to.
    expect(state.text).toBe('dos');
  });

  it('a tap on a suggestion does the same', () => {
    const [byKey] = run(initial(), type('dos'), suggest(item('itm_dosa', 'Masala Dosa')), press('Enter'));
    const [byTap] = run(initial(), type('dos'), suggest(item('itm_dosa', 'Masala Dosa')), {
      kind: 'tap-suggestion',
      index: 0,
    });
    expect(byTap.mode).toEqual(byKey.mode);
  });
});

describe('how many (step 2 of the counter flow)', () => {
  it('Enter alone adds one, and clears the box for the next item', () => {
    const [state, commands] = run(chosen(), press('Enter'));
    expect(commands).toEqual([
      { do: 'add-item', itemId: 'itm_dosa', qty: '1' },
      { do: 'focus-search' },
    ]);
    expect(state.mode.kind).toBe('searching');
    expect(state.text).toBe('');
    expect(state.suggestions).toHaveLength(0);
  });

  it('a typed number replaces the one', () => {
    const [, commands] = run(chosen(), { kind: 'typed-qty', text: '3' }, press('Enter'));
    expect(commands[0]).toEqual({ do: 'add-item', itemId: 'itm_dosa', qty: '3' });
  });

  it('the arrows step it, never below one', () => {
    let [state] = run(chosen(), press('ArrowUp'), press('ArrowUp'));
    expect(state.mode).toMatchObject({ kind: 'quantity', text: '3' });
    [state] = run(state, press('ArrowDown'), press('ArrowDown'), press('ArrowDown'));
    expect(state.mode).toMatchObject({ kind: 'quantity', text: '1' });
    // A weight steps by whole ones too.
    [state] = run(chosen(), { kind: 'typed-qty', text: '0.5' }, press('ArrowUp'));
    expect(state.mode).toMatchObject({ kind: 'quantity', text: '1.5' });
  });

  it('Esc leaves without adding, and the suggestions are still there', () => {
    const [state, commands] = run(chosen(), press('Escape'));
    expect(commands).toEqual([{ do: 'focus-search' }]);
    expect(state.mode.kind).toBe('searching');
    expect(state.suggestions).toHaveLength(1);
  });

  it('nothing usable typed means one', () => {
    expect(quantityOf('')).toBe('1');
    expect(quantityOf('0')).toBe('1');
    expect(quantityOf('.')).toBe('1');
    expect(quantityOf('2.5')).toBe('2.5');
  });
});

describe('Enter on an empty box (step 3)', () => {
  it('prints the kitchen ticket when the kitchen has not seen everything', () => {
    const [, commands] = run(initial(), cart(true, false), press('Enter'));
    expect(commands).toEqual([{ do: 'print-kitchen' }]);
  });

  it('completes the bill once the kitchen is up to date', () => {
    const [, commands] = run(initial(), cart(true, true), press('Enter'));
    expect(commands).toEqual([{ do: 'complete-bill' }]);
  });

  it('opens the first running order when the cart is empty', () => {
    const [, commands] = run(
      initial(),
      cart(false),
      floor(table('1'), table('6', true)),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_6' }]);
  });

  it('does nothing at all when there is nothing to do', () => {
    const [, commands] = run(initial(), cart(false), floor(table('1')), press('Enter'));
    expect(commands).toEqual([]);
  });
});

describe('a table name loads its order; anything else falls through', () => {
  it('opens the table when the text names one', () => {
    const [state, commands] = run(initial(), floor(table('6', true)), type('6'), press('Enter'));
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_6' }]);
    expect(state.text).toBe('');
  });

  it('opens a BUSY table with typed items without asking — the items go with you', () => {
    // The question that used to pop up here is gone: Rust puts the typed lines on that
    // table's bill, and a second party is the + on the tile.
    const [state, commands] = run(
      initial(),
      cart(true),
      floor(table('6', true)),
      type('6'),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_6' }]);
    expect(state.mode.kind).toBe('searching');
  });

  it('a tap on a busy tile does the same', () => {
    const [state, commands] = run(initial(), cart(true), floor(table('6', true)), {
      kind: 'tap-tile',
      index: 0,
    });
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_6' }]);
    expect(state.mode.kind).toBe('searching');
  });

  it('a second party is its own order, and a tap opens THAT', () => {
    const party = { ...table('6B', true), id: 'ord_6B', orderId: 'ord_6B' };
    const [, commands] = run(initial(), floor(table('6', true), party), type('6b'), press('Enter'));
    expect(commands).toEqual([{ do: 'open-order', orderId: 'ord_6B' }]);
  });

  it('a table number beats a menu item that happens to match it', () => {
    // "2" found "Gulab Jamun (2 pc)" on a real menu, and Enter asked how many jamuns.
    const [state, commands] = run(
      initial(),
      floor(table('2')),
      type('2'),
      suggest(item('itm_jamun', 'Gulab Jamun (2 pc)')),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_2' }]);
    expect(state.mode.kind).toBe('searching');
  });

  it('falls through to item search when it does not', () => {
    const [, commands] = run(initial(), floor(table('6', true)), type('dosa'), press('Enter'));
    expect(commands).toEqual([]);
  });

  it('matches a table EXACTLY, so "1" is not table 12', () => {
    const [, commands] = run(
      initial(),
      floor(table('1'), table('12', true)),
      type('1'),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_1' }]);
  });
});

describe('the order type, and the lock', () => {
  it('arrows cycle it both ways', () => {
    let [state, commands] = run(initial(), press('ArrowRight'));
    expect(state.orderType).toBe('Parcel');
    expect(commands).toEqual([{ do: 'set-order-type', value: 'Parcel' }]);

    [state] = run(state, press('ArrowLeft'));
    expect(state.orderType).toBe('Dine in');

    // And it wraps rather than stopping.
    [state] = run(state, press('ArrowLeft'));
    expect(state.orderType).toBe(ORDER_TYPES[ORDER_TYPES.length - 1]);
  });

  it('the LOCK stops them, which is the entire point of it', () => {
    const [state, commands] = run(
      initial(),
      { kind: 'order-type', value: 'Dine in', locked: true },
      press('ArrowRight'),
    );
    expect(state.orderType).toBe('Dine in');
    expect(commands).toEqual([]);
  });

  it('arrows belong to the suggestions when there are any', () => {
    const [state] = run(
      initial(),
      type('a'),
      suggest(item('a', 'A'), item('b', 'B')),
      press('ArrowRight'),
    );
    expect(state.orderType).toBe('Dine in');
  });
});

describe('Esc is a new order, from anywhere (step 1)', () => {
  const fresh = [{ do: 'new-order' }, { do: 'focus-search' }];

  it('with a search half typed', () => {
    const [state, commands] = run(
      initial(),
      type('dos'),
      suggest(item('itm_dosa', 'Masala Dosa')),
      press('Escape'),
    );
    expect(commands).toEqual(fresh);
    expect(state.text).toBe('');
    expect(state.suggestions).toHaveLength(0);
  });

  it('over a typed cart, without asking — nothing is in the books yet', () => {
    const [, commands] = run(initial(), cart(true), press('Escape'));
    expect(commands).toEqual(fresh);
  });

  it('from the processing orders', () => {
    const [state, commands] = run(
      initial(),
      cooking(table('6', true)),
      press('ArrowDown'),
      press('Escape'),
    );
    expect(commands).toEqual(fresh);
    expect(state.mode.kind).toBe('searching');
  });

  it('but the help sheet and the how-many box only close — one layer is theirs', () => {
    const [help, closed] = run(initial(), press('?'), press('Escape'));
    expect(help.mode.kind).toBe('searching');
    expect(closed).toEqual([{ do: 'focus-search' }]);
    const [, left] = run(chosen(), press('Escape'));
    expect(left).toEqual([{ do: 'focus-search' }]);
  });
});

describe('the processing orders by keyboard (step 4)', () => {
  const two = [table('3', true), { ...table('Parcel', true), section: null, id: 'ord_Parcel' }];

  it('Down on an empty box moves into them, and the arrows wrap', () => {
    let [state] = run(initial(), cooking(...two), press('ArrowDown'));
    expect(state.mode).toEqual({ kind: 'processing', index: 0 });
    [state] = run(state, press('ArrowDown'));
    expect(state.mode).toEqual({ kind: 'processing', index: 1 });
    [state] = run(state, press('ArrowDown'));
    expect(state.mode).toEqual({ kind: 'processing', index: 0 });
    [state] = run(state, press('ArrowUp'));
    expect(state.mode).toEqual({ kind: 'processing', index: 1 });
  });

  it('does not move when there is nothing cooking', () => {
    const [state] = run(initial(), floor(table('1')), press('ArrowDown'));
    expect(state.mode.kind).toBe('searching');
  });

  it('Enter opens the highlighted order in the cart, and the next Enter completes it', () => {
    let [state, commands] = run(initial(), cooking(...two), press('ArrowDown'), press('Enter'));
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_3' }, { do: 'focus-search' }]);
    // The highlight STAYS on the row, so the arrows carry on from here.
    expect(state.mode).toEqual({ kind: 'processing', index: 0 });
    // In the cart now (the floor says so), and the kitchen already has everything on it.
    const inCart = [{ ...two[0], selected: true }, two[1]] as TableView[];
    [state, commands] = run(state, cooking(...inCart), cart(true, true), press('Enter'));
    expect(commands).toEqual([{ do: 'complete-bill' }]);
    // Or from the box, the same second Enter.
    [, commands] = run(state, press('Escape'), cooking(...inCart), cart(true, true), press('Enter'));
    expect(commands).toEqual([{ do: 'complete-bill' }]);
  });

  it('the arrows carry on from the order that is in the cart, however it got there', () => {
    // A tap on a row, or on its tile, put it in the cart: Down goes to it, not to the top.
    const inCart = [two[0], { ...two[1], selected: true }] as TableView[];
    const [state] = run(initial(), cooking(...inCart), press('ArrowDown'));
    expect(state.mode).toEqual({ kind: 'processing', index: 1 });
  });

  it('a highlight past the end of a shorter list moves onto its last row', () => {
    // The row above was billed and left the list; the arrows are still in the list.
    let [state] = run(initial(), cooking(...two), press('ArrowDown'), press('ArrowDown'));
    expect(state.mode).toEqual({ kind: 'processing', index: 1 });
    [state] = run(state, cooking(two[1] as TableView));
    expect(state.mode).toEqual({ kind: 'processing', index: 0 });
  });

  it('a parcel in the list is its own order', () => {
    const [, commands] = run(
      initial(),
      cooking(...two),
      press('ArrowDown'),
      press('ArrowDown'),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'open-order', orderId: 'ord_Parcel' }, { do: 'focus-search' }]);
  });

  it('a highlight whose whole list was billed goes back to the box', () => {
    const [state] = run(initial(), cooking(...two), press('ArrowDown'), cooking());
    expect(state.mode.kind).toBe('searching');
  });
});

describe('focus is never stolen', () => {
  it('a click on nothing returns the caret to the search box', () => {
    const [, commands] = run(initial(), {
      kind: 'click-empty',
      textSelected: false,
      controlFocused: false,
    });
    expect(commands).toEqual([{ do: 'focus-search' }]);
  });

  it('but NOT while text is selected, and NOT while a control has focus', () => {
    for (const [textSelected, controlFocused] of [
      [true, false],
      [false, true],
    ] as const) {
      const [, commands] = run(initial(), { kind: 'click-empty', textSelected, controlFocused });
      expect(commands).toEqual([]);
    }
  });
});

describe('the help sheet', () => {
  it('opens on "?" and closes on Esc', () => {
    let [state] = run(initial(), press('?'));
    expect(state.mode.kind).toBe('help');
    [state] = run(state, press('Escape'));
    expect(state.mode.kind).toBe('searching');
  });

  it('does not open while somebody is typing a search', () => {
    const [state] = run(initial(), type('idl'), press('?'));
    expect(state.mode.kind).toBe('searching');
  });

  it('documents every group the state machine actually implements', () => {
    const groups = new Set(SHORTCUTS.map((s) => s.group));
    for (const group of ['Searching', 'The order', 'Processing orders']) {
      expect(groups.has(group), `nothing documented for "${group}"`).toBe(true);
    }
    expect(SHORTCUTS.length).toBeGreaterThan(12);
  });
});

describe('the whole counter flow, by keyboard alone', () => {
  it('item, how many, ticket, then the bill from the processing orders', () => {
    let state = initial();
    let commands: Command[];

    // 1. Type, choose, say how many.
    [state] = run(state, type('dos'), suggest(item('itm_dosa', 'Masala Dosa')), press('Enter'));
    [state, commands] = run(state, { kind: 'typed-qty', text: '2' }, press('Enter'));
    expect(commands[0]).toEqual({ do: 'add-item', itemId: 'itm_dosa', qty: '2' });

    // 2. Enter on the empty box: the kitchen ticket. The screen then clears the cart.
    [state] = run(state, cart(true, false));
    [state, commands] = run(state, press('Enter'));
    expect(commands).toEqual([{ do: 'print-kitchen' }]);
    [state] = run(state, cart(false), cooking(table('Parcel', true)));

    // 3. Later: down into the processing orders, Enter to open, Enter to bill.
    [state, commands] = run(state, press('ArrowDown'), press('Enter'));
    expect(commands[0]).toEqual({ do: 'open-table', tableId: 'tbl_Parcel' });
    // The floor comes back saying that order is the one in the cart.
    [, commands] = run(
      state,
      cooking({ ...table('Parcel', true), selected: true }),
      cart(true, true),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'complete-bill' }]);
  });
});
