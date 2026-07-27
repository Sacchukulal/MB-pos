import { getVersion } from "@tauri-apps/api/app";
import { SUPABASE_ANON_KEY, SUPABASE_FUNCTIONS_URL } from "../../config/supabase";
import { isDbOpen } from "../../db/client";
import * as ordersRepo from "../../db/repositories/ordersRepo";
import * as menuRepo from "../../db/repositories/menuRepo";
import {
  getOrderSyncState,
  setCatalogHash,
  setLastReconcileAt,
  setRoomId,
} from "../../db/repositories/orderSyncRepo";
import { pruneAppliedEvents } from "../../db/repositories/appliedEventsRepo";
import { loadSettings } from "../../db/repositories/settingsRepo";
import { getStoredLicenseKey } from "../license/licenseService";
import { getDeviceInfo } from "../license/device";
import { connectRealtime, disconnectRealtime } from "../realtime/client";
import { buildCatalogPayload } from "./catalogSync";
import { applyOrderEvent, type WireEvent } from "./eventApplier";
import { registerCatalogPushHandler, registerOrdersPushHandler } from "./signals";
import { finalizedRowToWire, processingRowToWire, type WireOrder } from "./wire";

/**
 * The mobile-orders bridge. Owns the whole cloud lifecycle: hello heartbeat,
 * realtime doorbell + presence, catalog push, live-order publishing, and
 * applying phone intents through eventApplier — one at a time, in order.
 *
 * Everything is fire-and-forget: a network failure must NEVER block billing,
 * throw into the UI, or lose a local write. Started from App.tsx next to
 * startBillSync() so it runs regardless of which screen is open.
 */

export interface InstallInfo {
  installId: string;
  label: string;
  actorKind: string;
  actorName: string;
  lastSeen: string;
  blocked: boolean;
}

export interface OrderBridgeStatus {
  /** The owner's master switch (order_sync_state.mobile_ordering_enabled). */
  featureEnabled: boolean;
  channel: "connected" | "degraded" | "off";
  phones: number;
  installs: InstallInfo[];
  maxMobileDevices: number;
  lastSyncAt: string | null;
  /** Events older than 2h held back from auto-printing (B8). */
  staleCount: number;
}

export interface NewOrderAlert {
  /** False for pure print-failure notices on existing orders. */
  isNewOrder: boolean;
  tableNumber: string;
  orderType: string;
  waiterName: string;
  total: number;
  /** Set when the order saved but the counter printer failed. */
  printError?: string;
}

const HEARTBEAT_MS = 30_000;
const RECONCILE_MS = 5 * 60_000;
const DEGRADED_POLL_MS = 3_000;
const PUSH_DEBOUNCE_MS = 300;
const CATALOG_DEBOUNCE_MS = 2_000;
const STALE_EVENT_MS = 2 * 60 * 60_000;

/* ------------------------------- state ------------------------------- */

let started = false;
let live = false;
let enabled = false;
let roomId = "";
let serverOffsetMs = 0;
let soundOnNewOrder = true;
let appVersion = "";

const status: OrderBridgeStatus = {
  featureEnabled: false,
  channel: "off",
  phones: 0,
  installs: [],
  maxMobileDevices: 1,
  lastSyncAt: null,
  staleCount: 0,
};

const staleHeld = new Map<string, WireEvent>();
/** remote_uuid -> latest print failure note, carried on the pushed order. */
const printErrors = new Map<string, string>();
/** Final truths (cancelled orders) whose local rows are already gone. */
let pendingCloudOrders: WireOrder[] = [];

let heartbeatTimer: ReturnType<typeof setInterval> | null = null;
let reconcileTimer: ReturnType<typeof setInterval> | null = null;
let degradedTimer: ReturnType<typeof setInterval> | null = null;
let retryTimer: ReturnType<typeof setTimeout> | null = null;
let pushTimer: ReturnType<typeof setTimeout> | null = null;
let catalogTimer: ReturnType<typeof setTimeout> | null = null;

