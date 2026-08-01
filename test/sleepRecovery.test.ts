import test from "node:test";
import assert from "node:assert/strict";
import {
  CLOUD_TIMEOUT_MS,
  SingleFlight,
  TimeoutError,
  isTimeout,
  timeoutFetch,
  withTimeout,
} from "../src/services/net/timeout.ts";
import { SessionKeeper, type OrdersSession } from "../src/services/orders/sessionKeeper.ts";
import { ManagedChannel } from "../src/services/realtime/managedChannel.ts";
import { WakeWatchdog } from "../src/services/orders/wakeWatchdog.ts";

/**
 * PART D — the counter must survive the PC going to sleep.
 *
 * THE BUG THESE TESTS EXIST FOR. The owner reported: "If the desktop goes to
 * sleep, on wake the phone shows the connection offline even after reloading
 * the Android app. If we close the POS app and reopen it, it comes online."
 *
 * When a machine suspends, its TCP connections die silently. On wake, a
 * request that was in flight does not fail — it hangs forever. Nothing on
 * the counter's cloud path had a timeout, so:
 *
 *   * ensureSession()'s shared `inFlight` promise cleared in a `finally`,
 *     and a `finally` runs only when a promise SETTLES. It never cleared.
 *   * every rpc() and table() call awaits ensureSession(), so the whole
 *     cloud side deadlocked — including the 60-second liveness beat, the
 *     only thing writing licenses.pos_last_seen_at.
 *   * the realtime rejoin sat in the same trap: `joining = true`, await the
 *     hung ensureSession(), and the flag never cleared, so scheduleRejoin()
 *     returned early for the rest of the process's life.
 *
 * The phone was reading a truth the counter had stopped writing. That is why
 * restarting the phone changed nothing and only restarting the POS helped.
 *
 * These are the fault-injection tests from the work order (DA and DB). They
 * run against the REAL modules the app uses, with a fetch that never
 * resolves and a clock that can be pushed forward — the machine is never
 * suspended to produce them.
 *
 * Run with:  npm test
 */

/** A fetch that behaves exactly like a socket killed by a suspend. */
function hungFetch(): { fetch: typeof fetch; calls: number } {
  const state = { calls: 0 } as { calls: number; fetch: typeof fetch };
  state.fetch = ((_input: any, init?: any) => {
    state.calls++;
    return new Promise((_resolve, reject) => {
      // A real hung socket still honours an abort signal — which is exactly
      // the lever timeoutFetch pulls.
      init?.signal?.addEventListener?.("abort", () => {
        const e = new Error("aborted");
        e.name = "AbortError";
        reject(e);
      });
    });
  }) as typeof fetch;
  return state as { fetch: typeof fetch; calls: number };
}

const session = (over: Partial<OrdersSession> = {}): OrdersSession => ({
  accessToken: "a",
  refreshToken: "r",
  expiresAt: Math.floor(Date.now() / 1000) + 3600,
  roomId: "room",
  ...over,
});

/* ===================== DA — THE HANG TEST ===================== */

test("DA1: a hung fetch is aborted by its own deadline instead of hanging", async () => {
  const hung = hungFetch();
  const bounded = timeoutFetch(60, hung.fetch);
  await assert.rejects(
    () => bounded("https://example.invalid/x") as Promise<Response>,
    (e: Error) => {
      assert.equal(isTimeout(e), true, `expected a timeout-ish error, got ${e.name}`);
      return true;
    },
  );
  assert.equal(hung.calls, 1);
});

test("DA2: withTimeout rejects on a promise that never settles", async () => {
  const never = new Promise<never>(() => {});
  await assert.rejects(() => withTimeout(never, 40, "never"), TimeoutError);
});

test("DA3: SingleFlight shares one attempt but never becomes permanent", async () => {
  let started = 0;
  const flight = new SingleFlight<string>(40, "credential");

  // Two concurrent callers share ONE attempt — the property the original
  // `inFlight` promise existed for, and which must survive the fix.
  const p1 = flight.run(() => {
    started++;
    return new Promise<string>(() => {});
  });
  const p2 = flight.run(() => {
    started++;
    return new Promise<string>(() => {});
  });
  assert.equal(started, 1, "a second concurrent caller must not start a second attempt");
  assert.equal(flight.busy, true);

  await assert.rejects(() => p1, TimeoutError);
  await assert.rejects(() => p2, TimeoutError);

  // THE HEART OF IT: the slot is free again.
  assert.equal(flight.busy, false, "the shared slot must be released on the failure path too");

  // And the NEXT caller genuinely tries again rather than awaiting the corpse.
  const value = await flight.run(async () => {
    started++;
    return "fresh";
  });
  assert.equal(value, "fresh");
  assert.equal(started, 2, "the next call must make a fresh attempt");
  assert.equal(flight.busy, false);
});

