/**
 * Which orders turned up while the screen was already open. Rust says only that the floor
 * changed; the difference between the list a screen was showing and the one it shows now is
 * the one fact nobody but the screen has — so it is worked out here, once, and every card
 * drawn from the list asks it the same question.
 */

import { useEffect, useRef, useState } from 'react';

import { beatsFor } from '../kit';
import type { TableView } from '../ipc/generated/TableView';

const NONE: ReadonlySet<string> = new Set();

/**
 * The ids of the orders that arrived since the last read — for as long as the theme beats,
 * then gone. `null` is a floor not read yet: the first list to come from Rust is what the
 * screen opened on, and nothing on it is new.
 */
export function useArrivals(floor: readonly TableView[] | null): ReadonlySet<string> {
  const shown = useRef<Set<string> | null>(null);
  const [arrived, setArrived] = useState<ReadonlySet<string>>(NONE);
  /** Every arrival stops on its own clock; a later read of the floor must not reset it. */
  const clocks = useRef(new Set<ReturnType<typeof setTimeout>>());

  useEffect(() => {
    if (floor === null) return;
    const now = new Set<string>();
    for (const tile of floor) if (tile.orderId !== null) now.add(tile.orderId);
    const before = shown.current;
    shown.current = now;
    if (before === null) return;

    const fresh = [...now].filter((id) => !before.has(id));
    if (fresh.length === 0) return;
    setArrived((was) => new Set([...was, ...fresh]));
    const clock = setTimeout(() => {
      clocks.current.delete(clock);
      setArrived((was) => {
        const left = new Set(was);
        for (const id of fresh) left.delete(id);
        return left.size === 0 ? NONE : left;
      });
    }, beatsFor());
    clocks.current.add(clock);
  }, [floor]);

  // Leaving the screen: nothing left to draw attention to.
  useEffect(() => {
    const running = clocks.current;
    return () => {
      for (const clock of running) clearTimeout(clock);
      running.clear();
    };
  }, []);

  return arrived;
}
