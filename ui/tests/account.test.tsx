/** The account screen. */

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
  restaurantCode: 'ANNA01',
  cloudCopy: 'Last copied to the cloud: 9 Aug, 6:10 pm. No rows waiting.',
  cloudTone: 'ok',
  trialSentence: 'Start your free trial at magicbill.in, then enter the key here.',
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
  // 2.10: "your plan renews on 12 September" beats a date field, and the difference is that one
  // of them is a sentence.
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

/** The screen writes nothing. */
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

it('says the licence is still held when a deactivate could not reach the server', async () => {
  show({
    ...activated,
    stillHeld:
      'This computer has stopped using the licence, but we could not tell our ' +
      'server. The licence is still held — we will keep trying.',
  });
  expect(await screen.findByText(/The licence is still held/)).toBeTruthy();
});

it('shows the shop code staff type on a phone, and the cloud copy in a sentence', async () => {
  show(activated);
  expect(await screen.findByText('ANNA01')).toBeTruthy();
  expect(screen.getByText(/Last copied to the cloud/)).toBeTruthy();
});

it('offers a key and an emergency code when nothing is activated, and the trial is one sentence', async () => {
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
  // No trial dialog, no contact box: the trial is the website's.
  expect(screen.queryByRole('button', { name: 'Start a free trial' })).toBeNull();
  expect(screen.getByText(/Start your free trial at magicbill.in/)).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Emergency code' })).toBeTruthy();
  // Nothing to deactivate.
  expect(screen.queryByRole('button', { name: 'Deactivate' })).toBeNull();
  // And it still says billing works, because that is the thing an owner is actually worried
  // about.
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

/** Activation sends the key and the proof. */
/** The key is the proof: it is shown only to whoever bought it, so there is no code box. */
it('activates on the key alone', async () => {
  show({
    ...activated,
    isActivated: false,
    standing: 'never-activated',
    chip: 'Not activated',
  });
  fireEvent.click(await screen.findByRole('button', { name: 'Enter licence key' }));

  const key = await screen.findByLabelText('Licence key');
  expect(screen.queryByLabelText('Code we sent you')).toBeNull();
  const activate = screen.getByRole('button', { name: 'Activate' });
  expect(activate.hasAttribute('disabled')).toBe(true);

  fireEvent.change(key, { target: { value: 'MB-1234-5678' } });
  expect(activate.hasAttribute('disabled')).toBe(false);

  fireEvent.click(activate);
  await waitFor(() => {
    expect(call).toHaveBeenCalledWith('activate', { key: 'MB-1234-5678' });
  });
});

/** Every command returns the whole view, so the screen never merges state. */
it('takes the whole view back from a command', async () => {
  show({ ...activated, isActivated: false, standing: 'never-activated', planName: 'No plan' });
  fireEvent.click(await screen.findByRole('button', { name: 'Enter licence key' }));
  fireEvent.change(await screen.findByLabelText('Licence key'), {
    target: { value: 'MB-1234-5678' },
  });

  call.mockResolvedValueOnce({
    ...activated,
    planName: 'Restaurant Plus',
    chip: 'Active',
  });
  fireEvent.click(screen.getByRole('button', { name: 'Activate' }));
  expect(await screen.findByText('Restaurant Plus')).toBeTruthy();
});
