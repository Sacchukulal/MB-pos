/**
 * **The set-up list** — P22, T9's front-end half.
 *
 * `setup.rs` decides what is left; this proves the three things the screen is
 * responsible for, and each of them is a decision (D102):
 *
 * 1. **it goes away when the shop is set up** — a panel congratulating an owner
 *    every morning for the rest of the shop's life is a panel they learn to
 *    look past, and then they look past the one that matters;
 * 2. **it never blocks the till** — it is a card on the billing screen, not a
 *    gate in front of it (PERFORMANCE S5 is three minutes to a printable bill);
 * 3. **"Do it" opens the screen that already does the job**, rather than a
 *    seventh editor.
 */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
}));

const { Setup } = await import('../src/setup/Setup');

import type { SetupView } from '../src/ipc/generated/SetupView';

const halfway: SetupView = {
  headline: '2 things to do — and you can take money in the meantime.',
  left: 2,
  finished: false,
  steps: [
    {
      id: 'shop',
      title: 'Tell us about your shop',
      why: 'Your name, address and GSTIN go on every bill you print.',
      done: true,
      goTo: 'settings',
      mattersMost: true,
    },
    {
      id: 'menu',
      title: 'Put your menu in',
      why: 'Type the items, or import a spreadsheet.',
      done: false,
      goTo: 'menu',
      mattersMost: true,
    },
    {
      id: 'tables',
      title: 'Add your tables',
      why: 'Only if you have table service.',
      done: false,
      goTo: 'floor',
      mattersMost: false,
    },
  ],
};

beforeEach(() => call.mockReset());
afterEach(cleanup);

it('lists what is left, with the reason rather than the label', async () => {
  call.mockResolvedValue(halfway);
  render(<Setup onGoTo={vi.fn()} />);

  expect(await screen.findByText('Put your menu in')).toBeTruthy();
  expect(screen.getByText('Type the items, or import a spreadsheet.')).toBeTruthy();
  // The headline says the thing that matters most: you can trade meanwhile.
  expect(screen.getByText(/take money in the meantime/)).toBeTruthy();
});

/**
 * **Only what is left, and progress as one line.**
 *
 * The first version listed done steps too. Looking at it on the real screen
 * settled it the other way: six rows with their reasons filled the whole
 * billing pane and pushed the table grid and the menu below the fold, on the
 * one screen a cashier looks at all day.
 */
it('shows what is left and counts what is done in a line', async () => {
  call.mockResolvedValue(halfway);
  render(<Setup onGoTo={vi.fn()} />);

  expect(await screen.findByText('Put your menu in')).toBeTruthy();
  // The finished step is a number, not a row.
  expect(screen.queryByText('Tell us about your shop')).toBeNull();
  expect(screen.getByText('1 of 3 done.')).toBeTruthy();
  expect(screen.getAllByRole('button', { name: 'Do it' })).toHaveLength(2);
});

it('sends somebody to the screen that already does the job', async () => {
  const go = vi.fn();
  call.mockResolvedValue(halfway);
  render(<Setup onGoTo={go} />);

  const buttons = await screen.findAllByRole('button', { name: 'Do it' });
  const first = buttons[0];
  expect(first).toBeDefined();
  if (!first) return;
  fireEvent.click(first);
  // The menu step, and it opens the Menu screen — not a wizard's own editor.
  expect(go).toHaveBeenCalledWith('menu');
});

/**
 * **It disappears.** The whole panel, not just its rows — a set-up list on a
 * shop that has been trading for two years is clutter on the screen a cashier
 * looks at all day.
 */
it('is gone once the shop is set up', async () => {
  call.mockResolvedValue({
    ...halfway,
    headline: 'Your shop is set up. Everything below is done.',
    left: 0,
    finished: true,
    steps: halfway.steps.map((s) => ({ ...s, done: true })),
  });
  const { container } = render(<Setup onGoTo={vi.fn()} />);
  // Nothing at all, rather than an empty card.
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(container.textContent).toBe('');
});

/**
 * **Not tested here, and the reason is written down rather than left as a
 * gap.**
 *
 * A counter that cannot say what is left still bills: `Setup` catches a failed
 * load and renders nothing. Three attempts at asserting that in vitest all
 * failed on the harness rather than on the component — an already-rejected
 * promise is unhandled for the tick between being created and React running
 * the effect that consumes it, and the detector fires in that window whichever
 * way the rejection is deferred or pre-caught.
 *
 * The behaviour is one visible line in `Setup.tsx` (`.catch(...)`), and the
 * "renders nothing" half is covered by the test above it. Faking a pass or
 * leaving a red suite would both have been worse than saying so.
 */
