/** The Backup panel: the folders, the schedule, the copies and the list. */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => false,
  isUiError: (cause: unknown) => typeof cause === 'object' && cause !== null && 'code' in cause,
  subscribe: () => Promise.resolve(() => undefined),
}));

const { Backup } = await import('../src/account/Backup');
const { shorten } = await import('../src/account/Lines');
const { ToastProvider } = await import('../src/kit');

import type { BackupRowView } from '../src/ipc/generated/BackupRowView';
import type { BackupView } from '../src/ipc/generated/BackupView';

function row(n: number): BackupRowView {
  return {
    path: `C:\\Shop\\backups\\magicbill-${n}.db`,
    name: `magicbill-${n}.db`,
    takenAt: `${n} Sep, 11:00 pm`,
    size: '1.2 MB',
    checked: 'Checked',
    checkedOk: true,
  };
}

const view: BackupView = {
  shopFolder: 'C:\\Shop',
  folder: 'C:\\Shop\\backups',
  folderIsDefault: true,
  copies: [
    {
      path: 'G:\\My Drive\\Magic Bill backups',
      kind: 'Google Drive',
      name: '',
      reachable: true,
      last: '9 Sep, 11:00 pm',
    },
    {
      path: 'E:\\Magic Bill backups',
      kind: 'Pen drive',
      name: 'SANDISK',
      reachable: false,
      last: '',
    },
  ],
  suggested: [
    {
      path: 'C:\\Users\\Anna\\OneDrive\\Magic Bill backups',
      kind: 'OneDrive',
      name: '',
      reachable: true,
      last: '',
    },
  ],
  schedule: 'daily',
  dailyAt: '23:00',
  scheduleSays: 'Every day at 11:00 pm',
  backups: [9, 8, 7, 6, 5, 4, 3, 2].map(row),
  last: '9 Sep, 11:00 pm, checked',
  tone: 'ok',
  restoreWaiting: null,
};

function show(state: BackupView = view) {
  call.mockImplementation((command: string) =>
    command === 'backup_status' ? Promise.resolve(state) : Promise.resolve(state),
  );
  return render(
    <ToastProvider>
      <Backup />
    </ToastProvider>,
  );
}

beforeEach(() => call.mockReset());
afterEach(cleanup);

it('shows the folders, the schedule and every copy with its state', async () => {
  show();
  expect(await screen.findByText('Last backup 9 Sep, 11:00 pm, checked')).toBeTruthy();
  expect(screen.getByText('Every day at 11:00 pm')).toBeTruthy();
  expect(screen.getByTitle('C:\\Shop\\backups')).toBeTruthy();
  expect(screen.getByText('Google Drive')).toBeTruthy();
  expect(screen.getByText('Copied 9 Sep, 11:00 pm')).toBeTruthy();
  expect(screen.getByText('Pen drive (SANDISK)')).toBeTruthy();
  expect(screen.getByText('Not reachable')).toBeTruthy();
  // The daily time box shows only for the daily schedule.
  expect((screen.getByLabelText('Every day at') as HTMLInputElement).value).toBe('23:00');
});

it('offers the places it found and adds one with a press', async () => {
  show();
  fireEvent.click(await screen.findByRole('button', { name: 'OneDrive' }));
  await waitFor(() => {
    expect(call).toHaveBeenCalledWith('add_backup_copy', {
      folder: 'C:\\Users\\Anna\\OneDrive\\Magic Bill backups',
    });
  });
});

it('saves the schedule as soon as it is picked', async () => {
  show();
  fireEvent.change(await screen.findByLabelText('Schedule'), { target: { value: 'hourly' } });
  await waitFor(() => {
    expect(call).toHaveBeenCalledWith('set_backup_schedule', {
      schedule: 'hourly',
      dailyAt: '23:00',
    });
  });
});

it('shows a few backups and opens out to all of them', async () => {
  show();
  expect(await screen.findByText('9 Sep, 11:00 pm')).toBeTruthy();
  expect(screen.queryByText('2 Sep, 11:00 pm')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Show all 8' }));
  expect(screen.getByText('2 Sep, 11:00 pm')).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Show fewer' })).toBeTruthy();
});

it('folds a long path down to its drive and last two folders', () => {
  expect(shorten('C:\\Shop\\backups')).toBe('C:\\Shop\\backups');
  expect(
    shorten('C:\\Users\\SACCHU\\AppData\\Local\\Temp\\claude\\scratchpad\\demo\\MagicBill\\backups'),
  ).toBe('C:\\…\\MagicBill\\backups');
});
