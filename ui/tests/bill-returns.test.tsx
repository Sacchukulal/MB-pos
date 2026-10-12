import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock('../src/ipc/call', () => ({ call,
  isUiError: (cause: unknown) => !!cause && typeof cause === 'object' && 'code' in cause,
}));
vi.mock('../src/shell/licence', () => ({ useLicenceRevision: () => 0 }));
import { Bills } from '../src/reports/Bills';
import { ToastProvider } from '../src/kit';

const money = (paise: number) => ({ paise: BigInt(paise), text: (paise / 100).toFixed(2) });
let state: string;
let closed: boolean;
let manager: boolean;
beforeEach(() => {
  call.mockReset(); state = 'settled'; closed = false; manager = false;
  call.mockImplementation((command: string) => {
    if (command === 'bills') return Promise.resolve({
      rows: [{ orderId: 'sale', number: 'A/7', state, stateWord: state === 'settled' ? 'Paid' : 'Voided',
        total: money(9000), paidBy: 'UPI', at: 'Today', items: 1, table: null, orderType: 'Parcel',
        cashier: 'Owner', reprints: 0, edited: false, returned: false, refunded: money(4000), approval: null }],
      totals: null, canExport: false, periods: [{ label: 'Today', from: '2026-10-08', to: '2026-10-08' }],
      cashiers: [], approvers: [{ id: 'manager', name: 'Manager' }], waiting: 0,
      canRevert: true, canVoid: true, canReprint: true, canApprove: false,
    });
    if (command === 'bill_correction_offer') return Promise.resolve({
      remaining: money(5000), billTotal: money(9000), needsApproval: manager,
      closedDay: closed, returned: false, tenders: [{ mode: 'upi', remaining: money(5000), input: '50.00' }],
    });
    if (command === 'reasons') return Promise.resolve([{ id: 'customer', text: 'Customer request' }]);
    return Promise.resolve();
  });
});
afterEach(cleanup);

