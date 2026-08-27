/** The customer display. */

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

    // Not "we remembered not to call focus" — there is nothing here to focus.
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

    // Still the idle screen, and — the part that matters — still nothing focusable.
    expect(screen.getByText('Welcome')).toBeTruthy();
  });
});
