import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock('../src/ipc/call', () => ({ call,
  isUiError: (cause: unknown) => !!cause && typeof cause === 'object' && 'code' in cause,
}));
vi.mock('../src/shell/licence', () => ({ useLicenceRevision: () => 0 }));
import { Bills, Correction } from '../src/reports/Bills';
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
    if (command === 'bill_detail') return Promise.resolve({ row: { orderId: 'sale', number: 'A/7', state, stateWord: state === 'settled' ? 'Paid' : 'Voided', total: money(9000), returned: false }, lines: [], edits: [], history: [], bill: null, canApprove: false, canRefund: true });
    if (command === 'bill_correction_offer') return Promise.resolve({
      remaining: money(5000), billTotal: money(9000), needsApproval: manager,
      closedDay: closed, returned: false, tenders: [{ mode: 'upi', remaining: money(5000), input: '50.00' }],
    });
    if (command === 'reasons') return Promise.resolve([{ id: 'customer', text: 'Customer request' }]);
    return Promise.resolve();
  });
});
afterEach(cleanup);

it('serializes pending approvals and blocks repeat clicks until refreshed', async () => {
  const implementation = call.getMockImplementation()!;
  let release!: () => void;
  const pending = new Promise<void>((resolve) => { release = resolve; });
  call.mockImplementation(async (command: string, args: unknown) => {
    if (command === 'approve_revert') return pending;
    const value = await implementation(command, args);
    if (command === 'bill_detail') {
      value.canApprove = true;
      value.edits = ['first', 'second'].map((id) => ({ id, by: 'Cashier', at: 'Today',
        reason: 'Wrong items', beforeTotal: money(9000), changes: [], beforeLines: [], approvedAt: null }));
    }
    return value;
  });
  render(<ToastProvider><Bills /></ToastProvider>);
  fireEvent.click(await screen.findByText('A/7'));
  const approve = await screen.findByRole('button', { name: 'Approve' });
  fireEvent.click(approve);
  fireEvent.click(approve);
  expect(approve).toBeDisabled();
  expect(call.mock.calls.filter(([name]) => name === 'approve_revert')).toEqual([
    ['approve_revert', { revertId: 'first' }],
  ]);
  release();
  await waitFor(() => expect(approve).toBeEnabled());
  expect(call.mock.calls.filter(([name]) => name === 'approve_revert')).toEqual([
    ['approve_revert', { revertId: 'first' }], ['approve_revert', { revertId: 'second' }],
  ]);
});

it('hides Return money when the original payment has already been fully refunded', async () => {
  state = 'voided';
  const implementation = call.getMockImplementation()!;
  call.mockImplementation(async (command: string, args: unknown) => {
    const value = await implementation(command, args);
    if (command === 'bill_detail') value.canRefund = false;
    return value;
  });
  render(<ToastProvider><Bills /></ToastProvider>);
  fireEvent.click(await screen.findByText('A/7'));
  await screen.findByRole('dialog', { name: 'Bill A/7' });
  expect(screen.queryByRole('button', { name: 'Return money' })).toBeNull();
});

