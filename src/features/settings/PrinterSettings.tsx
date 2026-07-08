import { useEffect, useState } from "react";
import { Hash, Printer, Save } from "lucide-react";
import { DEFAULT_PRINTER_CONFIG } from "../../config/defaults";
import { useSettings } from "../../hooks/useSettings";
import { useToast } from "../../hooks/useToast";
import { useUnsavedGuard } from "../../hooks/useUnsavedGuard";
import { savePrinterConfig } from "../../db/repositories/settingsRepo";
import * as menuRepo from "../../db/repositories/menuRepo";
import { listPrinters, printTestSlip } from "../../services/printing/printService";
import type { Category, KotStyle, PaperSize, PrinterConfig, PrinterMode, TokenPrintSize } from "../../types";

interface PrinterSettingsProps {
  dbReady: boolean;
}

export default function PrinterSettings({ dbReady }: PrinterSettingsProps) {
  const { settings, reload } = useSettings();
  const { toast } = useToast();

  const [form, setForm] = useState<PrinterConfig>(DEFAULT_PRINTER_CONFIG);
  const [categoryPrinters, setCategoryPrinters] = useState<Record<number, string>>({});
  const [initial, setInitial] = useState<string | null>(null);
  const [printers, setPrinters] = useState<string[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    if (settings) {
      setForm(settings.printer);
      setCategoryPrinters(settings.categoryPrinters);
      setInitial(JSON.stringify({ p: settings.printer, m: settings.categoryPrinters }));
    }
  }, [settings]);

  useEffect(() => {
    if (!dbReady) return;
    menuRepo.listCategories().then(setCategories).catch(console.error);
    listPrinters().then(setPrinters).catch((err) => console.error("Failed to fetch printers:", err));
  }, [dbReady]);

  const dirty = initial !== null && JSON.stringify({ p: form, m: categoryPrinters }) !== initial;

  const doSave = async (): Promise<boolean> => {
    if (!dbReady) return false;
    try {
      setSaving(true);
      await savePrinterConfig(form, categoryPrinters);
      setInitial(JSON.stringify({ p: form, m: categoryPrinters }));
      await reload();
      toast("Printer settings saved successfully!", "success");
      return true;
    } catch (error) {
      console.error("Failed to save printer settings:", error);
      toast(`Error saving settings: ${error}`, "danger");
      return false;
    } finally {
      setSaving(false);
    }
  };

  useUnsavedGuard(dirty, doSave);

  const handleTestPrint = async () => {
    if (!form.defaultPrinter) {
      toast("Select a default printer first (and Save).", "warning");
      return;
    }
    setTesting(true);
    try {
      await printTestSlip(form.defaultPrinter);
      toast(`Test sent to "${form.defaultPrinter}" successfully.`, "success");
    } catch (err) {
      toast(`Test print FAILED: ${err instanceof Error ? err.message : String(err)}`, "danger");
    } finally {
      setTesting(false);
    }
  };

  const refreshPrinters = async () => {
    try {
      const list = await listPrinters();
      setPrinters(list);
      toast(`Found ${list.length} printer(s).`);
    } catch (err) {
      toast(`Could not read printers: ${err instanceof Error ? err.message : String(err)}`, "danger");
    }
  };

  return (
    <div className="page settings-page">
      <div className="page-head">
        <h1>Printer Settings</h1>
        <p>Manage printing preferences and connections</p>
      </div>

      {/* Printer configuration */}
      <div className="section">
        <div className="section-head">
          <Printer size={14} /> Printer Configuration
        </div>

        <div className="field">
          <label>Printer Mode</label>
          <div className="form-grid">
            {(["Single Printer", "Multiple Printers"] as PrinterMode[]).map((mode) => (
              <label key={mode} className="check">
                <input type="radio" name="printer_mode" checked={form.printerMode === mode} onChange={() => setForm({ ...form, printerMode: mode })} />
                {mode === "Multiple Printers" ? "Multiple Printers (Category-wise)" : mode}
              </label>
            ))}
          </div>
        </div>

        {form.printerMode === "Single Printer" && (
          <div className="field">
            <label>KOT Printing Style</label>
            <div className="form-grid">
              {([
                { value: "Single KOT", label: "Single KOT (All items in one ticket)" },
                { value: "Category-wise KOTs", label: "Category-wise KOTs (Separate per category)" },
              ] as { value: KotStyle; label: string }[]).map((opt) => (
                <label key={opt.value} className="check">
                  <input type="radio" name="kot_style" checked={form.kotStyle === opt.value} onChange={() => setForm({ ...form, kotStyle: opt.value })} />
                  {opt.label}
                </label>
              ))}
            </div>
          </div>
        )}

        <div className="form-grid cols-3">
          <div className="field">
            <label>Default Printer (Bills &amp; Unassigned KOTs)</label>
            <select className="select" value={form.defaultPrinter} onChange={(e) => setForm({ ...form, defaultPrinter: e.target.value })}>
              <option value="">Select a printer</option>
              {printers.map((printer) => (
                <option key={printer} value={printer}>
                  {printer}
                </option>
              ))}
            </select>
            <div style={{ display: "flex", gap: "var(--space-2)", marginTop: "var(--space-1)" }}>
              <button type="button" className="btn btn--ghost btn--sm" onClick={handleTestPrint} disabled={testing || !form.defaultPrinter}>
                <Printer size={14} /> {testing ? "Testing…" : "Test Print"}
              </button>
              <button type="button" className="btn btn--ghost btn--sm" onClick={refreshPrinters}>
                Refresh List
              </button>
            </div>
          </div>
          <div className="field">
            <label>Paper Size</label>
            <select className="select" value={form.paperSize} onChange={(e) => setForm({ ...form, paperSize: e.target.value as PaperSize })}>
              <option value="2inch">58mm (2 inch)</option>
              <option value="3inch">80mm (3 inch)</option>
              <option value="4inch">100mm (4 inch)</option>
            </select>
            <p className="field-hint">Used for printing and the Bill Settings preview.</p>
          </div>
          <div className="field">
            <label>Print Options</label>
            <label className="check">
              <input type="checkbox" checked={form.printBold} onChange={(e) => setForm({ ...form, printBold: e.target.checked })} />
              Bold &amp; Dark (ESC/POS)
            </label>
          </div>
        </div>

        {form.printerMode === "Multiple Printers" && (
          <div className="field">
            <label>Category Printer Mapping</label>
            {categories.length === 0 ? (
              <p className="field-hint">No categories found. Please add categories first.</p>
            ) : (
              <div className="form-grid cols-3">
                {categories.map((cat) => (
                  <div key={cat.id} className="field">
                    <label style={{ textTransform: "none" }}>{cat.name}</label>
                    <select
                      className="select"
                      value={categoryPrinters[cat.id] || ""}
                      onChange={(e) => setCategoryPrinters({ ...categoryPrinters, [cat.id]: e.target.value })}
                    >
                      <option value="">Use Default</option>
                      {printers.map((printer) => (
                        <option key={printer} value={printer}>
                          {printer}
                        </option>
                      ))}
                    </select>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        <div className="field">
          <label>Printing Flow &amp; Confirmation</label>
          <div className="form-grid cols-3">
            <label className="check">
              <input type="checkbox" checked={form.kotConfirmation} onChange={(e) => setForm({ ...form, kotConfirmation: e.target.checked })} />
              Confirm before printing KOT
              <span className="check-hint">Saves paper</span>
            </label>
            <label className="check">
              <input type="checkbox" checked={form.billConfirmation} onChange={(e) => setForm({ ...form, billConfirmation: e.target.checked })} />
              Confirm before printing Bill
              <span className="check-hint">Prevents accidents</span>
            </label>
            <label className="check">
              <input type="checkbox" checked={form.disableKot} onChange={(e) => setForm({ ...form, disableKot: e.target.checked })} />
              Disable KOT entirely
              <span className="check-hint">Direct to Processing</span>
            </label>
          </div>
        </div>
      </div>

      {/* Token numbering */}
      <div className="section">
        <div className="section-head">
          <Hash size={14} /> Token Numbering
        </div>
        <label className="check" style={{ alignSelf: "flex-start" }}>
          <input
            type="checkbox"
            checked={form.token.resetDaily}
            onChange={(e) => setForm({ ...form, token: { ...form.token, resetDaily: e.target.checked } })}
          />
          Reset Token Number Daily
        </label>
        <div className="form-grid cols-3">
          <div className="field">
            <label>Daily Starting Number</label>
            <input
              type="number"
              className="input"
              value={form.token.startingNumber}
              onChange={(e) => setForm({ ...form, token: { ...form.token, startingNumber: parseInt(e.target.value) || 0 } })}
            />
          </div>
          <div className="field">
            <label>Current Token Number</label>
            <input
              type="number"
              className="input"
              value={form.token.currentNumber}
              onChange={(e) => setForm({ ...form, token: { ...form.token, currentNumber: parseInt(e.target.value) || 0 } })}
            />
          </div>
          <div className="field">
            <label>Token Print Size (Bill &amp; KOT)</label>
            <select
              className="select"
              value={form.token.printSize}
              onChange={(e) => setForm({ ...form, token: { ...form.token, printSize: e.target.value as TokenPrintSize } })}
            >
              <option value="Normal">Normal</option>
              <option value="Large">Large</option>
              <option value="Extra Large">Extra Large (Huge)</option>
            </select>
          </div>
        </div>
      </div>

      {/* Bill numbering */}
      <div className="section">
        <div className="section-head">
          <Hash size={14} /> Bill Numbering
        </div>
        <label className="check" style={{ alignSelf: "flex-start" }}>
          <input
            type="checkbox"
            checked={form.bill.resetDaily}
            onChange={(e) => setForm({ ...form, bill: { ...form.bill, resetDaily: e.target.checked } })}
          />
          Reset Bill Number Daily
        </label>
        <div className="form-grid cols-3">
          <div className="field">
            <label>Bill Prefix</label>
            <input
              type="text"
              className="input"
              value={form.bill.prefix}
              onChange={(e) => setForm({ ...form, bill: { ...form.bill, prefix: e.target.value } })}
              placeholder="e.g. BIR/ (optional)"
            />
          </div>
          <div className="field">
            <label>Daily Starting Number</label>
            <input
              type="number"
              className="input"
              value={form.bill.startingNumber}
              onChange={(e) => setForm({ ...form, bill: { ...form.bill, startingNumber: parseInt(e.target.value) || 0 } })}
            />
          </div>
          <div className="field">
            <label>Current Bill Number</label>
            <input
              type="number"
              className="input"
              value={form.bill.currentNumber}
              onChange={(e) => setForm({ ...form, bill: { ...form.bill, currentNumber: parseInt(e.target.value) || 0 } })}
            />
          </div>
        </div>
        <p className="field-hint">Counters reset to the starting numbers on the first order of a new day.</p>
      </div>

      <div className="save-bar">
        {dirty && <span className="dirty-hint">Unsaved changes</span>}
        <button className="btn btn--primary" onClick={doSave} disabled={saving}>
          <Save size={16} />
          {saving ? "Saving…" : "Save Printer Settings"}
        </button>
      </div>
    </div>
  );
}
