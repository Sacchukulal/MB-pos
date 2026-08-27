/** A button is dead while its own work runs. */

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

  // Five presses in one go, the way a cashier presses when nothing has happened yet.
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

  // And afterwards it works again — this switches a button off, it does not burn it out.
  fireEvent.click(button);
  expect(started).toBe(2);
});

it('lets go of the button when the work fails', async () => {
  render(<Counter work={() => Promise.reject(new Error('the printer said no'))} />);
  const button = screen.getByRole('button');

  fireEvent.click(button);

  // A failed action must not leave the counter frozen.
  await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(button);
  await waitFor(() => expect(screen.getByText('press me')).toBeTruthy());
});
