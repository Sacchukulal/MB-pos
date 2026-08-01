import type { RealtimeChannel } from "@supabase/supabase-js";
import { ensureSession, supabase } from "../orders/cloud";
import { ManagedChannel, type RealtimeStatus } from "./managedChannel";

/**
 * The counter's end of the private orders channels.
 *
 * WHY THIS FILE WAS REWRITTEN. The previous version cost ~43,000 Edge
 * Function invocations per restaurant per day, forever, after any single
 * transient drop. Reproduced and measured: one injected drop produced 31
 * channel joins and 31 `pull_events` calls in 65 seconds (41,206/day).
 *
 * The mechanism, from @supabase/realtime-js: `subscribe()` registers
 * `_onClose(() => callback(CLOSED))`. So calling `removeChannel(old)` inside
 * a rejoin makes the REMOVED channel's own callback fire CLOSED, which the
 * old code treated as a fault and answered with another reconnect. Every
 * deliberate rejoin scheduled the next one. `backoffMs` was reset on every
 * SUBSCRIBED, so the backoff could never grow past the first step and the
 * loop pinned at exactly 2 seconds.
 *
 * The five structural rules that make that impossible live in
 * ./managedChannel.ts, in their own file so the suite can drive them
 * directly. Read the header there before changing anything here.
 *
 * TWO TOPICS, one socket:
 *   orders-<room>       presence only — how the phones know the counter is
 *                       up and how the counter counts phones. Nothing is
 *                       ever broadcast here.
 *   orders-<room>-pos   phone intents. Exactly one subscriber: this counter.
 * The counter deliberately does NOT subscribe to orders-<room>-live: it
 * authors every message on it, and receiving them back cost a third of the
 * project's realtime budget.
 */

export type { RealtimeStatus } from "./managedChannel";

export interface Bell {
  kind: "order" | "event" | "catalog";
  seq?: number;
  order?: Record<string, unknown>;
  event?: Record<string, unknown>;
}

export interface RealtimeCallbacks {
  /** A bell carrying a phone's intent. */
  onBell: (bell: Bell) => void;
  /** Number of phones (mob:*) currently in the room. */
  onPhonesChange: (phones: number) => void;
  onStatusChange: (status: RealtimeStatus) => void;
  /** Raised once when a channel is flapping; the UI says so honestly. */
  onFlapping: () => void;
}

interface ConnectOptions {
  roomId: string;
  deviceId: string;
  deviceName: string;
  appVersion: string;
  callbacks: RealtimeCallbacks;
}

/**
 * How long the phone count holds its last non-zero reading when the channel
 * reports degraded.
 *
 * WHY. `phones` used to be slammed to 0 the instant the presence channel
 * blinked, so the owner's settings screen flickered between "1 phone" and
 * "0 phones" for a connection that recovered a second later. Same idea as
 * DEGRADED_UI_DEBOUNCE_MS in orderBridge: entering a bad state is debounced,
 * recovery is instant.
 */
const PHONES_ZERO_DEBOUNCE_MS = 10_000;

let presence: ManagedChannel<RealtimeChannel> | null = null;
let intents: ManagedChannel<RealtimeChannel> | null = null;
/** Both must be up before the UI says "connected". */
const up = { presence: false, intents: false };

let phonesZeroTimer: ReturnType<typeof setTimeout> | null = null;
let lastPhones = 0;

function clearPhonesTimer(): void {
  if (phonesZeroTimer) {
    clearTimeout(phonesZeroTimer);
    phonesZeroTimer = null;
  }
}

export function connectRealtime(options: ConnectOptions): void {
  void disconnectRealtime().then(async () => {
    const { roomId, deviceId, deviceName, appVersion, callbacks } = options;

    const report = () => {
      callbacks.onStatusChange(up.presence && up.intents ? "connected" : "degraded");
    };

    /** A real reading. Cancels any pending "drop to zero". */
    const reportPhones = (n: number) => {
      clearPhonesTimer();
      lastPhones = n;
      callbacks.onPhonesChange(n);
    };

    presence = new ManagedChannel<RealtimeChannel>({
      topic: `orders-${roomId}`,
      channelConfig: { presence: { key: `pos:${deviceId}` } },
      createChannel: (topic, config) => supabase().channel(topic, { config }),
      removeChannel: (ch) => supabase().removeChannel(ch),
      ensureSession: () => ensureSession(),
      configure: (ch) => {
        ch.on("presence", { event: "sync" }, () => {
          const state = ch.presenceState();
          reportPhones(Object.keys(state).filter((k) => k.startsWith("mob:")).length);
        });
      },
      subscribe: (ch, onStatus) => {
        ch.subscribe((status) => onStatus(status));
      },
      onSubscribed: async (ch) => {
        await ch.track({
          kind: "pos",
          name: deviceName,
          version: appVersion,
          at: Date.now(),
        });
      },
      onStatus: (s) => {
        up.presence = s === "connected";
        if (s === "degraded") {
          // A blink is not an empty restaurant. Hold the last real reading
          // for ten seconds; if the channel is genuinely gone by then, say
          // zero. Recovery cancels it.
          if (lastPhones > 0 && !phonesZeroTimer) {
            phonesZeroTimer = setTimeout(() => {
              phonesZeroTimer = null;
              lastPhones = 0;
              callbacks.onPhonesChange(0);
            }, PHONES_ZERO_DEBOUNCE_MS);
          }
        }
        report();
      },
      onFlapping: callbacks.onFlapping,
    });

    intents = new ManagedChannel<RealtimeChannel>({
      topic: `orders-${roomId}-pos`,
      channelConfig: {},
      createChannel: (topic, config) => supabase().channel(topic, { config }),
      removeChannel: (ch) => supabase().removeChannel(ch),
      ensureSession: () => ensureSession(),
      configure: (ch) => {
        ch.on("broadcast", { event: "mb" }, (msg) => {
          const p = (msg as { payload?: Bell }).payload;
          if (p && typeof p.kind === "string") callbacks.onBell(p);
        });
      },
      subscribe: (ch, onStatus) => {
        ch.subscribe((status) => onStatus(status));
      },
      onSubscribed: async () => {},
      onStatus: (s) => {
        up.intents = s === "connected";
        report();
      },
      onFlapping: callbacks.onFlapping,
    });

    presence.start();
    intents.start();
  });
}

/**
 * PART D — the wake path. After a machine suspend a WebSocket is commonly
 * half-open: it reports healthy and will never deliver another message. So
 * this does NOT ask the socket how it is; it drops both channels and
 * rebuilds them unconditionally.
 */
export async function forceRejoinRealtime(): Promise<void> {
  const both = [presence, intents];
  for (const c of both) if (c) await c.forceRejoin();
}

export async function disconnectRealtime(): Promise<void> {
  up.presence = false;
  up.intents = false;
  clearPhonesTimer();
  lastPhones = 0;
  const previous = [presence, intents];
  presence = intents = null;
  for (const c of previous) if (c) await c.stop();
}
