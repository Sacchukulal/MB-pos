import { PAPER, PRINT_MARGIN_DOTS, contentWidthPx, printScale } from "../../config/constants";
import type { LinePattern, PaperSize } from "../../types";

/**
 * Canvas receipt engine: lays a receipt out in preview pixels, draws it at
 * printer resolution, and emits ESC/POS raster bytes.
 *
 * Everything is authored in the same CSS-px coordinate space the on-screen
 * preview uses (see contentWidthPx) and multiplied by printScale() on the way
 * to the canvas, so what the shop sees in Bill Settings is what the paper gets —
 * font family, exact size, and real bold included.
 */

/** Generous scratch height; only the rows actually drawn are rasterized. */
const MAX_CANVAS_PX = 6000;
/** Anti-aliased pixels darker than this become black dots. */
const INK_THRESHOLD = 168;
/** Rows per GS v 0 block — small blocks keep modest printer buffers happy. */
const RASTER_BAND_ROWS = 128;

export interface Doc {
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  /** preview px -> printer dots */
  scale: number;
  /** Content width in preview px. */
  width: number;
  /** Cursor: distance from the top of the content box, in preview px. */
  y: number;
  fontFamily: string;
  /** Printer-level "Bold & Dark" — forces bold on every section. */
  forceBold: boolean;
}

export interface TextOpts {
  size: string | number;
  bold?: boolean;
  align?: "left" | "center" | "right";
  /** Extra px above the line. */
  gapBefore?: number;
  /** Line box height as a multiple of the font size. */
  lineHeight?: number;
}

export function px(size: string | number): number {
  const n = typeof size === "number" ? size : parseInt(size, 10);
  return Number.isFinite(n) && n > 0 ? n : 12;
}

/** Vertical padding encoded in a rowHeight setting like "4px 0". */
export function rowPadPx(rowHeight: string): number {
  return px(String(rowHeight).split(" ")[0]);
}

export function createDoc(paperSize: PaperSize, fontFamily: string, forceBold = false): Doc {
  const paper = PAPER[paperSize] ?? PAPER["3inch"];
  const scale = printScale(paperSize);
  const canvas = document.createElement("canvas");
  canvas.width = paper.dots;
  canvas.height = MAX_CANVAS_PX;
  const ctx = canvas.getContext("2d", { willReadFrequently: true }) as CanvasRenderingContext2D;
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.fillStyle = "#000";
  ctx.strokeStyle = "#000";
  ctx.textBaseline = "top";
  return { canvas, ctx, scale, width: contentWidthPx(paperSize), y: 0, fontFamily, forceBold };
}

/** Device-space x of a preview-space x. */
function dx(doc: Doc, x: number): number {
  return PRINT_MARGIN_DOTS + x * doc.scale;
}

function setFont(doc: Doc, size: string | number, bold?: boolean): void {
  doc.ctx.font = `${bold || doc.forceBold ? "bold " : ""}${px(size) * doc.scale}px ${doc.fontFamily}`;
}

export function measure(doc: Doc, text: string, size: string | number, bold?: boolean): number {
  setFont(doc, size, bold);
  return doc.ctx.measureText(text).width / doc.scale;
}

/** Break text to fit `maxWidth` preview px, splitting on spaces where possible. */
export function wrap(doc: Doc, text: string, maxWidth: number, size: string | number, bold?: boolean): string[] {
  const source = String(text ?? "").trim();
  if (!source) return [""];
  if (measure(doc, source, size, bold) <= maxWidth) return [source];

  const lines: string[] = [];
  let current = "";
  for (const word of source.split(/\s+/)) {
    let w = word;
    // Hard-split a single word that cannot fit on a line of its own.
    while (measure(doc, w, size, bold) > maxWidth && w.length > 1) {
      let cut = w.length - 1;
      while (cut > 1 && measure(doc, w.substring(0, cut), size, bold) > maxWidth) cut--;
      if (current) {
        lines.push(current);
        current = "";
      }
      lines.push(w.substring(0, cut));
      w = w.substring(cut);
    }
    if (!current) current = w;
    else if (measure(doc, `${current} ${w}`, size, bold) <= maxWidth) current += ` ${w}`;
    else {
      lines.push(current);
      current = w;
    }
  }
  if (current) lines.push(current);
  return lines;
}

/** Draws one already-fitted line and advances the cursor. */
function drawLine(doc: Doc, text: string, opts: TextOpts): void {
  const size = px(opts.size);
  const lineBox = size * (opts.lineHeight ?? 1.3);
  setFont(doc, size, opts.bold);
  const w = doc.ctx.measureText(text).width / doc.scale;
  let x = 0;
  if (opts.align === "center") x = (doc.width - w) / 2;
  else if (opts.align === "right") x = doc.width - w;
  // Sit the glyphs on the line box with the leading split above and below.
  doc.ctx.fillText(text, dx(doc, Math.max(0, x)), (doc.y + (lineBox - size) / 2) * doc.scale);
  doc.y += lineBox;
}

/** Draws text, wrapping to the content width, and advances the cursor. */
export function text(doc: Doc, value: string, opts: TextOpts): void {
  doc.y += opts.gapBefore ?? 0;
  wrap(doc, value, doc.width, opts.size, opts.bold).forEach((line) => drawLine(doc, line, opts));
}

export interface Cell {
  text: string;
  align?: "left" | "right";
  /** Width in preview px. Omit on exactly one cell to let it take the remainder. */
  width?: number;
}

/**
 * One row of columns. The flexible cell wraps; fixed cells are right-aligned
 * number columns, which is what keeps Qty / Price / Amt lined up in any font.
 */
