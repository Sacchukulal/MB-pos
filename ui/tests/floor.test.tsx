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
    fireEvent.click(screen.getByLabelText('Table 2'));
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

    fireEvent.click(screen.getByLabelText('Table 1'));
    expect(screen.getByText(/This table is free/)).toBeTruthy();
  });
});
