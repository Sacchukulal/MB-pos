import { getDb, isDbOpen } from "../../db/client";
import { rpc } from "../orders/cloud";

/**
 * Outbox-pattern bill sync: finalized_orders rows with synced=0 are pushed
 * to the cloud in batches, upserted on (license_key, local_id), so
 * re-sending a bill is always safe.
 *
 * WHAT CHANGED IN AUGUST, AND WHY IT MATTERED MORE THAN ANYTHING ELSE ON
 * THE LIST.
 *
 * This used to POST to the `sync-bills` EDGE FUNCTION, and requestBillSync()
 * is called after every finalised bill, every edited bill and every
 * mobile-order settle, debounced by 1,500 ms. In practice: ONE METERED CALL
 * PER BILL. Edge Function invocations are the only thing Supabase counts
 * project-wide, and at the owner's real volume that was
 *
 *     250 bills x 30 days x 30 shops = 225,000/month = 45% of the free plan
 *
 * from bill sync alone — against a rebuilt live-ordering feature that costs
 * zero. Measured, not estimated: MB-backend/test/quota-simulation.mjs Q11.
 *
 * Worse, the failure mode had no brake at all. A refused batch left the rows
 * synced=0 and the blind 60-second sweep retried forever: 1,440 calls per
 * shop per day, indefinitely, for one bill the server would never accept.
 * Across 30 shops that is 1.3 million calls a month — 2.6x the entire free
 * plan — from a single poison row.
 *
 * It now goes through mb_push_bills (migration 0020), a Postgres RPC under
 * the counter's own credential. PostgREST calls are not metered by count, so
 * the ongoing cost of billing is zero. It also inherits the PART D timeouts
 * and the enrolment logic from services/orders/cloud.ts, which means a
 * counter that has never enrolled (mobile ordering never switched on) enrols
 * itself on its first bill sync — one Edge call ever, then free forever.
 *
 * THREE RULES, all learned from the arithmetic above:
 *
 *   1. A row is marked synced=1 ONLY after the server has confirmed it. Not
 *      on a timeout, not on a network error, not optimistically.
 *   2. Repeated failure BACKS OFF: 1m, 2m, 5m, 15m, then 30m, reset on the
 *      first success. The blind 60-second retry is gone.
 *   3. A row that the SERVER keeps refusing is parked after a bounded number
 *      of attempts, surfaced to the owner, and retried only when the owner
 *      asks. A poison bill can never run a loop forever, even on a free path.
 *
 * Everything here is still fire-and-forget: a sync failure must never crash
 * or block billing.
 */

const BATCH_SIZE = 200;
const FIRST_RUN_DELAY_MS = 5_000;
/** Coalesces rapid consecutive bills (rush hour) into one request. */
const DEBOUNCE_MS = 1_500;

/** The healthy sweep. Catches offline recovery and anything a debounce missed. */
const SWEEP_MS = 60_000;
/** Rule 2. Consecutive-failure backoff, capped. */
const BACKOFF_MS = [60_000, 120_000, 300_000, 900_000, 1_800_000];

/**
 * Rule 3. After this many refusals a row stops being retried automatically.
 * With the backoff above, reaching 20 takes the better part of a day of the
 * server actively saying no — so an ordinary outage never gets near it.
 * Network failures do not count towards it at all (see recordTransportFail).
 */
export const MAX_SYNC_ATTEMPTS = 20;

/**
 * Reasons that are NOT the row's fault. A licence problem or a server error
 * says nothing about this particular bill, so it must not consume the row's
 * attempts — otherwise a weekend of downtime would park a shop's whole day
 * of billing and need a manual click to recover.
 */
const NOT_THE_ROWS_FAULT = new Set(["revoked", "server", "batch-too-large", "bad-json"]);

interface OutboxRow {
  id: number;
  cart_data: string | null;
  customer_name: string | null;
  customer_phone: string | null;
  payment_mode: string | null;
  subtotal: number | null;
  gst: number | null;
  total: number | null;
  order_type: string | null;
  table_number: string | null;
  created_at: string | null;
  token_number: number | null;
  bill_number: string | null;
}

/** created_at is either an ISO string (new rows) or SQLite's UTC "YYYY-MM-DD HH:MM:SS". */
function toIso(createdAt: string | null): string {
  if (createdAt) {
    const normalized = createdAt.includes("T")
      ? createdAt
      : createdAt.replace(" ", "T") + "Z";
    const d = new Date(normalized);
    if (!isNaN(d.getTime())) return d.toISOString();
  }
  return new Date().toISOString();
}

