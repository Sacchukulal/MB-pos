import { getDb, isDbOpen } from "../../db/client";
import { SUPABASE_FUNCTIONS_URL, SUPABASE_ANON_KEY } from "../../config/supabase";
import { getStoredLicenseKey } from "../license/licenseService";
import { getDeviceInfo } from "../license/device";

/**
 * Outbox-pattern bill sync: finalized_orders rows with synced=0 are pushed to
 * the Supabase `sync-bills` Edge Function in batches. The server upserts on
 * (license_key, local_id), so re-sending a bill is always safe.
 *
 * Everything here is fire-and-forget: a sync failure must never crash or block
 * billing. Failures increment sync_attempts / last_sync_error and are retried
 * on the next cycle.
 */

const BATCH_SIZE = 200;
const FIRST_RUN_DELAY_MS = 5_000;
// Fallback sweep only — every finalized bill also triggers an instant
// push via requestBillSync(), so this just catches offline recovery and
// anything a debounce window missed.
const INTERVAL_MS = 60_000;
// Coalesces rapid consecutive bills (rush hour) into one request.
const DEBOUNCE_MS = 1_500;

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

let pushing = false;
let rerunRequested = false;

/** Pushes ONE batch. Returns "drained" | "more" | "stop". */
async function pushBatch(): Promise<"drained" | "more" | "stop"> {
  if (!navigator.onLine || !isDbOpen()) return "stop";
  const key = getStoredLicenseKey();
  if (!key) return "stop";

  const rows = await getDb().select<OutboxRow[]>(
    `SELECT id, cart_data, customer_name, customer_phone, payment_mode,
            subtotal, gst, total, order_type, table_number, created_at,
            token_number, bill_number
       FROM finalized_orders WHERE synced = 0 ORDER BY id ASC LIMIT ${BATCH_SIZE}`
  );
  if (rows.length === 0) return "drained";

  const device = await getDeviceInfo();
  const ids = rows.map((r) => r.id);

  let data: { ok: boolean; reason?: string; saved?: number };
  try {
    const res = await fetch(`${SUPABASE_FUNCTIONS_URL}/sync-bills`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${SUPABASE_ANON_KEY}`,
        apikey: SUPABASE_ANON_KEY,
      },
      body: JSON.stringify({ key, deviceId: device.id, bills: rows.map(mapRowToBill) }),
    });
    data = await res.json();
  } catch (err: any) {
    // Offline blip or server unreachable — note it and retry next cycle.
    await markBatch(ids, "sync_attempts = sync_attempts + 1, last_sync_error = $1", [
      String(err?.message || err || "network error"),
    ]);
    return "stop";
  }

  if (data.ok) {
    await markBatch(ids, "synced = 1, last_sync_error = ''");
    console.info(`[billSync] pushed ${ids.length} bill(s) to cloud`);
    return rows.length === BATCH_SIZE ? "more" : "drained";
  }
  // unbound/invalid-key: licensing will handle deactivation separately;
  // here we just record the refusal and stop thrashing until next cycle.
  await markBatch(ids, "sync_attempts = sync_attempts + 1, last_sync_error = $1", [
    String(data.reason || "rejected"),
  ]);
  return "stop";
}

export async function pushUnsyncedBills(): Promise<void> {
  if (pushing) {
    // A bill landed while a push is in flight — run once more afterwards
    // so it doesn't wait for the fallback sweep.
    rerunRequested = true;
    return;
  }
  pushing = true;
  try {
    // Drain the whole outbox, batch by batch.
    while ((await pushBatch()) === "more") {
      /* keep going */
    }
  } catch (err) {
    console.error("[billSync] sync cycle failed:", err);
  } finally {
    pushing = false;
    if (rerunRequested) {
      rerunRequested = false;
      pushUnsyncedBills().catch(() => {});
    }
  }
}

let debounceTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Instant sync trigger — call right after finalizing/editing a bill.
 * Debounced a moment so a burst of checkouts becomes one request;
 * a bill reaches Supabase ~1.5s after printing instead of minutes.
 */
export function requestBillSync(): void {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    pushUnsyncedBills().catch(() => {});
  }, DEBOUNCE_MS);
}

// The instant a connection comes back, flush whatever billing queued
// while offline.
function handleOnline(): void {
  requestBillSync();
}

let startTimer: ReturnType<typeof setTimeout> | null = null;
let interval: ReturnType<typeof setInterval> | null = null;

/**
 * Kicks off the sync engine: an initial push ~5s after start, a 60s
 * fallback sweep, and an online-recovery listener. Bills themselves are
 * pushed instantly via requestBillSync() from the billing flow. Idempotent.
 */
export function startBillSync(): void {
  if (startTimer || interval) return; // Already running.
  window.addEventListener("online", handleOnline);
  startTimer = setTimeout(() => {
    pushUnsyncedBills().catch(() => {});
    interval = setInterval(() => pushUnsyncedBills().catch(() => {}), INTERVAL_MS);
  }, FIRST_RUN_DELAY_MS);
}

export function stopBillSync(): void {
  window.removeEventListener("online", handleOnline);
  if (startTimer) {
    clearTimeout(startTimer);
    startTimer = null;
  }
  if (interval) {
    clearInterval(interval);
    interval = null;
  }
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
}
