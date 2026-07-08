import { DownloadCloud, XCircle } from "lucide-react";
import Modal from "../components/ui/Modal";
import type { UpdaterActions, UpdaterState } from "./useUpdater";

/** Update install dialog + skippable auto-check snackbar + status toast. */
export default function UpdateUI(updater: UpdaterState & UpdaterActions) {
  return (
    <>
      {updater.showModal && (
        <Modal heavy width="400px" showClose={false} className="update-modal">
          {updater.install.state === "downloading" ? (
            <>
              <div className="update-icon-wrap">
                <DownloadCloud size={36} />
              </div>
              <h3 className="ui-modal-title text-center">Updating Magic Bill…</h3>
              <p className="ui-modal-message text-center">
                Please don't close the app — it will restart automatically when finished.
              </p>
              <div className="update-progress">
                <div className="update-progress-bar" style={{ width: `${updater.install.progress}%` }} />
              </div>
              <span className="update-progress-pct">{updater.install.progress}%</span>
            </>
          ) : updater.install.state === "error" ? (
            <>
              <div className="update-icon-wrap danger">
                <XCircle size={36} />
              </div>
              <h3 className="ui-modal-title text-center">Update Failed</h3>
              <p className="ui-modal-message text-center text-danger">{updater.install.error}</p>
              <div className="ui-modal-actions" style={{ justifyContent: "center" }}>
                <button className="btn btn--ghost" onClick={updater.closeModal}>
                  Close
                </button>
                <button className="btn btn--primary" onClick={updater.startInstall}>
                  Retry
                </button>
              </div>
            </>
          ) : (
            <>
              <div className="update-icon-wrap accent">
                <DownloadCloud size={36} />
              </div>
              <h3 className="ui-modal-title text-center">Update Available</h3>
              <p className="ui-modal-message text-center">
                Magic Bill <strong>v{updater.availableVersion}</strong> is ready to install. The app will restart to
                finish the update.
              </p>
              <div className="ui-modal-actions" style={{ justifyContent: "center" }}>
                <button className="btn btn--ghost" onClick={updater.closeModal}>
                  Later
                </button>
                <button className="btn btn--primary" onClick={updater.startInstall}>
                  <DownloadCloud size={16} /> Download &amp; Install
                </button>
              </div>
            </>
          )}
        </Modal>
      )}

      {updater.showSnackbar && (
        <div className="update-snackbar">
          <DownloadCloud size={22} className="update-snackbar-icon" />
          <div className="update-snackbar-text">
            <strong>Update available</strong>
            <span>Magic Bill v{updater.availableVersion} is ready to install.</span>
          </div>
          <div className="update-snackbar-actions">
            <button className="btn btn--primary btn--sm" onClick={updater.snackbarToModal}>
              Update
            </button>
            <button className="btn btn--ghost btn--sm" onClick={updater.dismissSnackbar}>
              Skip
            </button>
          </div>
        </div>
      )}

      {updater.toast && <div className="toast-stack"><div className="toast">{updater.toast}</div></div>}
    </>
  );
}
