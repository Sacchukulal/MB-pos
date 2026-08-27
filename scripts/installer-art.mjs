/**
 * The installer's two bitmaps, drawn from the one logo the app ships.
 *
 * NSIS wants BMPs: a 164x314 sidebar for the welcome and finish pages and a 150x57 header strip
 * for the rest. Both are white with the mark on them. Run it again whenever ui/src/kit/logo.png
 * changes (the .ico beside them comes from `cargo tauri icon <logo.png>`).
 *
 * Usage:
 *   node scripts/installer-art.mjs
 */

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const SOURCE = join(root, 'ui', 'src', 'kit', 'logo.png');
const OUT = join(root, 'src-tauri', 'icons');

/** An 8-bit RGBA PNG, no interlace — which is what the logo is. */
function png(path) {
  const b = readFileSync(path);
  const w = b.readUInt32BE(16);
  const h = b.readUInt32BE(20);
  if (b[24] !== 8 || b[25] !== 6 || b[28] !== 0) throw new Error(`want 8-bit RGBA, no interlace: ${path}`);
  let p = 8;
  const idat = [];
  while (p < b.length) {
    const len = b.readUInt32BE(p);
    if (b.toString('ascii', p + 4, p + 8) === 'IDAT') idat.push(b.subarray(p + 8, p + 8 + len));
    p += 12 + len;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const bpp = 4;
  const stride = w * bpp;
  const px = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y += 1) {
    const filter = raw[y * (stride + 1)];
    const src = y * (stride + 1) + 1;
    const dst = y * stride;
    for (let i = 0; i < stride; i += 1) {
      const x = raw[src + i];
      const a = i >= bpp ? px[dst + i - bpp] : 0;
      const up = y > 0 ? px[dst - stride + i] : 0;
      const ul = y > 0 && i >= bpp ? px[dst - stride + i - bpp] : 0;
      let v;
      if (filter === 0) v = x;
      else if (filter === 1) v = x + a;
      else if (filter === 2) v = x + up;
      else if (filter === 3) v = x + ((a + up) >> 1);
      else {
        const pp = a + up - ul;
        const pa = Math.abs(pp - a);
        const pb = Math.abs(pp - up);
        const pc = Math.abs(pp - ul);
        v = x + (pa <= pb && pa <= pc ? a : pb <= pc ? up : ul);
      }
      px[dst + i] = v & 255;
    }
  }
  return { w, h, px };
}

/** Smaller, by averaging the source pixels each target pixel covers. Only ever shrinks. */
function shrink(img, size) {
  const out = Buffer.alloc(size * size * 4);
  const scale = img.w / size;
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      const x0 = x * scale;
      const y0 = y * scale;
      const x1 = x0 + scale;
      const y1 = y0 + scale;
      const sum = [0, 0, 0, 0];
      let area = 0;
      for (let sy = Math.floor(y0); sy < Math.ceil(y1); sy += 1) {
        const hy = Math.min(sy + 1, y1) - Math.max(sy, y0);
        for (let sx = Math.floor(x0); sx < Math.ceil(x1); sx += 1) {
          const cover = hy * (Math.min(sx + 1, x1) - Math.max(sx, x0));
          const o = (sy * img.w + sx) * 4;
          // Colour weighted by alpha, so transparent pixels do not darken the edge.
          const a = img.px[o + 3] / 255;
          sum[0] += img.px[o] * a * cover;
          sum[1] += img.px[o + 1] * a * cover;
          sum[2] += img.px[o + 2] * a * cover;
          sum[3] += a * cover;
          area += cover;
        }
      }
      const o = (y * size + x) * 4;
      const a = sum[3];
      out[o] = a > 0 ? Math.round(sum[0] / a) : 0;
      out[o + 1] = a > 0 ? Math.round(sum[1] / a) : 0;
      out[o + 2] = a > 0 ? Math.round(sum[2] / a) : 0;
      out[o + 3] = Math.round((a / area) * 255);
    }
  }
  return { w: size, h: size, px: out };
}

/** 24-bit, bottom-up, BGR — the plainest BMP there is. */
function bmp(w, h, paint) {
  const rowBytes = (w * 3 + 3) & ~3;
  const size = 54 + rowBytes * h;
  const b = Buffer.alloc(size);
  b.write('BM', 0);
  b.writeUInt32LE(size, 2);
  b.writeUInt32LE(54, 10);
  b.writeUInt32LE(40, 14);
  b.writeInt32LE(w, 18);
  b.writeInt32LE(h, 22);
  b.writeUInt16LE(1, 26);
  b.writeUInt16LE(24, 28);
  b.writeUInt32LE(rowBytes * h, 34);
  b.writeInt32LE(2835, 38);
  b.writeInt32LE(2835, 42);
  for (let y = 0; y < h; y += 1) {
    for (let x = 0; x < w; x += 1) {
      const [r, g, bl] = paint(x, y);
      const o = 54 + (h - 1 - y) * rowBytes + x * 3;
      b[o] = bl;
      b[o + 1] = g;
      b[o + 2] = r;
    }
  }
  return b;
}

/** White, with the mark at (left, top). */
function compose(w, h, mark, left, top) {
  return bmp(w, h, (x, y) => {
    const ix = x - left;
    const iy = y - top;
    if (ix < 0 || iy < 0 || ix >= mark.w || iy >= mark.h) return [255, 255, 255];
    const o = (iy * mark.w + ix) * 4;
    const a = mark.px[o + 3] / 255;
    return [0, 1, 2].map((c) => Math.round(mark.px[o + c] * a + 255 * (1 - a)));
  });
}

const logo = png(SOURCE);
// The welcome and finish pages: the mark up where the eye lands, the text NSIS draws is to the right.
writeFileSync(join(OUT, 'installer-sidebar.bmp'), compose(164, 314, shrink(logo, 128), 18, 40));
// Every other page: a strip along the top, the mark at its right end.
writeFileSync(join(OUT, 'installer-header.bmp'), compose(150, 57, shrink(logo, 44), 100, 6));
console.log('wrote src-tauri/icons/installer-sidebar.bmp and installer-header.bmp');
