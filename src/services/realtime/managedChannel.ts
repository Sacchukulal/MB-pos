import { withTimeout } from "../net/timeout.ts";

/**
 * One managed realtime subscription, with all four structural rules.
 *
 * THIS CLASS IS SCAR TISSUE. Read `client.ts` for the incident: a single
 * transient drop once produced 31 channel joins and 31 paid calls in 65
 * seconds — ~43,000 invocations per restaurant per day, forever. The four
 * rules below are what make that impossible, whatever the cause. Do not
 * "simplify" them.
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
 * IT LIVES IN ITS OWN FILE, with its dependencies injected, so the suite can
 * construct one against a hung ensureSession and prove the fifth rule added
 * in August:
 *
 *   R5. NO FLAG MAY BECOME PERMANENT. `joining` used to be set true, then a
 *       hung `await ensureSession()` meant its `finally` never ran, so the
 *       flag stayed true forever and scheduleRejoin() returned early for the
 *       rest of the process's life. After the PC slept, that is why the
 *       counter never came back until it was restarted.
 */

export type RealtimeStatus = "connected" | "degraded";

/** R2: how long a subscription must hold before we trust it. */
const STABLE_MS = 30_000;
/** R3: more than this many cycles inside FLAP_WINDOW_MS means something is wrong. */
const FLAP_LIMIT = 3;
const FLAP_WINDOW_MS = 60_000;
const FLAP_BACKOFF_MS = 5 * 60_000;

const BACKOFF_START_MS = 1_000;
const BACKOFF_MAX_MS = 30_000;

/**
 * R5: a join that has not finished in this long is abandoned and
 * rescheduled. Deliberately longer than the 15s cloud timeout, so in normal
 * running the request's own deadline fires first and this is the last line
 * of defence rather than the usual path.
 */
export const JOIN_TIMEOUT_MS = 30_000;

export interface ManagedChannelOptions<TChannel> {
  topic: string;
  channelConfig: Record<string, unknown>;
  /** Build the channel object. Never called before the credential is current. */
  createChannel: (topic: string, config: Record<string, unknown>) => TChannel;
  removeChannel: (ch: TChannel) => Promise<unknown>;
  /** The credential must be current before the socket authenticates. */
  ensureSession: () => Promise<unknown>;
  /** Register handlers (broadcast, presence) before subscribing. */
  configure: (ch: TChannel) => void;
  /** Hand the channel to the transport and report status changes back. */
  subscribe: (ch: TChannel, onStatus: (status: string) => void) => void;
  /** Runs once per successful subscribe (presence track, etc). */
  onSubscribed: (ch: TChannel) => Promise<void>;
  onStatus: (status: RealtimeStatus) => void;
  onFlapping: () => void;
  /** R5's abandon deadline. Injectable so the suite need not wait 30 seconds. */
  joinTimeoutMs?: number;
  /** Injectable for the tests; defaults to the real clock and timers. */
  now?: () => number;
  setTimer?: (fn: () => void, ms: number) => any;
  clearTimer?: (handle: any) => void;
  log?: (message: string) => void;
}

export class ManagedChannel<TChannel> {
  private channel: TChannel | null = null;
  private backoffMs = BACKOFF_START_MS;
  private rejoinTimer: any = null;
  private stableTimer: any = null;
  private joining = false;
  private joinAt: number[] = [];
  private flagged = false;
  private stopped = true;
  /**
   * Bumped whenever the lifecycle moves on. An attempt that finishes after
   * its token has been superseded — the classic "it came back after we gave
   * up on it" — cleans up after itself and changes nothing.
   */
  private joinToken = 0;

  private readonly now: () => number;
  private readonly setTimer: (fn: () => void, ms: number) => any;
  private readonly clearTimer: (handle: any) => void;
  private readonly log: (message: string) => void;

  private readonly o: ManagedChannelOptions<TChannel>;

  constructor(o: ManagedChannelOptions<TChannel>) {
    this.o = o;
    this.now = o.now ?? (() => Date.now());
    this.setTimer = o.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
    this.clearTimer = o.clearTimer ?? ((h) => clearTimeout(h));
    this.log = o.log ?? ((m) => console.error(m));
  }

  /* ------------------------- inspection, for tests ------------------------- */

  /** R5 — this must NEVER still be true once a join has finished or failed. */
  isJoining(): boolean {
    return this.joining;
  }
  hasRejoinScheduled(): boolean {
    return this.rejoinTimer !== null;
  }
  isUp(): boolean {
    return !this.stopped && this.channel !== null;
  }
  currentBackoffMs(): number {
    return this.backoffMs;
  }

  /* ------------------------------ lifecycle ------------------------------ */

  start(): void {
    this.stopped = false;
    this.backoffMs = BACKOFF_START_MS;
    this.joinAt = [];
    this.flagged = false;
    // Any attempt still outstanding from a previous life belongs to a dead
    // token now, so releasing the flag cannot produce two live joins.
    this.joinToken++;
    this.joining = false;
    void this.join();
  }

