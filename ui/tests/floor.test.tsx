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

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
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

    // Table 2 is busy; open it and move its order to table 1, which the view
    // says is free.
    //
    // **A prefix match, because the tile announces its state too.** This
    // screen had its own copy of the table tile until 2026-08-17, whose whole
    // aria-label was "Table 2"; it uses the shared one now, which says
    // "Table 2, Hall, busy, 646.00, 12m" so a blind cashier hears whether the
    // table is free. Matching the number exactly was matching the duplicate.
    fireEvent.click(screen.getByLabelText(/^Table 2\b/));
    fireEvent.change(await screen.findByLabelText('To table'), {
      target: { value: 'tbl_1' },
    });
    fireEvent.click(screen.getByText('Move'));

    const sent = call.mock.calls.find((c) => c[0] === 'move_order');
    expect(sent?.[1]).toEqual({ orderId: 'ord_2', toTable: 'tbl_1' });
  });

  it('offers nothing to do to a free table but says so', async () => {
    call.mockResolvedValue(floor());
    show();
    await screen.findByText('3 of 4 tables busy');

    fireEvent.click(screen.getByLabelText(/^Table 1\b/));
    expect(screen.getByText(/This table is free/)).toBeTruthy();
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
    // The way to fix it is on the screen, and there are two of it: the toolbar
    // button a busy shop uses, and this one.
    expect(screen.getAllByRole('button', { name: 'Set up the room' }).length).toBe(2);
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
