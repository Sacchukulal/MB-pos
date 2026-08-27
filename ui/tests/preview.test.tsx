/** Anti-drift, across the IPC boundary. */

import { readFileSync } from 'node:fs';

import { render, screen, cleanup } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { Receipt } from '../src/preview/Receipt';
import type { PreviewDoc } from '../src/ipc/generated/PreviewDoc';
import type { PreviewLine } from '../src/ipc/generated/PreviewLine';

afterEach(cleanup);

/** One line of body text, as Rust sends it. */
function text(
  content: string,
  extra: Partial<Extract<PreviewLine, { kind: 'text' }>> = {},
): PreviewLine {
  return {
    kind: 'text',
    text: content,
    indent: 0,
    row: 24,
    cap: 15,
    advance: 13,
    scale: 1,
    bold: false,
    segments: [{ text: content.trim(), width: content.length, align: 'left' }],
    ...extra,
  };
}

const BILL: PreviewDoc = {
  dots: 576,
  columns: 44,
  millimetres: 64,
  engine: 'raster',
  lines: [
    text('       ANNA KUTEERA       ', {
      cap: 26,
      row: 40,
      advance: 22,
      scale: 2,
      bold: true,
      segments: [{ text: 'ANNA KUTEERA', width: 26, align: 'centre' }],
    }),
    {
      kind: 'rule',
      indent: 0,
      width: 576,
      row: 9,
      thickness: 1,
      strokes: 2,
      gap: 3,
      dash: null,
    },
    text('Masala Dosa                     240.00', {
      segments: [
        { text: 'Masala Dosa', width: 38, align: 'left' },
        { text: '240.00', width: 6, align: 'right' },
      ],
    }),
    {
      kind: 'rule',
      indent: 0,
      width: 576,
      row: 9,
      thickness: 1,
      strokes: 1,
      gap: 0,
      dash: [6, 4],
    },
    {
      kind: 'qr',
      payload: 'upi://pay?pa=anna@upi&cu=INR',
      indent: 0,
      row: 230,
      size: 230,
    },
    { kind: 'barcode', payload: 'BIR1207', indent: 0, row: 84, height: 60 },
    {
      kind: 'logo',
      indent: 0,
      row: 32,
      left: 0,
      width: 4,
      height: 2,
      ink: [1, 0, 0, 1, 0, 1, 1, 0],
    },
    { kind: 'blank', row: 24 },
  ],
  notes: ['Size 10 does not fit this paper here, so it printed at 8.'],
};