it('replaces a stale refund form with a closeable explanation if another refund used up the balance', async () => {
  const implementation = call.getMockImplementation()!;
  const view = await implementation('bills');
  call.mockImplementation((command: string, args: unknown) => command === 'bill_correction_offer'
    ? Promise.resolve({ remaining: money(0), tenders: [], needsApproval: false, billTotal: money(9000) })
    : implementation(command, args));
  render(<ToastProvider><Correction pending={{ kind: 'refund', bill: view.rows[0] }} approvers={[]}
    onClose={vi.fn()} onDone={vi.fn()} onFailed={vi.fn()} /></ToastProvider>);
  await screen.findByText(/There is no payment left to refund/);
  expect(screen.queryByLabelText('Additional note')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Record the money going back' })).toBeNull();
  expect(screen.getByRole('button', { name: 'Close' })).toBeEnabled();
});

it('keeps earlier pending approval visible while another correction is being edited', async () => {
  const implementation = call.getMockImplementation()!;
  const view = await implementation('bills');
  view.rows[0].correctionOpen = true;
  view.rows[0].approval = 'waiting';
  call.mockImplementation((command: string, args: unknown) => command === 'bills' ? Promise.resolve(view) : implementation(command, args));
  render(<ToastProvider><Bills /></ToastProvider>);
  await screen.findByRole('button', { name: 'Resume bill' });
  expect(screen.getByText('Editing')).toBeTruthy();
  expect(screen.getByText('Pending approval')).toBeTruthy();
});

it('approves only completed pending edits, excluding discarded and unfinished drafts', async () => {
  const implementation = call.getMockImplementation()!;
  call.mockImplementation(async (command: string, args: unknown) => {
    const value = await implementation(command, args);
    if (command === 'bill_detail') {
      value.canApprove = true;
      value.row.correctionOpen = true;
      value.edits = ['completed', 'discarded', 'draft'].map((id) => ({
        id, by: 'Cashier', at: 'Today', reason: 'Wrong items', beforeTotal: money(9000),
        changes: [], beforeLines: [], approvedAt: null,
        discardedAt: id === 'discarded' ? 'Today' : null, discardedBy: 'Cashier',
      }));
    }
    return value;
  });
  render(<ToastProvider><Bills /></ToastProvider>);
  fireEvent.click(await screen.findByText('A/7'));
  const approve = await screen.findByRole('button', { name: 'Approve' });
  expect(screen.getByText(/Changes discarded by Cashier/)).toBeTruthy();
  fireEvent.click(approve);
  await waitFor(() => expect(approve).toBeEnabled());
  expect(call.mock.calls.filter(([name]) => name === 'approve_revert')).toEqual([
    ['approve_revert', { revertId: 'completed' }],
  ]);
});

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

  fireEvent.click(await screen.findByRole('button', { name: 'Delete bill' }));
  await screen.findByLabelText('Customer request');
  await waitFor(() => expect(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete bill' })).toBeEnabled());
  expect(screen.getByText(/This cancels the bill without recording a refund/)).toBeTruthy();
  expect(call.mock.calls.some(([name]) => name === 'void_bill' || name === 'bill_detail')).toBe(false);
  fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete bill' }));
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

  await screen.findByText('A/7');
  expect(screen.queryByRole('button', { name: 'Reprint bill' })).toBeNull();
  expect(screen.queryByRole('menu')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Revert bill' })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Delete bill' })).toBeNull();
  expect(screen.queryByText('Invoice PDF')).toBeNull();
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

  fireEvent.click(await screen.findByText('A/7'));
  fireEvent.click(await screen.findByRole('button', { name: action }));
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
  await show('Refund entire bill');
  const button = screen.getByRole('button', { name: 'Confirm return' });
  fireEvent.click(button); fireEvent.click(button);
  await waitFor(() => expect(call.mock.calls.filter(([name]) => name === 'void_and_return_bill')).toHaveLength(1));
  expect(call.mock.calls.some(([name]) => name === 'void_bill' || name === 'refund_bill')).toBe(false);
  expect(screen.getByRole('button', { name: 'Saving…' })).toBeDisabled();
});

it('explains and submits a closed-day linked return instead of voiding the old bill', async () => {
  closed = true;
  await show('Refund entire bill');
  expect(screen.getByText(/original closed bill stays unchanged/i)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Confirm return' }));
  await waitFor(() => expect(call.mock.calls.some(([name]) => name === 'return_closed_bill')).toBe(true));
  expect(call.mock.calls.some(([name]) => name === 'void_bill' || name === 'void_and_return_bill')).toBe(false);
});

it('asks for required manager approval before attempting the write', async () => {
  manager = true;
  await show('Refund entire bill');
  expect(screen.getByLabelText('Their PIN')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Confirm return' }));
  expect(call.mock.calls.some(([name]) => name === 'void_and_return_bill')).toBe(false);
});
