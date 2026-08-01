/**
 * Nothing this counter sends to the cloud may take forever.
 *
 * WHY THIS FILE EXISTS. When the billing PC sleeps, the TCP connections it
 * holds die silently — no RST, no error, just a socket that will never
 * answer. On wake, a request that was in flight does not fail; it hangs.
 * That is the difference between "the counter reconnects" and the bug the
 * owner reported: after a sleep the phone showed the counter offline and
 * only restarting the POS process fixed it.
 *
 * The chain was:
 *   1. the Supabase client was created with no custom fetch, so no request
 *      had a deadline;
 *   2. ensureSession() held its work in one shared `inFlight` promise and
 *      cleared it in a `finally` — and a `finally` runs only when a promise
 *      SETTLES. A hung request meant `inFlight` never cleared;
 *   3. every rpc() and table() call awaits ensureSession(), so the entire
 *      cloud side of the counter deadlocked behind it — including the
 *      liveness beat, which is the only thing writing pos_last_seen_at;
 *   4. the realtime rejoin was caught in the same trap: `joining = true`,
 *      await a hung ensureSession(), and the flag never cleared, so
 *      scheduleRejoin() returned early forever.
 *
 * The phone was reading a truth the counter had stopped writing.
 *
 * THE RULE THIS FILE ENFORCES: no promise and no flag in the cloud path may
 * become permanent. Everything here is pure — no Tauri, no DOM beyond fetch
 * and AbortSignal — so it is exercised directly by the test suite.
 */

/**
 * 15 seconds. Long enough for a desktop on a bad restaurant connection to
 * finish a real request, short enough that a waiter's order is not left
 * hanging while the counter waits on a socket that died in its sleep.
 */
export const CLOUD_TIMEOUT_MS = 15_000;

/**
 * Written without TypeScript parameter properties on purpose: the POS test
 * suite runs on `node --test` with Node's strip-only type removal, which
 * cannot desugar them. Everything under test in PART D is plain fields.
 */
export class TimeoutError extends Error {
  readonly timeout = true;
  readonly label: string;
  readonly ms: number;
  constructor(label: string, ms: number) {
    super(`${label} timed out after ${ms}ms`);
    this.name = "TimeoutError";
    this.label = label;
    this.ms = ms;
  }
}

export const isTimeout = (e: unknown): boolean =>
  e instanceof TimeoutError ||
  (e instanceof Error && (e.name === "TimeoutError" || e.name === "AbortError"));

/** AbortSignal.any where it exists, with a hand-rolled fallback where it does not. */
function combineSignals(signals: AbortSignal[]): AbortSignal {
  const any = (AbortSignal as unknown as { any?: (s: AbortSignal[]) => AbortSignal }).any;
  if (typeof any === "function") return any.call(AbortSignal, signals);
  const controller = new AbortController();
  for (const s of signals) {
    if (s.aborted) {
      controller.abort(s.reason);
      break;
    }
    s.addEventListener("abort", () => controller.abort(s.reason), { once: true });
  }
  return controller.signal;
}

/**
 * A fetch with a deadline. Given to the Supabase client, it covers auth
 * refresh, PostgREST and RPC alike — one place, no call site to forget.
 *
 * A caller's own signal is preserved: supabase-js passes one for its own
 * cancellation, and dropping it would break that.
 */
export function timeoutFetch(
  timeoutMs: number = CLOUD_TIMEOUT_MS,
  base: typeof fetch = fetch,
): typeof fetch {
  return (input: any, init?: any) => {
    const deadline = AbortSignal.timeout(timeoutMs);
    const signal = init?.signal ? combineSignals([init.signal, deadline]) : deadline;
    return base(input, { ...(init ?? {}), signal });
  };
}

/**
 * Bounds any promise. The underlying work may still be hung forever — we
 * simply stop waiting for it, and make sure its eventual rejection (if it
 * ever arrives) does not surface as an unhandled one.
 */
export function withTimeout<T>(work: Promise<T>, ms: number, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  work.catch(() => {}); // the abandoned branch must not become noise later
  const deadline = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new TimeoutError(label, ms)), ms);
  });
  return Promise.race([work, deadline]).finally(() => {
    if (timer !== undefined) clearTimeout(timer);
  });
}

/**
 * One shared attempt that CANNOT become permanent.
 *
 * This replaces the `inFlight` promise in cloud.ts. Concurrent callers still
 * share one attempt — that property is why it existed, and losing it would
 * mean four callers buying four credentials. What changes is that the
 * attempt is bounded and the slot is cleared on BOTH the success and the
 * failure path, so a timed-out attempt always leaves the next caller free to
 * try again rather than joining a queue behind a corpse.
 */
export class SingleFlight<T> {
  private current: Promise<T> | null = null;
  private readonly timeoutMs: number;
  private readonly label: string;

  constructor(timeoutMs: number = CLOUD_TIMEOUT_MS, label = "request") {
    this.timeoutMs = timeoutMs;
    this.label = label;
  }

  /** True while an attempt is genuinely outstanding. Asserted by the tests. */
  get busy(): boolean {
    return this.current !== null;
  }

  run(factory: () => Promise<T>): Promise<T> {
    if (this.current) return this.current;

    const bounded = withTimeout(factory(), this.timeoutMs, this.label);
    // The slot is released before the caller sees the outcome, on both
    // paths. `settled === this.current` guards against a late timeout from
    // an older attempt clearing a newer one.
    const settled: Promise<T> = bounded.then(
      (value) => {
        if (this.current === settled) this.current = null;
        return value;
      },
      (error) => {
        if (this.current === settled) this.current = null;
        throw error;
      },
    );
    this.current = settled;
    return settled;
  }

  /** Abandon the current attempt without waiting for it. */
  reset(): void {
    this.current = null;
  }
}
