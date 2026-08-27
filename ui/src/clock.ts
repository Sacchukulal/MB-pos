/** One ticking source for the whole product. */

import { useEffect, useState } from 'react';

/** How often the shared clock wakes. */
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

/** Subscribe to the clock. */
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
