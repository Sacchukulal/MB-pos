/**
 * **The floor screen** — scope 14.1, 14.2, 14.3.
 *
 * Rust proves the rules (`floor_tests.rs` drives the commands end to end);
 * this proves what the screen does with them:
 *
 * 1. the **fifth** tile state is told apart from the other four WITHOUT
 *    colour — P14 split "waiting too long" into amber and red, and a state
 *    that only differs by hue fails §2 rule 2;
 * 2. a shop with no floor plan gets the section grid and everything works —
 *    the fallback is not a degraded mode;
 * 3. "needs attention" actually filters (audit F5);
 * 4. a move sends the table the screen showed, not the one it guessed.
 */

import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  isUiError: () => false,
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
    orderId: null,
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
    // The default is an owner, because the interesting cases are about the
    // arranging panel. A waiter is its own test.
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
    await screen.findByText('3 of 4 tables busy');

    for (const state of ['free', 'occupied', 'waiting', 'late']) {
      expect(
        container.querySelector(`.mb-tile--${state}`),
        `the ${state} state has no form of its own`,
      ).toBeTruthy();
    }
  });

  it('shows the occupancy line as sentences Rust wrote', async () => {
    call.mockResolvedValue(floor());
    show();
    // Not "3", "4", "7" for the screen to assemble — R8 applies to words too.
    expect(await screen.findByText('3 of 4 tables busy')).toBeTruthy();
    expect(screen.getByText('38 min at table')).toBeTruthy();
  });

  it('draws the section grid when no table has been placed', async () => {
    call.mockResolvedValue(floor());
    const { container } = show();
    await screen.findByText('3 of 4 tables busy');

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
    await screen.findByText('3 of 4 tables busy');

    const plan = container.querySelector('.mb-plan');
    expect(plan).toBeTruthy();
    // 16 x 16 squares, from the grid size RUST sent — not a number the screen
    // decided for itself.
    expect(plan?.querySelectorAll('.mb-plan__cell')).toHaveLength(256);
  });

  /** Audit F5: "with 20 tables open it becomes a scrolling exercise." */
  it('filters to the tables that need somebody', async () => {
    call.mockResolvedValue(floor());
    const { container } = show();
    await screen.findByText('3 of 4 tables busy');

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
    await screen.findByText('3 of 4 tables busy');

    // Table 2 is busy; tick it and move its order to table 1, which the view
    // says is free.
    //
    // **Tick, then act.** Pressing a tile used to open the order dialog
    // straight away. Since the room is arranged on this screen (2026-08-22) a
    // press ticks the table, and everything you can do to what you ticked is on
    // the bar — which also means the screen names the table before it offers to
    // move it.
    //
    // A prefix match, because the tile announces its state too: this screen had
    // its own copy of the table tile until 2026-08-17, whose whole aria-label
    // was "Table 2". It uses the shared one now, which says "Table 2, Hall,
    // busy, 646.00, 12m" so a blind cashier hears whether the table is free.
    fireEvent.click(screen.getByLabelText(/^Table 2\b/));
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
    await screen.findByText('3 of 4 tables busy');

    fireEvent.click(screen.getByLabelText(/^Table 1\b/));
    // It ticks, like any other table — free tables are the ones you delete.
    expect(await screen.findByText('1 ticked')).toBeTruthy();
    // But there is no order on it, so nothing about an order is offered ON THE
    // BAR. Scoped to the bar deliberately: table 2 is busy and still wears its
    // own print mark, which is a fact about that tile and not about this tick.
    const bar = screen.getByRole('group', { name: 'What to do with the ticked tables' });
    expect(within(bar).queryByRole('button', { name: 'Move or merge' })).toBeNull();
    expect(within(bar).queryByRole('button', { name: /Print the bill/ })).toBeNull();
  });
});

/**
 * **The room is arranged on the screen, not in a dialog** — the owner,
 * 2026-08-22.
 *
 * > *"No need for popup for setup room. Redesign the Floor section page to have
 * > a adding tables section in one side (at the starting side of the screen)…
 * > no need to show table list as it will already be visible in the screen in
 * > proper square format… make the tables selectable… and then i should be able
 * > to delete them."*
 */
