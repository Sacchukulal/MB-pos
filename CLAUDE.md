# CLAUDE.md — MB-pos (Tauri + React desktop counter)

The billing counter. It is the sole author of order truth, token and bill
numbers, printing and finalization; phones only submit intents. It must keep
billing perfectly with no internet at all.

Its backend lives in `../MB-backend` (own repo, no parent repo). Read that
repo's CLAUDE.md before changing anything that talks to the cloud.

## Commands

- `npm run dev` / `npm run tauri dev` — the dev counter. `tauri` is run
  through `dotenv -e .env`, so `.env` must hold the Supabase anon key or the
  build refuses (vite.config.ts).
- `npm run build` — `tsc && vite build`. Typecheck is part of it.
- `npm test` — `node --test` over `test/**/*.test.ts`, using Node's own
  TypeScript stripping. **Strip-only mode cannot desugar TypeScript
  parameter properties** (`constructor(private readonly x: T)`), so anything
  the tests import must declare its fields explicitly. Node also needs
  explicit `.ts` extensions on relative imports in those files.

## Fixed design decisions (do not re-question)

1. **ONE COUNTER AT A TIME.** The installed app and a `tauri dev` build share
   a licence and a hardware device id, so they present as the same counter
   and will fight over presence, orders and the liveness beat. Close one
   before starting the other.
2. **Edge Function invocations are the only metered call.** Since 2026-08-01
   the counter makes exactly one, ever: `orders-enroll`, to mint its
   credential. Bill sync moved to the `mb_push_bills` RPC — it used to be one
   metered call per bill, 45% of the whole free plan at 30 shops. Do not put
   anything back on the Edge path.
3. **NOTHING IN THE CLOUD PATH MAY HANG** (`src/services/net/timeout.ts`).
   When the PC sleeps its sockets die silently and a request in flight hangs
   rather than failing. Every request has a 15-second deadline, no shared
   promise may become permanent (`SingleFlight`), and no flag may become
   permanent (`ManagedChannel`'s R5). This is not theoretical: it deadlocked
   the whole cloud side of the counter, including the liveness beat, until
   the process was restarted.
4. **A bill is marked `synced = 1` only after the server confirms it** —
   never on a timeout, never optimistically. Repeated failure backs off
   (1m, 2m, 5m, 15m, 30m); a row the server keeps refusing is parked after 20
   attempts and surfaced in Settings for a manual retry. Network and licence
   failures never spend a row's attempts.
5. **The realtime client's structural rules are scar tissue**
   (`src/services/realtime/managedChannel.ts`). A single transient drop once
   produced 31 rejoins and 31 paid calls in 65 seconds — ~43,000 invocations
   per shop per day, forever. Removal is awaited before a rejoin, status
   events from a replaced channel are ignored by identity, the backoff resets
   only after 30 seconds of stability, a reconnect never fetches by itself,
   and a join is abandoned after 30 seconds. Do not "simplify" them.
6. **After a suspend, never ask the socket how it is.** It is commonly
   half-open: it reports healthy and will never deliver another message. The
   wake path force-rebuilds both channels, beats once, reconciles once and
   flushes the bill outbox.
7. **Reconcile republishes the FULL open set every five minutes on purpose.**
   It is what repairs a cloud row that drifted. The payload-hash skip in
   `pushOrdersNow` deliberately does not apply to a forced push — skipping
   there would switch the self-heal off.
8. **The counter proves it is alive by doing its job**, plus one unmetered
   60-second beat while idle. The server's window is 300s and is equal to the
   phone's trust window by design; change one and you must change the other.

## Guardrails

- NEVER commit `.env` or any Supabase key.
- A sync failure must never crash, block or slow down billing. Everything in
  `services/` is fire-and-forget behind `guarded()`.
- `src/services/orders/statusCopy.ts` is the ONE place bridge state becomes
  words. v1.3.0 showed a bare "Offline" for four different situations and it
  cost the owner an evening.
