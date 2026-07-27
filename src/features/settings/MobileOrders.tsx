import { useEffect, useState } from "react";
import { Ban, Check, Edit2, Eye, LayoutGrid, Plus, RefreshCw, Save, Smartphone, Trash2, X } from "lucide-react";
import {
  getOrderSyncState,
  setMobileOrderingEnabled,
  setSoundOnNewOrder,
} from "../../db/repositories/orderSyncRepo";
import {
  blockInstallRemote,
  forceCatalogPush,
  getOrderBridgeStatus,
  setMobileOrderingEnabledLive,
  setNewOrderSound,
  subscribeOrderBridge,
  type OrderBridgeStatus,
} from "../../services/orders/orderBridge";
import {
  addTable,
  bulkAddTables,
  deleteTable,
  labelInUse,
  listTables,
  openTableNumbers,
  setTableActive,
  tableLabelExists,
  updateTable,
} from "../../db/repositories/tablesRepo";
import { describeBridge, isLastSyncStale } from "../../services/orders/statusCopy";
import { useToast } from "../../hooks/useToast";
import { useUnsavedGuard } from "../../hooks/useUnsavedGuard";
import ConfirmDialog from "../../components/ui/ConfirmDialog";
import type { RestaurantTable } from "../../types";

interface MobileOrdersProps {
  dbReady: boolean;
}

interface SwitchForm {
  enabled: boolean;
  sound: boolean;
}

const TABLE_GRID = "110px 1fr 90px 90px 96px";

