import {
  CMD,
  lineWidth,
  padLeft,
  padRight,
  scaled,
  scaledWidth,
  separatorLine,
  tokenBlock,
  twoColumns,
  wrapText,
} from "../escpos";
import type { AppSettings, CartItem } from "../../../types";

export interface KotPrintData {
  items: CartItem[];
  tokenNumber: number | string;
  billNumber: string;
  orderType: string;
  tableNumber: string;
  date: Date;
  /** Set for category-wise tickets. */
  categoryName?: string;
  /** Mobile orders: waiter attribution + cancellation/reprint tickets. */
  waiterName?: string;
  variant?: "cancel" | "reprint";
  cancelReason?: string;
  cancelledBy?: string;
}

/** Kitchen order ticket layout, fully driven by KOT design settings. */
export function renderKotText(settings: AppSettings, data: KotPrintData): string {
  const { kot, bill, printer } = settings;
  const width = lineWidth(printer.paperSize);
  const sep = separatorLine(bill.linePattern, width);
  let text = "";

  if (kot.showToken) {
    text += tokenBlock(data.tokenNumber, printer.token.printSize);
    if (kot.separators.token) text += `${sep}\n`;
  }

  // Cancellation/reprint tickets always carry their marker, even when the
  // normal KOT title is hidden — the kitchen must never mistake them.
  const title =
    data.variant === "cancel" ? "*** CANCELLED ***" : data.variant === "reprint" ? "*** REPRINT ***" : "--- KOT ---";
  if (kot.showTitle || data.variant) {
    text += CMD.ALIGN_CENTER;
    text += `${scaled(title, kot.title.size, kot.title.bold)}\n`;
    if (data.categoryName) text += `[ ${data.categoryName} ]\n`;
    text += CMD.ALIGN_LEFT;
    if (kot.separators.header) text += `${sep}\n`;
  }

  // Meta block — per-content visibility with optional 2-column packing.
  // Bigger sizes eat columns, so the layout is measured in scaled columns.
  const metaWidth = scaledWidth(width, kot.meta.size);

  // Drop the seconds when the full timestamp no longer fits the scaled line.
  const fullDate = data.date.toLocaleString();
  const dateText =
    `Date: ${fullDate}`.length <= metaWidth
      ? fullDate
      : data.date.toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" });

  const metaParts: string[] = [];
  if (kot.showBillNo) metaParts.push(`Bill No: ${data.billNumber}`);
  if (kot.showOrderType) metaParts.push(`Order: ${data.orderType}`);
  if (kot.showTable && data.tableNumber) metaParts.push(`Table: ${data.tableNumber}`);
  if (kot.showDate) metaParts.push(`Date: ${dateText}`);
  if (kot.showWaiter && data.waiterName) metaParts.push(`Waiter: ${data.waiterName}`);

  const metaLine = (line: string) => `${scaled(line, kot.meta.size, kot.meta.bold)}\n`;
  const metaBlock = (part: string) => wrapText(part, metaWidth).forEach((l) => (text += metaLine(l)));

  if (metaParts.length > 0) {
    if (kot.metaTwoColumn) {
      for (let i = 0; i < metaParts.length; i += 2) {
        const left = metaParts[i];
        const right = metaParts[i + 1];
        // Fall back to full-width lines when a pair no longer fits side by side
        // (with at least one blank column between the halves).
        const pairFits =
          right &&
          left.length <= Math.floor(metaWidth / 2) &&
          right.length <= Math.ceil(metaWidth / 2) &&
          left.length + right.length < metaWidth;
        if (pairFits) text += metaLine(twoColumns(left, right, metaWidth));
        else {
          metaBlock(left);
          if (right) metaBlock(right);
        }
      }
    } else {
      metaParts.forEach(metaBlock);
    }
    if (kot.separators.meta) text += `${sep}\n`;
  }

  // Items: Item (flex) + Qty (4), laid out in the item size's scaled columns.
  const itemWidth = scaledWidth(width, kot.items.size);
  const qtyWidth = Math.min(4, Math.max(1, itemWidth - 2));
  const nameWidth = Math.max(1, itemWidth - qtyWidth - 1);
  const itemLine = (line: string) => `${scaled(line, kot.items.size, kot.items.bold)}\n`;
  const row = (name: string, qty: unknown) => `${padRight(name, nameWidth)} ${padLeft(qty, qtyWidth)}`;

  text += itemLine(row("Item", "Qty"));
  if (kot.separators.tableHeader) text += `${sep}\n`;

  // Wrapped names are indented so a continuation never reads as a new item.
  const contIndent = nameWidth >= 10 ? "  " : "";

  data.items.forEach((item) => {
    // Long names wrap instead of being cut off; the qty rides on the first line.
    const parts =
      item.name.length <= nameWidth
        ? [item.name]
        : wrapText(item.name, nameWidth - contIndent.length).map((p, i) => (i === 0 ? p : contIndent + p));
    parts.forEach((part, i) => {
      text += itemLine(row(part, i === 0 ? item.quantity : ""));
    });
    // Per-line note ("no onion") — indented so it reads as part of the item.
    if (item.note) {
      wrapText(`>> ${item.note}`, Math.max(1, nameWidth - contIndent.length)).forEach((l) => {
        text += itemLine(row(contIndent + l, ""));
      });
    }
  });
  if (kot.separators.tableBody) text += `${sep}\n`;

  // Cancellation footer: why + who, so the kitchen slip is self-explaining.
  if (data.variant === "cancel") {
    if (data.cancelReason) {
      wrapText(`Reason: ${data.cancelReason}`, metaWidth).forEach((l) => (text += metaLine(l)));
    }
    if (data.cancelledBy) text += metaLine(`By: ${data.cancelledBy}`);
  }

  text += "\n\n\n";
  text += CMD.ALIGN_LEFT;
  return text;
}
