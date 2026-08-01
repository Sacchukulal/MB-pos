import { CLOUD_TIMEOUT_MS, SingleFlight } from "../net/timeout.ts";

/**
 * The counter's credential, and the rules for getting a valid one.
 *
 * This is the body of cloud.ts's ensureSession(), lifted out so it has no
 * Tauri, no localStorage and no Vite `import.meta.env` in it — which means
 * the test suite can construct one with a fetch that never resolves and
 * prove the deadlock that stranded the counter after a PC sleep cannot
 * happen again (test DA).
 *
 * THE BUG THIS SHAPE EXISTS TO PREVENT. The old code held its work in a
 * single shared `inFlight` promise and cleared it in a `finally`. A
 * `finally` runs only when a promise SETTLES, and a request whose socket
 * died in the machine's sleep does not settle — it hangs. So `inFlight`
 * never cleared, and because every rpc() and table() call awaits it, the
 * whole cloud side of the counter stopped, silently, including the liveness
 * beat that is the only thing writing pos_last_seen_at.
 *
 * SingleFlight keeps the property that made the shared promise worth having
 * — four concurrent callers still buy one credential, not four — and drops
 * the property that made it dangerous.
 */

export interface OrdersSession {
  accessToken: string;
  refreshToken: string;
  /** Epoch SECONDS, as Supabase reports it. */
  expiresAt: number;
  roomId: string;
}

/** Seconds of headroom before expiry at which we refresh. */
export const REFRESH_SKEW_S = 120;

export interface SessionKeeperDeps {
  /** Read the credential out of durable storage. Called at most once. */
  loadStored: () => Promise<OrdersSession | null>;
  /** Renew. Resolve null when the server REJECTED the refresh token. */
  refresh: (refreshToken: string) => Promise<OrdersSession | null>;
  /** The one Edge Function call in the whole feature. */
  enroll: (essential: boolean) => Promise<OrdersSession>;
  /** Put it on the client and in storage. */
  persist: (session: OrdersSession) => Promise<void>;
  /** Epoch MILLIS. Injectable for tests. */
  now?: () => number;
  timeoutMs?: number;
  log?: (message: string) => void;
}

export class SessionKeeper {
  private session: OrdersSession | null = null;
  private loaded = false;
  private readonly flight: SingleFlight<OrdersSession>;
  private readonly now: () => number;
  private readonly log: (message: string) => void;

  /** Counts real attempts, so a test can prove the NEXT call tried again. */
  attempts = 0;

  private readonly d: SessionKeeperDeps;

  constructor(d: SessionKeeperDeps) {
    this.d = d;
    this.flight = new SingleFlight<OrdersSession>(d.timeoutMs ?? CLOUD_TIMEOUT_MS, "credential");
    this.now = d.now ?? (() => Date.now());
    this.log = d.log ?? ((m) => console.info(m));
  }

  /** True only while an attempt is genuinely outstanding. Asserted by test DA. */
  get busy(): boolean {
    return this.flight.busy;
  }

  current(): OrdersSession | null {
    return this.session;
  }

  /** Drop the credential (licence moved to another machine, etc). */
  forget(): void {
    this.session = null;
    this.loaded = true;
    this.flight.reset();
  }

  /**
   * A valid session, refreshing or enrolling as needed. Refresh is a Supabase
   * Auth call, not an Edge Function — it costs nothing against the quota.
   *
   * Bounded. If it cannot be obtained in time it REJECTS, and leaves this
   * object in a state where the next call tries again from scratch.
   */
  ensure(opts: { essential?: boolean } = {}): Promise<OrdersSession> {
    return this.flight.run(async () => {
      this.attempts++;

      if (!this.loaded) {
        this.loaded = true;
        try {
          this.session = await this.d.loadStored();
        } catch {
          /* no DB yet — enrolment will run when there is one */
          this.session = null;
        }
      }

      const nowS = Math.floor(this.now() / 1000);
      if (this.session && this.session.expiresAt - REFRESH_SKEW_S > nowS) {
        // Still valid — make sure the client is carrying it (the first call
        // after a restart reads it out of SQLite).
        await this.d.persist(this.session);
        return this.session;
      }

      if (this.session?.refreshToken) {
        const renewed = await this.d.refresh(this.session.refreshToken);
        if (renewed) {
          this.session = renewed;
          await this.d.persist(renewed);
          return renewed;
        }
        this.log("[orders] session refresh rejected — re-enrolling");
      }

      const fresh = await this.d.enroll(opts.essential === true);
      this.session = fresh;
      await this.d.persist(fresh);
      return fresh;
    });
  }
}