export default function MobileOrders({ dbReady }: MobileOrdersProps) {
  const { toast } = useToast();

  // -- Mobile ordering switches (save-bar + unsaved guard, like other settings)
  const [form, setForm] = useState<SwitchForm>({ enabled: false, sound: true });
  const [initial, setInitial] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [bridge, setBridge] = useState<OrderBridgeStatus>(() => getOrderBridgeStatus());
  const [resyncing, setResyncing] = useState(false);

  useEffect(() => subscribeOrderBridge(setBridge), []);

  // A dead heartbeat emits nothing, so "Last sync" would silently freeze with
  // no way to tell. This tick re-evaluates the staleness line on its own.
  const [nowTick, setNowTick] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNowTick(Date.now()), 10_000);
    return () => clearInterval(t);
  }, []);
  const bridgeCopy = describeBridge(bridge);
  const syncStale = isLastSyncStale(bridge.lastSyncAt, nowTick);

  // -- Table master (immediate CRUD, like Menu Management)
  const [tables, setTables] = useState<RestaurantTable[]>([]);
  const [bulkSection, setBulkSection] = useState("");
  const [bulkFrom, setBulkFrom] = useState("1");
  const [bulkTo, setBulkTo] = useState("10");
  const [addLabel, setAddLabel] = useState("");
  const [addSection, setAddSection] = useState("");
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editLabel, setEditLabel] = useState("");
  const [editSection, setEditSection] = useState("");
  const [editSort, setEditSort] = useState("0");
  const [deleteTarget, setDeleteTarget] = useState<RestaurantTable | null>(null);

  const refreshTables = async () => {
    setTables(await listTables());
  };

  useEffect(() => {
    if (!dbReady) return;
    (async () => {
      try {
        const state = await getOrderSyncState();
        setForm({ enabled: state.mobileOrderingEnabled, sound: state.soundOnNewOrder });
        setInitial(JSON.stringify({ enabled: state.mobileOrderingEnabled, sound: state.soundOnNewOrder }));
        await refreshTables();
      } catch (error) {
        console.error("Failed to load mobile ordering settings:", error);
      }
    })();
  }, [dbReady]);

  const dirty = initial !== null && JSON.stringify(form) !== initial;

  const doSave = async (): Promise<boolean> => {
    if (!dbReady) return false;
    try {
      setSaving(true);
      await setMobileOrderingEnabled(form.enabled);
      await setSoundOnNewOrder(form.sound);
      setInitial(JSON.stringify(form));
      // The bridge reacts immediately (connect/disconnect + hello to the cloud).
      setNewOrderSound(form.sound);
      void setMobileOrderingEnabledLive(form.enabled);
      toast("Mobile ordering settings saved successfully!", "success");
      return true;
    } catch (error) {
      console.error("Failed to save mobile ordering settings:", error);
      toast(`Error saving settings: ${error}`, "danger");
      return false;
    } finally {
      setSaving(false);
    }
  };

  useUnsavedGuard(dirty, doSave);

  // ---- table master actions (each saves immediately and toasts on failure)

  const handleBulkAdd = async () => {
    const from = parseInt(bulkFrom, 10);
    const to = parseInt(bulkTo, 10);
    const section = bulkSection.trim();
    if (isNaN(from) || isNaN(to) || from < 1 || to < from) {
      toast("Enter a valid table range (e.g. 1 to 20).", "warning");
      return;
    }
    if (to - from >= 200) {
      toast("That range is too large — add at most 200 tables at once.", "warning");
      return;
    }
    try {
      const created = await bulkAddTables(section, from, to);
      await refreshTables();
      toast(
        created > 0
          ? `Added ${created} table${created === 1 ? "" : "s"}${section ? ` in ${section}` : ""}.`
          : "Those tables already exist.",
        created > 0 ? "success" : "warning"
      );
    } catch (error) {
      console.error("Bulk add failed:", error);
      toast(`Error adding tables: ${error}`, "danger");
    }
  };

  const handleAdd = async () => {
    const label = addLabel.trim();
    const section = addSection.trim();
    if (!label) {
      toast("Enter a table name.", "warning");
      return;
    }
    try {
      if (await tableLabelExists(section, label)) {
        toast(`Table "${label}" already exists${section ? ` in ${section}` : ""}.`, "warning");
        return;
      }
      const maxSort = tables
        .filter((t) => t.section === section)
        .reduce((m, t) => Math.max(m, t.sort_order), 0);
      await addTable(section, label, maxSort + 1);
      setAddLabel("");
      await refreshTables();
    } catch (error) {
      console.error("Add table failed:", error);
      toast(`Error adding table: ${error}`, "danger");
    }
  };

  const startEdit = (t: RestaurantTable) => {
    setEditingId(t.id);
    setEditLabel(t.label);
    setEditSection(t.section);
    setEditSort(String(t.sort_order));
  };

  const saveEdit = async (id: number) => {
    const label = editLabel.trim();
    const section = editSection.trim();
    const sortOrder = parseInt(editSort, 10) || 0;
    if (!label) {
      toast("Table name cannot be empty.", "warning");
      return;
    }
    try {
      if (await tableLabelExists(section, label, id)) {
        toast(`Table "${label}" already exists${section ? ` in ${section}` : ""}.`, "warning");
        return;
      }
      await updateTable(id, section, label, sortOrder);
      setEditingId(null);
      await refreshTables();
    } catch (error) {
      console.error("Update table failed:", error);
      toast(`Error updating table: ${error}`, "danger");
    }
  };

  const handleToggleActive = async (t: RestaurantTable) => {
    try {
      await setTableActive(t.id, t.is_active !== 1);
      await refreshTables();
    } catch (error) {
      console.error("Toggle table failed:", error);
      toast(`Error updating table: ${error}`, "danger");
    }
  };

  const requestDelete = async (t: RestaurantTable) => {
    try {
      const open = await openTableNumbers();
      if (labelInUse(t.label, open)) {
        toast(`Table ${t.label} has an open order — settle or move it first.`, "danger");
        return;
      }
      setDeleteTarget(t);
    } catch (error) {
      console.error("Delete check failed:", error);
      toast(`Error checking table: ${error}`, "danger");
    }
  };

  const confirmDelete = async () => {
    if (!deleteTarget) return;
    try {
      await deleteTable(deleteTarget.id);
      setDeleteTarget(null);
      await refreshTables();
    } catch (error) {
      console.error("Delete table failed:", error);
      toast(`Error deleting table: ${error}`, "danger");
    }
  };

  // Preview groups: active tables per section, in the order the phone shows.
  const sections = [...new Set(tables.map((t) => t.section))];
  const previewSections = sections
    .map((s) => ({
      name: s,
      tables: tables.filter((t) => t.section === s && t.is_active === 1),
    }))
    .filter((s) => s.tables.length > 0);

  return (
    <div className="page settings-page">
      <div className="page-head">
        <h1>Tables &amp; Mobile Ordering</h1>
        <p>Define your table layout and control ordering from staff phones</p>
      </div>

      {/* Mobile ordering */}
      <div className="section">
        <div className="section-head">
          <Smartphone size={14} /> Mobile ordering
        </div>
        <div className="form-grid cols-2">
          <label className="check" style={{ alignSelf: "flex-start" }}>
            <input
              type="checkbox"
              checked={form.enabled}
              onChange={(e) => setForm({ ...form, enabled: e.target.checked })}
            />
            Enable mobile ordering
            <span className="check-hint">
              Staff with the "Take orders" permission can open tables and send orders from the
              Magic Bill app. Both the counter and the phone need internet.
            </span>
          </label>
          <label className="check" style={{ alignSelf: "flex-start" }}>
            <input
              type="checkbox"
              checked={form.sound}
              onChange={(e) => setForm({ ...form, sound: e.target.checked })}
            />
            Play sound on new order
            <span className="check-hint">A short chime when an order arrives from a phone.</span>
          </label>
        </div>

        {/* Live status + connected phones (only meaningful once enabled). */}
        <div style={{ marginTop: "var(--space-4)", display: "flex", alignItems: "center", gap: "var(--space-3)", flexWrap: "wrap" }}>
          <div className={`mo-pill ${bridgeCopy.tone}`}>
            <span className="mo-pill-dot" />
            {bridgeCopy.label}
          </div>
          {bridge.lastSyncAt && (
            <span
              style={{
                fontSize: "var(--text-xs)",
                color: syncStale ? "var(--warning)" : "var(--text-tertiary)",
              }}
            >
              Last sync: {new Date(bridge.lastSyncAt).toLocaleTimeString()}
              {syncStale ? " — not updating" : ""}
            </span>
          )}
          <button
            className="btn btn--ghost btn--sm"
            disabled={!bridge.featureEnabled || resyncing}
            onClick={async () => {
              try {
                setResyncing(true);
                await forceCatalogPush();
                toast("Menu, tables and customers resynced to phones.", "success");
              } catch (error) {
                console.error("Resync failed:", error);
                toast("Resync failed — check the internet connection.", "danger");
              } finally {
                setResyncing(false);
              }
            }}
          >
            <RefreshCw size={14} /> {resyncing ? "Resyncing…" : "Resync menu & tables now"}
          </button>
        </div>

        {/* Never make the owner guess what a status word means. */}
        {bridgeCopy.detail !== "" && (
          <p className="field-hint" style={{ marginTop: "var(--space-2)", maxWidth: "62ch" }}>
            {bridgeCopy.detail}
          </p>
        )}

        {bridge.installs.length > 0 && (
          <div style={{ marginTop: "var(--space-4)" }}>
            <p className="field-hint" style={{ marginTop: 0 }}>
              Phones that have used mobile ordering (plan limit: {bridge.maxMobileDevices}{" "}
              device{bridge.maxMobileDevices === 1 ? "" : "s"}). Block a lost phone to cut it off
              instantly.
            </p>
            <div className="data-list-head" style={{ gridTemplateColumns: "1fr 1fr 140px 96px" }}>
              <div>Device</div>
              <div>Signed in as</div>
              <div>Last seen</div>
              <div style={{ textAlign: "right" }}>Actions</div>
            </div>
            {bridge.installs.map((ins) => (
              <div key={ins.installId} className="data-row" style={{ gridTemplateColumns: "1fr 1fr 140px 96px" }}>
                <div style={{ color: ins.blocked ? "var(--text-tertiary)" : undefined }}>
                  {ins.label || "Phone"}
                  {ins.blocked && <span className="mo-unavailable">(blocked)</span>}
                </div>
                <div style={{ color: "var(--text-secondary)" }}>{ins.actorName || "—"}</div>
                <div style={{ color: "var(--text-tertiary)", fontSize: "var(--text-xs)" }}>
                  {ins.lastSeen ? new Date(ins.lastSeen).toLocaleString() : "—"}
                </div>
                <div className="data-row-actions">
                  <button
                    className={`row-action-btn ${ins.blocked ? "" : "danger"}`}
                    title={ins.blocked ? "Unblock this phone" : "Block this phone"}
                    onClick={async () => {
                      try {
                        await blockInstallRemote(ins.installId, !ins.blocked);
                        toast(ins.blocked ? "Phone unblocked." : "Phone blocked.", "success");
                      } catch (error) {
                        console.error("Block install failed:", error);
                        toast("Could not update the phone — check the internet connection.", "danger");
                      }
                    }}
                  >
                    <Ban size={16} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Table layout */}
      <div className="section">
        <div className="section-head">
          <LayoutGrid size={14} /> Table layout
        </div>

        <div className="form-grid cols-3">
          <div className="field">
            <label>Add many tables</label>
            <div style={{ display: "flex", gap: "var(--space-2)", alignItems: "center" }}>
              <input
                className="input"
                type="number"
                min={1}
                value={bulkFrom}
                onChange={(e) => setBulkFrom(e.target.value)}
                style={{ width: "72px" }}
              />
              <span style={{ color: "var(--text-tertiary)" }}>to</span>
              <input
                className="input"
                type="number"
                min={1}
                value={bulkTo}
                onChange={(e) => setBulkTo(e.target.value)}
                style={{ width: "72px" }}
              />
            </div>
            <p className="field-hint">Numbered tables, e.g. 1 to 20.</p>
          </div>
          <div className="field">
            <label>In section</label>
            <input
              className="input"
              placeholder="e.g. AC, Garden (optional)"
              value={bulkSection}
              onChange={(e) => setBulkSection(e.target.value)}
            />
            <p className="field-hint">Sections group tables on the phone.</p>
          </div>
          <div className="field">
            <label>&nbsp;</label>
            <button className="btn btn--primary" onClick={handleBulkAdd} disabled={!dbReady}>
              <Plus size={16} /> Add tables
            </button>
          </div>
        </div>

        <div className="form-grid cols-3" style={{ marginTop: "var(--space-2)" }}>
          <div className="field">
            <label>Add one table</label>
            <input
              className="input"
              placeholder='Name, e.g. "G3" or "Counter"'
              value={addLabel}
              onChange={(e) => setAddLabel(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleAdd()}
            />
          </div>
          <div className="field">
            <label>In section</label>
            <input
              className="input"
              placeholder="optional"
              value={addSection}
              onChange={(e) => setAddSection(e.target.value)}
            />
          </div>
          <div className="field">
            <label>&nbsp;</label>
            <button className="btn btn--ghost" onClick={handleAdd} disabled={!dbReady}>
              <Plus size={16} /> Add table
            </button>
          </div>
        </div>

        {tables.length === 0 ? (
          <p style={{ color: "var(--text-tertiary)", marginTop: "var(--space-4)" }}>
            No tables yet. Add your tables above to show a tappable grid on staff phones.
          </p>
        ) : (
          <div style={{ marginTop: "var(--space-4)" }}>
            <div className="data-list-head" style={{ gridTemplateColumns: TABLE_GRID }}>
              <div>Table</div>
              <div>Section</div>
              <div style={{ textAlign: "right" }}>Order</div>
              <div style={{ textAlign: "center" }}>Active</div>
              <div style={{ textAlign: "right" }}>Actions</div>
            </div>
            {tables.map((t) =>
              editingId === t.id ? (
                <div
                  key={t.id}
                  className="data-row"
                  style={{ display: "flex", gap: "var(--space-2)", alignItems: "center" }}
                >
                  <input
                    className="input"
                    value={editLabel}
                    onChange={(e) => setEditLabel(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && saveEdit(t.id)}
                    style={{ width: "110px" }}
                    autoFocus
                  />
                  <input
                    className="input"
                    value={editSection}
                    onChange={(e) => setEditSection(e.target.value)}
                    placeholder="Section"
                    style={{ flex: 1 }}
                  />
                  <input
                    className="input"
                    type="number"
                    value={editSort}
                    onChange={(e) => setEditSort(e.target.value)}
                    style={{ width: "80px" }}
                    title="Sort order"
                  />
                  <div className="data-row-actions">
                    <button className="row-action-btn" onClick={() => saveEdit(t.id)} title="Save">
                      <Check size={17} />
                    </button>
                    <button className="row-action-btn" onClick={() => setEditingId(null)} title="Cancel">
                      <X size={17} />
                    </button>
                  </div>
                </div>
              ) : (
                <div key={t.id} className="data-row" style={{ gridTemplateColumns: TABLE_GRID }}>
                  <div style={{ fontWeight: "var(--font-medium)" }}>{t.label}</div>
                  <div style={{ color: "var(--text-secondary)" }}>{t.section || "—"}</div>
                  <div style={{ textAlign: "right", color: "var(--text-tertiary)" }}>{t.sort_order}</div>
                  <div style={{ textAlign: "center" }}>
                    <input
                      type="checkbox"
                      checked={t.is_active === 1}
                      onChange={() => handleToggleActive(t)}
                      title={t.is_active === 1 ? "Shown on phones" : "Hidden on phones"}
                      style={{ accentColor: "var(--accent)" }}
                    />
                  </div>
                  <div className="data-row-actions">
                    <button className="row-action-btn" onClick={() => startEdit(t)} title="Edit table">
                      <Edit2 size={17} />
                    </button>
                    <button
                      className="row-action-btn danger"
                      onClick={() => requestDelete(t)}
                      title="Delete table"
                    >
                      <Trash2 size={17} />
                    </button>
                  </div>
                </div>
              )
            )}
          </div>
        )}
      </div>

      {/* Phone preview */}
      {previewSections.length > 0 && (
        <div className="section">
          <div className="section-head">
            <Eye size={14} /> Phone preview
          </div>
          <p className="field-hint" style={{ marginTop: 0 }}>
            How the table grid appears in the staff app. Inactive tables are hidden.
          </p>
          {previewSections.map((s) => (
            <div key={s.name || "(none)"} style={{ marginTop: "var(--space-3)" }}>
              {s.name !== "" && (
                <div
                  style={{
                    fontSize: "var(--text-xs)",
                    fontWeight: "var(--font-semibold)",
                    color: "var(--text-tertiary)",
                    textTransform: "uppercase",
                    letterSpacing: "0.05em",
                    marginBottom: "var(--space-2)",
                  }}
                >
                  {s.name}
                </div>
              )}
              <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--space-2)" }}>
                {s.tables.map((t) => (
                  <div
                    key={t.id}
                    style={{
                      minWidth: "56px",
                      padding: "var(--space-3)",
                      textAlign: "center",
                      background: "var(--bg-tertiary)",
                      border: "var(--border-thin) solid var(--border-subtle)",
                      borderRadius: "var(--radius-md)",
                      fontWeight: "var(--font-semibold)",
                      color: "var(--text-primary)",
                    }}
                  >
                    {t.label}
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="save-bar">
        {dirty && <span className="dirty-hint">Unsaved changes</span>}
        <button className="btn btn--primary" onClick={doSave} disabled={saving || !dirty}>
          <Save size={16} />
          {saving ? "Saving…" : "Save Mobile Ordering Settings"}
        </button>
      </div>

      {deleteTarget && (
        <ConfirmDialog
          title="Delete table?"
          message={
            <>
              Delete table <strong>{deleteTarget.label}</strong>
              {deleteTarget.section ? ` (${deleteTarget.section})` : ""}? Phones will no longer
              show it. Past orders are not affected.
            </>
          }
          confirmLabel="Delete"
          danger
          onConfirm={confirmDelete}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </div>
  );
}
