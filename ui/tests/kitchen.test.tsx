/** The kitchen display. */

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
  subscribe: () => Promise.resolve(() => undefined),
}));

const { Kitchen } = await import('../src/kitchen/Kitchen');
const { ToastProvider } = await import('../src/kit');

import type { KitchenLine } from '../src/ipc/generated/KitchenLine';
import type { KitchenTicket } from '../src/ipc/generated/KitchenTicket';
import type { KitchenView } from '../src/ipc/generated/KitchenView';

function line(over: Partial<KitchenLine> = {}): KitchenLine {
  return {
    key: 'k1',
    qty: '2',
    name: 'Butter Naan',
    note: null,
    course: '',
    isNew: false,
    isDone: false,
    ...over,
  };
}

function ticket(over: Partial<KitchenTicket> = {}): KitchenTicket {
  return {
    id: 'kds_1',
    orderId: 'ord_1',
    station: 'Tandoor',
    place: 'Table 5',
    token: '12',
    waiter: 'Ravi',
    course: '',
    lines: [line()],
    waitingMinutes: 3,
    waiting: '3 min',
    expected: '6 min',
    says: 'Cooking',
    tone: 'cooking',
    isCancelled: false,
    wasPrinted: false,
    ...over,
  };
}

function view(over: Partial<KitchenView> = {}): KitchenView {
  return {
    station: 'Tandoor',
    stations: ['Kitchen', 'Tandoor'],
    tickets: [ticket()],
    headline: '1 order waiting.',
    late: 0,
    waitingCourses: [],
    lastCleared: null,
    ...over,
  };
}

/** The screen reports failures through a toast, so it needs one to live in. */
function show() {
  return render(
    <ToastProvider>
      <Kitchen />
    </ToastProvider>,
  );
}

/** Answer `kitchen` with this view and every action with it too. */
function serve(v: KitchenView) {
  call.mockImplementation(() => Promise.resolve(v));
}

beforeEach(() => {
  call.mockReset();
  vi.useRealTimers();
});

afterEach(cleanup);

