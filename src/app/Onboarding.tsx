import { FolderOpen } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { STORAGE_KEYS } from "../config/constants";

interface OnboardingProps {
  onFolderSelected: (path: string) => void;
}

/** First-run screen: the user must choose where the database lives. */
export default function Onboarding({ onFolderSelected }: OnboardingProps) {
  const selectFolder = async () => {
    try {
      const selected = await open({ directory: true, multiple: false, title: "Select Database Folder" });
      if (selected && typeof selected === "string") {
        localStorage.setItem(STORAGE_KEYS.dbFolderPath, selected);
        onFolderSelected(selected);
      }
    } catch (error) {
      console.error("Failed to open folder dialog:", error);
    }
  };

  return (
    <div className="onboarding">
      <FolderOpen size={64} className="onboarding-icon" />
      <h1>Welcome to Magic Bill</h1>
      <p>
        To get started, please select a folder where your database and all application data will be safely stored.
      </p>
      <button className="btn btn--primary btn--lg" onClick={selectFolder}>
        <FolderOpen size={20} />
        Select Database Folder
      </button>
    </div>
  );
}
