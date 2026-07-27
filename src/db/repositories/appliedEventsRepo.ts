import { getDb } from "../client";

/**
 * Idempotency ledger for mobile order events. An event recorded here has
 * already been applied to SQLite/printers — if the network died between
 * apply and ack, the re-pulled event is acked without re-applying (nothing
 * double-prints).
 */

export async function hasAppliedEvent(eventId: string): Promise<boolean> {
  const rows = await getDb().select<{ event_id: string }[]>(
    "SELECT event_id FROM applied_order_events WHERE event_id = $1",
    [eventId]
  );
  return rows.length > 0;
}

export async function recordAppliedEvent(eventId: string, kind: string): Promise<void> {
  await getDb().execute(
    "INSERT OR IGNORE INTO applied_order_events (event_id, kind, applied_at) VALUES ($1, $2, $3)",
    [eventId, kind, new Date().toISOString()]
  );
}

/** Keep the ledger from growing forever (events older than 30 days are gone server-side anyway). */
export async function pruneAppliedEvents(): Promise<void> {
  await getDb().execute(
    "DELETE FROM applied_order_events WHERE applied_at < datetime('now', '-30 days')"
  );
}
