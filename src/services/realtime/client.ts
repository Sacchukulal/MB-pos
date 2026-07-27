import {
  createClient,
  type RealtimeChannel,
  type SupabaseClient,
} from "@supabase/supabase-js";
import { SUPABASE_ANON_KEY, SUPABASE_URL } from "../../config/supabase";

/**
 * Thin wrapper over the Supabase realtime channel for the orders doorbell
 * (ORDERS_CONTRACT.md §5). Presence + broadcast only — no Postgres-changes
 * subscriptions, and never any order data on the wire.
 *
 * Reconnects with exponential backoff (1s, 2s, 4s … 30s cap). While the
 * channel is down, `onStatusChange("degraded")` tells the bridge to fall
 * back to polling; "connected" turns polling back off.
 */

export interface RealtimeCallbacks {
  /** A doorbell ping: { kind: 'orders'|'catalog'|'events', seq } */
  onDoorbell: (payload: { kind: string; seq: number }) => void;
  /** Number of phones (mob:*) currently in the room. */
  onPhonesChange: (phones: number) => void;
  onStatusChange: (status: "connected" | "degraded") => void;
}

interface ConnectOptions {
  roomId: string;
  deviceId: string;
  deviceName: string;
  appVersion: string;
  callbacks: RealtimeCallbacks;
}

let client: SupabaseClient | null = null;
let channel: RealtimeChannel | null = null;
let opts: ConnectOptions | null = null;
let backoffMs = 1_000;
let rejoinTimer: ReturnType<typeof setTimeout> | null = null;
let stopped = true;

function clearRejoinTimer(): void {
  if (rejoinTimer) {
    clearTimeout(rejoinTimer);
    rejoinTimer = null;
  }
}

function scheduleRejoin(): void {
  if (stopped || rejoinTimer) return;
  opts?.callbacks.onStatusChange("degraded");
  rejoinTimer = setTimeout(() => {
    rejoinTimer = null;
    backoffMs = Math.min(backoffMs * 2, 30_000);
    join();
  }, backoffMs);
}

function join(): void {
  if (stopped || !client || !opts) return;

  if (channel) {
    try {
      client.removeChannel(channel);
    } catch {
      /* already gone */
    }
    channel = null;
  }

  const { roomId, deviceId, deviceName, appVersion, callbacks } = opts;
  const ch = client.channel(`orders-${roomId}`, {
    config: { presence: { key: `pos:${deviceId}` } },
  });
  channel = ch;

  ch.on("broadcast", { event: "mb" }, (msg) => {
    const p = (msg as { payload?: { kind?: string; seq?: number } }).payload;
    if (p && typeof p.kind === "string") {
      callbacks.onDoorbell({ kind: p.kind, seq: Number(p.seq ?? 0) });
    }
  });

  ch.on("presence", { event: "sync" }, () => {
    const state = ch.presenceState();
    const phones = Object.keys(state).filter((k) => k.startsWith("mob:")).length;
    callbacks.onPhonesChange(phones);
  });

  ch.subscribe(async (status) => {
    if (stopped) return;
    if (status === "SUBSCRIBED") {
      backoffMs = 1_000;
      callbacks.onStatusChange("connected");
      try {
        await ch.track({ kind: "pos", name: deviceName, version: appVersion, at: Date.now() });
      } catch (e) {
        console.error("[orders] presence track failed:", e);
      }
    } else if (status === "CHANNEL_ERROR" || status === "TIMED_OUT" || status === "CLOSED") {
      scheduleRejoin();
    }
  });
}

export function connectRealtime(options: ConnectOptions): void {
  disconnectRealtime();
  stopped = false;
  opts = options;
  backoffMs = 1_000;
  client = createClient(SUPABASE_URL, SUPABASE_ANON_KEY, {
    auth: { persistSession: false, autoRefreshToken: false },
  });
  join();
}

export function disconnectRealtime(): void {
  stopped = true;
  clearRejoinTimer();
  if (client) {
    try {
      if (channel) client.removeChannel(channel);
      client.realtime.disconnect();
    } catch {
      /* best effort */
    }
  }
  channel = null;
  client = null;
  opts = null;
}
