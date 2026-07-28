import { createClient, type SupabaseClient } from "@supabase/supabase-js";
import { SUPABASE_ANON_KEY, SUPABASE_FUNCTIONS_URL, SUPABASE_URL } from "../../config/supabase";
import { getStoredLicenseKey } from "../license/licenseService";
import { getDeviceInfo } from "../license/device";
import {
  getOrderSyncState,
  setEdgeCallLog,
  setOrdersSession,
} from "../../db/repositories/orderSyncRepo";

/**
 * The ONE seam between the counter and the cloud (decision D10). Everything
 * above this file talks in terms of orders and events; only this file knows
 * that the transport happens to be Supabase. A future LAN server replaces
 * this module and nothing else.
 *
 * Two rules define the whole design:
 *
 *  1. EDGE FUNCTION INVOCATIONS ARE THE ONLY METERED CALL. Exactly one
 *     Edge Function is ever called from here — `orders-enroll`, which mints
 *     this counter's credential. It runs once per install, and again only
 *     if the refresh token is lost or rejected. Everything else is
 *     PostgREST (reads, writes, RPCs), which is not metered by count.
 *
 *  2. A HARD CEILING NO BUG CAN BREACH (P5). Every Edge call is recorded in
 *     a rolling log that survives restarts, and the ceilings below are
 *     enforced before the call is made, not after.
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

/** The shared Supabase client. Sessions are held in SQLite, not localStorage. */
export function supabase(): SupabaseClient {
  if (!client) {
    client = createClient(SUPABASE_URL, SUPABASE_ANON_KEY, {
      auth: { persistSession: false, autoRefreshToken: false },
    });
  }
  return client;
}

export interface OrdersSession {
  accessToken: string;
  refreshToken: string;
  /** Epoch seconds. */
  expiresAt: number;
  roomId: string;
}

let session: OrdersSession | null = null;
let sessionLoaded = false;
let inFlight: Promise<OrdersSession> | null = null;

/** Seconds of headroom before expiry at which we refresh. */
const REFRESH_SKEW_S = 120;

async function loadStoredSession(): Promise<void> {
  if (sessionLoaded) return;
  sessionLoaded = true;
  try {
    const state = await getOrderSyncState();
    if (state.ordersRefreshToken) {
      session = {
        accessToken: state.ordersAccessToken,
        refreshToken: state.ordersRefreshToken,
        expiresAt: state.ordersTokenExpiresAt,
        roomId: state.roomId,
      };
    }
  } catch {
    /* no DB yet — enrolment will run when there is one */
  }
}

async function persist(s: OrdersSession): Promise<void> {
  session = s;
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
 */
async function enroll(essential: boolean): Promise<OrdersSession> {
  const key = getStoredLicenseKey();
  if (!key) throw new Error("no-license");
  const device = await getDeviceInfo();

  await spendEdgeCall(essential);

  const res = await fetch(`${SUPABASE_FUNCTIONS_URL}/orders-enroll`, {
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

  const next: OrdersSession = {
    accessToken: data.accessToken,
    refreshToken: data.refreshToken,
    expiresAt: Number(data.expiresAt ?? 0),
    roomId: String(data.roomId ?? ""),
  };
  await persist(next);
  return next;
}

/**
 * A valid session, refreshing or enrolling as needed. Refresh is a Supabase
 * Auth call, not an Edge Function — it costs nothing against the quota.
 */
export async function ensureSession(opts: { essential?: boolean } = {}): Promise<OrdersSession> {
  if (inFlight) return inFlight;
  inFlight = (async () => {
    await loadStoredSession();
    const nowS = Math.floor(Date.now() / 1000);

    if (session && session.expiresAt - REFRESH_SKEW_S > nowS) {
      // Still valid — make sure the client is carrying it (first call after
      // a restart reads it out of SQLite).
      await persist(session);
      return session;
    }

    if (session?.refreshToken) {
      const { data, error } = await supabase().auth.refreshSession({
        refresh_token: session.refreshToken,
      });
      if (!error && data?.session) {
        const next: OrdersSession = {
          accessToken: data.session.access_token,
          refreshToken: data.session.refresh_token,
          expiresAt: Number(data.session.expires_at ?? 0),
          roomId: session.roomId,
        };
        await persist(next);
        return next;
      }
      console.info("[orders] session refresh rejected — re-enrolling");
    }

    return await enroll(opts.essential === true);
  })();
  try {
    return await inFlight;
  } finally {
    inFlight = null;
  }
}

/** Drop the stored credential (licence moved to another machine, etc). */
export async function clearSession(): Promise<void> {
  session = null;
  sessionLoaded = true;
  await setOrdersSession({ accessToken: "", refreshToken: "", expiresAt: 0, roomId: "" })
    .catch(() => {});
}

export function currentRoomId(): string {
  return session?.roomId ?? "";
}

export function currentAccessToken(): string {
  return session?.accessToken ?? "";
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
