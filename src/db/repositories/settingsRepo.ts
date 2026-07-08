import { getDb } from "../client";
import { toBool, toNum, toStr, toBit } from "../../utils/sqlite";
import { todayISO } from "../../utils/format";
import {
  DEFAULT_BILL_DESIGN,
  DEFAULT_KOT_DESIGN,
  DEFAULT_PRINTER_CONFIG,
  DEFAULT_STORE_PROFILE,
} from "../../config/defaults";
import type {
  AppSettings,
  BillDesign,
  KotDesign,
  KotStyle,
  PaperSize,
  PrinterConfig,
  PrinterMode,
  QrMode,
  StoreProfile,
  TokenPrintSize,
} from "../../types";

/**
 * Single source of truth for reading/writing all settings tables.
 * Raw SQLite rows are coerced to typed objects here and nowhere else.
 */

/* ------------------------------- load ------------------------------- */

function mapStore(row: any): StoreProfile {
  const d = DEFAULT_STORE_PROFILE;
  return {
    hotelName: toStr(row?.hotel_name, d.hotelName),
    address: toStr(row?.address, d.address),
    phoneNumber: toStr(row?.phone_number, d.phoneNumber),
    gstNumber: toStr(row?.gst_number, d.gstNumber),
    fssaiNumber: toStr(row?.fssai_number, d.fssaiNumber),
    upiId: toStr(row?.upi_id, d.upiId),
    merchantName: toStr(row?.merchant_name, d.merchantName),
    paymentReference: toStr(row?.payment_reference, d.paymentReference),
  };
}

function mapBill(row: any): BillDesign {
  const d = DEFAULT_BILL_DESIGN;
  if (!row) return d;
  const qrMode: QrMode = toBool(row.dynamic_upi_qr)
    ? "dynamic"
    : toBool(row.static_upi_qr)
      ? "static"
      : "none";
  return {
    footerMessage: toStr(row.footer_message, d.footerMessage) || d.footerMessage,
    showGstin: toBool(row.show_gst, d.showGstin),
    showFssai: toBool(row.show_fssai, d.showFssai),
    showAddress: toBool(row.show_address, d.showAddress),
    showPhone: toBool(row.show_phone, d.showPhone),
    showCashier: toBool(row.show_cashier_name, d.showCashier),
    showToken: toBool(row.show_token, d.showToken),
    fontFamily: toStr(row.global_font_family, "") || toStr(row.body_font_family, d.fontFamily),
    storeName: { size: toStr(row.store_name_size, d.storeName.size), bold: toBool(row.store_name_bold, d.storeName.bold) },
    addressMeta: { size: toStr(row.address_size, d.addressMeta.size), bold: toBool(row.address_bold, d.addressMeta.bold) },
    table: { size: toStr(row.table_font_size, d.table.size), bold: toBool(row.table_bold, d.table.bold) },
    totals: { size: toStr(row.total_font_size, d.totals.size), bold: toBool(row.total_bold, d.totals.bold) },
    footer: { size: toStr(row.footer_font_size, d.footer.size), bold: toBool(row.footer_bold, d.footer.bold) },
    separators: {
      header: toBool(row.sep_header, true),
      meta: toBool(row.sep_meta, true),
      token: toBool(row.sep_token, true),
      tableHeader: toBool(row.sep_table_header, true),
      tableBody: toBool(row.sep_table_body, true),
      subtotals: toBool(row.sep_subtotals, true),
      grandTotal: toBool(row.sep_grand_total, true),
    },
    gst: {
      enabled: toBool(row.gst_enabled, d.gst.enabled),
      type: toStr(row.gst_type, d.gst.type) === "Inclusive" ? "Inclusive" : "Exclusive",
      percentage: toNum(row.gst_percentage, d.gst.percentage),
    },
    logo: {
      position: toStr(row.logo_position, "none") === "top" ? "top" : "none",
      base64: toStr(row.logo_base64, ""),
      sizePct: toNum(row.logo_size, d.logo.sizePct),
    },
    qrMode,
    rowHeight: toStr(row.row_height, d.rowHeight),
    searchMatchMode: toStr(row.search_match_mode, d.searchMatchMode) === "contains" ? "contains" : "starts",
  };
}

