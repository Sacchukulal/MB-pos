import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { CartView } from '../src/ipc/generated/CartView';
import type { Pushed } from '../src/ipc/generated/Pushed';
import type { TableView } from '../src/ipc/generated/TableView';

const { call } = vi.hoisted(() => ({ call: vi.fn() }));
let push: (message: Pushed) => void;
vi.mock('../src/ipc/call', () => ({
  call,
  inApp: () => true,
  isUiError: (cause: unknown) => typeof cause === 'object' && cause !== null && 'code' in cause,
  subscribe: (receive: typeof push) => { push = receive; return Promise.resolve(() => undefined); },
}));
vi.mock('../src/clock', () => ({ useTick: () => 0 }));
import { Billing } from '../src/billing/Billing';
import { ToastProvider } from '../src/kit';

const zero = { paise: 0n, text: '0.00' };
const money = { paise: 16000n, text: '160.00' };
let cart: CartView;
beforeEach(() => {
  call.mockReset();
  cart = {
    lines: [{ index: 0, editToken: 'original-line', name: 'Masala Dosa', note: null, qty: '2',
      rateLabel: '5%', unitPrice: money, gross: money, discount: zero, lineDiscount: zero,
      amount: money, modifiers: [] }],
    bill: { subtotal: money, lineDiscount: zero, billDiscount: zero, totalDiscount: zero,
      discountCapped: false, charges: [], taxRows: [], taxTotal: zero, nonGstValue: zero,
      exemptValue: zero, roundOff: zero, grandTotal: money },
    orderType: 'Dine in', table: '4', payments: [], paid: zero, balance: money, change: zero,
    isEmpty: false, kitchenUpToDate: false, kitchenTold: false, covers: null,
    orderId: 'order_4', token: null, billNumber: null, fromTheFloor: [], lengthSays: '',
    orderTypeLocked: true, kitchenTicketOff: false, tablesOnCounter: true,
    arrivalBeep: false, arrivalBeat: false,
  };
  call.mockImplementation((command: string) => {
    if (command === 'menu_items' || command === 'open_orders') return Promise.resolve([]);
    if (command === 'device_manager') return Promise.resolve({ devices: [] });
    if (command === 'reasons') return Promise.resolve([{ id: 'reason_1', text: 'Customer request' }]);
    return Promise.resolve(cart);
  });
});
afterEach(cleanup);

async function show() {
  render(<ToastProvider><Billing onGoTo={vi.fn()} /></ToastProvider>);
  await screen.findByRole('button', { name: 'Change the quantity of Masala Dosa' });
}

async function phoneChangedLine() {
  cart = { ...cart, lines: [{ ...cart.lines[0]!, editToken: 'newer-line', qty: '3' }] };
  await act(async () => { push({ kind: 'floor' }); });
}