describe('arranging the room (2026-08-22)', () => {
  it('puts the panel on the screen and no dialog behind a button', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('3 of 4 tables busy');

    expect(screen.getByRole('complementary', { name: 'Arrange the room' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Set up the room' })).toBeNull();
  });

  it('hides the panel from somebody who may not arrange the room', async () => {
    // A courtesy, not the control — guard::require is what refuses, and
    // FloorView::can_arrange is that same question asked once.
    call.mockResolvedValue(floor({ canArrange: false }));
    show();
    await screen.findByText('3 of 4 tables busy');

    expect(screen.queryByRole('complementary', { name: 'Arrange the room' })).toBeNull();
    // And a press does what it always did on a screen you cannot arrange.
    fireEvent.click(screen.getByLabelText(/^Table 2\b/));
    expect(await screen.findByText(/Move this order/)).toBeTruthy();
  });

  it('ticks one at a time, and all of them at once', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('3 of 4 tables busy');

    fireEvent.click(screen.getByLabelText(/^Table 1\b/));
    expect(await screen.findByText('1 ticked')).toBeTruthy();
    fireEvent.click(screen.getByLabelText(/^Table 2\b/));
    expect(await screen.findByText('2 ticked')).toBeTruthy();

    // Pressing a ticked table again unticks it.
    fireEvent.click(screen.getByLabelText(/^Table 2\b/));
    expect(await screen.findByText('1 ticked')).toBeTruthy();

    // **"Section wise" — the room segment narrows what is shown, and this ticks
    // all of what a tick can mean anything about.**
    //
    // Two of the four tiles in this fixture are tables; the other two are open
    // orders with no table behind them (§4, "so no order is ever invisible").
    // You cannot delete or hide one of those, so it cannot be ticked — and the
    // count says two, not four. Getting that wrong made the bar say "2 ticked"
    // after ticking four things, which is a screen contradicting itself.
    fireEvent.click(screen.getByLabelText(/^Tick all 2/));
    expect(await screen.findByText('2 ticked')).toBeTruthy();
  });

  /** **The point of ticking.** */
  it('deletes everything ticked in ONE command', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('3 of 4 tables busy');

    fireEvent.click(screen.getByLabelText(/^Table 1\b/));
    fireEvent.click(screen.getByLabelText(/^Table 2\b/));
    // **Scoped to the bar.** Each tile wears a bin of its own now ("Delete
    // table 1"), so a loose /^Delete/ finds three buttons meaning two different
    // things — one table, or everything ticked.
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
    await screen.findByText('3 of 4 tables busy');

    fireEvent.change(screen.getByLabelText('From'), { target: { value: '5' } });
    fireEvent.change(screen.getByLabelText('To'), { target: { value: '8' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add them' }));

    const sent = call.mock.calls.find((c) => c[0] === 'add_dining_tables');
    expect(sent?.[1]).toMatchObject({ from: 5, to: 8, seats: 4 });
  });

  /**
   * **The explanations are asked for, not given** — the owner, same day:
   * *"you are adding these explaination texts everywhere in the app, it is not
   * needed, it makes the app look cluttered and un professional."*
   *
   * The words are not deleted. They move into a tip, so the screen is quiet and
   * the answer is still one hover away.
   */
  it('keeps the timer explanation in a tip rather than on the screen', async () => {
    call.mockResolvedValue(floor());
    const { container } = show();
    await screen.findByText('3 of 4 tables busy');

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
 * **A shop with one room has no room picker, and a shop with none has no
 * tables screen worth the name** — P30.5.
 *
 * The default fixture has ONE section, and until P30.5 that drew a segmented
 * control containing the single word "All": a tall box in the corner of the
 * screen offering a choice between one thing. "All" and "Hall" are the same
 * set of tables under two names.
 *
 * And on a shop with no tables at all — a tea stall, a bakery, a parcel
 * counter — the screen said "Nothing here · Try another section, or show
 * everything", which is advice nobody can take.
 */
describe('empty is not a lecture (P30.5)', () => {
  it('hides the room picker until there is more than one room', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('3 of 4 tables busy');
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
    await screen.findByText('3 of 4 tables busy');
    expect(screen.getByRole('group', { name: 'Which room' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'AC room' })).toBeTruthy();
  });

  it('tells a shop with no tables what to do, not to try another section', async () => {
    call.mockResolvedValue(floor({ tiles: [], tables: [], sections: [] }));
    show();
    expect(await screen.findByText('No tables yet')).toBeTruthy();
    expect(screen.queryByText(/Try another section/)).toBeNull();
    // **The way to fix it is now beside the words rather than behind a button.**
    // It used to say "Set up the room" twice — a toolbar button and an empty
    // state — and both opened the same dialog. The panel is on the screen, so
    // the empty state points at it.
    expect(screen.getByRole('complementary', { name: 'Arrange the room' })).toBeTruthy();
    expect(screen.getByText(/Add a room and a run of tables on the left/)).toBeTruthy();
  });

  /**
   * **The Floor screen draws the SAME tile as the billing screen, from the
   * same file** — the owner, 2026-08-17:
   *
   * > *"billing screen table structure and in floor page tables cards
   * > structure completely looks different, instead use same in both, bcz the
   * > users are keeps open that floor page for billing also, so print button
   * > inside table cards also wil appeare here also, so make sure both are
   * > same source file."*
   *
   * This screen had its own copy of the tile until then — same class names,
   * different markup — so the two never quite matched and a fix to one silently
   * broke the other. The print mark is the visible proof that the shared one is
   * what is on screen: the copy never had it, and could not have grown it
   * without being edited a second time.
   *
   * `check-layout.mjs` guards the same claim from the other side: it fails the
   * build if any file but `billing/TableGrid.tsx` writes an `mb-tile` class.
   */
  it('offers the same print-the-bill mark the billing screen has', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('3 of 4 tables busy');

    // Table 2 is busy, so it can be printed. Table 1 is free, so it cannot —
    // a print button whose only possible outcome is an error message is worse
    // than no button.
    const print = screen.getByRole('button', { name: 'Print the bill for table 2' });
    expect(screen.queryByRole('button', { name: 'Print the bill for table 1' })).toBeNull();

    call.mockClear();
    call.mockResolvedValue('The bill for table 2 is printing.');
    fireEvent.click(print);

    expect(call).toHaveBeenCalledWith('print_open_bill', { orderId: 'ord_2' });
  });
});
