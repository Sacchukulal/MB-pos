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

export function Receipt({ doc }: { doc: PreviewDoc }) {
  return (
    <div className="mb-receipt">
      <pre
        className="mb-receipt__paper"
        data-columns={doc.columns}
        aria-label="Preview of the printed bill"
      >
        {doc.lines.map((line, index) => (
          <Line key={index} line={line} />
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

function Line({ line }: { line: PreviewLine }) {
  // One arm per variant. An arm that quietly did nothing would be a sink
  // forgetting a block — the exact thing D29 exists to prevent — so every
  // variant is handled and the ones with nothing to draw say so out loud.
  switch (line.kind) {
    case 'text':
      return (
        <span
          className={[
            'mb-receipt__line',
            line.scale > 1 ? `mb-receipt__line--x${line.scale}` : '',
            line.bold ? 'mb-receipt__line--bold' : '',
          ]
            .filter(Boolean)
            .join(' ')}
        >
          {indent(line.indent)}
          {line.text}
          {'\n'}
        </span>
      );

    case 'rule':
      return (
        <span className="mb-receipt__line">
          {indent(line.indent)}
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
          {indent(line.indent)}
          {line.payload}
          {'\n'}
        </span>
      );

    case 'logo':
      return (
        <span className="mb-receipt__logo">
          {indent(line.indent)}[ logo ]{'\n'}
        </span>
      );

    case 'blank':
      return <span>{'\n'}</span>;
  }
}

function indent(columns: number): string {
  return ' '.repeat(columns);
}