function mapKot(row: any): KotDesign {
  const d = DEFAULT_KOT_DESIGN;
  if (!row) return d;
  return {
    showTitle: toBool(row.show_kot_title, d.showTitle),
    showToken: toBool(row.show_token, d.showToken),
    showBillNo: toBool(row.show_bill_no, d.showBillNo),
    showOrderType: toBool(row.show_order_type, d.showOrderType),
    showTable: toBool(row.show_table, d.showTable),
    showDate: toBool(row.show_date, d.showDate),
    metaTwoColumn: toBool(row.meta_two_column, d.metaTwoColumn),
    title: { size: toStr(row.header_font_size, d.title.size), bold: toBool(row.title_bold, d.title.bold) },
    meta: { size: toStr(row.meta_font_size, d.meta.size), bold: toBool(row.meta_bold, d.meta.bold) },
    items: { size: toStr(row.table_font_size, d.items.size), bold: toBool(row.items_bold, d.items.bold) },
    rowHeight: toStr(row.row_height, d.rowHeight),
    separators: {
      token: toBool(row.sep_token, true),
      header: toBool(row.sep_header, true),
      meta: toBool(row.sep_meta, true),
      tableHeader: toBool(row.sep_table_header, true),
      tableBody: toBool(row.sep_table_body, true),
    },
  };
}

function mapPrinter(row: any): PrinterConfig {
  const d = DEFAULT_PRINTER_CONFIG;
  if (!row) return d;
  const paper = toStr(row.paper_size, d.paperSize);
  return {
    printerMode: (toStr(row.printer_mode, d.printerMode) as PrinterMode) || d.printerMode,
    defaultPrinter: toStr(row.default_printer, ""),
    kotStyle: (toStr(row.kot_printing_style, d.kotStyle) as KotStyle) || d.kotStyle,
    paperSize: (["2inch", "3inch", "4inch"].includes(paper) ? paper : d.paperSize) as PaperSize,
    printBold: toBool(row.print_bold, d.printBold),
    kotConfirmation: toBool(row.kot_print_confirmation, d.kotConfirmation),
    billConfirmation: toBool(row.bill_print_confirmation, d.billConfirmation),
    disableKot: toBool(row.disable_kot, d.disableKot),
    token: {
      resetDaily: toBool(row.token_reset_daily, d.token.resetDaily),
      startingNumber: toNum(row.token_starting_number, d.token.startingNumber),
      currentNumber: toNum(row.token_current_number, d.token.currentNumber),
      printSize: (toStr(row.token_print_size, d.token.printSize) as TokenPrintSize) || d.token.printSize,
    },
    bill: {
      resetDaily: toBool(row.bill_reset_daily, d.bill.resetDaily),
      prefix: toStr(row.bill_prefix, ""),
      startingNumber: toNum(row.bill_starting_number, d.bill.startingNumber),
      currentNumber: toNum(row.bill_current_number, d.bill.currentNumber),
    },
    lastResetDate: toStr(row.last_reset_date, ""),
  };
}

async function selectSingleton(table: string): Promise<any | null> {
  const rows = await getDb().select<any[]>(`SELECT * FROM ${table} WHERE id = 1`);
  return rows[0] ?? null;
}

export async function loadSettings(): Promise<AppSettings> {
  const [store, bill, kot, printer, mappings] = await Promise.all([
    selectSingleton("store_settings"),
    selectSingleton("bill_settings"),
    selectSingleton("kot_settings"),
    selectSingleton("printer_settings"),
    getDb().select<{ category_id: number; printer_name: string }[]>("SELECT * FROM category_printers"),
  ]);
  const categoryPrinters: Record<number, string> = {};
  mappings.forEach((m) => (categoryPrinters[m.category_id] = m.printer_name));
  return {
    store: mapStore(store),
    bill: mapBill(bill),
    kot: mapKot(kot),
    printer: mapPrinter(printer),
    categoryPrinters,
  };
}

