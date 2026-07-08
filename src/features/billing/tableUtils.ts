import type { ProcessingOrder } from "../../types";

/**
 * Table occupancy helpers. A "base" table number like "6" owns the plain slot
 * plus lettered sub-tables "6B".."6H" ('A' is implicitly the base order).
 * String comparison — no regexes built from user input.
 */

/** True if `tableNumber` is `base` or `base` + a single letter (e.g. "6", "6B"). */
export function belongsToTable(tableNumber: string | null | undefined, base: string): boolean {
  const value = String(tableNumber ?? "").toUpperCase();
  const b = base.trim().toUpperCase();
  if (!b) return false;
  if (value === b) return true;
  return value.length === b.length + 1 && value.startsWith(b) && /[A-Z]/.test(value[value.length - 1]);
}

/** All processing orders occupying the given base table, sorted by sub-letter. */
export function occupiedOrdersForTable(orders: ProcessingOrder[], base: string): ProcessingOrder[] {
  const trimmed = base.trim();
  if (!trimmed) return [];
  return orders
    .filter((o) => o.order_type === "Table" && o.table_number && belongsToTable(o.table_number, trimmed))
    .sort((a, b) => String(a.table_number).localeCompare(String(b.table_number)));
}

/** True if the exact table slot (e.g. "6B") is taken. */
export function isSlotOccupied(orders: ProcessingOrder[], slot: string): boolean {
  const target = slot.trim().toUpperCase();
  return orders.some((o) => o.order_type === "Table" && String(o.table_number).toUpperCase() === target);
}

/** Find the processing order whose table number matches the typed text exactly. */
export function findTableOrder(orders: ProcessingOrder[], typed: string): ProcessingOrder | undefined {
  const target = typed.trim().toLowerCase();
  if (!target) return undefined;
  return orders.find((o) => o.order_type === "Table" && String(o.table_number).toLowerCase() === target);
}
