/** The stock screen. */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  isUiError: () => false,
  // The screen asks this before it toasts.
  isLicenceRefusal: () => false,
}));

const { Stock } = await import('../src/stock/Stock');
const { ToastProvider } = await import('../src/kit');

import type { InventoryView } from '../src/ipc/generated/InventoryView';

function money(paise: number, text: string) {
  return { paise: BigInt(paise), text };
}

const rice = {
  id: 'mat_rice',
  name: 'Rice',
  dimension: 'weight',
  dimensionLabel: 'Weight',
  baseUnit: 'g',
  category: 'Dry goods',
  buyFrom: 'Metro',
  onHand: '1.712 bag',
  onHandBase: '42800',
  isNegative: false,
  value: money(256_800, '2,568.00'),
  cost: '₹1,500.00 per bag',
  costWhen: 'changed 3 Aug, 9:00 am',
  lastCounted: 'never counted',
  isLow: false,
  buy: '',
  reorderLevel: '1 bag',
  reorderQty: '2 bag',
  isPerishable: false,
  shelfLifeDays: null,
  warning: '',
  isActive: true,
  isMade: false,
  units: [
    { name: 'g', basePerUnit: '1', isStandard: true },
    { name: 'kg', basePerUnit: '1000', isStandard: true },
    { name: 'bag', basePerUnit: '25000', isStandard: false },
  ],
  purchaseUnit: 'bag',
  recipeUnit: 'g',
  usedBy: 1,
};

const paneer = {
  ...rice,
  id: 'mat_paneer',
  name: 'Paneer',
  buyFrom: 'The milk van',
  onHand: '0.4 kg',
  onHandBase: '400',
  value: money(12_000, '120.00'),
  cost: '₹300.00 per kg',
  isLow: true,
  buy: '2 kg',
  isPerishable: true,
  shelfLifeDays: 3,
  warning: 'Paneer keeps 3 days and has not moved in 6 days. Check it.',
  units: [
    { name: 'g', basePerUnit: '1', isStandard: true },
    { name: 'kg', basePerUnit: '1000', isStandard: true },
  ],
  purchaseUnit: 'kg',
};

const view: InventoryView = {
  materials: [rice, paneer],
  dishes: [
    {
      itemId: 'itm_dosa',
      name: 'Masala Dosa',
      sellsFor: money(12_000, '120.00'),
      hasRecipe: true,
      recipeCost: money(2_820, '28.20'),
      typedCost: money(2_000, '20.00'),
      // The gap IS the finding, and Rust writes it as a sentence.
      margin: '76.5% margin',
      gap: '₹8.20 more than you thought',
      isIncomplete: false,
    },
    {
      itemId: 'itm_tea',
      name: 'Tea',
      sellsFor: money(2_000, '20.00'),
      hasRecipe: false,
      recipeCost: money(0, '0.00'),
      typedCost: null,
      margin: '',
      gap: '',
      isIncomplete: false,
    },
  ],
  buyList: [
    {
      buyFrom: 'The milk van',
      lines: [
        {
          materialId: 'mat_paneer',
          material: 'Paneer',
          have: '0.4 kg',
          buy: '2 kg',
          line: 'Paneer — have 0.4 kg, buy 2 kg',
        },
      ],
    },
  ],
  problems: [
    {
      id: 'prb_no_recipe_item:itm_tea',
      kind: 'no_recipe',
      sentence:
        'Tea has no recipe, so selling it takes nothing off the shelf. Add a recipe to include it in stock and food cost.',
      times: 43,
      when: '9 Aug, 8:12 pm',
    },
  ],
  movements: [
    {
      id: 'mv_1',
      material: 'Rice',
      kind: 'Bought',
      kindTag: 'purchase',
      qty: '+2 bag',
      takesOut: false,
      value: money(300_000, '3,000.00'),
      when: '3 Aug, 9:00 am',
      who: 'staff_1',
      reason: '',
      note: '',
      wasAutomatic: false,
    },
    {
      id: 'mv_2',
      material: 'Rice',
      kind: 'Sold',
      kindTag: 'sale',
      qty: '−180 g',
      takesOut: true,
      value: money(1_080, '10.80'),
      when: '3 Aug, 1:15 pm',
      who: 'staff_1',
      reason: '',
      note: '',
      wasAutomatic: false,
    },
  ],
  wastageReasons: [{ id: 'rsn_wst_burnt', text: 'Burnt or overcooked' }],
  totalValue: money(268_800, '2,688.00'),
  summary: '2 materials · 1 low · 1 problem',
  cacheWarning: '',
  mayManage: true,
  mayWaste: true,
  mayAdjust: true,
};

