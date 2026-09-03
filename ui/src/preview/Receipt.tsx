/**
 * The paper, on the screen.
 *
 * For the graphics engine the preview IS the printer's raster — every dot Rust would send the
 * printer, drawn on one canvas at a whole or half number of dots per screen pixel, black ink on
 * white paper. There is no second renderer: no font, no cap-height ratio, nothing this file
 * works out for itself.
 *
 * The text engine prints with the printer's own ROM font, which no screen has, so its preview is
 * the structured list of rows Rust laid out, drawn in a monospace face at the row heights the
 * printer spends — an honest approximation, and the caption says so.
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react';

import type { PreviewDoc } from '../ipc/generated/PreviewDoc';
import type { PreviewLine } from '../ipc/generated/PreviewLine';
import type { PreviewRaster } from '../ipc/generated/PreviewRaster';

import './receipt.css';

export function Receipt({ doc }: { doc: PreviewDoc }) {
  return (
    <div className="mb-receipt">
      {doc.raster ? <Paper raster={doc.raster} /> : <RomPaper doc={doc} />}
      <p className="mb-receipt__length">
        {doc.millimetres} mm of paper · {doc.columns} characters across ·{' '}
        {doc.engine === 'text' ? "the printer's own font" : 'graphics'}
      </p>
      {doc.notes.length > 0 ? (
        <ul className="mb-receipt__notes">
          {doc.notes.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

/** Dots per screen pixel when nothing has been measured yet, or nothing can be. */
const DEFAULT_DOTS_PER_PIXEL = 2;

/** The coarsest the paper is ever drawn; beyond this a bill is a grey smudge. */
const COARSEST_DOTS_PER_PIXEL = 8;

/**
 * How many printer dots one screen pixel stands for, so that the paper fits in `available`
 * pixels — always a whole number or a half, never a fraction, so a one-dot rule is never
 * smeared across two pixels at partial strength and a two-dot bar is never lost.
 */
export function dotsPerPixel(dots: number, available: number): number {
  if (!(available > 0) || !(dots > 0)) return DEFAULT_DOTS_PER_PIXEL;
  for (let ratio = 0.5; ratio <= COARSEST_DOTS_PER_PIXEL; ratio += 0.5) {
    if (dots / ratio <= available) return ratio;
  }
  return COARSEST_DOTS_PER_PIXEL;
}

/** The base64 rows, back into bytes. */
function unpack(bits: string): Uint8Array {
  const text = atob(bits);
  const out = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i += 1) out[i] = text.charCodeAt(i);
  return out;
}

