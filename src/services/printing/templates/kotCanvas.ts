import { columns, createDoc, leftRight, measure, px, rowPadPx, separator, space, text, type Doc } from "../canvas";
import { clampSize } from "../fit";
import { TOKEN_SCALE } from "./billCanvas";
import type { AppSettings } from "../../../types";
import type { KotPrintData } from "./kot";

/** Graphics KOT: the KOT preview drawn at printer resolution. */
export function renderKotDoc(settings: AppSettings, data: KotPrintData): Doc {
  const { kot, bill, printer } = settings;
  const paper = printer.paperSize;
  const line = bill.linePattern;
  const doc = createDoc(paper, bill.fontFamily, printer.printBold);
  const pad = rowPadPx(kot.rowHeight);

  const metaSize = clampSize(kot.meta.size, "kotMeta", bill.fontFamily, paper);
  const itemSize = clampSize(kot.items.size, "kotItems", bill.fontFamily, paper);

  if (kot.showToken) {
    text(doc, `TOKEN: ${data.tokenNumber}`, {
      size: px(metaSize) * (TOKEN_SCALE[printer.token.printSize] ?? TOKEN_SCALE.Large),
      bold: true,
      align: "center",
    });
    space(doc, 4);
    if (kot.separators.token) separator(doc, line);
  }

  if (kot.showTitle) {
    text(doc, "--- KOT ---", { size: kot.title.size, bold: kot.title.bold, align: "center" });
    if (data.categoryName) {
      text(doc, `[ ${data.categoryName} ]`, { size: kot.title.size, bold: kot.title.bold, align: "center" });
    }
    if (kot.separators.header) separator(doc, line);
  }

  // --- Details ---
  const metaStyle = { size: metaSize, bold: kot.meta.bold };
  const metaParts: string[] = [];
  if (kot.showBillNo) metaParts.push(`Bill No: ${data.billNumber}`);
  if (kot.showOrderType) metaParts.push(`Order: ${data.orderType}`);
  if (kot.showTable && data.tableNumber) metaParts.push(`Table: ${data.tableNumber}`);
  if (kot.showDate) metaParts.push(`Date: ${data.date.toLocaleString()}`);

  if (metaParts.length > 0) {
    if (kot.metaTwoColumn) {
      for (let i = 0; i < metaParts.length; i += 2) {
        const left = metaParts[i];
        const right = metaParts[i + 1] ?? "";
        // Pair them only while both halves genuinely fit on one line.
        const fits =
          right &&
          measure(doc, left, metaSize, kot.meta.bold) + measure(doc, right, metaSize, kot.meta.bold) <
            doc.width - px(metaSize) * 0.5;
        if (fits) leftRight(doc, left, right, metaStyle);
        else {
          text(doc, left, metaStyle);
          if (right) text(doc, right, metaStyle);
        }
      }
    } else {
      metaParts.forEach((m) => text(doc, m, metaStyle));
    }
    if (kot.separators.meta) separator(doc, line);
  }

  // --- Items ---
  const style = { size: itemSize, bold: kot.items.bold };
  const qtyW =
    Math.max(
      measure(doc, "Qty", itemSize, kot.items.bold),
      ...data.items.map((i) => measure(doc, String(i.quantity), itemSize, kot.items.bold))
    ) +
    px(itemSize) * 0.5;

  space(doc, pad);
  columns(doc, [{ text: "Item" }, { text: "Qty", align: "right", width: qtyW }], style);
  space(doc, pad);
  if (kot.separators.tableHeader) separator(doc, line, 2);

  data.items.forEach((item) => {
    space(doc, pad);
    columns(doc, [{ text: item.name }, { text: String(item.quantity), align: "right", width: qtyW }], style);
    space(doc, pad);
  });
  if (kot.separators.tableBody) separator(doc, line);

  space(doc, 12);
  return doc;
}
