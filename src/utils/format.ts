/** Shared user-facing formatting — the only place ₹/date formats are defined. */

export function formatCurrency(n: number): string {
  return `₹${(n || 0).toLocaleString("en-IN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

/** Plain 2-decimal amount without symbol (receipts, CSV). */
export function formatAmount(n: number): string {
  return (n || 0).toFixed(2);
}

export function formatPercent(part: number, whole: number): string {
  return whole > 0 ? `${((part / whole) * 100).toFixed(1)}%` : "0%";
}

/** Local calendar date as ISO YYYY-MM-DD (locale-independent — used for daily resets). */
export function todayISO(): string {
  const d = new Date();
  const p = (v: number) => String(v).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** e.g. "26-Feb-2026" — receipt date format. */
export function receiptDate(d: Date): string {
  return d
    .toLocaleDateString("en-GB", { day: "2-digit", month: "short", year: "numeric" })
    .replace(/ /g, "-");
}

/** e.g. "12:30 PM" — receipt time format. */
export function receiptTime(d: Date): string {
  return d.toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: true });
}

/** e.g. "26 Feb 2026, 12:30 pm" — on-screen date-times. */
export function displayDateTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  return d.toLocaleString("en-GB", {
    day: "2-digit",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: true,
  });
}

export function displayDate(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  return d.toLocaleDateString("en-GB", { day: "2-digit", month: "short", year: "numeric" });
}
