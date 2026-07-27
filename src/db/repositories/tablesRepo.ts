import { getDb } from "../client";
import { requestCatalogPush } from "../../services/orders/signals";
import type { RestaurantTable } from "../../types";

/**
 * Table master CRUD (Tables & Mobile Ordering settings).
 * The billing screen's free-text table popup is untouched by this master —
 * it exists so the phone can render a tappable grid.
 */

export async function listTables(): Promise<RestaurantTable[]> {
  return getDb().select<RestaurantTable[]>(
    "SELECT * FROM restaurant_tables ORDER BY section, sort_order, label"
  );
}

export async function addTable(section: string, label: string, sortOrder: number): Promise<void> {
  await getDb().execute(
    "INSERT INTO restaurant_tables (section, label, sort_order) VALUES ($1, $2, $3)",
    [section, label, sortOrder]
  );
  requestCatalogPush();
}

export async function updateTable(
  id: number,
  section: string,
  label: string,
  sortOrder: number
): Promise<void> {
  await getDb().execute(
    "UPDATE restaurant_tables SET section = $1, label = $2, sort_order = $3 WHERE id = $4",
    [section, label, sortOrder, id]
  );
  requestCatalogPush();
}

export async function setTableActive(id: number, active: boolean): Promise<void> {
  await getDb().execute("UPDATE restaurant_tables SET is_active = $1 WHERE id = $2", [
    active ? 1 : 0,
    id,
  ]);
  requestCatalogPush();
}

export async function deleteTable(id: number): Promise<void> {
  await getDb().execute("DELETE FROM restaurant_tables WHERE id = $1", [id]);
  requestCatalogPush();
}

export async function tableLabelExists(
  section: string,
  label: string,
  excludeId?: number
): Promise<boolean> {
  const rows = await getDb().select<{ id: number }[]>(
    "SELECT id FROM restaurant_tables WHERE section = $1 AND LOWER(label) = LOWER($2)",
    [section, label]
  );
  return rows.some((r) => r.id !== excludeId);
}

/**
 * Table numbers currently held by open orders. A table's own label plus its
 * sub-table letters ("6" -> "6B".."6H", same semantics as the billing
 * alphabet popup) all count as "this table is in use".
 */
export async function openTableNumbers(): Promise<string[]> {
  const rows = await getDb().select<{ table_number: string | null }[]>(
    "SELECT table_number FROM processing_orders WHERE order_type = 'Table' AND table_number IS NOT NULL"
  );
  return rows.map((r) => (r.table_number ?? "").trim()).filter((t) => t !== "");
}

/** True when an open order sits on this label or one of its sub-tables. */
export function labelInUse(label: string, openNumbers: string[]): boolean {
  const base = label.trim().toUpperCase();
  if (base === "") return false;
  return openNumbers.some((n) => {
    const num = n.toUpperCase();
    if (num === base) return true;
    // Sub-table = base + one letter B..H (tableUtils semantics).
    return num.length === base.length + 1 && num.startsWith(base) && /[B-H]$/.test(num);
  });
}

/**
 * Bulk add "tables <from>..<to> in <section>". Labels that already exist in
 * the section are skipped. Returns how many were actually created.
 */
export async function bulkAddTables(section: string, from: number, to: number): Promise<number> {
  const db = getDb();
  const existing = await db.select<{ label: string }[]>(
    "SELECT label FROM restaurant_tables WHERE section = $1",
    [section]
  );
  const have = new Set(existing.map((r) => r.label.toUpperCase()));
  const maxRows = await db.select<{ m: number | null }[]>(
    "SELECT MAX(sort_order) AS m FROM restaurant_tables WHERE section = $1",
    [section]
  );
  let sortOrder = (maxRows[0]?.m ?? 0) + 1;

  let created = 0;
  for (let n = from; n <= to; n++) {
    const label = String(n);
    if (have.has(label)) continue;
    await db.execute(
      "INSERT INTO restaurant_tables (section, label, sort_order) VALUES ($1, $2, $3)",
      [section, label, sortOrder++]
    );
    created++;
  }
  if (created > 0) requestCatalogPush();
  return created;
}
