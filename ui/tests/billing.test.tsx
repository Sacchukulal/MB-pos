import { render, screen, cleanup, within, act, fireEvent } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { PaymentModes } from '../src/billing/Billing';
import { Processing, ProcessingHead, processingOrders } from '../src/billing/Processing';
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
    seats: 4,
    state: 'free',
    total: null,
    minutes: null,
    kitchenTold: true,
    kitchenMinutes: null,
    billAsked: false,
    orderId: null,
    billNumber: null,
    selected: false,
    ...over,
  };
}

describe('the table grid (scope 1.4)', () => {
  it('tells all four states apart WITHOUT colour', () => {
    // §2 rule 2: "colour is never the only signal." The class carries the form — dashed vs
    // solid, stripe — and a grey-scale reading of the screen must still distinguish them.
    const { container } = render(
      <TableGrid
        tables={[
          table({ id: '1', label: '1', state: 'free' }),
          table({ id: '2', label: '2', state: 'occupied', total: money(64_600, '646.00') }),
          table({ id: '3', label: '3', state: 'waiting', minutes: 20 }),
          table({ id: '4', label: '4', state: 'late', minutes: 47 }),
        ]}
        filter=""
        onOpen={vi.fn()}
        onPrintBill={vi.fn()}
      />,
    );

    for (const state of ['free', 'occupied', 'waiting', 'late']) {
      expect(
        container.querySelector(`.mb-tile--${state}`),
        `the ${state} state has no form of its own`,
      ).toBeTruthy();
    }
  });

  /** The table you are on is marked, and being on it costs nothing else. */
  it('marks the selected table WITHOUT taking its state away', () => {
    const { container } = render(
      <TableGrid
        tables={[
          table({ id: '1', label: '1', state: 'free', selected: true }),
          // And the case that would have gone wrong the other way — a late table must not stop
          // looking late because somebody opened it.
          table({ id: '2', label: '2', state: 'late', minutes: 47, selected: true }),
          table({ id: '3', label: '3', state: 'free' }),
        ]}
        filter=""
        onOpen={vi.fn()}
        onPrintBill={vi.fn()}
      />,
    );

    expect(container.querySelectorAll('.mb-tile--selected')).toHaveLength(2);
    expect(container.querySelector('.mb-tile--free.mb-tile--selected')).toBeTruthy();
    expect(
      container.querySelector('.mb-tile--late.mb-tile--selected'),
      'a selected table stopped looking late',
    ).toBeTruthy();
    // And an unselected one is left alone.
    expect(container.querySelectorAll('.mb-tile--free')).toHaveLength(2);
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
            minutes: 47,
          }),
        ]}
        filter=""
        onOpen={vi.fn()}
        onPrintBill={vi.fn()}
      />,
    );
    const tile = screen.getByRole('button');
    expect(tile.getAttribute('aria-label')).toContain('waiting a long time');
    expect(tile.getAttribute('aria-label')).toContain('646.00');
  });

  it('steps down a density past the threshold, and not before', () => {
    // "a busy shop has 40+ tables.
    const many = (count: number) =>
      Array.from({ length: count }, (_, n) =>
        table({ id: `t${n}`, label: `${n + 1}` }),
      );

    const small = render(
      <TableGrid tables={many(4)} filter="" onOpen={vi.fn()}
        onPrintBill={vi.fn()} />,
    );
    expect(small.container.querySelector('[data-dense="true"]')).toBeNull();
    cleanup();

    const big = render(
      <TableGrid tables={many(DENSE_ABOVE + 36)} filter="" onOpen={vi.fn()}
        onPrintBill={vi.fn()} />,
    );
    expect(
      big.container.querySelector('[data-dense="true"]'),
      'sixty tables did not engage the dense step',
    ).toBeTruthy();
  });

  it('never loses a parcel order — it goes in the "No table" group', () => {
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
        onPrintBill={vi.fn()}
      />,
    );
    expect(screen.getByText('No table')).toBeInTheDocument();
    expect(screen.getByText('Parcel')).toBeInTheDocument();
  });

  it('filters, because twenty open tables is otherwise a scrolling exercise', () => {
    render(
      <TableGrid
        tables={[table({ id: '1', label: '1' }), table({ id: '2', label: '12' })]}
        filter="12"
        onOpen={vi.fn()}
        onPrintBill={vi.fn()}
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

  it(`shows ONE tax figure — Rust's own sum, not a total worked out here`, () => {
    render(<Totals bill={bill} />);
    const row = screen.getByText('Tax').closest('.mb-totals__row');
    expect(within(row as HTMLElement).getByText('30.80')).toBeInTheDocument();
  });

  it('keeps the rate-by-rate breakdown OFF the till', () => {
    render(<Totals bill={bill} />);
    for (const gone of [
      'Taxable @ 5%',
      'Taxable @ 18%',
      'CGST 2.5%',
      'CGST 9%',
      'SGST 2.5%',
      'Non-GST value',
    ]) {
      expect(screen.queryByText(gone), `${gone} is on the till`).toBeNull();
    }
  });

  it('says when a discount was capped (D15)', () => {
    // "A discount that had to be capped says so; the flag reaches the bill." It reaches `Bill`;
    // this is the last hop, and dropping it here would kill a flag that has travelled three
    // phases.
    render(<Totals bill={bill} />);
    expect(screen.getByText(/was reduced/)).toBeInTheDocument();
  });

  it('shows the round-off as its own figure', () => {
    render(<Totals bill={bill} />);
    expect(screen.getByText('Round off')).toBeInTheDocument();
  });

  it('shows the subtotal, what came off, and what to say out loud', () => {
    render(<Totals bill={bill} />);
    for (const shown of ['Subtotal', 'Bill discount', 'Tax', 'Round off', 'TOTAL']) {
      expect(screen.getByText(shown), `${shown} is missing`).toBeInTheDocument();
    }
  });
});

/** An empty floor draws nothing at all. */
describe('an empty floor (P30.5)', () => {
  it('takes no space when there is nothing to show and no filter', () => {
    const { container } = render(<TableGrid tables={[]} filter="" onOpen={vi.fn()}
        onPrintBill={vi.fn()} />);
    expect(container.textContent).toBe('');
  });

  it('still says why when a filter is what emptied it', () => {
    render(<TableGrid tables={[]} filter="9" onOpen={vi.fn()}
        onPrintBill={vi.fn()} />);
    expect(screen.getByText('No table matches that')).toBeTruthy();
  });
});

/** The payment modes. */
describe('the payment modes (2026-08-23)', () => {
  /** The row is a CHOICE, not an action. */
  const mode = (label: string) =>
    screen.getByRole('button', { name: new RegExp(`^${label}`) });

  it('lights the mode it is given and no other', () => {
    render(<PaymentModes mode="Cash" onPick={vi.fn()} onCredit={vi.fn()} />);
    expect(mode('Cash').getAttribute('aria-pressed')).toBe('true');
    for (const other of ['Card', 'UPI']) {
      expect(mode(other).getAttribute('aria-pressed')).toBe('false');
    }
  });

  it('moves the light rather than taking money', () => {
    const onPick = vi.fn();
    render(<PaymentModes mode="Cash" onPick={onPick} onCredit={vi.fn()} />);
    mode('Card').click();
    expect(onPick).toHaveBeenCalledWith('Card');
  });

  it('never goes dead, because choosing a mode cannot fail', () => {
    render(<PaymentModes mode="UPI" onPick={vi.fn()} onCredit={vi.fn()} />);
    for (const label of ['Cash', 'Card', 'UPI']) expect(mode(label)).toBeEnabled();
  });

  it('says nothing at all — no essay, no clear button', () => {
    render(<PaymentModes mode="Cash" onPick={vi.fn()} onCredit={vi.fn()} />);
    expect(screen.queryByText(/paid in full/i)).toBeNull();
    expect(screen.queryByRole('button', { name: /clear payments/i })).toBeNull();
  });

  it('keeps credit behind the arrow, and it opens the picker', () => {
    const onCredit = vi.fn();
    render(<PaymentModes mode="Cash" onPick={vi.fn()} onCredit={onCredit} />);
    expect(screen.queryByRole('button', { name: /^Credit/ })).toBeNull();
    act(() => screen.getByRole('button', { name: /Show credit billing/ }).click());
    act(() => mode('Credit').click());
    expect(onCredit).toHaveBeenCalledTimes(1);
  });
});


/** The processing panel: the kitchen's orders, drawn from the same list as the grid. */
describe('the processing orders (2026-08-27)', () => {
  const cooking = (over: Partial<TableView> & Pick<TableView, 'id' | 'label'>) =>
    table({
      state: 'occupied',
      orderId: `ord_${over.id}`,
      total: money(16_800, '168.00'),
      billNumber: 'B-104',
      minutes: 12,
      ...over,
    });

  it('lists only what the kitchen has, oldest first', () => {
    const shown = processingOrders(
      [
        table({ id: '1', label: '1' }),
        cooking({ id: '2', label: '2', minutes: 5 }),
        cooking({ id: '3', label: '3', minutes: 40, state: 'late' }),
        // Open, but the kitchen has not been told: the tile's amber dot, not this list.
        cooking({ id: '4', label: '4', kitchenTold: false }),
        cooking({ id: 'p', label: 'Parcel', section: null, minutes: 20 }),
      ],
      false,
    );
    expect(shown.map((t) => t.label)).toEqual(['3', 'Parcel', '2']);
  });

  it('counts every open order for a shop with no kitchen ticket', () => {
    const shown = processingOrders(
      [table({ id: '1', label: '1' }), cooking({ id: '4', label: '4', kitchenTold: false })],
      true,
    );
    expect(shown.map((t) => t.label)).toEqual(['4']);
  });

  it('opens the order with the same press as the tile, and offers nothing else', () => {
    const onOpen = vi.fn();
    const order = cooking({ id: '3', label: '3', section: 'AC' });
    render(<Processing orders={[order]} onOpen={onOpen} />);
    fireEvent.click(screen.getByRole('button', { name: /Table 3/ }));
    expect(onOpen).toHaveBeenCalledWith(order);
    expect(screen.getAllByRole('button')).toHaveLength(1);
  });

  it('keeps the count in its head, open or folded', () => {
    const onToggle = vi.fn();
    const { rerender } = render(
      <ProcessingHead count={3} open controls="list" onToggle={onToggle} />,
    );
    const head = screen.getByRole('button', { name: /Processing orders/ });
    expect(head.getAttribute('aria-expanded')).toBe('true');
    expect(head.getAttribute('aria-controls')).toBe('list');
    expect(head.textContent).toContain('3');
    fireEvent.click(head);
    expect(onToggle).toHaveBeenCalledTimes(1);
    rerender(<ProcessingHead count={3} open={false} controls="list" onToggle={onToggle} />);
    expect(head.getAttribute('aria-expanded')).toBe('false');
    expect(head.textContent).toContain('3');
  });

  it('says where, how long, which bill and how much — and marks the one in the cart', () => {
    render(
      <Processing
        orders={[
          cooking({ id: '3', label: '3', selected: true, kitchenMinutes: 8 }),
          cooking({ id: 'p', label: 'Parcel', section: null, minutes: 70, state: 'late' }),
        ]}
        onOpen={vi.fn()}
      />,
    );
    const three = screen.getByRole('button', { name: /Table 3/ });
    expect(three.getAttribute('aria-pressed')).toBe('true');
    expect(three.textContent).toContain('12m');
    expect(three.textContent).toContain('B-104');
    expect(three.textContent).toContain('168.00');
    expect(three.textContent).toContain('8m');
    // A parcel names itself, and a late one carries the form as well as the colour.
    const parcel = screen.getByRole('button', { name: /^Parcel/ });
    expect(parcel.className).toContain('mb-processing__order--late');
    expect(parcel.textContent).toContain('1h 10m');
  });

  it('says so when nothing is cooking', () => {
    render(<Processing orders={[]} onOpen={vi.fn()} />);
    expect(screen.getByText('Nothing cooking')).toBeInTheDocument();
  });
});

/** The arrow keys walk the processing orders; the row they are on is marked, in form. */
describe('the processing orders under the arrow keys', () => {
  it('marks the highlighted row and no other', () => {
    const busy = (id: string) =>
      table({ id, label: id, state: 'occupied', orderId: `ord_${id}`, minutes: 5 });
    render(
      <Processing orders={[busy('3'), busy('4')]} highlighted={1} onOpen={vi.fn()} />,
    );
    const rows = screen.getAllByRole('button');
    expect(rows[0]?.className).not.toContain('--highlighted');
    expect(rows[1]?.className).toContain('mb-processing__order--highlighted');
    expect(rows[1]?.getAttribute('aria-current')).toBe('true');
  });
});
