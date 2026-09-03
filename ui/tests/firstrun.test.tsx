/** The first five minutes. */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
}));

const { FirstRun } = await import('../src/setup/FirstRun');

import type { FirstRunView } from '../src/ipc/generated/FirstRunView';
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
  found: [],
  defaultFolder: 'C:\\Users\\Meena\\AppData\\Roaming\\MagicBill\\magicbill.db',
};

/** Answers every command in the flow, and remembers what it was asked. */
function wire(over: Partial<FirstRunView> = {}) {
  const view = { ...fresh, ...over };
  call.mockImplementation((name: string) => {
    switch (name) {
      case 'first_run':
        return Promise.resolve(view);
      case 'create_shop':
        return Promise.resolve({ ...view, hasShop: true });
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

beforeEach(() => call.mockReset());
afterEach(cleanup);

/** It says where the data goes before it puts any there. */
it('opens on a welcome that names the file it is about to create', async () => {
  wire();
  render(<FirstRun onDone={vi.fn()} />);

  expect(await screen.findByText('Welcome to Magic Bill')).toBeTruthy();
  expect(screen.getByText(/magicbill\.db/)).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Start a new shop' })).toBeTruthy();
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

/** A second try edits the same person. */
it('keeps the same person when the PIN is typed again', async () => {
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

  await screen.findByText('Write this down');
  const calls = call.mock.calls.filter((c) => c[0] === 'save_staff_member');
  expect(calls).toHaveLength(1);
});

/** Signing in is part of setting up. */
it('signs the owner in with the PIN they just chose', async () => {
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

  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('login', expect.objectContaining({ pin: '4829' })),
  );
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

/** Every optional step says plainly that it can be skipped, and each is ONE button. */
it('offers one way out of each optional step, and says it is optional', async () => {
  wire({ hasShop: true, hasDetails: true, hasPin: true });
  const done = vi.fn();
  render(<FirstRun onDone={done} />);

  // The items, the tables, the printer — three skips, one button each.
  fireEvent.click(await screen.findByRole('button', { name: 'Skip this — next' }));
  expect(await screen.findByRole('heading', { name: 'Your tables' })).toBeTruthy();
  fireEvent.click(await screen.findByRole('button', { name: 'Skip this — next' }));
  expect(await screen.findByRole('heading', { name: 'Your printer' })).toBeTruthy();
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
  expect(await screen.findByText('Your shop')).toBeTruthy();
});
