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
 * Every line, every wrap, every column, the font cap and the print offset were
 * decided in Rust before this component saw anything. **The moment TypeScript
 * measures text and decides where to break it, there are two layout engines and
 * audit D1 is back:**
 *
 * > *"The same bill is drawn three separate times, by hand, in three places…
 * > every design change is triple work, and the three **will** drift apart.
 * > This is the single biggest source of 'the preview does not match the
 * > paper'."*
 *
 * So this file has no wrapping, no measuring, no truncation and no arithmetic.
 * It maps a line to a `<span>`. `tests/preview.test.tsx` asserts its text
 * against what the layout produced — P06's anti-drift test, extended across
 * IPC.
 *
 * # And it is what makes audit D6 cheap
 *
 * > *"No bill preview before printing. You cannot see the actual bill for the
 * > actual order before it comes out of the printer."*
 *
 * Being a sink rather than a drawing meant the preview cost one file.
 */

import type { PreviewDoc } from '../ipc/generated/PreviewDoc';
import type { PreviewLine } from '../ipc/generated/PreviewLine';

import './receipt.css';

export function Receipt({
  doc,
  font,
  monospace = true,
}: {
  doc: PreviewDoc;
  /**
   * **The typeface the printer will use**, as a Windows family name — so the
   * preview is in the face the paper will be (2026-08-17). Empty for the
   * built-in one, which the screen draws in its own monospace stack.
   */
  font?: string;
  /**
   * Whether that face has one width for every character. A proportional one is
   * laid out by the layout's BOXES rather than by the spaces it padded with —
   * see `Segments`.
   */
  monospace?: boolean;
}) {
  return (
    <div className="mb-receipt">
      <pre
        className={[
          'mb-receipt__paper',
          monospace ? '' : 'mb-receipt__paper--proportional',
        ]
          .filter(Boolean)
          .join(' ')}
        data-columns={doc.columns}
        aria-label="Preview of the printed bill"
        /* The face by NAME, which is a value out of Rust and not a design
           choice — the same argument as the per-line size below. */
        style={font ? { fontFamily: `${font}, monospace` } : undefined} /* mb-tokens-allow: the shop's chosen printer face, named by Rust */
      >
        {doc.lines.map((line, index) => (
          <Line key={index} line={line} monospace={monospace} />
        ))}
      </pre>
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

function Line({ line, monospace }: { line: PreviewLine; monospace: boolean }) {
  // One arm per variant. An arm that quietly did nothing would be a sink
  // forgetting a block — the exact thing D29 exists to prevent — so every
  // variant is handled and the ones with nothing to draw say so out loud.
  switch (line.kind) {
    case 'text':
      /*
        **A proportional face is aligned by its BOXES, not by its spaces.**

        The layout pads a line with spaces so the ESC/POS text sink — which
        prints with the printer's own fixed font — gets a line it can send
        straight out. In Times New Roman a space is about a third of a digit,
        so those same spaces put an amount nowhere near the right edge.

        `mb_print::layout::Segment` says where each box is and how the text
        sits in it, and `raster.rs` draws the paper from exactly those numbers.
        Laying the boxes out at the same widths here is what keeps the preview
        and the paper agreeing about where a figure lands.
      */
      if (!monospace && line.segments.length > 0) {
        return (
          <span className="mb-receipt__line">
            <Indent columns={line.indent} />
            {line.segments.map((segment, index) => (
              <span
                key={index}
                className={[
                  'mb-receipt__text',
                  'mb-receipt__box',
                  `mb-receipt__box--${segment.align}`,
                  line.bold ? 'mb-receipt__line--bold' : '',
                ]
                  .filter(Boolean)
                  .join(' ')}
                style={{ ['--receipt-px' as string]: line.px, ['--receipt-cols' as string]: segment.width }} /* mb-tokens-allow: a box width and size computed by Rust, not design values */
              >
                {segment.text}
              </span>
            ))}
            {'\n'}
          </span>
        );
      }
      return (
        <span className="mb-receipt__line">
          <Indent columns={line.indent} />
          <span
            className={[
              'mb-receipt__text',
              line.bold ? 'mb-receipt__line--bold' : '',
            ]
              .filter(Boolean)
              .join(' ')}
            /* **The one inline style in this product, and it is data.**
             *
             * A size used to be 1x, 2x or 3x, so three CSS classes covered it.
             * A shop chooses from twenty-two heights now (2026-08-17), and the
             * height is a NUMBER RUST COMPUTED for this line — not a design
             * value somebody typed, which is what `check-tokens.mjs` exists to
             * keep out. The alternative is twenty-two hand-written classes
             * that have to be kept in step with `catalog.rs` by hand, which is
             * the kind of second list this codebase keeps deleting.
             *
             * The relative size is `px / one cell`, so the preview has the same
             * proportions the paper will.
             */
            style={{ ['--receipt-px' as string]: line.px }} /* mb-tokens-allow: a per-line size computed by Rust, not a design value */
          >
            {line.text}
          </span>
          {'\n'}
        </span>
      );

    case 'rule':
      /* **A separator is always on the printer's own grid, in both places.**
         `raster.rs` draws a rule with the paper's cell and never with the
         chosen face's size — a line across the bill is as wide as the paper,
         not as wide as whatever section is above it. So the preview draws it
         in the monospace stack too: rendered in Times New Roman the same count
         of dashes came out two thirds of the width, and a preview whose rules
         stop short of the ones on the paper is a preview that lies. */
      return (
        <span className="mb-receipt__line mb-receipt__rule">
          <Indent columns={line.indent} />
          {line.glyph.repeat(line.width)}
          {'\n'}
        </span>
      );

    case 'qr':
      // The printer draws a real square (D36). On screen the payload is what a
      // person can actually check — the same call the text sink made, and for
      // the same reason: a URI you can read beats a blank space.
      return (
        <span className="mb-receipt__qr">
          <Indent columns={line.indent} />
          {line.payload}
          {'\n'}
        </span>
      );

    case 'barcode':
      // **This arm was missing, and it is exactly the failure D29 describes.**
      // The switch simply fell off the end, returned `undefined`, and React
      // drew nothing — so a shop that turned on `receipt.bill_barcode` saw a
      // preview that did not have the barcode the paper would. A sink forgot a
      // block, silently, for as long as the setting existed.
      //
      // The printer draws real bars (`GS k`); the screen shows the characters,
      // which is the same call the QR arm makes and for the same reason.
      return (
        <span className="mb-receipt__barcode">
          <Indent columns={line.indent} />
          {line.payload}
          {'\n'}
        </span>
      );

    case 'logo':
      return (
        <span className="mb-receipt__logo">
          <Indent columns={line.indent} />[ logo ]{'\n'}
        </span>
      );

    case 'blank':
      return <span>{'\n'}</span>;

    default:
      // **The compiler now fails the build if a variant is added and not
      // drawn.** `line` narrows to `never` only when every arm above exists;
      // the moment one does not, this assignment is a type error at the line
      // that caused it. That is the guard the missing barcode arm needed, and
      // it costs three lines.
      return assertDrawn(line);
  }
}

function assertDrawn(line: never): never {
  throw new Error(
    `The bill preview has no way to draw ${JSON.stringify(line)}. ` +
      'Every laid-out line must reach the screen (D29).',
  );
}

/**
 * **The indent is in PAPER columns, so it is drawn in paper columns.**
 *
 * It used to be spaces inside the line's own span, and a `2×` heading made
 * those spaces `2×` too — so a centred shop name was pushed sideways by twice
 * what the layout asked for, and a big line could push the whole preview wider
 * than the paper it claims to be. Found by putting a kitchen ticket (whose
 * items are `2×` by default) next to its settings.
 *
 * `mb_print::layout` decided this number against a 48-column grid; this span
 * renders it at the base character width and never at the line's.
 */
function Indent({ columns }: { columns: number }) {
  if (columns === 0) return null;
  return <span className="mb-receipt__indent">{' '.repeat(columns)}</span>;
}
