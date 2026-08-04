/**
 * **T1 — the keyboard state machine. The most important test file in the UI.**
 *
 * > Crown jewel 1: *"the billing keyboard flow… **this is why your counter is
 * > fast.** Copy it line by line and test it line by line."*
 *
 * No DOM, no IPC, no React. A reducer, a table of events, and assertions on the
 * state and on the commands it asked for — which is the whole reason
 * `keyboard.ts` was written as a pure function. v1's focus bugs were
 * unreasonable-about precisely because there was nothing like this file.
 */

import { describe, expect, it } from 'vitest';

import {
  MAX_SUGGESTIONS,
  ORDER_TYPES,
  SHORTCUTS,
  SUB_TABLE_LETTERS,
  initial,
  reduce,
  type Command,
  type Event,
  type State,
} from '../src/billing/keyboard';
import type { MenuItemView } from '../src/ipc/generated/MenuItemView';
import type { TableView } from '../src/ipc/generated/TableView';

// --- fixtures ---------------------------------------------------------------

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
    seats: 4n,
    state: busy ? 'occupied' : 'free',
    total: busy ? { paise: 64_600n, text: '646.00' } : null,
    minutes: busy ? 12n : null,
    kitchenTold: true,
    orderId: busy ? `ord_${label}` : null,
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
const cart = (hasItems: boolean, kitchenUpToDate = true): Event => ({
  kind: 'cart',
  hasItems,
  kitchenUpToDate,
});

// ---------------------------------------------------------------------------

describe('searching', () => {
  it('typing searches, and the first result is highlighted', () => {
    // Highlighted immediately, so Enter is always one keystroke away — that
    // is what makes "name, Enter, Enter" the whole interaction.
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
    // Not a performance limit: a list you can choose from without reading is
    // faster than a list that is complete.
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

  it('Enter on a suggestion opens the quantity popup', () => {
    const [state] = run(
      initial(),
      type('dos'),
      suggest(item('itm_dosa', 'Masala Dosa')),
      press('Enter'),
    );
    expect(state.mode.kind).toBe('quantity');
  });
});

describe('the quantity popup', () => {
  it('adds the item, and a blank quantity means one', () => {
    const [state, commands] = run(
      initial(),
      type('dos'),
      suggest(item('itm_dosa', 'Masala Dosa')),
      press('Enter'),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'add-item', itemId: 'itm_dosa', qty: '1' }]);
    // And the box is cleared, ready for the next item — no keystroke wasted.
    expect(state.text).toBe('');
    expect(state.mode.kind).toBe('searching');
  });

  it('takes a typed quantity, including a fractional one', () => {
    // Scope 1.10: 0.5 kg of sweets is a real sale.
    const [, commands] = run(
      initial(),
      type('kaju'),
      suggest(item('itm_sweet', 'Kaju Katli')),
      press('Enter'),
      type('0.5'),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'add-item', itemId: 'itm_sweet', qty: '0.5' }]);
  });

  it('Esc cancels without adding anything', () => {
    const [state, commands] = run(
      initial(),
      type('dos'),
      suggest(item('itm_dosa', 'Masala Dosa')),
      press('Enter'),
      press('Escape'),
    );
    expect(state.mode.kind).toBe('searching');
    expect(commands).toEqual([]);
  });

  it('swallows everything else while it is open', () => {
    // The popup owns the keyboard; an arrow key must not move a suggestion
    // behind it.
    const [state] = run(
      initial(),
      type('dos'),
      suggest(item('itm_dosa', 'Masala Dosa')),
      press('Enter'),
      press('ArrowDown'),
    );
    expect(state.mode.kind).toBe('quantity');
    expect(state.highlighted).toBe(0);
  });
});

