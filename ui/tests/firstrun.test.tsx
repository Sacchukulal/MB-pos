/** The first five minutes. */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
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
import type { TaxSlabView } from '../src/ipc/generated/TaxSlabView';

const shopsClasses: TaxSlabView[] = [
  {
    id: 'tax_food_5',
    name: 'GST 5%',
    rate: '5%',
    rateBp: 500,
    kind: 'gst',
    basis: 'shop',
    priceWords: 'Shop default (added on top)',
    isActive: true,
    itemsUsing: 0,
  },
  {
    id: 'tax_liquor',
    name: 'Liquor — state VAT',
    rate: '20%',
    rateBp: 2000,
    kind: 'outside_gst',
    basis: 'inclusive',
    priceWords: 'In the price',
    isActive: true,
    itemsUsing: 0,
  },
];

const fresh: FirstRunView = {
  needed: true,
  hasShop: false,
  hasDetails: false,
  hasPin: false,
  hasItems: false,
  hasTables: false,
  shopPath: '',
  owner: null,
};

const anand: OwnerShopView = {
  id: 'rest_anand',
  name: 'Anand Bhavan',
  address: '14 Kamaraj Street, Chennai',
  gstin: '33AAAAA0000A1Z5',
  shortCode: 'ABC123',
  licence: 'active',
};

const saravana: OwnerShopView = { ...anand, id: 'rest_saravana', name: 'Saravana', address: '' };

const meena: OwnerSignInView = { name: 'Meena', email: 'meena@example.in', shops: [anand] };

/** What opening a new shop answers: the shop is there, named after nobody yet, with Meena's row. */
function opened(over: Partial<FirstRunView> = {}): OwnerOpenedView {
  return {
    firstRun: {
      ...fresh,
      hasShop: true,
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
      case 'save_staff_member':
        return Promise.resolve([]);
      case 'set_staff_pin':
        return Promise.resolve('H8BVY-QGXWV');
      case 'login':
        return Promise.resolve({ signedIn: true });
      case 'save_menu_item':
        return Promise.resolve([]);
      case 'tax_slabs':
        return Promise.resolve(shopsClasses);
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
  fireEvent.click(await screen.findByRole('button', { name: 'Choose the folder' }));
  await screen.findByText('D:\\Anand Bhavan');
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));

  fireEvent.change(await screen.findByLabelText('Email'), {
    target: { value: 'meena@example.in' },
  });
  fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'correct-horse' } });
  fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));

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
});

/** Two shops on one account: the owner says which. */
it('asks which shop when the account owns more than one', async () => {
  wire({}, { sign_in_owner: { ...meena, shops: [anand, saravana] } });
  render(<FirstRun onDone={vi.fn()} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Choose the folder' }));
  await screen.findByText('D:\\Anand Bhavan');
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  fireEvent.change(await screen.findByLabelText('Email'), { target: { value: 'm@x.in' } });
  fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'pw' } });
  fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));

  expect(await screen.findByText('Which shop is this counter for?')).toBeTruthy();
  expect(call).not.toHaveBeenCalledWith('open_as_owner', expect.anything());
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
  fireEvent.click(await screen.findByRole('button', { name: 'Choose the folder' }));
  await screen.findByText('D:\\Anand Bhavan');
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  fireEvent.change(await screen.findByLabelText('Email'), { target: { value: 'm@x.in' } });
  fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'wrong' } });
  fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));

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
  fireEvent.click(await screen.findByRole('button', { name: 'Choose the folder' }));
  await screen.findByText('D:\\Anand Bhavan');
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  expect(screen.queryByLabelText(/move the licence here/)).toBeNull();
  fireEvent.change(await screen.findByLabelText('Email'), { target: { value: 'm@x.in' } });
  fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'pw' } });
  fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));

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
        hasItems: true,
        hasTables: true,
        owner: { id: 'staff_sachin', name: 'Sachin', hasPin: true },
      }),
    },
  );
  render(<FirstRun onDone={done} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Choose the folder' }));
  await screen.findByText('D:\\Anand Bhavan');
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  fireEvent.change(await screen.findByLabelText('Email'), { target: { value: 'm@x.in' } });
  fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'pw' } });
  fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
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
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));

  expect(await screen.findByText('A PIN is 4 digits.')).toBeTruthy();
  // And it refused BEFORE creating anybody — a retry must not leave the shop with two owners in
  // the staff list.
  expect(call).not.toHaveBeenCalledWith('save_staff_member', expect.anything());
});

