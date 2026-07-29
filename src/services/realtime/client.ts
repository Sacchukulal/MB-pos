import type { RealtimeChannel } from "@supabase/supabase-js";
import { ensureSession, supabase } from "../orders/cloud";

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
 * old code treated as a fault and answered with another rejoin. Every
 * deliberate rejoin scheduled the next one. `backoffMs` was reset on every
 * SUBSCRIBED, so the backoff could never grow past the first step and the
 * loop pinned at exactly 2 seconds.
 *
 * Four structural rules make that impossible here, whatever the cause:
 *
 *   R1. Channel removal is AWAITED before a new join, and a status event
 *       from a channel we have already replaced is ignored by identity.
 *   R2. The backoff resets only after a subscription has been STABLE for
 *       30 seconds — never on the SUBSCRIBED event itself.
 *   R3. A flap detector: more than 3 subscribe/drop cycles in 60 seconds
 *       drops to a long backoff, logs ONE clear error, and reports an
 *       honest fault to the UI.
 *   R4. A reconnect never triggers a data fetch by itself.
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

export type RealtimeStatus = "connected" | "degraded";

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

/** R2: how long a subscription must hold before we trust it. */
const STABLE_MS = 30_000;
/** R3: more than this many cycles inside FLAP_WINDOW_MS means something is wrong. */
const FLAP_LIMIT = 3;
const FLAP_WINDOW_MS = 60_000;
const FLAP_BACKOFF_MS = 5 * 60_000;

const BACKOFF_START_MS = 1_000;
const BACKOFF_MAX_MS = 30_000;

/**
 * One managed subscription. All four structural rules live here, so both
 * topics get them and neither can regress independently.
 */
class ManagedChannel {
  private channel: RealtimeChannel | null = null;
  private backoffMs = BACKOFF_START_MS;
  private rejoinTimer: ReturnType<typeof setTimeout> | null = null;
  private stableTimer: ReturnType<typeof setTimeout> | null = null;
  private joining = false;
  private joinAt: number[] = [];
  private flagged = false;
  private stopped = true;

  constructor(
    private readonly topic: string,
    private readonly channelConfig: Record<string, unknown>,
    private readonly configure: (ch: RealtimeChannel) => void,
    private readonly onSubscribed: (ch: RealtimeChannel) => Promise<void>,
    private readonly onStatus: (status: RealtimeStatus) => void,
    private readonly onFlapping: () => void,
  ) {}

  start(): void {
    this.stopped = false;
    this.backoffMs = BACKOFF_START_MS;
    this.joinAt = [];
    this.flagged = false;
    void this.join();
  }

  async stop(): Promise<void> {
    this.stopped = true;
    this.clearTimers();
    const previous = this.channel;
    this.channel = null;
    if (previous) {
      try {
        await supabase().removeChannel(previous);
      } catch {
        /* best effort */
      }
    }
  }

  isUp(): boolean {
    return !this.stopped && this.channel !== null;
  }

  private clearTimers(): void {
    if (this.rejoinTimer) clearTimeout(this.rejoinTimer);
    if (this.stableTimer) clearTimeout(this.stableTimer);
    this.rejoinTimer = this.stableTimer = null;
  }

  private scheduleRejoin(why: string): void {
    if (this.stopped || this.rejoinTimer || this.joining) return;
    this.onStatus("degraded");

    // R3 — flap detection.
    const now = Date.now();
    this.joinAt = this.joinAt.filter((t) => now - t < FLAP_WINDOW_MS);
    if (this.joinAt.length > FLAP_LIMIT) {
      if (!this.flagged) {
        this.flagged = true;
        console.error(
          `[orders] the live connection is flapping (${this.joinAt.length} reconnects in a ` +
            `minute, last cause: ${why}). Backing off for 5 minutes. Orders still arrive — ` +
            `the counter reads them when the connection settles.`,
        );
        this.onFlapping();
      }
      this.backoffMs = FLAP_BACKOFF_MS;
    }

    const delay = this.backoffMs;
    this.rejoinTimer = setTimeout(() => {
      this.rejoinTimer = null;
      this.backoffMs = Math.min(this.backoffMs * 2, BACKOFF_MAX_MS);
      void this.join();
    }, delay);
  }

