/**
 * **T2, T3, T4 and T5 — the billing screen.**
 *
 * The Rust side proves the figures (`src-tauri/src/billing.rs`); this proves
 * what the screen does with them: that the four table states are told apart
 * without colour, that the density mechanism engages, that a parcel order is
 * never lost, and that the totals block does not collapse.
 */

import { render, screen, cleanup, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { DENSE_ABOVE, TableGrid } from '../src/billing/TableGrid';
import { Totals } from '../src/billing/Totals';
import type { BillView } from '../src/ipc/generated/BillView';
import type { MoneyView } from '../src/ipc/generated/MoneyView';
import type { TableView } from '../src/ipc/generated/TableView';

afterEach(cleanup);

function money(paise: number, text: string): MoneyView {
  return { paise: BigInt(paise), text };
}

function table(over: Partial<TableView> & Pick<TableView, 'id' | 'label'>): TableView {
  return {
    section: 'Main Hall',
    seats: 4n,
    state: 'free',
    total: null,
    minutes: null,
    kitchenTold: true,
    orderId: null,
    ...over,
  };
}

describe('the table grid (scope 1.4)', () => {
  it('tells all four states apart WITHOUT colour', () => {
    // §2 rule 2: "colour is never the only signal." The class carries the
    // form — dashed vs solid, stripe, ring — and a grey-scale reading of the
    // screen must still distinguish them.
    const { container } = render(
      <TableGrid
        tables={[
          table({ id: '1', label: '1', state: 'free' }),
          table({ id: '2', label: '2', state: 'occupied', total: money(64_600, '646.00') }),
          table({ id: '3', label: '3', state: 'late', minutes: 47n }),
          table({ id: '4', label: '4', state: 'loaded' }),
        ]}
        filter=""
        onOpen={vi.fn()}
      />,
    );

    for (const state of ['free', 'occupied', 'late', 'loaded']) {
      expect(
        container.querySelector(`.mb-tile--${state}`),
        `the ${state} state has no form of its own`,
      ).toBeTruthy();
    }
  });

  it('names every tile for a screen reader, not just "6"', () => {
    render(
      <TableGrid
        tables={[
          table({
            id: '6',
            label: '6',
            state: 'late',
            total: money(64_600, '646.00'),
            minutes: 47n,
          }),
        ]}
        filter=""
        onOpen={vi.fn()}
      />,
    );
    const tile = screen.getByRole('button');
    expect(tile.getAttribute('aria-label')).toContain('waiting a long time');
    expect(tile.getAttribute('aria-label')).toContain('646.00');
  });

  it('steps down a density past the threshold, and not before', () => {
    // T3. §4: "a busy shop has 40+ tables. Tiles must stay readable at that
    // count without scrolling becoming the interaction."
    const many = (count: number) =>
      Array.from({ length: count }, (_, n) =>
        table({ id: `t${n}`, label: `${n + 1}` }),
      );

    const small = render(
      <TableGrid tables={many(4)} filter="" onOpen={vi.fn()} />,
    );
    expect(small.container.querySelector('[data-dense="true"]')).toBeNull();
    cleanup();

    const big = render(
      <TableGrid tables={many(DENSE_ABOVE + 36)} filter="" onOpen={vi.fn()} />,
    );
    expect(
      big.container.querySelector('[data-dense="true"]'),
      'sixty tables did not engage the dense step',
    ).toBeTruthy();
  });

  it('never loses a parcel order — it goes in the "No table" group', () => {
    // T4, and §4: "so no order is ever invisible." This is exactly the kind of
    // order that goes missing.
    render(
      <TableGrid
        tables={[
          table({ id: '1', label: '1' }),
          table({
            id: 'ord_9',
            label: 'Parcel',
            section: null,
            state: 'occupied',
            orderId: 'ord_9',
            total: money(9_900, '99.00'),
          }),
        ]}
        filter=""
        onOpen={vi.fn()}
      />,
    );
    expect(screen.getByText('No table')).toBeInTheDocument();
    expect(screen.getByText('Parcel')).toBeInTheDocument();
  });

  it('filters, because twenty open tables is otherwise a scrolling exercise', () => {
    // Audit F5.
    render(
      <TableGrid
        tables={[table({ id: '1', label: '1' }), table({ id: '2', label: '12' })]}
        filter="12"
        onOpen={vi.fn()}
      />,
    );
    expect(screen.getByText('12')).toBeInTheDocument();
    expect(screen.queryByText('1')).toBeNull();
  });
});

describe('the totals block (audit B10 and B11)', () => {
  const bill: BillView = {
    subtotal: money(79_500, '795.00'),
    lineDiscount: money(0, '0.00'),
    billDiscount: money(5_000, '50.00'),
    totalDiscount: money(5_000, '50.00'),
    discountCapped: true,
    charges: [
      { name: 'Service Charge', amount: money(3_000, '30.00'), rateLabel: '5%' },
    ],
    taxRows: [
      {
        rateLabel: '5%',
        taxable: money(55_500, '555.00'),
        cgst: money(1_388, '13.88'),
        sgst: money(1_387, '13.87'),
        igst: money(0, '0.00'),
        isInterstate: false,
      },
      {
        rateLabel: '18%',
        taxable: money(1_695, '16.95'),
        cgst: money(153, '1.53'),
        sgst: money(152, '1.52'),
        igst: money(0, '0.00'),
        isInterstate: false,
      },
    ],
    taxTotal: money(3_080, '30.80'),
    nonGstValue: money(22_000, '220.00'),
    exemptValue: money(0, '0.00'),
    roundOff: money(25, '0.25'),
    grandTotal: money(82_300, '823.00'),
  };

  it('shows one row per rate and never collapses them into "GST"', () => {
    render(<Totals bill={bill} />);
    expect(screen.getByText('Taxable @ 5%')).toBeInTheDocument();
    expect(screen.getByText('Taxable @ 18%')).toBeInTheDocument();
    expect(screen.getByText('CGST 2.5%')).toBeInTheDocument();
    expect(screen.getByText('CGST 9%')).toBeInTheDocument();
    // The 50/50 split v1 always did, with no rates, is the thing this replaces.
    expect(screen.queryByText('GST')).toBeNull();
  });

  it('lists the non-GST value separately — the line that lets a bar bill', () => {
    // Scope 2.3, and audit B10.
    render(<Totals bill={bill} />);
    const row = screen.getByText('Non-GST value').closest('.mb-totals__row');
    expect(within(row as HTMLElement).getByText('220.00')).toBeInTheDocument();
  });

  it('says when a discount was capped (D15)', () => {
    // "A discount that had to be capped says so; the flag reaches the bill."
    // It reaches `Bill`; this is the last hop, and dropping it here would kill
    // a flag that has travelled three phases.
    render(<Totals bill={bill} />);
    expect(screen.getByText(/was reduced/)).toBeInTheDocument();
  });

  it('shows the round-off as its own figure', () => {
    render(<Totals bill={bill} />);
    expect(screen.getByText('Round off')).toBeInTheDocument();
  });

  it('shows an IGST row instead of CGST/SGST when the supply is inter-state', () => {
    // Scope 2.4 — the thing v1 could not do at all.
    render(
      <Totals
        bill={{
          ...bill,
          taxRows: [
            {
              rateLabel: '18%',
              taxable: money(10_000, '100.00'),
              cgst: money(0, '0.00'),
              sgst: money(0, '0.00'),
              igst: money(1_800, '18.00'),
              isInterstate: true,
            },
          ],
        }}
      />,
    );
    expect(screen.getByText('IGST 18%')).toBeInTheDocument();
    expect(screen.queryByText('CGST 9%')).toBeNull();
  });
});
