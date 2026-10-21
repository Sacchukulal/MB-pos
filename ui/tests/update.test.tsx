/** The update: the same dialog, offered by the counter and by the Updates page. */

import { act, cleanup, fireEvent, render, renderHook, screen, waitFor, within } from '@testing-library/react';
import { readFileSync } from 'node:fs';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Pushed } from '../src/ipc/generated/Pushed';

const call = vi.fn();
const updateListeners = new Set<(message: Pushed) => void>();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => false,
  isUiError: (cause: unknown) => typeof cause === 'object' && cause !== null && 'code' in cause,
  subscribe: (listener: (message: Pushed) => void) => {
    updateListeners.add(listener);
    return Promise.resolve(() => { updateListeners.delete(listener); });
  },
}));

const { UpdateOffer, useUpdateRun } = await import('../src/account/Update');
const { Version } = await import('../src/account/Version');
const { ToastProvider } = await import('../src/kit');
const { MayProvider } = await import('../src/shell/permissions');

const shelf = {
  available: '1.7.0',
  notes: 'Faster kitchen tickets.',
  downloaded: false,
  previous: '1.6.14',
  daysOnThisVersion: 3,
  running: '1.6.15',
  isDevBuild: false,
  lastFailure: null,
};

beforeEach(() => {
  call.mockReset();
  window.localStorage.clear();
  updateListeners.clear();
  call.mockImplementation((command: string) => {
    if (command === 'look_for_an_update') return Promise.resolve(shelf);
    if (command === 'install_update') return Promise.resolve('Magic Bill is closing.');
    return Promise.resolve(null);
  });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

function offer(version: string | null) {
  return render(
    <ToastProvider>
      <UpdateOffer version={version} />
    </ToastProvider>,
  );
}

it('offers the version the counter found, without anybody opening Updates', async () => {
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

/** Later is not "never": the bell and the Updates page still hold it. */
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
  call.mockResolvedValue({ ...shelf, available: '1.7.1' });
  rerender(
    <ToastProvider>
      <UpdateOffer version="1.7.1" />
    </ToastProvider>,
  );
  expect(await screen.findByText('Version 1.7.1 is ready')).toBeTruthy();
});

it('keeps a newer offer from replacing an update in progress or its installer handoff', async () => {
  let finish!: (says: string) => void;
  const installing = new Promise<string>((resolve) => { finish = resolve; });
  let available = '1.8.2';
  call.mockImplementation((command: string) => command === 'install_update'
    ? installing : Promise.resolve({ ...shelf, available }));
  const { rerender } = offer('1.8.2');
  fireEvent.click(await screen.findByRole('button', { name: /Download and install/ }));
  expect(screen.getByRole('dialog', { name: 'Updating to 1.8.2' })).toBeTruthy();

  available = '1.8.3';
  rerender(<ToastProvider><UpdateOffer version="1.8.3" /></ToastProvider>);
  await waitFor(() => expect(call.mock.calls.filter(([name]) => name === 'look_for_an_update')).toHaveLength(2));
  expect(screen.queryByRole('button', { name: /Download and install/ })).toBeNull();
  expect(screen.getByRole('progressbar')).toBeTruthy();

  await act(async () => { finish('Magic Bill is closing.'); });
  available = '1.8.4';
  rerender(<ToastProvider><UpdateOffer version="1.8.4" /></ToastProvider>);
  await waitFor(() => expect(call.mock.calls.filter(([name]) => name === 'look_for_an_update')).toHaveLength(3));
  expect(screen.getByRole('dialog', { name: 'Installing' })).toBeTruthy();
  expect(call.mock.calls.filter(([name]) => name === 'install_update')).toHaveLength(1);
});

it('rejects repeated install and rollback clicks until restart', async () => {
  const { result } = renderHook(() => useUpdateRun());
  await act(async () => {
    result.current.install('1.8.3');
    result.current.install('1.8.3');
    result.current.goBack('1.8.0');
  });
  expect(call.mock.calls.filter(([name]) => name === 'install_update')).toHaveLength(1);
  expect(call).not.toHaveBeenCalledWith('go_back_a_version');
  act(() => {
    result.current.close();
    result.current.offer('1.8.4', '', false);
    result.current.install('1.8.4');
  });
  expect(result.current.step?.kind).toBe('closing');
  expect(call.mock.calls.filter(([name]) => name === 'install_update')).toHaveLength(1);
});

it('allows another attempt after an installation error', async () => {
  call.mockRejectedValueOnce({ code: 'update.missing', message: 'Download it again.' });
  const { result } = renderHook(() => useUpdateRun());
  await act(async () => { result.current.install('1.8.3'); });
  expect(result.current.step?.kind).toBe('failed');
  await act(async () => { result.current.install('1.8.3'); });
  expect(result.current.step?.kind).toBe('closing');
  expect(call.mock.calls.filter(([name]) => name === 'install_update')).toHaveLength(2);
});

it('recovers an accepted handoff when the backend watchdog reports failure', async () => {
  const { result } = renderHook(() => useUpdateRun());
  await act(async () => { result.current.install('1.8.5'); });
  expect(result.current.step?.kind).toBe('closing');
  call.mockImplementation((command: string) => command === 'look_for_an_update'
    ? Promise.resolve({ ...shelf, lastFailure: { version: '1.8.5', message: 'The updater stopped before installation.' } })
    : Promise.resolve('Magic Bill is closing.'));
  await act(async () => {
    for (const listener of updateListeners) listener({ kind: 'version', available: '1.8.5' });
  });
  expect(result.current.step).toEqual({ kind: 'failed', says: 'The updater stopped before installation.' });
  act(() => { result.current.close(); });
  expect(result.current.step).toBeNull();
  await act(async () => { result.current.install('1.8.5'); });
  expect(result.current.step?.kind).toBe('closing');
  expect(call.mock.calls.filter(([name]) => name === 'install_update')).toHaveLength(2);
});

it.each(['1.8.4', '1.8.6'])('ignores a failure for unrelated version %s during a handoff', async (version) => {
  const { result } = renderHook(() => useUpdateRun());
  await act(async () => { result.current.install('1.8.5'); });
  call.mockResolvedValue({ ...shelf, lastFailure: { version, message: 'An unrelated attempt failed.' } });
  await act(async () => {
    for (const listener of updateListeners) listener({ kind: 'version', available: version });
  });
  expect(result.current.step?.kind).toBe('closing');
});

it('ignores an earlier same-version failure until the new backend attempt starts', async () => {
  let finish!: (says: string) => void;
  call.mockImplementation((command: string) => command === 'install_update'
    ? new Promise<string>((resolve) => { finish = resolve; })
    : Promise.resolve({ ...shelf, lastFailure: { version: '1.8.5', message: 'An earlier attempt failed.' } }));
  const { result } = renderHook(() => useUpdateRun());
  act(() => { result.current.install('1.8.5'); });
  await act(async () => {
    for (const listener of updateListeners) listener({ kind: 'version', available: '1.8.5' });
  });
  expect(result.current.step?.kind).toBe('busy');
  expect(call).not.toHaveBeenCalledWith('look_for_an_update');
  await act(async () => { finish('Magic Bill is closing.'); });
  expect(result.current.step?.kind).toBe('closing');
});

it('keeps a reported failure when a delayed handoff response arrives afterward', async () => {
  let finish!: (says: string) => void;
  call.mockImplementation((command: string) => command === 'install_update'
    ? new Promise<string>((resolve) => { finish = resolve; })
    : Promise.resolve({ ...shelf, lastFailure: { version: '1.8.5', message: 'The worker exited.' } }));
  const { result } = renderHook(() => useUpdateRun());
  act(() => { result.current.install('1.8.5'); });
  await act(async () => {
    for (const listener of updateListeners) {
      listener({ kind: 'update', version: '1.8.5', stage: 'installing', percent: 100, bytes: 1, total: 1 });
      listener({ kind: 'version', available: '1.8.5' });
    }
  });
  expect(result.current.step).toEqual({ kind: 'failed', says: 'The worker exited.' });
  await act(async () => { finish('Magic Bill is closing.'); });
  expect(result.current.step).toEqual({ kind: 'failed', says: 'The worker exited.' });
});

it('does not apply a delayed old failure snapshot to a new manual attempt', async () => {
  const failure = { ...shelf, lastFailure: { version: '1.8.5', message: 'The previous worker exited.' } };
  let finishSnapshot!: (state: typeof failure) => void;
  let reads = 0;
  call.mockImplementation((command: string) => {
    if (command === 'install_update') return Promise.resolve('Magic Bill is closing.');
    reads += 1;
    return reads === 1
      ? new Promise<typeof failure>((resolve) => { finishSnapshot = resolve; })
      : Promise.resolve(failure);
  });
  const { result } = renderHook(() => useUpdateRun());
  await act(async () => { result.current.install('1.8.5'); });
  await act(async () => {
    for (const listener of updateListeners) {
      listener({ kind: 'version', available: '1.8.5' });
      listener({ kind: 'version', available: '1.8.5' });
    }
  });
  expect(result.current.step?.kind).toBe('failed');
  await act(async () => { result.current.install('1.8.5'); });
  await act(async () => { finishSnapshot(failure); });
  expect(result.current.step?.kind).toBe('closing');
});

it('installs from Updates without granting any business permissions', async () => {
  render(
    <ToastProvider>
      <MayProvider held={[]}>
        <Version />
      </MayProvider>
    </ToastProvider>,
  );
  fireEvent.click(await screen.findByRole('button', { name: /Install 1.7.0/ }));
  expect(screen.queryByRole('button', { name: 'Go back' })).toBeNull();
  expect(await screen.findByText('Version 1.7.0 is ready')).toBeTruthy();
  fireEvent.click(await screen.findByRole('button', { name: /Download and install/ }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('install_update'));
});

/** The dialog is written once. A second copy is how two of them drift apart. */
it('is drawn in one place', () => {
  const version = readFileSync('src/account/Version.tsx', 'utf8');
  expect(version).not.toContain('is ready`');
  expect(version).toContain("from './Update'");
});

it('keeps rollback available to staff with shop-settings permission', async () => {
  render(
    <ToastProvider>
      <MayProvider held={['settings.store']}>
        <Version />
      </MayProvider>
    </ToastProvider>,
  );
  fireEvent.click(await screen.findByRole('button', { name: 'Go back' }));
  fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Go back' }));
  await waitFor(() => expect(call).toHaveBeenCalledWith('go_back_a_version'));
});

it('allows a staff member to check for updates without reports permission', async () => {
  call.mockImplementation((command: string) => {
    if (command === 'look_for_an_update') return Promise.resolve({ ...shelf, available: null });
    if (command === 'check_for_update') return Promise.resolve(shelf);
    return Promise.resolve(null);
  });
  render(
    <ToastProvider>
      <MayProvider held={[]}>
        <Version />
      </MayProvider>
    </ToastProvider>,
  );
  fireEvent.click(await screen.findByRole('button', { name: 'Check for updates' }));
  expect(await screen.findByRole('dialog', { name: 'Version 1.7.0 is ready' })).toBeTruthy();
  expect(call).toHaveBeenCalledWith('check_for_update');
});

it('explains why billing must be saved and lets staff dismiss the refusal', async () => {
  call.mockImplementation((command: string) => command === 'install_update'
    ? Promise.reject({ code: 'update.unsaved_bill', message: 'Finish or save the current bill before updating.' })
    : Promise.resolve(shelf));
  offer('1.7.0');
  fireEvent.click(await screen.findByRole('button', { name: /Download and install/ }));
  expect(await screen.findByText('Finish or save the current bill before updating.')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'OK' }));
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('shows one progress dialog when Updates and a dismissed startup offer are both mounted', async () => {
  let finish!: (says: string) => void;
  const installing = new Promise<string>((resolve) => { finish = resolve; });
  call.mockImplementation((command: string) => command === 'install_update'
    ? installing : Promise.resolve(shelf));
  render(
    <ToastProvider>
      <UpdateOffer version="1.7.0" />
      <Version />
    </ToastProvider>,
  );
  fireEvent.click(await screen.findByRole('button', { name: 'Later' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Install 1.7.0' }));
  fireEvent.click(await screen.findByRole('button', { name: /Download and install/ }));
  act(() => {
    for (const listener of updateListeners) {
      listener({ kind: 'update', version: '1.7.0', stage: 'downloading', percent: 50, bytes: 50, total: 100 });
    }
  });
  expect(screen.getAllByRole('dialog')).toHaveLength(1);
  expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe('50');
  await act(async () => { finish('Magic Bill is closing.'); });
  expect(screen.getAllByRole('dialog')).toHaveLength(1);
});

it('keeps Later snoozed after the offer is unmounted and reopened', async () => {
  const first = offer('1.7.0');
  fireEvent.click(await screen.findByRole('button', { name: 'Later' }));
  first.unmount();
  offer('1.7.0');
  await act(async () => undefined);
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(call).not.toHaveBeenCalledWith('install_update');
});

it('offers the same version after its one-day snooze expires', async () => {
  let now = Date.now();
  vi.spyOn(Date, 'now').mockImplementation(() => now);
  const first = offer('1.7.0');
  fireEvent.click(await screen.findByRole('button', { name: 'Later' }));
  first.unmount();
  now += 25 * 60 * 60 * 1000;
  offer('1.7.0');
  expect(await screen.findByRole('dialog', { name: 'Version 1.7.0 is ready' })).toBeTruthy();
});

it('uses the current snapshot instead of an older version event', async () => {
  call.mockResolvedValue({ ...shelf, available: '1.7.1', notes: 'Current release.' });
  offer('1.7.0');
  expect(await screen.findByRole('dialog', { name: 'Version 1.7.1 is ready' })).toBeTruthy();
  expect(screen.queryByText('Version 1.7.0 is ready')).toBeNull();
});

it('does not offer a version withdrawn before its snapshot is read', async () => {
  call.mockResolvedValue({ ...shelf, available: null });
  offer('1.7.0');
  await act(async () => undefined);
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('withdraws an open offer when the backend reports no available version', async () => {
  const { rerender } = offer('1.7.0');
  await screen.findByRole('dialog', { name: 'Version 1.7.0 is ready' });
  rerender(<ToastProvider><UpdateOffer version={null} /></ToastProvider>);
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('ignores an in-flight snapshot after its version has been withdrawn', async () => {
  let finish!: (state: typeof shelf) => void;
  call.mockReturnValue(new Promise<typeof shelf>((resolve) => { finish = resolve; }));
  const { rerender } = offer('1.7.0');
  rerender(<ToastProvider><UpdateOffer version={null} /></ToastProvider>);
  await act(async () => { finish(shelf); });
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('does not manufacture a download prompt after its snapshot read fails', async () => {
  call.mockRejectedValue({ code: 'auth.locked', message: 'The screen is locked.' });
  offer('1.7.0');
  await act(async () => undefined);
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('keeps a failed attempt quiet across reopening and allows an explicit retry', async () => {
  let attempts = 0;
  call.mockImplementation((command: string) => {
    if (command === 'install_update') {
      attempts += 1;
      return attempts === 1
        ? Promise.reject({ code: 'update.missing', message: 'The installer is missing.' })
        : Promise.resolve('Magic Bill is closing.');
    }
    return Promise.resolve(shelf);
  });
  const first = offer('1.7.0');
  fireEvent.click(await screen.findByRole('button', { name: /Download and install/ }));
  await screen.findByRole('dialog', { name: 'Update failed' });
  first.unmount();
  offer('1.7.0');
  await act(async () => undefined);
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(attempts).toBe(1);

  render(<ToastProvider><Version /></ToastProvider>);
  fireEvent.click(await screen.findByRole('button', { name: 'Install 1.7.0' }));
  fireEvent.click(await screen.findByRole('button', { name: /Download and install/ }));
  await screen.findByRole('dialog', { name: 'Installing' });
  expect(attempts).toBe(2);
});

it('shows a recorded installer failure on Updates and requires a manual retry', async () => {
  call.mockResolvedValue({
    ...shelf,
    downloaded: true,
    lastFailure: { version: '1.7.0', message: 'The installer did not complete.' },
  });
  render(<ToastProvider><UpdateOffer version="1.7.0" /><Version /></ToastProvider>);
  expect(await screen.findByText(/The installer did not complete/)).toBeTruthy();
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(call).not.toHaveBeenCalledWith('install_update');
  fireEvent.click(await screen.findByRole('button', { name: 'Install 1.7.0' }));
  expect(await screen.findByRole('button', { name: /Install now/ })).toBeTruthy();
});

it('offers a new release even when the earlier release failed', async () => {
  call.mockResolvedValue({
    ...shelf,
    available: '1.7.1',
    lastFailure: { version: '1.7.0', message: 'The earlier installer did not complete.' },
  });
  offer('1.7.1');
  expect(await screen.findByRole('dialog', { name: 'Version 1.7.1 is ready' })).toBeTruthy();
});

it('allows a manual refresh while an update is already pending', async () => {
  call.mockImplementation((command: string) => command === 'check_for_update'
    ? Promise.resolve({ ...shelf, available: null }) : Promise.resolve(shelf));
  render(<ToastProvider><Version /></ToastProvider>);
  await screen.findByRole('button', { name: 'Install 1.7.0' });
  fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
  await screen.findByText('This is the newest version, 1.6.15.');
  expect(call).toHaveBeenCalledWith('check_for_update');
  expect(screen.queryByRole('button', { name: 'Install 1.7.0' })).toBeNull();
});

it('refreshes download status after launch failure before an explicit retry', async () => {
  let downloaded = false;
  call.mockImplementation((command: string) => {
    if (command === 'install_update') {
      downloaded = true;
      return Promise.reject({ code: 'update.launch', message: 'The installer could not start.' });
    }
    return Promise.resolve({ ...shelf, downloaded });
  });
  render(<ToastProvider><Version /></ToastProvider>);
  fireEvent.click(await screen.findByRole('button', { name: 'Install 1.7.0' }));
  fireEvent.click(await screen.findByRole('button', { name: /Download and install/ }));
  await screen.findByRole('dialog', { name: 'Update failed' });
  fireEvent.click(screen.getByRole('button', { name: 'OK' }));
  fireEvent.click(screen.getByRole('button', { name: 'Install 1.7.0' }));
  expect(await screen.findByRole('button', { name: /Install now/ })).toBeTruthy();
  expect(screen.queryByRole('button', { name: /Download and install/ })).toBeNull();
});

it('checks a pending offer again before opening it manually', async () => {
  render(<ToastProvider><Version /></ToastProvider>);
  await screen.findByRole('button', { name: 'Install 1.7.0' });
  call.mockResolvedValue({ ...shelf, available: null });
  fireEvent.click(screen.getByRole('button', { name: 'Install 1.7.0' }));
  await screen.findByText('This update is no longer available. Check for updates for the latest version.');
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('withdraws a manual offer when the backend announces it is no longer available', async () => {
  render(<ToastProvider><Version /></ToastProvider>);
  fireEvent.click(await screen.findByRole('button', { name: 'Install 1.7.0' }));
  await screen.findByRole('dialog', { name: 'Version 1.7.0 is ready' });
  call.mockResolvedValue({ ...shelf, available: null });
  await act(async () => {
    for (const listener of updateListeners) listener({ kind: 'version', available: null });
  });
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Install 1.7.0' })).toBeNull();
});
