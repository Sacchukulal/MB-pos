import type { ProcessingOrder } from "../../types";

/**
 * Table occupancy helpers. A "base" table number like "6" owns the plain slot
 * plus lettered sub-tables "6B".."6H" ('A' is implicitly the base order).
 * String comparison — no regexes built from user input.
 */

/**
 * The ONE place a table's identity string is built — never compose it inline.
 *
 * A table's identity is its section plus its label ("AC 1", "SELF TABLE 2"),
 * because `processing_orders.table_number` is free text with no section
 * column: without the section, "1" in AC and "1" in NORMAL are the same table
 * to the counter and the second one opened silently becomes "1B". The phone
 * sends this string as the table number and the KOT and the bill print it
 * verbatim, so what the waiter taps is what the kitchen reads.
 */
export function composeTableName(
  section: string | null | undefined,
  label: string | null | undefined
): string {
  const s = String(section ?? "").trim();
  const l = String(label ?? "").trim();
  if (!s) return l;
  if (!l) return s;
  return `${s} ${l}`;
}

/**
 * Display form for the Processing Orders badge: a trailing sub-table letter is
 * split off ("6B" -> "6-B", "AC 1B" -> "AC 1-B") while the name itself is left
 * alone ("AC 1" stays "AC 1", "Counter" stays "Counter"). The old rule
 * hyphenated every run of letters, which turned "AC 1" into "-AC 1".
 */
export function formatTableBadge(tableNumber: string | null | undefined): string {
  const value = String(tableNumber ?? "");
  if (value.length < 2) return value;
  const last = value[value.length - 1];
  const beforeLast = value[value.length - 2];
  const isLetter = (c: string) => /[A-Za-z]/.test(c);
  if (!isLetter(last) || isLetter(beforeLast)) return value;
  return `${value.slice(0, -1)}-${last}`;
}

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
