import { getVersion } from "@tauri-apps/api/app";
import { SUPABASE_ANON_KEY } from "../../config/supabase";
import { isDbOpen } from "../../db/client";
import * as ordersRepo from "../../db/repositories/ordersRepo";
import * as menuRepo from "../../db/repositories/menuRepo";
import {
  getOrderSyncState,
  setCatalogHash,
  setLastOrdersSeq,
  setLastReconcileAt,
  setRoomId,
} from "../../db/repositories/orderSyncRepo";
import { pruneAppliedEvents } from "../../db/repositories/appliedEventsRepo";
import { loadSettings } from "../../db/repositories/settingsRepo";
import { getStoredLicenseKey } from "../license/licenseService";
import { getDeviceInfo } from "../license/device";
import {
  connectRealtime,
  disconnectRealtime,
  forceRejoinRealtime,
  type Bell,
} from "../realtime/client";
import { pushUnsyncedBills } from "../sync/billSync";
import { WakeWatchdog } from "./wakeWatchdog";
import {
  clearSession,
  currentRoomId,
  EdgeBudgetExceeded,
  ensureSession,
  getEdgeUsage,
  rpc,
  table,
  type EdgeUsage,
} from "./cloud";
import { buildCatalogPayload } from "./catalogSync";
import { applyOrderEvent, type WireEvent } from "./eventApplier";
import { registerCatalogPushHandler, registerOrdersPushHandler } from "./signals";
import { finalizedRowToWire, processingRowToWire, wireOrderToRow, type WireOrder } from "./wire";

/**
 * The mobile-orders bridge. Owns the whole cloud lifecycle for live orders.
 *
 * WHAT CHANGED IN THE REBUILD:
 *  - Nothing asks "anything new?" on a timer. Phone intents arrive ON THE
 *    BELL, carrying the intent itself; the counter applies it without a
 *    fetch. An idle counter makes no data calls at all.
 *  - Reads and writes are PostgREST (unmetered), not Edge Functions. The
 *    only Edge call in the feature is enrolment, once per install.
 *  - The 30-second `hello` heartbeat is gone. Liveness is a side effect of
 *    the counter's ordinary writes, plus one unmetered 60-second beat while
 *    the counter is idle (decision D8).
 *
 * Everything is still fire-and-forget: a network failure must NEVER block
 * billing, throw into the UI, or lose a local write.
 */

export interface InstallInfo {
  installId: string;
  label: string;
  actorKind: string;
  actorName: string;
  lastSeen: string;
  blocked: boolean;
}

/**
 * Why the channel is not fully connected. "" means nothing is wrong.
 *  - misconfigured: this build has no Supabase anon key, so the cloud can
 *    never be reached. The owner needs a newer installer.
 *  - realtime-down: the socket failed or dropped.
 *  - flapping: the socket keeps dropping; we have backed off deliberately.
 *  - budget: the invocation ceiling was reached (a safety limit, not normal).
 */
export type OrderBridgeFault = "" | "misconfigured" | "realtime-down" | "flapping" | "budget";

export interface OrderBridgeStatus {
  featureEnabled: boolean;
  channel: "connected" | "degraded" | "off";
  fault: OrderBridgeFault;
  /** False when the counter cannot reach the cloud at all (no internet). */
  cloudReachable: boolean;
  phones: number;
  installs: InstallInfo[];
  maxMobileDevices: number;
  lastSyncAt: string | null;
  /** Events older than 2h held back from auto-printing. */
  staleCount: number;
  /** Plain-English cloud usage for the owner (5.4). */
  usage: EdgeUsage;
}

export interface NewOrderAlert {
  isNewOrder: boolean;
  tableNumber: string;
  orderType: string;
  waiterName: string;
  total: number;
  printError?: string;
}

/* ------------------------------ intervals ------------------------------ */

