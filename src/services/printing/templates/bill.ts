import { CMD, lineWidth, padLeft, padRight, styled, tokenBlock, twoColumns } from "../escpos";
import { receiptDate, receiptTime, formatAmount } from "../../../utils/format";
import type { AppSettings, CartItem } from "../../../types";

export interface BillPrintData {
  cart: CartItem[];
  subtotal: number;
  gst: number;
  total: number;
  billNumber: string;
  tokenNumber: number | null;
  orderType: string;
  tableNumber: string;
  customerName?: string;
  cashierName?: string;
  date: Date;
  /** When known (live checkout), renders "GST (5%)". Reprints omit the rate. */
  gstPercentage?: number;
  gstInclusive?: boolean;
}

/**
 * The one bill layout — used for live checkout printing AND reprints so both
 * always honor the same design settings.
 */
export function renderBillText(settings: AppSettings, data: BillPrintData): string {
  const { bill, store, printer } = settings;
  const width = lineWidth(printer.paperSize);
  const sep = "-".repeat(width);
  let text = "";

  // --- Header ---
  text += CMD.ALIGN_CENTER;
  if (store.hotelName) {
    text += `${styled(store.hotelName.toUpperCase(), bill.storeName.size, bill.storeName.bold)}\n`;
  }
  const meta = (line: string) => `${styled(line, bill.addressMeta.size, bill.addressMeta.bold, false)}\n`;
  if (bill.showAddress && store.address) text += meta(store.address);
  if (bill.showPhone && store.phoneNumber) text += meta(`Tel: ${store.phoneNumber}`);
  if (bill.showGstin && store.gstNumber) text += meta(`GSTIN: ${store.gstNumber}`);
  if (bill.showFssai && store.fssaiNumber) text += meta(`FSSAI: ${store.fssaiNumber}`);
  text += "\n";
  text += CMD.ALIGN_LEFT;
  if (bill.separators.header) text += `${sep}\n`;

  // --- Meta ---
  text += meta(twoColumns(`Bill No: ${data.billNumber}`, `Date: ${receiptDate(data.date)}`, width));
  let timeLine = padRight(`Time: ${receiptTime(data.date)}`, Math.floor(width / 2));
  if (bill.showCashier) timeLine += padLeft(`Cashier: ${data.cashierName || "Admin"}`, Math.ceil(width / 2));
  text += meta(timeLine);
  if (data.orderType === "Table" && data.tableNumber) {
    text += meta(padRight(`Order: Table ${data.tableNumber}`, width));
  } else if (data.orderType && data.orderType !== "Self Service") {
    text += meta(padRight(`Order: ${data.orderType}`, width));
  }
  if (data.customerName) text += meta(padRight(`Customer: ${data.customerName}`, width));
  if (bill.separators.meta) text += `${sep}\n`;

  // --- Token ---
  if (bill.showToken && data.tokenNumber != null) {
    text += tokenBlock(data.tokenNumber, printer.token.printSize);
    if (bill.separators.token) text += `${sep}\n`;
  }

  // --- Item table: Item (flex), Qty (4), Price (8), Amt (8) ---
  const tbl = bill.table;
  const itemWidth = width - 4 - 8 - 8 - 3;
  const row = (line: string) => `${styled(line, tbl.size, tbl.bold, false)}\n`;
  text += row(`${padRight("Item", itemWidth)} ${padLeft("Qty", 4)} ${padLeft("Price", 8)} ${padLeft("Amt", 8)}`);
  if (bill.separators.tableHeader) text += `${sep}\n`;

  data.cart.forEach((item) => {
    const name = String(item.name || "");
    const qty = padLeft(item.quantity ?? 1, 4);
    const price = padLeft(formatAmount(item.price), 8);
    const amount = padLeft(formatAmount((item.quantity || 1) * (item.price || 0)), 8);
    if (name.length > itemWidth) {
      // Long names get their own line; numbers go on the next.
      text += row(name);
      text += row(`${padRight("", itemWidth)} ${qty} ${price} ${amount}`);
    } else {
      text += row(`${padRight(name, itemWidth)} ${qty} ${price} ${amount}`);
    }
  });
  if (bill.separators.tableBody) text += `${sep}\n`;

  // --- Totals ---
  const totals = bill.totals;
  const totalLine = (line: string, forceBold = false) =>
    `${styled(line, totals.size, forceBold || totals.bold, false)}\n`;
  text += totalLine(`${padRight("Subtotal:", width - 12)}${padLeft(formatAmount(data.subtotal), 12)}`);
  if (data.gst > 0) {
    if (data.gstInclusive) {
      text += totalLine(`(Includes Rs. ${formatAmount(data.gst)} GST)`);
    } else {
      const label = data.gstPercentage !== undefined ? `GST (${data.gstPercentage}%):` : "GST:";
      text += totalLine(`${padRight(label, width - 12)}${padLeft(formatAmount(data.gst), 12)}`);
    }
  }
  if (bill.separators.subtotals) text += `${sep}\n`;

  // Grand total is always bold.
  text += totalLine(`${padRight("GRAND TOTAL:", width - 14)}${padLeft(`Rs. ${formatAmount(data.total)}`, 14)}`, true);
  if (bill.separators.grandTotal) text += `${sep}\n`;
  text += "\n";

  // --- Footer ---
  text += CMD.ALIGN_CENTER;
  text += `${styled(bill.footerMessage || "Thank you! Visit again.", bill.footer.size, bill.footer.bold)}\n\n`;
  text += CMD.ALIGN_LEFT;

  return text;
}
