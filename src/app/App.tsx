import { useCallback, useEffect, useRef, useState } from "react";
import { STORAGE_KEYS } from "../config/constants";
import { openDatabase } from "../db/client";
import { syncSubscriptionFromRemote } from "../services/license/licenseService";
import { SettingsProvider, useSettings } from "../hooks/useSettings";
import { ToastProvider } from "../hooks/useToast";
import { UnsavedGuardContext, type SaveFn } from "../hooks/useUnsavedGuard";
import Modal from "../components/ui/Modal";
import Onboarding from "./Onboarding";
import Sidebar from "./Sidebar";
import UpdateUI from "./UpdateUI";
import { useUpdater } from "./useUpdater";
import { SETTINGS_SCREENS, type ScreenId } from "./screens";

import Dashboard from "../features/dashboard/Dashboard";
import Billing from "../features/billing/Billing";
import Expenses from "../features/expenses/Expenses";
import Reports from "../features/reports/Reports";
import Account from "../features/account/Account";
import GeneralSettings from "../features/settings/GeneralSettings";
import BillSettings from "../features/settings/BillSettings";
import PrinterSettings from "../features/settings/PrinterSettings";
import MenuManagement from "../features/menu/MenuManagement";
import StaffManagement from "../features/staff/StaffManagement";

function Screen({ id, dbReady }: { id: ScreenId; dbReady: boolean }) {
  switch (id) {
    case "dashboard":
      return <Dashboard dbReady={dbReady} />;
    case "billing":
      return <Billing dbReady={dbReady} />;
    case "expenses":
      return <Expenses dbReady={dbReady} />;
    case "reports":
      return <Reports dbReady={dbReady} />;
    case "account":
      return <Account dbReady={dbReady} />;
    case "settings-general":
      return <GeneralSettings dbReady={dbReady} />;
    case "settings-bill":
      return <BillSettings dbReady={dbReady} />;
    case "settings-printer":
      return <PrinterSettings dbReady={dbReady} />;
    case "settings-menu":
      return <MenuManagement dbReady={dbReady} />;
    case "settings-staff":
      return <StaffManagement dbReady={dbReady} />;
  }
}

function AppShell({ dbReady }: { dbReady: boolean }) {
  const [active, setActive] = useState<ScreenId>("dashboard");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const updater = useUpdater();
  const { reload } = useSettings();

  // --- Unsaved-changes guard -----------------------------------------
  const dirtyRef = useRef(false);
  const saveRef = useRef<SaveFn | null>(null);
  const [pendingNav, setPendingNav] = useState<ScreenId | null>(null);

  const report = useCallback((dirty: boolean, save: SaveFn | null) => {
    dirtyRef.current = dirty;
    saveRef.current = save;
  }, []);

  const completeNavigation = useCallback(
    (target: ScreenId) => {
      const leavingSettings = SETTINGS_SCREENS.includes(active);
      setActive(target);
      if (!SETTINGS_SCREENS.includes(target)) setSettingsOpen(false);
      // Screens read settings from context — refresh it after leaving a settings tab.
      if (leavingSettings) reload().catch(() => {});
    },
    [active, reload]
  );

  const navigate = useCallback(
    (target: ScreenId) => {
      if (target === active) {
        if (!SETTINGS_SCREENS.includes(target)) setSettingsOpen(false);
        return;
      }
      if (dirtyRef.current) {
        setPendingNav(target);
        return;
      }
      completeNavigation(target);
    },
    [active, completeNavigation]
  );

  const resolvePendingNav = useCallback(
    async (action: "save" | "discard" | "cancel") => {
      const target = pendingNav;
      setPendingNav(null);
      if (action === "cancel" || !target) return;
      if (action === "save" && saveRef.current) {
        const ok = await saveRef.current();
        if (!ok) return;
      }
      dirtyRef.current = false;
      completeNavigation(target);
    },
    [pendingNav, completeNavigation]
  );

  return (
    <UnsavedGuardContext.Provider value={{ report }}>
      <div className="app-shell">
        <Sidebar
          active={active}
          settingsOpen={settingsOpen}
          onNavigate={navigate}
          onToggleSettings={() => setSettingsOpen((v) => !v)}
          updater={updater}
        />
        <main className="main-content">
          <Screen id={active} dbReady={dbReady} />
        </main>
      </div>

      {pendingNav && (
        <Modal width="420px" showClose={false}>
          <h3 className="ui-modal-title">Unsaved Changes</h3>
          <p className="ui-modal-message">
            You have unsaved changes in the current settings tab. Do you want to save them before leaving?
          </p>
          <div className="ui-modal-actions">
            <button className="btn btn--ghost" onClick={() => resolvePendingNav("cancel")}>
              Cancel
            </button>
            <button className="btn btn--danger" onClick={() => resolvePendingNav("discard")}>
              Discard
            </button>
            <button className="btn btn--primary" onClick={() => resolvePendingNav("save")}>
              Save
            </button>
          </div>
        </Modal>
      )}

      <UpdateUI {...updater} />
    </UnsavedGuardContext.Provider>
  );
}

export default function App() {
  const [dbFolderPath, setDbFolderPath] = useState<string | null>(() =>
    localStorage.getItem(STORAGE_KEYS.dbFolderPath)
  );
  const [dbReady, setDbReady] = useState(false);
  const [dbError, setDbError] = useState<string | null>(null);

  useEffect(() => {
    if (!dbFolderPath) return;
    let cancelled = false;
    (async () => {
      try {
        await openDatabase(dbFolderPath);
        if (cancelled) return;
        setDbReady(true);
        // Refresh the license snapshot in the background (silent when offline).
        syncSubscriptionFromRemote().catch(() => {});
      } catch (error) {
        console.error("Failed to initialize database:", error);
        if (!cancelled) setDbError(String(error));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [dbFolderPath]);

  if (!dbFolderPath) {
    return <Onboarding onFolderSelected={setDbFolderPath} />;
  }

  if (dbError) {
    return (
      <div className="onboarding">
        <h1>Database Error</h1>
        <p className="text-danger">{dbError}</p>
        <button
          className="btn btn--ghost"
          onClick={() => {
            localStorage.removeItem(STORAGE_KEYS.dbFolderPath);
            setDbError(null);
            setDbFolderPath(null);
          }}
        >
          Choose a different folder
        </button>
      </div>
    );
  }

  return (
    <ToastProvider>
      <SettingsProvider dbReady={dbReady}>
        <AppShell dbReady={dbReady} />
      </SettingsProvider>
    </ToastProvider>
  );
}