/**
 * The counter proves it is alive by doing its job: every order push and
 * every event ack stamps licenses.pos_last_seen_at (migration 0013). This
 * beat covers an IDLE counter only — it is skipped entirely whenever the
 * counter has written something recently.
 *
 * It is a PostgREST call, which is not metered by count, and it carries
 * about 200 bytes. The server accepts phone intents while pos_last_seen_at
 * is within 300s (mb_pos_live_window, migration 0019), i.e. FIVE beats, so
 * a run of lost beats can never refuse a waiter, and a counter that has
 * genuinely died stops accepting orders within five minutes.
 *
 * The window is 300s because it is deliberately equal to the phone's trust
 * window: the phone checks the counter on an event and then believes the
 * answer for five minutes, so any shorter server window would let the badge
 * and the server disagree. THIS BEAT IS WHAT MAKES THAT SAFE — do not
 * lengthen it without revisiting mb_pos_live_window().
 */
const POS_ALIVE_MS = 60_000;
const POS_ALIVE_SKIP_MS = 45_000;

const RECONCILE_MS = 5 * 60_000;
const PUSH_DEBOUNCE_MS = 300;
const CATALOG_DEBOUNCE_MS = 2_000;
const STALE_EVENT_MS = 2 * 60 * 60_000;

/**
 * 5.1 — a fallback read may arm only after the socket has been continuously
 * down for this long, and stops the instant it recovers. It is a PostgREST
 * read, so even this costs nothing against the invocation quota.
 */
const FALLBACK_ARM_AFTER_MS = 30_000;
const FALLBACK_POLL_MS = 45_000;

/** 5.5 — the UI only admits to being degraded after this long, so a blink
 *  can never move the page. Recovery is instant. */
const DEGRADED_UI_DEBOUNCE_MS = 10_000;

/**
 * v1.3.0's installer was built without VITE_SUPABASE_ANON_KEY. The build now
 * refuses to produce a keyless bundle (vite.config.ts), but a running app
 * must still cope: with no key nothing cloud-side can work, so we say so
 * plainly instead of failing in the dark.
 */
const ANON_KEY_MISSING = !SUPABASE_ANON_KEY || String(SUPABASE_ANON_KEY).trim().length < 20;
const ANON_KEY_MISSING_MSG =
  "[orders] this build has no Supabase anon key: mobile ordering cannot run. " +
  "Install the latest version of Magic Bill.";

/* ------------------------------- state ------------------------------- */

let started = false;
let live = false;
let enabled = false;
let roomId = "";
let serverOffsetMs = 0;
let soundOnNewOrder = true;
let appVersion = "";
let lastOrdersSeq = 0;
let lastPosWriteAt = 0;
let socketUp = false;
let socketDownSince = 0;

const status: OrderBridgeStatus = {
  featureEnabled: false,
  channel: "off",
  fault: "",
  cloudReachable: true,
  phones: 0,
  installs: [],
  maxMobileDevices: 1,
  lastSyncAt: null,
  staleCount: 0,
  usage: getEdgeUsage(),
};

const staleHeld = new Map<string, WireEvent>();
const printErrors = new Map<string, string>();
let pendingCloudOrders: WireOrder[] = [];

let aliveTimer: ReturnType<typeof setInterval> | null = null;
let reconcileTimer: ReturnType<typeof setInterval> | null = null;
let fallbackTimer: ReturnType<typeof setInterval> | null = null;
let fallbackArmTimer: ReturnType<typeof setTimeout> | null = null;
let degradedUiTimer: ReturnType<typeof setTimeout> | null = null;
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
  status.usage = getEdgeUsage();
  const snapshot = { ...status, installs: [...status.installs] };
  statusSubs.forEach((cb) => cb(snapshot));
}

function emitOrdersChanged(): void {
  ordersChangedSubs.forEach((cb) => cb());
}

