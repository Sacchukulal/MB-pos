import { createClient, type SupabaseClient } from "@supabase/supabase-js";
import { SUPABASE_ANON_KEY, SUPABASE_FUNCTIONS_URL, SUPABASE_URL } from "../../config/supabase";
import { getStoredLicenseKey } from "../license/licenseService";
import { getDeviceInfo } from "../license/device";
import {
  getOrderSyncState,
  setEdgeCallLog,
  setOrdersSession,
} from "../../db/repositories/orderSyncRepo";
import { CLOUD_TIMEOUT_MS, timeoutFetch } from "../net/timeout";
import { SessionKeeper, type OrdersSession } from "./sessionKeeper";

/**
 * The ONE seam between the counter and the cloud (decision D10). Everything
 * above this file talks in terms of orders and events; only this file knows
 * that the transport happens to be Supabase. A future LAN server replaces
 * this module and nothing else.
 *
 * Three rules define the whole design:
 *
 *  1. EDGE FUNCTION INVOCATIONS ARE THE ONLY METERED CALL. Exactly one
 *     Edge Function is ever called from here — `orders-enroll`, which mints
 *     this counter's credential. It runs once per install, and again only
 *     if the refresh token is lost or rejected. Everything else is
 *     PostgREST (reads, writes, RPCs), which is not metered by count.
 *     Since August that includes BILL SYNC, which used to be one metered
 *     call per bill and is now mb_push_bills (migration 0020).
 *
 *  2. A HARD CEILING NO BUG CAN BREACH (P5). Every Edge call is recorded in
 *     a rolling log that survives restarts, and the ceilings below are
 *     enforced before the call is made, not after.
 *
 *  3. NOTHING HERE MAY HANG (August, PART D). Every request has a 15-second
 *     deadline and no shared promise can become permanent. A PC that sleeps
 *     leaves its sockets half-open, and before this the counter deadlocked
 *     behind one of them until the process was restarted. See
 *     ../net/timeout.ts for the full account.
 */

/* -------------------------- the invocation ceiling -------------------------- */

/**
 * Hourly runaway guard. A reconnect loop, a retry storm or a bad deploy can
 * burn an hour's worth of calls in seconds; this stops it inside one hour
 * without waiting for the 30-day ceiling to notice.
 */
export const EDGE_CALLS_PER_HOUR = 4;

/**
 * The real budget. Arithmetic, against the goal of under 50,000 Edge
 * invocations per month across 30 restaurants (10% of the free plan's
 * 500,000):
 *
 *     30 restaurants x 7 clients (1 counter + up to 6 phones) x 60 calls
 *       = 12,600 invocations per 30 days
 *
 * That is 25% of the 50,000 goal and 2.5% of the free plan — WITH EVERY
 * CLIENT PINNED AT ITS CEILING, permanently. Steady state is one call per
 * device ever, so the expected figure is ~210 per month in total.
 */
export const EDGE_CALLS_PER_30_DAYS = 60;

const HOUR_MS = 60 * 60_000;
const DAY_MS = 24 * HOUR_MS;
const WINDOW_MS = 30 * DAY_MS;

/** Epoch millis of every Edge call this install has made in the last 30 days. */
let edgeCallLog: number[] = [];
let edgeLogLoaded = false;

async function loadEdgeLog(): Promise<void> {
  if (edgeLogLoaded) return;
  edgeLogLoaded = true;
  try {
    const state = await getOrderSyncState();
    edgeCallLog = state.edgeCallLog.filter((t) => Date.now() - t < WINDOW_MS);
  } catch {
    edgeCallLog = [];
  }
}

function pruneEdgeLog(): void {
  const cutoff = Date.now() - WINDOW_MS;
  edgeCallLog = edgeCallLog.filter((t) => t >= cutoff);
}

/** Plain-English usage readout for the owner (5.4). */
export interface EdgeUsage {
  lastHour: number;
  last24h: number;
  last30Days: number;
  hourlyCeiling: number;
  monthlyCeiling: number;
  throttled: boolean;
}

export function getEdgeUsage(): EdgeUsage {
  pruneEdgeLog();
  const now = Date.now();
  const lastHour = edgeCallLog.filter((t) => now - t < HOUR_MS).length;
  return {
    lastHour,
    last24h: edgeCallLog.filter((t) => now - t < DAY_MS).length,
    last30Days: edgeCallLog.length,
    hourlyCeiling: EDGE_CALLS_PER_HOUR,
    monthlyCeiling: EDGE_CALLS_PER_30_DAYS,
    throttled:
      lastHour >= EDGE_CALLS_PER_HOUR || edgeCallLog.length >= EDGE_CALLS_PER_30_DAYS,
  };
}

export class EdgeBudgetExceeded extends Error {
  constructor(readonly window: "hour" | "month") {
    super(`edge-budget-${window}`);
  }
}

/**
 * @param essential this call carries something a person just asked for.
 * It may exceed the hourly guard (a waiter must never be refused because of
 * a rate limiter) but never the 30-day budget.
 */
async function spendEdgeCall(essential: boolean): Promise<void> {
  await loadEdgeLog();
  pruneEdgeLog();
  const now = Date.now();
  if (edgeCallLog.length >= EDGE_CALLS_PER_30_DAYS) {
    console.error(
      "[orders] monthly cloud-call ceiling reached — running on cache and the " +
        "live connection only. This is a safety limit; if you see it, something " +
        "is retrying that should not be."
    );
    throw new EdgeBudgetExceeded("month");
  }
  if (!essential && edgeCallLog.filter((t) => now - t < HOUR_MS).length >= EDGE_CALLS_PER_HOUR) {
    throw new EdgeBudgetExceeded("hour");
  }
  edgeCallLog.push(now);
  void setEdgeCallLog(edgeCallLog).catch(() => {});
}

