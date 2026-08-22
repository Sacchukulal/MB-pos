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

import { PaymentModes } from '../src/billing/Billing';
import { DENSE_ABOVE, TableGrid } from '../src/billing/TableGrid';
import { Totals } from '../src/billing/Totals';
import type { BillView } from '../src/ipc/generated/BillView';
import type { CartView } from '../src/ipc/generated/CartView';
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
    orderId: null,
    selected: false,
    ...over,
  };
}

describe('the table grid (scope 1.4)', () => {
  it('tells all four states apart WITHOUT colour', () => {
    // §2 rule 2: "colour is never the only signal." The class carries the
    // form — dashed vs solid, stripe — and a grey-scale reading of the screen
    // must still distinguish them.
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

  /**
   * **The table you are on is marked, and being on it costs nothing else.**
   *
   * The owner, 2026-08-22: *"In billing page, selected table is not
   * highlighted. user should know which table he selected right?"*
   *
   * There used to be a fifth STATE for this, `loaded`, and the test above
   * asserted it happily — while the screen could not mark an empty table at
   * all, because that state was decided by matching the cart's order and an
   * empty table has no order. A state is one fact; this is two.
   */
  it('marks the selected table WITHOUT taking its state away', () => {
    const { container } = render(
      <TableGrid
        tables={[
          // The case the owner hit: nothing typed on it yet.
          table({ id: '1', label: '1', state: 'free', selected: true }),
          // And the case that would have gone wrong the other way — a late
          // table must not stop looking late because somebody opened it. §4
          // calls the late signal "not optional".
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
    // T3. §4: "a busy shop has 40+ tables. Tiles must stay readable at that
    // count without scrolling becoming the interaction."
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
        onPrintBill={vi.fn()}
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

/**
 * **An empty floor draws nothing at all** — P30.5.
 *
 * It used to answer with a card in the middle of the counter: "No tables set
 * up yet · Tables are added in Settings." A tea stall, a bakery and a parcel
 * counter have no tables and never will, so that was permanent furniture
 * explaining a feature they do not want, on the one screen a cashier looks at
 * all day. The owner's word for it was *"big buttons without proper styling
 * eating so much spaces unnessorily"*, and they had installed it on a real
 * machine to find out.
 *
 * A filter that matches nothing is the OPPOSITE case and keeps its answer:
 * there is something, and the reason it is not on screen is the filter.
 */
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

/**
 * **The payment modes** — the owner's second point, 2026-08-22.
 *
 * > *"the payment mode selection is also not visible, and it also shows some
 * > error notification, what is it? check properly."*
 *
 * The notification was **"That payment could not be taken — a payment has to be
 * more than zero"**. Each mode button takes *the balance Rust computed*; once
 * the bill was covered that balance was zero, and the buttons went on offering
 * themselves. Rust's refusal is right and stays — a zero-rupee payment row is
 * noise in every report downstream. The button that could only ever produce it
 * was the bug.
 *
 * Nothing here reaches Rust. That is the point: the rule is about a number Rust
 * already sent, and it is now assertable in four lines.
 */
describe('the payment modes (2026-08-22)', () => {
  function cart(over: Partial<CartView> = {}): CartView {
    return {
      lines: [],
      bill: emptyBill,
      orderType: 'dine_in',
      table: 'tbl_3',
      payments: [],
      paid: money(0, '0.00'),
      balance: money(10_500, '105.00'),
      change: money(0, '0.00'),
      isEmpty: false,
      kitchenUpToDate: true,
      kitchenTold: false,
      covers: null,
      orderId: 'ord_1',
      fromTheFloor: [],
      lengthSays: '',
      ...over,
    };
  }

  /** One mode button by its label. `getByRole` throws if it is not there. */
  const mode = (label: string) =>
    screen.getByRole('button', { name: new RegExp(`^${label}`) });
  const modes = () => ['Cash', 'Card', 'UPI', 'Credit'].map(mode);

  it('offers every mode while there is something left to pay', () => {
    render(<PaymentModes cart={cart()} onTake={vi.fn()} onCredit={vi.fn()} />);
    for (const button of modes()) expect(button).toBeEnabled();
  });

  /** **This is the error the owner photographed.** */
  it('offers NO mode once the balance is zero', () => {
    render(
      <PaymentModes
        cart={cart({
          balance: money(0, '0.00'),
          paid: money(10_500, '105.00'),
          payments: [{ index: 0, mode: 'Cash', amount: money(10_500, '105.00'), reference: null }],
        })}
        onTake={vi.fn()}
        onCredit={vi.fn()}
      />,
    );
    for (const button of modes()) {
      expect(button, `${button.textContent} could still send a zero payment`).toBeDisabled();
    }
  });

  it('says WHY they are off, rather than going quietly dead', () => {
    render(
      <PaymentModes
        cart={cart({ balance: money(0, '0.00'), paid: money(10_500, '105.00') })}
        onTake={vi.fn()}
        onCredit={vi.fn()}
      />,
    );
    expect(screen.getByText(/paid in full/i)).toBeTruthy();
  });

  it('says nothing about a bill nobody has paid yet', () => {
    // A row of four live buttons needs no explanation, and furniture on the
    // one screen a cashier lives on is what P30.5 spent a session removing.
    render(<PaymentModes cart={cart()} onTake={vi.fn()} onCredit={vi.fn()} />);
    expect(screen.queryByText(/paid in full/i)).toBeNull();
  });

  /**
   * The other half of the owner's sentence. Four identical outlines before the
   * money and four identical outlines after it is why a settled bill looked
   * exactly like an unpaid one.
   */
  it('marks the mode the money was taken in, and only that one', () => {
    render(
      <PaymentModes
        cart={cart({
          balance: money(0, '0.00'),
          paid: money(10_500, '105.00'),
          payments: [{ index: 0, mode: 'Card', amount: money(10_500, '105.00'), reference: null }],
        })}
        onTake={vi.fn()}
        onCredit={vi.fn()}
      />,
    );
    expect(mode('Card').getAttribute('aria-pressed')).toBe('true');
    expect(mode('Card').className).toContain('mb-payment__mode--taken');
    for (const other of ['Cash', 'UPI', 'Credit']) {
      expect(mode(other).getAttribute('aria-pressed')).toBe('false');
      expect(mode(other).className).not.toContain('--taken');
    }
  });

  it('marks BOTH halves of a part-cash part-card bill', () => {
    // Audit B9: v1 allowed one mode per bill and the cashier had to lie about
    // it. Two rows means two marks, or the screen is telling the old lie.
    render(
      <PaymentModes
        cart={cart({
          balance: money(0, '0.00'),
          paid: money(10_500, '105.00'),
          payments: [
            { index: 0, mode: 'Cash', amount: money(5_000, '50.00'), reference: null },
            { index: 1, mode: 'Card', amount: money(5_500, '55.00'), reference: null },
          ],
        })}
        onTake={vi.fn()}
        onCredit={vi.fn()}
      />,
    );
    expect(mode('Cash').className).toContain('--taken');
    expect(mode('Card').className).toContain('--taken');
    expect(mode('UPI').className).not.toContain('--taken');
  });

  it('keeps the mark on a part-paid bill, where the modes are still live', () => {
    // Cash covered half; there is still 55.00 owing, so every mode stays
    // pressable AND the cashier can see what has already gone in.
    render(
      <PaymentModes
        cart={cart({
          balance: money(5_500, '55.00'),
          paid: money(5_000, '50.00'),
          payments: [{ index: 0, mode: 'Cash', amount: money(5_000, '50.00'), reference: null }],
        })}
        onTake={vi.fn()}
        onCredit={vi.fn()}
      />,
    );
    expect(mode('Cash')).toBeEnabled();
    expect(mode('Cash').className).toContain('--taken');
    expect(mode('Card').className).not.toContain('--taken');
    expect(screen.queryByText(/paid in full/i)).toBeNull();
  });

  it('offers nothing at all on an empty bill', () => {
    render(
      <PaymentModes cart={cart({ isEmpty: true, balance: money(0, '0.00') })} onTake={vi.fn()} onCredit={vi.fn()} />,
    );
    for (const button of modes()) expect(button).toBeDisabled();
    // And no "paid in full" either — an empty bill is not a paid one.
    expect(screen.queryByText(/paid in full/i)).toBeNull();
  });

  it('takes the whole balance when a live mode is pressed', () => {
    const onTake = vi.fn();
    const onCredit = vi.fn();
    render(<PaymentModes cart={cart()} onTake={onTake} onCredit={onCredit} />);
    mode('Cash').click();
    expect(onTake).toHaveBeenCalledWith('Cash');
    // Credit is a dialog, not a payment — P15.
    mode('Credit').click();
    expect(onCredit).toHaveBeenCalledTimes(1);
    expect(onTake).toHaveBeenCalledTimes(1);
  });
});

/** A bill with nothing on it — the payment tests never read these figures. */
const emptyBill: BillView = {
  subtotal: money(0, '0.00'),
  lineDiscount: money(0, '0.00'),
  billDiscount: money(0, '0.00'),
  totalDiscount: money(0, '0.00'),
  discountCapped: false,
  charges: [],
  taxRows: [],
  nonGstValue: money(0, '0.00'),
  exemptValue: money(0, '0.00'),
  taxTotal: money(0, '0.00'),
  roundOff: money(0, '0.00'),
  grandTotal: money(10_500, '105.00'),
};
