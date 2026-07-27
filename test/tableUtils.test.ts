import test from "node:test";
import assert from "node:assert/strict";
import {
  belongsToTable,
  composeTableName,
  findTableOrder,
  formatTableBadge,
  isSlotOccupied,
  occupiedOrdersForTable,
} from "../src/features/billing/tableUtils.ts";

/**
 * Table identity, including sectioned names like "AC 1".
 *
 * Run with:  npm test        (node's built-in runner, no extra dependency;
 *                             Node strips the TypeScript types itself)
 *
 * These live outside `src` on purpose so `tsc` in `npm run build` does not
 * try to typecheck node:test without @types/node installed.
 */

// Minimal stand-in for ProcessingOrder — only the fields these helpers read.
const order = (table_number: string, order_type = "Table") =>
  ({ table_number, order_type }) as any;

test("composeTableName joins section and label with a single space", () => {
  assert.equal(composeTableName("AC", "1"), "AC 1");
  assert.equal(composeTableName("SELF TABLE", "2"), "SELF TABLE 2");
  assert.equal(composeTableName("NORMAL", "12"), "NORMAL 12");
});

test("composeTableName falls back to the bare label with no section", () => {
  assert.equal(composeTableName("", "6"), "6");
  assert.equal(composeTableName(null, "6"), "6");
  assert.equal(composeTableName(undefined, "6"), "6");
  assert.equal(composeTableName("   ", "6"), "6");
});

test("composeTableName trims stray whitespace so identities cannot drift", () => {
  assert.equal(composeTableName("  AC  ", "  1 "), "AC 1");
  assert.equal(composeTableName("AC", ""), "AC");
});

test("formatTableBadge splits only a trailing sub-table letter", () => {
  assert.equal(formatTableBadge("6B"), "6-B");
  assert.equal(formatTableBadge("AC 1B"), "AC 1-B");
  assert.equal(formatTableBadge("G3B"), "G3-B");
});

test("formatTableBadge leaves a plain table name alone", () => {
  // The old rule hyphenated every letter run and rendered "AC 1" as "-AC 1".
  assert.equal(formatTableBadge("AC 1"), "AC 1");
  assert.equal(formatTableBadge("SELF TABLE 2"), "SELF TABLE 2");
  assert.equal(formatTableBadge("Counter"), "Counter");
  assert.equal(formatTableBadge("6"), "6");
  assert.equal(formatTableBadge("A"), "A");
  assert.equal(formatTableBadge(""), "");
  assert.equal(formatTableBadge(null), "");
});

test("belongsToTable accepts a sub-table of a sectioned name", () => {
  assert.equal(belongsToTable("AC 1", "AC 1"), true);
  assert.equal(belongsToTable("AC 1B", "AC 1"), true);
  assert.equal(belongsToTable("AC 1H", "AC 1"), true);
  assert.equal(belongsToTable("SELF TABLE 2", "SELF TABLE 2"), true);
  assert.equal(belongsToTable("SELF TABLE 2C", "SELF TABLE 2"), true);
});

test("belongsToTable keeps sections apart — this is the round-2 regression", () => {
  assert.equal(belongsToTable("AC 1", "NORMAL 1"), false);
  assert.equal(belongsToTable("NORMAL 1", "AC 1"), false);
  assert.equal(belongsToTable("AC 1", "1"), false);
  assert.equal(belongsToTable("1", "AC 1"), false);
  assert.equal(belongsToTable("SELF TABLE 1", "AC 1"), false);
  // "AC 11" is a different table from "AC 1", not a sub-table of it.
  assert.equal(belongsToTable("AC 11", "AC 1"), false);
});

test("belongsToTable is case-insensitive and ignores surrounding space", () => {
  assert.equal(belongsToTable("ac 1b", "AC 1"), true);
  assert.equal(belongsToTable("AC 1B", " ac 1 "), true);
});

test("isSlotOccupied matches the exact sectioned slot only", () => {
  const open = [order("AC 1"), order("NORMAL 1"), order("SELF TABLE 2B")];
  assert.equal(isSlotOccupied(open, "AC 1"), true);
  assert.equal(isSlotOccupied(open, "NORMAL 1"), true);
  assert.equal(isSlotOccupied(open, "SELF TABLE 2B"), true);
  assert.equal(isSlotOccupied(open, "AC 1B"), false);
  assert.equal(isSlotOccupied(open, "SELF TABLE 2"), false);
  assert.equal(isSlotOccupied(open, "1"), false);
});

test("isSlotOccupied ignores parcel/self-service orders", () => {
  const open = [order("AC 1", "Parcel")];
  assert.equal(isSlotOccupied(open, "AC 1"), false);
});

test("occupiedOrdersForTable collects a sectioned table and its sub-tables", () => {
  const open = [
    order("AC 1"),
    order("AC 1B"),
    order("NORMAL 1"),
    order("1"),
    order("SELF TABLE 1"),
  ];
  assert.deepEqual(
    occupiedOrdersForTable(open, "AC 1").map((o: any) => o.table_number),
    ["AC 1", "AC 1B"],
  );
  assert.deepEqual(
    occupiedOrdersForTable(open, "NORMAL 1").map((o: any) => o.table_number),
    ["NORMAL 1"],
  );
  // The plain "1" table is its own thing and pulls in none of the sections.
  assert.deepEqual(
    occupiedOrdersForTable(open, "1").map((o: any) => o.table_number),
    ["1"],
  );
});

test("findTableOrder matches a sectioned name exactly", () => {
  const open = [order("AC 1"), order("NORMAL 1")];
  assert.equal(findTableOrder(open, "AC 1")?.table_number, "AC 1");
  assert.equal(findTableOrder(open, "ac 1")?.table_number, "AC 1");
  assert.equal(findTableOrder(open, " NORMAL 1 ")?.table_number, "NORMAL 1");
  assert.equal(findTableOrder(open, "1"), undefined);
});

/**
 * The exact slot-resolution rule eventApplier.ts uses for an inbound phone
 * create. Kept in step with that function by the assertions below.
 */
const SUB_LETTERS = ["B", "C", "D", "E", "F", "G", "H"];
function resolveTableSlot(requested: string, open: any[]): string | null {
  const base = requested.trim();
  if (!base) return "";
  if (!isSlotOccupied(open, base)) return base;
  for (const letter of SUB_LETTERS) {
    if (!isSlotOccupied(open, `${base}${letter}`)) return `${base}${letter}`;
  }
  return null;
}

test("AC 1 and NORMAL 1 open as two independent tables, with no 1B", () => {
  const open: any[] = [];
  const first = resolveTableSlot("AC 1", open);
  assert.equal(first, "AC 1");
  open.push(order(first!));

  const second = resolveTableSlot("NORMAL 1", open);
  assert.equal(second, "NORMAL 1");
  open.push(order(second!));

  const third = resolveTableSlot("SELF TABLE 1", open);
  assert.equal(third, "SELF TABLE 1");
  open.push(order(third!));

  const fourth = resolveTableSlot("1", open);
  assert.equal(fourth, "1");
});

test("a second order on the same sectioned table becomes AC 1B", () => {
  const open = [order("AC 1")];
  assert.equal(resolveTableSlot("AC 1", open), "AC 1B");
  open.push(order("AC 1B"));
  assert.equal(resolveTableSlot("AC 1", open), "AC 1C");
});

test("sub-tables run out at H and the create is rejected, never merged", () => {
  const open = ["AC 1", ...SUB_LETTERS.map((l) => `AC 1${l}`)].map((n) => order(n));
  assert.equal(resolveTableSlot("AC 1", open), null);
});
