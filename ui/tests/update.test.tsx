/** The update: the same dialog, offered by the counter and by the Account page. */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { readFileSync } from 'node:fs';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => false,
  isUiError: (cause: unknown) => typeof cause === 'object' && cause !== null && 'code' in cause,
  subscribe: () => Promise.resolve(() => undefined),
}));

const { UpdateOffer } = await import('../src/account/Update');
const { Version } = await import('../src/account/Version');
const { ToastProvider } = await import('../src/kit');

const shelf = {
  available: '1.7.0',
  notes: 'Faster kitchen tickets.',
  downloaded: false,
  previous: '1.6.14',
  daysOnThisVersion: 3,
  running: '1.6.15',
  isDevBuild: false,
};

beforeEach(() => {
  call.mockReset();
  call.mockImplementation((command: string) => {
    if (command === 'look_for_an_update') return Promise.resolve(shelf);
    if (command === 'install_update') return Promise.resolve('Magic Bill is closing.');
    return Promise.resolve(null);
  });
});
afterEach(cleanup);

function offer(version: string | null) {
  return render(
    <ToastProvider>
      <UpdateOffer version={version} />
    </ToastProvider>,
  );
}

it('offers the version the counter found, without anybody opening Account', async () => {
  offer('1.7.0');
  expect(await screen.findByText('Version 1.7.0 is ready')).toBeTruthy();
  expect(screen.getByText('Faster kitchen tickets.')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: /Download and install/ }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('install_update'));
});

it('says nothing when there is no version waiting', async () => {
  offer(null);
  await waitFor(() => expect(call).not.toHaveBeenCalledWith('look_for_an_update'));
  expect(screen.queryByText(/is ready/)).toBeNull();
});

/** Later is not "never": the bell and the Account page still hold it. */
it('closes on Later and does not come back on its own', async () => {
  const { rerender } = offer('1.7.0');
  fireEvent.click(await screen.findByRole('button', { name: 'Later' }));
  expect(screen.queryByText('Version 1.7.0 is ready')).toBeNull();

  // The shelf is read again every six hours and finds the same version.
  rerender(
    <ToastProvider>
      <UpdateOffer version="1.7.0" />
    </ToastProvider>,
  );
  await waitFor(() => expect(screen.queryByText('Version 1.7.0 is ready')).toBeNull());
});

/** A version found while the counter is open is offered, the same as one found at start-up. */
it('offers a newer version that arrives while the counter is open', async () => {
  const { rerender } = offer('1.7.0');
  fireEvent.click(await screen.findByRole('button', { name: 'Later' }));
  rerender(
    <ToastProvider>
      <UpdateOffer version="1.7.1" />
    </ToastProvider>,
  );
  expect(await screen.findByText('Version 1.7.1 is ready')).toBeTruthy();
});

it('installs from the Account page through the same dialog', async () => {
  render(
    <ToastProvider>
      <Version />
    </ToastProvider>,
  );
  fireEvent.click(await screen.findByRole('button', { name: /Install 1.7.0/ }));
  expect(await screen.findByText('Version 1.7.0 is ready')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: /Download and install/ }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('install_update'));
});

/** The dialog is written once. A second copy is how two of them drift apart. */
it('is drawn in one place', () => {
  const version = readFileSync('src/account/Version.tsx', 'utf8');
  expect(version).not.toContain('is ready`');
  expect(version).toContain("from './Update'");
});

/** Only somebody who may install it is asked about it. */
it('is offered by the shell only to whoever can install it', () => {
  const shell = readFileSync('src/shell/Shell.tsx', 'utf8');
  expect(shell).toContain("held.includes('settings.store') ? <UpdateOffer");
});
