/** Anti-drift, across the IPC boundary. */

import { readFileSync } from 'node:fs';

import { render, screen, cleanup } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { Receipt, dotsPerPixel } from '../src/preview/Receipt';
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
    cap: 17,
    advance: 12,
    scale: 1,
    bold: false,
    segments: [{ text: content.trim(), width: content.length, align: 'left' }],
    ...extra,
  };
}

/** Sixteen dots across, four rows: a rule, a gap, a two-dot bar, a gap. */
const RASTER = {
  width: 16,
  height: 4,
  // Packed rows, most significant bit leftmost.
  bits: btoa(String.fromCharCode(0xff, 0xff, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00)),
};

/** What the graphics engine sends: the raster, and the rows a test can read. */
const BILL: PreviewDoc = {
  dots: 576,
  columns: 48,
  millimetres: 64,
  engine: 'raster',
  lines: [text('ANNA KUTEERA'), text('Masala Dosa                     240.00')],
  notes: ['Size 10 does not fit this paper here, so it printed at 8.'],
  raster: RASTER,
};

/** What the text engine sends: no raster, rows of the printer's own characters. */
const ROM: PreviewDoc = {
  dots: 576,
  columns: 48,
  millimetres: 64,
  engine: 'text',
  lines: [
    text('       ANNA KUTEERA       ', {
      row: 48,
      cap: 34,
      advance: 24,
      scale: 2,
      bold: true,
      segments: [{ text: 'ANNA KUTEERA', width: 24, align: 'centre' }],
    }),
    { kind: 'rule', glyphs: '='.repeat(48), indent: 0, row: 24, advance: 12 },
    text('Masala Dosa                     240.00', {
      segments: [
        { text: 'Masala Dosa', width: 42, align: 'left' },
        { text: '240.00', width: 6, align: 'right' },
      ],
    }),
    { kind: 'qr', payload: 'upi://pay?pa=anna@upi&cu=INR', indent: 0, row: 230, size: 230 },
    { kind: 'barcode', payload: 'BIR1207', indent: 0, row: 84, height: 60 },
    { kind: 'blank', row: 24 },
  ],
  notes: [],
  raster: null,
};

describe('the graphics engine preview is the printer raster on one canvas', () => {
  it('draws one canvas with exactly the raster dots, and no text of its own', () => {
    const { container } = render(<Receipt doc={BILL} />);
    const canvases = container.querySelectorAll('canvas');
    expect(canvases).toHaveLength(1);
    const canvas = canvases[0];
    expect(canvas?.getAttribute('width')).toBe('16');
    expect(canvas?.getAttribute('height')).toBe('4');
    expect(canvas?.className).toContain('mb-receipt__paper--raster');
    // No second renderer: none of the layout's text is drawn as HTML.
    expect(container.querySelectorAll('.mb-receipt__box')).toHaveLength(0);
    expect(container.querySelectorAll('.mb-receipt__row')).toHaveLength(0);
    expect(container.textContent).not.toContain('Masala Dosa');
  });

  it('sizes the canvas at a whole or half number of dots per pixel, never a fraction', () => {
    const { container } = render(<Receipt doc={BILL} />);
    const canvas = container.querySelector('canvas');
    const ratio = Number(canvas?.getAttribute('data-dots-per-pixel'));
    expect(ratio * 2, `${ratio} dots a pixel is a fraction`).toBe(Math.round(ratio * 2));
    expect(ratio).toBeGreaterThanOrEqual(0.5);
    const style = canvas?.getAttribute('style') ?? '';
    expect(style).toContain(`width: ${16 / ratio}px`);
    expect(style).toContain(`height: ${4 / ratio}px`);
  });

  it('chooses the finest ratio that fits the room it has', () => {
    // 576 dots in 600 px: one dot a pixel fits; in 300 px it takes two; in 400, one and a half.
    expect(dotsPerPixel(576, 600)).toBe(1);
    expect(dotsPerPixel(576, 300)).toBe(2);
    expect(dotsPerPixel(576, 400)).toBe(1.5);
    expect(dotsPerPixel(576, 1200)).toBe(0.5);
    // Nothing measured yet is a sensible default, not a division by zero.
    expect(dotsPerPixel(576, 0)).toBe(2);
    // And nothing coarser than the ceiling, however narrow the screen.
    expect(dotsPerPixel(832, 10)).toBe(8);
    for (const available of [97, 233, 401, 777]) {
      const ratio = dotsPerPixel(576, available);
      expect(ratio * 2).toBe(Math.round(ratio * 2));
    }
  });

  it('says how much paper the bill costs, and which engine', () => {
    render(<Receipt doc={BILL} />);
    expect(screen.getByText(/64 mm of paper/)).toBeInTheDocument();
    expect(screen.getByText(/graphics/)).toBeInTheDocument();
  });

  it('explains what the layout had to do, in words', () => {
    render(<Receipt doc={BILL} />);
    expect(screen.getByText(/does not fit this paper/)).toBeInTheDocument();
  });
});