function mapRowToBill(row: OutboxRow) {
  let items: unknown = null;
  try {
    items = row.cart_data ? JSON.parse(row.cart_data) : null;
  } catch {
    items = null; // Unparseable cart JSON — sync the bill totals anyway.
  }
  return {
    local_id: row.id,
    bill_number: row.bill_number,
    token_number: row.token_number,
    order_type: row.order_type,
    table_number: row.table_number,
    customer_name: row.customer_name,
    customer_phone: row.customer_phone,
    payment_mode: row.payment_mode,
    subtotal: row.subtotal ?? 0,
    gst: row.gst ?? 0,
    total: row.total ?? 0,
    items,
    billed_at: toIso(row.created_at),
  };
}

async function markBatch(ids: number[], set: string, params: unknown[] = []): Promise<void> {
  if (ids.length === 0) return;
  const offset = params.length;
  const placeholders = ids.map((_, i) => `$${offset + i + 1}`).join(",");
  await getDb().execute(
    `UPDATE finalized_orders SET ${set} WHERE id IN (${placeholders})`,
    [...params, ...ids]
  );
}

/** A transport failure. Records the reason but does NOT spend the row's attempts. */
async function recordTransportFail(ids: number[], message: string): Promise<void> {
  await markBatch(ids, "last_sync_error = $1", [message.slice(0, 300)]);
}

/* ------------------------------ status ------------------------------ */

export interface BillSyncStatus {
  /** Bills waiting to go, still being retried automatically. */
  pending: number;
  /** Bills the server kept refusing. Parked; the owner can retry them. */
  stuck: number;
  lastError: string;
  lastSuccessAt: string | null;
  consecutiveFailures: number;
}

let status: BillSyncStatus = {
  pending: 0,
  stuck: 0,
  lastError: "",
  lastSuccessAt: null,
  consecutiveFailures: 0,
};

const statusSubs = new Set<(s: BillSyncStatus) => void>();

export function subscribeBillSync(cb: (s: BillSyncStatus) => void): () => void {
  statusSubs.add(cb);
  return () => statusSubs.delete(cb);
}

export function getBillSyncStatus(): BillSyncStatus {
  return { ...status };
}

async function refreshCounts(): Promise<void> {
  if (!isDbOpen()) return;
  try {
    const rows = await getDb().select<{ pending: number; stuck: number }[]>(
      `SELECT
         SUM(CASE WHEN sync_attempts < ${MAX_SYNC_ATTEMPTS} THEN 1 ELSE 0 END) AS pending,
         SUM(CASE WHEN sync_attempts >= ${MAX_SYNC_ATTEMPTS} THEN 1 ELSE 0 END) AS stuck
       FROM finalized_orders WHERE synced = 0`
    );
    status.pending = Number(rows?.[0]?.pending ?? 0);
    status.stuck = Number(rows?.[0]?.stuck ?? 0);
  } catch {
    /* the counter must never break because a count failed */
  }
}

function emitStatus(): void {
  const snapshot = { ...status };
  statusSubs.forEach((cb) => cb(snapshot));
}

/* ------------------------------ the push ------------------------------ */

let pushing = false;
let rerunRequested = false;

type BatchResult = "drained" | "more" | "parked" | "retry";

/** Pushes ONE batch. */
async function pushBatch(): Promise<BatchResult> {
  if (!navigator.onLine || !isDbOpen()) return "retry";

  const rows = await getDb().select<OutboxRow[]>(
    `SELECT id, cart_data, customer_name, customer_phone, payment_mode,
            subtotal, gst, total, order_type, table_number, created_at,
            token_number, bill_number
       FROM finalized_orders
      WHERE synced = 0 AND sync_attempts < ${MAX_SYNC_ATTEMPTS}
      ORDER BY id ASC LIMIT ${BATCH_SIZE}`
  );
  if (rows.length === 0) return "drained";

  const ids = rows.map((r) => r.id);

  let data: { ok: boolean; reason?: string; saved?: number; local_id?: unknown };
  try {
    // Through cloud.ts: the counter's own credential, a 15-second deadline,
    // and enrolment on first use if this counter has never enrolled.
    data = await rpc("mb_push_bills", { p_bills: rows.map(mapRowToBill) });
  } catch (err: any) {
    // Offline blip, timeout, or the cloud refusing to talk to us. NOT the
    // row's fault, so the attempt counter is untouched — we simply back off.
    await recordTransportFail(ids, String(err?.message || err || "network error"));
    return "retry";
  }

  if (data?.ok) {
    // RULE 1. Only now, and only for the rows the server confirmed.
    await markBatch(ids, "synced = 1, last_sync_error = ''");
    console.info(`[billSync] pushed ${ids.length} bill(s) to cloud`);
    status.lastSuccessAt = new Date().toISOString();
    status.lastError = "";
    return rows.length === BATCH_SIZE ? "more" : "drained";
  }

  const reason = String(data?.reason ?? "rejected");
  status.lastError = reason;

  // RULE 3, precisely. The server rejects the WHOLE batch for one bad row
  // and tells us which one, so park exactly that row and let the rest
  // through on the next pass rather than punishing the batch.
  if (reason === "bad-bill") {
    const badId = Number(data?.local_id);
    if (Number.isInteger(badId) && ids.includes(badId)) {
      await markBatch(
        [badId],
        `sync_attempts = ${MAX_SYNC_ATTEMPTS}, last_sync_error = $1`,
        ["the server could not accept this bill (bad-bill)"]
      );
      console.error(
        `[billSync] bill ${badId} was refused as malformed and has been set aside. ` +
          `The rest of the queue continues. Retry it from Settings once it is fixed.`
      );
      return "parked";
    }
  }

  if (NOT_THE_ROWS_FAULT.has(reason)) {
    await recordTransportFail(ids, reason);
    return "retry";
  }

  // An unknown refusal. Spend an attempt so a batch nobody can diagnose
  // still stops eventually instead of running forever.
  await markBatch(ids, "sync_attempts = sync_attempts + 1, last_sync_error = $1", [reason]);
  return "retry";
}

