/** The day close screen. */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
}));

const { DayClose } = await import('../src/reports/DayClose');
const { ToastProvider } = await import('../src/kit');

import type { DayCloseView } from '../src/ipc/generated/DayCloseView';

/** `MoneyView.paise` is a `bigint`, because Rust's is an `i64`. */
function money(paise: number, text: string) {
  return { paise: BigInt(paise), text };
}

const view: DayCloseView = {
  day: '2026-08-09',
  daySays: 'The day of 2026-08-09',
  takings: [{ label: 'Bills', amount: money(2_412_000, '24120.00') }],
  drawer: [{ label: 'Opening float', amount: money(200_000, '2000.00') }],
  expected: money(1_140_000, '11400.00'),
  denominations: [
    { value: 50_000, label: '500', count: 0, total: money(0, '0.00') },
    { value: 1_000, label: '10', count: 0, total: money(0, '0.00') },
  ],
  counted: money(0, '0.00'),
  variance: money(-1_140_000, '-11400.00'),
  varianceSays: 'Short by 11400.00.',
  varianceKind: 'short',
  needsReason: true,
  reasonSays: 'The drawer is out by more than 20.00, so this needs a reason.',
  reason: '',
  isClosed: false,
  closedSays: '',
  carrySays: '',
  mayClose: true,
  tillsSay: '',
  openOrders: [],
  openSays: '',
};

/** What Rust sends back once twenty ₹500 notes are counted. */
const counted: DayCloseView = {
  ...view,
  denominations: [
    { value: 50_000, label: '500', count: 20, total: money(1_000_000, '10000.00') },
    { value: 1_000, label: '10', count: 0, total: money(0, '0.00') },
  ],
  counted: money(1_000_000, '10000.00'),
  variance: money(-140_000, '-1400.00'),
  varianceSays: 'Short by 1400.00.',
};

beforeEach(() => {
  call.mockReset();
  call.mockImplementation((command: string) => {
    if (command === 'day_close') return Promise.resolve(view);
    if (command === 'count_cash') return Promise.resolve(counted);
    if (command === 'close_day') return Promise.resolve({ ...counted, isClosed: true });
    return Promise.resolve(null);
  });
});
afterEach(cleanup);

function open() {
  return render(
    <ToastProvider>
      <DayClose />
    </ToastProvider>,
  );
}

it('sends the count to Rust and shows the total Rust sent back', async () => {
  open();
  await waitFor(() => expect(screen.getByText('11400.00')).toBeTruthy());

  fireEvent.change(screen.getByLabelText('How many 500 notes'), { target: { value: '20' } });

  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('count_cash', {
      counts: [{ value: 50_000, count: 20 }],
    }),
  );
  // The running total is the one Rust computed.
  await waitFor(() => expect(screen.getAllByText('10000.00')).toHaveLength(2));
});

it('shows the difference as a sentence rather than a signed number', async () => {
  open();
  await waitFor(() => expect(screen.getByText('Short by 11400.00.')).toBeTruthy());
  // The signed figure is never put on screen on its own.
  expect(screen.queryByText('-11400.00')).toBeNull();
  // And the reason box appears with the sentence that explains why.
  expect(screen.getByText(/needs a reason/)).toBeTruthy();
});

it('closes the day with what was typed, and offers not to print', async () => {
  open();
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Close the day' })).toBeTruthy(),
  );
  fireEvent.change(screen.getByLabelText('Why is the drawer out?'), {
    target: { value: 'paid the vegetable man' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Close the day' }));

  // Three ways out, not two: print, do not print, or change your mind.
  await waitFor(() => expect(screen.getByText('Close and print the slip')).toBeTruthy());
  expect(screen.getByText('Close without printing')).toBeTruthy();

  fireEvent.click(screen.getByRole('button', { name: 'Close without printing' }));
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('close_day', {
      counts: [],
      reason: 'paid the vegetable man',
      print: false,
    }),
  );
});