/* ------------------------------- the client ------------------------------- */

let client: SupabaseClient | null = null;

/**
 * The shared Supabase client. Sessions are held in SQLite, not localStorage.
 *
 * The custom fetch is the single most important line in this file: it gives
 * EVERY request a deadline — auth refresh, PostgREST and RPC alike — so no
 * call site can forget one and a socket that died in the machine's sleep can
 * never hold the counter open indefinitely.
 */
export function supabase(): SupabaseClient {
  if (!client) {
    client = createClient(SUPABASE_URL, SUPABASE_ANON_KEY, {
      auth: { persistSession: false, autoRefreshToken: false },
      global: { fetch: timeoutFetch(CLOUD_TIMEOUT_MS) },
    });
  }
  return client;
}

export type { OrdersSession } from "./sessionKeeper";

/**
 * The room id survives a token refresh: Supabase's refresh reply carries a
 * new access token and nothing about which restaurant this counter is. Held
 * here rather than read back off the keeper, which would make the keeper's
 * own definition circular.
 */
let roomIdHint = "";

async function loadStoredSession(): Promise<OrdersSession | null> {
  const state = await getOrderSyncState();
  if (!state.ordersRefreshToken) return null;
  roomIdHint = state.roomId;
  return {
    accessToken: state.ordersAccessToken,
    refreshToken: state.ordersRefreshToken,
    expiresAt: state.ordersTokenExpiresAt,
    roomId: state.roomId,
  };
}

async function persist(s: OrdersSession): Promise<void> {
  roomIdHint = s.roomId || roomIdHint;
  await supabase().auth.setSession({
    access_token: s.accessToken,
    refresh_token: s.refreshToken,
  });
  await supabase().realtime.setAuth(s.accessToken);
  await setOrdersSession({
    accessToken: s.accessToken,
    refreshToken: s.refreshToken,
    expiresAt: s.expiresAt,
    roomId: s.roomId,
  }).catch(() => {});
}

/**
 * Enrol this counter. THE ONLY EDGE FUNCTION CALL IN THE WHOLE FEATURE.
 * Runs once per install; after that the session refreshes itself for free.
 *
 * The raw fetch is wrapped too — it is outside the Supabase client, so the
 * client's custom fetch does not cover it, and an unbounded call here would
 * reopen exactly the hole PART D closed.
 */
async function enroll(essential: boolean): Promise<OrdersSession> {
  const key = getStoredLicenseKey();
  if (!key) throw new Error("no-license");
  const device = await getDeviceInfo();

  await spendEdgeCall(essential);

  const res = await timeoutFetch(CLOUD_TIMEOUT_MS)(`${SUPABASE_FUNCTIONS_URL}/orders-enroll`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${SUPABASE_ANON_KEY}`,
      apikey: SUPABASE_ANON_KEY,
    },
    body: JSON.stringify({
      role: "pos",
      key,
      deviceId: device.id,
      deviceLabel: device.name,
    }),
  });
  const data = await res.json();
  if (!data?.ok) throw new Error(String(data?.reason ?? `http-${res.status}`));

  return {
    accessToken: data.accessToken,
    refreshToken: data.refreshToken,
    expiresAt: Number(data.expiresAt ?? 0),
    roomId: String(data.roomId ?? ""),
  };
}

/**
 * The credential state machine lives in sessionKeeper.ts so the suite can
 * drive it against a fetch that never resolves (test DA). Everything this
 * module knows about storage, Tauri and Vite stays on this side of the line.
 */
const keeper = new SessionKeeper({
  loadStored: loadStoredSession,
  refresh: async (refreshToken): Promise<OrdersSession | null> => {
    const { data, error } = await supabase().auth.refreshSession({ refresh_token: refreshToken });
    if (error || !data?.session) return null;
    return {
      accessToken: data.session.access_token,
      refreshToken: data.session.refresh_token,
      expiresAt: Number(data.session.expires_at ?? 0),
      roomId: roomIdHint,
    };
  },
  enroll,
  persist,
});

/**
 * A valid session, refreshing or enrolling as needed. Refresh is a Supabase
 * Auth call, not an Edge Function — it costs nothing against the quota.
 *
 * REJECTS rather than hangs. Callers already treat a throw as "could not
 * reach the cloud, try later", which is the honest reading.
 */
export function ensureSession(opts: { essential?: boolean } = {}): Promise<OrdersSession> {
  return keeper.ensure(opts);
}

/** Drop the stored credential (licence moved to another machine, etc). */
export async function clearSession(): Promise<void> {
  keeper.forget();
  roomIdHint = "";
  await setOrdersSession({ accessToken: "", refreshToken: "", expiresAt: 0, roomId: "" })
    .catch(() => {});
}

export function currentRoomId(): string {
  return keeper.current()?.roomId ?? "";
}

/** True while a credential attempt is genuinely outstanding. Diagnostics only. */
export function credentialBusy(): boolean {
  return keeper.busy;
}

/* ------------------------------ call helpers ------------------------------ */

/** A Postgres RPC under the counter's credential. Unmetered. */
export async function rpc<T = any>(
  fn: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  await ensureSession();
  const { data, error } = await supabase().rpc(fn, args);
  if (error) throw new Error(error.message);
  return data as T;
}

/** A PostgREST table handle under the counter's credential. Unmetered. */
export async function table(name: string) {
  await ensureSession();
  return supabase().from(name);
}