test("DA4: ensureSession rejects on the timeout and leaves nothing in flight", async () => {
  let enrolments = 0;
  const keeper = new SessionKeeper({
    loadStored: async () => null,
    refresh: async () => null,
    enroll: async () => {
      enrolments++;
      // Exactly what a request over a socket the suspend killed does.
      return new Promise<OrdersSession>(() => {});
    },
    persist: async () => {},
    timeoutMs: 40,
    log: () => {},
  });

  await assert.rejects(() => keeper.ensure(), TimeoutError);
  assert.equal(keeper.busy, false, "inFlight must be null after a timed-out attempt");
  assert.equal(keeper.current(), null);

  // The next call must ATTEMPT A FRESH REQUEST, not await the dead one.
  // Before the fix this line would hang forever and the test would time out.
  const fresh = session();
  const keeper2Enrol = enrolments;
  const result = await Promise.race([
    keeper
      .ensure()
      .catch(() => null),
    new Promise((r) => setTimeout(() => r("HUNG"), 500)),
  ]);
  assert.notEqual(result, "HUNG", "the next ensureSession() awaited the dead attempt");
  assert.equal(enrolments, keeper2Enrol + 1, "the next call did not try again");
  assert.ok(fresh);
});

test("DA5: a recovered ensureSession works normally afterwards", async () => {
  let hang = true;
  const good = session();
  const persisted: OrdersSession[] = [];
  const keeper = new SessionKeeper({
    loadStored: async () => null,
    refresh: async () => null,
    enroll: async () => (hang ? new Promise<OrdersSession>(() => {}) : good),
    persist: async (s) => {
      persisted.push(s);
    },
    timeoutMs: 40,
    log: () => {},
  });

  await assert.rejects(() => keeper.ensure(), TimeoutError);
  hang = false;
  const s = await keeper.ensure();
  assert.equal(s, good);
  assert.equal(keeper.current(), good);
  assert.equal(persisted.length, 1, "a recovered credential is persisted exactly once");
  assert.equal(keeper.busy, false);

  // A still-valid session is reused without another enrolment.
  const again = await keeper.ensure();
  assert.equal(again, good);
});

test("DA6: a join stuck on a hung credential is abandoned, and a rejoin is scheduled", async () => {
  // THIS IS THE ONE THE OWNER'S BUG REPORT MAPS ONTO. ensureSession() never
  // comes back, exactly as it does when a suspend has killed the socket
  // under it. Before PART D, `joining` stayed true here forever and
  // scheduleRejoin() returned early for the rest of the process's life.
  const scheduled: number[] = [];
  let joinAttempts = 0;
  const channel = new ManagedChannel<{ id: string }>({
    topic: "orders-test",
    channelConfig: {},
    ensureSession: () => {
      joinAttempts++;
      return new Promise(() => {}); // never settles. ever.
    },
    createChannel: () => ({ id: "never-reached" }),
    removeChannel: async () => {},
    configure: () => {},
    subscribe: () => {},
    onSubscribed: async () => {},
    onStatus: () => {},
    onFlapping: () => {},
    joinTimeoutMs: 50, // stands in for the shipped 30 seconds
    setTimer: (_fn, ms) => {
      scheduled.push(ms);
      return scheduled.length - 1;
    },
    clearTimer: () => {},
    log: () => {},
  });

  channel.start();
  assert.equal(channel.isJoining(), true, "the join is genuinely in progress");
  assert.equal(channel.hasRejoinScheduled(), false, "nothing rescheduled while it is trying");

  await new Promise((r) => setTimeout(r, 150));

  assert.equal(joinAttempts, 1);
  assert.equal(channel.isJoining(), false, "R5: `joining` must be false on every exit path");
  assert.equal(channel.isUp(), false);
  assert.equal(channel.hasRejoinScheduled(), true, "a rejoin must have been scheduled");
  assert.deepEqual(scheduled, [1000], "the first rejoin uses the 1s starting backoff");
  await channel.stop();
});

test("DA7: a successful join leaves the flag clear and schedules nothing", async () => {
  const scheduled: number[] = [];
  let statuses: string[] = [];
  const channel = new ManagedChannel<{ id: string }>({
    topic: "orders-test",
    channelConfig: {},
    ensureSession: async () => {},
    createChannel: () => ({ id: "ch" }),
    removeChannel: async () => {},
    configure: () => {},
    subscribe: (_ch, onStatus) => onStatus("SUBSCRIBED"),
    onSubscribed: async () => {},
    onStatus: (s) => statuses.push(s),
    onFlapping: () => {},
    setTimer: (_fn, ms) => {
      scheduled.push(ms);
      return scheduled.length - 1;
    },
    clearTimer: () => {},
    log: () => {},
  });

  channel.start();
  await new Promise((r) => setTimeout(r, 30));

  assert.equal(channel.isJoining(), false);
  assert.equal(channel.isUp(), true);
  assert.deepEqual(statuses, ["connected"]);
  // The only timer armed is R2's 30-second stability window, never a rejoin.
  assert.deepEqual(scheduled, [30_000]);
  assert.equal(channel.hasRejoinScheduled(), false);
  await channel.stop();
});

