/** The first five minutes. */

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: (v: unknown) => typeof v === 'object' && v !== null && 'code' in v,
}));

const { FirstRun } = await import('../src/setup/FirstRun');

import type { FirstRunView } from '../src/ipc/generated/FirstRunView';
import type { OwnerOpenedView } from '../src/ipc/generated/OwnerOpenedView';
import type { OwnerShopView } from '../src/ipc/generated/OwnerShopView';
import type { OwnerSignInView } from '../src/ipc/generated/OwnerSignInView';
import type { StaffDetailView } from '../src/ipc/generated/StaffDetailView';

const fresh: FirstRunView = {
  needed: true,
  hasShop: false,
  hasDetails: false,
  hasPin: false,
  shopPath: '',
  owner: null,
};

const anand: OwnerShopView = {
  id: 'rest_anand',
  name: 'Anand Bhavan',
  address: '14 Kamaraj Street, Chennai',
  phone: '9840011223',
  gstin: '33AAAAA0000A1Z5',
  shortCode: 'ABC123',
  licence: 'active',
};

const saravana: OwnerShopView = { ...anand, id: 'rest_saravana', name: 'Saravana', address: '' };

const meena: OwnerSignInView = { name: 'Meena', email: 'meena@example.in', shops: [anand] };
/** The owner row Rust made from the account, before it has a PIN. */
const meenaRow: StaffDetailView = {
  id: 'staff_meena',
  name: 'Meena',
  roleId: 'role_owner',
  role: 'Owner',
  status: 'active',
  hasPin: false,
  phone: '9845012345',
  designation: '',
  department: '',
  employmentType: 'full_time',
  address: '',
  emergencyName: '',
  emergencyPhone: '',
  idProof: '',
  joined: '',
  leftOn: '',
  salarySays: '',
};

/** What opening a new shop answers: the shop is there, named after nobody yet, with Meena's row. */
function opened(over: Partial<FirstRunView> = {}): OwnerOpenedView {
  return {
    firstRun: {
      ...fresh,
      hasShop: true,
      shopPath: 'D:\\Anand Bhavan\\magicbill.db',
      owner: { id: 'staff_meena', name: 'Meena', hasPin: false },
      ...over,
    },
    shop: anand,
    cameDown: null,
  };
}

/** Answers every command in the flow, and remembers what it was asked. */
function wire(over: Partial<FirstRunView> = {}, answers: Record<string, unknown> = {}) {
  const view = { ...fresh, ...over };
  call.mockImplementation((name: string) => {
    if (name in answers) {
      const answer = answers[name];
      return answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer);
    }
    switch (name) {
      case 'first_run':
        return Promise.resolve(view);
      case 'pick_a_folder':
        return Promise.resolve('D:\\Anand Bhavan');
      case 'sign_in_owner':
        return Promise.resolve(meena);
      case 'open_as_owner':
        return Promise.resolve(opened());
      case 'save_settings':
        return Promise.resolve([]);
      case 'staff_details':
        return Promise.resolve(meenaRow);
      case 'save_staff_member':
        return Promise.resolve([]);
      case 'login':
        return Promise.resolve({ signedIn: true });
      case 'open_magicbill':
        return Promise.resolve('https://magicbill.in/signup');
      default:
        return Promise.resolve(null);
    }
  });
  return view;
}

/** A refusal in Rust's shape. */
function refusal(code: string, message: string) {
  return Object.assign(new Error(message), { code, message, detail: null, tone: 'danger' });
}

/** The folder chosen and Next pressed: the sign-in step, with its two doors. */
async function atSignIn() {
  fireEvent.click(await screen.findByRole('button', { name: 'Choose the folder' }));
  await screen.findByText('D:\\Anand Bhavan');
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  await screen.findByRole('heading', { name: 'Sign in' });
}

/** The account typed in and Next pressed. */
async function signInAs(email: string, password: string) {
  fireEvent.change(await screen.findByLabelText('Email'), { target: { value: email } });
  fireEvent.change(screen.getByLabelText('Password'), { target: { value: password } });
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
}

beforeEach(() => call.mockReset());
afterEach(cleanup);

/** Nobody chooses the folder but the owner: there is no default, and no way past without one. */
it('opens on the folder step with nothing chosen, and Next waits for a folder', async () => {
  wire();
  render(<FirstRun onDone={vi.fn()} />);

  expect(await screen.findByText('Welcome to Magic Bill')).toBeTruthy();
  expect(screen.getByText('No folder chosen yet')).toBeTruthy();
  expect(screen.queryByText(/Roaming/)).toBeNull();
  const next = screen.getByRole('button', { name: 'Next' }) as HTMLButtonElement;
  expect(next.disabled).toBe(true);
  // The old doors are gone: no shop is made without an account.
  expect(screen.queryByRole('button', { name: 'Start a new shop' })).toBeNull();
  expect(screen.queryByLabelText('Licence key')).toBeNull();

  fireEvent.click(screen.getByRole('button', { name: 'Choose the folder' }));
  expect(await screen.findByText('D:\\Anand Bhavan')).toBeTruthy();
  expect(next.disabled).toBe(false);
  fireEvent.click(next);
  expect(await screen.findByRole('heading', { name: 'Sign in' })).toBeTruthy();
});

