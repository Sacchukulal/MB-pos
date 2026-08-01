import { useEffect, useState } from "react";
import {
  Building2,
  Check,
  FileText,
  FolderOpen,
  HardDrive,
  Hash,
  MapPin,
  Moon,
  Palette,
  Phone,
  QrCode,
  RotateCcw,
  RefreshCw,
  Save,
  Store,
  Sun,
  UploadCloud,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getBillSyncStatus,
  retryStuckBills,
  subscribeBillSync,
  type BillSyncStatus,
} from "../../services/sync/billSync";
import { STORAGE_KEYS } from "../../config/constants";
import { DEFAULT_STORE_PROFILE } from "../../config/defaults";
import { useSettings } from "../../hooks/useSettings";
import { useToast } from "../../hooks/useToast";
import { useUnsavedGuard } from "../../hooks/useUnsavedGuard";
import { saveStoreProfile } from "../../db/repositories/settingsRepo";
import { getCustomPreview, THEMES, useTheme } from "../../theme/ThemeContext";
import type { StoreProfile } from "../../types";

interface GeneralSettingsProps {
  dbReady: boolean;
}

export default function GeneralSettings({ dbReady }: GeneralSettingsProps) {
  const { settings, reload } = useSettings();
  const { toast } = useToast();
  const { theme, setTheme, customColors, setCustomColor, resetCustomColors } = useTheme();

  const [form, setForm] = useState<StoreProfile>(DEFAULT_STORE_PROFILE);
  const [initial, setInitial] = useState<StoreProfile | null>(null);
  const [saving, setSaving] = useState(false);
  const [dbFolderPath] = useState<string | null>(() => localStorage.getItem(STORAGE_KEYS.dbFolderPath));

  // Bill sync, so a bill the cloud refuses is visible rather than silently
  // retried forever. Before PART E a refused bill was retried 1,440 times a
  // day, permanently, and nothing on screen ever said so.
  const [billSync, setBillSync] = useState<BillSyncStatus>(() => getBillSyncStatus());
  const [retrying, setRetrying] = useState(false);
  useEffect(() => subscribeBillSync(setBillSync), []);

  useEffect(() => {
    if (settings) {
      setForm(settings.store);
      setInitial(settings.store);
    }
  }, [settings]);

  const dirty = initial !== null && JSON.stringify(form) !== JSON.stringify(initial);

  const doSave = async (): Promise<boolean> => {
    if (!dbReady) return false;
    try {
      setSaving(true);
      await saveStoreProfile(form);
      setInitial(form);
      await reload();
      toast("Settings saved successfully!", "success");
      return true;
    } catch (error) {
      console.error("Failed to save settings:", error);
      toast(`Error saving settings: ${error}`, "danger");
      return false;
    } finally {
      setSaving(false);
    }
  };

  useUnsavedGuard(dirty, doSave);

  const set = (key: keyof StoreProfile, value: string) => setForm((prev) => ({ ...prev, [key]: value }));

  const handleChangeDbFolder = async () => {
    try {
      const selected = await open({ directory: true, multiple: false, title: "Select New Database Folder" });
      if (selected && typeof selected === "string") {
        localStorage.setItem(STORAGE_KEYS.dbFolderPath, selected);
        toast("Database folder updated. Please restart the app to apply changes.", "warning");
      }
    } catch (error) {
      console.error("Failed to open dialog:", error);
    }
  };

  return (
    <div className="page settings-page">
      <div className="page-head">
        <h1>General Settings</h1>
        <p>Manage your establishment details and system configuration</p>
      </div>

      {/* Appearance */}
      <div className="section">
        <div className="section-head">
          <Palette size={14} /> Appearance
        </div>
        <div className="theme-grid">
          {THEMES.map((t) => {
            const preview = t.id === "custom" ? getCustomPreview(customColors) : t.preview;
            return (
              <div key={t.id} className={`theme-option ${theme === t.id ? "active" : ""}`} onClick={() => setTheme(t.id)}>
                {theme === t.id && (
                  <div className="theme-option-check">
                    <Check size={13} />
                  </div>
                )}
                <div className="theme-option-preview">
                  <div style={{ backgroundColor: preview.bg }} />
                  <div style={{ backgroundColor: preview.panelBg }} />
                  <div style={{ backgroundColor: preview.headerBg }} />
                  <div style={{ backgroundColor: preview.accent }} />
                </div>
                <div>
                  <span className="theme-option-name">{t.label}</span>
                  <span className="theme-option-desc">{t.description}</span>
                </div>
              </div>
            );
          })}
        </div>

        {theme === "custom" && (
          <div className="custom-palette">
            <div className="custom-palette-head">
              <span>Pick a base tint &amp; accent — everything else is auto-balanced for readability</span>
              <button type="button" className="btn btn--ghost btn--sm" onClick={resetCustomColors}>
                <RotateCcw size={14} /> Reset
              </button>
            </div>
            <div className="custom-palette-row">
              <div className="custom-mode">
                <span className="custom-field-label">Mode</span>
                <div className="custom-mode-seg">
                  <button type="button" className={`custom-mode-btn ${customColors.mode === "dark" ? "active" : ""}`} onClick={() => setCustomColor("mode", "dark")}>
                    <Moon size={14} /> Dark
                  </button>
                  <button type="button" className={`custom-mode-btn ${customColors.mode === "light" ? "active" : ""}`} onClick={() => setCustomColor("mode", "light")}>
                    <Sun size={14} /> Light
                  </button>
                </div>
              </div>
              <label className="custom-swatch">
                <input type="color" value={customColors.base} onChange={(e) => setCustomColor("base", e.target.value)} />
                <div className="custom-swatch-info">
                  <span className="custom-swatch-name">Base Tint</span>
                  <span className="custom-swatch-hint">Backgrounds, panels &amp; borders</span>
                  <span className="custom-swatch-value">{customColors.base.toUpperCase()}</span>
                </div>
              </label>
              <label className="custom-swatch">
                <input type="color" value={customColors.accent} onChange={(e) => setCustomColor("accent", e.target.value)} />
                <div className="custom-swatch-info">
                  <span className="custom-swatch-name">Accent</span>
                  <span className="custom-swatch-hint">Buttons, highlights &amp; active states</span>
                  <span className="custom-swatch-value">{customColors.accent.toUpperCase()}</span>
                </div>
              </label>
            </div>
          </div>
        )}
      </div>

      {/* Store information */}
      <div className="section">
        <div className="section-head">
          <Store size={14} /> Store Information
        </div>
        <div className="form-grid">
          <div className="field">
            <label>
              <Building2 size={13} /> Hotel Name
            </label>
            <input type="text" className="input" value={form.hotelName} onChange={(e) => set("hotelName", e.target.value)} placeholder="e.g. Grand Restaurant" />
          </div>
          <div className="field">
            <label>
              <Phone size={13} /> Phone Number
            </label>
            <input
              type="text"
              className="input"
              value={form.phoneNumber}
              onChange={(e) => set("phoneNumber", e.target.value.replace(/\D/g, "").slice(0, 10))}
              placeholder="Contact number"
            />
          </div>
          <div className="field">
            <label>
              <Hash size={13} /> GST Number
            </label>
            <input type="text" className="input" value={form.gstNumber} onChange={(e) => set("gstNumber", e.target.value)} placeholder="GSTIN" />
          </div>
          <div className="field">
            <label>
              <FileText size={13} /> FSSAI Number
            </label>
            <input type="text" className="input" value={form.fssaiNumber} onChange={(e) => set("fssaiNumber", e.target.value)} placeholder="FSSAI License Number" />
          </div>
          <div className="field span-full">
            <label>
              <MapPin size={13} /> Address
            </label>
            <textarea className="textarea" rows={2} value={form.address} onChange={(e) => set("address", e.target.value)} placeholder="Complete address" />
          </div>
        </div>
      </div>

      {/* UPI payment */}
      <div className="section">
        <div className="section-head">
          <QrCode size={14} /> UPI Payment
        </div>
        <div className="form-grid cols-3">
          <div className="field">
            <label>UPI ID</label>
            <input type="text" className="input" value={form.upiId} onChange={(e) => set("upiId", e.target.value)} placeholder="e.g. merchant@upi" />
          </div>
          <div className="field">
            <label>Merchant / Restaurant Name</label>
            <input type="text" className="input" value={form.merchantName} onChange={(e) => set("merchantName", e.target.value)} placeholder="Shown to customer" />
          </div>
          <div className="field">
            <label>Payment Reference (Optional)</label>
            <input type="text" className="input" value={form.paymentReference} onChange={(e) => set("paymentReference", e.target.value)} placeholder="e.g. Bill Payment" />
          </div>
        </div>
      </div>

      {/* System configuration */}
      <div className="section">
        <div className="section-head">
          <HardDrive size={14} /> System Configuration
        </div>
        <div className="field">
          <label>
            <FolderOpen size={13} /> Database Location
          </label>
          <div style={{ display: "flex", gap: "var(--space-3)", alignItems: "center", flexWrap: "wrap" }}>
            <span className="readonly-box" style={{ flex: 1, minWidth: 220 }}>{dbFolderPath || "Not Selected"}</span>
            <button className="btn btn--ghost" onClick={handleChangeDbFolder}>
              <FolderOpen size={16} /> Change Folder
            </button>
          </div>
        </div>
      </div>

      {/* Cloud backup of bills */}
      <div className="section">
        <div className="section-head">
          <UploadCloud size={14} /> Cloud Backup of Bills
        </div>
        <p className="field-hint" style={{ marginTop: 0, maxWidth: "68ch" }}>
          Every finalised bill is copied to Magic Bill's cloud so your reports work
          from your phone. Billing at this counter never waits for it, and a bill is
          only ticked off once the cloud has confirmed it.
        </p>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "var(--space-3)",
            flexWrap: "wrap",
            marginTop: "var(--space-3)",
          }}
        >
          <span className="readonly-box">
            {billSync.pending === 0 && billSync.stuck === 0
              ? "All bills are backed up."
              : `${billSync.pending} waiting to upload`}
          </span>
          {billSync.stuck > 0 && (
            <>
              <span className="readonly-box" style={{ color: "var(--warning)" }}>
                {billSync.stuck} bill{billSync.stuck === 1 ? "" : "s"} set aside
                {billSync.lastError ? ` (${billSync.lastError})` : ""}
              </span>
              <button
                className="btn btn--ghost"
                disabled={retrying}
                onClick={async () => {
                  try {
                    setRetrying(true);
                    await retryStuckBills();
                    toast("Trying those bills again…", "success");
                  } finally {
                    setRetrying(false);
                  }
                }}
              >
                <RefreshCw size={16} /> {retrying ? "Retrying…" : "Try these again"}
              </button>
            </>
          )}
        </div>
        {billSync.stuck > 0 && (
          <p className="field-hint" style={{ maxWidth: "68ch" }}>
            Magic Bill stopped retrying these on its own so a single bad bill cannot
            repeat forever in the background. Nothing has been lost — they are still
            on this computer and still on your reports here.
          </p>
        )}
      </div>

      <div className="save-bar">
        {dirty && <span className="dirty-hint">Unsaved changes</span>}
        <button className="btn btn--primary" onClick={doSave} disabled={saving}>
          <Save size={16} />
          {saving ? "Saving…" : "Save Settings"}
        </button>
      </div>
    </div>
  );
}