function draw(over: Partial<InventoryView> = {}) {
  call.mockImplementation((name: string) => {
    if (name === 'inventory') return Promise.resolve({ ...view, ...over });
    if (name === 'rebuild_stock_balances')
      return Promise.resolve({ ...view, ...over, cacheWarning: '' });
    return Promise.resolve(null);
  });
  return render(
    <ToastProvider>
      <Stock />
    </ToastProvider>,
  );
}

describe('the stock screen', () => {
  beforeEach(() => call.mockReset());
  afterEach(cleanup);

  it('shows every quantity exactly as Rust said it', async () => {
    draw();
    // The screen never converts: 42,800 g reaches it as "1.712 bag" and 400 g as "0.4 kg", each
    // in the unit a person would use.
    expect(await screen.findByText('1.712 bag')).toBeInTheDocument();
    expect(screen.getByText('0.4 kg')).toBeInTheDocument();
    expect(screen.getByText('2,688.00')).toBeInTheDocument();
    expect(screen.getByText('2 materials · 1 low · 1 problem')).toBeInTheDocument();
    // The low one carries what to buy, in the pack the shop buys in.
    expect(screen.getByText('Buy 2 kg')).toBeInTheDocument();
  });

  it('shows D117 as a sentence rather than a batch table', async () => {
    draw();
    expect(
      await screen.findByText('Paneer keeps 3 days and has not moved in 6 days. Check it.'),
    ).toBeInTheDocument();
  });

  it('shows a problem as the whole sentence Rust wrote', async () => {
    draw();
    fireEvent.click(await screen.findByText(/Needs a look/));
    // The row carries its own fix, and this file composes nothing.
    expect(
      screen.getByText(/Tea has no recipe, so selling it takes nothing off the shelf/),
    ).toBeInTheDocument();
    expect(screen.getByText('43 times, last 9 Aug, 8:12 pm')).toBeInTheDocument();
  });

  it('groups the buy list by where you buy it', async () => {
    draw();
    fireEvent.click(await screen.findByText('What to buy'));
    expect(screen.getByText('The milk van')).toBeInTheDocument();
    expect(screen.getByText('have 0.4 kg')).toBeInTheDocument();
    expect(screen.getByText('buy 2 kg')).toBeInTheDocument();
  });

  it('shows a movement in the unit it was typed in, with its direction', async () => {
    draw();
    fireEvent.click(await screen.findByText('Movements'));
    expect(screen.getByText('+2 bag')).toBeInTheDocument();
    // A minus sign, not a hyphen (§6).
    expect(screen.getByText('−180 g')).toBeInTheDocument();
  });

  it('D114 — a cache that disagrees with the ledger carries the button that fixes it', async () => {
    draw({
      cacheWarning:
        '1 material does not match the movement list. Press Rebuild to work them out again from the movements.',
    });
    expect(await screen.findByText(/does not match the movement list/)).toBeInTheDocument();
    const rebuild = screen.getByRole('button', { name: 'Rebuild' });
    fireEvent.click(rebuild);
    expect(call).toHaveBeenCalledWith('rebuild_stock_balances');
  });

  it('sends back what a person typed and which unit they typed it in', async () => {
    draw();
    fireEvent.click((await screen.findAllByText('Move'))[0]!);
    fireEvent.change(screen.getByLabelText('How much'), { target: { value: '2' } });
    fireEvent.click(screen.getByRole('button', { name: 'Record it' }));
    // "2" and "bag", never 50000. Rust converts.
    expect(call).toHaveBeenCalledWith('record_stock_movement', {
      edit: {
        materialId: 'mat_rice',
        kind: 'purchase',
        qty: '2',
        unit: 'bag',
        reasonId: null,
        note: null,
        cost: null,
      },
    });
  });

  it('D119 — a dish shows both cost figures and the gap between them', async () => {
    draw();
    fireEvent.click(await screen.findByText('What each dish costs'));
    expect(screen.getByText('28.20')).toBeInTheDocument();
    // Twice: Tea sells for ₹20 and the dosa was down as costing ₹20.
    expect(screen.getAllByText('20.00')).toHaveLength(2);
    expect(screen.getByText('₹8.20 more than you thought')).toBeInTheDocument();
    expect(screen.getByText('76.5% margin')).toBeInTheDocument();
    // A dish nobody has costed is not a dish that costs nothing, so it offers to add a recipe
    // rather than showing ₹0.00 as a food cost.
    expect(screen.getByRole('button', { name: 'Add a recipe' })).toBeInTheDocument();
  });

  it('tells a shop with nothing in it what a material is for', async () => {
    draw({ materials: [], dishes: [], buyList: [], problems: [], movements: [] });
    // An empty state that only says "No data" wastes the one moment somebody was looking for
    // guidance.
    expect(await screen.findByText('No materials yet')).toBeInTheDocument();
    expect(screen.getByText(/rice, oil, paneer/)).toBeInTheDocument();
  });
});