it('sends the rendered line token when removing an item', async () => {
  await show();
  fireEvent.click(screen.getByRole('button', { name: 'Take Masala Dosa off the bill' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('cart_remove', { index: 0, expectedLine: 'original-line' }));
});

it('keeps the original token while quantity typing overlaps a phone update', async () => {
  await show();
  fireEvent.click(screen.getByRole('button', { name: 'Change the quantity of Masala Dosa' }));
  await phoneChangedLine();
  const input = screen.getByRole('textbox', { name: 'Quantity of Masala Dosa' });
  fireEvent.change(input, { target: { value: '4' } });
  fireEvent.blur(input);
  await waitFor(() => expect(call).toHaveBeenCalledWith('cart_set_qty', {
    index: 0, qty: '4', expectedLine: 'original-line',
  }));
});

it('keeps the selected line token while a void reason dialog is open', async () => {
  cart.kitchenTold = true;
  await show();
  fireEvent.click(screen.getByRole('button', { name: 'Take Masala Dosa off the bill' }));
  await screen.findByText('Customer request');
  await phoneChangedLine();
  fireEvent.click(screen.getByRole('button', { name: 'Take it off' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('void_line', {
    index: 0, reason: 'Customer request', expectedLine: 'original-line',
  }));
});

it('keeps the selected line token while a line discount dialog is open', async () => {
  await show();
  fireEvent.click(screen.getByRole('button', { name: 'More for Masala Dosa' }));
  fireEvent.click(screen.getByRole('button', { name: 'Money off' }));
  await phoneChangedLine();
  fireEvent.change(screen.getByLabelText('Per cent'), { target: { value: '10' } });
  fireEvent.click(screen.getByRole('button', { name: 'Take it off' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('cart_set_discount', {
    line: 0, kind: 'percent', value: '10', reason: null, expectedLine: 'original-line',
  }));
});

it('keeps the selected line token when clearing a discount after a phone update', async () => {
  cart.lines[0]!.lineDiscount = money;
  await show();
  fireEvent.click(screen.getByRole('button', { name: 'More for Masala Dosa' }));
  fireEvent.click(screen.getByRole('button', { name: 'Money off' }));
  await phoneChangedLine();
  fireEvent.click(screen.getByRole('button', { name: 'Remove the discount' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('cart_clear_discount', {
    line: 0, expectedLine: 'original-line',
  }));
});

it('shows the whole-quantity reduction refusal without rounding a later fractional increase', async () => {
  const answer = call.getMockImplementation()!;
  const message = 'Use a whole quantity of 1 or more when reducing an item. To remove it completely, use Remove.';
  call.mockImplementation((command: string, args?: { qty?: string }) => {
    if (command === 'cart_set_qty' && args?.qty === '0.5') {
      return Promise.reject({ code: 'cart.qty_whole', message });
    }
    return answer(command, args);
  });
  await show();
  fireEvent.click(screen.getByRole('button', { name: 'Change the quantity of Masala Dosa' }));
  let input = screen.getByRole('textbox', { name: 'Quantity of Masala Dosa' });
  expect(input).toHaveAttribute('title', expect.stringContaining('whole quantity'));
  fireEvent.change(input, { target: { value: '0.5' } });
  fireEvent.blur(input);
  await screen.findByText(message);
  expect(screen.getByRole('button', { name: 'Change the quantity of Masala Dosa' })).toHaveTextContent('2');

  fireEvent.click(screen.getByRole('button', { name: 'Change the quantity of Masala Dosa' }));
  input = screen.getByRole('textbox', { name: 'Quantity of Masala Dosa' });
  fireEvent.change(input, { target: { value: '2.5' } });
  fireEvent.blur(input);
  await waitFor(() => expect(call).toHaveBeenCalledWith('cart_set_qty', {
    index: 0, qty: '2.5', expectedLine: 'original-line',
  }));
});

it('keeps the latest tile total when overlapping floor replies arrive in reverse order', async () => {
  await show();
  let oldReply!: (tables: TableView[]) => void;
  let newReply!: (tables: TableView[]) => void;
  const older = new Promise<TableView[]>((resolve) => { oldReply = resolve; });
  const newer = new Promise<TableView[]>((resolve) => { newReply = resolve; });
  let reads = 0;
  const answer = call.getMockImplementation()!;
  call.mockImplementation((command: string, args?: unknown) => {
    if (command === 'open_orders') return ++reads === 1 ? older : newer;
    return answer(command, args);
  });
  const tile: TableView = {
    id: 'table_4', label: '4', section: 'Main', sectionOrder: 0, seats: 4, state: 'occupied',
    total: { paise: 60000n, text: '600.00' }, minutes: 0, createdAt: null,
    kitchenTold: false, kitchenMinutes: null, billAsked: false, settleAsked: false,
    by: null, byId: null, orderId: 'order_4', seat: null, token: null, billNumber: null, selected: true,
  };
  act(() => { push({ kind: 'floor' }); push({ kind: 'floor' }); });
  await act(async () => { newReply([tile]); });
  expect(screen.getByRole('button', { name: /600\.00/ })).toBeInTheDocument();
  await act(async () => { oldReply([{ ...tile, total: { paise: 10000n, text: '100.00' } }]); });
  expect(screen.getByRole('button', { name: /600\.00/ })).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: /100\.00/ })).not.toBeInTheDocument();
});
