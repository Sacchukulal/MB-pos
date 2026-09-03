/** The day gate and the Days screen. */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
}));

const { DayGate } = await import('../src/shell/DayGate');
const { Days } = await import('../src/reports/Days');
const { ToastProvider } = await import('../src/kit');

import type { DayStateView } from '../src/ipc/generated/DayStateView';
import type { DaysView } from '../src/ipc/generated/DaysView';
import type { PendingDayView } from '../src/ipc/generated/PendingDayView';
import type { DrawerView } from '../src/ipc/generated/DrawerView';

/** `MoneyView.paise` is a `bigint`, because Rust's is an `i64`. */
function money(paise: number, text: string) {
  return { paise: BigInt(paise), text };
}

/** A trading day left open. */
const tuesday: PendingDayView = {
  day: '2026-09-01',
  daySays: 'Tuesday 1 September',
  bills: 42,
  net: money(2_412_000, '24120.00'),
  cash: money(1_800_000, '18000.00'),
  upiAndCard: money(612_000, '6120.00'),
  expenses: money(30_000, '300.00'),
  openOrders: [],
  openSays: '',
  looksLikeHoliday: false,
  suggested: 'close',
};

/** An empty one Rust thinks was a holiday. */
const wednesday: PendingDayView = {
  day: '2026-09-02',
  daySays: 'Wednesday 2 September',
  bills: 0,
  net: money(0, '0.00'),
  cash: money(0, '0.00'),
  upiAndCard: money(0, '0.00'),
  expenses: money(0, '0.00'),
  openOrders: [],
  openSays: '',
  looksLikeHoliday: true,
  suggested: 'holiday',
};

const gate: DayStateView = {
  today: '2026-09-03',
  todaySays: 'Today, Thursday 3 September',
  pending: [tuesday, wednesday],
  pendingSays: '2 days were never closed.',
  mayAct: true,
  blockedSays: '',
  todayState: 'open',
  todayClosedSays: '',
  actionLabel: 'Close 1 day and mark 1 holiday',
  escapeLabel: '',
};

/** The same gate after the person switched Wednesday to Close. */
const bothClosed: DayStateView = {
  ...gate,
  pending: [tuesday, { ...wednesday, suggested: 'close' }],
  actionLabel: 'Close 2 days',
};

const done: DayStateView = { ...gate, pending: [], pendingSays: '', actionLabel: '' };

const days: DaysView = {
  today: '2026-09-03',
  todaySays: 'Today, Thursday 3 September',
  todayState: 'open',
  todayClosedSays: '',
  mayAct: true,
  carrySays: '',
  days: [
    {
      day: '2026-09-03',
      daySays: 'Thursday 3 September',
      kind: 'trading',
      isLocked: false,
      bills: 3,
      net: money(36_000, '360.00'),
      closedSays: 'Open.',
      state: 'open',
      mayBeHoliday: false,
    },
    {
      day: '2026-09-02',
      daySays: 'Wednesday 2 September',
      kind: 'holiday',
      isLocked: true,
      bills: 0,
      net: money(0, '0.00'),
      closedSays: 'Holiday, marked 3 Sep, 9:02 am by Meena.',
      state: 'holiday',
      mayBeHoliday: true,
    },
    {
      day: '2026-09-01',
      daySays: 'Tuesday 1 September',
      kind: 'trading',
      isLocked: true,
      bills: 42,
      net: money(2_412_000, '24120.00'),
      closedSays: 'Closed 3 Sep, 9:02 am by Meena.',
      state: 'closed',
      mayBeHoliday: false,
    },
  ],
  upcoming: [],
  mayPlanHoliday: true,
};

const drawer: DrawerView = {
  day: '2026-09-03',
  daySays: 'Today, Thursday 3 September',
  takings: [{ label: 'Bills', amount: money(36_000, '360.00') }],
  drawer: [{ label: 'Opening float', amount: money(200_000, '2000.00') }],
  expected: money(236_000, '2360.00'),
  denominations: [
    { value: 50_000, label: '500', count: 0, total: money(0, '0.00') },
    { value: 1_000, label: '10', count: 0, total: money(0, '0.00') },
  ],
  counted: money(0, '0.00'),
  variance: money(-236_000, '-2360.00'),
  varianceSays: 'Short by 2360.00.',
  varianceKind: 'short',
  needsReason: true,
  reasonSays: 'The drawer is out by more than 20.00, so this needs a reason.',
  reason: '',
  countedSays: '',
  mayCount: true,
  tillsSay: '',
};

beforeEach(() => {
  call.mockReset();
  call.mockImplementation((command: string, args?: { holidays?: string[] | null }) => {
    if (command === 'day_state') {
      return Promise.resolve(args?.holidays && args.holidays.length === 0 ? bothClosed : gate);
    }
    if (command === 'close_pending') return Promise.resolve(done);
    if (command === 'days') return Promise.resolve(days);
    if (command === 'count_cash') return Promise.resolve(drawer);
    if (command === 'close_day' || command === 'mark_holiday' || command === 'unmark_holiday' || command === 'reopen_day') {
      return Promise.resolve(days);
    }
    return Promise.resolve(null);
  });
});
afterEach(cleanup);

function openGate(state: DayStateView = gate) {
  const onChange = vi.fn();
  const onSignOut = vi.fn();
  const onEscape = vi.fn();
  render(
    <ToastProvider>
      <DayGate state={state} onChange={onChange} onEscape={onEscape} onSignOut={onSignOut} />
    </ToastProvider>,
  );
  return { onChange, onSignOut, onEscape };
}

