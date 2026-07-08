import { CMD, lineWidth, padLeft, padRight, styled, tokenBlock, twoColumns } from "../escpos";
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
}

/** Kitchen order ticket layout, fully driven by KOT design settings. */
export function renderKotText(settings: AppSettings, data: KotPrintData): string {
  const { kot, printer } = settings;
  const width = lineWidth(printer.paperSize);
  const sep = "-".repeat(width);
  let text = "";

  if (kot.showToken) {
    text += tokenBlock(data.tokenNumber, printer.token.printSize);
    if (kot.separators.token) text += `${sep}\n`;
  }

  if (kot.showTitle) {
    text += CMD.ALIGN_CENTER;
    text += `${styled("--- KOT ---", kot.title.size, kot.title.bold)}\n`;
    if (data.categoryName) text += `[ ${data.categoryName} ]\n`;
    text += CMD.ALIGN_LEFT;
    if (kot.separators.header) text += `${sep}\n`;
  }

  // Meta block — per-content visibility with optional 2-column packing.
  const metaParts: string[] = [];
  if (kot.showBillNo) metaParts.push(`Bill No: ${data.billNumber}`);
  if (kot.showOrderType) metaParts.push(`Order: ${data.orderType}`);
  if (kot.showTable && data.tableNumber) metaParts.push(`Table: ${data.tableNumber}`);
  if (kot.showDate) metaParts.push(`Date: ${data.date.toLocaleString()}`);

  const metaLine = (line: string) => `${styled(line, kot.meta.size, kot.meta.bold, false)}\n`;
  if (metaParts.length > 0) {
    if (kot.metaTwoColumn) {
      for (let i = 0; i < metaParts.length; i += 2) {
        const right = metaParts[i + 1];
        text += metaLine(right ? twoColumns(metaParts[i], right, width) : metaParts[i]);
      }
    } else {
      metaParts.forEach((m) => (text += metaLine(m)));
    }
    if (kot.separators.meta) text += `${sep}\n`;
  }

  // Items: Item (flex) + Qty (4)
  const itemLine = (line: string) => `${styled(line, kot.items.size, kot.items.bold, false)}\n`;
  text += itemLine(`${padRight("Item", width - 5)} ${padLeft("Qty", 4)}`);
  if (kot.separators.tableHeader) text += `${sep}\n`;

  data.items.forEach((item) => {
    text += itemLine(`${padRight(item.name, width - 5)} ${padLeft(item.quantity, 4)}`);
  });
  if (kot.separators.tableBody) text += `${sep}\n`;

  text += "\n\n\n";
  text += CMD.ALIGN_LEFT;
  return text;
}
