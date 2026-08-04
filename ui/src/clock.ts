/**
 * **One ticking source for the whole product.**
 *
 * `PERFORMANCE.md` §5 rule 10: *"timers are one clock — table timers, KDS
 * timers and elapsed displays share a single ticking source, not one interval
 * per tile."* Budgets **B8** (a 60-table grid repaints in one frame) and **M3**
 * (memory growth across eight hours is bounded).
 *
 * This is the **only `setInterval` in the product**, and `tests/guards.test.ts`
 * asserts that — it allows this file and nothing else. A second one is either a
 * poll (M4) or a second clock (B8), and both are forbidden.
 *
 * # Why fifteen seconds
 *
 * A table that has been open forty-one minutes does not need a 1 Hz repaint,
 * and the thing being shown is a *minute* count. Fifteen seconds means the
 * displayed minute is never more than fifteen seconds stale, at a twentieth of
 * the wake-ups a per-second clock would cost — which on the reference machine's
 * i3 is the difference between M4's 1 % idle CPU and something a shopkeeper can
 * hear the fan doing.
 *
 * # The screen never counts
 *
 * Subscribers re-read an elapsed time the **order** carries and Rust computed.
 * A screen that kept its own count would lose it on every re-render, which is
 * the same argument D5 makes about business days: derive once, from the stored
 * value, never accumulate.
 */

import { useEffect, useState } from 'react';

/** How often the shared clock wakes. See the note above before changing it. */
export const TICK_MS = 15_000;

type Listener = () => void;

const listeners = new Set<Listener>();
let timer: ReturnType<typeof setInterval> | undefined;

function start(): void {
  if (timer !== undefined) return;
  timer = setInterval(() => {
    for (const listener of listeners) listener();
  }, TICK_MS);
}

function stop(): void {
  if (timer === undefined) return;
  clearInterval(timer);
  timer = undefined;
}

/**
 * Subscribe to the clock. Returns a number that changes on every tick, so a
 * component can simply put it in a dependency array.
 *
 * The interval exists only while something is listening: a counter sitting on
 * the settings screen has no timers to run, and M4 is measured with the app
 * idle.
 */
export function useTick(): number {
  const [tick, setTick] = useState(0);

  useEffect(() => {
    const listener = () => setTick((n) => n + 1);
    listeners.add(listener);
    start();
    return () => {
      listeners.delete(listener);
      if (listeners.size === 0) stop();
    };
  }, []);

  return tick;
}
