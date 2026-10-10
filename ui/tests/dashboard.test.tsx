import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { DashboardView } from '../src/ipc/generated/DashboardView';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  isUiError: (cause: unknown) => !!cause && typeof cause === 'object' && 'message' in cause,
  isLicenceRefusal: (cause: { code?: string }) => cause?.code?.startsWith('licence.'),
}));
import { Dashboard, DashboardPage } from '../src/reports/Dashboard';
import { MayProvider } from '../src/shell/permissions';

const presets = [
  { label: 'Today', from: '2026-09-28', to: '2026-09-28' },
  { label: 'Yesterday', from: '2026-09-27', to: '2026-09-27' },
] as const;
const view: DashboardView = {
  title: 'Today, so far', from: presets[0].from, to: presets[0].to,
  stats: [{ label: 'Takings', value: '2,400.00', note: '12 bills' }],
  compare: null, attention: [], quiet: 'Nothing needs you.',
  charts: [{ id: 'sales', title: 'Sales by hour', kind: 'columns', note: '', empty: 'Nothing sold.',
    points: [{ label: '9 am', value: '800.00', note: '4 bills', share: 500, hue: 0 },
      { label: '10 am', value: '1,600.00', note: '8 bills', share: 1000, hue: 0 }] }],
};
beforeEach(() => { localStorage.clear(); call.mockReset(); call.mockImplementation((command: string) => Promise.resolve(command === 'report_list' ? { periods: presets } : view)); });
afterEach(cleanup);

it('clears paid dashboard data after expiry and refreshes access after renewal', async () => {
  render(<DashboardPage />);
  await screen.findByText('2,400.00');
  call.mockRejectedValue({ code: 'licence.not_operating', message: 'Reports need an active licence.' });
  fireEvent(window, new Event('focus'));
  await screen.findByText('Reports need an active licence.');
  expect(screen.queryByText('2,400.00')).toBeNull();
  call.mockImplementation((command: string) => Promise.resolve(command === 'report_list' ? { periods: presets } : view));
  fireEvent(window, new Event('focus'));
  await screen.findByText('2,400.00');
});

it('does not restore a stale dashboard response after access was refused', async () => {
  let finish!: (fresh: DashboardView) => void;
  call.mockImplementationOnce(() => new Promise<DashboardView>((resolve) => { finish = resolve; }));
  render(<Dashboard presets={presets} />);
  call.mockRejectedValue({ code: 'licence.not_operating', message: 'Reports need an active licence.' });
  fireEvent(window, new Event('focus'));
  await screen.findByText('Reports need an active licence.');
  await act(async () => finish(view));
  expect(screen.queryByText('2,400.00')).toBeNull();
});

