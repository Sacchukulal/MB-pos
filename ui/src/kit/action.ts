/**
 * **A button is dead while its own work is running** — the owner's round of
 * 22 August 2026.
 *
 * The counter had 401 clickable actions and 49 of them switched off while they
 * worked. The two that mattered most were in the other 352: the kitchen ticket
 * and Settle were disabled only when the bill was empty, so nothing at all
 * stopped a cashier pressing them five times while the first press was still
 * being written.
 *
 * At a real counter that is not an edge case, it is *the* case: nothing has
 * happened yet, so you press again.
 *
 * # The ref is the guard, not the state
 *
 * `setBusy` schedules a re-render; it does not change anything this instant.
 * Two clicks inside the same tick would both read `busy === false` and both go
 * through, which is the bug wearing a disguise. The ref changes on the line it
 * is written, so the second press is refused before React has been told
 * anything. The state exists only so the button can *look* disabled.
 *
 * # And this is the second line of defence, never the first
 *
 * Rust holds the counter to one action at a time (`App::begin_action`). It has
 * to: a screen can be bypassed by a keyboard shortcut, by an order arriving
 * from a phone over the network, or by a second terminal — none of which can
 * see this hook. What this adds is that the cashier can *see* the counter is
 * busy instead of pressing into silence.
 */

import { useCallback, useRef, useState } from 'react';

export type Action = (work: () => Promise<unknown>) => void;

/**
 * Returns the runner and whether it is busy.
 *
 * ```tsx
 * const [run, busy] = useAction();
 * <Button disabled={busy} onClick={() => run(settleTheBill)}>Settle</Button>
 * ```
 *
 * A press that arrives while the previous one is still running is **dropped**,
 * not queued: the cashier meant to do the thing once, and doing it twice a
 * moment later is the whole problem.
 */
export function useAction(): [Action, boolean] {
  const [busy, setBusy] = useState(false);
  const running = useRef(false);

  const run = useCallback<Action>((work) => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    void work()
      // **Every action in this product reports its own failure** — it catches,
      // calls `report`, and returns normally. That was already the assumption
      // behind `onClick={() => void settle()}`, and nothing here changes what a
      // cashier sees. This exists so a rejection cannot escape as an unhandled
      // one from a promise the hook owns, and so the button below is released
      // either way: an action that fails must never leave the till frozen.
      .catch(() => {})
      .finally(() => {
        running.current = false;
        setBusy(false);
      });
  }, []);

  return [run, busy];
}
