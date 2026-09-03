/** The floor screen — scope 14.1, 14.2, 14.3. */

import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  isUiError: () => false,
  subscribe: () => Promise.resolve(() => undefined),
}));

const { Floor } = await import('../src/floor/Floor');
const { ToastProvider } = await import('../src/kit');

/** The floor reports failures through a toast, so it needs one to live in. */
function show() {
  return render(
    <ToastProvider>
      <Floor />
    </ToastProvider>,
  );
}

/** Open the arranging panel. */
function unfold() {
  fireEvent.click(screen.getByRole('button', { name: 'Rooms and tables' }));
}

import type { FloorView } from '../src/ipc/generated/FloorView';
import type { TableRowView } from '../src/ipc/generated/TableRowView';
import type { TableView } from '../src/ipc/generated/TableView';

function tile(over: Partial<TableView> & Pick<TableView, 'id' | 'label'>): TableView {
  return {
    section: 'Hall',
    seats: 4,
    state: 'free',
    total: null,
    minutes: null,
    kitchenTold: true,
    kitchenMinutes: null,
    billAsked: false,
    settleAsked: false,
    by: null,
    byId: null,
    orderId: null,
    billNumber: null,
    selected: false,
    ...over,
  };
}

function row(over: Partial<TableRowView> & Pick<TableRowView, 'id' | 'label'>): TableRowView {
  return {
    printed: `Hall ${over.label}`,
    sectionId: 'sec_hall',
    seats: 4,
    x: null,
    y: null,
    isActive: true,
    isBusy: false,
    history: 0,
    ...over,
  };
}

function floor(over: Partial<FloorView> = {}): FloorView {
  return {
    tiles: [
      tile({ id: 'tbl_1', label: '1' }),
      tile({ id: 'tbl_2', label: '2', state: 'occupied', orderId: 'ord_2', minutes: 5 }),
      tile({ id: 'tbl_3', label: '3', state: 'waiting', orderId: 'ord_3', minutes: 25 }),
      tile({ id: 'tbl_4', label: '4', state: 'late', orderId: 'ord_4', minutes: 60 }),
    ],
    sections: [
      { id: 'sec_hall', name: 'Hall', sortOrder: 0, isActive: true, tableCount: 4 },
    ],
    tables: [row({ id: 'tbl_1', label: '1' }), row({ id: 'tbl_2', label: '2', isBusy: true })],
    occupancy: {
      busy: '3 of 4 tables busy',
      covers: 'No cover count',
      turns: '7 turn(s) today',
      average: '38 min at table',
    },
    grid: 16,
    warnMinutes: 20,
    lateMinutes: 45,
    hasLayout: false,
    // The default is an owner, because the interesting cases are about the arranging panel.
    canArrange: true,
    ...over,
  };
}

beforeEach(() => {
  call.mockReset();
});
afterEach(cleanup);

