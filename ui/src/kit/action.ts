/** A button is dead while its own work is running. */

import { useCallback, useRef, useState } from 'react';

export type Action = (work: () => Promise<unknown>) => void;

/** Returns the runner and whether it is busy. */
export function useAction(): [Action, boolean] {
  const [busy, setBusy] = useState(false);
  const running = useRef(false);

  const run = useCallback<Action>((work) => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    void work()
      // Every action in this product reports its own failure — it catches, calls `report`, and
      // returns normally.
      .catch(() => {})
      .finally(() => {
        running.current = false;
        setBusy(false);
      });
  }, []);

  return [run, busy];
}
