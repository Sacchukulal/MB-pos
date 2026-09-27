import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

import type { CartView } from '../src/ipc/generated/CartView';
import type { Pushed } from '../src/ipc/generated/Pushed';

const { call, subscribe } = vi.hoisted(() => ({ call: vi.fn(), subscribe: vi.fn() }));
vi.mock('../src/ipc/call', () => ({ call, subscribe, inApp: () => true }));
import { useFloorUpdates } from '../src/billing/floorUpdates';

let push: (message: Pushed) => void;
let cartRevision: { current: number };
const stop = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  cartRevision = { current: 0 };
  subscribe.mockImplementation((receive: typeof push) => {
    push = receive;
    return Promise.resolve(stop);
  });
});
afterEach(cleanup);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

it.each<Pushed>([{ kind: 'floor' }, { kind: 'floorChanged', waiting: 1 }])(
  'refreshes both the open cart and tiles for $kind',
  async (message) => {
    const cart = { orderId: 'table_4' } as CartView;
    call.mockResolvedValue(cart);
    const show = vi.fn();
    const floor = vi.fn().mockResolvedValue(undefined);
    renderHook(() => useFloorUpdates(show, floor, cartRevision));
    await act(async () => { push(message); });
    expect(call).toHaveBeenCalledWith('current_cart');
    expect(show).toHaveBeenCalledWith(cart);
    expect(floor).toHaveBeenCalledTimes(1);
  },
);

it('does not replace a newer cart with a slow reply from an earlier floor change', async () => {
  const older = deferred<CartView>();
  const newer = deferred<CartView>();
  call.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
  const show = vi.fn();
  const floor = vi.fn().mockResolvedValue(undefined);
  renderHook(() => useFloorUpdates(show, floor, cartRevision));
  act(() => {
    push({ kind: 'floor' });
    push({ kind: 'floorChanged', waiting: 1 });
  });
  const fresh = { orderId: 'table_4', lines: [{ qty: '3' }] } as CartView;
  await act(async () => { newer.resolve(fresh); });
  await act(async () => { older.resolve({ orderId: 'table_4', lines: [] } as unknown as CartView); });
  expect(show).toHaveBeenCalledExactlyOnceWith(fresh);
});

it('disposes a subscription that finishes opening after the screen closes', async () => {
  const opening = deferred<() => void>();
  subscribe.mockImplementation((receive: typeof push) => {
    push = receive;
    return opening.promise;
  });
  const show = vi.fn();
  const floor = vi.fn().mockResolvedValue(undefined);
  const { unmount } = renderHook(() => useFloorUpdates(show, floor, cartRevision));
  unmount();
  await act(async () => { opening.resolve(stop); });
  push({ kind: 'floor' });
  expect(stop).toHaveBeenCalledTimes(1);
  expect(call).not.toHaveBeenCalled();
});

it('does not undo a local edit or table switch completed while a floor read was pending', async () => {
  const pending = deferred<CartView>();
  call.mockReturnValue(pending.promise);
  const show = vi.fn();
  const floor = vi.fn().mockResolvedValue(undefined);
  renderHook(() => useFloorUpdates(show, floor, cartRevision));
  act(() => { push({ kind: 'floor' }); });
  // Billing increments this revision whenever a command puts its result on screen.
  cartRevision.current += 1;
  await act(async () => { pending.resolve({ orderId: 'old_table' } as CartView); });
  expect(show).not.toHaveBeenCalled();
});
