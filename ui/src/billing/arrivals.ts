/** One shared arrival tracker for Billing, Processing and the Floor plan. */
import { useEffect, useRef, useState } from 'react';

import { beep } from '../kit';
import type { TableView } from '../ipc/generated/TableView';

/** Each new generation restarts only the passive highlight, never the card's controls. */
export type Arrivals = ReadonlyMap<string, number>;
const NONE: Arrivals = new Map();

function shape(tile: TableView): string {
  // Older saved previews do not have the new field. Live Rust always supplies it.
  return tile.arrivalKey ?? tile.total?.text ?? '';
}

/** The duration remains readable when motion is disabled: the cue then stays still. */
function arrivalDuration(): number {
  const duration = Number.parseFloat(
    getComputedStyle(document.documentElement).getPropertyValue('--arrival-duration'),
  );
  return Number.isFinite(duration) && duration > 0 ? duration : 3000;
}

export function useArrivals(
  floor: readonly TableView[] | null,
  { beat = true, sound = false }: { beat?: boolean; sound?: boolean } = {},
): Arrivals {
  const shown = useRef<Map<string, string> | null>(null);
  const [arrived, setArrived] = useState<Arrivals>(NONE);
  const generation = useRef(0);
  const clocks = useRef(new Map<string, ReturnType<typeof setTimeout>>());

  useEffect(() => {
    if (floor === null) return;
    const now = new Map<string, string>();
    for (const tile of floor) if (tile.orderId !== null) now.set(tile.orderId, shape(tile));
    const before = shown.current;
    shown.current = now;
    if (before === null) return;

    // Settlement/removal is not an arrival and must not leave a timer behind.
    for (const [id, clock] of clocks.current) {
      if (!now.has(id)) {
        clearTimeout(clock);
        clocks.current.delete(id);
      }
    }
    const fresh = [...now].filter(([id, value]) => before.get(id) !== value).map(([id]) => id);
    if (fresh.length > 0 && sound) beep();
    const additions = new Map<string, number>();
    if (beat) {
      for (const id of fresh) {
        const previousClock = clocks.current.get(id);
        if (previousClock !== undefined) clearTimeout(previousClock);
        const next = ++generation.current;
        additions.set(id, next);
        const clock = setTimeout(() => {
          // An old callback cannot expire a newer arrival for this order.
          if (clocks.current.get(id) !== clock) return;
          clocks.current.delete(id);
          setArrived((was) => {
            if (was.get(id) !== next) return was;
            const left = new Map(was);
            left.delete(id);
            return left.size === 0 ? NONE : left;
          });
        }, arrivalDuration());
        clocks.current.set(id, clock);
      }
    }
    setArrived((was) => {
      const next = new Map([...was].filter(([id]) => now.has(id)));
      for (const [id, value] of additions) next.set(id, value);
      if (next.size === was.size && [...next].every(([id, value]) => was.get(id) === value)) return was;
      return next.size === 0 ? NONE : next;
    });
    // Settings are applied to new arrivals; changing them does not replay the floor.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [floor]);

  useEffect(() => {
    if (beat) return;
    for (const clock of clocks.current.values()) clearTimeout(clock);
    clocks.current.clear();
    setArrived(NONE);
  }, [beat]);

  useEffect(() => {
    const running = clocks.current;
    return () => {
      for (const clock of running.values()) clearTimeout(clock);
      running.clear();
    };
  }, []);

  return arrived;
}

export function arrivalFor(arrived: Arrivals | undefined, tile: TableView): number | undefined {
  return tile.orderId === null ? undefined : arrived?.get(tile.orderId);
}
