import { act, cleanup, render, renderHook, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useArrivals } from '../src/billing/arrivals';
import { TableGrid } from '../src/billing/TableGrid';
import { Processing } from '../src/billing/Processing';
import type { TableView } from '../src/ipc/generated/TableView';
import { beep } from '../src/kit';

vi.mock('../src/kit', async (original) => ({
  ...await original<typeof import('../src/kit')>(),
  beep: vi.fn(),
}));

const order = (id: string, arrivalKey = 'food-v1'): TableView => ({
  id, label: id, orderId: id, arrivalKey, processing: true,
  section: 'Main', sectionOrder: 0, seats: 4, state: 'occupied',
  total: { paise: 10000n, text: '100.00' }, minutes: 1, createdAt: 1,
  kitchenTold: true, kitchenMinutes: 1, billAsked: false, settleAsked: false,
  by: null, byId: null, seat: null, token: null, billNumber: null, selected: false,
});

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  document.documentElement.style.setProperty('--arrival-duration', '3000ms');
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  document.documentElement.removeAttribute('style');
});

describe('order arrival lifetime', () => {
  it('restarts a same-total item update and leaves the second pulse for its full duration', () => {
    const { result, rerender } = renderHook(({ floor }) => useArrivals(floor, { sound: true }), {
      initialProps: { floor: [order('1')] },
    });
    rerender({ floor: [order('1', 'food-v2')] });
    const first = result.current.get('1');
    expect(first).toBeDefined();
    act(() => vi.advanceTimersByTime(2000));
    rerender({ floor: [order('1', 'food-v3')] });
    const second = result.current.get('1');
    expect(second).not.toBe(first);
    act(() => vi.advanceTimersByTime(1000));
    expect(result.current.get('1')).toBe(second);
    // A duplicate event response, selection, timers and a payment request do not replay it.
    rerender({ floor: [{ ...order('1', 'food-v3'), minutes: 9, selected: true, billAsked: true }] });
    expect(result.current.get('1')).toBe(second);
    expect(beep).toHaveBeenCalledTimes(2);
    act(() => vi.advanceTimersByTime(1999));
    expect(result.current.has('1')).toBe(true);
    act(() => vi.advanceTimersByTime(1));
    expect(result.current.size).toBe(0);
  });

  it('tracks simultaneous orders separately, removes settled orders, and cancels clocks on unmount', () => {
    const { result, rerender, unmount } = renderHook(({ floor }) => useArrivals(floor, { sound: true }), {
      initialProps: { floor: [] as TableView[] },
    });
    rerender({ floor: [order('1'), order('2')] });
    expect([...result.current.keys()]).toEqual(['1', '2']);
    expect(beep).toHaveBeenCalledTimes(1);
    act(() => vi.advanceTimersByTime(2000));
    rerender({ floor: [order('1', 'more'), order('2')] });
    act(() => vi.advanceTimersByTime(1000));
    expect([...result.current.keys()]).toEqual(['1']);
    rerender({ floor: [order('2')] });
    expect(result.current.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
    rerender({ floor: [order('2'), order('3')] });
    unmount();
    expect(vi.getTimerCount()).toBe(0);
    const mounted = renderHook(() => useArrivals([order('2'), order('3')]));
    expect(mounted.result.current.size).toBe(0);
  });

  it('keeps a short cue when motion tokens are zero, while honoring the explicit disabled setting', () => {
    document.documentElement.style.setProperty('--motion-beat', '0ms');
    document.documentElement.style.setProperty('--beats', '0');
    const { result, rerender } = renderHook(({ floor, beat }) => useArrivals(floor, { beat }), {
      initialProps: { floor: [] as TableView[], beat: true },
    });
    rerender({ floor: [order('1')], beat: true });
    act(() => vi.advanceTimersByTime(2000));
    expect(result.current.has('1')).toBe(true);
    rerender({ floor: [order('1')], beat: false });
    expect(result.current.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
    rerender({ floor: [order('1'), order('2')], beat: false });
    expect(result.current.size).toBe(0);
    rerender({ floor: [order('1'), order('2')], beat: true });
    expect(result.current.size).toBe(0);
  });

  it('replaces only the passive pulse in the grid and processing list, preserving focus', () => {
    const tiles = [order('1')];
    const draw = (generation: number) => <>
      <TableGrid tables={tiles} filter="" onOpen={() => {}} onPrintBill={() => {}} arrived={new Map([['1', generation]])} />
      <Processing orders={tiles} onOpen={() => {}} arrived={new Map([['1', generation]])} />
    </>;
    const { container, rerender } = render(draw(1));
    const button = screen.getByRole('button', { name: /Table 1, Main/ });
    button.focus();
    const pulses = [...container.querySelectorAll('.mb-arrival-pulse')];
    expect(pulses).toHaveLength(2);
    rerender(draw(2));
    expect(screen.getByRole('button', { name: /Table 1, Main/ })).toBe(button);
    expect(document.activeElement).toBe(button);
    const restarted = [...container.querySelectorAll('.mb-arrival-pulse')];
    expect(restarted[0]).not.toBe(pulses[0]);
    expect(restarted[1]).not.toBe(pulses[1]);
  });
});
