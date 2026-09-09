/** The Account page: the licence panel and its doors. */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => false,
  isUiError: (cause: unknown) => typeof cause === 'object' && cause !== null && 'code' in cause,
  subscribe: () => Promise.resolve(() => undefined),
}));

const { Account } = await import('../src/account/Account');
const { ToastProvider } = await import('../src/kit');

import type { LicenceView } from '../src/ipc/generated/LicenceView';

const active: LicenceView = {
  standing: 'fine',
  chip: 'Active',
  tone: 'ok',
  headline: '',
  hasLicence: true,
  boundElsewhere: false,
  ownerName: 'Anna Kuteera',
  ownerPhone: '9840011223',
  shopName: "Anna's Kitchen",
  planName: 'Restaurant Standard',
  dateLabel: 'Renews on',
  date: '12 September',
  key: 'MB-1234-5678-9ABC',
  restaurantCode: 'ANNA01',
  phonesAllowed: 4,
  tillsAllowed: 1,
  included: ['reports', 'phone ordering'],
  checked: '9 Aug, 6:12 pm',
  stillHeld: '',
  clockNote: '',
  mayManage: true,
  cloudCopy: 'Last copied to the cloud: 9 Aug, 6:10 pm. Nothing waiting.',
  cloudTone: 'ok',
};

const none: LicenceView = {
  ...active,
  standing: 'never-activated',
  chip: 'Not activated',
  tone: 'warn',
  headline: 'This computer has no licence yet. You can bill and print.',
  hasLicence: false,
  ownerName: '',
  ownerPhone: '',
  shopName: '',
  planName: 'No plan',
  dateLabel: '',
  date: '',
  key: '',
  restaurantCode: '',
  included: [],
};

function show(view: LicenceView) {
  call.mockImplementation((command: string) =>
    command === 'account' ? Promise.resolve(view) : Promise.resolve(null),
  );
  return render(
    <ToastProvider>
      <Account />
    </ToastProvider>,
  );
}

beforeEach(() => call.mockReset());
afterEach(cleanup);

it('shows every fact in its own place', async () => {
  show(active);
  expect(await screen.findByText('Anna Kuteera')).toBeTruthy();
  expect(screen.getByText('9840011223')).toBeTruthy();
  expect(screen.getAllByText(/Anna's Kitchen/).length).toBeGreaterThan(0);
  expect(screen.getAllByText('Restaurant Standard').length).toBeGreaterThan(0);
  expect(screen.getByText('Renews on')).toBeTruthy();
  expect(screen.getByText('12 September')).toBeTruthy();
  expect(screen.getByText('MB-1234-5678-9ABC')).toBeTruthy();
  expect(screen.getByText('ANNA01')).toBeTruthy();
  expect(screen.getByText(/Last copied to the cloud/)).toBeTruthy();
  expect(screen.getAllByText('Active').length).toBeGreaterThan(0);
});

it('offers Check now, Manage plan, Sign out and Change licence on a licensed counter', async () => {
  show(active);
  expect(await screen.findByRole('button', { name: 'Check now' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Manage plan' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Sign out of this computer' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Change licence' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Code from support' })).toBeNull();
});

it('shows the sentences Rust wrote, whatever they say', async () => {
  show({
    ...active,
    standing: 'expired',
    chip: 'ZZZ-CHIP',
    tone: 'danger',
    headline: 'ZZZ-HEADLINE about the plan.',
    dateLabel: 'Ran out on',
    clockNote: 'ZZZ-CLOCK note.',
  });
  expect(await screen.findByText('ZZZ-HEADLINE about the plan.')).toBeTruthy();
  expect(screen.getByText('ZZZ-CHIP')).toBeTruthy();
  expect(screen.getByText('ZZZ-CLOCK note.')).toBeTruthy();
  expect(screen.getByText('Ran out on')).toBeTruthy();
  // Support's code is offered only when something is wrong.
  expect(screen.getByRole('button', { name: 'Code from support' })).toBeTruthy();
});

it('hides the key when this person may not change the licence', async () => {
  show({ ...active, mayManage: false, key: '' });
  await screen.findByText('Anna Kuteera');
  expect(screen.queryByText('Licence key')).toBeNull();
});

it('offers one button to bring a licence back from another computer', async () => {
  show({
    ...active,
    standing: 'bound-elsewhere',
    chip: 'Another computer',
    tone: 'danger',
    boundElsewhere: true,
    headline: 'This licence is on another computer.',
  });
  fireEvent.click(await screen.findByRole('button', { name: 'Use it on this computer' }));
  await waitFor(() => {
    expect(call).toHaveBeenCalledWith('bring_licence_here');
  });
});

it('shows the two doors and nothing else when there is no licence', async () => {
  show(none);
  expect(await screen.findByText(/You can bill and print/)).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Buy a plan' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Sign out of this computer' })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Check now' })).toBeNull();

  fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
  expect(await screen.findByLabelText('Email')).toBeTruthy();
  expect(screen.getByLabelText('Password')).toBeTruthy();
  expect(screen.getByLabelText('Licence key')).toBeTruthy();
});

it('takes the key, then the PIN twice, then changes the licence', async () => {
  show(active);
  fireEvent.click(await screen.findByRole('button', { name: 'Change licence' }));

  const next = await screen.findByRole('button', { name: 'Next' });
  expect(next.hasAttribute('disabled')).toBe(true);
  fireEvent.change(screen.getByLabelText('Licence key'), { target: { value: 'MB-NEW-0001' } });
  expect(next.hasAttribute('disabled')).toBe(false);
  fireEvent.click(next);

  const pin = await screen.findByLabelText('Your PIN');
  fireEvent.change(pin, { target: { value: '4839' } });
  fireEvent.change(screen.getByLabelText('The same PIN again'), { target: { value: '4839' } });

  call.mockResolvedValueOnce({ ...active, ownerName: 'Ravi', shopName: "Ravi's Cafe" });
  fireEvent.click(screen.getByRole('button', { name: 'Use this licence' }));
  await waitFor(() => {
    expect(call).toHaveBeenCalledWith('change_licence', {
      door: { by: 'key', key: 'MB-NEW-0001' },
      moveHere: false,
      newPin: '4839',
    });
  });
  expect(await screen.findByText('Ravi')).toBeTruthy();
});

it('asks the account which shop when it owns more than one', async () => {
  show(active);
  fireEvent.click(await screen.findByRole('button', { name: 'Change licence' }));
  fireEvent.change(await screen.findByLabelText('Email'), { target: { value: 'r@x.in' } });
  fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'pw' } });

  call.mockResolvedValueOnce({
    name: 'Ravi',
    email: 'r@x.in',
    shops: [
      { id: 'r1', name: 'Cafe One', address: '', phone: '', gstin: '', shortCode: '', licence: 'active' },
      { id: 'r2', name: 'Cafe Two', address: '', phone: '', gstin: '', shortCode: '', licence: 'active' },
    ],
  });
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Cafe Two' }));
  expect(await screen.findByLabelText('Your PIN')).toBeTruthy();
  expect(screen.getByText(/Cafe Two takes over this counter/)).toBeTruthy();
});

it('asks before signing out, and takes the whole view back', async () => {
  show(active);
  fireEvent.click(await screen.findByRole('button', { name: 'Sign out of this computer' }));
  call.mockResolvedValueOnce(none);
  fireEvent.click(await screen.findByRole('button', { name: 'Sign out' }));
  await waitFor(() => {
    expect(call).toHaveBeenCalledWith('sign_out_licence');
  });
  expect(await screen.findByRole('button', { name: 'Buy a plan' })).toBeTruthy();
});