/* ---------------------------- subscriptions ---------------------------- */

type Unsubscribe = () => void;
const statusSubs = new Set<(s: OrderBridgeStatus) => void>();
const ordersChangedSubs = new Set<() => void>();
const alertSubs = new Set<(a: NewOrderAlert) => void>();

function emitStatus(): void {
  status.featureEnabled = enabled;
  status.staleCount = staleHeld.size;
  const snapshot = { ...status, installs: [...status.installs] };
  statusSubs.forEach((cb) => cb(snapshot));
}

function emitOrdersChanged(): void {
  ordersChangedSubs.forEach((cb) => cb());
}

export function getOrderBridgeStatus(): OrderBridgeStatus {
  status.featureEnabled = enabled;
  status.staleCount = staleHeld.size;
  return { ...status, installs: [...status.installs] };
}

export function subscribeOrderBridge(cb: (s: OrderBridgeStatus) => void): Unsubscribe {
  statusSubs.add(cb);
  return () => statusSubs.delete(cb);
}

/** Fires whenever the bridge changed local order data (billing refetches). */
export function subscribeOrdersChanged(cb: () => void): Unsubscribe {
  ordersChangedSubs.add(cb);
  return () => ordersChangedSubs.delete(cb);
}

export function subscribeNewOrderAlerts(cb: (a: NewOrderAlert) => void): Unsubscribe {
  alertSubs.add(cb);
  return () => alertSubs.delete(cb);
}

/* ------------------------------ cloud API ------------------------------ */

