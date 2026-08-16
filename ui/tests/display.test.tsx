/**
 * **The customer display** — P29, scope 7.8, and T6.
 *
 * One thing matters on this screen and it is not what it looks like:
 *
 * > **IT MUST NEVER STEAL FOCUS FROM THE BILLING WINDOW. EVER.**
 *
 * A cashier who has to click back into the search box after every item will
 * unplug the display by Friday, so this is the condition on the feature
 * existing at all.
 *
 * The promise is kept in two places and asserted in both. Rust builds the
 * window unfocused and never asks for focus — `device_tests.rs` reads the
 * source and says so. This half is the other one: **the page has nothing
 * focusable on it**, so it could not take the keyboard even if it were shown
 * inside the billing window.
 *
 * The rest is what a customer actually needs to see: their lines, and their
 * total, in figures somebody else formatted (R8).
 */

import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

type Listener = (message: unknown) => void;
let listener: Listener | null = null;

vi.mock('../src/ipc/call', () => ({
  call: vi.fn(),
  inApp: () => true,
  isUiError: () => false,
  subscribe: (onPush: Listener) => {
    listener = onPush;
    return Promise.resolve(() => {
      listener = null;
    });
  },
}));

const { Display } = await import('../src/display/Display');

afterEach(() => {
  cleanup();
  listener = null;
});

/** Everything a browser will move focus to with Tab, or a click. */
const FOCUSABLE =
  'a[href], button, input, select, textarea, [tabindex], [contenteditable="true"]';

describe('the customer display', () => {
  it('has nothing on it that could ever take the keyboard', () => {
    const { container } = render(<Display />);
    act(() => listener?.({
      kind: 'customerBill',
      title: 'Anand Bhavan',
      lines: [
        { name: 'Masala Dosa', qty: '2', amount: '240.00' },
        { name: 'Filter Coffee', qty: '1', amount: '30.00' },
      ],
      total: '270.00',
      idle: false,
    }));

    // **T6.** Not "we remembered not to call focus" — there is nothing here to
    // focus. A later session adding a "clear" button to this page fails right
    // here, which is the point.
    expect(container.querySelectorAll(FOCUSABLE)).toHaveLength(0);
  });

  it('shows the lines and the total exactly as Rust formatted them', () => {
    render(<Display />);
    act(() => listener?.({
      kind: 'customerBill',
      title: 'Anand Bhavan',
      lines: [{ name: 'Masala Dosa', qty: '2', amount: '240.00' }],
      total: '270.00',
      idle: false,
    }));

    expect(screen.getByText('Masala Dosa')).toBeTruthy();
    expect(screen.getByText('240.00')).toBeTruthy();
    expect(screen.getByText('270.00')).toBeTruthy();
  });

  it('shows the shop between bills rather than an empty table', () => {
    render(<Display />);
    act(() => listener?.({
      kind: 'customerBill',
      title: 'Anand Bhavan',
      lines: [],
      total: '0.00',
      idle: true,
    }));

    expect(screen.getByText('Anand Bhavan')).toBeTruthy();
    expect(screen.getByText('Welcome')).toBeTruthy();
  });

  it('ignores every other push, because it is one channel for the whole app', () => {
    render(<Display />);
    act(() => listener?.({ kind: 'printQueue', jobs: [] }));
    act(() =>
      listener?.({ kind: 'session', who: 'Ravi', role: 'Cashier', stand_in: false }),
    );

    // Still the idle screen, and — the part that matters — still nothing
    // focusable.
    expect(screen.getByText('Welcome')).toBeTruthy();
  });
});
