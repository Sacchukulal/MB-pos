/**
 * **Buying and the stock count** — P26.
 *
 * Rust proves the arithmetic (`crates/mb-db/tests/buying.rs`). This proves the
 * screens' claims, and two of them are decisions a future session could undo
 * without any test noticing:
 *
 * 1. **the landed cost is shown as Rust said it** — "₹909.09 per bag" beside a
 *    rate of ₹1,000 — because that gap IS the feature (D123), and a screen that
 *    divided a value by a quantity would be a second answer to it;
 * 2. **the count shows the book AS IT WAS WHEN COUNTED** (D127), never today's
 *    figure, which is the whole reason approving posts a delta;
 * 3. **approving says what it will do before anybody presses it**;
 * 4. a shop that has never counted its store is told so, in Rust's words.
 */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  isUiError: () => false,
}));

const { Buying } = await import('../src/buying/Buying');
const { Count } = await import('../src/buying/Count');
const { ToastProvider } = await import('../src/kit');

import type { BuyingView } from '../src/ipc/generated/BuyingView';
import type { StockCountView } from '../src/ipc/generated/StockCountView';

function money(paise: number, text: string) {
  return { paise: BigInt(paise), text };
}

const metro = {
  id: 'sup_metro',
  name: 'Metro',
  phone: '9880012345',
  gstin: null,
  address: null,
  termsDays: 15,
  terms: '15 days',
  balance: money(200_000, '2,000.00'),
  owes: true,
  when: '5 days overdue',
  isOverdue: true,
  isActive: true,
};

const delivery = {
  id: 'pur_1',
  supplierId: 'sup_metro',
  supplier: 'Metro',
  kind: 'Delivery',
  isReturn: false,
  parentId: null,
  invoiceNo: 'INV-9',
  date: '2026-08-12',
  due: '2026-08-27',
  lines: [
    {
      seq: 1,
      materialId: 'mat_rice',
      material: 'Rice',
      qty: '10 bag',
      free: '1 bag free',
      rate: money(100_000, '1,000.00'),
      discount: money(0, '0.00'),
      tax: '',
      taxAmount: money(0, '0.00'),
      value: money(1_000_000, '10,000.00'),
      // **D123's number.** Eleven bags arrived and ₹10,000 was paid.
      landed: '₹909.09 per bag',
      returnable: '',
    },
  ],
  goods: money(1_000_000, '10,000.00'),
  discount: money(0, '0.00'),
  charges: money(0, '0.00'),
  tax: money(0, '0.00'),
  total: money(1_000_000, '10,000.00'),
  creditable: money(0, '0.00'),
  paid: money(0, '0.00'),
  outstanding: money(1_000_000, '10,000.00'),
  cancelled: '',
  hasPhoto: false,
  note: null,
};

const buyingView: BuyingView = {
  suppliers: [metro],
  purchases: [delivery],
  orders: [],
  materials: [
    {
      id: 'mat_rice',
      name: 'Rice',
      baseUnit: 'g',
      packs: [{ name: 'bag', size: '25 kg' }],
      purchaseUnit: 'bag',
      lastRate: '₹1,000.00 last on 2026-08-12',
      lastRatePaise: BigInt(100_000),
      cost: '₹1,000.00 per bag',
    },
  ],
  owed: money(200_000, '2,000.00'),
  overdue: money(200_000, '2,000.00'),
  bought: money(1_000_000, '10,000.00'),
  claimsInputTax: false,
  taxNote:
    'You bill under the 5% scheme, so purchase GST is a cost and not a credit. It is already inside your food cost.',
  attention: ['1 supplier is overdue, 2,000.00 in all.'],
  mayManageSuppliers: true,
  mayEnterPurchases: true,
} as BuyingView;

const countView: StockCountView = {
  id: 'cnt_1',
  location: 'Store',
  state: 'Being counted',
  stateTag: 'draft',
  date: '2026-08-12',
  openedBy: 'staff_1',
  lines: [
    {
      materialId: 'mat_paneer',
      material: 'Paneer',
      counted: '10 kg',
      // The book AS IT WAS at 11 pm on Sunday — D127.
      book: '12 kg',
      variance: '2 kg short',
      varianceValue: money(-80_000, '−800.00'),
      isShort: true,
      isOver: false,
      needsReason: true,
      reasonId: null,
      note: null,
    },
  ],
  remaining: [
    {
      materialId: 'mat_rice',
      material: 'Rice',
      baseUnit: 'g',
      units: ['kg', 'bag'],
      defaultUnit: 'bag',
    },
  ],
  effect: 'This will add to no materials and take away from 1 material.',
  shortValue: money(-80_000, '−800.00'),
  overValue: money(0, '0.00'),
  netValue: money(-80_000, '−800.00'),
  // One sentence, composed in Rust — "Short −800.00" said it twice.
  totalsSays: '800.00 short.',
  reasons: [{ id: 'rsn_cnt_wastage', text: 'Wastage nobody recorded' }],
  history: [],
  mayApprove: true,
  reasonAbove: money(50_000, '500.00'),
  note: 'Nobody has counted this store yet, so every stock figure is what the software worked out from your recipes and not what is on the shelf.',
} as StockCountView;