describe('the receipt preview is a sink, not a renderer', () => {
  it('shows every line the layout produced, unchanged', () => {
    const { container } = render(<Receipt doc={BILL} />);
    const shown = container.textContent ?? '';

    for (const line of BILL.lines) {
      if (line.kind === 'text') {
        for (const segment of line.segments) {
          expect(
            shown,
            `the preview dropped ${JSON.stringify(segment.text)}`,
          ).toContain(segment.text);
        }
      }
    }
  });

  it('draws every box at the dots the layout put it at', () => {
    const { container } = render(<Receipt doc={BILL} />);
    const boxes = [...container.querySelectorAll('.mb-receipt__box')];
    const amount = boxes.find((b) => b.textContent === '240.00');
    expect(amount, 'the amount was not drawn').toBeTruthy();
    expect(amount?.getAttribute('style')).toContain('--at: 494');
    expect(amount?.getAttribute('style')).toContain('--wide: 78');
    expect(amount?.className).toContain('mb-receipt__box--right');
  });

  it('draws text at the cap height the printer will draw', () => {
    const { container } = render(<Receipt doc={BILL} />);
    const heading = [...container.querySelectorAll('.mb-receipt__box')].find(
      (b) => b.textContent === 'ANNA KUTEERA',
    );
    // 26 dots of capital, which is what the shop chose and what comes out.
    expect(heading?.getAttribute('style')).toContain('--cap: 26');
  });

  it('spends the row the paper will spend', () => {
    const { container } = render(<Receipt doc={BILL} />);
    const rows = [...container.querySelectorAll('.mb-receipt__row')];
    expect(rows).toHaveLength(BILL.lines.length);
    expect(rows[0]?.getAttribute('style')).toContain('--tall: 40');
    expect(rows[1]?.getAttribute('style')).toContain('--tall: 9');
  });

  it('draws a rule as a rule, not as a row of characters', () => {
    const { container } = render(<Receipt doc={BILL} />);
    const rules = [...container.querySelectorAll('.mb-receipt__rule')];
    // Two strokes for the double rule, one for the dashed.
    expect(rules).toHaveLength(3);
    expect(rules[0]?.getAttribute('style')).toContain('--wide: 576');
    expect(rules[2]?.getAttribute('data-dashed')).toBe('yes');
    expect(rules[2]?.getAttribute('style')).toContain('--on: 6');
  });

  it('draws the logo at the size it will print', () => {
    const { container } = render(<Receipt doc={BILL} />);
    const logo = container.querySelector('.mb-receipt__logo');
    expect(logo, 'the logo was not drawn').toBeTruthy();
    expect(logo?.getAttribute('style')).toContain('--wide: 4');
    expect(logo?.getAttribute('style')).toContain('--tall: 2');
  });

  it('says how much paper the bill costs', () => {
    render(<Receipt doc={BILL} />);
    expect(screen.getByText(/64 mm of paper/)).toBeInTheDocument();
  });

  it('says which engine it is showing', () => {
    render(<Receipt doc={{ ...BILL, engine: 'text' }} />);
    expect(screen.getByText(/the printer's own font/)).toBeInTheDocument();
  });

  it('explains what the layout had to do, in words', () => {
    render(<Receipt doc={BILL} />);
    expect(screen.getByText(/does not fit this paper/)).toBeInTheDocument();
  });

  it('draws one thing per line, for every kind there is', () => {
    // The claim the fixture makes, checked instead of asserted.
    const SAMPLES: Record<PreviewLine['kind'], PreviewLine> = {
      text: text('Masala Dosa'),
      rule: {
        kind: 'rule',
        indent: 0,
        width: 100,
        row: 9,
        thickness: 1,
        strokes: 1,
        gap: 0,
        dash: null,
      },
      qr: { kind: 'qr', payload: 'upi://pay', indent: 0, row: 200, size: 200 },
      barcode: { kind: 'barcode', payload: 'BIR1207', indent: 0, row: 84, height: 60 },
      logo: { kind: 'logo', indent: 0, row: 20, left: 0, width: 2, height: 2, ink: [1, 0, 0, 1] },
      band: {
        kind: 'band',
        row: 60,
        image: { kind: 'logo', indent: 0, row: 60, left: 0, width: 2, height: 2, ink: [1, 1, 1, 1] },
        lines: [
          {
            text: 'SADGURU',
            left: 173,
            top: 0,
            width: 403,
            row: 40,
            cap: 26,
            bold: true,
            align: 'centre',
          },
        ],
      },
      blank: { kind: 'blank', row: 24 },
    };

    for (const [kind, line] of Object.entries(SAMPLES)) {
      cleanup();
      const doc: PreviewDoc = {
        dots: 576,
        columns: 44,
        millimetres: 10,
        engine: 'raster',
        lines: [line],
        notes: [],
      };
      const { container } = render(<Receipt doc={doc} />);
      const paper = container.querySelector('.mb-receipt__paper');
      expect(paper, `${kind} did not render`).not.toBeNull();
      // A blank line is a gap and nothing else; every other kind must put something on the
      // paper — ink, a rule, or a picture.
      if (kind !== 'blank') {
        const drew =
          (paper?.textContent ?? '').trim().length > 0 ||
          (paper?.querySelectorAll(
            '.mb-receipt__rule, .mb-receipt__logo, .mb-receipt__qr, .mb-receipt__barcode',
          ).length ?? 0) > 0;
        expect(drew, `a "${kind}" line drew nothing at all`).toBe(true);
      }
    }
  });

  it('shows the letterhead beside the logo', () => {
    cleanup();
    const doc: PreviewDoc = {
      dots: 576,
      columns: 44,
      millimetres: 10,
      engine: 'raster',
      lines: [
        {
          kind: 'band',
          row: 60,
          image: {
            kind: 'logo',
            indent: 0,
            row: 60,
            left: 0,
            width: 2,
            height: 2,
            ink: [1, 1, 1, 1],
          },
          lines: [
            {
              text: 'SADGURU',
              left: 173,
              top: 0,
              width: 403,
              row: 40,
              cap: 26,
              bold: true,
              align: 'centre',
            },
          ],
        },
      ],
      notes: [],
    };
    const { container } = render(<Receipt doc={doc} />);
    expect(container.textContent).toContain('SADGURU');
    const box = container.querySelector('.mb-receipt__box');
    expect(box?.getAttribute('style')).toContain('--at: 173');
  });
});

/** The paper is white and the ink is black, whatever the theme is. */
describe('the preview draws on paper, not on the theme', () => {
  const CSS = readFileSync('src/preview/receipt.css', 'utf8');

  /** One rule's body, by selector. */
  function ruleFor(selector: string): string {
    const at = CSS.indexOf(`${selector} {`);
    expect(at, `${selector} is not in receipt.css any more`).toBeGreaterThan(-1);
    return CSS.slice(at, CSS.indexOf('}', at));
  }

  it('paints the roll with the print tokens and not the surface ones', () => {
    const paper = ruleFor('.mb-receipt__paper');
    expect(paper).toContain('background: var(--print-paper)');
    expect(paper).toContain('color: var(--print-ink)');
    // The two that would follow the theme, and did.
    expect(paper).not.toContain('var(--surface)');
    expect(paper).not.toContain('color: var(--text)');
  });

  /**
   * Nothing drawn ON the paper may take a themed colour either — a QR in `--text` is a QR that
   * goes white-on-white the moment the app is dark.
   */
  it('leaves no themed colour anywhere on the roll', () => {
    for (const selector of [
      '.mb-receipt__nologo',
      '.mb-receipt__qr',
      '.mb-receipt__code',
    ]) {
      const body = ruleFor(selector);
      // Only what PAINTS. `font-size: var(--text-sm)` is a size and the text scale still
      // applies to a preview — the rule is about colour.
      expect(body, `${selector} follows the theme`).not.toMatch(
        /(?:^|[\s;])(?:color|background(?:-color)?)\s*:\s*[^;]*var\(--(?:surface|text)[-)]/,
      );
    }
  });

  /** They are tokens, and they live where every value lives. */
  it('defines the two print colours once, outside every theme block', () => {
    const tokens = readFileSync('src/theme/tokens.css', 'utf8');
    for (const name of ['--print-paper', '--print-ink']) {
      expect(
        tokens.split(`${name}:`).length - 1,
        `${name} is defined more than once, so a theme can move it`,
      ).toBe(1);
    }
  });
});