export function getOrderBridgeStatus(): OrderBridgeStatus {
  status.featureEnabled = enabled;
  status.staleCount = staleHeld.size;
  status.usage = getEdgeUsage();
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

/* ------------------------------ plumbing ------------------------------ */

function setCloudReachable(next: boolean): void {
  if (status.cloudReachable === next) return;
  status.cloudReachable = next;
  if (!next) {
    console.error("[orders] cannot reach the Magic Bill cloud — check this PC's internet");
  }
  emitStatus();
}

/**
 * Wraps every cloud call so one failure can never reach billing.
 *
 * It also EMITS on success. That is not cosmetic: while a restaurant is idle
 * the only thing talking to the cloud is the 60-second liveness beat, and
 * without an emission the settings screen keeps rendering the last snapshot
 * it was given. "Last sync" then freezes and, after 3 minutes, turns orange
 * with "— not updating" on a counter that is working perfectly. Caught on
 * real hardware during a 15-minute idle watch: the beat was landing (the
 * server's pos_last_seen_at was 33s old) while the screen claimed it had
 * stopped.
 */
async function guarded<T>(what: string, fn: () => Promise<T>): Promise<T | null> {
  try {
    const result = await fn();
    setCloudReachable(true);
    status.lastSyncAt = new Date().toISOString();
    if (status.fault === "budget") status.fault = "";
    emitStatus();
    return result;
  } catch (e) {
    if (e instanceof EdgeBudgetExceeded) {
      status.fault = "budget";
      emitStatus();
      return null;
    }
    const reason = String((e as Error)?.message ?? e);
    if (reason === "invalid-key" || reason === "unbound" || reason === "no-license") {
      console.info("[orders] bridge stopping:", reason);
      void clearSession();
      goIdle();
      return null;
    }
    // A timed-out request is exactly as much of a "cannot reach the cloud"
    // as a refused one. Before PART D it could not even get this far — the
    // call simply never came back.
    if (/fetch|network|Failed to fetch|timed out|timeout|abort/i.test(reason)) {
      setCloudReachable(false);
    }
    console.info(`[orders] ${what} failed (will retry):`, reason);
    return null;
  }
}

function markPosWrite(): void {
  lastPosWriteAt = Date.now();
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
 * PART F5 — belt and braces on top of the database's own no-op guard.
 *
 * Migration 0021 stops the bell ringing when a write produces an identical
 * wire payload. This stops the write happening at all: the last payload
 * SUCCESSFULLY pushed for each order is remembered, and an unchanged one is
 * not sent again.
 *
 * IT DELIBERATELY DOES NOT APPLY TO A FORCED PUSH. The five-minute reconcile
 * republishes the full open set precisely so a cloud row that drifted gets
 * repaired — and drift is invisible from here, because our copy is exactly
 * what it always was. Skipping on force would turn the self-heal off, which
 * is a far worse bug than the one being fixed.
 *
 * In memory only. A restart re-pushes everything once, which is correct: we
 * have no idea what the cloud holds until we have written to it.
 */
const pushedHashes = new Map<string, number>();

/** FNV-1a, 32-bit. Cheap, stable, and small enough to hold for every order. */
function payloadHash(o: WireOrder): number {
  const s = JSON.stringify(o);
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

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
    const extras = pendingCloudOrders.slice();

    const candidates: WireOrder[] = [
      ...dirtyOpen.map((o) => processingRowToWire(o, printErrors.get(o.remote_uuid!) ?? "")),
      ...dirtyBilled.map(finalizedRowToWire),
      ...extras,
    ];
    if (candidates.length === 0) return;

    const licenseKey = getStoredLicenseKey();
    if (!licenseKey) return;

    /** Clears the local bookkeeping for everything this cycle considered. */
    const settleLocalState = async () => {
      await ordersRepo.clearProcessingCloudDirty(dirtyOpen);
      await ordersRepo.clearFinalizedCloudDirty(dirtyBilled.map((o) => o.id));
      pendingCloudOrders = pendingCloudOrders.filter((o) => !extras.includes(o));
      [...dirtyBilled, ...extras].forEach((o) => {
        const uuid = "clientUuid" in o ? (o as WireOrder).clientUuid : (o as any).remote_uuid;
        if (uuid) printErrors.delete(uuid);
      });
    };

    // F5. On a FORCED push everything goes, because force is the self-heal.
    const orders = force
      ? candidates
      : candidates.filter((o) => pushedHashes.get(o.clientUuid) !== payloadHash(o));

    if (orders.length === 0) {
      // Every dirty row turned out to be byte-for-byte what we last sent.
      // Nothing to say to the cloud; just stop flagging them as dirty.
      await settleLocalState();
      return;
    }

    const ok = await guarded("push orders", async () => {
      const t = await table("live_orders");
      const { error } = await t.upsert(
        orders.map((o) => wireOrderToRow(o, licenseKey)),
        { onConflict: "license_key,client_uuid" },
      );
      if (error) throw new Error(error.message);
      return true;
    });
    if (!ok) return;

    markPosWrite();
    // Only ever recorded AFTER the cloud confirmed the write.
    orders.forEach((o) => pushedHashes.set(o.clientUuid, payloadHash(o)));
    await settleLocalState();
    emitStatus();
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

/**
 * Reconcile, then republish the FULL open set. Reconcile closes cloud rows
 * the counter no longer has; the republish repairs any open order whose
 * cloud copy drifted, so the cloud converges on the counter's truth within
 * one cycle even if a write was lost.
 */
async function reconcileNow(): Promise<void> {
  if (!live) return;
  await ensureRemoteUuids();
  const open = await ordersRepo.listProcessingOrders();
  const openClientUuids = open.map((o) => o.remote_uuid).filter(Boolean) as string[];
  const result = await guarded("reconcile", () =>
    rpc("mb_reconcile_orders", { p_open_client_uuids: openClientUuids }),
  );
  if (result === null) return;
  markPosWrite();
  await setLastReconcileAt(new Date().toISOString());
  await pushOrdersNow(true);
  // Keep the F5 memory bounded: an order that has left the open set will
  // never be pushed again from a dirty flag, and if it somehow is, sending
  // it once more is the safe direction to be wrong in.
  const stillOpen = new Set(openClientUuids);
  for (const uuid of [...pushedHashes.keys()]) {
    if (!stillOpen.has(uuid)) pushedHashes.delete(uuid);
  }
}

async function catalogPushNow(force = false): Promise<void> {
  if (!live) return;
  const payload = await buildCatalogPayload();
  const state = await getOrderSyncState();
  if (!force && state.catalogHash === payload.catalogHash) return;
  const result = await guarded("push catalog", () =>
    rpc("mb_push_catalog", {
      p_categories: payload.categories,
      p_items: payload.items,
      p_tables: payload.tables,
      p_customers: payload.customers,
      p_catalog_hash: payload.catalogHash,
    }),
  );
  if (result === null) return;
  markPosWrite();
  await setCatalogHash(payload.catalogHash);
  console.info("[orders] catalog pushed");
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

/**
 * Intents are applied ONE AT A TIME, in arrival order. The bell can deliver
 * two intents in the same millisecond and the applier is not reentrant —
 * it claims token numbers and drives printers.
 */
let applyChain: Promise<void> = Promise.resolve();
function enqueue(fn: () => Promise<void>): void {
  applyChain = applyChain.then(fn).catch((e) => {
    console.error("[orders] apply queue error:", e);
  });
}

async function applyEventList(events: WireEvent[], ignoreStaleness: boolean): Promise<void> {
  if (events.length === 0) return;
  const settings = await loadSettings();
  const categories = await menuRepo.listCategories();
  const state = await getOrderSyncState();
  soundOnNewOrder = state.soundOnNewOrder;

  const acks: {
    eventId: string;
    status: "applied" | "rejected";
    reason?: string;
    /** The order this event changed, so the ack and the push are one call. */
    orderClientUuid?: string | null;
  }[] = [];
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
      acks.push({
        eventId: ev.eventId,
        status: result.status,
        reason: result.reason,
        orderClientUuid: ev.orderClientUuid,
      });
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

  // Acking is what resolves the phone's "Sending…" — including a REJECTION,
  // which must reach the phone or a waiter sits on "Sending…" forever.
  await ackEvents(acks);

  if (anyApplied || acks.length > 0) {
    // Any order the ack did not already carry (a counter-side edit that
    // happened while we were applying) still goes out here.
    void pushOrdersNow();
    emitOrdersChanged();
  }
  if (alerts.length > 0) {
    if (alerts.some((a) => a.isNewOrder)) playNewOrderChime();
    alerts.forEach((a) => alertSubs.forEach((cb) => cb(a)));
  }
  emitStatus();
}

/**
 * Ack an applied (or rejected) intent AND publish the resulting order truth
 * in ONE call. `mb_apply_event` does both inside one transaction and rings
 * the bell once, so:
 *   - a phone can never see a new order before it learns its "Sending…"
 *     resolved, and
 *   - a rejection always reaches the phone — the failure that used to leave
 *     a waiter stuck on "Sending…" once the phone stopped polling.
 *
 * It also halved the realtime traffic: this used to be two writes and
 * therefore two broadcasts, milliseconds apart, to the same audience.
 *
 * The local applied_order_events ledger makes a failed call harmless: the
 * event is re-applied-and-acked later without reprinting anything.
 */
async function ackEvents(
  acks: {
    eventId: string;
    status: "applied" | "rejected";
    reason?: string;
    orderClientUuid?: string | null;
  }[],
): Promise<void> {
  if (acks.length === 0) return;
  const open = await ordersRepo.listProcessingOrders();

  for (const a of acks) {
    let orderJson: Record<string, unknown> | null = null;
    if (a.orderClientUuid) {
      const row = open.find((o) => o.remote_uuid === a.orderClientUuid);
      if (row) {
        orderJson = processingRowToWire(row, printErrors.get(a.orderClientUuid) ?? "") as
          unknown as Record<string, unknown>;
      } else {
        // The order closed as part of applying this event (settled or
        // cancelled). Its final truth is queued in pendingCloudOrders.
        const closed = pendingCloudOrders.find((o) => o.clientUuid === a.orderClientUuid);
        if (closed) {
          orderJson = closed as unknown as Record<string, unknown>;
          pendingCloudOrders = pendingCloudOrders.filter((o) => o !== closed);
        }
      }
    }

    const ok = await guarded("apply event", () =>
      rpc("mb_apply_event", {
        p_event_id: a.eventId,
        p_status: a.status,
        p_reason: a.reason ?? null,
        p_order: orderJson,
      }),
    );
    if (ok && a.orderClientUuid) {
      // Published — no need for the debounced push to send it again.
      const row = open.find((o) => o.remote_uuid === a.orderClientUuid);
      if (row) await ordersRepo.clearProcessingCloudDirty([row]);
      printErrors.delete(a.orderClientUuid);
    }
  }
  markPosWrite();
}

/**
 * The catch-up read. Runs on startup, and only otherwise when the bridge
 * has REASON to think it missed something (a sequence gap, or the socket
 * having been down). It is never on a timer while the socket is healthy.
 */
let catchingUp = false;
async function catchUpEvents(): Promise<void> {
  if (!live || !isDbOpen() || catchingUp) return;
  catchingUp = true;
  try {
    const rows = await guarded("read pending intents", async () => {
      const t = await table("order_events");
      const { data, error } = await t
        .select(
          "id, client_event_id, kind, order_id, order_client_uuid, payload, " +
            "actor_kind, actor_id, actor_name, created_at",
        )
        .eq("status", "pending")
        .order("created_at", { ascending: true })
        .limit(100);
      if (error) throw new Error(error.message);
      return data ?? [];
    });
    if (!rows || rows.length === 0) return;

    const events: WireEvent[] = rows
      .map((e: any) => ({
        eventId: e.id,
        clientEventId: e.client_event_id,
        kind: e.kind,
        orderId: e.order_id,
        orderClientUuid: e.order_client_uuid,
        payload: e.payload ?? {},
        actorKind: e.actor_kind,
        actorId: e.actor_id,
        actorName: e.actor_name ?? "",
        createdAt: e.created_at,
      }))
      .filter((e) => !staleHeld.has(e.eventId));
    await applyEventList(events, false);
  } finally {
    catchingUp = false;
  }
}

/** "N orders arrived while you were offline — Print now". */
export async function printStaleOrders(): Promise<void> {
  const held = [...staleHeld.values()];
  staleHeld.clear();
  enqueue(() => applyEventList(held, true));
}

/** "… — Discard": reject the held events; reconcile cancels their cloud rows. */
export async function discardStaleOrders(): Promise<void> {
  const held = [...staleHeld.values()];
  staleHeld.clear();
  await ackEvents(
    held.map((ev) => ({
      eventId: ev.eventId,
      status: "rejected" as const,
      reason: "order-gone",
      orderClientUuid: ev.orderClientUuid,
    })),
  );
  await reconcileNow();
  emitStatus();
}

/* ------------------------------- the bell ------------------------------- */

function onBell(bell: Bell): void {
  const seq = Number(bell.seq ?? 0);

  if (bell.kind === "event") {
    const e = bell.event as Record<string, any> | undefined;
    if (!e?.eventId) return;
    const ev: WireEvent = {
      eventId: String(e.eventId),
      clientEventId: String(e.clientEventId ?? ""),
      kind: String(e.kind ?? ""),
      orderId: e.orderId ?? null,
      orderClientUuid: e.orderClientUuid ?? null,
      payload: (e.payload ?? {}) as Record<string, unknown>,
      actorKind: String(e.actorKind ?? "staff"),
      actorId: e.actorId ?? null,
      actorName: String(e.actorName ?? ""),
      createdAt: String(e.createdAt ?? new Date().toISOString()),
    };
    // The intent arrives WITH the bell — no fetch, no fan-out. The ledger in
    // eventApplier makes a duplicate bell (or a bell plus a catch-up read)
    // completely harmless.
    enqueue(() => applyEventList([ev], false));
    return;
  }

  if (bell.kind === "order") {
    // Authored by this counter. The only thing worth acting on is a gap,
    // which means we were away while something changed.
    if (seq > 0) {
      if (lastOrdersSeq > 0 && seq > lastOrdersSeq + 1) {
        enqueue(catchUpEvents);
      }
      lastOrdersSeq = seq;
      void setLastOrdersSeq(seq).catch(() => {});
    }
    return;
  }

  // 'event_status' and 'catalog' bells are echoes of this counter's own
  // writes. Nothing to do.
}

/* ---------------------------- lifecycle ---------------------------- */

function clearTimers(): void {
  for (const t of [aliveTimer, reconcileTimer, fallbackTimer]) if (t) clearInterval(t);
  aliveTimer = reconcileTimer = fallbackTimer = null;
  for (const t of [retryTimer, pushTimer, catalogTimer, fallbackArmTimer, degradedUiTimer]) {
    if (t) clearTimeout(t);
  }
  retryTimer = pushTimer = catalogTimer = fallbackArmTimer = degradedUiTimer = null;
}

/**
 * 5.1 — the fallback arms only after the socket has been continuously down
 * for 30s, then reads at 45s, and stops the instant the socket recovers.
 */
function setFallback(on: boolean): void {
  if (on) {
    if (fallbackTimer || fallbackArmTimer) return;
    fallbackArmTimer = setTimeout(() => {
      fallbackArmTimer = null;
      if (socketUp || !live) return;
      console.info("[orders] live connection down for 30s — reading intents every 45s instead");
      fallbackTimer = setInterval(() => enqueue(catchUpEvents), FALLBACK_POLL_MS);
      enqueue(catchUpEvents);
    }, FALLBACK_ARM_AFTER_MS);
  } else {
    if (fallbackArmTimer) {
      clearTimeout(fallbackArmTimer);
      fallbackArmTimer = null;
    }
    if (fallbackTimer) {
      clearInterval(fallbackTimer);
      fallbackTimer = null;
    }
  }
}

/**
 * 5.5 — entering the degraded state is debounced by 10s so a momentary
 * reconnect can never move the page; recovery is instant.
 */
function setChannelStatus(next: "connected" | "degraded"): void {
  if (next === "connected") {
    socketUp = true;
    socketDownSince = 0;
    if (degradedUiTimer) {
      clearTimeout(degradedUiTimer);
      degradedUiTimer = null;
    }
    setFallback(false);
    if (status.channel !== "connected" || status.fault !== "") {
      status.channel = "connected";
      status.fault = "";
      emitStatus();
    }
    return;
  }

  socketUp = false;
  if (socketDownSince === 0) socketDownSince = Date.now();
  setFallback(true);
  if (status.channel === "degraded") return;
  if (degradedUiTimer) return;
  degradedUiTimer = setTimeout(() => {
    degradedUiTimer = null;
    if (socketUp) return;
    status.channel = "degraded";
    if (status.fault === "") status.fault = "realtime-down";
    emitStatus();
  }, DEGRADED_UI_DEBOUNCE_MS);
}

async function sendHello(mobileOrderingEnabled?: boolean): Promise<void> {
  const data = await guarded<any>("hello", () =>
    rpc("mb_pos_hello", {
      p_app_version: appVersion,
      p_mobile_ordering_enabled: mobileOrderingEnabled ?? null,
    }),
  );
  if (!data?.ok) return;
  markPosWrite();
  serverOffsetMs = Date.parse(data.serverTime) - Date.now();
  status.installs = Array.isArray(data.installs)
    ? data.installs.map((i: any) => ({
        installId: i.installId,
        label: i.label ?? "",
        actorKind: i.actorKind ?? "",
        actorName: i.actorName ?? "",
        lastSeen: i.lastSeen,
        blocked: i.blocked === true,
      }))
    : [];
  status.maxMobileDevices = Number(data.maxMobileDevices ?? 1);
  lastOrdersSeq = Number(data.ordersSeq ?? 0);
  if (data.roomId && data.roomId !== roomId) {
    roomId = data.roomId;
    await setRoomId(roomId);
  }
  emitStatus();
}

/**
 * The idle-counter liveness beat (D8). Skipped whenever we wrote recently.
 *
 * @param force ignore the skip window. The wake path uses this: after a
 * sleep, proving the counter is alive is the single most urgent thing to do,
 * because the phone is sitting there reading a pos_last_seen_at that stopped
 * advancing when the machine suspended.
 */
async function posAliveBeat(force = false): Promise<void> {
  if (!live) return;
  if (!force && Date.now() - lastPosWriteAt < POS_ALIVE_SKIP_MS) return;
  const data = await guarded<any>("liveness beat", () => rpc("mb_pos_alive"));
  if (data?.ok) markPosWrite();
}

/* ------------------------------ waking up ------------------------------ */

let wakeWatchdog: WakeWatchdog | null = null;
let resumeHintHandler: (() => void) | null = null;

/**
 * PART D. Everything the counter must do after the PC has been asleep, in
 * the order that matters:
 *
 *  1. rebuild both channels WITHOUT asking the socket how it is — after a
 *     suspend it is commonly half-open and answers confidently wrong;
 *  2. beat once, so the phone stops reading a stale "the counter was last
 *     seen before the machine slept";
 *  3. reconcile once, so any order that changed at either end converges;
 *  4. flush the bill outbox, because anything finalised just before the
 *     sleep is still sitting there unsynced.
 *
 * Each step is independently guarded: a failure in one must not stop the
 * rest, and the watchdog will simply try again on its next tick.
 */
async function recoverFromWake(): Promise<void> {
  if (!live) return;
  await forceRejoinRealtime().catch((e) =>
    console.info("[orders] wake: channel rebuild failed (will retry):", e),
  );
  await posAliveBeat(true);
  await reconcileNow().catch(() => {});
  await pushUnsyncedBills().catch(() => {});
  enqueue(catchUpEvents);
  emitStatus();
}

function startWakeWatchdog(): void {
  if (wakeWatchdog) return;
  wakeWatchdog = new WakeWatchdog({
    onWake: () => recoverFromWake(),
    log: (m) => console.info(m),
  });
  wakeWatchdog.start();

  // Platform resume signals are an EXTRA trigger, never the only one — they
  // differ between Windows builds and webview versions, and the clock
  // comparison above is the part that is reliable everywhere. These are free:
  // resumeHint() returns immediately unless real time actually jumped.
  resumeHintHandler = () => void wakeWatchdog?.resumeHint();
  window.addEventListener("focus", resumeHintHandler);
  document.addEventListener("visibilitychange", resumeHintHandler);
}

function stopWakeWatchdog(): void {
  if (resumeHintHandler) {
    window.removeEventListener("focus", resumeHintHandler);
    document.removeEventListener("visibilitychange", resumeHintHandler);
    resumeHintHandler = null;
  }
  wakeWatchdog?.stop();
  wakeWatchdog = null;
}

function startRealtime(device: { id: string; name: string }): void {
  if (!live || !roomId) return;
  connectRealtime({
    roomId,
    deviceId: device.id,
    deviceName: device.name,
    appVersion,
    callbacks: {
      onBell,
      onPhonesChange: (phones) => {
        status.phones = phones;
        emitStatus();
      },
      onStatusChange: setChannelStatus,
      onFlapping: () => {
        status.fault = "flapping";
        status.channel = "degraded";
        emitStatus();
      },
    },
  });
}

async function goLive(): Promise<void> {
  if (live) return;
  live = true;
  try {
    const device = await getDeviceInfo();

    // The credential first: everything below needs it, and it is the only
    // step that can call an Edge Function.
    await ensureSession();
    if (!roomId) roomId = currentRoomId();

    await sendHello(enabled);
    startRealtime(device);

    aliveTimer = setInterval(() => void posAliveBeat(), POS_ALIVE_MS);
    reconcileTimer = setInterval(() => void reconcileNow(), RECONCILE_MS);
    startWakeWatchdog();

    await reconcileNow();
    void pushOrdersNow();
    void catalogPushNow();
    enqueue(catchUpEvents);
    emitStatus();
  } catch (e) {
    // Never leave `live = true` behind a failed start-up: initialize()'s
    // retry calls goLive() again and must be able to genuinely retry.
    console.error("[orders] bridge failed to go live (will retry):", e);
    live = false;
    void disconnectRealtime();
    stopWakeWatchdog();
    clearTimers();
    status.channel = "off";
    status.phones = 0;
    if (e instanceof EdgeBudgetExceeded) status.fault = "budget";
    emitStatus();
    throw e;
  }
}

function goIdle(): void {
  live = false;
  void disconnectRealtime();
  stopWakeWatchdog();
  clearTimers();
  status.channel = "off";
  status.fault = "";
  status.phones = 0;
  socketUp = false;
  // We know nothing about what the cloud holds until we have written to it
  // again, so the F5 memory must not survive a lifecycle.
  pushedHashes.clear();
  emitStatus();
}

async function initialize(): Promise<void> {
  try {
    if (!isDbOpen() || !getStoredLicenseKey()) {
      // No DB / no license yet — check back quietly; billing is unaffected.
      retryTimer = setTimeout(() => void initialize(), 60_000);
      return;
    }
    if (ANON_KEY_MISSING) {
      console.error(ANON_KEY_MISSING_MSG);
      status.fault = "misconfigured";
      status.channel = "off";
      emitStatus();
      return;
    }
    const state = await getOrderSyncState();
    enabled = state.mobileOrderingEnabled;
    soundOnNewOrder = state.soundOnNewOrder;
    roomId = state.roomId;
    lastOrdersSeq = state.lastOrdersSeq;

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
  if (!started) return;
  if (next) {
    if (!live) await goLive().catch(() => {});
    else await sendHello(true);
  } else {
    // Tell the server BEFORE going idle, or phones keep ordering into a
    // counter that has stopped listening.
    await sendHello(false);
    goIdle();
  }
}

/** Settings screen: refresh sound preference without a restart. */
export function setNewOrderSound(on: boolean): void {
  soundOnNewOrder = on;
}

export async function blockInstallRemote(installId: string, blocked: boolean): Promise<void> {
  const t = await table("mobile_installs");
  const { error } = await t.update({ blocked }).eq("install_id", installId);
  if (error) throw new Error(error.message);
  markPosWrite();
  await sendHello();
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
