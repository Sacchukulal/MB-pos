/**
 * What the person at the counter may do, for every screen to ask.
 *
 * Rust is the one that says no: every command checks its permission. This is only so a screen
 * can leave out a button that would be refused, instead of showing it and refusing after the
 * press.
 */

import { createContext, useCallback, useContext, type ReactNode } from 'react';

/** `null` outside the shell — a screen drawn on its own (a test, the gallery) hides nothing. */
const Held = createContext<readonly string[] | null>(null);

export function MayProvider({
  held,
  children,
}: {
  /** The permission codes the signed-in person holds — `LockState::permissions`. */
  held: readonly string[];
  children: ReactNode;
}) {
  return <Held.Provider value={held}>{children}</Held.Provider>;
}

/** `may('bill.void')` — true when the person holds it, or when nobody is providing the list. */
export function useMay(): (code: string) => boolean {
  const held = useContext(Held);
  return useCallback((code: string) => held === null || held.includes(code), [held]);
}
