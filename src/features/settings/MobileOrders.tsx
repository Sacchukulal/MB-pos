import { useEffect, useState } from "react";
import { Ban, Check, EyeOff, LayoutGrid, Plus, RefreshCw, Save, Smartphone, Trash2 } from "lucide-react";
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
  deleteTables,
  labelInUse,
  listTables,
  openTableNumbers,
  setTableActive,
  tableLabelExists,
  updateTable,
} from "../../db/repositories/tablesRepo";
import { composeTableName } from "../billing/tableUtils";
import { describeBridge, describeUsage, isLastSyncStale } from "../../services/orders/statusCopy";
import { useToast } from "../../hooks/useToast";
import { useUnsavedGuard } from "../../hooks/useUnsavedGuard";
import ConfirmDialog from "../../components/ui/ConfirmDialog";
import Modal from "../../components/ui/Modal";
import type { RestaurantTable } from "../../types";

interface MobileOrdersProps {
  dbReady: boolean;
}

interface SwitchForm {
  enabled: boolean;
  sound: boolean;
}

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
  // The grid tile IS the row: clicking one opens this editor.
  const [editing, setEditing] = useState<RestaurantTable | null>(null);
  const [editLabel, setEditLabel] = useState("");
  const [editSection, setEditSection] = useState("");
  const [editSort, setEditSort] = useState("0");
  const [editActive, setEditActive] = useState(true);
  const [deleteTarget, setDeleteTarget] = useState<RestaurantTable | null>(null);

  // Tick-and-delete: ids ticked in the grid, plus the bulk confirm's payload.
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [bulkDelete, setBulkDelete] = useState<{ doomed: RestaurantTable[]; busy: string[] } | null>(
    null
  );

  const refreshTables = async () => {
    const rows = await listTables();
    setTables(rows);
    // Deleted tables must not linger in the selection and re-arm the toolbar.
    setSelected((prev) => {
      const live = new Set(rows.map((r) => r.id));
      const next = new Set([...prev].filter((id) => live.has(id)));
      return next.size === prev.size ? prev : next;
    });
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

  /**
   * Two rows can be different in the master and still be the SAME table to the
   * kitchen: a table literally named "AC 1" with no section prints exactly
   * what label "1" in section "AC" prints, and the counter would merge their
   * orders (the second one silently becoming "AC 1B"). Reject that up front.
   */
  const composedClash = (
    section: string,
    label: string,
    excludeId?: number
  ): RestaurantTable | undefined => {
    const target = composeTableName(section, label).toUpperCase();
    if (!target) return undefined;
    return tables.find(
      (t) => t.id !== excludeId && composeTableName(t.section, t.label).toUpperCase() === target
    );
  };

  const clashToast = (section: string, label: string, clash: RestaurantTable) => {
    const where = clash.section ? `in ${clash.section}` : "with no section";
    toast(
      `That table would print as "${composeTableName(section, label)}", exactly like the ` +
        `existing table "${clash.label}" ${where}. Give it a different name or section.`,
      "warning"
    );
  };

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
    // Reject the whole range rather than creating half of it: a partial add is
    // harder for the owner to reason about than a single clear message.
    for (let n = from; n <= to; n++) {
      const clash = composedClash(section, String(n));
      if (clash && !(clash.section === section && clash.label === String(n))) {
        clashToast(section, String(n), clash);
        return;
      }
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
      const clash = composedClash(section, label);
      if (clash) {
        clashToast(section, label, clash);
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
    setEditing(t);
    setEditLabel(t.label);
    setEditSection(t.section);
    setEditSort(String(t.sort_order));
    setEditActive(t.is_active === 1);
  };

  const saveEdit = async () => {
    if (!editing) return;
    const id = editing.id;
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
      const clash = composedClash(section, label, id);
      if (clash) {
        clashToast(section, label, clash);
        return;
      }
      await updateTable(id, section, label, sortOrder);
      if (editActive !== (editing.is_active === 1)) {
        await setTableActive(id, editActive);
      }
      setEditing(null);
      await refreshTables();
    } catch (error) {
      console.error("Update table failed:", error);
      toast(`Error updating table: ${error}`, "danger");
    }
  };

  const requestDelete = async (t: RestaurantTable) => {
    try {
      const open = await openTableNumbers();
      // Orders are stored under the composed name, so that is what "in use"
      // means — checking the bare label would flag the wrong section's table.
      const name = composeTableName(t.section, t.label);
      if (labelInUse(name, open)) {
        toast(`Table ${name} has an open order — settle or move it first.`, "danger");
        return;
      }
      // The editor and the confirm are never on screen together.
      setEditing(null);
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

  // ---- ticking tables for a bulk delete

  const toggleSelected = (id: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  };

  /** Header tick: all on unless every one is already ticked, then all off. */
  const toggleMany = (group: RestaurantTable[]) => {
    const allOn = group.every((t) => selected.has(t.id));
    setSelected((prev) => {
      const next = new Set(prev);
      group.forEach((t) => (allOn ? next.delete(t.id) : next.add(t.id)));
      return next;
    });
  };

  /**
   * A table holding an open order is never deleted — but one busy table must
   * not block the other nineteen either, so the busy ones are set aside and
   * named in the confirm rather than aborting the whole batch.
   */
  const requestBulkDelete = async () => {
    const chosen = tables.filter((t) => selected.has(t.id));
    if (chosen.length === 0) return;
    try {
      const open = await openTableNumbers();
      const busy: string[] = [];
      const doomed: RestaurantTable[] = [];
      for (const t of chosen) {
        const name = composeTableName(t.section, t.label);
        if (labelInUse(name, open)) busy.push(name);
        else doomed.push(t);
      }
      if (doomed.length === 0) {
        toast(
          `${busy.length === 1 ? "That table has" : "All the selected tables have"} open orders — ` +
            `settle or move them first.`,
          "danger"
        );
        return;
      }
      setEditing(null);
      setBulkDelete({ doomed, busy });
    } catch (error) {
      console.error("Bulk delete check failed:", error);
      toast(`Error checking tables: ${error}`, "danger");
    }
  };

  const confirmBulkDelete = async () => {
    if (!bulkDelete) return;
    const n = bulkDelete.doomed.length;
    try {
      await deleteTables(bulkDelete.doomed.map((t) => t.id));
      setBulkDelete(null);
      await refreshTables();
      toast(`Deleted ${n} table${n === 1 ? "" : "s"}.`, "success");
    } catch (error) {
      console.error("Bulk delete failed:", error);
      toast(`Error deleting tables: ${error}`, "danger");
    }
  };

  /**
   * The grid is both the preview and the editor, so unlike the phone it must
   * also show hidden tables — otherwise a table switched off could never be
   * switched back on. They are rendered dimmed so the preview still reads
   * like the phone at a glance.
   */
  const sections = [...new Set(tables.map((t) => t.section))];
  const tableGroups = sections.map((s) => ({
    name: s,
    tables: tables.filter((t) => t.section === s),
  }));
  const onlyUnsectioned = tableGroups.length === 1 && tableGroups[0].name === "";
  const hiddenCount = tables.filter((t) => t.is_active !== 1).length;

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

        {/* Never make the owner guess what a status word means.

            The block below keeps a FIXED height while mobile ordering is on,
            so an explanation appearing or clearing can never shove the rest
            of the page down and back — that reflow was the "blinking" the
            owner reported. The 10-second debounce in the bridge stops the
            state oscillating; this stops the layout moving even if it does. */}
        {form.enabled && (
          <div style={{ minHeight: "68px", marginTop: "var(--space-2)" }}>
            {bridgeCopy.detail !== "" && (
              <p className="field-hint" style={{ marginTop: 0, maxWidth: "62ch" }}>
                {bridgeCopy.detail}
              </p>
            )}
            <p
              className="field-hint"
              style={{ marginTop: "var(--space-1)", color: "var(--text-tertiary)" }}
            >
              {describeUsage(bridge)}
            </p>
          </div>
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
          <div style={{ marginTop: "var(--space-5)" }}>
            <p className="field-hint" style={{ marginTop: 0 }}>
              This is exactly how the grid appears in the staff app, and the name on each tile is
              what the kitchen slip and the bill will say. Click a table to rename, move, reorder,
              hide or delete it — or tick the boxes to delete several at once.
              {hiddenCount > 0 && ` ${hiddenCount} hidden table${hiddenCount === 1 ? " is" : "s are"} shown dimmed — the phone does not show ${hiddenCount === 1 ? "it" : "them"}.`}
            </p>

            {/* Bulk bar — only once something is ticked, so the grid stays calm. */}
            {selected.size > 0 && (
              <div className="mo-bulk-bar">
                <strong>
                  {selected.size} table{selected.size === 1 ? "" : "s"} selected
                </strong>
                <button className="btn btn--ghost btn--sm" onClick={() => toggleMany(tables)}>
                  {selected.size === tables.length ? "Unselect all" : `Select all ${tables.length}`}
                </button>
                <button className="btn btn--ghost btn--sm" onClick={() => setSelected(new Set())}>
                  Clear
                </button>
                <button className="btn btn--danger btn--sm" onClick={requestBulkDelete}>
                  <Trash2 size={14} /> Delete selected
                </button>
              </div>
            )}

            {tableGroups.map((g) => {
              const allTicked = g.tables.every((t) => selected.has(t.id));
              return (
                <div key={g.name || "(none)"} className="mo-tile-group">
                  <div className="mo-tile-group-head">
                    {!(onlyUnsectioned && g.name === "") && <span>{g.name || "No section"}</span>}
                    <button
                      type="button"
                      className="mo-select-all"
                      onClick={() => toggleMany(g.tables)}
                    >
                      {allTicked ? "Unselect" : "Select"} all {g.tables.length}
                    </button>
                  </div>
                  <div className="mo-tiles">
                    {g.tables.map((t) => {
                      const ticked = selected.has(t.id);
                      return (
                        <div
                          key={t.id}
                          className={`mo-tile-wrap ${ticked ? "is-selected" : ""}`}
                        >
                          <button
                            type="button"
                            className={`mo-tile ${t.is_active === 1 ? "" : "mo-tile--off"}`}
                            onClick={() => startEdit(t)}
                            title={
                              t.is_active === 1
                                ? `Prints as "Table: ${composeTableName(t.section, t.label)}" — click to edit`
                                : `Hidden on phones — click to edit`
                            }
                          >
                            {/* Mirrors the phone tile: section caption over the number. */}
                            {t.section !== "" && <div className="mo-tile-section">{t.section}</div>}
                            <div className="mo-tile-label">{t.label}</div>
                            {t.is_active !== 1 && <EyeOff className="mo-tile-badge" size={12} />}
                          </button>
                          {/* A sibling, not a child: nesting it inside the tile
                              button would be invalid and unreachable by keyboard. */}
                          <button
                            type="button"
                            className="mo-tile-check"
                            role="checkbox"
                            aria-checked={ticked}
                            aria-label={`Select ${composeTableName(t.section, t.label)}`}
                            title="Select for bulk delete"
                            onClick={() => toggleSelected(t.id)}
                          >
                            {ticked && <Check size={12} strokeWidth={3} />}
                          </button>
                        </div>
                      );
                    })}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Table editor — opened from a tile in the grid above. */}
      {editing && (
        <Modal onClose={() => setEditing(null)} width="460px">
          <div className="ui-modal-title">Edit table</div>
          <div className="form-grid cols-2">
            <div className="field">
              <label>Table name</label>
              <input
                className="input"
                value={editLabel}
                onChange={(e) => setEditLabel(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && saveEdit()}
                autoFocus
              />
            </div>
            <div className="field">
              <label>Section</label>
              <input
                className="input"
                placeholder="optional"
                value={editSection}
                onChange={(e) => setEditSection(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && saveEdit()}
              />
            </div>
          </div>
          <div className="form-grid cols-2" style={{ marginTop: "var(--space-3)" }}>
            <div className="field">
              <label>Order</label>
              <input
                className="input"
                type="number"
                value={editSort}
                onChange={(e) => setEditSort(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && saveEdit()}
              />
              <p className="field-hint">Position within the section.</p>
            </div>
            <div className="field">
              <label>Prints as</label>
              <div style={{ fontWeight: "var(--font-semibold)", paddingTop: "10px" }}>
                {composeTableName(editSection.trim(), editLabel.trim()) || "—"}
              </div>
              <p className="field-hint">What the kitchen slip and the bill say.</p>
            </div>
          </div>
          <label className="check" style={{ marginTop: "var(--space-3)" }}>
            <input
              type="checkbox"
              checked={editActive}
              onChange={(e) => setEditActive(e.target.checked)}
            />
            Show on phones
            <span className="check-hint">
              A hidden table stays here in the grid, dimmed, but the staff app does not show it.
            </span>
          </label>
          <div className="ui-modal-actions" style={{ marginTop: "var(--space-5)", justifyContent: "space-between" }}>
            <button className="btn btn--ghost danger" onClick={() => requestDelete(editing)}>
              <Trash2 size={16} /> Delete
            </button>
            <div style={{ display: "flex", gap: "var(--space-2)" }}>
              <button className="btn btn--ghost" onClick={() => setEditing(null)}>
                Cancel
              </button>
              <button className="btn btn--primary" onClick={saveEdit}>
                <Save size={16} /> Save table
              </button>
            </div>
          </div>
        </Modal>
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

      {bulkDelete && (
        <ConfirmDialog
          title={`Delete ${bulkDelete.doomed.length} table${bulkDelete.doomed.length === 1 ? "" : "s"}?`}
          message={
            <>
              Deleting{" "}
              <strong>
                {bulkDelete.doomed
                  .slice(0, 8)
                  .map((t) => composeTableName(t.section, t.label))
                  .join(", ")}
                {bulkDelete.doomed.length > 8 && ` and ${bulkDelete.doomed.length - 8} more`}
              </strong>
              . Phones will no longer show them. Past orders are not affected.
              {bulkDelete.busy.length > 0 && (
                <>
                  <br />
                  <br />
                  Keeping <strong>{bulkDelete.busy.join(", ")}</strong> — {bulkDelete.busy.length === 1 ? "it has" : "they have"}{" "}
                  an open order right now.
                </>
              )}
            </>
          }
          confirmLabel={`Delete ${bulkDelete.doomed.length}`}
          danger
          onConfirm={confirmBulkDelete}
          onCancel={() => setBulkDelete(null)}
        />
      )}
    </div>
  );
}
