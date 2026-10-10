import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CartView } from '../src/ipc/generated/CartView';
import type { TableView } from '../src/ipc/generated/TableView';

const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock('../src/ipc/call', () => ({
  call,
  inApp: () => true,
  isUiError: (cause: unknown) => typeof cause === 'object' && cause !== null && 'code' in cause,
  subscribe: () => Promise.resolve(() => undefined),
}));
vi.mock('../src/clock', () => ({ useTick: () => 0 }));
import { Billing } from '../src/billing/Billing';
import { TableBox } from '../src/billing/Keys';
import { ToastProvider } from '../src/kit';

const table: TableView = {
  processing: false,
  id: 'table_6', label: '6', section: 'Main', sectionOrder: 0, seats: 4,
  state: 'occupied', total: null, minutes: 0, createdAt: null,
  kitchenTold: false, kitchenMinutes: null, billAsked: false, settleAsked: false,
  by: null, byId: null, orderId: 'order_6', seat: null, token: null, billNumber: null, selected: false,
};
afterEach(cleanup);
beforeEach(() => call.mockReset());

function enterTable() {
  const input = screen.getByLabelText('Table number');
  fireEvent.change(input, { target: { value: '6' } });
  fireEvent.keyDown(input, { key: 'Enter' });
}
function key(key: string) {
  fireEvent.keyDown(document.activeElement ?? document.body, { key });
}