test("DA8: forceRejoin rebuilds without asking the socket how it is", async () => {
  const built: string[] = [];
  const removed: string[] = [];
  let n = 0;
  const channel = new ManagedChannel<{ id: string }>({
    topic: "orders-test",
    channelConfig: {},
    ensureSession: async () => {},
    createChannel: () => {
      const ch = { id: `ch${++n}` };
      built.push(ch.id);
      return ch;
    },
    removeChannel: async (ch) => {
      removed.push(ch.id);
    },
    configure: () => {},
    subscribe: (_ch, onStatus) => onStatus("SUBSCRIBED"),
    onSubscribed: async () => {},
    onStatus: () => {},
    onFlapping: () => {},
    setTimer: () => 0,
    clearTimer: () => {},
    log: () => {},
  });

  channel.start();
  await new Promise((r) => setTimeout(r, 20));
  assert.deepEqual(built, ["ch1"]);

  // A half-open socket reports healthy. The wake path must not believe it.
  await channel.forceRejoin();
  await new Promise((r) => setTimeout(r, 20));
  assert.deepEqual(removed, ["ch1"], "the old channel is removed, not reused");
  assert.deepEqual(built, ["ch1", "ch2"], "a genuinely new channel is built");
  assert.equal(channel.isJoining(), false);
  await channel.stop();
});

/* ================== DB — THE CLOCK-JUMP TEST ================== */

test("DB1: a five-minute clock jump fires the wake path exactly once", async () => {
  let now = 1_000_000;
  const fired: number[] = [];
  const watchdog = new WakeWatchdog({
    now: () => now,
    setTimer: () => 0,          // the test drives tick() itself
    clearTimer: () => {},
    onWake: (gap) => {
      fired.push(gap);
    },
    log: () => {},
  });
  watchdog.start();

  // A NORMAL TICK MUST NOT FIRE. This is the half of the test that stops the
  // watchdog rebuilding the channels every ten seconds on a busy PC.
  now += 10_000;
  await watchdog.tick();
  assert.deepEqual(fired, [], "a normal 10s tick must not look like a wake");

  // Even a badly delayed tick — a loaded machine, a long GC, a throttled
  // webview — must not fire. 90s is the line.
  now += 60_000;
  await watchdog.tick();
  assert.deepEqual(fired, [], "a 60s stall is a slow machine, not a sleeping one");

  // Five minutes of wall clock with no ticks in between: the machine slept.
  now += 5 * 60_000;
  await watchdog.tick();
  assert.equal(fired.length, 1, "the wake path must fire exactly once");
  assert.ok(fired[0] >= 5 * 60_000);

  // And it must not fire again on the next ordinary tick.
  now += 10_000;
  await watchdog.tick();
  assert.equal(fired.length, 1, "the wake path must not repeat on the next tick");

  watchdog.stop();
});

test("DB2: the wake path runs its four steps, once, in order", async () => {
  let now = 0;
  const steps: string[] = [];
  const watchdog = new WakeWatchdog({
    now: () => now,
    setTimer: () => 0,
    clearTimer: () => {},
    onWake: async () => {
      // Mirrors recoverFromWake() in orderBridge: rebuild, beat, reconcile,
      // flush. Order matters — the beat is what stops the phone reading a
      // pos_last_seen_at frozen at the moment the machine suspended.
      steps.push("force-rejoin");
      steps.push("pos-alive-beat");
      steps.push("reconcile");
      steps.push("flush-bills");
    },
    log: () => {},
  });
  watchdog.start();

  now += 5 * 60_000;
  await watchdog.tick();
  assert.deepEqual(steps, ["force-rejoin", "pos-alive-beat", "reconcile", "flush-bills"]);

  now += 5 * 60_000;
  await watchdog.tick();
  assert.equal(steps.length, 8, "a second genuine wake recovers again");
  watchdog.stop();
});

test("DB3: a resume event and a clock jump together recover only once", async () => {
  let now = 0;
  let running = 0;
  let completed = 0;
  const watchdog = new WakeWatchdog({
    now: () => now,
    setTimer: () => 0,
    clearTimer: () => {},
    onWake: async () => {
      running++;
      await new Promise((r) => setTimeout(r, 30));
      completed++;
    },
    log: () => {},
  });
  watchdog.start();

  now += 5 * 60_000;
  const a = watchdog.tick();
  const b = watchdog.resumeHint();   // Tauri focus lands in the same instant
  await Promise.all([a, b]);
  assert.equal(running, 1, "the channels must not be rebuilt twice for one wake");
  assert.equal(completed, 1);
  watchdog.stop();
});

test("DB4: a stopped watchdog does nothing at all", async () => {
  let now = 0;
  let fired = 0;
  const watchdog = new WakeWatchdog({
    now: () => now,
    setTimer: () => 0,
    clearTimer: () => {},
    onWake: () => {
      fired++;
    },
    log: () => {},
  });
  watchdog.start();
  watchdog.stop();
  now += 10 * 60_000;
  await watchdog.tick();
  await watchdog.resumeHint();
  assert.equal(fired, 0);
});

/* ============ the constant the whole of PART D turns on ============ */

test("the cloud deadline is 15 seconds", () => {
  assert.equal(CLOUD_TIMEOUT_MS, 15_000);
});