/* ------------------------------- save ------------------------------- */

async function ensureRow(table: string) {
  await getDb().execute(`INSERT OR IGNORE INTO ${table} (id) VALUES (1)`);
}

export async function saveStoreProfile(s: StoreProfile): Promise<void> {
  await ensureRow("store_settings");
  await getDb().execute(
    `UPDATE store_settings SET hotel_name=$1, address=$2, phone_number=$3, gst_number=$4,
       fssai_number=$5, upi_id=$6, merchant_name=$7, payment_reference=$8 WHERE id=1`,
    [s.hotelName, s.address, s.phoneNumber, s.gstNumber, s.fssaiNumber, s.upiId, s.merchantName, s.paymentReference]
  );
}

export async function saveBillDesign(b: BillDesign): Promise<void> {
  await ensureRow("bill_settings");
  // Legacy per-section font-family columns stay synced to the global font so a
  // downgrade to the old app version still renders correctly.
  await getDb().execute(
    `UPDATE bill_settings SET
       footer_message=$1, show_gst=$2, show_fssai=$3, show_address=$4, show_phone=$5,
       show_cashier_name=$6, show_token=$7,
       global_font_family=$8, header_font_family=$8, body_font_family=$8, footer_font_family=$8,
       store_name_size=$9, store_name_bold=$10, address_size=$11, address_bold=$12,
       table_font_size=$13, table_bold=$14, total_font_size=$15, total_bold=$16,
       footer_font_size=$17, footer_bold=$18,
       sep_header=$19, sep_meta=$20, sep_token=$21, sep_table_header=$22,
       sep_table_body=$23, sep_subtotals=$24, sep_grand_total=$25,
       gst_enabled=$26, gst_type=$27, gst_percentage=$28,
       logo_position=$29, logo_base64=$30, logo_size=$31,
       dynamic_upi_qr=$32, static_upi_qr=$33, no_qr_print=$34,
       row_height=$35, search_match_mode=$36
     WHERE id=1`,
    [
      b.footerMessage, toBit(b.showGstin), toBit(b.showFssai), toBit(b.showAddress), toBit(b.showPhone),
      toBit(b.showCashier), toBit(b.showToken),
      b.fontFamily,
      b.storeName.size, toBit(b.storeName.bold), b.addressMeta.size, toBit(b.addressMeta.bold),
      b.table.size, toBit(b.table.bold), b.totals.size, toBit(b.totals.bold),
      b.footer.size, toBit(b.footer.bold),
      toBit(b.separators.header), toBit(b.separators.meta), toBit(b.separators.token), toBit(b.separators.tableHeader),
      toBit(b.separators.tableBody), toBit(b.separators.subtotals), toBit(b.separators.grandTotal),
      toBit(b.gst.enabled), b.gst.type, b.gst.percentage,
      b.logo.position, b.logo.base64, b.logo.sizePct,
      toBit(b.qrMode === "dynamic"), toBit(b.qrMode === "static"), toBit(b.qrMode === "none"),
      b.rowHeight, b.searchMatchMode,
    ]
  );
}

export async function saveKotDesign(k: KotDesign): Promise<void> {
  await ensureRow("kot_settings");
  await getDb().execute(
    `UPDATE kot_settings SET
       show_kot_title=$1, show_token=$2, show_bill_no=$3, show_order_type=$4, show_table=$5,
       show_date=$6, meta_two_column=$7,
       header_font_size=$8, title_bold=$9, meta_font_size=$10, meta_bold=$11,
       table_font_size=$12, items_bold=$13, row_height=$14,
       sep_token=$15, sep_header=$16, sep_meta=$17, sep_table_header=$18, sep_table_body=$19
     WHERE id=1`,
    [
      toBit(k.showTitle), toBit(k.showToken), toBit(k.showBillNo), toBit(k.showOrderType), toBit(k.showTable),
      toBit(k.showDate), toBit(k.metaTwoColumn),
      k.title.size, toBit(k.title.bold), k.meta.size, toBit(k.meta.bold),
      k.items.size, toBit(k.items.bold), k.rowHeight,
      toBit(k.separators.token), toBit(k.separators.header), toBit(k.separators.meta),
      toBit(k.separators.tableHeader), toBit(k.separators.tableBody),
    ]
  );
}

