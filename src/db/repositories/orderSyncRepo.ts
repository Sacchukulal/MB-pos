import { getDb } from "../client";

/**
 * Single-row bookkeeping for the mobile-orders bridge (order_sync_state).
 * The enabled/sound switches live here so the owner's choice survives
 * restarts and the bridge can react without a cloud round trip.
 */

export interface OrderSyncState {
  catalogHash: string;
  roomId: string;
  lastOrdersSeq: number;
  lastReconcileAt: string;
  mobileOrderingEnabled: boolean;
  soundOnNewOrder: boolean;
  /** The counter's Supabase Auth session, minted once by orders-enroll. */
  ordersAccessToken: string;
  ordersRefreshToken: string;
  /** Epoch SECONDS, as Supabase reports it. */
  ordersTokenExpiresAt: number;
  /** Epoch millis of every Edge Function call in the last 30 days (P5). */
  edgeCallLog: number[];
}

function parseCallLog(raw: unknown): number[] {
  if (typeof raw !== "string" || raw === "") return [];
  try {
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((n) => typeof n === "number") : [];
  } catch {
    return [];
  }
}

export async function getOrderSyncState(): Promise<OrderSyncState> {
  const rows = await getDb().select<any[]>("SELECT * FROM order_sync_state WHERE id = 1");
  const row = rows[0] ?? {};
  return {
    catalogHash: row.catalog_hash ?? "",
    roomId: row.room_id ?? "",
    lastOrdersSeq: Number(row.last_orders_seq ?? 0),
    lastReconcileAt: row.last_reconcile_at ?? "",
    mobileOrderingEnabled: Number(row.mobile_ordering_enabled ?? 0) === 1,
    soundOnNewOrder: Number(row.sound_on_new_order ?? 1) === 1,
    ordersAccessToken: row.orders_access_token ?? "",
    ordersRefreshToken: row.orders_refresh_token ?? "",
    ordersTokenExpiresAt: Number(row.orders_token_expires_at ?? 0),
    edgeCallLog: parseCallLog(row.edge_call_log),
  };
}

export async function setOrdersSession(s: {
  accessToken: string;
  refreshToken: string;
  expiresAt: number;
  roomId: string;
}): Promise<void> {
  await getDb().execute(
    `UPDATE order_sync_state
        SET orders_access_token = $1,
            orders_refresh_token = $2,
            orders_token_expires_at = $3,
            room_id = CASE WHEN $4 = '' THEN room_id ELSE $4 END
      WHERE id = 1`,
    [s.accessToken, s.refreshToken, s.expiresAt, s.roomId]
  );
}

export async function setEdgeCallLog(timestamps: number[]): Promise<void> {
  await getDb().execute("UPDATE order_sync_state SET edge_call_log = $1 WHERE id = 1", [
    JSON.stringify(timestamps),
  ]);
}

export async function setMobileOrderingEnabled(enabled: boolean): Promise<void> {
  await getDb().execute("UPDATE order_sync_state SET mobile_ordering_enabled = $1 WHERE id = 1", [
    enabled ? 1 : 0,
  ]);
}

export async function setSoundOnNewOrder(enabled: boolean): Promise<void> {
  await getDb().execute("UPDATE order_sync_state SET sound_on_new_order = $1 WHERE id = 1", [
    enabled ? 1 : 0,
  ]);
}

export async function setCatalogHash(hash: string): Promise<void> {
  await getDb().execute("UPDATE order_sync_state SET catalog_hash = $1 WHERE id = 1", [hash]);
}

export async function setRoomId(roomId: string): Promise<void> {
  await getDb().execute("UPDATE order_sync_state SET room_id = $1 WHERE id = 1", [roomId]);
}

export async function setLastOrdersSeq(seq: number): Promise<void> {
  await getDb().execute("UPDATE order_sync_state SET last_orders_seq = $1 WHERE id = 1", [seq]);
}

export async function setLastReconcileAt(iso: string): Promise<void> {
  await getDb().execute("UPDATE order_sync_state SET last_reconcile_at = $1 WHERE id = 1", [iso]);
}