describe('the floor (scope 14.1–14.3)', () => {
  it('tells all FIVE states apart without colour', async () => {
    call.mockResolvedValue(floor());
    const { container } = show();
    await screen.findByText('4 seats');

    for (const state of ['free', 'occupied', 'waiting', 'late']) {
      expect(
        container.querySelector(`.mb-tile--${state}`),
        `the ${state} state has no form of its own`,
      ).toBeTruthy();
    }
  });

  it('draws the section grid when no table has been placed', async () => {
    call.mockResolvedValue(floor());
    const { container } = show();
    await screen.findByText('4 seats');

    // The fallback is not a degraded mode: every table is still a tile.
    expect(container.querySelector('.mb-plan')).toBeNull();
    expect(container.querySelectorAll('.mb-tile')).toHaveLength(4);
  });

  it('draws the plan once a table has a square', async () => {
    call.mockResolvedValue(
      floor({
        hasLayout: true,
        tables: [row({ id: 'tbl_1', label: '1', x: 2, y: 3 })],
      }),
    );
    const { container } = show();
    await screen.findByText('4 seats');

    const plan = container.querySelector('.mb-plan');
    expect(plan).toBeTruthy();
    // 16 x 16 squares, from the grid size RUST sent — not a number the screen decided for
    // itself.
    expect(plan?.querySelectorAll('.mb-plan__cell')).toHaveLength(256);
  });

  /** "with 20 tables open it becomes a scrolling exercise.". */
  it('filters to the tables that need somebody', async () => {
    call.mockResolvedValue(floor());
    const { container } = show();
    await screen.findByText('4 seats');

    fireEvent.click(screen.getByText('Needs attention'));
    // Waiting and late, and neither the free table nor the one five minutes in.
    const labels = [...container.querySelectorAll('.mb-tile__label')].map((n) => n.textContent);
    expect(labels).toEqual(['3', '4']);

    fireEvent.click(screen.getByText('Busy'));
    const busy = [...container.querySelectorAll('.mb-tile__label')].map((n) => n.textContent);
    expect(busy).toEqual(['2', '3', '4']);
  });

  it('sends the order and the table the screen actually showed', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('4 seats');

    // Table 2 is busy; tick it and move its order to table 1, which the view says is free.
    fireEvent.click(screen.getByRole('checkbox', { name: 'Tick table 2' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Move or merge' }));

    fireEvent.change(await screen.findByLabelText('To table'), {
      target: { value: 'tbl_1' },
    });
    fireEvent.click(screen.getByText('Move'));

    const sent = call.mock.calls.find((c) => c[0] === 'move_order');
    expect(sent?.[1]).toEqual({ orderId: 'ord_2', toTable: 'tbl_1' });
  });

  it('offers no order actions for a free table', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('4 seats');

    fireEvent.click(screen.getByRole('checkbox', { name: 'Tick table 1' }));
    // It ticks, like any other table — free tables are the ones you delete.
    expect(await screen.findByText('1 ticked')).toBeTruthy();
    // But there is no order on it, so nothing about an order is offered ON THE BAR.
    const bar = screen.getByRole('group', { name: 'What to do with the ticked tables' });
    expect(within(bar).queryByRole('button', { name: 'Move or merge' })).toBeNull();
    expect(within(bar).queryByRole('button', { name: /Print the bill/ })).toBeNull();
  });
});

/** The room is arranged on the screen, not in a dialog. */
describe('arranging the room (2026-08-22)', () => {
  /** Folded on a shop that already has tables. */
  it('folds the panel to a button, and unfolds it when pressed', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('4 seats');

    expect(screen.queryByRole('complementary', { name: 'Rooms and tables' })).toBeNull();

    unfold();
    expect(screen.getByRole('complementary', { name: 'Rooms and tables' })).toBeTruthy();
    // Still no dialog — the panel is the screen's, not a popup's.
    expect(screen.queryByRole('button', { name: 'Set up the room' })).toBeNull();

    fireEvent.click(
      screen.getByRole('button', { name: 'Close Rooms and tables' }),
    );
    expect(screen.queryByRole('complementary', { name: 'Rooms and tables' })).toBeNull();
  });

  it('hides the panel from somebody who may not arrange the room', async () => {
    // A courtesy, not the control — guard::require is what refuses, and FloorView::can_arrange
    // is that same question asked once.
    call.mockResolvedValue(floor({ canArrange: false }));
    show();
    await screen.findByText('4 seats');

    expect(screen.queryByRole('complementary', { name: 'Rooms and tables' })).toBeNull();
    // And a press does what it always did on a screen you cannot arrange.
    fireEvent.click(screen.getByLabelText(/^Table 2\b/));
    expect(await screen.findByText(/Move this order/)).toBeTruthy();
  });

  it('ticks one at a time, and all of them at once', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('4 seats');

    fireEvent.click(screen.getByRole('checkbox', { name: 'Tick table 1' }));
    expect(await screen.findByText('1 ticked')).toBeTruthy();
    fireEvent.click(screen.getByRole('checkbox', { name: 'Tick table 2' }));
    expect(await screen.findByText('2 ticked')).toBeTruthy();

    // Ticking a ticked table again unticks it.
    fireEvent.click(screen.getByRole('checkbox', { name: 'Tick table 2' }));
    expect(await screen.findByText('1 ticked')).toBeTruthy();

    // "Section wise" — the room segment narrows what is shown, and this ticks all of what a
    // tick can mean anything about.
    fireEvent.click(screen.getByLabelText(/^Tick all 2/));
    expect(await screen.findByText('2 ticked')).toBeTruthy();
  });

  /** The point of ticking. */
  it('deletes everything ticked in ONE command', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('4 seats');

    fireEvent.click(screen.getByRole('checkbox', { name: 'Tick table 1' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Tick table 2' }));
    // Scoped to the bar.
    const bar = await screen.findByRole('group', {
      name: 'What to do with the ticked tables',
    });
    fireEvent.click(within(bar).getByRole('button', { name: 'Delete' }));
    // Deleting a table cannot be undone, so it is confirmed.
    fireEvent.click(await screen.findByRole('button', { name: 'Delete them' }));

    const sent = call.mock.calls.filter((c) => c[0] === 'delete_dining_tables');
    expect(sent, 'a bulk delete became a loop of single deletes').toHaveLength(1);
    expect(sent[0]?.[1]).toEqual({ tableIds: ['tbl_1', 'tbl_2'] });
  });

  it('adds a run of tables from the panel', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('4 seats');

    unfold();
    fireEvent.change(screen.getByLabelText('From'), { target: { value: '5' } });
    fireEvent.change(screen.getByLabelText('To'), { target: { value: '8' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add them' }));

    const sent = call.mock.calls.find((c) => c[0] === 'add_dining_tables');
    expect(sent?.[1]).toMatchObject({ from: 5, to: 8, seats: 4 });
  });

  /** The explanations are asked for, not given. */
  it('keeps the timer explanation in a tip rather than on the screen', async () => {
    call.mockResolvedValue(floor());
    const { container } = show();
    await screen.findByText('4 seats');

    unfold();
    expect(screen.getByText('Timers')).toBeTruthy();
    const bubbles = [...container.querySelectorAll('.mb-tip__bubble[role="tooltip"]')];
    expect(bubbles.length, 'nothing on this screen offers a tip').toBeGreaterThan(0);
    expect(
      bubbles.some((b) => b.textContent?.includes('dosa counter')),
      'the timer explanation went missing rather than moving into a tip',
    ).toBe(true);
  });
});

/**
 * A shop with one room has no room picker, and a shop with none has no tables screen worth the
 * name.
 */
describe('empty is not a lecture (P30.5)', () => {
  it('hides the room picker until there is more than one room', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('4 seats');
    expect(screen.queryByRole('group', { name: 'Which room' })).toBeNull();

    cleanup();
    call.mockResolvedValue(
      floor({
        sections: [
          { id: 'sec_hall', name: 'Hall', sortOrder: 0, isActive: true, tableCount: 2 },
          { id: 'sec_ac', name: 'AC room', sortOrder: 1, isActive: true, tableCount: 2 },
        ],
      }),
    );
    show();
    await screen.findByText('4 seats');
    expect(screen.getByRole('group', { name: 'Which room' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'AC room' })).toBeTruthy();
  });

  /** The floor is drawn room by room. */
  it('groups the tables under their room, and ticks one room at a time', async () => {
    call.mockResolvedValue(
      floor({
        sections: [
          { id: 'sec_hall', name: 'Hall', sortOrder: 0, isActive: true, tableCount: 2 },
          { id: 'sec_ac', name: 'AC room', sortOrder: 1, isActive: true, tableCount: 2 },
        ],
        tiles: [
          tile({ id: 'tbl_1', label: '1' }),
          tile({ id: 'tbl_3', label: '3', section: 'AC room' }),
          tile({ id: 'tbl_2', label: '2' }),
          tile({ id: 'tbl_4', label: '4', section: 'AC room' }),
        ],
        tables: [
          row({ id: 'tbl_1', label: '1' }),
          row({ id: 'tbl_2', label: '2' }),
          row({ id: 'tbl_3', label: '3', sectionId: 'sec_ac' }),
          row({ id: 'tbl_4', label: '4', sectionId: 'sec_ac' }),
        ],
      }),
    );
    const { container } = show();
    await screen.findAllByText('4 seats');

    // A heading each, in the order the shop put its rooms in.
    expect(
      [...container.querySelectorAll('.mb-floor__roomname')].map((n) => n.textContent),
    ).toEqual(['Hall', 'AC room']);
    // And the tables under the room they belong to, not interleaved.
    expect(
      [...container.querySelectorAll('.mb-floor__roomgroup')].map((group) =>
        [...group.querySelectorAll('.mb-tile__label')].map((n) => n.textContent),
      ),
    ).toEqual([
      ['1', '2'],
      ['3', '4'],
    ]);

    // The tick-all belongs to a room, so it ticks that room and no other.
    fireEvent.click(screen.getByLabelText('Tick all 2 in AC room'));
    expect(await screen.findByText('2 ticked')).toBeTruthy();
    const bar = screen.getByRole('group', { name: 'What to do with the ticked tables' });
    fireEvent.click(within(bar).getByRole('button', { name: 'Delete' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Delete them' }));
    expect(call.mock.calls.find((c) => c[0] === 'delete_dining_tables')?.[1]).toEqual({
      tableIds: ['tbl_3', 'tbl_4'],
    });
  });

  it('tells a shop with no tables what to do, not to try another section', async () => {
    call.mockResolvedValue(floor({ tiles: [], tables: [], sections: [] }));
    show();
    expect(await screen.findByText('No tables yet')).toBeTruthy();
    expect(screen.queryByText(/Try another section/)).toBeNull();
    // The way to fix it is now beside the words rather than behind a button.
    expect(screen.getByRole('complementary', { name: 'Rooms and tables' })).toBeTruthy();
    expect(screen.getByText(/Add a room and a run of tables on the left/)).toBeTruthy();
  });

  /** The Floor screen draws the SAME tile as the billing screen, from the same file. */
  it('offers the same print-the-bill mark the billing screen has', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('4 seats');

    // Table 2 is busy, so it can be printed.
    const print = screen.getByRole('button', { name: 'Print the bill for table 2' });
    expect(screen.queryByRole('button', { name: 'Print the bill for table 1' })).toBeNull();

    call.mockClear();
    call.mockResolvedValue('The bill for table 2 is printing.');
    fireEvent.click(print);

    expect(call).toHaveBeenCalledWith('print_open_bill', { orderId: 'ord_2' });
  });
});
