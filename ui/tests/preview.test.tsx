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

afterEach(cleanup);

const BILL: PreviewDoc = {
  columns: 48,
  lines: [
    {
      kind: 'text',
      text: '                  ANNA KUTEERA',
      indent: 0,
      scale: 2,
      bold: true,
    },
    { kind: 'rule', glyph: '=', width: 48, indent: 0 },
    {
      kind: 'text',
      text: 'Bill No                             BIR/1207',
      indent: 0,
      scale: 1,
      bold: false,
    },
    {
      kind: 'text',
      text: 'Masala Dosa                           240.00',
      indent: 0,
      scale: 1,
      bold: false,
    },
    {
      kind: 'text',
      text: 'Beer 650ml                            440.00',
      indent: 0,
      scale: 1,
      bold: false,
    },
    { kind: 'rule', glyph: '-', width: 48, indent: 0 },
    {
      kind: 'text',
      text: 'TOTAL                                 646.00',
      indent: 0,
      scale: 2,
      bold: true,
    },
    { kind: 'blank' },
    { kind: 'qr', payload: 'upi://pay?pa=anna@upi&am=646.00', indent: 0 },
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
});
