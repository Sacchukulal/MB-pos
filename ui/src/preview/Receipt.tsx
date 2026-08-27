/** The fourth sink. */

import { useEffect, useRef } from 'react';

import type { PreviewDoc } from '../ipc/generated/PreviewDoc';
import type { PreviewLine } from '../ipc/generated/PreviewLine';
import type { PreviewBandLine } from '../ipc/generated/PreviewBandLine';

import './receipt.css';

// How tall a capital is, as a fraction of the font size.

export function Receipt({
  doc,
  font,
}: {
  doc: PreviewDoc;
  /**
   * The typeface the printer will use, as a Windows family name — so the preview is in the face
   * the paper will be.
   */
  font?: string;
}) {
  return (
    // No monospace/proportional branch any more.
    <div className="mb-receipt">
      <div
        className="mb-receipt__paper"
        aria-label="Preview of the printed bill"
        // The two numbers the whole page is drawn from, both out of Rust.
        style={{ /* mb-tokens-allow: the paper's own dot count and the shop's chosen face, both named by Rust */
          ['--receipt-dots' as string]: doc.dots,
          ...(font ? { fontFamily: `${font}, monospace` } : {}),
        }}
      >
        {doc.lines.map((line, index) => (
          <Line key={index} line={line} />
        ))}
      </div>
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
                ['--at' as string]: line.indent + segmentStart(line, segment.width, index),
                ['--wide' as string]: segment.width * line.advance,
                ['--cap' as string]: line.cap,
              }}
            >
              {segment.text}
            </span>
          ))}
        </div>
      );

    case 'rule':
      /* A drawn line, like the paper draws. */
      return (
        <div className="mb-receipt__row" style={rows(line.row)}>
          {Array.from({ length: line.strokes }, (_, stroke) => (
            <span
              key={stroke}
              className="mb-receipt__rule"
              style={{ /* mb-tokens-allow: rule geometry computed by Rust */
                ['--at' as string]: line.indent,
                ['--wide' as string]: line.width,
                ['--thick' as string]: line.thickness,
                ['--stroke' as string]: stroke * (line.thickness + line.gap),
                ['--strokes' as string]:
                  line.strokes * line.thickness + (line.strokes - 1) * line.gap,
                ...(line.dash
                  ? { ['--on' as string]: line.dash[0], ['--off' as string]: line.dash[1] }
                  : {}),
              }}
              data-dashed={line.dash ? 'yes' : undefined}
            />
          ))}
        </div>
      );

    case 'logo':
      return (
        <div className="mb-receipt__row" style={rows(line.row)}>
          <Dots line={line} />
        </div>
      );

    case 'band':
      /* The letterhead — a logo and the shop's name side by side. */
      return (
        <div className="mb-receipt__row" style={rows(line.row)}>
          {line.image.kind === 'logo' ? <Dots line={line.image} /> : null}
          {line.lines.map((text, index) => (
            <BandText key={index} line={text} />
          ))}
        </div>
      );

    case 'qr':
      /* The square at the size the printer will make it. */
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

/** The real dots, at the size they will print. */
function Dots({
  line,
}: {
  line: Extract<PreviewLine, { kind: 'logo' }>;
}) {
  const canvas = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const target = canvas.current;
    if (!target || !line.ink || line.width === 0 || line.height === 0) return;
    target.width = line.width;
    target.height = line.height;
    const context = target.getContext('2d');
    if (!context) return;
    const image = context.createImageData(line.width, line.height);
    for (let i = 0; i < line.ink.length; i += 1) {
      const on = line.ink[i] === 1;
      const at = i * 4;
      // Ink is black on white — the paper's colours, not the theme's, because this is a picture
      // of paper.
      image.data[at] = on ? 0 : 255;
      image.data[at + 1] = on ? 0 : 255;
      image.data[at + 2] = on ? 0 : 255;
      image.data[at + 3] = on ? 255 : 0;
    }
    context.putImageData(image, 0, 0);
  }, [line.ink, line.width, line.height]);

  if (!line.ink) {
    /*
     * A logo that will not read does not print, and the preview says the same thing rather than
     * drawing a picture that is not there.
     */
    return (
      <span
        className="mb-receipt__nologo"
        style={{ ['--at' as string]: line.left }} /* mb-tokens-allow: a position in printer dots */
      >
        (your logo could not be read)
      </span>
    );
  }

  return (
    <canvas
      ref={canvas}
      className="mb-receipt__logo"
      style={{ /* mb-tokens-allow: the picture's real place on the paper, in dots */
        ['--at' as string]: line.left,
        ['--wide' as string]: line.width,
        ['--tall' as string]: line.height,
        ['--down' as string]: line.indent,
      }}
    />
  );
}

function BandText({ line }: { line: PreviewBandLine }) {
  return (
    <span
      className={[
        'mb-receipt__box',
        `mb-receipt__box--${line.align}`,
        line.bold ? 'mb-receipt__box--bold' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      style={{ /* mb-tokens-allow: a letterhead line, placed by Rust */
        ['--at' as string]: line.left,
        ['--wide' as string]: line.width,
        ['--cap' as string]: line.cap,
        ['--down' as string]: line.top,
        ['--tall' as string]: line.row,
      }}
    >
      {line.text}
    </span>
  );
}

/** Where a box starts, in dots. */
function segmentStart(
  line: Extract<PreviewLine, { kind: 'text' }>,
  _width: number,
  index: number,
): number {
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