export async function pushUnsyncedBills(): Promise<void> {
  if (pushing) {
    // A bill landed while a push is in flight — run once more afterwards
    // so it doesn't wait for the sweep.
    rerunRequested = true;
    return;
  }
  pushing = true;
  let failed = false;
  try {
    // Drain the whole outbox, batch by batch. "parked" also continues: a row
    // was removed from the queue, so there is progress to be made.
    let guard = 0;
    for (;;) {
      const result = await pushBatch();
      if (result === "more" || result === "parked") {
        if (++guard > 200) break; // cannot loop on a shop-sized outbox
        continue;
      }
      failed = result === "retry";
      break;
    }
  } catch (err) {
    failed = true;
    console.error("[billSync] sync cycle failed:", err);
  } finally {
    pushing = false;
    status.consecutiveFailures = failed ? status.consecutiveFailures + 1 : 0;
    await refreshCounts();
    emitStatus();
    scheduleSweep();
    if (rerunRequested) {
      rerunRequested = false;
      pushUnsyncedBills().catch(() => {});
    }
  }
}

/**
 * The owner has fixed whatever was wrong (or just wants to try again).
 * Un-parks every set-aside bill and pushes immediately.
 */
export async function retryStuckBills(): Promise<void> {
  if (!isDbOpen()) return;
  await getDb().execute(
    `UPDATE finalized_orders SET sync_attempts = 0, last_sync_error = ''
      WHERE synced = 0 AND sync_attempts >= ${MAX_SYNC_ATTEMPTS}`
  );
  status.consecutiveFailures = 0;
  await refreshCounts();
  emitStatus();
  await pushUnsyncedBills();
}

/* ---------------------------- scheduling ---------------------------- */

let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let sweepTimer: ReturnType<typeof setTimeout> | null = null;
let startTimer: ReturnType<typeof setTimeout> | null = null;
let running = false;

/** RULE 2. Healthy: every minute. Failing: 1m, 2m, 5m, 15m, then 30m. */
function nextSweepDelayMs(): number {
  if (status.consecutiveFailures === 0) return SWEEP_MS;
  const i = Math.min(status.consecutiveFailures - 1, BACKOFF_MS.length - 1);
  return BACKOFF_MS[i];
}

function scheduleSweep(): void {
  if (!running) return;
  if (sweepTimer) clearTimeout(sweepTimer);
  sweepTimer = setTimeout(() => {
    sweepTimer = null;
    pushUnsyncedBills().catch(() => {});
  }, nextSweepDelayMs());
}

/**
 * Instant sync trigger — call right after finalizing/editing a bill.
 * Debounced a moment so a burst of checkouts becomes one request.
 */
export function requestBillSync(): void {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    pushUnsyncedBills().catch(() => {});
  }, DEBOUNCE_MS);
}

// The instant a connection comes back, flush whatever billing queued
// while offline — and forget the backoff, because the reason for it is gone.
function handleOnline(): void {
  status.consecutiveFailures = 0;
  requestBillSync();
}

/**
 * Kicks off the sync engine: an initial push ~5s after start, then a
 * self-scheduling sweep whose interval is the backoff above, plus an
 * online-recovery listener. Idempotent.
 */
export function startBillSync(): void {
  if (running) return;
  running = true;
  window.addEventListener("online", handleOnline);
  startTimer = setTimeout(() => {
    startTimer = null;
    pushUnsyncedBills().catch(() => {});
  }, FIRST_RUN_DELAY_MS);
}

export function stopBillSync(): void {
  running = false;
  window.removeEventListener("online", handleOnline);
  for (const t of [startTimer, sweepTimer, debounceTimer]) if (t) clearTimeout(t);
  startTimer = sweepTimer = debounceTimer = null;
}
