import { PAPER } from "../../config/constants";
import type { PaperSize, TokenPrintSize } from "../../types";

/**
 * ESC/POS text-mode toolkit — the only place raw printer command bytes live.
 * All commands are embedded as escape strings inside the receipt text and
 * encoded to bytes by buildDocument().
 */

export const CMD = {
  INIT: "\x1B\x40",
  ALIGN_LEFT: "\x1B\x61\x00",
  ALIGN_CENTER: "\x1B\x61\x01",
  BOLD_ON: "\x1B\x45\x01",
  BOLD_OFF: "\x1B\x45\x00",
  SIZE_NORMAL: "\x1D\x21\x00",
  SIZE_DOUBLE_HEIGHT: "\x1D\x21\x01",
  SIZE_DOUBLE_BOTH: "\x1D\x21\x11",
  SIZE_QUAD: "\x1D\x21\x22",
} as const;

/** Character columns for a paper size. */
export function lineWidth(paperSize: PaperSize): number {
  return PAPER[paperSize]?.columns ?? PAPER["3inch"].columns;
}

export function padRight(text: unknown, width: number): string {
  const str = String(text ?? "");
  return str.length >= width ? str.substring(0, width) : str.padEnd(width);
}

export function padLeft(text: unknown, width: number): string {
  const str = String(text ?? "");
  return str.length >= width ? str.substring(0, width) : str.padStart(width);
}

/** Split a line into a left- and right-aligned half. */
export function twoColumns(left: string, right: string, width: number): string {
  return `${padRight(left, Math.floor(width / 2))}${padLeft(right, Math.ceil(width / 2))}`;
}

/**
 * Map a px font-size setting to an ESC/POS size command.
 * allowWidth=false keeps column math intact (height-only doubling); double-width
 * is reserved for large free-flow lines like the store name.
 */
export function sizeCmd(size?: string, allowWidth = true): string {
  const px = parseInt(String(size || "12"), 10) || 12;
  if (allowWidth && px >= 24) return CMD.SIZE_DOUBLE_BOTH;
  if (px >= 16) return CMD.SIZE_DOUBLE_HEIGHT;
  return CMD.SIZE_NORMAL;
}

/** Wrap an already-padded line with size + bold codes (codes are non-printing). */
export function styled(line: string, size?: string, bold?: boolean, allowWidth = true): string {
  return `${sizeCmd(size, allowWidth)}${bold ? CMD.BOLD_ON : ""}${line}${bold ? CMD.BOLD_OFF : ""}${CMD.SIZE_NORMAL}`;
}

/** ESC/POS size command for the token line per the configured print size. */
export function tokenSizeCmd(printSize: TokenPrintSize): string {
  switch (printSize) {
    case "Extra Large":
      return CMD.SIZE_QUAD;
    case "Large":
      return CMD.SIZE_DOUBLE_BOTH;
    default:
      return CMD.SIZE_DOUBLE_HEIGHT;
  }
}

/** A centered TOKEN block, shared by bill and KOT templates. */
export function tokenBlock(tokenNumber: number | string, printSize: TokenPrintSize): string {
  return (
    tokenSizeCmd(printSize) +
    CMD.ALIGN_CENTER +
    `TOKEN: ${tokenNumber}\n` +
    CMD.SIZE_NORMAL +
    CMD.ALIGN_LEFT
  );
}

/**
 * Assemble the final byte stream: init, optional top image, optional global bold,
 * text, optional bottom image, feed and cut.
 */
export function buildDocument(
  text: string,
  opts: { printBold?: boolean; imageBytes?: number[]; imagePosition?: "top" | "bottom" | "none"; trailer?: number[] } = {}
): number[] {
  const encoder = new TextEncoder();
  let data: number[] = [];

  data.push(0x1b, 0x40); // ESC @ init

  if (opts.imageBytes?.length && opts.imagePosition === "top") {
    data = data.concat(opts.imageBytes);
  }

  if (opts.printBold) data.push(0x1b, 0x45, 0x01);
  data = data.concat(Array.from(encoder.encode(text)));
  if (opts.printBold) data.push(0x1b, 0x45, 0x00);

  if (opts.imageBytes?.length && opts.imagePosition === "bottom") {
    data.push(0x1b, 0x61, 0x01);
    data = data.concat(opts.imageBytes);
    data.push(0x1b, 0x61, 0x00);
  }

  if (opts.trailer?.length) data = data.concat(opts.trailer);

  data.push(0x1d, 0x56, 0x41, 0x10); // GS V A 16 — partial cut
  return data;
}

/**
 * Remove ESC/POS command sequences for the plain-text fallback printer path,
 * matching each known command's exact byte shape so no visible text is eaten.
 */
export function stripEscCodes(text: string): string {
  return text
    .replace(/\x1B\x61[\x00-\x02]/g, "") // alignment
    .replace(/\x1B\x45[\x00\x01]/g, "") // bold
    .replace(/\x1D\x21[\s\S]/g, "") // size
    .replace(/\x1B\x40/g, ""); // init
}
