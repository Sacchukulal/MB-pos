/**
 * Which orders turned up, or grew, while the screen was already open. Rust says only that the
 * floor changed; the difference between the list a screen was showing and the one it shows
 * now is the one fact nobody but the screen has — so it is worked out here, once, and every
 * card drawn from the list asks it the same question. The billing grid, the processing list
 * and the floor plan all ask it.
 */

import { useEffect, useRef, useState } from 'react';

import { beatsFor, beep } from '../kit';
import type { TableView } from '../ipc/generated/TableView';

const NONE: ReadonlySet<string> = new Set();

/**
 * What a card would have to change for a cashier to want to look at it: it is a new order,
 * or its bill is not the bill it was — a phone added food, or took some off. The timers move
 * on their own and mean nothing here.
 */
function shape(tile: TableView): string {
  return tile.total?.text ?? '';
}

/**
 * The ids of the orders that arrived or changed since the last read — for as long as the
 * theme beats, then gone. `null` is a floor not read yet: the first list to come from Rust is
 * what the screen opened on, and nothing on it is new. `sound` is the shop's switch: each
 * arrival beeps once, alongside the beat.
 */
export function useArrivals(
  floor: readonly TableView[] | null,
  sound = false,
): ReadonlySet<string> {
  const shown = useRef<Map<string, string> | null>(null);
  const [arrived, setArrived] = useState<ReadonlySet<string>>(NONE);
  /** Every arrival stops on its own clock; a later read of the floor must not reset it. */
  const clocks = useRef(new Set<ReturnType<typeof setTimeout>>());

  useEffect(() => {
    if (floor === null) return;
    const now = new Map<string, string>();
    for (const tile of floor) if (tile.orderId !== null) now.set(tile.orderId, shape(tile));
    const before = shown.current;
    shown.current = now;
    if (before === null) return;

    const fresh = [...now].filter(([id, it]) => before.get(id) !== it).map(([id]) => id);
    if (fresh.length === 0) return;
    setArrived((was) => new Set([...was, ...fresh]));
    if (sound) beep();
    const clock = setTimeout(() => {
      clocks.current.delete(clock);
      setArrived((was) => {
        const left = new Set(was);
        for (const id of fresh) left.delete(id);
        return left.size === 0 ? NONE : left;
      });
    }, beatsFor());
    clocks.current.add(clock);
    // `sound` is read when the floor changes, never a reason to look again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

/** Whether this card is one of them — the one test every card runs. */
export function hasArrived(arrived: ReadonlySet<string> | undefined, tile: TableView): boolean {
  return tile.orderId !== null && arrived?.has(tile.orderId) === true;
}
