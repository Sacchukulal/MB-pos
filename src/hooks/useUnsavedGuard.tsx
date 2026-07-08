import { createContext, useContext, useEffect } from "react";

export type SaveFn = () => Promise<boolean>;

export interface UnsavedGuardContextValue {
  /** Report whether the current screen has unsaved changes, and how to save them. */
  report: (dirty: boolean, save: SaveFn | null) => void;
}

export const UnsavedGuardContext = createContext<UnsavedGuardContextValue>({ report: () => {} });

/**
 * Settings screens call this each render with their dirty flag and save function;
 * the app shell uses it to prompt Save / Discard / Cancel before navigating away.
 */
export function useUnsavedGuard(dirty: boolean, save: SaveFn) {
  const { report } = useContext(UnsavedGuardContext);

  useEffect(() => {
    report(dirty, save);
  }, [dirty, save, report]);

  // Clear the guard when the screen unmounts.
  useEffect(() => {
    return () => report(false, null);
  }, [report]);
}