it('opens the existing reprint confirmation directly without opening the bill or actions menu', async () => {
  render(<ToastProvider><Bills /></ToastProvider>);
  fireEvent.click(await screen.findByRole('button', { name: 'Reprint bill' }));
  await screen.findByLabelText('Customer request');
  expect(screen.queryByRole('menu')).toBeNull();
  expect(call.mock.calls.some(([name]) => name === 'bill_detail' || name === 'reprint_bill')).toBe(false);
  fireEvent.click(screen.getByRole('button', { name: 'Print another copy' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('reprint_bill', { orderId: 'sale', reason: 'Customer request' }));
});

it('keeps cancellation without refund on the existing reason and void command', async () => {
  render(<ToastProvider><Bills /></ToastProvider>);
  fireEvent.click(await screen.findByRole('button', { name: 'Actions for bill A/7' }));
  fireEvent.click(screen.getByRole('menuitem', { name: 'Cancel bill (no refund)' }));
  await screen.findByLabelText('Customer request');
  await waitFor(() => expect(screen.getByRole('button', { name: 'Cancel bill (no refund)' })).toBeEnabled());
  expect(screen.getByText(/This cancels the bill without recording a refund/)).toBeTruthy();
  expect(call.mock.calls.some(([name]) => name === 'void_bill' || name === 'bill_detail')).toBe(false);
  fireEvent.click(screen.getByRole('button', { name: 'Cancel bill (no refund)' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('void_bill', {
    orderId: 'sale', reason: 'Customer request', approverStaffId: null, approverPin: null,
  }));
  expect(call.mock.calls.some(([name]) => ['void_and_return_bill', 'return_closed_bill', 'return_bill_money'].includes(name))).toBe(false);
});

it('keeps restricted actions hidden when the menu opens', async () => {
  const implementation = call.getMockImplementation()!;
  const view = await implementation('bills');
  view.canRevert = false; view.canVoid = false; view.canReprint = false;
  call.mockImplementation((command: string, args: unknown) => command === 'bills' ? Promise.resolve(view) : implementation(command, args));
  render(<ToastProvider><Bills /></ToastProvider>);
  fireEvent.click(await screen.findByRole('button', { name: 'Actions for bill A/7' }));
  expect(screen.queryByRole('button', { name: 'Reprint bill' })).toBeNull();
  expect(screen.getAllByRole('menuitem').map((item) => item.textContent)).toEqual(['Invoice PDF']);
});

it.each([false, true])('checks whole-return permission before opening the correction route (allowed: %s)', async (canVoid) => {
  const implementation = call.getMockImplementation()!;
  const view = await implementation('bills');
  view.canVoid = canVoid;
  const detail = { row: view.rows[0], lines: [], edits: [], history: [], bill: null, canApprove: false };
  call.mockImplementation((command: string, args: unknown) => {
    if (command === 'bills') return Promise.resolve(view);
    if (command === 'bill_detail') return Promise.resolve(detail);
    return implementation(command, args);
  });
  render(<ToastProvider><Bills initialOrderId="sale" /></ToastProvider>);
  if (canVoid) {
    await screen.findByRole('button', { name: 'Confirm return' });
    expect(screen.queryByText(/manager with bill-return permission/)).toBeNull();
  } else {
    await screen.findByText(/manager with bill-return permission must sign in/);
    expect(screen.queryByRole('button', { name: 'Confirm return' })).toBeNull();
    expect(call.mock.calls.some(([name]) => name === 'bill_correction_offer')).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'A/7' } });
    await waitFor(() => expect(call).toHaveBeenCalledWith('bills', expect.objectContaining({ filter: expect.objectContaining({ query: 'A/7' }) })));
    expect(screen.queryByRole('dialog')).toBeNull();
  }
  expect(call.mock.calls.filter(([name]) => name === 'bill_detail')).toHaveLength(1);
});

async function show(action: string) {
  render(<ToastProvider><Bills /></ToastProvider>);
  fireEvent.click(await screen.findByRole('button', { name: 'Actions for bill A/7' }));
  fireEvent.click(screen.getByRole('menuitem', { name: action }));
  await screen.findByLabelText('Return by upi');
  await screen.findByLabelText('Customer request');
}

it('offers the remaining original tender and hides report exports in bill-only access', async () => {
  state = 'voided';
  await show('Return money');
  expect((screen.getByLabelText('Return by upi') as HTMLInputElement).value).toBe('50.00');
  expect(screen.queryByLabelText('Return by cash')).toBeNull();
  expect(screen.queryByText('Save as CSV')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Record the money going back' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('return_bill_money', {
    orderId: 'sale', amounts: [['upi', '50.00']], reason: 'Customer request', requestId: expect.any(String),
  }));
});

it('sends one atomic whole return and blocks a second click while it is saving', async () => {
  const implementation = call.getMockImplementation()!;
  call.mockImplementation((command: string, args: unknown) => command === 'void_and_return_bill'
    ? new Promise(() => undefined) : implementation(command, args));
  await show('Return whole bill');
  const button = screen.getByRole('button', { name: 'Confirm return' });
  fireEvent.click(button); fireEvent.click(button);
  await waitFor(() => expect(call.mock.calls.filter(([name]) => name === 'void_and_return_bill')).toHaveLength(1));
  expect(call.mock.calls.some(([name]) => name === 'void_bill' || name === 'refund_bill')).toBe(false);
  expect(screen.getByRole('button', { name: 'Saving…' })).toBeDisabled();
});

it('explains and submits a closed-day linked return instead of voiding the old bill', async () => {
  closed = true;
  await show('Return whole bill');
  expect(screen.getByText(/original closed bill stays unchanged/i)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Confirm return' }));
  await waitFor(() => expect(call.mock.calls.some(([name]) => name === 'return_closed_bill')).toBe(true));
  expect(call.mock.calls.some(([name]) => name === 'void_bill' || name === 'void_and_return_bill')).toBe(false);
});

it('asks for required manager approval before attempting the write', async () => {
  manager = true;
  await show('Return whole bill');
  expect(screen.getByLabelText('Their PIN')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Confirm return' }));
  expect(call.mock.calls.some(([name]) => name === 'void_and_return_bill')).toBe(false);
});
