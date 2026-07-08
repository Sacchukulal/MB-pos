import { invoke } from "@tauri-apps/api/core";
import { buildDocument, stripEscCodes } from "./escpos";
import { rasterizeImage, upiQrBytes } from "./image";
import { renderBillText, type BillPrintData } from "./templates/bill";
import { renderKotText, type KotPrintData } from "./templates/kot";
import type { AppSettings, CartItem, Category } from "../../types";

const INVOKE_TIMEOUT_MS = 15_000;

function invokeWithTimeout<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  return Promise.race([
    invoke<T>(cmd, args),
    new Promise<never>((_, reject) =>
      setTimeout(() => reject(new Error(`Printer did not respond (${cmd})`)), INVOKE_TIMEOUT_MS)
    ),
  ]);
}

export async function listPrinters(): Promise<string[]> {
  return invoke<string[]>("get_printers");
}

export async function printRaw(printerName: string, data: number[]): Promise<void> {
  await invokeWithTimeout("print_receipt_raw", { printerName, data });
}

export async function printText(printerName: string, text: string): Promise<void> {
  await invokeWithTimeout("print_receipt_text", { printerName, text });
}

/** Raw ESC/POS print with automatic plain-text fallback. Throws only if both fail. */
async function printWithFallback(printerName: string, data: number[], plainText: string): Promise<void> {
  try {
    await printRaw(printerName, data);
  } catch (rawError) {
    console.error("Raw print failed, trying text fallback:", rawError);
    await printText(printerName, stripEscCodes(plainText));
  }
}

export type PrintResult = { ok: true } | { ok: false; error: string };

function toError(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/* ------------------------------- bill ------------------------------ */

/** Prints the customer bill: template text + optional logo + optional UPI QR trailer. */
export async function printBill(settings: AppSettings, data: BillPrintData): Promise<PrintResult> {
  const { printer, bill, store } = settings;
  if (!printer.defaultPrinter) return { ok: false, error: "NO_PRINTER" };

  const text = renderBillText(settings, data);

  let logoBytes: number[] = [];
  if (bill.logo.base64 && bill.logo.position === "top") {
    try {
      logoBytes = await rasterizeImage(bill.logo.base64, bill.logo.sizePct, printer.paperSize);
    } catch (e) {
      console.error("Failed to rasterize logo:", e);
    }
  }

  // UPI QR trailer (printed after the footer, before the cut).
  let trailer: number[] = [];
  if (bill.qrMode !== "none" && store.upiId) {
    const qr = await upiQrBytes(
      {
        upiId: store.upiId,
        merchantName: store.merchantName || store.hotelName,
        reference: store.paymentReference,
        amount: bill.qrMode === "dynamic" ? data.total : undefined,
      },
      printer.paperSize
    );
    if (qr.length > 0) {
      trailer = [
        0x1b, 0x61, 0x01, // center
        ...Array.from(new TextEncoder().encode("Scan to Pay via UPI\n")),
        ...qr,
        0x0a, 0x0a, 0x0a, 0x0a,
      ];
    }
  }

  const doc = buildDocument(text, {
    printBold: printer.printBold,
    imageBytes: logoBytes,
    imagePosition: bill.logo.position === "top" ? "top" : "none",
    trailer,
  });

  try {
    await printWithFallback(printer.defaultPrinter, doc, text);
    return { ok: true };
  } catch (e) {
    return { ok: false, error: toError(e) };
  }
}

/* ------------------------------- KOT ------------------------------- */

export interface KotJobMeta {
  tokenNumber: number | string;
  billNumber: string;
  orderType: string;
  tableNumber: string;
}

/**
 * Routes KOT items to printers (per-category mapping in Multiple Printers mode,
 * default printer otherwise) and prints combined or category-wise tickets.
 * Returns NO_PRINTER if no printer could be resolved for any item.
 */
export async function printKot(
  settings: AppSettings,
  itemsToPrint: CartItem[],
  categories: Category[],
  meta: KotJobMeta
): Promise<PrintResult> {
  const { printer, categoryPrinters } = settings;

  // printerName -> categoryId -> items
  const jobs = new Map<string, Map<number, CartItem[]>>();
  itemsToPrint.forEach((item) => {
    let target = printer.defaultPrinter;
    if (printer.printerMode === "Multiple Printers") {
      target = categoryPrinters[item.category_id] || printer.defaultPrinter;
    }
    if (!target) return;
    if (!jobs.has(target)) jobs.set(target, new Map());
    const byCategory = jobs.get(target)!;
    if (!byCategory.has(item.category_id)) byCategory.set(item.category_id, []);
    byCategory.get(item.category_id)!.push(item);
  });

  if (jobs.size === 0) return { ok: false, error: "NO_PRINTER" };

  const date = new Date();
  let lastError = "";

  const printTicket = async (target: string, data: KotPrintData) => {
    const text = renderKotText(settings, data);
    const doc = buildDocument(text, { printBold: printer.printBold });
    try {
      await printWithFallback(target, doc, text);
    } catch (e) {
      lastError = toError(e);
    }
  };

  for (const [target, byCategory] of jobs.entries()) {
    if (printer.kotStyle === "Category-wise KOTs") {
      for (const [catId, items] of byCategory.entries()) {
        const categoryName = categories.find((c) => c.id === catId)?.name || "Items";
        await printTicket(target, { items, date, categoryName, ...meta });
      }
    } else {
      const items = Array.from(byCategory.values()).flat();
      await printTicket(target, { items, date, ...meta });
    }
  }

  return lastError ? { ok: false, error: lastError } : { ok: true };
}

/* ----------------------------- reports ----------------------------- */

/** Prints pre-rendered report/statement text on the default printer. */
export async function printReport(settings: AppSettings, text: string): Promise<PrintResult> {
  const { printer } = settings;
  if (!printer.defaultPrinter) return { ok: false, error: "NO_PRINTER" };
  const doc = buildDocument(text);
  try {
    await printWithFallback(printer.defaultPrinter, doc, text);
    return { ok: true };
  } catch (e) {
    return { ok: false, error: toError(e) };
  }
}

/** ESC/POS connectivity test slip. */
export async function printTestSlip(printerName: string): Promise<void> {
  const body =
    "\n" +
    "    *** TEST PRINT ***\n" +
    "         Magic Bill\n\n" +
    `  Printer: ${printerName}\n` +
    `  ${new Date().toLocaleString()}\n` +
    "  Connection OK!\n\n\n\n";
  await printRaw(printerName, buildDocument(body));
}
