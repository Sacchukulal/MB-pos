/**
 * **The alerts bell** — P30.6, and it replaces `setup.test.tsx`.
 *
 * The owner installed the counter and found the top of every screen taken by
 * standing notices, and under them a six-row set-up checklist with a button on
 * each row:
 *
 * > *"instead of showing like this big line notification/error, just make a
 * > small bell button near sun moon button, so all push notifications and
 * > alerts are sent to there only."*
 *
 * So the claims D102 used to make about the set-up strip are made about this
 * panel now, plus two the bell adds: the count is the whole count, and the
 * colour is the WORST thing waiting rather than an average of everything.
 */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';

const { AlertsPanel, loudest } = await import('../src/shell/Alerts');
const { ToastProvider } = await import('../src/kit');

import type { Alert } from '../src/shell/Alerts';

const LICENCE: Alert = {
  id: 'licence',
  tone: 'danger',
  icon: 'badge',
  title: 'Your licence',
  says: 'Your plan ended on 10 August. Everything keeps working for another 5 days.',
  goTo: 'account',
  goLabel: 'Open Account',
};

const MENU: Alert = {
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

/**
 * **The sentence is Rust's and it is shown whole** (§6). Nothing in the panel
 * composes words about a machine state — the licence line here is the same
 * string the Account screen shows, and a shop that reads it twice reads the
 * same thing twice.
 */
it('shows each alert as the sentence it arrived as', () => {
  show([LICENCE, MENU]);
  expect(screen.getByText(/Your plan ended on 10 August/)).toBeTruthy();
  expect(screen.getByText('Type the items, or import a spreadsheet.')).toBeTruthy();
});

/**
 * **Every alert carries the button that fixes it** — D102's one idea worth
 * keeping. The panel is not a seventh editor; it opens the screen that already
 * does the job, and closes itself on the way.
 */
it('sends somebody to the screen that already does the job, and gets out of the way', () => {
  const { onGo, onClose } = show([MENU]);
  fireEvent.click(screen.getByRole('button', { name: 'Do it' }));
  expect(onGo).toHaveBeenCalledWith('menu');
  expect(onClose).toHaveBeenCalled();
});

/**
 * **Nothing is dismissible**, and that is the reason the banners were not
 * either: a dismissed warning is a problem that was never fixed. The only way
 * to clear one is to deal with it.
 */
it('offers no way to dismiss an alert', () => {
  show([LICENCE]);
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  // The close buttons shut the PANEL, which is a different thing.
  for (const button of screen.getAllByRole('button', { name: /close the alerts/i })) {
    expect(button).toBeTruthy();
  }
});

/**
 * **A quiet shop is told it is quiet.** An empty panel with nothing in it reads
 * as broken.
 */
it('says so when nothing needs anybody', () => {
  show([]);
  expect(screen.getByText(/Nothing needs you/)).toBeTruthy();
});

/**
 * **The badge takes the worst tone, not the commonest.** Five set-up steps and
 * one ended licence is a red bell: the number cannot say which of the six
 * matters, so the colour does (§2 rule 2 — and the count is still there in
 * words for anybody who cannot see the colour).
 */
it('is as loud as the worst thing waiting', () => {
  expect(loudest([])).toBe(null);
  expect(loudest([MENU])).toBe('info');
  expect(loudest([MENU, { ...MENU, id: 'b', tone: 'warn' }])).toBe('warn');
  expect(loudest([MENU, LICENCE])).toBe('danger');
});
