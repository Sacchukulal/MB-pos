import QRCode from "qrcode";
import { PAPER } from "../../config/constants";
import type { PaperSize } from "../../types";

/**
 * Bitmap rasterization for thermal printers (GS v 0 raster format).
 * Used for logos and UPI QR codes.
 */
export function rasterizeImage(base64: string, sizePercent: number, paperSize: PaperSize): Promise<number[]> {
  return new Promise((resolve) => {
    if (!base64) {
      resolve([]);
      return;
    }
    const img = new Image();
    img.onload = () => {
      if (!img.width || !img.height) {
        resolve([]);
        return;
      }
      const canvas = document.createElement("canvas");
      const ctx = canvas.getContext("2d");
      if (!ctx) {
        resolve([]);
        return;
      }

      const maxWidth = PAPER[paperSize]?.dots ?? PAPER["3inch"].dots;
      const targetWidth = Math.max(8, Math.floor(maxWidth * (sizePercent / 100)));
      const targetHeight = Math.max(8, Math.floor((img.height / img.width) * targetWidth));
      if (!isFinite(targetWidth) || !isFinite(targetHeight)) {
        resolve([]);
        return;
      }

      // ESC/POS raster width must be a multiple of 8 dots.
      const width = Math.floor((targetWidth + 7) / 8) * 8;
      const height = targetHeight;

      canvas.width = width;
      canvas.height = height;
      ctx.fillStyle = "white";
      ctx.fillRect(0, 0, width, height);
      ctx.drawImage(img, 0, 0, targetWidth, height);

      const pixels = ctx.getImageData(0, 0, width, height).data;
      const bytes: number[] = [];

      bytes.push(0x1b, 0x61, 0x01); // center
      bytes.push(0x1d, 0x76, 0x30, 0x00); // GS v 0, normal mode
      bytes.push((width / 8) % 256, Math.floor(width / 8 / 256), height % 256, Math.floor(height / 256));

      for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x += 8) {
          let byte = 0;
          for (let bit = 0; bit < 8; bit++) {
            if (x + bit < width) {
              const idx = (y * width + (x + bit)) * 4;
              const luma = pixels[idx] * 0.299 + pixels[idx + 1] * 0.587 + pixels[idx + 2] * 0.114;
              const isBlack = pixels[idx + 3] > 128 && luma < 128;
              if (isBlack) byte |= 1 << (7 - bit);
            }
          }
          bytes.push(byte);
        }
      }

      bytes.push(0x1b, 0x61, 0x00); // back to left align
      bytes.push(0x0a);
      resolve(bytes);
    };
    img.onerror = () => resolve([]);
    img.src = base64;
  });
}

export interface UpiQrParams {
  upiId: string;
  merchantName: string;
  reference?: string;
  /** Included only for dynamic QR codes. */
  amount?: number;
}

/** Builds the UPI intent string and returns it rasterized for the printer. */
export async function upiQrBytes(params: UpiQrParams, paperSize: PaperSize): Promise<number[]> {
  let upiString = `upi://pay?pa=${params.upiId}&pn=${encodeURIComponent(params.merchantName || "Restaurant")}&cu=INR`;
  if (params.reference) upiString += `&tr=${encodeURIComponent(params.reference)}`;
  if (params.amount !== undefined) upiString += `&am=${params.amount.toFixed(2)}`;

  try {
    const qrBase64 = await QRCode.toDataURL(upiString, { margin: 1, width: 250 });
    return await rasterizeImage(qrBase64, 40, paperSize);
  } catch (err) {
    console.error("Failed to generate UPI QR:", err);
    return [];
  }
}
