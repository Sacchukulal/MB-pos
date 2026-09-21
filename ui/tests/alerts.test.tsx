/** The alerts bell. */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';

const { AlertsPanel, loudest } = await import('../src/shell/Alerts');
const { ToastProvider } = await import('../src/kit');

import type { Alert } from '../src/shell/Alerts';

const LICENCE: Alert = {
  seen: false,
  id: 'licence',
  tone: 'danger',
  icon: 'badge',
  title: 'Your licence',
  says: 'Your plan ended on 10 August. Everything keeps working for another 5 days.',
  goTo: 'account',
  goLabel: 'Open Account',
};

const MENU: Alert = {
  seen: false,
  id: 'setup-menu',
  tone: 'info',
  icon: 'info',
  title: 'Put your menu in',
  says: 'Type the items, or import a spreadsheet.',
  goTo: 'menu',
};

afterEach(cleanup);

function show(alerts: readonly Alert[], onGo = vi.fn(), onClose = vi.fn()) {
  render(
    <ToastProvider>
      <AlertsPanel alerts={alerts} onGo={onGo} onClose={onClose} />
    </ToastProvider>,
  );
  return { onGo, onClose };
}

/** The sentence is Rust's and it is shown whole (§6). */
it('shows each alert as the sentence it arrived as', () => {
  show([LICENCE, MENU]);
  expect(screen.getByText(/Your plan ended on 10 August/)).toBeTruthy();
  expect(screen.getByText('Type the items, or import a spreadsheet.')).toBeTruthy();
});

/** Every alert carries the button that fixes it. */
it('sends somebody to the screen that already does the job, and gets out of the way', () => {
  const { onGo, onClose } = show([MENU]);
  fireEvent.click(screen.getByRole('button', { name: 'Do it' }));
  expect(onGo).toHaveBeenCalledWith('menu');
  expect(onClose).toHaveBeenCalled();
});

/**
 * Nothing is dismissible, and that is the reason the banners were not either: a dismissed
 * warning is a problem that was never fixed.
 */
it('offers no way to dismiss an alert', () => {
  show([LICENCE]);
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  // The close buttons shut the PANEL, which is a different thing.
  for (const button of screen.getAllByRole('button', { name: /close the alerts/i })) {
    expect(button).toBeTruthy();
  }
});

/** A quiet shop is told it is quiet. */
it('says so when nothing needs anybody', () => {
  show([]);
  expect(screen.getByText(/Nothing needs you/)).toBeTruthy();
});

/** The badge takes the worst tone, not the commonest. */
it('is as loud as the worst thing waiting', () => {
  expect(loudest([])).toBe(null);
  expect(loudest([MENU])).toBe('info');
  expect(loudest([MENU, { ...MENU, id: 'b', tone: 'warn' }])).toBe('warn');
  expect(loudest([MENU, LICENCE])).toBe('danger');
});

/** What was read stays in the list — it just does not count. */
it('shows a read alert like any other, and names where a notice came from', () => {
  show([
    { ...MENU, seen: true },
    {
      id: 'notice-1',
      tone: 'accent',
      icon: 'info',
      title: 'Update available',
      says: 'Version 1.8 is out.',
      when: '21 Sep, 1:08 pm',
      from: 'Magic Bill',
      seen: false,
    },
  ]);
  expect(screen.getByText('Put your menu in')).toBeTruthy();
  expect(screen.getByText('From Magic Bill')).toBeTruthy();
  expect(screen.getByText('21 Sep, 1:08 pm')).toBeTruthy();
});

/** The key Rust remembers an alert by changes with its sentence, so a changed alert is new. */
it('remembers an alert by what it says, not only by what it is', async () => {
  const { alertKey } = await import('../src/shell/Alerts');
  expect(alertKey(LICENCE)).not.toBe(alertKey({ ...LICENCE, says: 'Your plan ended today.' }));
  expect(alertKey(LICENCE)).toBe(alertKey({ id: LICENCE.id, says: LICENCE.says }));
});
