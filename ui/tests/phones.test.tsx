/** The Phones screen — what it says about the road the phones come down. */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => false,
  isUiError: () => false,
  subscribe: () => Promise.resolve(() => undefined),
}));

const { Phones } = await import('../src/phones/Phones');
const { ToastProvider } = await import('../src/kit');

import type { NetworkView } from '../src/ipc/generated/NetworkView';

function network(over: Partial<NetworkView> = {}): NetworkView {
  return {
    headline: 'This counter is waiting for phones at 192.168.1.100 on port 7331.',
    tone: 'ok',
    certificateNote: '',
    devices: [],
    waiting: [],
    qr: [],
    code: '',
    phonesAllowed: 10,
    firewall: 'allowed',
    firewallSays: 'Windows Firewall lets phones in.',
    mayFixFirewall: false,
    people: [],
    connected: 0,
    ...over,
  };
}

function show() {
  return render(
    <ToastProvider>
      <Phones />
    </ToastProvider>,
  );
}

beforeEach(() => {
  call.mockReset();
});
afterEach(cleanup);

describe('the firewall, on the Phones screen', () => {
  /**
   * The rules are read once at start and remembered. A counter blocked after it started would
   * have gone on saying the phones could get in until somebody restarted it.
   */
  it('reads the rules again every time the screen opens', async () => {
    call.mockImplementation((name: string) =>
      Promise.resolve(
        name === 'check_firewall'
          ? network({
              tone: 'danger',
              headline: 'Windows Firewall is BLOCKING this program.',
              firewall: 'blocked',
              mayFixFirewall: true,
            })
          : network(),
      ),
    );
    show();

    // The remembered answer paints first, and the fresh one replaces it.
    expect(await screen.findByText(/Windows Firewall is BLOCKING/)).toBeTruthy();
    expect(call.mock.calls.some((c) => c[0] === 'check_firewall')).toBe(true);
  });

  it('offers one press to fix it, and says what it did', async () => {
    call.mockResolvedValue(network({ mayFixFirewall: true }));
    show();

    const button = await screen.findByRole('button', {
      name: 'Allow through Windows Firewall',
    });
    fireEvent.click(button);
    expect(call.mock.calls.some((c) => c[0] === 'allow_firewall')).toBe(true);
  });

  it('offers no button when the firewall already lets phones in', async () => {
    call.mockResolvedValue(network());
    show();
    await screen.findByText(/waiting for phones/);
    expect(
      screen.queryByRole('button', { name: 'Allow through Windows Firewall' }),
    ).toBeNull();
  });
});
