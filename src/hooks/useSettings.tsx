import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { loadSettings, applyDailyResetIfNeeded } from "../db/repositories/settingsRepo";
import type { AppSettings } from "../types";

interface SettingsContextValue {
  /** null until the database is open and settings have loaded. */
  settings: AppSettings | null;
  /** Re-read all settings from the database (call after a save). */
  reload: () => Promise<void>;
}

const SettingsContext = createContext<SettingsContextValue | undefined>(undefined);

export function SettingsProvider({ dbReady, children }: { dbReady: boolean; children: ReactNode }) {
  const [settings, setSettings] = useState<AppSettings | null>(null);

  const reload = useCallback(async () => {
    const loaded = await loadSettings();
    // Token/bill counters roll over to their starting numbers on the first load of a new day.
    loaded.printer = await applyDailyResetIfNeeded(loaded.printer);
    setSettings(loaded);
  }, []);

  useEffect(() => {
    if (dbReady) {
      reload().catch((err) => console.error("Failed to load settings:", err));
    }
  }, [dbReady, reload]);

  return <SettingsContext.Provider value={{ settings, reload }}>{children}</SettingsContext.Provider>;
}

export function useSettings(): SettingsContextValue {
  const ctx = useContext(SettingsContext);
  if (!ctx) throw new Error("useSettings must be used within a SettingsProvider");
  return ctx;
}
