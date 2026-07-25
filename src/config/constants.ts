import type { LinePattern, OrderType, PaperSize, PaymentMode } from "../types";

/** Thermal paper geometry: character columns for text layout, dot width for images. */
export const PAPER: Record<PaperSize, { columns: number; dots: number; previewPx: number }> = {
  "2inch": { columns: 32, dots: 384, previewPx: 260 },
  "3inch": { columns: 48, dots: 576, previewPx: 320 },
  "4inch": { columns: 64, dots: 800, previewPx: 380 },
};

/** Padding inside .paper-preview — the preview's usable content width is previewPx - 2x this. */
export const PREVIEW_PADDING_PX = 20;
/** Unprinted margin kept at each edge of the paper, in printer dots. */
export const PRINT_MARGIN_DOTS = 8;

/**
 * Preview-pixel → printer-dot scale. Derived from the two content widths so a
 * layout that fits the on-screen preview always fits the paper, and vice versa.
 */
export function printScale(paperSize: PaperSize): number {
  const paper = PAPER[paperSize] ?? PAPER["3inch"];
  return (paper.dots - 2 * PRINT_MARGIN_DOTS) / (paper.previewPx - 2 * PREVIEW_PADDING_PX);
}

/** Usable width of the preview paper, in CSS px — the layout budget for both preview and print. */
export function contentWidthPx(paperSize: PaperSize): number {
  return (PAPER[paperSize] ?? PAPER["3inch"]).previewPx - 2 * PREVIEW_PADDING_PX;
}

export const LINE_PATTERNS: { value: LinePattern; label: string }[] = [
  { value: "dashed", label: "Dashed  – – – –" },
  { value: "dotted", label: "Dotted  · · · ·" },
  { value: "solid", label: "Solid  ————" },
  { value: "bold", label: "Bold Solid  ▬▬▬▬" },
  { value: "double", label: "Double  ═══" },
];

export const ORDER_TYPES: OrderType[] = ["Self Service", "Table", "Parcel"];
export const PAYMENT_MODES: PaymentMode[] = ["Cash", "Card", "UPI", "Credit"];

/** Days a subscription keeps working past its next billing date. */
export const GRACE_PERIOD_DAYS = 10;

export const GST_PERCENTAGES = [5, 12, 18];

export const EXPENSE_CATEGORIES = ["Daily Needs", "Salary", "Rent", "Utilities", "Maintenance", "Other"];

export const STAFF_ROLES = ["Manager", "Waiter", "Chef", "Cashier"];

/** 'A' is implicitly the base order of a table; sub-tables start at 'B'. */
export const SUB_TABLE_LETTERS = ["B", "C", "D", "E", "F", "G", "H"];

export const FONT_FAMILIES = [
  { label: "Monospace", value: "monospace" },
  { label: "Sans-Serif", value: "sans-serif" },
  { label: "Serif", value: "serif" },
  { label: "Arial", value: "Arial, sans-serif" },
  { label: "Courier New", value: "'Courier New', monospace" },
  { label: "Times New Roman", value: "'Times New Roman', serif" },
];

export const FONT_SIZES = ["10px", "12px", "14px", "16px", "18px", "20px", "24px", "28px"];

export const MAX_SEARCH_SUGGESTIONS = 10;

export const DB_FILE_NAME = "restaurant.db";

export const STORAGE_KEYS = {
  dbFolderPath: "dbFolderPath",
  orderTypeLocked: "orderTypeLocked",
  lockedOrderType: "lockedOrderType",
  dashboardTimeRange: "dashboardTimeRange",
  licenseKey: "magicbill_license_key",
  licenseKeyHistory: "magicbill_license_key_history",
  theme: "magicbill_theme",
  customColors: "magicbill_custom_colors",
} as const;

export const RENEW_URL = "https://magicbill.in";

/** Latest Android app APK — each MB-android release attaches magic-bill.apk. */
export const MOBILE_APK_URL =
  "https://github.com/Sacchukulal/MB-android/releases/latest/download/magic-bill.apk";
