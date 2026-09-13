import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { CartView } from '../src/ipc/generated/CartView';
import type { MoneyView } from '../src/ipc/generated/MoneyView';
import type { TableView } from '../src/ipc/generated/TableView';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  isUiError: () => false,
  subscribe: () => Promise.resolve(() => undefined),
}));

const { MergeBill, mergeCandidates } = await import('../src/billing/MergeBill');
const { SplitBill } = await import('../src/billing/SplitBill');

afterEach(() => {
  cleanup();
  call.mockReset();
});

function money(paise: number, text: string): MoneyView {
  return { paise: BigInt(paise), text };
}

function table(over: Partial<TableView> & Pick<TableView, 'id' | 'label'>): TableView {
  return {
    section: 'Main Hall',
    seats: 4,
    state: 'occupied',
    total: null,
    minutes: null,
    kitchenTold: true,
    kitchenMinutes: null,
    billAsked: false,
    settleAsked: false,
    by: null,
    byId: null,
    orderId: null,
    token: null,
    billNumber: null,
    selected: false,
    ...over,
  };
}

/** A parked dine-in order on table 4 with two lines. */
function cart(over: Partial<CartView> = {}): CartView {
  const zero = money(0, '0.00');
  return {
    lines: [
      {
        index: 0,
        name: 'Masala Dosa',
        note: null,
        qty: '2',
        rateLabel: '5%',
        unitPrice: money(8_000, '80.00'),
        gross: money(16_000, '160.00'),
        discount: zero,
        lineDiscount: zero,
        amount: money(16_000, '160.00'),
        modifiers: [],
      },
      {
        index: 1,
        name: 'Filter Coffee',
        note: null,
        qty: '1',
        rateLabel: '5%',
        unitPrice: money(3_000, '30.00'),
        gross: money(3_000, '30.00'),
        discount: zero,
        lineDiscount: zero,
        amount: money(3_000, '30.00'),
        modifiers: [],
      },
    ],
    bill: {
      subtotal: money(19_000, '190.00'),
      lineDiscount: zero,
      billDiscount: zero,
      totalDiscount: zero,
      discountCapped: false,
      charges: [],
      taxRows: [],
      taxTotal: zero,
      nonGstValue: zero,
      exemptValue: zero,
      roundOff: zero,
      grandTotal: money(19_000, '190.00'),
    },
    orderType: 'dine_in',
    table: '4',
    payments: [],
    paid: zero,
    balance: money(19_000, '190.00'),
    change: zero,
    isEmpty: false,
    kitchenUpToDate: true,
    kitchenTold: true,
    covers: null,
    orderId: 'ord_four',
    token: null,
    billNumber: null,
    fromTheFloor: [],
    lengthSays: '',
    orderTypeLocked: true,
    kitchenTicketOff: false,
    tablesOnCounter: true,
    ...over,
  };
}