  async stop(): Promise<void> {
    this.stopped = true;
    this.joinToken++;
    this.clearTimers();
    const previous = this.channel;
    this.channel = null;
    if (previous) {
      try {
        await this.o.removeChannel(previous);
      } catch {
        /* best effort */
      }
    }
  }

  /**
   * Force a rebuild without waiting for the socket's own opinion of itself.
   * After a machine suspend a WebSocket is commonly half-open: it reports
   * healthy and will never deliver another message. Nothing the socket says
   * can be trusted at that moment, so the wake path calls this instead of
   * asking.
   */
  async forceRejoin(): Promise<void> {
    if (this.stopped) return;
    this.joinToken++;
    this.joining = false;
    this.clearTimers();
    const previous = this.channel;
    this.channel = null;
    if (previous) {
      try {
        await this.o.removeChannel(previous);
      } catch {
        /* best effort */
      }
    }
    this.o.onStatus("degraded");
    await this.join();
  }

  private clearTimers(): void {
    if (this.rejoinTimer) this.clearTimer(this.rejoinTimer);
    if (this.stableTimer) this.clearTimer(this.stableTimer);
    this.rejoinTimer = this.stableTimer = null;
  }

  private scheduleRejoin(why: string): void {
    if (this.stopped || this.rejoinTimer || this.joining) return;
    this.o.onStatus("degraded");

    // R3 — flap detection.
    const now = this.now();
    this.joinAt = this.joinAt.filter((t) => now - t < FLAP_WINDOW_MS);
    if (this.joinAt.length > FLAP_LIMIT) {
      if (!this.flagged) {
        this.flagged = true;
        this.log(
          `[orders] the live connection is flapping (${this.joinAt.length} reconnects in a ` +
            `minute, last cause: ${why}). Backing off for 5 minutes. Orders still arrive — ` +
            `the counter reads them when the connection settles.`,
        );
        this.o.onFlapping();
      }
      this.backoffMs = FLAP_BACKOFF_MS;
    }

    const delay = this.backoffMs;
    this.rejoinTimer = this.setTimer(() => {
      this.rejoinTimer = null;
      this.backoffMs = Math.min(this.backoffMs * 2, BACKOFF_MAX_MS);
      void this.join();
    }, delay);
  }

  private async join(): Promise<void> {
    if (this.stopped || this.joining) return;
    this.joining = true;
    const token = ++this.joinToken;
    let failure: unknown = null;
    try {
      // R5 — the whole attempt is bounded. Before August this awaited
      // ensureSession() directly, and a request hung by a machine suspend
      // meant control never reached the `finally` below.
      await withTimeout(
        this.attemptJoin(token),
        this.o.joinTimeoutMs ?? JOIN_TIMEOUT_MS,
        `join ${this.o.topic}`,
      );
    } catch (e) {
      failure = e;
    } finally {
      // ON EVERY PATH. This is the whole point of the file.
      this.joining = false;
    }

    if (this.stopped || token !== this.joinToken) return;
    if (failure !== null || this.channel === null) {
      const why = failure instanceof Error ? failure.message : "join-failed";
      this.scheduleRejoin(why);
    }
  }

  private async attemptJoin(token: number): Promise<void> {
    // R1 — the previous channel is fully gone before a new one exists, and
    // its dying CLOSED can no longer be mistaken for a fresh fault.
    if (this.channel) {
      const previous = this.channel;
      this.channel = null;
      try {
        await this.o.removeChannel(previous);
      } catch {
        /* already gone */
      }
    }

    // The credential must be current before the socket authenticates, or
    // the server closes the join on a private topic.
    await this.o.ensureSession();
    if (this.stopped || token !== this.joinToken) return;

    const ch = this.o.createChannel(this.o.topic, { private: true, ...this.o.channelConfig });
    this.channel = ch;
    this.joinAt.push(this.now());
    this.o.configure(ch);

    this.o.subscribe(ch, (status) => {
      // R1 — a status event from a channel we have already replaced says
      // nothing about the connection we currently care about.
      if (this.stopped || ch !== this.channel) return;

      if (status === "SUBSCRIBED") {
        this.o.onStatus("connected");
        // R2 — the backoff resets only once this subscription has HELD.
        if (this.stableTimer) this.clearTimer(this.stableTimer);
        this.stableTimer = this.setTimer(() => {
          this.stableTimer = null;
          this.backoffMs = BACKOFF_START_MS;
          this.joinAt = [];
          this.flagged = false;
        }, STABLE_MS);
        void (async () => {
          try {
            await this.o.onSubscribed(ch);
          } catch (e) {
            this.log(`[orders] channel setup failed: ${String((e as Error)?.message ?? e)}`);
          }
        })();
      } else if (status === "CHANNEL_ERROR" || status === "TIMED_OUT" || status === "CLOSED") {
        if (this.stableTimer) {
          this.clearTimer(this.stableTimer);
          this.stableTimer = null;
        }
        this.scheduleRejoin(status);
      }
    });
  }
}