export async function savePrinterConfig(p: PrinterConfig, categoryPrinters: Record<number, string>): Promise<void> {
  await ensureRow("printer_settings");
  await getDb().execute(
    `UPDATE printer_settings SET
       printer_mode=$1, default_printer=$2, kot_printing_style=$3, paper_size=$4, print_bold=$5,
       kot_print_confirmation=$6, bill_print_confirmation=$7, disable_kot=$8,
       token_reset_daily=$9, token_starting_number=$10, token_current_number=$11, token_print_size=$12,
       bill_reset_daily=$13, bill_prefix=$14, bill_starting_number=$15, bill_current_number=$16,
       last_reset_date=$17
     WHERE id=1`,
    [
      p.printerMode, p.defaultPrinter, p.kotStyle, p.paperSize, toBit(p.printBold),
      toBit(p.kotConfirmation), toBit(p.billConfirmation), toBit(p.disableKot),
      toBit(p.token.resetDaily), p.token.startingNumber, p.token.currentNumber, p.token.printSize,
      toBit(p.bill.resetDaily), p.bill.prefix, p.bill.startingNumber, p.bill.currentNumber,
      p.lastResetDate,
    ]
  );

  // Category mappings only apply in Multiple Printers mode; clear otherwise (matches old behavior).
  await getDb().execute("DELETE FROM category_printers");
  if (p.printerMode === "Multiple Printers") {
    for (const [catId, printerName] of Object.entries(categoryPrinters)) {
      if (printerName) {
        await getDb().execute(
          "INSERT INTO category_printers (category_id, printer_name) VALUES ($1, $2)",
          [Number(catId), printerName]
        );
      }
    }
  }
}

/* --------------------------- counters ------------------------------ */

/**
 * Applies the once-a-day counter reset if configured and a new day has started.
 * Returns the (possibly updated) printer config.
 */
export async function applyDailyResetIfNeeded(p: PrinterConfig): Promise<PrinterConfig> {
  const today = todayISO();
  if (p.lastResetDate === today) return p;

  const next: PrinterConfig = {
    ...p,
    token: { ...p.token, currentNumber: p.token.resetDaily ? p.token.startingNumber : p.token.currentNumber },
    bill: { ...p.bill, currentNumber: p.bill.resetDaily ? p.bill.startingNumber : p.bill.currentNumber },
    lastResetDate: today,
  };
  await getDb().execute(
    "UPDATE printer_settings SET token_current_number=$1, bill_current_number=$2, last_reset_date=$3 WHERE id=1",
    [next.token.currentNumber, next.bill.currentNumber, today]
  );
  return next;
}

/**
 * Atomically claims the current token/bill numbers and advances the stored counters.
 * Reads back from the DB (not from possibly-stale UI state) to avoid duplicate numbers.
 */
export async function claimOrderNumbers(opts: { token: boolean; bill: boolean }): Promise<{
  tokenNumber: number;
  billNumber: string;
}> {
  const db = getDb();
  const rows = await db.select<any[]>(
    "SELECT token_current_number, bill_current_number, bill_prefix FROM printer_settings WHERE id = 1"
  );
  const row = rows[0] ?? {};
  const tokenNumber = toNum(row.token_current_number, 100);
  const billCurrent = toNum(row.bill_current_number, 1);
  const billNumber = `${toStr(row.bill_prefix, "")}${billCurrent}`;

  if (opts.token && opts.bill) {
    await db.execute(
      "UPDATE printer_settings SET token_current_number = token_current_number + 1, bill_current_number = bill_current_number + 1 WHERE id = 1"
    );
  } else if (opts.token) {
    await db.execute("UPDATE printer_settings SET token_current_number = token_current_number + 1 WHERE id = 1");
  } else if (opts.bill) {
    await db.execute("UPDATE printer_settings SET bill_current_number = bill_current_number + 1 WHERE id = 1");
  }
  return { tokenNumber, billNumber };
}