describe('occupied table choice', () => {
  it('consumes the table-number Enter before it can confirm the next dialog', () => {
    const onOpen = vi.fn();
    render(<TableBox tables={[table]} onOpen={onOpen} onClose={vi.fn()} />);
    const reachedDocument = vi.fn();
    document.addEventListener('keydown', reachedDocument);
    try {
      enterTable();
      expect(reachedDocument).not.toHaveBeenCalled();
      expect(onOpen).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener('keydown', reachedDocument);
    }
  });

  it('asks again before adding to the existing order, and Enter confirms', () => {
    const onOpen = vi.fn();
    render(<TableBox tables={[table]} onOpen={onOpen} onClose={vi.fn()} />);
    enterTable();
    expect(onOpen).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog', { name: 'Table' })).toBeInTheDocument();
    expect(screen.getAllByRole('radio').map((option) => option.textContent)).toEqual(['6', '6B', '6C', '6D', '6E', '6F', '6G', '6H', '6I']);
    expect(screen.getByRole('radio', { name: '6' })).toHaveAttribute('aria-checked', 'true');
    key('Enter');
    expect(onOpen).toHaveBeenCalledExactlyOnceWith(table, undefined);
  });

  it('uses left and right for 6, 6B, 6C and confirms the chosen letter', () => {
    const onOpen = vi.fn();
    render(<TableBox tables={[table]} onOpen={onOpen} onClose={vi.fn()} />);
    enterTable();
    key('ArrowRight');
    expect(screen.getByRole('radio', { name: '6B' })).toHaveFocus();
    key('ArrowRight');
    expect(screen.getByRole('radio', { name: '6C' })).toHaveFocus();
    key('ArrowLeft');
    key('Enter');
    expect(onOpen).toHaveBeenCalledExactlyOnceWith(table, 'B');
  });

  it('requires a fresh Enter press when the table-number key is held down', () => {
    const onOpen = vi.fn();
    render(<TableBox tables={[table]} onOpen={onOpen} onClose={vi.fn()} />);
    enterTable();
    fireEvent.keyDown(document.activeElement!, { key: 'Enter', repeat: true });
    expect(onOpen).not.toHaveBeenCalled();
    key('Enter');
    expect(onOpen).toHaveBeenCalledExactlyOnceWith(table, undefined);
  });

  it('moves between grid rows and stops at the eighth subtable', () => {
    const onOpen = vi.fn();
    render(<TableBox tables={[table]} onOpen={onOpen} onClose={vi.fn()} />);
    enterTable();
    key('ArrowDown');
    expect(screen.getByRole('radio', { name: '6D' })).toHaveFocus();
    key('ArrowUp');
    expect(screen.getByRole('radio', { name: '6' })).toHaveFocus();
    key('End');
    key('ArrowRight');
    key('ArrowDown');
    expect(screen.getByRole('radio', { name: '6I' })).toHaveFocus();
    key('Enter');
    expect(onOpen).toHaveBeenCalledExactlyOnceWith(table, 'I');
  });

  it('does not reserve a letter belonging to the same table number in another room', () => {
    render(<TableBox tables={[table, { ...table, id: 'other_order', section: 'Patio', label: '6B', seat: 'B' }]}
      onOpen={vi.fn()} onClose={vi.fn()} />);
    enterTable();
    key('ArrowRight');
    expect(screen.getByRole('radio', { name: '6B' })).toHaveFocus();
  });

  it('skips letters already used and also supports touch', () => {
    const onOpen = vi.fn();
    render(<TableBox tables={[table, { ...table, id: 'order_6B', label: '6B', seat: 'B', orderId: 'order_6B' }]}
      onOpen={onOpen} onClose={vi.fn()} />);
    enterTable();
    expect(screen.getByRole('radio', { name: '6B' })).toBeDisabled();
    key('ArrowRight');
    expect(screen.getByRole('radio', { name: '6C' })).toHaveFocus();
    fireEvent.click(screen.getByRole('radio', { name: '6D' }));
    fireEvent.click(screen.getByRole('radio', { name: '6C' }));
    fireEvent.click(screen.getByRole('button', { name: 'Open' }));
    expect(onOpen).toHaveBeenCalledExactlyOnceWith(table, 'C');
  });

  it('cancels without opening an order and opens free tables directly', () => {
    const onOpen = vi.fn();
    const onClose = vi.fn();
    const view = render(<TableBox tables={[table]} onOpen={onOpen} onClose={onClose} />);
    enterTable();
    key('Escape');
    expect(onClose).toHaveBeenCalledOnce();
    expect(onOpen).not.toHaveBeenCalled();
    view.unmount();
    const free: TableView = { ...table, state: 'free', orderId: null };
    render(<TableBox tables={[free]} onOpen={onOpen} onClose={onClose} />);
    enterTable();
    expect(onOpen).toHaveBeenCalledExactlyOnceWith(free);
  });

  it('blocks a letter taken while choosing and allows another letter', () => {
    const onOpen = vi.fn();
    const onClose = vi.fn();
    const view = render(<TableBox tables={[table]} onOpen={onOpen} onClose={onClose} />);
    enterTable();
    key('ArrowRight');
    view.rerender(<TableBox tables={[table, { ...table, id: 'order_6B', label: '6B', seat: 'B' }]} onOpen={onOpen} onClose={onClose} />);
    expect(screen.getByRole('alert')).toHaveTextContent('6B is unavailable.');
    expect(screen.getByRole('button', { name: 'Open' })).toBeDisabled();
    key('Enter');
    expect(onOpen).not.toHaveBeenCalled();
    key('ArrowRight');
    key('Enter');
    expect(onOpen).toHaveBeenCalledExactlyOnceWith(table, 'C');
  });

  it('keeps only the main table available when all eight subtables are occupied', () => {
    const tables = [table, ...Array.from('BCDEFGHIJKLMNOPQRSTUVWXYZ').map((seat) => ({ ...table, label: `6${seat}`, seat }))];
    render(<TableBox tables={tables} onOpen={vi.fn()} onClose={vi.fn()} />);
    enterTable();
    expect(screen.getAllByRole('radio').filter((option) => !option.hasAttribute('disabled'))).toEqual([screen.getByRole('radio', { name: '6' })]);
    key('ArrowRight');
    expect(screen.getByRole('radio', { name: '6' })).toHaveAttribute('aria-checked', 'true');
  });

  it('ignores confirmation, cancellation and arrows while assignment is running', () => {
    const onOpen = vi.fn();
    const onClose = vi.fn();
    const view = render(<TableBox tables={[table]} onOpen={onOpen} onClose={onClose} />);
    enterTable();
    view.rerender(<TableBox tables={[table]} busy onOpen={onOpen} onClose={onClose} />);
    key('ArrowRight');
    key('Enter');
    key('Escape');
    expect(onOpen).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole('radio', { name: '6' })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('button', { name: 'Open' })).toBeDisabled();
  });
});

