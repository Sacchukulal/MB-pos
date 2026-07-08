import { useEffect, useState } from "react";
import { Eye, QrCode, Save, Scissors, Search, Settings2, Type } from "lucide-react";
import { FONT_FAMILIES, FONT_SIZES, GST_PERCENTAGES } from "../../config/constants";
import { DEFAULT_BILL_DESIGN, DEFAULT_KOT_DESIGN } from "../../config/defaults";
import { useSettings } from "../../hooks/useSettings";
import { useToast } from "../../hooks/useToast";
import { useUnsavedGuard } from "../../hooks/useUnsavedGuard";
import { saveBillDesign, saveKotDesign } from "../../db/repositories/settingsRepo";
import { BillPreview, KotPreview } from "./previews";
import type { BillDesign, GstType, KotDesign, QrMode, SearchMatchMode, SectionStyle } from "../../types";

interface BillSettingsProps {
  dbReady: boolean;
}

/* Compact -/+ stepper that walks the FONT_SIZES scale. */
function SizeStepper({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const idx = Math.max(0, FONT_SIZES.indexOf(value));
  return (
    <div className="stepper">
      <button type="button" onClick={() => onChange(FONT_SIZES[Math.max(0, idx - 1)])} disabled={idx <= 0} aria-label="Decrease size">
        −
      </button>
      <span>{value.replace("px", "")}px</span>
      <button
        type="button"
        onClick={() => onChange(FONT_SIZES[Math.min(FONT_SIZES.length - 1, idx + 1)])}
        disabled={idx >= FONT_SIZES.length - 1}
        aria-label="Increase size"
      >
        +
      </button>
    </div>
  );
}

/* One row: section label + size stepper + bold toggle. */
function StyleRow({ label, style, onChange }: { label: string; style: SectionStyle; onChange: (s: SectionStyle) => void }) {
  return (
    <div className="style-row">
      <span className="style-row-label">{label}</span>
      <SizeStepper value={style.size} onChange={(size) => onChange({ ...style, size })} />
      <label className="check check--inline">
        <input type="checkbox" checked={style.bold} onChange={(e) => onChange({ ...style, bold: e.target.checked })} /> Bold
      </label>
    </div>
  );
}

export default function BillSettings({ dbReady }: BillSettingsProps) {
  const { settings, reload } = useSettings();
  const { toast } = useToast();

  const [bill, setBill] = useState<BillDesign>(DEFAULT_BILL_DESIGN);
  const [kot, setKot] = useState<KotDesign>(DEFAULT_KOT_DESIGN);
  const [initial, setInitial] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (settings) {
      setBill(settings.bill);
      setKot(settings.kot);
      setInitial(JSON.stringify({ b: settings.bill, k: settings.kot }));
    }
  }, [settings]);

  const dirty = initial !== null && JSON.stringify({ b: bill, k: kot }) !== initial;

  const doSave = async (): Promise<boolean> => {
    if (!dbReady) return false;
    try {
      setSaving(true);
      await saveBillDesign(bill);
      await saveKotDesign(kot);
      setInitial(JSON.stringify({ b: bill, k: kot }));
      await reload();
      toast("Bill settings saved successfully!", "success");
      return true;
    } catch (error) {
      console.error("Failed to save bill settings:", error);
      toast(`Error saving settings: ${error}`, "danger");
      return false;
    } finally {
      setSaving(false);
    }
  };

  useUnsavedGuard(dirty, doSave);

  const handleLogoUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onloadend = () => setBill((prev) => ({ ...prev, logo: { ...prev.logo, base64: reader.result as string } }));
    reader.readAsDataURL(file);
  };

  const setSep = (key: keyof BillDesign["separators"], value: boolean) =>
    setBill((prev) => ({ ...prev, separators: { ...prev.separators, [key]: value } }));
  const setKotSep = (key: keyof KotDesign["separators"], value: boolean) =>
    setKot((prev) => ({ ...prev, separators: { ...prev.separators, [key]: value } }));
  const setQrMode = (qrMode: QrMode) => setBill((prev) => ({ ...prev, qrMode }));

  const paperSize = settings?.printer.paperSize ?? "3inch";
  const store = settings?.store ?? {
    hotelName: "", address: "", phoneNumber: "", gstNumber: "", fssaiNumber: "", upiId: "", merchantName: "", paymentReference: "",
  };

  return (
    <div className="split-page">
      {/* Configuration column */}
      <div className="split-main settings-page" style={{ maxWidth: "none", margin: 0 }}>
        <div className="page-head">
          <h1>Bill Settings</h1>
          <p>Configure your receipt design and formatting</p>
        </div>

        {/* Receipt format */}
        <div className="section">
          <div className="section-head">
            <Settings2 size={14} /> Receipt Format
          </div>
          <div className="form-grid">
            <div className="field">
              <label>Paper Size</label>
              <span className="readonly-box">
                {paperSize === "2inch" ? "58mm (2 inch)" : paperSize === "4inch" ? "100mm (4 inch)" : "80mm (3 inch)"}
              </span>
              <p className="field-hint">Set in Printer Settings — preview and print always match.</p>
            </div>
            <div className="field">
              <label>Row Height (Item Spacing)</label>
              <select className="select" value={bill.rowHeight} onChange={(e) => setBill({ ...bill, rowHeight: e.target.value })}>
                <option value="2px 0">Compact</option>
                <option value="4px 0">Standard</option>
                <option value="8px 0">Relaxed</option>
              </select>
            </div>
          </div>
        </div>

        {/* Item search */}
        <div className="section">
          <div className="section-head">
            <Search size={14} /> Billing Item Search
          </div>
          <div className="field">
            <label>Search Match Mode</label>
            <select
              className="select"
              value={bill.searchMatchMode}
              onChange={(e) => setBill({ ...bill, searchMatchMode: e.target.value as SearchMatchMode })}
            >
              <option value="starts">Starts With — show items whose name begins with typed letters</option>
              <option value="contains">Contains — show items that include the typed letters anywhere</option>
            </select>
            <p className="field-hint">Controls how the search dropdown on the Billing page matches menu items as you type.</p>
          </div>
        </div>

        {/* Global font */}
        <div className="section">
          <div className="section-head">
            <Type size={14} /> Font (Bill &amp; KOT)
          </div>
          <div className="field">
            <label>Font Family — applies to entire Bill and KOT</label>
            <select className="select" value={bill.fontFamily} onChange={(e) => setBill({ ...bill, fontFamily: e.target.value })}>
              {FONT_FAMILIES.map((f) => (
                <option key={f.value} value={f.value}>
                  {f.label}
                </option>
              ))}
            </select>
            <p className="field-hint">One font for the whole receipt and kitchen ticket. Use the per-section controls below for size and bold.</p>
          </div>
        </div>

        {/* Receipt section sizes */}
        <div className="section">
          <div className="section-head">
            <Type size={14} /> Receipt — Section Size &amp; Bold
          </div>
          <div className="style-rows">
            <StyleRow label="Hotel Name" style={bill.storeName} onChange={(s) => setBill({ ...bill, storeName: s })} />
            <StyleRow label="Address / Meta" style={bill.addressMeta} onChange={(s) => setBill({ ...bill, addressMeta: s })} />
            <StyleRow label="Table Items" style={bill.table} onChange={(s) => setBill({ ...bill, table: s })} />
            <StyleRow label="Totals" style={bill.totals} onChange={(s) => setBill({ ...bill, totals: s })} />
            <StyleRow label="Footer" style={bill.footer} onChange={(s) => setBill({ ...bill, footer: s })} />
          </div>
        </div>

        {/* Separators */}
        <div className="section">
          <div className="section-head">
            <Scissors size={14} /> Receipt Line Separators
          </div>
          <div className="form-grid cols-3">
            <label className="check"><input type="checkbox" checked={bill.separators.header} onChange={(e) => setSep("header", e.target.checked)} /> Below Store Header</label>
            <label className="check"><input type="checkbox" checked={bill.separators.meta} onChange={(e) => setSep("meta", e.target.checked)} /> Below Meta (Date/Time)</label>
            <label className="check"><input type="checkbox" checked={bill.separators.token} onChange={(e) => setSep("token", e.target.checked)} /> Below Token Number</label>
            <label className="check"><input type="checkbox" checked={bill.separators.tableHeader} onChange={(e) => setSep("tableHeader", e.target.checked)} /> Below Column Names</label>
            <label className="check"><input type="checkbox" checked={bill.separators.tableBody} onChange={(e) => setSep("tableBody", e.target.checked)} /> Below Item List</label>
            <label className="check"><input type="checkbox" checked={bill.separators.subtotals} onChange={(e) => setSep("subtotals", e.target.checked)} /> Below Subtotals &amp; GST</label>
            <label className="check"><input type="checkbox" checked={bill.separators.grandTotal} onChange={(e) => setSep("grandTotal", e.target.checked)} /> Below Grand Total</label>
          </div>
        </div>

        {/* Visibility */}
        <div className="section">
          <div className="section-head">
            <Eye size={14} /> Receipt Content Visibility
          </div>
          <div className="form-grid cols-3">
            <label className="check"><input type="checkbox" checked={bill.showToken} onChange={(e) => setBill({ ...bill, showToken: e.target.checked })} /> Show Token Number</label>
            <label className="check"><input type="checkbox" checked={bill.showGstin} onChange={(e) => setBill({ ...bill, showGstin: e.target.checked })} /> Show GSTIN Header</label>
            <label className="check"><input type="checkbox" checked={bill.showFssai} onChange={(e) => setBill({ ...bill, showFssai: e.target.checked })} /> Show FSSAI Header</label>
            <label className="check"><input type="checkbox" checked={bill.showAddress} onChange={(e) => setBill({ ...bill, showAddress: e.target.checked })} /> Show Address</label>
            <label className="check"><input type="checkbox" checked={bill.showPhone} onChange={(e) => setBill({ ...bill, showPhone: e.target.checked })} /> Show Phone</label>
            <label className="check"><input type="checkbox" checked={bill.showCashier} onChange={(e) => setBill({ ...bill, showCashier: e.target.checked })} /> Show Cashier</label>
          </div>
        </div>

        {/* UPI QR */}
        <div className="section">
          <div className="section-head">
            <QrCode size={14} /> UPI QR Printing
          </div>
          <div className="form-grid cols-3">
            <label className="check">
              <input type="radio" name="qr_mode" checked={bill.qrMode === "dynamic"} onChange={() => setQrMode("dynamic")} />
              Dynamic UPI QR
              <span className="check-hint">Amount included</span>
            </label>
            <label className="check">
              <input type="radio" name="qr_mode" checked={bill.qrMode === "static"} onChange={() => setQrMode("static")} />
              Static UPI QR
              <span className="check-hint">Direct to UPI ID</span>
            </label>
            <label className="check">
              <input type="radio" name="qr_mode" checked={bill.qrMode === "none"} onChange={() => setQrMode("none")} />
              No QR Print
            </label>
          </div>
        </div>

        {/* GST */}
        <div className="section">
          <div className="section-head">GST Calculation</div>
          <label className="check" style={{ alignSelf: "flex-start" }}>
            <input
              type="checkbox"
              checked={bill.gst.enabled}
              onChange={(e) => setBill({ ...bill, gst: { ...bill.gst, enabled: e.target.checked } })}
            />
            Enable GST
          </label>
          {bill.gst.enabled && (
            <div className="form-grid cols-3">
              <div className="field">
                <label>GST Type</label>
                <select className="select" value={bill.gst.type} onChange={(e) => setBill({ ...bill, gst: { ...bill.gst, type: e.target.value as GstType } })}>
                  <option value="Exclusive">Exclusive (Added to total)</option>
                  <option value="Inclusive">Inclusive (Included in price)</option>
                </select>
              </div>
              <div className="field">
                <label>GST %</label>
                <select
                  className="select"
                  value={bill.gst.percentage}
                  onChange={(e) => setBill({ ...bill, gst: { ...bill.gst, percentage: Number(e.target.value) } })}
                >
                  {GST_PERCENTAGES.map((p) => (
                    <option key={p} value={p}>
                      {p}%
                    </option>
                  ))}
                </select>
              </div>
            </div>
          )}
        </div>

        {/* Logo */}
        <div className="section">
          <div className="section-head">Logo</div>
          <div className="form-grid cols-3">
            <div className="field">
              <label>Position</label>
              <select
                className="select"
                value={bill.logo.position}
                onChange={(e) => setBill({ ...bill, logo: { ...bill.logo, position: e.target.value as "none" | "top" } })}
              >
                <option value="none">None</option>
                <option value="top">Top</option>
              </select>
            </div>
            {bill.logo.position !== "none" && (
              <>
                <div className="field">
                  <label>Logo Image</label>
                  <div style={{ display: "flex", gap: "var(--space-2)", alignItems: "center" }}>
                    <input type="file" accept="image/*" onChange={handleLogoUpload} style={{ fontSize: "var(--text-sm)", flex: 1, minWidth: 0 }} />
                    {bill.logo.base64 && (
                      <button type="button" className="btn btn--danger btn--sm" onClick={() => setBill({ ...bill, logo: { ...bill.logo, base64: "" } })}>
                        Remove
                      </button>
                    )}
                  </div>
                </div>
                <div className="field">
                  <label>Size — {bill.logo.sizePct || 50}%</label>
                  <input
                    type="range"
                    min={10}
                    max={100}
                    step={5}
                    value={bill.logo.sizePct || 50}
                    onChange={(e) => setBill({ ...bill, logo: { ...bill.logo, sizePct: Number(e.target.value) } })}
                    style={{ accentColor: "var(--accent)", width: "100%" }}
                  />
                </div>
              </>
            )}
          </div>
        </div>

        {/* Footer message */}
        <div className="section">
          <div className="section-head">Footer Message</div>
          <textarea
            className="textarea"
            rows={2}
            value={bill.footerMessage}
            onChange={(e) => setBill({ ...bill, footerMessage: e.target.value })}
            placeholder="e.g. Thank you! Visit again."
          />
        </div>

        {/* KOT visibility */}
        <div className="section">
          <div className="section-head">
            <Eye size={14} /> KOT — Content Visibility
          </div>
          <div className="form-grid cols-3">
            <label className="check"><input type="checkbox" checked={kot.showTitle} onChange={(e) => setKot({ ...kot, showTitle: e.target.checked })} /> Show "KOT" Title</label>
            <label className="check"><input type="checkbox" checked={kot.showToken} onChange={(e) => setKot({ ...kot, showToken: e.target.checked })} /> Show Token Number</label>
            <label className="check"><input type="checkbox" checked={kot.showBillNo} onChange={(e) => setKot({ ...kot, showBillNo: e.target.checked })} /> Show Bill No</label>
            <label className="check"><input type="checkbox" checked={kot.showOrderType} onChange={(e) => setKot({ ...kot, showOrderType: e.target.checked })} /> Show Order Type</label>
            <label className="check"><input type="checkbox" checked={kot.showTable} onChange={(e) => setKot({ ...kot, showTable: e.target.checked })} /> Show Table</label>
            <label className="check"><input type="checkbox" checked={kot.showDate} onChange={(e) => setKot({ ...kot, showDate: e.target.checked })} /> Show Date / Time</label>
            <label className="check span-full">
              <input type="checkbox" checked={kot.metaTwoColumn} onChange={(e) => setKot({ ...kot, metaTwoColumn: e.target.checked })} />
              Pack details in 2 columns (saves paper)
              <span className="check-hint">Bill No / Order / Table / Date side-by-side</span>
            </label>
          </div>
        </div>

        {/* KOT sizes */}
        <div className="section">
          <div className="section-head">
            <Settings2 size={14} /> KOT — Section Size &amp; Bold
          </div>
          <div className="style-rows">
            <StyleRow label="KOT Title" style={kot.title} onChange={(s) => setKot({ ...kot, title: s })} />
            <StyleRow label="Details (Bill / Order / Table / Date)" style={kot.meta} onChange={(s) => setKot({ ...kot, meta: s })} />
            <StyleRow label="Items" style={kot.items} onChange={(s) => setKot({ ...kot, items: s })} />
          </div>
          <div className="field" style={{ maxWidth: 360 }}>
            <label>Row Height (Item Spacing)</label>
            <select className="select" value={kot.rowHeight} onChange={(e) => setKot({ ...kot, rowHeight: e.target.value })}>
              <option value="2px 0">Compact</option>
              <option value="4px 0">Standard</option>
              <option value="8px 0">Relaxed</option>
            </select>
          </div>
        </div>

        {/* KOT separators */}
        <div className="section">
          <div className="section-head">
            <Scissors size={14} /> KOT — Line Separators
          </div>
          <div className="form-grid cols-3">
            <label className="check"><input type="checkbox" checked={kot.separators.token} onChange={(e) => setKotSep("token", e.target.checked)} /> Below Token Number</label>
            <label className="check"><input type="checkbox" checked={kot.separators.header} onChange={(e) => setKotSep("header", e.target.checked)} /> Below KOT Title</label>
            <label className="check"><input type="checkbox" checked={kot.separators.meta} onChange={(e) => setKotSep("meta", e.target.checked)} /> Below Details</label>
            <label className="check"><input type="checkbox" checked={kot.separators.tableHeader} onChange={(e) => setKotSep("tableHeader", e.target.checked)} /> Below Column Names</label>
            <label className="check"><input type="checkbox" checked={kot.separators.tableBody} onChange={(e) => setKotSep("tableBody", e.target.checked)} /> Below Item List</label>
          </div>
        </div>

        <div className="save-bar">
          {dirty && <span className="dirty-hint">Unsaved changes</span>}
          <button className="btn btn--primary" onClick={doSave} disabled={saving}>
            <Save size={16} />
            {saving ? "Saving…" : "Save Settings"}
          </button>
        </div>
      </div>

      {/* Preview column */}
      <div className="preview-col">
        <div className="preview-caption">
          <Eye size={16} /> Bill Preview
        </div>
        <BillPreview bill={bill} store={store} paperSize={paperSize} />
        <div className="preview-caption">
          <Eye size={16} /> KOT Preview
        </div>
        <KotPreview kot={kot} fontFamily={bill.fontFamily} paperSize={paperSize} />
        <p className="preview-note">Note: This is a digital preview. Actual print may vary depending on your printer hardware.</p>
      </div>
    </div>
  );
}
