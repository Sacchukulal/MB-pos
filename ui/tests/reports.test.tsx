/** The reports screen. */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
  // The real one, because the test below is about the difference it makes.
  isLicenceRefusal: (cause: unknown) =>
    typeof cause === 'object' &&
    cause !== null &&
    'code' in cause &&
    String((cause as { code: unknown }).code).startsWith('licence.'),
}));

const { Reports } = await import('../src/reports/Reports');
const { ToastProvider } = await import('../src/kit');

import type { ReportListView } from '../src/ipc/generated/ReportListView';
import type { ReportView } from '../src/ipc/generated/ReportView';

const list: ReportListView = {
  periods: [
    { label: 'Today', from: '2026-08-09', to: '2026-08-09' },
    { label: 'This month', from: '2026-08-01', to: '2026-08-09' },
  ],
  reports: [
    { id: 'sales_day', title: 'Sales by day', group: 'Sales' },
    { id: 'tax_rate', title: 'Tax, rate-wise', group: 'Tax' },
    // A report invented for this test.
    { id: 'wastage', title: 'Wastage by kitchen', group: 'Kitchen' },
  ],
};

/** A report whose columns this file has never seen either. */
const invented: ReportView = {
  id: 'wastage',
  title: 'Wastage by kitchen',
  subtitle: '2026-08-01 to 2026-08-09 · 9 days',
  columns: [
    { header: 'Kitchen', numeric: false },
    { header: 'Thrown away', numeric: true },
    { header: 'Cost', numeric: true },
  ],
  rows: [
    ['Main', '14', '1,240.00'],
    ['Tandoor', '3', '410.00'],
  ],
  totals: ['Total', '17', '1,650.00'],
  compare: {
    period: '2026-07-23 to 2026-07-31 · 9 days',
    before: '2,000.00',
    now: '1,650.00',
    summary: 'Down 17% on the 9 days before (2,000.00).',
    direction: 'down',
  },
  notes: ['Two kitchens have no waste log for this period.'],
};

function answer(command: string) {
  if (command === 'report_list') return Promise.resolve(list);
  if (command === 'dashboard') {
    return Promise.resolve({
      title: 'Today, so far — 2026-08-09',
      stats: [],
      compare: null,
      attention: [],
      quiet: 'Nothing needs you.',
    });
  }
  if (command === 'report') return Promise.resolve(invented);
  if (command === 'days') {
    return Promise.resolve({
      today: '2026-08-09',
      todaySays: 'Today, Sunday 9 August',
      todayState: 'open',
      todayClosedSays: '',
      mayAct: true,
      carrySays: '',
      days: [],
      upcoming: [],
      mayPlanHoliday: true,
    });
  }
  if (command === 'report_csv' || command === 'report_pdf') {
    return Promise.resolve({
      path: 'C:/Users/x/Documents/Magic Bill reports/Wastage.csv',
      message: 'Saved as Wastage.csv, in your Documents folder.',
    });
  }
  return Promise.resolve(null);
}

beforeEach(() => {
  call.mockReset();
  call.mockImplementation((command: string) => answer(command));
});
afterEach(cleanup);

function open() {
  return render(
    <ToastProvider>
      <Reports />
    </ToastProvider>,
  );
}

/** The screen opens on the dashboard. */
async function openOnAReport() {
  open();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Sales by day' })).toBeTruthy());
  fireEvent.click(screen.getByRole('button', { name: 'Sales by day' }));
}

it('renders a report it has never heard of, columns and all', async () => {
  open();
  // The list groups itself from what Rust sent, including a group this file invented.
  await waitFor(() => expect(screen.getByRole('button', { name: 'Wastage by kitchen' })).toBeTruthy());
  // The group heading came from the data too.
  expect(screen.getByRole('heading', { name: 'Kitchen' })).toBeTruthy();

  fireEvent.click(screen.getByRole('button', { name: 'Wastage by kitchen' }));

  await waitFor(() => expect(screen.getByText('Thrown away')).toBeTruthy());
  expect(screen.getByText('Tandoor')).toBeTruthy();
  expect(screen.getByText('1,240.00')).toBeTruthy();
  // The totals row is present and is not part of the table body.
  expect(screen.getByText('1,650.00')).toBeTruthy();
  // The note came written.
  expect(screen.getByText('Two kitchens have no waste log for this period.')).toBeTruthy();
});

it('shows the comparison as the sentence Rust wrote, not as a percentage it worked out', async () => {
  await openOnAReport();
  await waitFor(() => expect(screen.getByText(invented.compare!.summary)).toBeTruthy());
  // And it names what it compared against, so the figure can be checked.
  expect(screen.getByText(/2026-07-23 to 2026-07-31/)).toBeTruthy();
});

it('asks Rust for the period rather than working out what today is', async () => {
  await openOnAReport();
  // Exactly one "Today" — the period preset.
  await waitFor(() => expect(screen.getByRole('button', { name: 'Today' })).toBeTruthy());
  // The first preset is used on open — and it came down the wire, because the shop's "today"
  // starts at 5 am and a browser does not know that.
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('report', {
      id: 'sales_day',
      period: { from: '2026-08-09', to: '2026-08-09' },
    }),
  );

  fireEvent.click(screen.getByRole('button', { name: 'This month' }));
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('report', {
      id: 'sales_day',
      period: { from: '2026-08-01', to: '2026-08-09' },
    }),
  );
});

it('exports the report on screen, through Rust, and says where it went', async () => {
  await openOnAReport();
  await waitFor(() => expect(screen.getByText('Save as CSV')).toBeTruthy());

  fireEvent.click(screen.getByRole('button', { name: 'Save as CSV' }));
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('report_csv', {
      id: 'sales_day',
      period: { from: '2026-08-09', to: '2026-08-09' },
    }),
  );
  // The whole sentence, written in Rust.
  await waitFor(() =>
    expect(screen.getByText('Saved as Wastage.csv, in your Documents folder.')).toBeTruthy(),
  );

  fireEvent.click(screen.getByRole('button', { name: 'Save as PDF' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('report_pdf', expect.anything()));
});

/** A licence refusal is an ANSWER, and it stays on the screen. */
it('says why, on the screen, when the licence does not cover reports', async () => {
  const refusal = {
    code: 'licence.not_operating',
    message:
      'This computer has no licence yet. You can bill and print — enter your ' +
      'licence key, or start a free trial.',
    detail: 'reports · never-activated',
  };
  call.mockImplementation((command: string) =>
    command === 'report_list' ? Promise.reject(refusal) : answer(command),
  );

  const go = vi.fn();
  render(
    <ToastProvider>
      <Reports onGoTo={go} />
    </ToastProvider>,
  );

  expect(await screen.findByText('This part needs a licence')).toBeTruthy();
  expect(screen.getByText(/no licence yet/)).toBeTruthy();
  // And the way out is on the same screen, not somewhere they have to find.
  fireEvent.click(screen.getByRole('button', { name: 'Open Account' }));
  expect(go).toHaveBeenCalledWith('account');
});

it('puts the days beside the dashboard, and opens them without asking for a report', async () => {
  open();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Days' })).toBeTruthy());
  fireEvent.click(screen.getByRole('button', { name: 'Days' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('days'));
  // Today's state is the subtitle, and its one press is right there.
  await waitFor(() => expect(screen.getByText('Today, Sunday 9 August')).toBeTruthy());
  expect(screen.getByRole('button', { name: 'Close today' })).toBeTruthy();
  // The old entry is gone with the old model.
  expect(screen.queryByRole('button', { name: 'Close the day' })).toBeNull();
});
