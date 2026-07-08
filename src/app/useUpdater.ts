import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** Silent auto-check runs this long after launch to keep startup fast. */
const AUTO_CHECK_DELAY_MS = 5 * 60 * 1000;
const TOAST_DURATION_MS = 3500;

export interface UpdaterState {
  appVersion: string;
  available: boolean;
  availableVersion?: string;
  checking: boolean;
  install: { state: "idle" | "downloading" | "error"; progress: number; error?: string };
  showModal: boolean;
  showSnackbar: boolean;
  toast: string | null;
}

export interface UpdaterActions {
  checkManually: () => void;
  openModal: () => void;
  closeModal: () => void;
  dismissSnackbar: () => void;
  snackbarToModal: () => void;
  startInstall: () => void;
}

/**
 * Auto-update state machine: manual check (modal/toast feedback), delayed silent
 * auto-check (skippable snackbar), and modal-driven download + relaunch.
 */
export function useUpdater(): UpdaterState & UpdaterActions {
  const [appVersion, setAppVersion] = useState("");
  const [available, setAvailable] = useState(false);
  const [availableVersion, setAvailableVersion] = useState<string | undefined>();
  const [checking, setChecking] = useState(false);
  const [install, setInstall] = useState<UpdaterState["install"]>({ state: "idle", progress: 0 });
  const [showModal, setShowModal] = useState(false);
  const [showSnackbar, setShowSnackbar] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch((err) => console.error("Failed to get app version:", err));
  }, []);

  const runCheck = useCallback(
    async (manual: boolean) => {
      if (checking || install.state === "downloading") return;
      try {
        if (manual) setChecking(true);
        const update = await check();
        if (update) {
          setAvailable(true);
          setAvailableVersion(update.version);
          if (manual) setShowModal(true);
          else setShowSnackbar(true);
        } else {
          setAvailable(false);
          if (manual) setToast("You're on the latest version ✓");
        }
      } catch (error) {
        console.error("Failed to check for updates:", error);
        if (manual) setToast("Couldn't check for updates. Try again later.");
      } finally {
        setChecking(false);
      }
    },
    [checking, install.state]
  );

  // One silent auto-check, delayed off the startup path.
  useEffect(() => {
    const t = setTimeout(() => runCheck(false), AUTO_CHECK_DELAY_MS);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Transient status toast auto-dismiss.
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), TOAST_DURATION_MS);
    return () => clearTimeout(t);
  }, [toast]);

  const startInstall = useCallback(async () => {
    try {
      setInstall({ state: "downloading", progress: 0 });
      const update = await check();
      if (!update) {
        // No longer available (already updated elsewhere).
        setInstall({ state: "idle", progress: 0 });
        setAvailable(false);
        setShowModal(false);
        return;
      }
      let downloaded = 0;
      let contentLength = 0;
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            contentLength = event.data.contentLength || 0;
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            if (contentLength > 0) {
              setInstall({ state: "downloading", progress: Math.round((downloaded / contentLength) * 100) });
            }
            break;
          case "Finished":
            setInstall({ state: "downloading", progress: 100 });
            break;
        }
      });
      await relaunch();
    } catch (error: any) {
      console.error("Failed to update:", error);
      setInstall({ state: "error", progress: 0, error: error?.message || String(error) });
    }
  }, []);

  return {
    appVersion,
    available,
    availableVersion,
    checking,
    install,
    showModal,
    showSnackbar,
    toast,
    checkManually: () => runCheck(true),
    openModal: () => setShowModal(true),
    closeModal: () => {
      setShowModal(false);
      if (install.state === "error") setInstall({ state: "idle", progress: 0 });
    },
    dismissSnackbar: () => setShowSnackbar(false),
    snackbarToModal: () => {
      setShowSnackbar(false);
      setShowModal(true);
    },
    startInstall,
  };
}