it('renders real figures, switches chart styles and exposes point details to the keyboard', async () => {
  render(<Dashboard presets={presets} />);
  expect(await screen.findByText('2,400.00')).toBeTruthy();
  fireEvent.focus(screen.getByRole('img', { name: '9 am · 800.00 · 4 bills' }));
  expect(screen.getByText('800.00')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Bars' }));
  expect(screen.getByRole('button', { name: 'Bars' }).getAttribute('aria-pressed')).toBe('true');
  expect(document.querySelectorAll('.mb-trend__bar')).toHaveLength(2);
});

it('ignores a late response from an older period and remembers the selected preset', async () => {
  let resolveOld!: (value: DashboardView) => void;
  call.mockImplementationOnce(() => new Promise<DashboardView>((resolve) => { resolveOld = resolve; }));
  render(<Dashboard presets={presets} />);
  fireEvent.click(screen.getByRole('button', { name: 'Yesterday' }));
  await screen.findByText('2,400.00');
  await act(async () => resolveOld({ ...view, stats: [{ label: 'Takings', value: 'OLD', note: '' }] }));
  expect(screen.queryByText('OLD')).toBeNull();
  expect(call).toHaveBeenLastCalledWith('dashboard', { period: { from: presets[1].from, to: presets[1].to } });
  cleanup();
  render(<Dashboard presets={presets} />);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Yesterday' }).getAttribute('aria-pressed')).toBe('true'));
});

it('recovers from a loading failure using Retry', async () => {
  call.mockRejectedValueOnce(new Error('network'));
  render(<Dashboard presets={presets} />);
  await screen.findByRole('alert');
  fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
  expect(await screen.findByText('2,400.00')).toBeTruthy();
});

it('keeps the chart mounted during refresh and replaces figures when the request finishes', async () => {
  render(<Dashboard presets={presets} />);
  await screen.findByText('2,400.00');
  fireEvent.click(screen.getByRole('button', { name: 'Bars' }));
  const plot = document.querySelector('.mb-trend__plot');
  let finish!: (fresh: DashboardView) => void;
  call.mockImplementationOnce(() => new Promise<DashboardView>((resolve) => { finish = resolve; }));
  fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
  expect(screen.getByText('2,400.00')).toBeTruthy();
  expect(screen.getByText('Updating figures…')).toBeTruthy();
  expect(document.querySelector('.mb-trend__plot')).toBe(plot);
  await act(async () => finish({ ...view, stats: [{ label: 'Takings', value: '3,200.00', note: '16 bills' }] }));
  expect(screen.getByText('3,200.00')).toBeTruthy();
  expect(screen.queryByText('Updating figures…')).toBeNull();
  expect(screen.getByRole('button', { name: 'Bars' }).getAttribute('aria-pressed')).toBe('true');
});

it('removes stale figures if a refresh fails', async () => {
  render(<Dashboard presets={presets} />);
  await screen.findByText('2,400.00');
  call.mockRejectedValueOnce(new Error('Could not refresh'));
  fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
  await screen.findByRole('alert');
  expect(screen.queryByText('2,400.00')).toBeNull();
});

it('switches daily and hourly trends in one chart area for a longer period', async () => {
  call.mockResolvedValue({ ...view, charts: [
    { ...view.charts[0], id: 'trend', title: 'Sales by day' },
    { ...view.charts[0], id: 'hours' },
  ] });
  render(<Dashboard presets={presets} />);
  await screen.findByRole('heading', { name: 'Sales by day' });
  expect(document.querySelectorAll('.mb-trend__plot')).toHaveLength(1);
  fireEvent.click(screen.getByRole('button', { name: 'Sales by hour' }));
  expect(screen.getByRole('heading', { name: 'Sales by hour' })).toBeTruthy();
  expect(document.querySelectorAll('.mb-trend__plot')).toHaveLength(1);
});

it('clears old figures and avoids a request for a missing or reversed date range', async () => {
  render(<Dashboard presets={presets} />);
  await screen.findByText('2,400.00');
  const requests = call.mock.calls.length;
  fireEvent.change(screen.getByLabelText('From'), { target: { value: '2026-09-30' } });
  expect(await screen.findByText('Choose an end date on or after the start date.')).toBeTruthy();
  expect(screen.queryByText('2,400.00')).toBeNull();
  fireEvent.change(screen.getByLabelText('From'), { target: { value: '' } });
  expect(await screen.findByText('Choose a start and end date to see your overview.')).toBeTruthy();
  expect(call).toHaveBeenCalledTimes(requests);
});

it('shows a licence refusal with a route to Account', async () => {
  call.mockRejectedValueOnce({ code: 'licence.required', message: 'Activate reports to see your overview.' });
  const go = vi.fn();
  render(<DashboardPage onGoTo={go} />);
  await screen.findByText('Activate reports to see your overview.');
  fireEvent.click(screen.getByRole('button', { name: 'Open Account' }));
  expect(go).toHaveBeenCalledWith('account');
});

it('keeps empty charts honest and hides billing shortcuts without billing permission', async () => {
  call.mockResolvedValue({ ...view, charts: [{ ...view.charts[0], points: [] }] });
  const go = vi.fn();
  render(<MayProvider held={['reports.view']}><Dashboard presets={presets} onGoTo={go} /></MayProvider>);
  expect(await screen.findByText('Nothing sold.')).toBeTruthy();
  expect(document.querySelector('.mb-trend__plot')).toBeNull();
  expect(screen.queryByRole('button', { name: 'New bill' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Explore reports' }));
  expect(go).toHaveBeenCalledWith('reports');
});