describe('merge bill', () => {
  it('offers every other open order, never this one and never a free table', () => {
    const others = mergeCandidates(
      [
        table({ id: '4', label: '4', orderId: 'ord_four' }),
        table({ id: '5', label: '5', orderId: 'ord_five' }),
        table({ id: '6', label: '6', state: 'free' }),
        table({ id: 'parcel', label: 'Parcel 3', section: null, orderId: 'ord_parcel' }),
      ],
      'ord_four',
    );
    expect(others.map((t) => t.orderId)).toEqual(['ord_five', 'ord_parcel']);
  });

  it('merges the picked order INTO the one on the counter, and says who joined', async () => {
    call.mockResolvedValue({});
    const onMerged = vi.fn();
    render(
      <MergeBill
        cart={cart()}
        orders={[
          table({ id: '4', label: '4', orderId: 'ord_four' }),
          table({
            id: '5',
            label: '5',
            orderId: 'ord_five',
            total: money(32_000, '320.00'),
            billNumber: 'A/0012',
          }),
        ]}
        onClose={vi.fn()}
        onMerged={onMerged}
        onFailed={vi.fn()}
      />,
    );

    expect(screen.getByText('Table 5')).toBeInTheDocument();
    expect(screen.getByText('320.00')).toBeInTheDocument();
    expect(screen.queryByText('Table 4')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Merge' }));
    await waitFor(() => expect(onMerged).toHaveBeenCalled());
    // The counter's order survives; the picked one is absorbed.
    expect(call).toHaveBeenCalledWith('merge_orders', {
      fromOrder: 'ord_five',
      intoOrder: 'ord_four',
    });
    expect(onMerged).toHaveBeenCalledWith('Table 5 joined this bill.');
  });

  it('says so when nothing else is open, and when this order is not on disk yet', () => {
    const { unmount } = render(
      <MergeBill
        cart={cart()}
        orders={[table({ id: '4', label: '4', orderId: 'ord_four' })]}
        onClose={vi.fn()}
        onMerged={vi.fn()}
        onFailed={vi.fn()}
      />,
    );
    expect(screen.getByText('Nothing to merge')).toBeInTheDocument();
    unmount();

    render(
      <MergeBill
        cart={cart({ orderId: null })}
        orders={[table({ id: '5', label: '5', orderId: 'ord_five' })]}
        onClose={vi.fn()}
        onMerged={vi.fn()}
        onFailed={vi.fn()}
      />,
    );
    expect(screen.getByText(/Print the kitchen ticket first/)).toBeInTheDocument();
  });
});

describe('split bill', () => {
  const plan = {
    tiles: [],
    sections: [],
    tables: [
      { id: 't4', label: '4', printed: 'Table 4', sectionId: null, seats: 4, x: null, y: null, isActive: true, isBusy: true, history: 1 },
      { id: 't7', label: '7', printed: 'Table 7', sectionId: null, seats: 4, x: null, y: null, isActive: true, isBusy: false, history: 0 },
      { id: 't9', label: '9', printed: 'Table 9', sectionId: null, seats: 4, x: null, y: null, isActive: false, isBusy: false, history: 0 },
    ],
    occupancy: { busy: '0', covers: '0', turns: '0', average: '0' },
    grid: 10,
    warnMinutes: 15,
    lateMinutes: 30,
    hasLayout: false,
    canArrange: false,
    tablesOnCounter: true,
  };

  it('moves the typed quantities to a new bill on the same table by default', async () => {
    call.mockImplementation((name: string) =>
      Promise.resolve(name === 'floor_plan' ? plan : {}),
    );
    const onSplit = vi.fn();
    render(
      <SplitBill cart={cart()} onClose={vi.fn()} onSplit={onSplit} onFailed={vi.fn()} />,
    );

    // Only the free, active table is offered; the busy one and the hidden one are not.
    const where = await screen.findByLabelText('New bill goes to');
    const offered = Array.from((where as HTMLSelectElement).options).map((o) => o.textContent);
    expect(offered).toEqual(['This table (4)', 'Table 7']);

    const split = screen.getByRole('button', { name: 'Split' });
    expect(split).toBeDisabled();
    fireEvent.change(screen.getAllByLabelText('Move')[0]!, { target: { value: '1' } });
    expect(split).toBeEnabled();
    fireEvent.click(split);

    await waitFor(() => expect(onSplit).toHaveBeenCalled());
    expect(call).toHaveBeenCalledWith('split_order', {
      request: { orderId: 'ord_four', lines: [[0, '1']], toTable: null, seat: null },
    });
    expect(onSplit.mock.calls[0]?.[0]).toMatch(/two bills on that table/);
  });

  it('sends the new bill to the table picked', async () => {
    call.mockImplementation((name: string) =>
      Promise.resolve(name === 'floor_plan' ? plan : {}),
    );
    const onSplit = vi.fn();
    render(
      <SplitBill cart={cart()} onClose={vi.fn()} onSplit={onSplit} onFailed={vi.fn()} />,
    );
    const where = await screen.findByLabelText('New bill goes to');
    fireEvent.change(where, { target: { value: 't7' } });
    fireEvent.change(screen.getAllByLabelText('Move')[1]!, { target: { value: '1' } });
    fireEvent.click(screen.getByRole('button', { name: 'Split' }));

    await waitFor(() => expect(onSplit).toHaveBeenCalled());
    expect(call).toHaveBeenCalledWith('split_order', {
      request: { orderId: 'ord_four', lines: [[1, '1']], toTable: 't7', seat: null },
    });
    expect(onSplit.mock.calls[0]?.[0]).toMatch(/on Table 7/);
  });
});