/** A `#rrggbb` token value as three channels; `fallback` when the token cannot be read. */
function channels(value: string, fallback: [number, number, number]): [number, number, number] {
  const hex = value.trim().replace('#', '');
  if (hex.length !== 6) return fallback;
  const n = Number.parseInt(hex, 16);
  if (Number.isNaN(n)) return fallback;
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** The printer's raster, dot for dot. */
function Paper({ raster }: { raster: PreviewRaster }) {
  const frame = useRef<HTMLDivElement | null>(null);
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const [ratio, setRatio] = useState(DEFAULT_DOTS_PER_PIXEL);

  // How much room the paper has, and again whenever that changes.
  useLayoutEffect(() => {
    const target = frame.current;
    if (!target) return undefined;
    const measure = () => setRatio(dotsPerPixel(raster.width, target.clientWidth));
    measure();
    if (typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(measure);
    observer.observe(target);
    return () => observer.disconnect();
  }, [raster.width]);

  // The dots.
  useEffect(() => {
    const target = canvas.current;
    if (!target || raster.width === 0 || raster.height === 0) return;
    const context = target.getContext('2d');
    if (!context) return;
    // The paper's two colours are the two print tokens — never the theme's.
    const style = getComputedStyle(target);
    const ink = channels(style.getPropertyValue('--print-ink'), [0, 0, 0]);
    const paper = channels(style.getPropertyValue('--print-paper'), [255, 255, 255]);
    const bytes = unpack(raster.bits);
    const stride = Math.ceil(raster.width / 8);
    const image = context.createImageData(raster.width, raster.height);
    for (let y = 0; y < raster.height; y += 1) {
      for (let x = 0; x < raster.width; x += 1) {
        const byte = bytes[y * stride + (x >> 3)] ?? 0;
        const on = (byte >> (7 - (x & 7))) & 1;
        const colour = on ? ink : paper;
        const at = (y * raster.width + x) * 4;
        image.data[at] = colour[0];
        image.data[at + 1] = colour[1];
        image.data[at + 2] = colour[2];
        image.data[at + 3] = 255;
      }
    }
    context.putImageData(image, 0, 0);
  }, [raster]);

  return (
    <div ref={frame} className="mb-receipt__frame">
      <canvas
        ref={canvas}
        className="mb-receipt__paper mb-receipt__paper--raster"
        aria-label="Preview of the printed bill, exactly as the printer will draw it"
        role="img"
        width={raster.width}
        height={raster.height}
        data-dots-per-pixel={ratio}
        // The paper's size on screen: its dots, at a whole or half number of dots a pixel.
        style={{ /* mb-tokens-allow: printer dots into screen pixels, at the ratio chosen above */
          width: `${raster.width / ratio}px`,
          height: `${raster.height / ratio}px`,
        }}
      />
    </div>
  );
}

/** The text engine's paper: rows of the printer's own characters. */
function RomPaper({ doc }: { doc: PreviewDoc }) {
  return (
    <div
      className="mb-receipt__paper mb-receipt__paper--rom"
      aria-label="Preview of the printed bill"
      // The one number the page is drawn from, out of Rust.
      style={{ /* mb-tokens-allow: the paper's own dot count, named by Rust */
        ['--receipt-dots' as string]: doc.dots,
      }}
    >
      {doc.lines.map((line, index) => (
        <Line key={index} line={line} />
      ))}
    </div>
  );
}

function Line({ line }: { line: PreviewLine }) {
  // One arm per variant.
  switch (line.kind) {
    case 'text':
      return (
        <div className="mb-receipt__row" style={rows(line.row)}>
          {line.segments.map((segment, index) => (
            <span
              key={index}
              className={[
                'mb-receipt__box',
                `mb-receipt__box--${segment.align}`,
                line.bold ? 'mb-receipt__box--bold' : '',
              ]
                .filter(Boolean)
                .join(' ')}
              // Every one of these is a dot count Rust computed.
              style={{ /* mb-tokens-allow: positions and sizes computed by Rust, not design values */
                ['--at' as string]: line.indent + segmentStart(line, index),
                ['--wide' as string]: segment.width * line.advance,
                ['--tall' as string]: line.row,
              }}
            >
              {segment.text}
            </span>
          ))}
        </div>
      );

    case 'rule':
      /* The printer's own character, repeated — which is what the text engine prints. */
      return (
        <div className="mb-receipt__row" style={rows(line.row)}>
          <span
            className="mb-receipt__box mb-receipt__box--left"
            style={{ /* mb-tokens-allow: the rule's place and width in printer dots */
              ['--at' as string]: line.indent,
              ['--wide' as string]: line.glyphs.length * line.advance,
              ['--tall' as string]: line.row,
            }}
          >
            {line.glyphs}
          </span>
        </div>
      );

    case 'qr':
      /* The printer draws the square; this is its size. */
      return (
        <div className="mb-receipt__row" style={rows(line.row)}>
          <span
            className="mb-receipt__qr"
            style={{ /* mb-tokens-allow: the square's real size in printer dots */
              ['--at' as string]: line.indent,
              ['--wide' as string]: line.size,
              ['--tall' as string]: line.size,
            }}
            title={line.payload}
          >
            <span className="mb-receipt__code">QR</span>
          </span>
        </div>
      );

    case 'barcode':
      return (
        <div className="mb-receipt__row" style={rows(line.row)}>
          <span
            className="mb-receipt__barcode"
            style={{ /* mb-tokens-allow: the bars' real height in printer dots */
              ['--at' as string]: line.indent,
              ['--tall' as string]: line.height,
            }}
            title={line.payload}
          />
          <span className="mb-receipt__code mb-receipt__code--under">{line.payload}</span>
        </div>
      );

    case 'blank':
      return <div className="mb-receipt__row" style={rows(line.row)} />;

    default:
      // The compiler fails the build if a variant is added and not drawn.
      return assertDrawn(line);
  }
}

/** Where a box starts, in dots. */
function segmentStart(line: Extract<PreviewLine, { kind: 'text' }>, index: number): number {
  let at = 0;
  for (let i = 0; i < index; i += 1) {
    at += (line.segments[i]?.width ?? 0) * line.advance;
  }
  return at;
}

/** A row's height, in dots. */
function rows(dots: number): React.CSSProperties {
  return { ['--tall' as string]: dots } as React.CSSProperties;
}

function assertDrawn(line: never): never {
  throw new Error(
    `The bill preview has no way to draw ${JSON.stringify(line)}. ` +
      'Every laid-out line must reach the screen (D29).',
  );
}