describe('T2 — Enter on an empty box, all four cases (audit 2.3)', () => {
  it('prints the kitchen ticket when the kitchen has not seen everything', () => {
    const [, commands] = run(initial(), cart(true, false), press('Enter'));
    expect(commands).toEqual([{ do: 'print-kitchen' }]);
  });

  it('completes the bill once the kitchen is up to date', () => {
    const [, commands] = run(initial(), cart(true, true), press('Enter'));
    expect(commands).toEqual([{ do: 'complete-bill' }]);
  });

  it('opens the first running order when the cart is empty', () => {
    // The case that matters more than it looks: it is how a cashier gets back
    // to work without touching the mouse.
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

describe('T3 — a table name loads its order; anything else falls through', () => {
  it('opens the table when the text names one', () => {
    const [state, commands] = run(
      initial(),
      floor(table('6', true)),
      type('6'),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_6' }]);
    expect(state.text).toBe('');
  });

  it('falls through to item search when it does not', () => {
    const [, commands] = run(
      initial(),
      floor(table('6', true)),
      type('dosa'),
      press('Enter'),
    );
    expect(commands).toEqual([]);
  });

  it('matches a table EXACTLY, so "1" is not table 12', () => {
    // A prefix match here would make "1" ambiguous on a busy floor, and the
    // cashier who typed it meant table one.
    const [, commands] = run(
      initial(),
      floor(table('1'), table('12', true)),
      type('1'),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_1' }]);
  });
});

describe('the order type, and the lock (crown jewel 1)', () => {
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
    // A parcel counter should not be re-selecting the type forty times an
    // hour, and should not lose it to a stray arrow key either.
    const [state, commands] = run(
      initial(),
      { kind: 'toggle-lock' },
      press('ArrowRight'),
    );
    expect(state.orderType).toBe('Dine in');
    expect(commands).toEqual([]);
  });

  it('arrows belong to the suggestions when there are any', () => {
    // Correction (a): the surface is decided by what is on screen, not by
    // where a caret happens to be.
    const [state] = run(
      initial(),
      type('a'),
      suggest(item('a', 'A'), item('b', 'B')),
      press('ArrowRight'),
    );
    expect(state.orderType).toBe('Dine in');
  });
});

describe('Esc unwinds one layer at a time (correction (b))', () => {
  it('clears the search first', () => {
    const [state, commands] = run(
      initial(),
      type('dos'),
      suggest(item('itm_dosa', 'Masala Dosa')),
      press('Escape'),
    );
    expect(state.text).toBe('');
    expect(state.suggestions).toHaveLength(0);
    expect(commands).toEqual([{ do: 'search', text: '' }]);
  });

  it('starts a new order once there is nothing left to clear', () => {
    const [, commands] = run(initial(), cart(false), press('Escape'));
    expect(commands).toEqual([{ do: 'new-order' }]);
  });

  it('ASKS before throwing away a cart with something in it', () => {
    // Esc never destroys work silently.
    const [, commands] = run(initial(), cart(true), press('Escape'));
    expect(commands).toEqual([{ do: 'confirm-new-order' }]);
  });
});

describe('T5 — the grid is reachable and nothing is a dead end', () => {
  const many = Array.from({ length: 22 }, (_, n) => table(`${n + 1}`));

  it('Down on an empty box enters the grid', () => {
    const [state] = run(initial(), floor(...many), press('ArrowDown'));
    expect(state.mode).toEqual({ kind: 'grid', index: 0 });
  });

  it('Esc comes back to the search box', () => {
    const [state, commands] = run(
      initial(),
      floor(...many),
      press('ArrowDown'),
      press('Escape'),
    );
    expect(state.mode.kind).toBe('searching');
    expect(commands).toEqual([{ do: 'focus-search' }]);
  });

  it('reaches EVERY tile, across sections and into "No table"', () => {
    // Walked rather than reasoned about: if any tile is unreachable, a
    // cashier cannot get to that table without the mouse.
    const mixed = [
      ...many,
      { ...table('Parcel', true), section: null, id: 'ord_p' },
    ];
    let [state] = run(initial(), floor(...mixed), press('ArrowDown'));

    const seen = new Set<number>();
    for (let step = 0; step < mixed.length * 4; step += 1) {
      if (state.mode.kind !== 'grid') break;
      seen.add(state.mode.index);
      [state] = run(state, press('ArrowRight'));
    }
    expect(seen.size).toBe(mixed.length);
  });

  it('Enter on a free tile opens it', () => {
    const [, commands] = run(
      initial(),
      floor(table('1')),
      press('ArrowDown'),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_1' }]);
  });
});

describe('T6 — a busy table offers merge or a sub-table letter (scope 1.6)', () => {
  it('asks rather than silently merging', () => {
    const [state] = run(
      initial(),
      cart(true),
      floor(table('6', true)),
      press('ArrowDown'),
      press('Enter'),
    );
    expect(state.mode.kind).toBe('table-busy');
  });

  it('merge is the first choice, because it is the common one', () => {
    const [, commands] = run(
      initial(),
      cart(true),
      floor(table('6', true)),
      press('ArrowDown'),
      press('Enter'),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'merge-into', tableId: 'tbl_6' }]);
  });

  it('the letters B to H open a second party on the same table', () => {
    // Crown jewel 3: "this solves a real problem — two parties on one table —
    // that most POS systems handle badly."
    const [, commands] = run(
      initial(),
      cart(true),
      floor(table('6', true)),
      press('ArrowDown'),
      press('Enter'),
      press('ArrowRight'),
      press('Enter'),
    );
    expect(commands).toEqual([
      { do: 'sub-table', tableId: 'tbl_6', letter: SUB_TABLE_LETTERS[0] },
    ]);
  });

  it('Esc backs out and changes nothing', () => {
    const [state, commands] = run(
      initial(),
      cart(true),
      floor(table('6', true)),
      press('ArrowDown'),
      press('Enter'),
      press('Escape'),
    );
    expect(state.mode.kind).toBe('searching');
    expect(commands).toEqual([]);
  });

  it('does not ask when the cart is empty — it just opens the order', () => {
    const [, commands] = run(
      initial(),
      cart(false),
      floor(table('6', true)),
      press('ArrowDown'),
      press('Enter'),
    );
    expect(commands).toEqual([{ do: 'open-table', tableId: 'tbl_6' }]);
  });
});

describe('T4 — focus is never stolen (v1 stole it from custom controls)', () => {
  it('a click on nothing returns the caret to the search box', () => {
    const [, commands] = run(initial(), {
      kind: 'click-empty',
      textSelected: false,
      controlFocused: false,
    });
    expect(commands).toEqual([{ do: 'focus-search' }]);
  });

  it('but NOT while text is selected', () => {
    const [, commands] = run(initial(), {
      kind: 'click-empty',
      textSelected: true,
      controlFocused: false,
    });
    expect(commands).toEqual([]);
  });

  it('and NOT while a control has focus', () => {
    const [, commands] = run(initial(), {
      kind: 'click-empty',
      textSelected: false,
      controlFocused: true,
    });
    expect(commands).toEqual([]);
  });
});

describe('T10 — touch reaches everything the keyboard does (scope 1.28)', () => {
  it('tapping a suggestion opens the same popup Enter does', () => {
    const [byKey] = run(
      initial(),
      type('dos'),
      suggest(item('itm_dosa', 'Masala Dosa')),
      press('Enter'),
    );
    const [byTap] = run(
      initial(),
      type('dos'),
      suggest(item('itm_dosa', 'Masala Dosa')),
      { kind: 'tap-suggestion', index: 0 },
    );
    expect(byTap.mode).toEqual(byKey.mode);
  });

  it('tapping a busy tile asks the same question arrowing to it does', () => {
    const [byKey] = run(
      initial(),
      cart(true),
      floor(table('6', true)),
      press('ArrowDown'),
      press('Enter'),
    );
    const [byTap] = run(initial(), cart(true), floor(table('6', true)), {
      kind: 'tap-tile',
      index: 0,
    });
    expect(byTap.mode.kind).toBe(byKey.mode.kind);
  });
});

describe('the help sheet (audit F4)', () => {
  it('opens on "?" and closes on Esc', () => {
    let [state] = run(initial(), press('?'));
    expect(state.mode.kind).toBe('help');
    [state] = run(state, press('Escape'));
    expect(state.mode.kind).toBe('searching');
  });

  it('does not open while somebody is typing a search', () => {
    // "?" is a character in a search box before it is a shortcut.
    const [state] = run(initial(), type('idl'), press('?'));
    expect(state.mode.kind).toBe('searching');
  });

  it('documents every group the state machine actually implements', () => {
    // The sheet is generated from this table, so an undocumented key is
    // impossible rather than unlikely.
    const groups = new Set(SHORTCUTS.map((s) => s.group));
    for (const group of ['Searching', 'Quantity', 'The order', 'The floor']) {
      expect(groups.has(group), `nothing documented for "${group}"`).toBe(true);
    }
    expect(SHORTCUTS.length).toBeGreaterThan(12);
  });
});

describe('the whole thing, end to end, by keyboard alone', () => {
  it('types an item, adds it, and completes the bill without a mouse', () => {
    // The acceptance test in miniature: if a cashier who used v1 has to think
    // about any of this, the rebuild has failed.
    let state = initial();
    let commands: Command[];

    [state] = run(state, type('dos'), suggest(item('itm_dosa', 'Masala Dosa')));
    [state] = run(state, press('Enter'));       // quantity popup
    [state, commands] = run(state, press('Enter')); // blank means one
    expect(commands).toEqual([{ do: 'add-item', itemId: 'itm_dosa', qty: '1' }]);

    [state] = run(state, cart(true, false));
    [state, commands] = run(state, press('Enter'));  // kitchen ticket
    expect(commands).toEqual([{ do: 'print-kitchen' }]);

    [state] = run(state, cart(true, true));
    [, commands] = run(state, press('Enter'));       // and then the bill
    expect(commands).toEqual([{ do: 'complete-bill' }]);
  });
});
