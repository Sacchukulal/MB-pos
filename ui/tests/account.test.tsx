/**
 * **The account screen** — P21.
 *
 * `mb-license` proves the entitlement and `licence_tests.rs` proves the gate.
 * This proves the three claims that are the SCREEN'S own, and every one of them
 * is a thing v1 got wrong:
 *
 * 1. **the screen composes no sentences.** Every string on it arrives from
 *    `words.rs`, which is *the one place a machine state becomes words* — so
 *    the test hands it a view with nonsense sentences in it and expects to see
 *    those exact strings, not better ones;
 * 2. **BACKEND-C5's sentence is actually shown.** An offline deactivate leaves
 *    the licence held, and the screen says so — v1 said "done";
 * 3. **the buttons a person may not press are not pressable.** `mayManage` is
 *    a courtesy on top of `guard::require`, and a courtesy that does not
 *    happen is a screen that produces refusals nobody understands.
 */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
}));

const { Account } = await import('../src/account/Account');
const { ToastProvider } = await import('../src/kit');

import type { LicenceView } from '../src/ipc/generated/LicenceView';

const activated: LicenceView = {
  standing: 'fine',
  chip: 'Active',
  tone: 'ok',
  headline: '',
  shopName: "Anna's Kitchen",
  planName: 'Restaurant Standard',
  renewsOn: '12 September',
  renewalSentence: 'Your plan renews on 12 September.',
  registeredContact: '+91 98••••••10',
  machine: '4C4C4544',
  machineHow: 'from Windows',
  machineIsFragile: false,
  phonesAllowed: 4,
  tillsAllowed: 1,
  included: ['reports', 'phone ordering'],
  checked: '9 Aug, 6:12 pm',
  stillHeld: '',
  clockNote: '',
  mayManage: true,
  isActivated: true,
};

function show(view: LicenceView) {
  call.mockResolvedValue(view);
  return render(
    <ToastProvider>
      <Account />
    </ToastProvider>,
  );
}

beforeEach(() => call.mockReset());
afterEach(cleanup);

it('shows the renewal as a sentence and not as a date field', async () => {
  show(activated);
  // 2.10: "your plan renews on 12 September" beats a date field, and the
  // difference is that one of them is a sentence.
  expect(await screen.findByText('Your plan renews on 12 September.')).toBeTruthy();
  expect(screen.getByText("Anna's Kitchen")).toBeTruthy();
  expect(screen.getByText('Restaurant Standard')).toBeTruthy();
  expect(screen.getByText('Active')).toBeTruthy();
});

it('shows the machine id, because support asks for it', async () => {
  show(activated);
  expect(await screen.findByText('4C4C4544')).toBeTruthy();
  expect(screen.getByText('from Windows')).toBeTruthy();
});

/**
 * **The screen writes nothing.**
 *
 * Given a view whose sentences are deliberately not English anybody would
 * choose, the screen shows those. If this test ever fails because the screen
 * "improved" a message, the sentence has moved out of `words.rs` and the two
 * copies will drift.
 */
it('shows the sentences Rust wrote, whatever they say', async () => {
  show({
    ...activated,
    standing: 'expired',
    chip: 'ZZZ-CHIP',
    tone: 'danger',
    headline: 'ZZZ-HEADLINE about the plan.',
    renewalSentence: '',
    clockNote: 'ZZZ-CLOCK note.',
  });
  expect(await screen.findByText('ZZZ-HEADLINE about the plan.')).toBeTruthy();
  expect(screen.getByText('ZZZ-CHIP')).toBeTruthy();
  expect(screen.getByText('ZZZ-CLOCK note.')).toBeTruthy();
});

/** **BACKEND-C5.** The one sentence v1 never said. */
it('says the licence is still held when a deactivate could not reach the server', async () => {
  show({
    ...activated,
    stillHeld:
      'This computer has stopped using the licence, but we could not tell our ' +
      'server. The licence is still held — we will keep trying.',
  });
  expect(await screen.findByText(/The licence is still held/)).toBeTruthy();
});

it('offers a key, a trial and an emergency code when nothing is activated', async () => {
  show({
    ...activated,
    standing: 'never-activated',
    chip: 'Not activated',
    tone: 'warn',
    isActivated: false,
    shopName: '',
    planName: 'No plan',
    renewsOn: '',
    renewalSentence: '',
    headline: 'This computer has no licence yet. You can bill and print.',
  });
  expect(await screen.findByRole('button', { name: 'Enter licence key' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Start a free trial' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Emergency code' })).toBeTruthy();
  // Nothing to deactivate.
  expect(screen.queryByRole('button', { name: 'Deactivate' })).toBeNull();
  // And it still says billing works, because that is the thing an owner is
  // actually worried about.
  expect(screen.getByText(/You can bill and print/)).toBeTruthy();
});

it('does not let somebody without licence.manage press anything that changes it', async () => {
  show({ ...activated, mayManage: false });
  await screen.findByText("Anna's Kitchen");
  expect(screen.getByRole('button', { name: 'Deactivate' }).hasAttribute('disabled')).toBe(true);
  expect(
    screen.getByRole('button', { name: 'Move a licence here' }).hasAttribute('disabled'),
  ).toBe(true);
  // Reading is `reports.view`, so Check again stays live.
  expect(screen.getByRole('button', { name: 'Check again' }).hasAttribute('disabled')).toBe(false);
});

/**
 * Activation sends the key **and the proof** — BACKEND-C6, from the screen's
 * side. There is no path through this dialog that sends a key alone.
 */
it('will not activate on a key alone', async () => {
  show({
    ...activated,
    isActivated: false,
    standing: 'never-activated',
    chip: 'Not activated',
  });
  fireEvent.click(await screen.findByRole('button', { name: 'Enter licence key' }));

  const key = await screen.findByLabelText('Licence key');
  const proof = screen.getByLabelText('Code we sent you');
  const activate = screen.getByRole('button', { name: 'Activate' });

  fireEvent.change(key, { target: { value: 'MB-1234-5678' } });
  expect(activate.hasAttribute('disabled')).toBe(true);

  fireEvent.change(proof, { target: { value: '123456' } });
  expect(activate.hasAttribute('disabled')).toBe(false);

  fireEvent.click(activate);
  await waitFor(() => {
    expect(call).toHaveBeenCalledWith('activate', { key: 'MB-1234-5678', proof: '123456' });
  });
});

/** Every command returns the whole view, so the screen never merges state. */
it('takes the whole view back from a command', async () => {
  show({ ...activated, isActivated: false, standing: 'never-activated' });
  fireEvent.click(await screen.findByRole('button', { name: 'Start a free trial' }));
  fireEvent.change(await screen.findByLabelText('Mobile or email'), {
    target: { value: '9812345610' },
  });

  call.mockResolvedValueOnce({
    ...activated,
    planName: 'Free trial',
    chip: 'Active',
  });
  fireEvent.click(screen.getByRole('button', { name: 'Start' }));
  expect(await screen.findByText('Free trial')).toBeTruthy();
});
