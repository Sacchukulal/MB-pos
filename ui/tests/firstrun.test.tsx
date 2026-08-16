/**
 * **The first five minutes** — P30.5, D155.
 *
 * The owner installed the first build on a second computer and the product had
 * no way to create a shop at all: the counter opened onto nothing, every
 * command failed, the failures stacked up as toasts and a six-item checklist
 * took the page. What follows is the set of claims that must not quietly come
 * undone, and each of them is one the screenshot argued for.
 */

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

const fresh: FirstRunView = {
  needed: true,
  hasShop: false,
  hasDetails: false,
  hasPin: false,
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
      default:
        return Promise.resolve(null);
    }
  });
  return view;
}

beforeEach(() => call.mockReset());
afterEach(cleanup);

/**
 * **It says where the data goes before it puts any there.**
 *
 * A shopkeeper who cannot answer "where are my bills kept?" has no way to back
 * the shop up, move it to a new computer, or believe the promise on the line
 * above it that nothing leaves the machine.
 */
it('opens on a welcome that names the file it is about to create', async () => {
  wire();
  render(<FirstRun onDone={vi.fn()} />);

  expect(await screen.findByText('Welcome to Magic Bill')).toBeTruthy();
  expect(screen.getByText(/magicbill\.db/)).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Start a new shop' })).toBeTruthy();
});

/**
 * **The screen asks for the PIN the program will accept.**
 *
 * The first draft said "four digits or more" and checked for four, while
 * `mb_auth::pin` has always required six to eight — so the form invited a PIN
 * and then refused it. Found by typing 1234 into it.
 */
it('asks for the PIN rule Rust actually holds', async () => {
  wire({ hasShop: true, hasDetails: true });
  render(<FirstRun onDone={vi.fn()} />);

  const pin = await screen.findByLabelText('A PIN, 6 to 8 digits');
  fireEvent.change(screen.getByLabelText('Your name'), { target: { value: 'Meena' } });
  fireEvent.change(pin, { target: { value: '1234' } });
  fireEvent.change(screen.getByLabelText('The same PIN again'), { target: { value: '1234' } });
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));

  expect(await screen.findByText('A PIN is 6 to 8 digits.')).toBeTruthy();
  // And it refused BEFORE creating anybody — a retry must not leave the shop
  // with two owners in the staff list.
  expect(call).not.toHaveBeenCalledWith('save_staff_member', expect.anything());
});

/**
 * **A second try edits the same person.** The id is kept, so mistyping the PIN
 * once does not hire a second owner nobody can explain.
 */
it('keeps the same person when the PIN is typed again', async () => {
  wire({ hasShop: true, hasDetails: true });
  render(<FirstRun onDone={vi.fn()} />);

  await screen.findByLabelText('Your name');
  fireEvent.change(screen.getByLabelText('Your name'), { target: { value: 'Meena' } });
  fireEvent.change(screen.getByLabelText('A PIN, 6 to 8 digits'), {
    target: { value: '482913' },
  });
  fireEvent.change(screen.getByLabelText('The same PIN again'), {
    target: { value: '482913' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));

  await screen.findByText('Write this down');
  const calls = call.mock.calls.filter((c) => c[0] === 'save_staff_member');
  expect(calls).toHaveLength(1);
});

/**
 * **Signing in is part of setting up** (D155).
 *
 * Asking for the PIN twenty seconds after choosing it reads as the program
 * having lost it.
 */
it('signs the owner in with the PIN they just chose', async () => {
  wire({ hasShop: true, hasDetails: true });
  render(<FirstRun onDone={vi.fn()} />);

  await screen.findByLabelText('Your name');
  fireEvent.change(screen.getByLabelText('Your name'), { target: { value: 'Meena' } });
  fireEvent.change(screen.getByLabelText('A PIN, 6 to 8 digits'), {
    target: { value: '482913' },
  });
  fireEvent.change(screen.getByLabelText('The same PIN again'), {
    target: { value: '482913' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));

  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('login', expect.objectContaining({ pin: '482913' })),
  );
});

/**
 * **The recovery code gets a page to itself, and it is a door you cannot walk
 * past.**
 *
 * It is shown once and never again. The first draft printed it in a box above
 * the item form on the next step, which is how the one line nobody can recover
 * guarantees itself to be skipped.
 */
it('will not move on until the recovery code is written down', async () => {
  wire({ hasShop: true, hasDetails: true });
  render(<FirstRun onDone={vi.fn()} />);

  await screen.findByLabelText('Your name');
  fireEvent.change(screen.getByLabelText('Your name'), { target: { value: 'Meena' } });
  fireEvent.change(screen.getByLabelText('A PIN, 6 to 8 digits'), {
    target: { value: '482913' },
  });
  fireEvent.change(screen.getByLabelText('The same PIN again'), {
    target: { value: '482913' },
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

/**
 * **The last step says plainly that it can be skipped**, and it is ONE button.
 *
 * The first draft had "I will do this later" beside "Skip and start billing",
 * which are two ways of writing the same click.
 */
it('offers one way out of the optional step, and says it is optional', async () => {
  wire({ hasShop: true, hasDetails: true, hasPin: true });
  const done = vi.fn();
  render(<FirstRun onDone={done} />);

  const out = await screen.findByRole('button', { name: 'Skip this — start billing' });
  expect(screen.queryByRole('button', { name: 'I will do this later' })).toBeNull();
  fireEvent.click(out);
  expect(done).toHaveBeenCalled();
});

/**
 * **Somebody who stopped halfway comes back where they stopped.** The step is
 * derived from what is in the shop, never remembered — the one thing D102 got
 * exactly right and this screen keeps.
 */
it('resumes at the first thing that is still missing', async () => {
  wire({ hasShop: true });
  render(<FirstRun onDone={vi.fn()} />);
  expect(await screen.findByText('Your shop')).toBeTruthy();
});