describe('the kitchen display', () => {
  it('shows the food, the place and the wait', async () => {
    serve(view());
    show();

    expect(await screen.findByText('Table 5')).toBeTruthy();
    expect(screen.getByText('#12')).toBeTruthy();
    expect(screen.getByText('Butter Naan')).toBeTruthy();
    expect(screen.getByText('2')).toBeTruthy();
    expect(screen.getByText('3 min')).toBeTruthy();
  });

  /** The ack means DRAWN. */
  it('tells the counter it drew a new ticket, and only a new one', async () => {
    serve(view({ tickets: [ticket({ tone: 'new' }), ticket({ id: 'kds_2', tone: 'cooking' })] }));
    show();

    await screen.findAllByText('Butter Naan');
    await waitFor(() => {
      expect(call).toHaveBeenCalledWith('kitchen_shown', { id: 'kds_1' });
    });
    expect(call).not.toHaveBeenCalledWith('kitchen_shown', { id: 'kds_2' });
  });

  it('says every state in words, not only in colour', async () => {
    serve(
      view({
        tickets: [
          ticket({ id: 'a', says: 'New', tone: 'new' }),
          ticket({ id: 'b', says: 'Cooking', tone: 'cooking' }),
          ticket({ id: 'c', says: 'LATE', tone: 'late' }),
          ticket({ id: 'd', says: 'On paper', tone: 'printed', wasPrinted: true }),
        ],
      }),
    );
    show();

    for (const word of ['New', 'Cooking', 'LATE', 'On paper']) {
      expect(await screen.findByText(word)).toBeTruthy();
    }
  });

  /** Both ways in, always. */
  // Given longer than the default five seconds for the reason the shell's sign-in test is: it
  // runs in about half a second on an idle machine, and vitest runs twenty-five files at once.
  it('clears a card from the number key as well as the button', { timeout: 20_000 }, async () => {
    serve(view({ tickets: [ticket({ id: 'first' }), ticket({ id: 'second' })] }));
    show();
    await screen.findAllByText('Table 5');

    fireEvent.keyDown(window, { key: '2' });
    await waitFor(() => {
      expect(call).toHaveBeenCalledWith('kitchen_bump', { id: 'second' });
    });

    const [firstDone] = screen.getAllByRole('button', { name: /Done/ });
    if (!firstDone) throw new Error('no card to clear');
    fireEvent.click(firstDone);
    await waitFor(() => {
      expect(call).toHaveBeenCalledWith('kitchen_bump', { id: 'first' });
    });
  });

  /** Food already cooking is thrown away; food not started is cooked for nobody. */
  it('a cancelled card can only be acknowledged, never cleared', async () => {
    serve(view({ tickets: [ticket({ isCancelled: true, says: 'CANCELLED', tone: 'cancelled' })] }));
    show();

    expect(await screen.findByText('CANCELLED')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Done/ })).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: /Got it/ }));
    await waitFor(() => {
      expect(call).toHaveBeenCalledWith('kitchen_acknowledge', { id: 'kds_1' });
    });
    expect(call).not.toHaveBeenCalledWith('kitchen_bump', expect.anything());
  });

  /** And the number key on a cancelled card acknowledges rather than clears. */
  it('the number key acknowledges a cancelled card', async () => {
    serve(view({ tickets: [ticket({ isCancelled: true, tone: 'cancelled' })] }));
    show();
    await screen.findByText('Table 5');

    fireEvent.keyDown(window, { key: '1' });
    await waitFor(() => {
      expect(call).toHaveBeenCalledWith('kitchen_acknowledge', { id: 'kds_1' });
    });
    expect(call).not.toHaveBeenCalledWith('kitchen_bump', expect.anything());
  });

  /** Tapping one dish as it comes off the pass. */
  it('ticks one dish off without touching the others', async () => {
    serve(view({ tickets: [ticket({ lines: [line(), line({ key: 'k2', name: 'Paneer Tikka' })] })] }));
    show();

    fireEvent.click(await screen.findByText('Paneer Tikka'));
    await waitFor(() => {
      expect(call).toHaveBeenCalledWith('kitchen_bump_line', { id: 'kds_1', key: 'k2' });
    });
  });

  /** The undo has to be reachable. */
  it('offers the last cleared card back, by name', async () => {
    serve(view({ lastCleared: { id: 'kds_9', what: 'Table 5 #12' } }));
    show();

    const back = await screen.findByRole('button', { name: /Bring back Table 5 #12/ });
    fireEvent.click(back);
    await waitFor(() => {
      expect(call).toHaveBeenCalledWith('kitchen_recall', { id: 'kds_9' });
    });
  });

  it('offers nothing to bring back when nothing has been cleared', async () => {
    serve(view());
    show();

    await screen.findByText('Table 5');
    expect(screen.queryByRole('button', { name: /Bring back/ })).toBeNull();
  });

  /** 5, and the shops that do not use courses never see any of it. */
  it('fires the next course, and hides the whole idea from a shop without one', async () => {
    serve(view());
    const plain = show();
    await screen.findByText('Table 5');
    expect(screen.queryByText(/Ready to fire/)).toBeNull();
    plain.unmount();

    serve(
      view({
        waitingCourses: [
          { orderId: 'ord_1', place: 'Table 5', course: 'Main', what: '2 dishes' },
        ],
      }),
    );
    show();

    fireEvent.click(await screen.findByRole('button', { name: /Main/ }));
    await waitFor(() => {
      expect(call).toHaveBeenCalledWith('kitchen_fire', { orderId: 'ord_1', course: 'Main' });
    });
  });

  /** A shop with one station is not asked to choose between one thing. */
  it('only offers the station tabs when there is more than one', async () => {
    serve(view({ station: 'Kitchen', stations: ['Kitchen'] }));
    const one = show();
    await screen.findByText('Table 5');
    expect(screen.queryByRole('button', { name: 'Kitchen' })).toBeNull();
    one.unmount();

    serve(view());
    show();
    expect(await screen.findByRole('button', { name: 'Kitchen' })).toBeTruthy();
  });

  it('says so plainly when the kitchen is clear', async () => {
    serve(view({ tickets: [], headline: 'Nothing waiting.' }));
    show();

    expect(await screen.findByText(/Nothing waiting. The kitchen is clear./)).toBeTruthy();
  });

  it('runs one clock for the whole screen, however many cards', async () => {
    vi.useFakeTimers();

    serve(view({ tickets: [ticket()] }));
    const one = show();
    await vi.advanceTimersByTimeAsync(0);
    const withOneCard = vi.getTimerCount();
    one.unmount();

    serve(view({ tickets: Array.from({ length: 12 }, (_, n) => ticket({ id: `kds_${n}` })) }));
    show();
    await vi.advanceTimersByTimeAsync(0);

    // The number does not grow with the grid.
    expect(vi.getTimerCount()).toBe(withOneCard);

    vi.useRealTimers();
  });

  /** A kitchen screen that goes blank is the failure this feature exists to prevent. */
  it('keeps the last cards on screen when a read fails', async () => {
    serve(view());
    show();
    await screen.findByText('Butter Naan');

    call.mockImplementation(() => Promise.reject(new Error('the shop is unreadable')));
    fireEvent.keyDown(window, { key: 'x' });

    await new Promise((r) => setTimeout(r, 10));
    expect(screen.getByText('Butter Naan')).toBeTruthy();
  });
});