  private async join(): Promise<void> {
    if (this.stopped || this.joining) return;
    this.joining = true;
    try {
      // R1 — the previous channel is fully gone before a new one exists, and
      // its dying CLOSED can no longer be mistaken for a fresh fault.
      if (this.channel) {
        const previous = this.channel;
        this.channel = null;
        try {
          await supabase().removeChannel(previous);
        } catch {
          /* already gone */
        }
      }

      // The credential must be current before the socket authenticates, or
      // the server closes the join on a private topic.
      await ensureSession();
      if (this.stopped) return;

      const ch = supabase().channel(this.topic, {
        config: { private: true, ...this.channelConfig },
      });
      this.channel = ch;
      this.joinAt.push(Date.now());
      this.configure(ch);

      ch.subscribe(async (status) => {
        // R1 — a status event from a channel we have already replaced says
        // nothing about the connection we currently care about.
        if (this.stopped || ch !== this.channel) return;

        if (status === "SUBSCRIBED") {
          this.onStatus("connected");
          // R2 — the backoff resets only once this subscription has HELD.
          if (this.stableTimer) clearTimeout(this.stableTimer);
          this.stableTimer = setTimeout(() => {
            this.stableTimer = null;
            this.backoffMs = BACKOFF_START_MS;
            this.joinAt = [];
            this.flagged = false;
          }, STABLE_MS);
          try {
            await this.onSubscribed(ch);
          } catch (e) {
            console.error("[orders] channel setup failed:", e);
          }
        } else if (status === "CHANNEL_ERROR" || status === "TIMED_OUT" || status === "CLOSED") {
          if (this.stableTimer) {
            clearTimeout(this.stableTimer);
            this.stableTimer = null;
          }
          this.scheduleRejoin(status);
        }
      });
    } finally {
      this.joining = false;
    }
  }
}

let presence: ManagedChannel | null = null;
let intents: ManagedChannel | null = null;
/** Both must be up before the UI says "connected". */
const up = { presence: false, intents: false };

export function connectRealtime(options: ConnectOptions): void {
  void disconnectRealtime().then(async () => {
    const { roomId, deviceId, deviceName, appVersion, callbacks } = options;

    const report = () => {
      callbacks.onStatusChange(up.presence && up.intents ? "connected" : "degraded");
    };

    presence = new ManagedChannel(
      `orders-${roomId}`,
      { presence: { key: `pos:${deviceId}` } },
      (ch) => {
        ch.on("presence", { event: "sync" }, () => {
          const state = ch.presenceState();
          callbacks.onPhonesChange(
            Object.keys(state).filter((k) => k.startsWith("mob:")).length,
          );
        });
      },
      async (ch) => {
        await ch.track({
          kind: "pos",
          name: deviceName,
          version: appVersion,
          at: Date.now(),
        });
      },
      (s) => {
        up.presence = s === "connected";
        if (s === "degraded") callbacks.onPhonesChange(0);
        report();
      },
      callbacks.onFlapping,
    );

    intents = new ManagedChannel(
      `orders-${roomId}-pos`,
      {},
      (ch) => {
        ch.on("broadcast", { event: "mb" }, (msg) => {
          const p = (msg as { payload?: Bell }).payload;
          if (p && typeof p.kind === "string") callbacks.onBell(p);
        });
      },
      async () => {},
      (s) => {
        up.intents = s === "connected";
        report();
      },
      callbacks.onFlapping,
    );

    presence.start();
    intents.start();
  });
}

export async function disconnectRealtime(): Promise<void> {
  up.presence = false;
  up.intents = false;
  const previous = [presence, intents];
  presence = intents = null;
  for (const c of previous) if (c) await c.stop();
}
