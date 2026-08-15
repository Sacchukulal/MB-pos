/**
 * **The tills screen** — P27.
 *
 * Rust proves the money (`src-tauri/src/terminal_tests.rs`, T1 to T11). This
 * proves the three claims a future session could quietly undo:
 *
 * 1. **the sentences come from Rust and are shown whole** (R8) — "Bills print
 *    as A/0001", what the plan allows, and D138's "the main till is off";
 * 2. **a till that is holding bills says so, and says nothing is lost** — the
 *    fear this answers is a shop thinking the money has gone;
 * 3. **the prefix is editable and reaches Rust as typed**, because it is
 *    D135's one remaining risk and the refusal lives on the other side.
 */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
  subscribe: () => Promise.resolve(() => undefined),
}));

const { Tills } = await import('../src/settings/Tills');
const { ToastProvider } = await import('../src/kit');

import type { TillsView } from '../src/ipc/generated/TillsView';

const MAIN = {
  id: 't_a',
  name: 'Counter 1',
  prefix: 'A/',
  isMaster: true,
  isThisOne: false,
  lastSeen: '2 minutes ago',
  numbersSay: 'Bills print as A/0001.',
};

const SECOND = {
  id: 't_b',
  name: 'Counter 2',
  prefix: 'B/',
  isMaster: false,
  isThisOne: true,
  lastSeen: 'just now',
  numbersSay: 'Bills print as B/0001.',
};

const QUIET: TillsView = {
  tills: [MAIN, SECOND],
  me: 't_b',
  isMaster: false,
  awaySays: '',
  waitingSays: '',
  waiting: 0,
  allowed: 3,
  limitSays: 'Your plan allows 3 tills. You are using 2.',
  mayManage: true,
};

function show(view: TillsView) {
  call.mockImplementation((name: string) => {
    if (name === 'tills') return Promise.resolve(view);
    return Promise.resolve(view);
  });
  return render(
    <ToastProvider>
      <Tills />
    </ToastProvider>,
  );
}

describe('the tills screen', () => {
  beforeEach(() => call.mockReset());
  afterEach(cleanup);

  it('shows each till with the numbers it prints, in Rust’s words', async () => {
    show(QUIET);

    expect(await screen.findByText('Counter 1')).toBeTruthy();
    // **The whole sentence, not a prefix the screen dressed up.** A screen that
    // built "Bills print as " + prefix + "0001" would be a second answer to
    // what this till numbers look like, and the padding is a setting.
    expect(screen.getByText(/Bills print as A\/0001\./)).toBeTruthy();
    expect(screen.getByText(/Bills print as B\/0001\./)).toBeTruthy();
    // The plan's sentence, also from Rust.
    expect(screen.getByText('Your plan allows 3 tills. You are using 2.')).toBeTruthy();
  });

  it('says the main till is off, and that nothing is lost', async () => {
    show({
      ...QUIET,
      awaySays:
        'The main till is off. This till can take counter and parcel bills — table service needs the main till.',
      waitingSays:
        '3 bills are waiting to reach the main till. Nothing is lost — they go across as soon as it is back.',
      waiting: 3,
    });

    // D138's sentence, whole. It tells a shop what still works and what does
    // not, which is the entire feature.
    expect(
      await screen.findByText(/This till can take counter and parcel bills/),
    ).toBeTruthy();
    // And the half that stops somebody panicking about the money.
    expect(screen.getByText(/Nothing is lost/)).toBeTruthy();
  });

  it('pushes what is waiting when somebody presses Send now', async () => {
    show({ ...QUIET, waitingSays: '1 bill is waiting to reach the main till.', waiting: 1 });

    fireEvent.click(await screen.findByText('Send now'));
    expect(call.mock.calls.some(([name]) => name === 'send_waiting_bills')).toBe(true);
  });

  it('sends the prefix to Rust exactly as it was typed', async () => {
    show(QUIET);

    fireEvent.click((await screen.findAllByText('Change'))[0]!);
    const prefix = screen.getByLabelText(/in front of its bill numbers/);
    fireEvent.change(prefix, { target: { value: 'C/' } });
    fireEvent.click(screen.getByText('Save'));

    const saved = call.mock.calls.find(([name]) => name === 'save_till');
    expect(saved).toBeTruthy();
    // **Untouched** — no trimming, no upper-casing, no slash appended. Rust owns
    // the rule and Rust writes the refusal that names the clashing till.
    expect((saved![1] as { edit: { prefix: string } }).edit.prefix).toBe('C/');
  });

  it('hides every button from somebody who may not manage tills', async () => {
    show({ ...QUIET, mayManage: false });

    expect(await screen.findByText('Counter 1')).toBeTruthy();
    expect(screen.queryByText('Change')).toBeNull();
    expect(screen.queryByText('Make it the main till')).toBeNull();
    expect(screen.queryByText('Join a shop')).toBeNull();
  });
});
