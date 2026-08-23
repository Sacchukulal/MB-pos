/**
 * **The fourth sink.**
 *
 * text (P06) · PDF (P06) · raster (P07) · **screen (P08)** — all four render
 * the same laid-out document, which is decision **D29**:
 *
 * > *"There is exactly one function that walks a laid-out document, and every
 * > renderer is a `Sink` it calls. A sink cannot forget: it is handed
 * > everything, in order."*
 *
 * # It renders. It does not lay out.
 *
 * Every line, every wrap, every column, every size and the print offset were
 * decided in Rust before this component saw anything. **The moment TypeScript
 * measures text and decides where to break it, there are two layout engines and
 * audit D1 is back:**
 *
 * > *"The same bill is drawn three separate times, by hand, in three places…
 * > every design change is triple work, and the three **will** drift apart.
 * > This is the single biggest source of 'the preview does not match the
 * > paper'."*
 *
 * # It draws in printer dots — P32
 *
 * This used to lay text out in `ch` units and scale a size against 24 in CSS,
 * while the raster sink drew whatever fitted a twelve-dot column. Two size
 * models: the screen said 24 and the paper drew 13, and the owner found it on a
 * photograph of real paper.
 *
 * So **one number scales the whole page** — `--dot`, how many screen pixels one
 * printer dot is worth — and every position, every width and every height comes
 * from Rust in dots. A box is at `start × advance` dots because that is where
 * `raster.rs` puts it. Change `--dot` and the preview zooms; nothing else about
 * it can move.
 *
 * The one thing the screen cannot copy is the printer's own typeface, so the
 * text inside each box is aligned **by the box**, exactly as the raster sink
 * aligns a proportional face — which is what makes a column of amounts land on
 * the same right edge in both.
 */

import { useEffect, useRef } from 'react';

import type { PreviewDoc } from '../ipc/generated/PreviewDoc';
import type { PreviewLine } from '../ipc/generated/PreviewLine';
import type { PreviewBandLine } from '../ipc/generated/PreviewBandLine';

import './receipt.css';

// **How tall a capital is, as a fraction of the font size.**
//
// A browser sizes text by em and the layout sizes it by cap height, so one
// conversion is unavoidable. It is done in `receipt.css` — 0.72, the cap ratio
// of every face this product offers to within a few per cent (IBM Plex Mono is
// 0.698, Arial 0.716, Times 0.662, Verdana 0.727). Being a few per cent out on
// screen is a different order of thing from the 2x the old model was out by.

export function Receipt({
  doc,
  font,
}: {
  doc: PreviewDoc;
  /**
   * **The typeface the printer will use**, as a Windows family name — so the
   * preview is in the face the paper will be. Empty for the built-in one,
   * which the screen draws in its own monospace stack.
   */
  font?: string;
}) {
  return (
    // **No monospace/proportional branch any more** (P32). Both are drawn box
    // by box at the dots the layout gave, which is what `raster.rs` does — so
    // the class that used to switch between them changed nothing, and it has
    // gone with it.
    <div className="mb-receipt">
      <div
        className="mb-receipt__paper"
        aria-label="Preview of the printed bill"
        // The two numbers the whole page is drawn from, both out of Rust.
        // `--dot` is derived from them in the stylesheet, so a narrow panel
        // shrinks the paper instead of scrolling it.
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
  // One arm per variant. An arm that quietly did nothing would be a sink
  // forgetting a block — the exact thing D29 exists to prevent — so every
  // variant is handled and the ones with nothing to draw say so out loud.
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
              // Every one of these is a dot count Rust computed. The screen
              // turns dots into pixels and does nothing else.
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
      /* **A drawn line, like the paper draws.** It was `glyph.repeat(width)`
         in a CSS monospace font, which came out nearly solid on screen while
         the paper printed five dots of ink in every twelve-dot cell — a row of
         spaced ticks. Both draw a rule from the same numbers now. */
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
      /* **The letterhead** — a logo and the shop's name side by side (P32).
         Every position is already decided; this places two things. */
      return (
        <div className="mb-receipt__row" style={rows(line.row)}>
          {line.image.kind === 'logo' ? <Dots line={line.image} /> : null}
          {line.lines.map((text, index) => (
            <BandText key={index} line={text} />
          ))}
        </div>
      );

    case 'qr':
      /* **The square at the size the printer will make it.**
         The screen does not encode a QR — the printer's own encoder does that
         (D36) and there is no encoder in this product to copy. What a shop
         tuning its letterhead actually needs to know is how much paper the
         square costs and whether the payload is right, and both are here. */
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
      // **The compiler fails the build if a variant is added and not drawn.**
      // `line` narrows to `never` only when every arm above exists; the moment
      // one does not, this assignment is a type error at the line that caused
      // it. That is the guard the missing barcode arm needed at P31, and it
      // costs three lines.
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
      // Ink is black on white — the paper's colours, not the theme's, because
      // this is a picture of paper.
      image.data[at] = on ? 0 : 255;
      image.data[at + 1] = on ? 0 : 255;
      image.data[at + 2] = on ? 0 : 255;
      image.data[at + 3] = on ? 255 : 0;
    }
    context.putImageData(image, 0, 0);
  }, [line.ink, line.width, line.height]);

  if (!line.ink) {
    /* D37: a logo that will not read does not print, and the preview says the
       same thing rather than drawing a picture that is not there. */
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

/**
 * Where a box starts, in dots.
 *
 * The layout gives a box its start in **characters** of that line's own size,
 * and the widths that came before it are the same characters — so the sum is
 * the same arithmetic `raster.rs` does, and it is done here rather than in the
 * view model for the same reason: one number crosses the wire, not two.
 */
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
