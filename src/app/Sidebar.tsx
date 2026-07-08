import {
  DownloadCloud,
  LayoutDashboard,
  Printer,
  Receipt,
  ReceiptText,
  RefreshCw,
  Settings,
  TrendingUp,
  UserCircle,
  Users,
  UtensilsCrossed,
  Wallet,
} from "lucide-react";
import type { ScreenId } from "./screens";
import type { UpdaterActions, UpdaterState } from "./useUpdater";

export const NAV_ITEMS: { id: ScreenId; icon: typeof LayoutDashboard; label: string }[] = [
  { id: "dashboard", icon: LayoutDashboard, label: "Dashboard" },
  { id: "billing", icon: ReceiptText, label: "Billing" },
  { id: "expenses", icon: Wallet, label: "Expenses" },
  { id: "reports", icon: TrendingUp, label: "Reports" },
];

export const SETTINGS_ITEMS: { id: ScreenId; icon: typeof Settings; label: string }[] = [
  { id: "settings-general", icon: Settings, label: "General Settings" },
  { id: "settings-bill", icon: Receipt, label: "Bill Settings" },
  { id: "settings-printer", icon: Printer, label: "Printer Settings" },
  { id: "settings-menu", icon: UtensilsCrossed, label: "Menu Management" },
  { id: "settings-staff", icon: Users, label: "Staff Management" },
];

interface SidebarProps {
  active: ScreenId;
  settingsOpen: boolean;
  onNavigate: (id: ScreenId) => void;
  onToggleSettings: () => void;
  updater: UpdaterState & UpdaterActions;
}

export default function Sidebar({ active, settingsOpen, onNavigate, onToggleSettings, updater }: SidebarProps) {
  return (
    <>
      <aside className="sidebar">
        <button
          className={`account-nav-btn ${active === "account" ? "active" : ""}`}
          onClick={() => onNavigate("account")}
          title="Account"
        >
          <div className="account-nav-logo">
            <img src="/magic_bill_logo.png" alt="Magic Bill" />
            <span className="account-nav-badge">
              <UserCircle size={13} />
            </span>
          </div>
          <span className="account-nav-brand">MAGIC BILL</span>
          <span className="account-nav-label">ACCOUNT</span>
        </button>

        <nav className="nav-menu">
          {NAV_ITEMS.map((item) => (
            <button
              key={item.id}
              className={`nav-item ${active === item.id ? "active" : ""}`}
              onClick={() => onNavigate(item.id)}
              title={item.label}
            >
              <item.icon size={24} className="nav-icon" />
              <span className="nav-label">{item.label}</span>
            </button>
          ))}
        </nav>

        <div className="sidebar-footer">
          <button
            className={`nav-item ${settingsOpen ? "active" : ""}`}
            onClick={onToggleSettings}
            title="Settings"
          >
            <Settings size={24} className="nav-icon" />
            <span className="nav-label">Settings</span>
          </button>

          <div className="sidebar-divider" />

          {updater.available ? (
            <button
              className="update-chip available"
              onClick={updater.openModal}
              title={`Update available — v${updater.availableVersion}. Click to install.`}
            >
              <DownloadCloud size={16} className="update-chip-icon" />
              <span className="update-chip-text">
                <span className="update-chip-label">Update</span>
                <span className="update-chip-ver">v{updater.availableVersion}</span>
              </span>
            </button>
          ) : (
            <button
              className="update-chip"
              onClick={updater.checkManually}
              disabled={updater.checking}
              title="Check for updates"
            >
              <RefreshCw size={14} className={`update-chip-icon ${updater.checking ? "ui-spinner-rotate" : ""}`} />
              <span className="update-chip-text">
                <span className="update-chip-ver">{updater.appVersion ? `v${updater.appVersion}` : "—"}</span>
                <span className="update-chip-label">{updater.checking ? "Checking…" : "Check updates"}</span>
              </span>
            </button>
          )}
        </div>
      </aside>

      {/* Slide-out settings panel */}
      <aside className={`settings-panel ${settingsOpen ? "open" : ""}`}>
        <nav className="settings-nav">
          {SETTINGS_ITEMS.map((item) => (
            <button
              key={item.id}
              className={`nav-item settings-item ${active === item.id ? "active" : ""}`}
              onClick={() => onNavigate(item.id)}
              title={item.label}
            >
              <item.icon size={20} className="nav-icon" />
              <span className="nav-label multiline">{item.label}</span>
            </button>
          ))}
        </nav>
      </aside>
    </>
  );
}
