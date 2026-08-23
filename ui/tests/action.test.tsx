/**
 * **A button is dead while its own work runs** — the owner's round of
 * 22 August 2026, and the screen's half of the duplicate-press fix.
 *
 * The engine holds the counter to one action at a time, so nothing here is what
 * keeps the books right. What this proves is the part the engine cannot do:
 * that a second press is *dropped* rather than queued, and dropped **on the
 * press**, before React has re-rendered anything.
 *
 * That last part is the whole reason the hook keeps a ref as well as a state.
 * `setBusy` does not change anything this instant, so two clicks inside one
 * tick would both read `busy === false` — which is the original bug wearing a
 * disguise, and exactly what a cashier produces by pressing twice quickly.
 */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it } from 'vitest';

import { useAction } from '../src/kit/action';

afterEach(cleanup);

/** A button wired the way the billing screen wires Settle. */
function Counter({ work }: { work: () => Promise<unknown> }) {
  const [act, acting] = useAction();
  return (
    <button type="button" disabled={acting} onClick={() => act(work)}>
      {acting ? 'working' : 'press me'}
    </button>
  );
}

it('runs the work once however many times it is pressed', async () => {
  let started = 0;
  let release = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });

  render(
    <Counter
      work={() => {
        started += 1;
        return held;
      }}
    />,
  );

  const button = screen.getByRole('button');

  // Five presses in one go, the way a cashier presses when nothing has
  // happened yet. `fireEvent` is synchronous, so all five land before React
  // has re-rendered — which is precisely the case a state flag cannot catch.
  fireEvent.click(button);
  fireEvent.click(button);
  fireEvent.click(button);
  fireEvent.click(button);
  fireEvent.click(button);

  expect(started).toBe(1);

  await waitFor(() => expect(screen.getByText('working')).toBeTruthy());
  expect((button as HTMLButtonElement).disabled).toBe(true);

  release();
  await waitFor(() => expect(screen.getByText('press me')).toBeTruthy());
  expect((button as HTMLButtonElement).disabled).toBe(false);

  // And afterwards it works again — this switches a button off, it does not
  // burn it out.
  fireEvent.click(button);
  expect(started).toBe(2);
});

it('lets go of the button when the work fails', async () => {
  render(<Counter work={() => Promise.reject(new Error('the printer said no'))} />);
  const button = screen.getByRole('button');

  fireEvent.click(button);

  // A failed action must not leave the counter frozen. Before, a screen that
  // forgot its `finally` left the till dead until it was navigated away from.
  await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(button);
  await waitFor(() => expect(screen.getByText('press me')).toBeTruthy());
});
