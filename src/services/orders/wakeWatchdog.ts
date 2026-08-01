/**
 * Did this machine just wake up from a sleep?
 *
 * WHY IT IS A CLOCK COMPARISON AND NOT AN EVENT. Windows, Tauri and the
 * webview all have their own opinions about suspend and resume, they differ
 * between builds, and none of them is reliable enough to be the only signal.
 * What IS reliable, on every platform, is that wall-clock time keeps running
 * while the process is frozen: a timer set for 10 seconds that comes back 5
 * minutes later means the machine was asleep in between. Nothing else
 * produces that signature.
 *
 * Platform resume events are still wired up (see orderBridge), but as an
 * EXTRA trigger, never the only one.
 *
 * WHY IT MATTERS. After a suspend, the sockets the counter holds are
 * commonly half-open: they report healthy and will never deliver another
 * message. Asking the socket how it is gets a confident wrong answer. So on
 * a detected wake the counter does not ask — it drops and rebuilds both
 * channels, beats once, reconciles once, and flushes the bill outbox.
 *
 * The clock is injectable so the suite can push time forward five minutes
 * and assert the wake path fires exactly once, and does NOT fire on a normal
 * tick (test DB).
 */

/** How often we look. Cheap: two numbers and a subtraction. */
export const WATCHDOG_TICK_MS = 10_000;

/**
 * A gap larger than this means the machine was not merely busy.
 *
 * 90 seconds is deliberately far above the tick: a loaded PC, a long
 * garbage collection or a webview throttling background timers can easily
 * stretch a 10-second tick to 30 or 60. Below ~90s we would be calling a
 * slow machine a sleeping one, and every false positive costs a full
 * channel rebuild plus a reconcile.
 */
export const WAKE_GAP_MS = 90_000;

export interface WakeWatchdogOptions {
  /** Wall clock. Injected so a test can move it. */
  now?: () => number;
  setTimer?: (fn: () => void, ms: number) => any;
  clearTimer?: (handle: any) => void;
  tickMs?: number;
  gapMs?: number;
  /** Runs once per detected wake. Never runs concurrently with itself. */
  onWake: (gapMs: number) => Promise<void> | void;
  log?: (message: string) => void;
}

export class WakeWatchdog {
  private timer: any = null;
  private lastTickAt = 0;
  private running = false;
  private handling = false;

  private readonly now: () => number;
  private readonly setTimer: (fn: () => void, ms: number) => any;
  private readonly clearTimer: (handle: any) => void;
  private readonly tickMs: number;
  private readonly gapMs: number;
  private readonly log: (message: string) => void;

  private readonly o: WakeWatchdogOptions;

  constructor(o: WakeWatchdogOptions) {
    this.o = o;
    this.now = o.now ?? (() => Date.now());
    this.setTimer = o.setTimer ?? ((fn, ms) => setInterval(fn, ms));
    this.clearTimer = o.clearTimer ?? ((h) => clearInterval(h));
    this.tickMs = o.tickMs ?? WATCHDOG_TICK_MS;
    this.gapMs = o.gapMs ?? WAKE_GAP_MS;
    this.log = o.log ?? ((m) => console.info(m));
  }

  start(): void {
    if (this.running) return;
    this.running = true;
    this.lastTickAt = this.now();
    this.timer = this.setTimer(() => void this.tick(), this.tickMs);
  }

  stop(): void {
    this.running = false;
    if (this.timer !== null) this.clearTimer(this.timer);
    this.timer = null;
  }

  /** Exposed so a test can drive it without real timers. */
  async tick(): Promise<void> {
    if (!this.running) return;
    const now = this.now();
    const elapsed = now - this.lastTickAt;
    this.lastTickAt = now;
    if (elapsed < this.gapMs) return;
    await this.fire(elapsed, "a clock jump");
  }

  /**
   * A platform resume event (Tauri window focus, webview visibility). Treated
   * as a hint: it triggers the same recovery, but the clock check above runs
   * regardless, because these events cannot be relied on.
   */
  async resumeHint(): Promise<void> {
    if (!this.running) return;
    const now = this.now();
    const elapsed = now - this.lastTickAt;
    this.lastTickAt = now;
    if (elapsed < this.gapMs) return;
    await this.fire(elapsed, "a resume event");
  }

  private async fire(gap: number, why: string): Promise<void> {
    // One recovery at a time. A resume event landing on top of a clock jump
    // must not rebuild the channels twice.
    if (this.handling) return;
    this.handling = true;
    try {
      this.log(
        `[orders] this PC appears to have been asleep for ${Math.round(gap / 1000)}s ` +
          `(detected by ${why}). Rebuilding the live connection, proving the counter ` +
          `is alive, reconciling orders and flushing unsynced bills.`,
      );
      await this.o.onWake(gap);
    } catch (e) {
      this.log(`[orders] wake recovery failed (will retry on the next tick): ${String(e)}`);
    } finally {
      this.handling = false;
    }
  }
}
