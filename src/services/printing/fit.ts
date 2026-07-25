import { FONT_SIZES, contentWidthPx } from "../../config/constants";
import type { PaperSize } from "../../types";

/**
 * Font-size ceilings for the sections laid out in columns.
 *
 * A receipt column layout only stays aligned while its widest realistic row fits
 * the paper. Since the user picks both the font family and the paper size, the
 * limit is measured for real (Canvas metrics) instead of guessed — a condensed
 * font allows a bigger size than a wide monospace one on the same paper.
 */

export type FitSection = "billTable" | "billMeta" | "billTotals" | "kotItems" | "kotMeta";

/**
 * The narrowest row each section must still fit on one line. Item names wrap, so
 * these hold the number columns plus a readable minimum of name — the point
 * where columns would start colliding rather than merely wrapping.
 */
const SAMPLES: Record<FitSection, string> = {
  billTable: "Biriyani  12  1250.00  12500.00",
  billMeta: "Bill No: 1234    Date: 26-Feb-2026",
  billTotals: "GRAND TOTAL:  Rs. 12500.00",
  kotItems: "Chicken Biriyani  12",
  kotMeta: "Date: 25/07/2026, 2:35:31 PM",
};

let measureCtx: CanvasRenderingContext2D | null = null;

function ctx(): CanvasRenderingContext2D | null {
  if (measureCtx) return measureCtx;
  if (typeof document === "undefined") return null;
  measureCtx = document.createElement("canvas").getContext("2d");
  return measureCtx;
}

/** Width of `text` in CSS px at the given font — the same metric the preview lays out with. */
export function measureTextPx(text: string, sizePx: number, fontFamily: string, bold = false): number {
  const c = ctx();
  if (!c) return text.length * sizePx * 0.6; // headless fallback: monospace-ish estimate
  c.font = `${bold ? "bold " : ""}${sizePx}px ${fontFamily}`;
  return c.measureText(text).width;
}

/**
 * Largest FONT_SIZES entry whose worst-case row still fits the paper.
 * Measured bold, because bold is the widest a section can print.
 */
export function maxFontSize(section: FitSection, fontFamily: string, paperSize: PaperSize): string {
  const budget = contentWidthPx(paperSize);
  const sample = SAMPLES[section];
  for (let i = FONT_SIZES.length - 1; i >= 0; i--) {
    const px = parseInt(FONT_SIZES[i], 10);
    if (measureTextPx(sample, px, fontFamily, true) <= budget) return FONT_SIZES[i];
  }
  return FONT_SIZES[0];
}

/** Clamps a stored size to the section's ceiling — print and preview both go through this. */
export function clampSize(size: string, section: FitSection, fontFamily: string, paperSize: PaperSize): string {
  const max = maxFontSize(section, fontFamily, paperSize);
  const idx = FONT_SIZES.indexOf(size);
  const maxIdx = FONT_SIZES.indexOf(max);
  if (idx < 0 || maxIdx < 0) return size;
  return idx > maxIdx ? max : size;
}