function draw(node: React.ReactNode) {
  return render(<ToastProvider>{node}</ToastProvider>);
}

afterEach(cleanup);

describe('the buying screen (P26)', () => {
  beforeEach(() => {
    call.mockReset();
    call.mockImplementation((name: string) => {
      if (name === 'buying') return Promise.resolve(buyingView);
      if (name === 'purchase') return Promise.resolve(delivery);
      return Promise.resolve(buyingView);
    });
  });

  it('shows what is owed and how late it is, in Rust words', async () => {
    draw(<Buying />);
    // Owed and overdue are the same figure here, so both are expected.
    expect((await screen.findAllByText('2,000.00')).length).toBeGreaterThan(1);
    // D100 — the row carries its own fix, and this file composed none of it.
    expect(screen.getByText('1 supplier is overdue, 2,000.00 in all.')).toBeTruthy();

    // The supplier list is one tab away: the screen opens on deliveries,
    // because that is what somebody is standing there to enter.
    fireEvent.click(screen.getByText('Suppliers'));
    expect(await screen.findByText('5 days overdue')).toBeTruthy();
    expect(screen.getByText('15 days')).toBeTruthy();
  });

  it('tells a 5%-scheme shop the purchase GST is a cost, not an empty column', async () => {
    draw(<Buying />);
    expect(
      await screen.findByText(/purchase GST is a cost and not a credit/),
    ).toBeTruthy();
  });

  it('shows the landed cost beside the rate, which is the whole feature', async () => {
    draw(<Buying />);
    fireEvent.click(await screen.findByText('Open'));
    // **D123.** Ten bags at ₹1,000 with one free: a bag cost ₹909.09, and the
    // screen shows the figure Rust computed rather than the rate on the paper.
    expect(await screen.findByText('₹909.09 per bag')).toBeTruthy();
    expect(screen.getByText('10 bag + 1 bag free')).toBeTruthy();
  });
});

describe('the stock count (P26)', () => {
  beforeEach(() => {
    call.mockReset();
    call.mockImplementation(() => Promise.resolve(countView));
  });

  it('shows the book AS IT WAS when the shelf was counted', async () => {
    draw(<Count />);
    // **D127.** If this ever shows today's balance instead, approving has
    // become a "set" and Monday's delivery is being erased.
    expect(await screen.findByText('12 kg')).toBeTruthy();
    expect(screen.getByText('Book said')).toBeTruthy();
    expect(screen.getByText('2 kg short')).toBeTruthy();
    // A variance in kilos is one nobody reads; the rupees are the finding.
    expect(screen.getByText('−800.00')).toBeTruthy();
  });

  it('says what approving will do before anybody presses it', async () => {
    draw(<Count />);
    expect(
      await screen.findByText('This will add to no materials and take away from 1 material.'),
    ).toBeTruthy();
  });

  it('tells a shop that has never counted its store that its figures are worked out', async () => {
    draw(<Count />);
    expect(await screen.findByText(/Nobody has counted this store yet/)).toBeTruthy();
  });

  it('offers nothing on an approved count, because Rust would refuse it', async () => {
    // **D129, and it is three lying buttons otherwise.** Found by approving a
    // count in the running app and seeing Remove, "Say why" and "Give up on
    // this count" still on the sheet — every one of which the command layer
    // refuses. P21 spent a session removing exactly this kind of button.
    call.mockImplementation(() =>
      Promise.resolve({
        ...countView,
        state: 'Approved',
        stateTag: 'approved',
        mayApprove: false,
      }),
    );
    draw(<Count />);
    expect(await screen.findByText(/cannot be changed/)).toBeTruthy();
    expect(screen.queryByText('Remove')).toBeNull();
    expect(screen.queryByText('Say why')).toBeNull();
    expect(screen.queryByText('Give up on this count')).toBeNull();
    expect(screen.queryByText('Write it down')).toBeNull();
    // And the one thing that IS offered is a new count.
    expect(screen.getByText('Start a new count')).toBeTruthy();
  });

  it('asks why a big difference happened, rather than silently accepting it', async () => {
    draw(<Count />);
    fireEvent.click(await screen.findByText('Say why'));
    expect(await screen.findByText('Paneer is 2 kg short')).toBeTruthy();
    expect(screen.getByText('Wastage nobody recorded')).toBeTruthy();
  });
});
