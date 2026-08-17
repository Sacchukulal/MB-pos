/**
 * **T8 — anti-drift, across the IPC boundary.**
 *
 * P06's T1 proved no *sink* can drop anything. This proves the same across the
 * wire: everything the layout produced reaches the screen, in order, unchanged.
 *
 * > Audit D1: *"the same bill is drawn three separate times, by hand, in three
 * > places… this is the single biggest source of 'the preview does not match
 * > the paper'."*
 *
 * The fixture is the shape Rust really sends. `src-tauri/src/preview.rs` tests
 * the other half — that its conversion carries every line of a real `Laid`
 * through character for character. Between that test and this one, the chain
 * from `mb_print::layout` to a pixel has no gap in it.
 */

import { render, screen, cleanup } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { Receipt } from '../src/preview/Receipt';
import type { PreviewDoc } from '../src/ipc/generated/PreviewDoc';
import type { PreviewLine } from '../src/ipc/generated/PreviewLine';

afterEach(cleanup);

const BILL: PreviewDoc = {
  columns: 48,
  lines: [
    {
      kind: 'text',
      text: '                  ANNA KUTEERA',
      indent: 0,
      scale: 2,
      px: 48,
      segments: [],
      bold: true,
    },
    { kind: 'rule', glyph: '=', width: 48, indent: 0 },
    {
      kind: 'text',
      text: 'Bill No                             BIR/1207',
      indent: 0,
      scale: 1,
      px: 24,
      segments: [],
      bold: false,
    },
    {
      kind: 'text',
      text: 'Masala Dosa                           240.00',
      indent: 0,
      scale: 1,
      px: 24,
      segments: [],
      bold: false,
    },
    {
      kind: 'text',
      text: 'Beer 650ml                            440.00',
      indent: 0,
      scale: 1,
      px: 24,
      segments: [],
      bold: false,
    },
    { kind: 'rule', glyph: '-', width: 48, indent: 0 },
    {
      kind: 'text',
      text: 'TOTAL                                 646.00',
      indent: 0,
      scale: 2,
      px: 48,
      segments: [],
      bold: true,
    },
    { kind: 'blank' },
    { kind: 'qr', payload: 'upi://pay?pa=anna@upi&am=646.00', indent: 0 },
    { kind: 'barcode', payload: 'BIR1207', indent: 0 },
    { kind: 'logo', indent: 0 },
  ],
  notes: [
    'A heading was too big for this paper, so it printed at 2× instead of 3×.',
  ],
};

describe('the receipt preview is a sink, not a renderer', () => {
  it('shows every line the layout produced, unchanged', () => {
    const { container } = render(<Receipt doc={BILL} />);
    const shown = container.textContent ?? '';

    for (const line of BILL.lines) {
      if (line.kind === 'text') {
        expect(
          shown,
          `the preview dropped ${JSON.stringify(line.text)}`,
        ).toContain(line.text);
      }
    }
  });

  it('does not re-wrap, re-align or trim anything', () => {
    // The padding IS the alignment: `mb_print::layout` centred that heading by
    // padding it, and a preview that trimmed would be centring it again — a
    // second layout engine, which is D1 coming back by a different route.
    const { container } = render(<Receipt doc={BILL} />);
    expect(container.textContent).toContain('                  ANNA KUTEERA');
  });

  it('draws a separator at exactly the width the layout gave it', () => {
    const { container } = render(<Receipt doc={BILL} />);
    expect(container.textContent).toContain('='.repeat(48));
    expect(container.textContent).toContain('-'.repeat(48));
  });

  it('shows the QR payload, because a URI a customer can type beats a blank', () => {
    render(<Receipt doc={BILL} />);
    expect(screen.getByText(/upi:\/\/pay/)).toBeInTheDocument();
  });

  it('explains what the layout had to do, in words', () => {
    render(<Receipt doc={BILL} />);
    expect(screen.getByText(/too big for this paper/)).toBeInTheDocument();
  });

  it('is exactly as wide as the paper it is previewing', () => {
    const { container } = render(<Receipt doc={BILL} />);
    expect(container.querySelector('.mb-receipt__paper')).toHaveAttribute(
      'data-columns',
      '48',
    );
  });

  it('handles every kind of line, including the ones it cannot draw', () => {
    // A sink that quietly ignored a block would be the exact failure D29
    // exists to prevent, so the logo gets a visible placeholder rather than
    // nothing at all.
    const { container } = render(<Receipt doc={BILL} />);
    expect(container.textContent).toContain('[ logo ]');
  });

  it('shows the barcode payload, for the same reason as the QR', () => {
    // **This is the test that was missing.** The component had no `barcode`
    // arm at all: the switch fell off the end, returned `undefined`, and a
    // shop with `receipt.bill_barcode` on saw a preview that did not have what
    // the paper would. The fixture above claimed to hold "every kind of line"
    // and did not hold this one.
    render(<Receipt doc={BILL} />);
    expect(screen.getByText('BIR1207')).toBeInTheDocument();
  });

  it('draws one thing per line, for every kind there is', () => {
    // **The claim the fixture makes, checked instead of asserted.**
    //
    // `SAMPLES` is keyed by `PreviewLine['kind']`, so it is the TYPE CHECKER
    // that enforces the "every kind" part: add a variant in Rust, regenerate,
    // and this object stops compiling until somebody has decided what it looks
    // like on screen. A fixture nobody updated is what let the barcode through.
    const SAMPLES: Record<PreviewLine['kind'], PreviewLine> = {
      text: { kind: 'text', text: 'Masala Dosa', indent: 0, scale: 1, px: 24, segments: [], bold: false },
      rule: { kind: 'rule', glyph: '-', width: 8, indent: 0 },
      qr: { kind: 'qr', payload: 'upi://pay', indent: 0 },
      barcode: { kind: 'barcode', payload: 'BIR1207', indent: 0 },
      logo: { kind: 'logo', indent: 0 },
      blank: { kind: 'blank' },
    };

    for (const [kind, line] of Object.entries(SAMPLES)) {
      cleanup();
      const doc: PreviewDoc = { columns: 48, lines: [line], notes: [] };
      const { container } = render(<Receipt doc={doc} />);
      const paper = container.querySelector('.mb-receipt__paper');
      expect(paper, `${kind} did not render`).not.toBeNull();
      // A blank line is a newline and nothing else; every other kind must put
      // something a person can see on the paper.
      if (kind !== 'blank') {
        expect(
          (paper?.textContent ?? '').trim().length,
          `a "${kind}" line drew nothing at all`,
        ).toBeGreaterThan(0);
      }
    }
  });
});
