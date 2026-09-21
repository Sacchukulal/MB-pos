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
    chip: 'Ready for phones',
    address: '192.168.1.100',
    port: 7331,
    certificateNote: '',
    devices: [],
    waiting: [],
    qr: [],
    code: '',
    downloadQr: ['#.#', '.#.', '#.#'],
    phonesAllowed: 10,
    firewall: 'allowed',
    firewallSays: 'Windows Firewall lets phones in.',
    firewallWord: 'Lets phones in',
    mayFixFirewall: false,
    people: [],
    connected: 0,
    ...over,
  };
}

/** The view Rust builds when Windows Firewall has no rule yet: a warning, and the button. */
function noRule(): NetworkView {
  return network({
    tone: 'warn',
    chip: 'Check the firewall',
    headline: 'This counter is at 192.168.1.100 on port 7331. Windows Firewall has no rule.',
    firewall: 'no_rule',
    firewallWord: 'No rule yet',
    mayFixFirewall: true,
  });
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
              chip: 'Check the firewall',
              headline: 'Windows Firewall is BLOCKING this program.',
              firewall: 'blocked',
              firewallWord: 'Blocking',
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
    call.mockResolvedValue(noRule());
    show();

    const button = await screen.findByRole('button', {
      name: 'Allow through Windows Firewall',
    });
    fireEvent.click(button);
    expect(call.mock.calls.some((c) => c[0] === 'allow_firewall')).toBe(true);
  });

  /** A clear road needs no sentence: the chip and the facts beside the list say it. */
  it('says the facts and nothing else when the firewall already lets phones in', async () => {
    call.mockResolvedValue(network());
    show();
    expect(await screen.findByText('Ready for phones')).toBeTruthy();
    expect(screen.getByText('192.168.1.100')).toBeTruthy();
    expect(screen.getByText('7331')).toBeTruthy();
    expect(screen.getByText('Lets phones in')).toBeTruthy();
    expect(screen.queryByText(/waiting for phones/)).toBeNull();
    expect(
      screen.queryByRole('button', { name: 'Allow through Windows Firewall' }),
    ).toBeNull();
  });
});

describe('the app, on the Phones screen', () => {
  /** A waiter needs the app before Add phone can do anything; the way to it is on the page. */
  it('shows the downloads QR and opens the downloads page in the browser', async () => {
    call.mockResolvedValue(network());
    show();

    expect(await screen.findByRole('img', { name: /downloads page/ })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: /magicbill.in\/downloads/ }));
    expect(call).toHaveBeenCalledWith('open_magicbill', { page: 'downloads' });
  });
});
