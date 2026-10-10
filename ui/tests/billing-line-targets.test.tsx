import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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
  window.localStorage.clear();
  call.mockReset();
  cart = {
    isCorrection: false,
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
    if (command === 'correction_save_preview') return Promise.resolve({ cart, proposalToken: 'review', originalTotal: money, newTotal: money, needsApproval: false, approvers: [] });
    if (command === 'menu_items' || command === 'open_orders') return Promise.resolve([]);
    if (command === 'device_manager') return Promise.resolve({ devices: [] });
    if (command === 'reasons') return Promise.resolve([{ id: 'reason_1', text: 'Customer request' }]);
    return Promise.resolve(cart);
  });
});
afterEach(cleanup);

it('shows explicit correction actions and confirms before discarding', async () => {
  cart.isCorrection = true;
  cart.billNumber = 'A/7';
  await show();
  expect(screen.getByRole('button', { name: 'Save changes' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Discard changes' }));
  expect(call.mock.calls.some(([name]) => name === 'discard_bill_correction')).toBe(false);
  fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Discard changes' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('discard_bill_correction', { orderId: 'order_4' }));
});

it('takes an empty correction to the full return flow after saving its draft', async () => {
  cart = { ...cart, isCorrection: true, isEmpty: true, lines: [], billNumber: 'A/7' };
  const go = vi.fn();
  render(<ToastProvider><Billing onGoTo={go} /></ToastProvider>);
  fireEvent.click(await screen.findByRole('button', { name: 'Return whole bill' }));
  await waitFor(() => expect(go).toHaveBeenCalledWith('reports/bills/order_4'));
  expect(call).toHaveBeenCalledWith('save_bill_correction_draft');
  expect(call.mock.calls.some(([name]) => name === 'complete_bill')).toBe(false);
});

it('asks for the original payment return before submitting a reduced correction', async () => {
  cart.isCorrection = true;
  cart.billNumber = 'A/7';
  cart.change = money;
  cart.payments = [{ index: 0, reference: null, mode: 'Card', refundMode: 'card', amount: money }];
  await show();
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await screen.findByRole('button', { name: 'Confirm return' });
  expect(screen.getByLabelText('Return by Card')).toBeTruthy();
  expect(call.mock.calls.some(([name]) => name === 'complete_bill')).toBe(false);
});

it('uses the fresh preflight cart for return amounts after a concurrent order update', async () => {
  cart.isCorrection = true;
  cart.billNumber = 'A/7';
  await show();
  const current = { ...cart, change: money, payments: [{ index: 0, reference: null, mode: 'UPI', refundMode: 'upi', amount: money }] };
  call.mockImplementation((command: string) => command === 'correction_save_preview'
    ? Promise.resolve({ cart: current, proposalToken: 'new-review', originalTotal: money, newTotal: zero, needsApproval: false, approvers: [] })
    : Promise.resolve(cart));
  fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
  await screen.findByRole('button', { name: 'Confirm return' });
  expect(screen.getByLabelText('Return by UPI')).toHaveValue('160.00');
  expect(call.mock.calls.some(([name]) => name === 'complete_bill')).toBe(false);
});

it('starts a fresh default order on Escape when unlocked', async () => {
  cart.orderTypeLocked = false;
  await show();
  fireEvent.keyDown(window, { key: 'Escape' });
  await waitFor(() => expect(call).toHaveBeenCalledWith('cart_clear', { keepType: false }));
});

it('keeps the current mode on Escape when arrow-locked, while mouse switching stays available', async () => {
  cart.orderTypeLocked = false;
  await show();
  fireEvent.click(screen.getByRole('button', { name: 'Lock the arrow keys' }));
  fireEvent.keyDown(window, { key: 'ArrowRight' });
  expect(call.mock.calls.some(([command]) => command === 'cart_set_order_type')).toBe(false);
  fireEvent.click(screen.getByRole('button', { name: 'Parcel' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('cart_set_order_type', { orderType: 'Parcel' }));
  fireEvent.keyDown(window, { key: 'Escape' });
  await waitFor(() => expect(call).toHaveBeenCalledWith('cart_clear', { keepType: true }));
});

it('shows Self service first in the service buttons', async () => {
  cart.orderTypeLocked = false;
  await show();
  const modes = screen.getByRole('group', { name: 'Order type' });
  expect([...modes.querySelectorAll('button')].map((button) => button.textContent))
    .toEqual(['Self service', 'Dine in', 'Parcel', 'Delivery']);
});

it.each([['Increase', '3'], ['Decrease', '1']])('uses the existing guarded quantity edit for %s', async (action, qty) => {
  await show();
  fireEvent.click(screen.getByRole('button', { name: `${action} the quantity of Masala Dosa` }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('cart_set_qty', {
    index: 0, qty, expectedLine: 'original-line',
  }));
});

it('adjusts a typed quantity with arrows without navigating orders or losing the original line token', async () => {
  await show();
  fireEvent.click(screen.getByRole('button', { name: 'Change the quantity of Masala Dosa' }));
  await phoneChangedLine();
  const input = screen.getByRole('textbox', { name: 'Quantity of Masala Dosa' });
  fireEvent.keyDown(input, { key: 'ArrowUp' });
  expect(input).toHaveValue('3');
  fireEvent.keyDown(input, { key: 'ArrowDown' });
  expect(input).toHaveValue('2');
  fireEvent.click(screen.getByRole('button', { name: 'Increase the quantity of Masala Dosa' }));
  expect(input).toHaveValue('3');
  fireEvent.keyDown(input, { key: 'Enter' });
  await waitFor(() => expect(call).toHaveBeenCalledWith('cart_set_qty', {
    index: 0, qty: '3', expectedLine: 'original-line',
  }));
  expect(call.mock.calls.filter(([command]) => command === 'cart_set_qty')).toHaveLength(1);
  expect(call.mock.calls.some(([command]) => command === 'cart_clear' || command === 'open_table')).toBe(false);
});

it('does not reduce one item to zero through the quantity controls', async () => {
  cart.lines[0]!.qty = '1';
  await show();
  fireEvent.click(screen.getByRole('button', { name: 'Decrease the quantity of Masala Dosa' }));
  fireEvent.keyDown(screen.getByRole('button', { name: 'Change the quantity of Masala Dosa' }), { key: 'ArrowDown' });
  expect(call.mock.calls.some(([command]) => command === 'cart_set_qty' || command === 'cart_remove')).toBe(false);
});

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
    processing: false,
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
