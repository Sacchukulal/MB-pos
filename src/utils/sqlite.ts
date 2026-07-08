/**
 * SQLite value coercion — the single place raw row values become typed.
 * SQLite booleans arrive as 0/1 (sometimes "0"/"1" or true/false depending on driver path).
 */

export function toBool(v: unknown, fallback = false): boolean {
  if (v === null || v === undefined) return fallback;
  return v !== 0 && v !== false && v !== "0" && v !== "";
}

export function toNum(v: unknown, fallback: number): number {
  const n = Number(v);
  return Number.isFinite(n) ? n : fallback;
}

export function toStr(v: unknown, fallback = ""): string {
  return v === null || v === undefined ? fallback : String(v);
}

export function toBit(b: boolean): number {
  return b ? 1 : 0;
}