it('draws one row per open day, preselected the way Rust suggested, with no way to dismiss it', () => {
  openGate();
  // The title is the sentence Rust wrote.
  expect(screen.getByRole('dialog', { name: '2 days were never closed.' })).toBeTruthy();
  expect(screen.getByText('Tuesday 1 September')).toBeTruthy();
  expect(screen.getByText('24120.00')).toBeTruthy();
  expect(screen.getByText('6120.00')).toBeTruthy();

  const tuesday = screen.getByRole('group', { name: 'What to do with Tuesday 1 September' });
  const wednesday = screen.getByRole('group', { name: 'What to do with Wednesday 2 September' });
  expect(tuesday.querySelector('[aria-pressed="true"]')?.textContent).toBe('Close');
  expect(wednesday.querySelector('[aria-pressed="true"]')?.textContent).toBe('Holiday');
  // A day with bills cannot be switched to Holiday.
  expect((tuesday.querySelector('button:last-child') as HTMLButtonElement).disabled).toBe(true);

  // The one button carries the words Rust wrote — and there is no close button.
  expect(screen.getByRole('button', { name: 'Close 1 day and mark 1 holiday' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: /cancel|dismiss|later/i })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Sign out' })).toBeNull();
});

it('asks Rust again when a switch moves, and sends the choice with the press', async () => {
  const { onChange } = openGate();
  const wednesday = screen.getByRole('group', { name: 'What to do with Wednesday 2 September' });
  fireEvent.click(wednesday.querySelector('button:first-child') as HTMLButtonElement);
  await waitFor(() => expect(call).toHaveBeenCalledWith('day_state', { holidays: [] }));
  await waitFor(() => expect(onChange).toHaveBeenCalledWith(bothClosed));

  fireEvent.click(screen.getByRole('button', { name: 'Close 1 day and mark 1 holiday' }));
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('close_pending', { holidays: ['2026-09-02'] }),
  );
  await waitFor(() => expect(onChange).toHaveBeenCalledWith(done));
});

it('offers only the way out to somebody who may not close a day', () => {
  const { onSignOut } = openGate({
    ...gate,
    mayAct: false,
    blockedSays: 'Closing a day needs permission, and Priya does not have it. Ask somebody who can, or sign out.',
  });
  expect(screen.getByText(/Priya does not have it/)).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Close 1 day and mark 1 holiday' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
  expect(onSignOut).toHaveBeenCalled();
});

it('shows the open-order sentence instead of a choice, and the way past the gate', () => {
  const { onEscape } = openGate({
    ...gate,
    pending: [
      {
        ...tuesday,
        openOrders: ['Table 7 #12'],
        openSays: '1 order is still open: Table 7 #12. Settle or cancel them before this day can be closed.',
      },
    ],
    actionLabel: '',
    escapeLabel: 'Finish the open orders first',
  });
  expect(screen.getByText(/Table 7 #12/)).toBeTruthy();
  expect(screen.queryByRole('group', { name: /What to do with/ })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Finish the open orders first' }));
  expect(onEscape).toHaveBeenCalled();
});

function openDays() {
  return render(
    <ToastProvider>
      <Days />
    </ToastProvider>,
  );
}

it('lists the last days with the chip and the sentence Rust wrote, and closes today in one press', async () => {
  openDays();
  await waitFor(() => expect(screen.getByText('Tuesday 1 September')).toBeTruthy());
  expect(screen.getByText('Holiday, marked 3 Sep, 9:02 am by Meena.')).toBeTruthy();
  expect(screen.getAllByText('Holiday').length).toBeGreaterThan(0);
  expect(screen.getByText('Closed')).toBeTruthy();

  fireEvent.click(screen.getByRole('button', { name: 'Close today' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('close_day', { day: '2026-09-03' }));

  // A holiday can be taken back from its row.
  fireEvent.click(screen.getByRole('button', { name: 'Not a holiday' }));
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('unmark_holiday', { days: ['2026-09-02'] }),
  );
});

it('opens a closed day again only with a reason', async () => {
  openDays();
  await waitFor(() => expect(screen.getAllByRole('button', { name: 'Open again' }).length).toBe(2));
  // The second locked row is Tuesday.
  const [, tuesdays] = screen.getAllByRole('button', { name: 'Open again' });
  fireEvent.click(tuesdays as HTMLElement);
  fireEvent.change(screen.getByLabelText('Why?'), { target: { value: 'a bill was missed' } });
  fireEvent.click(screen.getByRole('button', { name: 'Open it' }));
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('reopen_day', { day: '2026-09-01', reason: 'a bill was missed' }),
  );
});

it('counts the drawer through Rust and writes it without locking anything', async () => {
  openDays();
  await waitFor(() => expect(screen.getByText('2360.00')).toBeTruthy());
  expect(screen.getByText('Short by 2360.00.')).toBeTruthy();
  // The signed figure is never put on screen on its own.
  expect(screen.queryByText('-2360.00')).toBeNull();

  fireEvent.change(screen.getByLabelText('How many 500 notes'), { target: { value: '4' } });
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('count_cash', { counts: [{ value: 50_000, count: 4 }] }),
  );

  fireEvent.change(screen.getByLabelText('Why is the drawer out?'), {
    target: { value: 'paid the vegetable man' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Write the count' }));
  await waitFor(() => expect(screen.getByText('Write it without printing')).toBeTruthy());
  fireEvent.click(screen.getByRole('button', { name: 'Write it without printing' }));
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('count_drawer', {
      counts: [{ value: 50_000, count: 4 }],
      reason: 'paid the vegetable man',
      print: false,
    }),
  );
});