async function api<T = any>(action: string, args: Record<string, unknown> = {}): Promise<T> {
  const key = getStoredLicenseKey();
  if (!key) throw new Error("no-license");
  const device = await getDeviceInfo();
  const res = await fetch(`${SUPABASE_FUNCTIONS_URL}/pos-orders`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${SUPABASE_ANON_KEY}`,
      apikey: SUPABASE_ANON_KEY,
    },
    body: JSON.stringify({ key, deviceId: device.id, action, ...args }),
  });
  const data = await res.json();
  if (!data?.ok) throw new Error(String(data?.reason ?? `http-${res.status}`));
  status.lastSyncAt = new Date().toISOString();
  return data as T;
}

async function sendHello(): Promise<any> {
  const data = await api("hello", { appVersion, mobileOrderingEnabled: enabled });
  serverOffsetMs = Date.parse(data.serverTime) - Date.now();
  status.installs = Array.isArray(data.installs) ? data.installs : [];
  status.maxMobileDevices = Number(data.maxMobileDevices ?? 1);
  if (data.roomId && data.roomId !== roomId) {
    roomId = data.roomId;
    await setRoomId(roomId);
  }
  emitStatus();
  return data;
}

/* ------------------------------ push side ------------------------------ */

async function ensureRemoteUuids(): Promise<void> {
  const open = await ordersRepo.listProcessingOrders();
  for (const o of open) {
    if (!o.remote_uuid) {
      await ordersRepo.setOrderBridgeFields(o.id, {
        remoteUuid: crypto.randomUUID(),
        cloudDirty: true,
      });
    }
  }
}

let pushing = false;
let pushRerun = false;
let pushForceRerun = false;

/**
 * @param force republish EVERY open order, not just the dirty ones. Used by
 * reconcile so the cloud converges on the counter's truth even if a dirty
 * flag was lost (crash, or an edit racing an in-flight push).
 */
async function pushOrdersNow(force = false): Promise<void> {
  if (!live) return;
  if (pushing) {
    pushRerun = true;
    if (force) pushForceRerun = true;
    return;
  }
  pushing = true;
  try {
    await ensureRemoteUuids();
    const open = await ordersRepo.listProcessingOrders();
    const dirtyOpen = open.filter((o) => o.remote_uuid && (force || o.cloud_dirty === 1));
    const dirtyBilled = await ordersRepo.listDirtyFinalizedRemote();
    // Snapshot: anything queued while this request is in flight must survive
    // the cleanup below and go out on the next push.
    const extras = pendingCloudOrders.slice();

    const orders: WireOrder[] = [
      ...dirtyOpen.map((o) => processingRowToWire(o, printErrors.get(o.remote_uuid!) ?? "")),
      ...dirtyBilled.map(finalizedRowToWire),
      ...extras,
    ];
    if (orders.length === 0) return;

    await api("push_orders", { orders });
    await ordersRepo.clearProcessingCloudDirty(dirtyOpen);
    await ordersRepo.clearFinalizedCloudDirty(dirtyBilled.map((o) => o.id));
    pendingCloudOrders = pendingCloudOrders.filter((o) => !extras.includes(o));
    // Closed orders don't need their print note any more.
    [...dirtyBilled, ...extras].forEach((o) => {
      const uuid = "clientUuid" in o ? (o as WireOrder).clientUuid : (o as any).remote_uuid;
      if (uuid) printErrors.delete(uuid);
    });
    emitStatus();
  } catch (e) {
    console.info("[orders] push_orders failed (will retry):", e);
  } finally {
    pushing = false;
    if (pushRerun) {
      pushRerun = false;
      const rerunForce = pushForceRerun;
      pushForceRerun = false;
      void pushOrdersNow(rerunForce);
    }
  }
}

function requestPushDebounced(): void {
  if (!live) return;
  if (pushTimer) clearTimeout(pushTimer);
  pushTimer = setTimeout(() => {
    pushTimer = null;
    void pushOrdersNow();
  }, PUSH_DEBOUNCE_MS);
}

async function reconcileNow(): Promise<void> {
  if (!live) return;
  try {
    await ensureRemoteUuids();
    const open = await ordersRepo.listProcessingOrders();
    const openClientUuids = open.map((o) => o.remote_uuid).filter(Boolean);
    await api("reconcile_orders", { openClientUuids });
    await setLastReconcileAt(new Date().toISOString());
    // Republish the full open set: reconcile closes cloud rows the counter no
    // longer has, this repairs any open order whose cloud copy drifted.
    await pushOrdersNow(true);
  } catch (e) {
    console.info("[orders] reconcile failed (will retry):", e);
  }
}

async function catalogPushNow(force = false): Promise<void> {
  if (!live) return;
  try {
    const payload = await buildCatalogPayload();
    const state = await getOrderSyncState();
    if (!force && state.catalogHash === payload.catalogHash) return;
    await api("push_catalog", { ...payload });
    await setCatalogHash(payload.catalogHash);
    console.info("[orders] catalog pushed");
  } catch (e) {
    console.info("[orders] push_catalog failed (will retry):", e);
  }
}

function requestCatalogDebounced(): void {
  if (!live) return;
  if (catalogTimer) clearTimeout(catalogTimer);
  catalogTimer = setTimeout(() => {
    catalogTimer = null;
    void catalogPushNow();
  }, CATALOG_DEBOUNCE_MS);
}

export function forceCatalogPush(): Promise<void> {
  return catalogPushNow(true);
}

/* ------------------------------ pull side ------------------------------ */

function playNewOrderChime(): void {
  if (!soundOnNewOrder) return;
  try {
    const audio = new Audio("/sounds/new-order.wav");
    audio.volume = 0.6;
    void audio.play().catch(() => {});
  } catch {
    /* autoplay blocked or file missing — never break the flow */
  }
}

let draining = false;
let drainRerun = false;

async function applyEventList(events: WireEvent[], ignoreStaleness: boolean): Promise<void> {
  if (events.length === 0) return;
  const settings = await loadSettings();
  const categories = await menuRepo.listCategories();
  const state = await getOrderSyncState();
  soundOnNewOrder = state.soundOnNewOrder;

  const acks: { eventId: string; status: "applied" | "rejected"; reason?: string }[] = [];
  let anyApplied = false;
  const alerts: NewOrderAlert[] = [];

  for (const ev of events) {
    if (!ignoreStaleness) {
      const serverNow = Date.now() + serverOffsetMs;
      const age = serverNow - Date.parse(ev.createdAt);
      if (isFinite(age) && age > STALE_EVENT_MS) {
        // Do not spew yesterday's tickets on a morning start — hold them.
        if (!staleHeld.has(ev.eventId)) staleHeld.set(ev.eventId, ev);
        continue;
      }
    }
    try {
      const result = await applyOrderEvent(ev, { settings, categories });
      acks.push({ eventId: ev.eventId, status: result.status, reason: result.reason });
      if (result.status === "applied") {
        anyApplied = true;
        if (ev.orderClientUuid) {
          if (result.printError) printErrors.set(ev.orderClientUuid, result.printError);
          else printErrors.delete(ev.orderClientUuid);
        }
        if (result.cloudOrder) pendingCloudOrders.push(result.cloudOrder);
        if (result.alert) {
          alerts.push({ isNewOrder: true, ...result.alert, printError: result.printError });
        } else if (result.printError) {
          // Saved, but the counter printer failed — the cashier must know.
          alerts.push({
            isNewOrder: false,
            tableNumber: "",
            orderType: "",
            waiterName: ev.actorName,
            total: 0,
            printError: result.printError,
          });
        }
      }
    } catch (e) {
      console.error("[orders] apply failed:", ev.kind, e);
      acks.push({ eventId: ev.eventId, status: "rejected", reason: "server" });
    }
  }

  if (acks.length > 0) {
    try {
      await api("ack_events", { acks });
    } catch (e) {
      // The local applied_order_events ledger makes a re-pull harmless.
      console.info("[orders] ack failed (ledger prevents double-apply):", e);
    }
  }
  if (anyApplied || acks.length > 0) {
    void pushOrdersNow();
    emitOrdersChanged();
  }
  if (alerts.length > 0) {
    if (alerts.some((a) => a.isNewOrder)) playNewOrderChime();
    alerts.forEach((a) => alertSubs.forEach((cb) => cb(a)));
  }
  emitStatus();
}

async function drainEvents(): Promise<void> {
  if (!live || !isDbOpen()) return;
  if (draining) {
    drainRerun = true;
    return;
  }
  draining = true;
  try {
    for (let round = 0; round < 10; round++) {
      const data = await api<{ events: WireEvent[] }>("pull_events", { limit: 50 });
      const events = (data.events ?? []).filter((e) => !staleHeld.has(e.eventId));
      if (events.length === 0) break;
      await applyEventList(events, false);
      if ((data.events ?? []).length < 50) break;
    }
  } catch (e) {
    console.info("[orders] pull_events failed (will retry):", e);
  } finally {
    draining = false;
    if (drainRerun) {
      drainRerun = false;
      void drainEvents();
    }
  }
}

/** "N orders arrived while you were offline — Print now". */
export async function printStaleOrders(): Promise<void> {
  const held = [...staleHeld.values()];
  staleHeld.clear();
  await applyEventList(held, true);
}

/** "… — Discard": reject the held events; reconcile cancels their cloud rows. */
export async function discardStaleOrders(): Promise<void> {
  const held = [...staleHeld.values()];
  staleHeld.clear();
  try {
    await api("ack_events", {
      acks: held.map((ev) => ({ eventId: ev.eventId, status: "rejected", reason: "order-gone" })),
    });
  } catch (e) {
    console.info("[orders] discard ack failed:", e);
  }
  await reconcileNow();
  emitStatus();
}

/* ---------------------------- lifecycle ---------------------------- */

function clearTimers(): void {
  for (const t of [heartbeatTimer, reconcileTimer, degradedTimer]) if (t) clearInterval(t);
  heartbeatTimer = reconcileTimer = degradedTimer = null;
  if (retryTimer) clearTimeout(retryTimer);
  if (pushTimer) clearTimeout(pushTimer);
  if (catalogTimer) clearTimeout(catalogTimer);
  retryTimer = pushTimer = catalogTimer = null;
}

function setDegradedPolling(on: boolean): void {
  if (on && !degradedTimer) {
    degradedTimer = setInterval(() => void drainEvents(), DEGRADED_POLL_MS);
  } else if (!on && degradedTimer) {
    clearInterval(degradedTimer);
    degradedTimer = null;
  }
}

async function goLive(): Promise<void> {
  if (live) return;
  live = true;
  const device = await getDeviceInfo();

  connectRealtime({
    roomId,
    deviceId: device.id,
    deviceName: device.name,
    appVersion,
    callbacks: {
      onDoorbell: (payload) => {
        if (payload.kind === "events") void drainEvents();
        // 'orders'/'catalog' pings originate from this POS — nothing to fetch.
      },
      onPhonesChange: (phones) => {
        status.phones = phones;
        emitStatus();
      },
      onStatusChange: (s) => {
        status.channel = s;
        setDegradedPolling(s === "degraded");
        if (s === "connected") void drainEvents(); // catch anything missed
        emitStatus();
      },
    },
  });
  status.channel = "degraded"; // until SUBSCRIBED lands
  setDegradedPolling(true);

  heartbeatTimer = setInterval(
    () =>
      sendHello().catch((e) => {
        const reason = String((e as Error)?.message ?? e);
        // License deleted / moved to another machine -> stop cleanly.
        if (reason === "invalid-key" || reason === "unbound" || reason === "no-license") {
          console.info("[orders] bridge stopping:", reason);
          goIdle();
        }
      }),
    HEARTBEAT_MS
  );
  reconcileTimer = setInterval(() => void reconcileNow(), RECONCILE_MS);

  await reconcileNow();
  void pushOrdersNow();
  void catalogPushNow();
  void drainEvents();
  emitStatus();
}

function goIdle(): void {
  live = false;
  disconnectRealtime();
  clearTimers();
  status.channel = "off";
  status.phones = 0;
  emitStatus();
}

async function initialize(): Promise<void> {
  try {
    if (!isDbOpen() || !getStoredLicenseKey()) {
      // No DB / no license yet — check back quietly; billing is unaffected.
      retryTimer = setTimeout(() => void initialize(), 60_000);
      return;
    }
    const state = await getOrderSyncState();
    enabled = state.mobileOrderingEnabled;
    soundOnNewOrder = state.soundOnNewOrder;
    roomId = state.roomId;

    await sendHello(); // also informs the server of the current flag state
    void pruneAppliedEvents().catch(() => {});

    if (enabled) await goLive();
    else emitStatus();
  } catch (e) {
    console.info("[orders] bridge init failed (retrying):", e);
    retryTimer = setTimeout(() => void initialize(), 15_000);
  }
}

/** Called by the settings screen the moment the owner flips the switch. */
export async function setMobileOrderingEnabledLive(next: boolean): Promise<void> {
  enabled = next;
  emitStatus();
  try {
    await sendHello();
  } catch (e) {
    console.info("[orders] hello after toggle failed:", e);
  }
  if (!started) return;
  if (next && !live) await goLive();
  else if (!next && live) goIdle();
}

/** Settings screen: refresh sound preference without a restart. */
export function setNewOrderSound(on: boolean): void {
  soundOnNewOrder = on;
}

export async function blockInstallRemote(installId: string, blocked: boolean): Promise<void> {
  await api("block_install", { installId, blocked });
  await sendHello(); // refresh the installs list
}

export function startOrderBridge(): void {
  if (started) return;
  started = true;
  registerOrdersPushHandler(requestPushDebounced);
  registerCatalogPushHandler(requestCatalogDebounced);
  getVersion()
    .then((v) => (appVersion = v))
    .catch(() => {})
    .finally(() => void initialize());
}

export function stopOrderBridge(): void {
  if (!started) return;
  started = false;
  registerOrdersPushHandler(null);
  registerCatalogPushHandler(null);
  goIdle();
}