describe('the text engine preview is the rows the printer prints', () => {
  it('has no canvas and shows every line the layout produced, unchanged', () => {
    const { container } = render(<Receipt doc={ROM} />);
    expect(container.querySelectorAll('canvas')).toHaveLength(0);
    const shown = container.textContent ?? '';
    for (const line of ROM.lines) {
      if (line.kind === 'text') {
        for (const segment of line.segments) {
          expect(shown, `the preview dropped ${JSON.stringify(segment.text)}`).toContain(
            segment.text,
          );
        }
      }
    }
  });

  it('draws every box at the dots the layout put it at', () => {
    const { container } = render(<Receipt doc={ROM} />);
    const boxes = [...container.querySelectorAll('.mb-receipt__box')];
    const amount = boxes.find((b) => b.textContent === '240.00');
    expect(amount, 'the amount was not drawn').toBeTruthy();
    expect(amount?.getAttribute('style')).toContain('--at: 504');
    expect(amount?.getAttribute('style')).toContain('--wide: 72');
    expect(amount?.className).toContain('mb-receipt__box--right');
  });

  it('spends the row the paper will spend', () => {
    const { container } = render(<Receipt doc={ROM} />);
    const rows = [...container.querySelectorAll('.mb-receipt__row')];
    expect(rows).toHaveLength(ROM.lines.length);
    expect(rows[0]?.getAttribute('style')).toContain('--tall: 48');
    expect(rows[1]?.getAttribute('style')).toContain('--tall: 24');
  });

  it("draws a separator as the printer's own character row", () => {
    const { container } = render(<Receipt doc={ROM} />);
    const rule = [...container.querySelectorAll('.mb-receipt__box')].find(
      (b) => b.textContent === '='.repeat(48),
    );
    expect(rule, 'the rule was not drawn as characters').toBeTruthy();
    expect(rule?.getAttribute('style')).toContain('--wide: 576');
    expect(container.querySelector('.mb-receipt__rule')).toBeNull();
  });

  it('shows the square and the bars at the size the printer will make them', () => {
    const { container } = render(<Receipt doc={ROM} />);
    expect(container.querySelector('.mb-receipt__qr')?.getAttribute('style')).toContain(
      '--wide: 230',
    );
    expect(container.querySelector('.mb-receipt__barcode')?.getAttribute('style')).toContain(
      '--tall: 60',
    );
    expect(container.textContent).toContain('BIR1207');
  });

  it('says which engine it is showing', () => {
    render(<Receipt doc={ROM} />);
    expect(screen.getByText(/the printer's own font/)).toBeInTheDocument();
  });

  it('draws one thing per line, for every kind there is', () => {
    const SAMPLES: Record<PreviewLine['kind'], PreviewLine> = {
      text: text('Masala Dosa'),
      rule: { kind: 'rule', glyphs: '----------', indent: 0, row: 24, advance: 12 },
      qr: { kind: 'qr', payload: 'upi://pay', indent: 0, row: 200, size: 200 },
      barcode: { kind: 'barcode', payload: 'BIR1207', indent: 0, row: 84, height: 60 },
      blank: { kind: 'blank', row: 24 },
    };

    for (const [kind, line] of Object.entries(SAMPLES)) {
      cleanup();
      const doc: PreviewDoc = { ...ROM, lines: [line], notes: [] };
      const { container } = render(<Receipt doc={doc} />);
      const paper = container.querySelector('.mb-receipt__paper');
      expect(paper, `${kind} did not render`).not.toBeNull();
      // A blank line is a gap and nothing else; every other kind must put something on the
      // paper — characters, or a placeholder for what the printer draws itself.
      if (kind !== 'blank') {
        const drew =
          (paper?.textContent ?? '').trim().length > 0 ||
          (paper?.querySelectorAll('.mb-receipt__qr, .mb-receipt__barcode').length ?? 0) > 0;
        expect(drew, `a "${kind}" line drew nothing at all`).toBe(true);
      }
    }
  });
});

/** The paper is white and the ink is black, whatever the theme is. */
describe('the preview draws on paper, not on the theme', () => {
  const CSS = readFileSync('src/preview/receipt.css', 'utf8');
  const TSX = readFileSync('src/preview/Receipt.tsx', 'utf8');

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

  it('draws the raster dot for dot, from the two print tokens', () => {
    // Shrunk, the dots are averaged (paper from a step back); at one dot a pixel, every dot.
    expect(ruleFor('.mb-receipt__paper--raster')).toContain('image-rendering: auto');
    expect(CSS).toContain("[data-dots-per-pixel='1'] {");
    expect(CSS).toContain('  image-rendering: pixelated;');
    expect(TSX).toContain("getPropertyValue('--print-ink')");
    expect(TSX).toContain("getPropertyValue('--print-paper')");
  });

  /** The second renderer is gone: no cap-height ratio, no font handed in from outside. */
  it('guesses nothing about the face', () => {
    expect(CSS).not.toMatch(/0\.7\d/);
    expect(TSX).not.toContain('fontFamily');
    expect(TSX).not.toMatch(/font\?:/);
  });

  /**
   * Nothing drawn ON the paper may take a themed colour either — a QR in `--text` is a QR that
   * goes white-on-white the moment the app is dark.
   */
  it('leaves no themed colour anywhere on the roll', () => {
    for (const selector of ['.mb-receipt__qr', '.mb-receipt__code']) {
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