describe('saving after the table choice', () => {
  let cart: CartView;
  beforeEach(() => {
    const zero = { paise: 0n, text: '0.00' };
    const money = { paise: 16000n, text: '160.00' };
    cart = {
      isCorrection: false,
      lines: [{ index: 0, editToken: 'line', name: 'Masala Dosa', note: null, qty: '2',
        rateLabel: '5%', unitPrice: money, gross: money, discount: zero, lineDiscount: zero,
        amount: money, modifiers: [] }],
      bill: { subtotal: money, lineDiscount: zero, billDiscount: zero, totalDiscount: zero,
        discountCapped: false, charges: [], taxRows: [], taxTotal: zero, nonGstValue: zero,
        exemptValue: zero, roundOff: zero, grandTotal: money },
      orderType: 'Dine in', table: null, payments: [], paid: zero, balance: money, change: zero,
      isEmpty: false, kitchenUpToDate: false, kitchenTold: false, covers: null,
      orderId: null, token: null, billNumber: null, fromTheFloor: [], lengthSays: '',
      orderTypeLocked: false, kitchenTicketOff: false, tablesOnCounter: true,
      arrivalBeep: false, arrivalBeat: false,
    };
    call.mockImplementation((command: string, args?: { seat?: string }) => {
      if (command === 'open_orders') return Promise.resolve([table]);
      if (command === 'menu_items') return Promise.resolve([]);
      if (command === 'device_manager') return Promise.resolve({ devices: [] });
      if ((command === 'print_kitchen_ticket' || command === 'complete_bill') && !cart.table) {
        return Promise.reject({ code: 'bill.no_table', message: 'Choose a table.' });
      }
      if (command === 'open_table' || command === 'join_table') {
        cart = { ...cart, table: `6${args?.seat ?? ''}` };
      }
      return Promise.resolve(cart);
    });
  });

  async function showChoice() {
    render(<ToastProvider><Billing onGoTo={vi.fn()} /></ToastProvider>);
    await screen.findByRole('button', { name: 'Change the quantity of Masala Dosa' });
    fireEvent.keyDown(screen.getByRole('searchbox', { name: /Item or table number/ }), { key: 'Enter' });
    await screen.findByLabelText('Table number');
    await waitFor(() => expect(screen.getByRole('button', { name: 'Open' })).not.toBeDisabled());
    enterTable();
  }

  it.each([undefined, 'B'])('assigns %s then resumes the kitchen action once', async (seat) => {
    await showChoice();
    expect(call.mock.calls.filter(([command]) => command === 'print_kitchen_ticket')).toHaveLength(1);
    if (seat) key('ArrowRight');
    key('Enter');
    await waitFor(() => expect(call).toHaveBeenCalledWith('cart_clear', { keepType: true }));
    expect(call).toHaveBeenCalledWith(seat ? 'join_table' : 'open_table', seat ? { tableId: 'table_6', seat } : { tableId: 'table_6' });
    expect(call.mock.calls.filter(([command]) => command === 'print_kitchen_ticket')).toHaveLength(2);
    expect(cart.lines[0]?.name).toBe('Masala Dosa');
  });

  it('keeps the choice open and never sends or clears after assignment fails', async () => {
    const answer = call.getMockImplementation()!;
    call.mockImplementation((command: string, args?: unknown) => command === 'join_table'
      ? Promise.reject({ code: 'table.seat_taken', message: 'Choose another letter.' })
      : answer(command, args));
    await showChoice();
    key('ArrowRight');
    key('Enter');
    await screen.findByText('Choose another letter.');
    await waitFor(() => expect(screen.getByRole('button', { name: 'Open' })).not.toBeDisabled());
    expect(screen.getByRole('dialog', { name: 'Table' })).toBeInTheDocument();
    expect(call.mock.calls.filter(([command]) => command === 'print_kitchen_ticket')).toHaveLength(1);
    expect(call.mock.calls.some(([command]) => command === 'cart_clear')).toBe(false);
    expect(cart.table).toBeNull();
  });

  it('resumes settlement when it was the action waiting for a table', async () => {
    cart.kitchenUpToDate = true;
    await showChoice();
    key('ArrowRight');
    key('Enter');
    await waitFor(() => expect(call.mock.calls.filter(([command]) => command === 'complete_bill')).toHaveLength(2));
    expect(call).toHaveBeenCalledWith('join_table', { tableId: 'table_6', seat: 'B' });
    expect(call.mock.calls.some(([command]) => command === 'print_kitchen_ticket')).toBe(false);
  });
});