export function columns(doc: Doc, cells: Cell[], opts: TextOpts): void {
  doc.y += opts.gapBefore ?? 0;
  const size = px(opts.size);
  const lineBox = size * (opts.lineHeight ?? 1.3);
  const fixed = cells.reduce((sum, c) => sum + (c.width ?? 0), 0);
  const flexIndex = cells.findIndex((c) => c.width === undefined);
  const flexWidth = Math.max(size, doc.width - fixed);

  const wrapped = cells.map((c, i) =>
    i === flexIndex ? wrap(doc, c.text, flexWidth, size, opts.bold) : [c.text]
  );
  const rows = Math.max(...wrapped.map((w) => w.length));

  for (let r = 0; r < rows; r++) {
    let x = 0;
    cells.forEach((cell, i) => {
      const cellWidth = i === flexIndex ? flexWidth : (cell.width as number);
      const value = wrapped[i][r] ?? "";
      if (value) {
        setFont(doc, size, opts.bold);
        const w = doc.ctx.measureText(value).width / doc.scale;
        const cellX = cell.align === "right" ? x + cellWidth - w : x;
        doc.ctx.fillText(value, dx(doc, Math.max(x, cellX)), (doc.y + (lineBox - size) / 2) * doc.scale);
      }
      x += cellWidth;
    });
    doc.y += lineBox;
  }
}

/** Left text + right text on one line (meta rows, totals). */
export function leftRight(doc: Doc, left: string, right: string, opts: TextOpts): void {
  const rightWidth = right ? measure(doc, right, opts.size, opts.bold) : 0;
  columns(
    doc,
    [
      { text: left },
      { text: right, align: "right", width: rightWidth + (right ? px(opts.size) * 0.4 : 0) },
    ],
    opts
  );
}

export function space(doc: Doc, amount: number): void {
  doc.y += amount;
}

/** Horizontal separator in the shop's chosen line style. */
export function separator(doc: Doc, pattern: LinePattern, gap = 6): void {
  const ctx = doc.ctx;
  doc.y += gap;
  const x1 = dx(doc, 0);
  const x2 = dx(doc, doc.width);
  const thin = Math.max(1, Math.round(doc.scale));
  const thick = Math.max(2, Math.round(doc.scale * 2));

  ctx.save();
  ctx.strokeStyle = "#000";
  ctx.setLineDash([]);
  const stroke = (yPx: number, lineWidth: number, dash: number[]) => {
    ctx.lineWidth = lineWidth;
    ctx.setLineDash(dash);
    ctx.beginPath();
    ctx.moveTo(x1, yPx + lineWidth / 2);
    ctx.lineTo(x2, yPx + lineWidth / 2);
    ctx.stroke();
  };

  const top = doc.y * doc.scale;
  switch (pattern) {
    case "dotted":
      stroke(top, thin, [thin, thin * 2]);
      doc.y += gap + thin / doc.scale;
      break;
    case "solid":
      stroke(top, thin, []);
      doc.y += gap + thin / doc.scale;
      break;
    case "bold":
      stroke(top, thick, []);
      doc.y += gap + thick / doc.scale;
      break;
    case "double":
      stroke(top, thin, []);
      stroke(top + thin * 2, thin, []);
      doc.y += gap + (thin * 3) / doc.scale;
      break;
    default: // dashed
      stroke(top, thin, [thin * 4, thin * 3]);
      doc.y += gap + thin / doc.scale;
      break;
  }
  ctx.restore();
}

/** Draws an already-loaded image centered, scaled to `widthPct` of the content width. */
export function image(doc: Doc, img: HTMLImageElement, widthPct: number): void {
  if (!img.width || !img.height) return;
  const w = doc.width * (Math.max(5, Math.min(100, widthPct)) / 100);
  const h = (img.height / img.width) * w;
  doc.ctx.drawImage(img, dx(doc, (doc.width - w) / 2), doc.y * doc.scale, w * doc.scale, h * doc.scale);
  doc.y += h;
}

export function loadImage(src: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    if (!src) {
      resolve(null);
      return;
    }
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = src;
  });
}

/**
 * Converts the drawn part of the canvas to ESC/POS raster bytes, emitted in
 * horizontal bands so a large receipt never overruns the printer's buffer.
 */
export function docToRaster(doc: Doc): number[] {
  const width = doc.canvas.width; // already a multiple of 8
  const height = Math.min(MAX_CANVAS_PX, Math.max(1, Math.ceil(doc.y * doc.scale) + PRINT_MARGIN_DOTS));
  const bytesPerRow = width / 8;
  const pixels = doc.ctx.getImageData(0, 0, width, height).data;
  const out: number[] = [];

  for (let bandTop = 0; bandTop < height; bandTop += RASTER_BAND_ROWS) {
    const bandRows = Math.min(RASTER_BAND_ROWS, height - bandTop);
    out.push(0x1d, 0x76, 0x30, 0x00); // GS v 0, normal mode
    out.push(bytesPerRow % 256, Math.floor(bytesPerRow / 256), bandRows % 256, Math.floor(bandRows / 256));
    for (let y = bandTop; y < bandTop + bandRows; y++) {
      for (let x = 0; x < width; x += 8) {
        let byte = 0;
        for (let bit = 0; bit < 8; bit++) {
          const idx = (y * width + x + bit) * 4;
          const luma = pixels[idx] * 0.299 + pixels[idx + 1] * 0.587 + pixels[idx + 2] * 0.114;
          if (pixels[idx + 3] > 128 && luma < INK_THRESHOLD) byte |= 1 << (7 - bit);
        }
        out.push(byte);
      }
    }
  }
  return out;
}