/** Back and Next are on every step; the first step's Back has nowhere to go. */
it('has Back and Next on every step, and Back walks the steps in reverse', async () => {
  wire();
  render(<FirstRun onDone={vi.fn()} />);

  await screen.findByText('Welcome to Magic Bill');
  expect((screen.getByRole('button', { name: 'Back' }) as HTMLButtonElement).disabled).toBe(true);
  await atSignIn();
  expect((screen.getByRole('button', { name: 'Back' }) as HTMLButtonElement).disabled).toBe(false);
  expect(screen.getByRole('button', { name: 'Next' })).toBeTruthy();
  await signInAs('meena@example.in', 'correct-horse');

  // The shop is open: the details step, and Back from it shows where the counter stands.
  expect(await screen.findByRole('heading', { name: 'Your shop' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Back' }));
  expect(await screen.findByRole('heading', { name: 'Sign in' })).toBeTruthy();
  expect(screen.getByText('Meena · meena@example.in')).toBeTruthy();
  expect(screen.queryByLabelText('Email')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Back' }));
  expect(await screen.findByText('Welcome to Magic Bill')).toBeTruthy();
  expect(screen.getByText('D:\\Anand Bhavan\\magicbill.db')).toBeTruthy();
  expect(screen.queryByRole('button', { name: /Choose/ })).toBeNull();

  // And Next walks forward again without opening anything twice.
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  await screen.findByRole('heading', { name: 'Sign in' });
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  expect(await screen.findByRole('heading', { name: 'Your shop' })).toBeTruthy();
  expect(call.mock.calls.filter((c) => c[0] === 'open_as_owner')).toHaveLength(1);
  expect(screen.getByRole('button', { name: 'Back' })).toBeTruthy();
});

/** The explanation is there for whoever asks, not in the way of whoever does not. */
it('keeps the explanations behind a tip on the heading', async () => {
  wire();
  render(<FirstRun onDone={vi.fn()} />);
  await screen.findByText('Welcome to Magic Bill');
  expect(screen.getByRole('button', { name: 'About Welcome to Magic Bill' })).toBeTruthy();
  expect(screen.getByRole('tooltip').textContent).toContain('one folder');
});

/** The account is what names the shop; one shop opens by itself, in the chosen folder. */
it('signs the owner in and opens the one shop the account owns, in the chosen folder', async () => {
  wire();
  render(<FirstRun onDone={vi.fn()} />);
  await atSignIn();
  await signInAs('meena@example.in', 'correct-horse');

  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('open_as_owner', {
      restaurantId: 'rest_anand',
      folder: 'D:\\Anand Bhavan',
      moveHere: false,
    }),
  );
  // The details step starts filled in from the account.
  expect(await screen.findByRole('heading', { name: 'Your shop' })).toBeTruthy();
  expect((screen.getByLabelText('Shop name') as HTMLInputElement).value).toBe('Anand Bhavan');
  expect((screen.getByLabelText('Address') as HTMLInputElement).value).toBe(
    '14 Kamaraj Street, Chennai',
  );
  expect((screen.getByLabelText('Phone') as HTMLInputElement).value).toBe('9840011223');
});

/** The other door: the key from the dashboard, with no password asked for, behind the same Next. */
it('opens the shop a pasted licence key names, without signing in', async () => {
  wire({}, { open_with_key: opened() });
  render(<FirstRun onDone={vi.fn()} />);
  await atSignIn();

  // Nothing typed: Next says what it needs, and opens nothing.
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  expect(
    await screen.findByText(/Type the email and the password of your Magic Bill account, or paste/),
  ).toBeTruthy();
  expect(call).not.toHaveBeenCalledWith('open_with_key', expect.anything());

  fireEvent.change(screen.getByLabelText('Licence key'), {
    target: { value: ' mb-qyf8-xbgj-vxcq ' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));

  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('open_with_key', {
      key: 'mb-qyf8-xbgj-vxcq',
      folder: 'D:\\Anand Bhavan',
      moveHere: false,
    }),
  );
  expect(call).not.toHaveBeenCalledWith('sign_in_owner', expect.anything());
  expect(await screen.findByRole('heading', { name: 'Your shop' })).toBeTruthy();
  expect((screen.getByLabelText('Shop name') as HTMLInputElement).value).toBe('Anand Bhavan');
  expect((screen.getByLabelText('Phone') as HTMLInputElement).value).toBe('9840011223');
});

/** Sign up happens on the website: the browser opens on it, and the screen says where to. */
it('opens magicbill.in sign-up in the browser behind Sign up, and says so', async () => {
  wire();
  render(<FirstRun onDone={vi.fn()} />);
  await atSignIn();
  expect(call).not.toHaveBeenCalledWith('open_magicbill', expect.anything());

  fireEvent.click(await screen.findByRole('button', { name: 'Sign up' }));
  expect(call).toHaveBeenCalledWith('open_magicbill', { page: 'signup' });
  expect(
    await screen.findByText(/magicbill\.in\/signup is opening in your browser/),
  ).toBeTruthy();
  // No dialog, no QR code, and the sign-in doors are still right here for when they come back.
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(screen.queryByRole('img')).toBeNull();
  expect(screen.getByLabelText('Email')).toBeTruthy();
  expect(screen.getByLabelText('Licence key')).toBeTruthy();
});

/** No browser to hand the page to: one sentence, with the address in it. */
it('says so when the browser could not be opened for sign-up', async () => {
  wire(
    {},
    {
      open_magicbill: refusal(
        'website.launch',
        'The browser could not be opened. Open https://magicbill.in/signup on any phone or computer.',
      ),
    },
  );
  render(<FirstRun onDone={vi.fn()} />);
  await atSignIn();
  fireEvent.click(await screen.findByRole('button', { name: 'Sign up' }));
  expect(await screen.findByText(/browser could not be opened/)).toBeTruthy();
});

/** Two shops on one account: the owner says which. */
it('asks which shop when the account owns more than one', async () => {
  wire({}, { sign_in_owner: { ...meena, shops: [anand, saravana] } });
  render(<FirstRun onDone={vi.fn()} />);
  await atSignIn();
  await signInAs('m@x.in', 'pw');

  expect(await screen.findByText('Which shop is this counter for?')).toBeTruthy();
  expect(call).not.toHaveBeenCalledWith('open_as_owner', expect.anything());
  // Next without a choice says so, rather than signing in again.
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  expect(await screen.findByText('Choose which shop this counter is for.')).toBeTruthy();
  expect(call.mock.calls.filter((c) => c[0] === 'sign_in_owner')).toHaveLength(1);

  fireEvent.click(screen.getByRole('button', { name: 'Saravana' }));
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith(
      'open_as_owner',
      expect.objectContaining({ restaurantId: 'rest_saravana' }),
    ),
  );
});

/** A wrong password is one sentence, and the counter stays where it is. */
it('says so when the email and password do not match, and opens nothing', async () => {
  wire(
    {},
    {
      sign_in_owner: refusal(
        'cloud.refused',
        'That email and password do not match a Magic Bill account.',
      ),
    },
  );
  render(<FirstRun onDone={vi.fn()} />);
  await atSignIn();
  await signInAs('m@x.in', 'wrong');

  expect(await screen.findByText(/do not match a Magic Bill account/)).toBeTruthy();
  expect(call).not.toHaveBeenCalledWith('open_as_owner', expect.anything());
  expect(screen.getByLabelText('Email')).toBeTruthy();
});

/** A licence on a computer that died: the checkbox appears only then, and the press moves it. */
it('offers to move the licence only when it is bound elsewhere', async () => {
  let asked = 0;
  wire();
  call.mockImplementation((name: string, args?: { moveHere?: boolean }) => {
    if (name === 'first_run') return Promise.resolve(fresh);
    if (name === 'pick_a_folder') return Promise.resolve('D:\\Anand Bhavan');
    if (name === 'sign_in_owner') return Promise.resolve(meena);
    if (name === 'open_as_owner') {
      asked += 1;
      return args?.moveHere
        ? Promise.resolve(opened())
        : Promise.reject(
            refusal(
              'licence.bound_elsewhere',
              'This licence is being used on another computer (OLD-PC).',
            ),
          );
    }
    return Promise.resolve(null);
  });
  render(<FirstRun onDone={vi.fn()} />);
  await atSignIn();
  expect(screen.queryByLabelText(/move the licence here/)).toBeNull();
  await signInAs('m@x.in', 'pw');

  expect(await screen.findByText(/another computer/)).toBeTruthy();
  const move = await screen.findByLabelText(/move the licence here/);
  const open = screen.getByRole('button', { name: 'Open it here' }) as HTMLButtonElement;
  expect(open.disabled).toBe(true);
  fireEvent.click(move);
  expect(open.disabled).toBe(false);
  fireEvent.click(open);
  await waitFor(() => expect(asked).toBe(2));
  expect(call).toHaveBeenLastCalledWith(
    'open_as_owner',
    expect.objectContaining({ moveHere: true }),
  );
  expect(await screen.findByRole('heading', { name: 'Your shop' })).toBeTruthy();
});

/** A reinstall: the folder already holds the whole shop, and there is nothing left to ask. */
it('goes straight to the counter when the folder held a shop that was already set up', async () => {
  const done = vi.fn();
  wire(
    {},
    {
      open_as_owner: opened({
        needed: false,
        hasDetails: true,
        hasPin: true,
        owner: { id: 'staff_sachin', name: 'Sachin', hasPin: true },
      }),
    },
  );
  render(<FirstRun onDone={done} />);
  await atSignIn();
  await signInAs('m@x.in', 'pw');
  await waitFor(() => expect(done).toHaveBeenCalled());
});

/** The screen asks for the PIN the program will accept. */
it('asks for the PIN rule Rust actually holds', async () => {
  wire({ hasShop: true, hasDetails: true });
  render(<FirstRun onDone={vi.fn()} />);

  const pin = await screen.findByLabelText('A PIN, 4 digits');
  fireEvent.change(screen.getByLabelText('Your name'), { target: { value: 'Meena' } });
  fireEvent.change(pin, { target: { value: '123' } });
  fireEvent.change(screen.getByLabelText('The same PIN again'), { target: { value: '123' } });
  fireEvent.click(screen.getByRole('button', { name: 'Start billing' }));

  expect(await screen.findByText('A PIN is 4 digits.')).toBeTruthy();
  // And it refused BEFORE creating anybody — a retry must not leave the shop with two owners in
  // the staff list.
  expect(call).not.toHaveBeenCalledWith('save_staff_member', expect.anything());
});

/**
 * The PIN goes on the owner's row, the one Rust made from the account — never on a second one —
 * and the PIN step is the LAST page: the owner is signed in and the counter opens.
 */
it('gives the PIN to the owner row that already exists, signs in with it and opens the counter', async () => {
  const done = vi.fn();
  wire({
    hasShop: true,
    hasDetails: true,
    owner: { id: 'staff_meena', name: 'Meena', hasPin: false },
  });
  render(<FirstRun onDone={done} />);

  const who = (await screen.findByLabelText('Your name')) as HTMLInputElement;
  expect(who.value).toBe('Meena');
  fireEvent.change(screen.getByLabelText('A PIN, 4 digits'), { target: { value: '4829' } });
  fireEvent.change(screen.getByLabelText('The same PIN again'), { target: { value: '4829' } });
  fireEvent.click(screen.getByRole('button', { name: 'Start billing' }));

  await waitFor(() => expect(done).toHaveBeenCalled());
  const saved = call.mock.calls.filter((c) => c[0] === 'save_staff_member');
  expect(saved).toHaveLength(1);
  // One save carries the PIN, on top of what the row already holds — the phone the cloud knew
  // is not wiped by the PIN step.
  const staff = (saved[0]?.[1] as { staff: { id: string; pin: string; phone: string; roleId: string } })
    .staff;
  expect(staff.id).toBe('staff_meena');
  expect(staff.pin).toBe('4829');
  expect(staff.roleId).toBe('role_owner');
  expect(staff.phone).toBe('9845012345');
  expect(call).not.toHaveBeenCalledWith('set_staff_pin', expect.anything());
  expect(call).toHaveBeenCalledWith('login', expect.objectContaining({ pin: '4829' }));
  // No code to write down, and nothing after the PIN.
  expect(screen.queryByText(/Write this down/)).toBeNull();
  expect(screen.queryByText(/recovery/i)).toBeNull();
});

/**
 * Four steps and no more. The menu, the tables and the printer are the counter's own screens,
 * not questions a first run asks, so nothing here ever reads or writes them.
 */
it('asks for the folder, the account, the shop and the PIN, and nothing else', async () => {
  wire({ hasShop: true, hasDetails: true });
  render(<FirstRun onDone={vi.fn()} />);
  await screen.findByLabelText('Your name');

  const steps = screen.getByRole('list', { name: 'Setting up' });
  expect(
    within(steps)
      .getAllByRole('listitem')
      .map((li) => li.querySelector('.mb-firstrun__steplabel')?.textContent),
  ).toEqual(['Shop folder', 'Sign in', 'Shop name', 'Your PIN']);
  expect(screen.queryByText(/skip/i)).toBeNull();
  for (const name of ['tax_slabs', 'printer_setup', 'save_menu_item', 'add_dining_tables']) {
    expect(call).not.toHaveBeenCalledWith(name, expect.anything());
    expect(call).not.toHaveBeenCalledWith(name);
  }
});

/** Somebody who stopped halfway comes back where they stopped. */
it('resumes at the first thing that is still missing', async () => {
  wire({ hasShop: true });
  render(<FirstRun onDone={vi.fn()} />);
  expect(await screen.findByRole('heading', { name: 'Your shop' })).toBeTruthy();
});
