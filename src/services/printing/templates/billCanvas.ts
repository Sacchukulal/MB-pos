import {
  columns,
  createDoc,
  image,
  leftRight,
  loadImage,
  measure,
  px,
  rowPadPx,
  separator,
  space,
  text,
  type Doc,
} from "../canvas";
import { clampSize } from "../fit";
import { formatAmount, receiptDate, receiptTime } from "../../../utils/format";
import type { AppSettings, TokenPrintSize } from "../../../types";
import type { BillPrintData } from "./bill";

/** Token line size relative to the body text, per the Printer Settings choice. */
export const TOKEN_SCALE: Record<TokenPrintSize, number> = {
  Normal: 1.3,
  Large: 1.8,
  "Extra Large": 2.4,
};

/**
 * Graphics bill: the on-screen preview drawn at printer resolution, so the
 * shop's font family, sizes and bold flags print exactly as configured.
 */
export async function renderBillDoc(settings: AppSettings, data: BillPrintData): Promise<Doc> {
  const { bill, store, printer } = settings;
  const paper = printer.paperSize;
  const line = bill.linePattern;
  const doc = createDoc(paper, bill.fontFamily, printer.printBold);
  const pad = rowPadPx(bill.rowHeight);

  const metaSize = clampSize(bill.addressMeta.size, "billMeta", bill.fontFamily, paper);
  const tableSize = clampSize(bill.table.size, "billTable", bill.fontFamily, paper);
  const subSize = clampSize(bill.subtotals.size, "billTotals", bill.fontFamily, paper);
  const grandSize = clampSize(bill.grandTotal.size, "billTotals", bill.fontFamily, paper);

  // --- Logo ---
  if (bill.logo.position === "top" && bill.logo.base64) {
    const img = await loadImage(bill.logo.base64);
    if (img) {
      image(doc, img, bill.logo.sizePct || 50);
      space(doc, 8);
    }
  }

  // --- Store header ---
  if (store.hotelName) {
    text(doc, store.hotelName.toUpperCase(), {
      size: bill.storeName.size,
      bold: bill.storeName.bold,
      align: "center",
    });
  }
  const headerLine = (value: string) => text(doc, value, { size: metaSize, bold: bill.addressMeta.bold, align: "center" });
  if (bill.showAddress && store.address) headerLine(store.address);
  if (bill.showPhone && store.phoneNumber) headerLine(`Tel: ${store.phoneNumber}`);
  if (bill.showGstin && store.gstNumber) headerLine(`GSTIN: ${store.gstNumber}`);
  if (bill.showFssai && store.fssaiNumber) headerLine(`FSSAI: ${store.fssaiNumber}`);
  if (bill.separators.header) separator(doc, line);

  // --- Meta ---
  const meta = { size: metaSize, bold: bill.addressMeta.bold };
  leftRight(doc, `Bill No: ${data.billNumber}`, `Date: ${receiptDate(data.date)}`, meta);
  leftRight(doc, `Time: ${receiptTime(data.date)}`, bill.showCashier ? `Cashier: ${data.cashierName || "Admin"}` : "", meta);
  if (data.orderType === "Table" && data.tableNumber) text(doc, `Order: Table ${data.tableNumber}`, meta);
  else if (data.orderType && data.orderType !== "Self Service") text(doc, `Order: ${data.orderType}`, meta);
  if (data.customerName) text(doc, `Customer: ${data.customerName}`, meta);
  if (bill.separators.meta) separator(doc, line);

  // --- Token ---
  if (bill.showToken && data.tokenNumber != null) {
    space(doc, 4);
    text(doc, `TOKEN: ${data.tokenNumber}`, {
      size: px(metaSize) * (TOKEN_SCALE[printer.token.printSize] ?? TOKEN_SCALE.Large),
      bold: true,
      align: "center",
    });
    space(doc, 4);
    if (bill.separators.token) separator(doc, line);
  }

  // --- Item table ---
  const style = { size: tableSize, bold: bill.table.bold };
  const rows = data.cart.map((item) => ({
    name: String(item.name || ""),
    qty: String(item.quantity ?? 1),
    price: formatAmount(item.price),
    amount: formatAmount((item.quantity || 1) * (item.price || 0)),
  }));

  // Number columns are sized to their widest value so they always line up.
  const gap = px(tableSize) * 0.5;
  const colWidth = (values: string[]) =>
    Math.max(...values.map((v) => measure(doc, v, tableSize, bill.table.bold))) + gap;
  const qtyW = colWidth(["Qty", ...rows.map((r) => r.qty)]);
  const priceW = colWidth(["Price", ...rows.map((r) => r.price)]);
  const amtW = colWidth(["Amt", ...rows.map((r) => r.amount)]);

  space(doc, pad);
  columns(
    doc,
    [
      { text: "Item" },
      { text: "Qty", align: "right", width: qtyW },
      { text: "Price", align: "right", width: priceW },
      { text: "Amt", align: "right", width: amtW },
    ],
    style
  );
  space(doc, pad);
  if (bill.separators.tableHeader) separator(doc, line, 2);

  rows.forEach((r) => {
    space(doc, pad);
    columns(
      doc,
      [
        { text: r.name },
        { text: r.qty, align: "right", width: qtyW },
        { text: r.price, align: "right", width: priceW },
        { text: r.amount, align: "right", width: amtW },
      ],
      style
    );
    space(doc, pad);
  });
  if (bill.separators.tableBody) separator(doc, line);

  // --- Totals ---
  const subStyle = { size: subSize, bold: bill.subtotals.bold };
  leftRight(doc, "Subtotal:", formatAmount(data.subtotal), subStyle);
  if (data.gst > 0) {
    if (data.gstInclusive) {
      text(doc, `(Includes Rs. ${formatAmount(data.gst)} GST)`, subStyle);
    } else {
      const label = data.gstPercentage !== undefined ? `GST (${data.gstPercentage}%):` : "GST:";
      leftRight(doc, label, formatAmount(data.gst), subStyle);
    }
  }
  if (bill.separators.subtotals) separator(doc, line);

  leftRight(doc, "GRAND TOTAL:", `Rs. ${formatAmount(data.total)}`, {
    size: grandSize,
    bold: bill.grandTotal.bold,
  });
  if (bill.separators.grandTotal) separator(doc, line);

  // --- Footer ---
  space(doc, 8);
  text(doc, bill.footerMessage || "Thank you! Visit again.", {
    size: bill.footer.size,
    bold: bill.footer.bold,
    align: "center",
  });
  space(doc, 12);

  return doc;
}
