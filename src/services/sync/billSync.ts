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
const FIRST_RUN_DELAY_MS = 20_000;
const INTERVAL_MS = 5 * 60_000;

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

export async function pushUnsyncedBills(): Promise<void> {
  if (pushing) return; // A previous push is still running — skip this cycle.
  pushing = true;
  try {
    if (!navigator.onLine || !isDbOpen()) return;
    const key = getStoredLicenseKey();
    if (!key) return;

    const rows = await getDb().select<OutboxRow[]>(
      `SELECT id, cart_data, customer_name, customer_phone, payment_mode,
              subtotal, gst, total, order_type, table_number, created_at,
              token_number, bill_number
         FROM finalized_orders WHERE synced = 0 ORDER BY id ASC LIMIT ${BATCH_SIZE}`
    );
    if (rows.length === 0) return;

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
      return;
    }

    if (data.ok) {
      await markBatch(ids, "synced = 1, last_sync_error = ''");
      console.info(`[billSync] pushed ${ids.length} bill(s) to cloud`);
    } else {
      // unbound/invalid-key: licensing will handle deactivation separately;
      // here we just record the refusal and stop thrashing until next cycle.
      await markBatch(ids, "sync_attempts = sync_attempts + 1, last_sync_error = $1", [
        String(data.reason || "rejected"),
      ]);
    }
  } catch (err) {
    console.error("[billSync] sync cycle failed:", err);
  } finally {
    pushing = false;
  }
}

let startTimer: ReturnType<typeof setTimeout> | null = null;
let interval: ReturnType<typeof setInterval> | null = null;

/** Kicks off the periodic sync (once ~20s after start, then every 5 minutes). Idempotent. */
export function startBillSync(): void {
  if (startTimer || interval) return; // Already running.
  startTimer = setTimeout(() => {
    pushUnsyncedBills().catch(() => {});
    interval = setInterval(() => pushUnsyncedBills().catch(() => {}), INTERVAL_MS);
  }, FIRST_RUN_DELAY_MS);
}

export function stopBillSync(): void {
  if (startTimer) {
    clearTimeout(startTimer);
    startTimer = null;
  }
  if (interval) {
    clearInterval(interval);
    interval = null;
  }
}
