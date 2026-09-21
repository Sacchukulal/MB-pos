import { render, renderHook, screen, cleanup, within, act, fireEvent } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { PaymentModes, paymentAnswer } from '../src/billing/Billing';
import { Suggestions } from '../src/billing/Keys';
import { Processing, ProcessingHead, processingOrders } from '../src/billing/Processing';
import { TableGrid } from '../src/billing/TableGrid';
import { useArrivals } from '../src/billing/arrivals';
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
    sectionOrder: 0,
    seats: 4,
    state: 'free',
    total: null,
    minutes: null,
    kitchenTold: true,
    kitchenMinutes: null,
    billAsked: false,
    settleAsked: false,
    by: null,
    byId: null,
    orderId: null,
    seat: null,
    token: null,
    billNumber: null,
    selected: false,
    ...over,
  };
}
/**
 * A new order is the difference between the list the screen was showing and the one it shows
 * now — Rust only says the floor changed. The first read is what the screen opened on.
 */
describe('arrivals', () => {
  it('beats for an order that landed while the screen was open, never for the first read', () => {
    vi.useFakeTimers();
    document.documentElement.style.setProperty('--motion-beat', '1000ms');
    document.documentElement.style.setProperty('--beats', '3');
    const busy = (label: string) => table({ id: `tbl_${label}`, label, state: 'occupied', orderId: `ord_${label}` });

    const { result, rerender } = renderHook(({ floor }) => useArrivals(floor), {
      initialProps: { floor: null as readonly TableView[] | null },
    });
    expect(result.current.size).toBe(0);

    // The floor the screen opened on: nothing on it is new.
    rerender({ floor: [busy('1'), busy('2')] });
    expect(result.current.size).toBe(0);

    // A phone puts an order on table 3.
    rerender({ floor: [busy('1'), busy('2'), busy('3')] });
    expect([...result.current]).toEqual(['ord_3']);

    // A re-read of the same floor fifteen seconds on changes nothing, and does not reset the clock.
    act(() => vi.advanceTimersByTime(2000));
    rerender({ floor: [busy('1'), busy('2'), busy('3')] });
    expect([...result.current]).toEqual(['ord_3']);
    act(() => vi.advanceTimersByTime(1000));
    expect(result.current.size).toBe(0);

    // The grid and the list draw the same fact.
    render(
      <TableGrid tables={[busy('3')]} filter="" onOpen={() => {}} onPrintBill={() => {}} arrived={new Set(['ord_3'])} />,
    );
    expect(document.querySelector('.mb-tile.mb-arrived')).toBeTruthy();
    vi.useRealTimers();
  });
});


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

  it('wears the token beside the money, so a tile can be matched to its kitchen ticket', () => {
    render(
      <TableGrid
        tables={[
          table({
            id: '6',
            label: '6',
            state: 'occupied',
            orderId: 'ord_6',
            total: money(29_500, '295.00'),
            token: '68',
          }),
          table({ id: '7', label: '7', state: 'free' }),
        ]}
        filter=""
        onOpen={vi.fn()}
        onPrintBill={vi.fn()}
      />,
    );
    expect(screen.getByText('#68')).toBeInTheDocument();
    expect(screen.getByText('#68').closest('.mb-tile')?.textContent).toContain('295.00');
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

    expect(container.querySelectorAll('.mb-chosen')).toHaveLength(2);
    expect(container.querySelector('.mb-tile--free.mb-chosen')).toBeTruthy();
    expect(
      container.querySelector('.mb-tile--late.mb-chosen'),
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

  it('draws sixty tables at the same size as four', () => {
    const many = (count: number) =>
      Array.from({ length: count }, (_, n) =>
        table({ id: `t${n}`, label: `${n + 1}`, total: money(10_000, '100.00'), state: 'occupied' }),
      );
    const big = render(
      <TableGrid tables={many(60)} filter="" onOpen={vi.fn()} onPrintBill={vi.fn()} />,
    );
    // Every tile still carries its amount: no smaller step for a big floor.
    expect(big.container.querySelectorAll('.mb-tile__amount')).toHaveLength(60);
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

  /**
   * The counter groups its rooms in the SHOP'S order, the one the Floor screen shows — not in
   * the alphabet's. Sorting by name made the two screens disagree the moment a room was named
   * something that sorts the other way.
   */
  it('puts the rooms in the order the shop put them in, not the alphabet', () => {
    const { container } = render(
      <TableGrid
        tables={[
          table({ id: 'a', label: '9', section: 'AC', sectionOrder: 1 }),
          table({ id: 'b', label: '1', section: 'Hall', sectionOrder: 0 }),
          table({
            id: 'ord_9',
            label: 'Parcel',
            section: null,
            sectionOrder: null,
            state: 'occupied',
            orderId: 'ord_9',
          }),
        ]}
        filter=""
        onOpen={vi.fn()}
        onPrintBill={vi.fn()}
      />,
    );
    // Hall was made first, so Hall comes first — and the group with no room is last.
    expect(
      [...container.querySelectorAll('.mb-floor__heading')].map((n) => n.textContent),
    ).toEqual(['Hall', 'AC', 'No table']);
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

/** The money row: Cash and its box, the answer, and the arrow that keeps the other ways. */
describe('the payment modes (2026-08-23, one row 2026-09-03)', () => {
  /** The row is a CHOICE, not an action. */
  const mode = (label: string) =>
    screen.getByRole('button', { name: new RegExp(`^${label}`) });
  const arrow = () => screen.getByRole('button', { name: /Other ways to pay/ });
  const money = (paise: bigint) => ({ paise, text: (Number(paise) / 100).toFixed(2) });
  /** A cart with only what the row reads. */
  const cartWith = (over: {
    balance?: bigint;
    change?: bigint;
    payments?: number;
    isEmpty?: boolean;
  }) =>
    ({
      isEmpty: over.isEmpty ?? false,
      balance: money(over.balance ?? 0n),
      change: money(over.change ?? 0n),
      payments: Array.from({ length: over.payments ?? 0 }, (_, index) => ({
        index,
        mode: 'Cash',
        amount: money(0n),
        reference: null,
      })),
    }) as unknown as CartView;
  const row = (over: Partial<Parameters<typeof PaymentModes>[0]> = {}) => (
    <PaymentModes
      mode="Cash"
      onPick={vi.fn()}
      onCredit={vi.fn()}
      cash=""
      onCash={vi.fn()}
      onCashDone={vi.fn()}
      onEnter={vi.fn()}
      cart={null}
      {...over}
    />
  );

  it('lights Cash on its own, with the box beside it and the arrow at the end', () => {
    render(row());
    expect(mode('Cash').getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('textbox', { name: 'Cash given' })).toBeTruthy();
    expect(arrow().getAttribute('aria-pressed')).toBe('false');
    // Card and UPI are behind the arrow, not on the row.
    expect(screen.queryByRole('button', { name: /^Card/ })).toBeNull();
    expect(screen.queryByRole('button', { name: /^UPI/ })).toBeNull();
  });

  it('moves the light rather than taking money', () => {
    const onPick = vi.fn();
    render(row({ onPick }));
    act(() => arrow().click());
    act(() => mode('Card').click());
    expect(onPick).toHaveBeenCalledWith('Card');
  });

  it('says on the arrow which other way is in force, and the box stays for a split', () => {
    render(row({ mode: 'UPI' }));
    expect(mode('Cash').getAttribute('aria-pressed')).toBe('false');
    const button = screen.getByRole('button', { name: /Paid by UPI/ });
    expect(button.getAttribute('aria-pressed')).toBe('true');
    expect(button.textContent).toContain('UPI');
    expect(screen.getByRole('textbox', { name: 'Cash given' })).toBeTruthy();
  });

  it('never goes dead, because choosing a mode cannot fail', () => {
    render(row({ mode: 'UPI' }));
    expect(mode('Cash')).toBeEnabled();
    act(() => arrow().click());
    for (const label of ['Card', 'UPI', 'Credit']) expect(mode(label)).toBeEnabled();
  });

  it('says nothing at all with nothing typed — no essay, no clear button', () => {
    render(row({ cart: cartWith({ balance: 2000n }) }));
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.queryByRole('button', { name: /clear payments/i })).toBeNull();
  });

  it('keeps credit behind the arrow, and it opens the picker', () => {
    const onCredit = vi.fn();
    render(row({ onCredit }));
    expect(screen.queryByRole('button', { name: /^Credit/ })).toBeNull();
    act(() => arrow().click());
    act(() => mode('Credit').click());
    expect(onCredit).toHaveBeenCalledTimes(1);
  });

  it('completes the bill on Enter in the cash box, and only then', () => {
    const onEnter = vi.fn();
    const onCashDone = vi.fn();
    render(row({ onEnter, onCashDone }));
    const box = screen.getByRole('textbox', { name: 'Cash given' });
    fireEvent.keyDown(box, { key: '5' });
    expect(onEnter).not.toHaveBeenCalled();
    fireEvent.keyDown(box, { key: 'Enter' });
    expect(onEnter).toHaveBeenCalledTimes(1);
  });

  it('hands the cash box to whoever presses Cash', () => {
    render(row({ mode: 'Card' }));
    act(() => mode('Cash').click());
    expect(document.activeElement).toBe(screen.getByRole('textbox', { name: 'Cash given' }));
  });
});

/** What the row says beside the box. Rust did the sums; this only picks the sentence. */
describe('the answer beside the cash box (2026-09-03)', () => {
  const money = (paise: bigint) => ({ paise, text: (Number(paise) / 100).toFixed(2) });
  const cartWith = (over: { balance?: bigint; change?: bigint; payments?: number }) =>
    ({
      isEmpty: false,
      balance: money(over.balance ?? 0n),
      change: money(over.change ?? 0n),
      payments: Array.from({ length: over.payments ?? 0 }, () => ({})),
    }) as unknown as CartView;

  it('is silent on an empty cart', () => {
    expect(paymentAnswer(null, 'Cash')).toBeNull();
    expect(paymentAnswer({ isEmpty: true } as CartView, 'Cash')).toBeNull();
  });

  it('gives change back in green, whichever mode is lit', () => {
    const cart = cartWith({ change: 1000n, payments: 1 });
    const back = { tone: 'back', text: '10.00', means: 'Return' };
    expect(paymentAnswer(cart, 'Cash')).toEqual(back);
    expect(paymentAnswer(cart, 'Card')).toEqual(back);
  });

  it('asks for the rest in red once some cash is down — the figure alone (2026-09-20)', () => {
    expect(paymentAnswer(cartWith({ balance: 1000n, payments: 1 }), 'Cash')).toEqual({
      tone: 'short',
      text: '10.00',
      means: 'Still to pay',
    });
    // Nothing typed yet: nothing to say.
    expect(paymentAnswer(cartWith({ balance: 2000n }), 'Cash')).toBeNull();
  });

  it('sends the rest by card or UPI — the whole bill, or what the cash left', () => {
    expect(paymentAnswer(cartWith({ balance: 2000n }), 'UPI')).toEqual({
      tone: 'by',
      text: '20.00',
      means: 'By UPI',
    });
    expect(paymentAnswer(cartWith({ balance: 500n, payments: 1 }), 'Card')).toEqual({
      tone: 'by',
      text: '5.00',
      means: 'By Card',
    });
  });

  it('shows nothing to return when the cash was exact', () => {
    expect(paymentAnswer(cartWith({ payments: 1 }), 'Cash')).toEqual({
      tone: 'back',
      text: '0.00',
      means: 'Paid exactly',
    });
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
    // A parcel names itself, and a late one carries a dot as well as the colour.
    const parcel = screen.getByRole('button', { name: /^Parcel/ });
    expect(parcel.querySelector('.mb-dot--late')).toBeTruthy();
    expect(parcel.textContent).toContain('1h 10m');
  });

  it('says so when nothing is cooking', () => {
    render(<Processing orders={[]} onOpen={vi.fn()} />);
    expect(screen.getByText('Nothing cooking')).toBeInTheDocument();
  });

  // 2026-09-13: the token is the number on the kitchen ticket and the bill, and the only one
  // an open order has — the bill number is born when the bill is paid.
  it('shows the token, the number the kitchen ticket carries', () => {
    render(
      <Processing
        orders={[cooking({ id: '3', label: '3', token: '68', billNumber: null })]}
        onOpen={vi.fn()}
      />,
    );
    expect(screen.getByRole('button', { name: /Table 3/ }).textContent).toContain('#68');
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

/**
 * The list is taller than the box it pops in, so the arrows have to carry the box with them.
 * They did not, and the last rows were highlighted where nobody could see them.
 */
describe('the suggestion list under the arrow keys', () => {
  const dish = (id: string, name: string) => ({
    id,
    name,
    price: money(12_000, '120.00'),
    rateLabel: '5%',
    category: null,
  });

  it('scrolls the chosen row into sight', () => {
    const seen: number[] = [];
    const items = Array.from({ length: 10 }, (_, n) => dish(`i${n}`, `Item ${n}`));
    const { container, rerender } = render(
      <Suggestions items={items} highlighted={0} onPick={vi.fn()} />,
    );
    // jsdom does not scroll, so each row is asked whether it was told to.
    for (const [at, row] of [...container.querySelectorAll('li')].entries()) {
      row.scrollIntoView = () => seen.push(at);
    }
    rerender(<Suggestions items={items} highlighted={9} onPick={vi.fn()} />);
    expect(seen, 'the last row was highlighted out of sight').toEqual([9]);
  });

  it('marks the chosen row for a screen reader too', () => {
    const items = [dish('a', 'A'), dish('b', 'B')];
    render(<Suggestions items={items} highlighted={1} onPick={vi.fn()} />);
    const rows = screen.getAllByRole('option');
    expect(rows[0]?.getAttribute('aria-selected')).toBe('false');
    expect(rows[1]?.getAttribute('aria-selected')).toBe('true');
  });
});