/** The PIN goes on the owner's row, the one Rust made from the account — never on a second one. */
it('gives the PIN to the owner row that already exists, with the name filled in', async () => {
  wire({
    hasShop: true,
    hasDetails: true,
    owner: { id: 'staff_meena', name: 'Meena', hasPin: false },
  });
  render(<FirstRun onDone={vi.fn()} />);

  const who = (await screen.findByLabelText('Your name')) as HTMLInputElement;
  expect(who.value).toBe('Meena');
  fireEvent.change(screen.getByLabelText('A PIN, 4 digits'), { target: { value: '4829' } });
  fireEvent.change(screen.getByLabelText('The same PIN again'), { target: { value: '4829' } });
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));

  await screen.findByText('Write this down');
  const saved = call.mock.calls.filter((c) => c[0] === 'save_staff_member');
  expect(saved).toHaveLength(1);
  expect((saved[0]?.[1] as { staff: { id: string } }).staff.id).toBe('staff_meena');
  expect(call).toHaveBeenCalledWith('set_staff_pin', { staffId: 'staff_meena', pin: '4829' });
  expect(call).toHaveBeenCalledWith('login', expect.objectContaining({ pin: '4829' }));
});

/** The recovery code gets a page to itself, and it is a door you cannot walk past. */
it('will not move on until the recovery code is written down', async () => {
  wire({ hasShop: true, hasDetails: true });
  render(<FirstRun onDone={vi.fn()} />);

  await screen.findByLabelText('Your name');
  fireEvent.change(screen.getByLabelText('Your name'), { target: { value: 'Meena' } });
  fireEvent.change(screen.getByLabelText('A PIN, 4 digits'), {
    target: { value: '4829' },
  });
  fireEvent.change(screen.getByLabelText('The same PIN again'), {
    target: { value: '4829' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));

  expect(await screen.findByText('H8BVY-QGXWV')).toBeTruthy();
  const next = screen.getByRole('button', { name: 'Next' }) as HTMLButtonElement;
  expect(next.disabled).toBe(true);

  fireEvent.click(screen.getByLabelText('I have written it down'));
  expect(next.disabled).toBe(false);
  fireEvent.click(next);

  expect(await screen.findByText('What you sell')).toBeTruthy();
});

/** Every optional step says plainly that it can be skipped, each is ONE button, and each has a way back. */
it('offers one way out of each optional step, and a way back', async () => {
  wire({ hasShop: true, hasDetails: true, hasPin: true });
  const done = vi.fn();
  render(<FirstRun onDone={done} />);

  // The items, the tables, the printer — three skips, one button each.
  fireEvent.click(await screen.findByRole('button', { name: 'Skip this — next' }));
  expect(await screen.findByRole('heading', { name: 'Your tables' })).toBeTruthy();
  // Back goes back.
  fireEvent.click(screen.getByRole('button', { name: 'Back' }));
  expect(await screen.findByRole('heading', { name: 'What you sell' })).toBeTruthy();
  fireEvent.click(await screen.findByRole('button', { name: 'Skip this — next' }));
  await screen.findByRole('heading', { name: 'Your tables' });
  fireEvent.click(await screen.findByRole('button', { name: 'Skip this — next' }));
  expect(await screen.findByRole('heading', { name: 'Your printer' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Back' })).toBeTruthy();
  const out = await screen.findByRole('button', { name: 'Skip this — start billing' });
  expect(screen.queryByRole('button', { name: 'I will do this later' })).toBeNull();
  fireEvent.click(out);
  expect(done).toHaveBeenCalled();
});

/** The wizard offers the shop's own classes. */
it('offers the shop own tax classes, not a hardcoded slab list', async () => {
  wire({ hasShop: true, hasDetails: true, hasPin: true });
  render(<FirstRun onDone={vi.fn()} />);

  const tax = (await screen.findByLabelText('Tax slab')) as HTMLSelectElement;
  await waitFor(() => expect(tax.options.length).toBe(2));
  expect([...tax.options].map((o) => o.value)).toEqual(['tax_food_5', 'tax_liquor']);
  expect([...tax.options].some((o) => o.value === 'tax_packaged_12')).toBe(false);

  fireEvent.change(screen.getByLabelText('Item'), { target: { value: 'Beer' } });
  fireEvent.change(screen.getByLabelText('Price'), { target: { value: '180' } });
  fireEvent.change(tax, { target: { value: 'tax_liquor' } });
  fireEvent.click(screen.getByRole('button', { name: 'Add' }));

  await waitFor(() => {
    const sent = call.mock.calls.find((c) => c[0] === 'save_menu_item');
    expect((sent?.[1] as { edit: { taxClassId: string } }).edit.taxClassId).toBe('tax_liquor');
  });
});

/** Somebody who stopped halfway comes back where they stopped. */
it('resumes at the first thing that is still missing', async () => {
  wire({ hasShop: true });
  render(<FirstRun onDone={vi.fn()} />);
  expect(await screen.findByRole('heading', { name: 'Your shop' })).toBeTruthy();
});
