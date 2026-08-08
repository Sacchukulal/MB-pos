/**
 * **The composition screens — scope 6.1, 6.2, 6.3.**
 *
 * Rust proves the rules (`menu_tests.rs` drives the commands end to end); this
 * proves what the screens do with them. Three things worth a test:
 *
 * 1. a size shows **its own price**, not a difference from the parent;
 * 2. "how many may they pick" is one choice on the screen and two numbers on
 *    the wire — and the mapping between them is where a group could silently
 *    become "at least 3 of 1";
 * 3. a combo shows every part's **share and rate**, because that is the whole
 *    reason a mixed-rate combo can be sold at all.
 */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  isUiError: () => false,
}));

const { Combos, Composition, ModifierGroups } = await import('../src/menu/Composition');

import type { ComboView } from '../src/ipc/generated/ComboView';
import type { ItemComposition } from '../src/ipc/generated/ItemComposition';
import type { MenuRowView } from '../src/ipc/generated/MenuRowView';
import type { MoneyView } from '../src/ipc/generated/MoneyView';

function money(paise: number, text: string): MoneyView {
  return { paise: BigInt(paise), text };
}

const dosa: MenuRowView = {
  id: 'itm_dosa',
  name: 'Masala dosa',
  categoryId: null,
  price: money(12_000, '120.00'),
  taxClassId: 'tax_food_5',
  rate: '5% · Tax added on top',
  hsn: null,
  shortCode: null,
  cost: null,
  margin: null,
  isOpenPrice: false,
  isAvailable: true,
  variants: 1n,
};

const made: ItemComposition = {
  itemId: 'itm_dosa',
  itemName: 'Masala dosa',
  variants: [
    { id: 'var_half', name: 'Half', price: money(7_000, '70.00'), isActive: true },
  ],
  groups: [
    {
      id: 'grp_spice',
      name: 'Spice level',
      minSelect: 1,
      maxSelect: 1,
      rule: 'Choose one',
      attached: false,
      modifiers: [
        { id: 'mod_mild', name: 'Mild', priceDelta: money(0, '0.00'), isActive: true },
        { id: 'mod_hot', name: 'Extra spicy', priceDelta: money(1_000, '10.00'), isActive: true },
      ],
    },
  ],
};

beforeEach(() => {
  call.mockReset();
});
afterEach(cleanup);

describe('a size (scope 6.1)', () => {
  it('shows its OWN price, not a difference from the full plate', async () => {
    call.mockResolvedValue(made);
    render(<Composition row={dosa} onClose={vi.fn()} onFailed={vi.fn()} />);

    expect(await screen.findByText('Half')).toBeTruthy();
    // 70.00, and nowhere a "-50.00" that would make it a discount.
    expect(screen.getByText('70.00')).toBeTruthy();
    expect(screen.queryByText('-50.00')).toBeNull();
  });
});

describe('a group of choices (scope 6.2)', () => {
  it('offers every group the shop has, ticked only where the item offers it', async () => {
    call.mockResolvedValue(made);
    render(<Composition row={dosa} onClose={vi.fn()} onFailed={vi.fn()} />);

    const tick = (await screen.findByLabelText('Spice level')) as HTMLInputElement;
    expect(tick.checked, 'this item does not offer it yet').toBe(false);
    // The rule is words from Rust, not a pair of numbers on the screen.
    expect(screen.getByText('Choose one')).toBeTruthy();
    expect(screen.getByText('Mild, Extra spicy')).toBeTruthy();

    fireEvent.click(tick);
    expect(call).toHaveBeenCalledWith('attach_modifier_group', {
      itemId: 'itm_dosa',
      groupId: 'grp_spice',
      attach: true,
    });
  });

  it('turns "any number" into no upper limit rather than a large one', async () => {
    call.mockResolvedValue([]);
    render(<ModifierGroups onFailed={vi.fn()} />);

    fireEvent.click(await screen.findByText('Add a group'));
    fireEvent.change(screen.getByLabelText('Name'), {
      target: { value: 'Add-ons' },
    });
    fireEvent.change(screen.getByLabelText('How many may they pick'), {
      target: { value: 'any' },
    });
    fireEvent.click(screen.getByText('Save'));

    const sent = call.mock.calls.find((c) => c[0] === 'save_modifier_group');
    expect(sent, 'the group was saved').toBeTruthy();
    // Plain numbers, deliberately: `JSON.stringify` throws on a BigInt, so a
    // count that crosses the wire is a `u32` in Rust and a `number` here.
    const group = (sent?.[1] as { group: { minSelect: number; maxSelect: number | null } }).group;
    expect(group.minSelect).toBe(0);
    expect(group.maxSelect).toBeNull();
  });

  it('cannot express "at least three of at most one" — the shape is one choice', async () => {
    call.mockResolvedValue([]);
    render(<ModifierGroups onFailed={vi.fn()} />);

    fireEvent.click(await screen.findByText('Add a group'));
    const shape = screen.getByLabelText('How many may they pick') as HTMLSelectElement;
    const offered = [...shape.options].map((o) => o.value);
    // Four shapes, every one of them satisfiable. Two number boxes would have
    // let an owner type a group no cashier can ever get past.
    expect(offered).toEqual(['one', 'atMostOne', 'any', 'atLeastOne']);
  });
});

describe('a combo (scope 6.3)', () => {
  const lunch: ComboView = {
    id: 'cmb_lunch',
    name: 'Lunch deal',
    price: money(13_000, '130.00'),
    isActive: true,
    separately: money(14_000, '140.00'),
    parts: [
      {
        itemId: 'itm_dosa',
        itemName: 'Masala dosa',
        qty: '1',
        share: money(11_143, '111.43'),
        rate: '5%',
      },
      {
        itemId: 'itm_water',
        itemName: 'Water bottle',
        qty: '1',
        share: money(1_857, '18.57'),
        rate: '18%',
      },
    ],
  };

  it('shows each part with its share AND its rate', async () => {
    call.mockResolvedValue([lunch]);
    render(<Combos rows={[dosa]} onFailed={vi.fn()} />);

    expect(await screen.findByText('Lunch deal')).toBeTruthy();
    const parts = screen.getByText(/Masala dosa/);
    expect(parts.textContent).toContain('111.43');
    expect(parts.textContent).toContain('5%');
    expect(parts.textContent).toContain('18.57');
    expect(parts.textContent).toContain('18%');
    // What the deal gives away, worked out in Rust.
    expect(screen.getByText('140.00')).toBeTruthy();
    expect(screen.getByText('130.00')).toBeTruthy();
  });

  it('sends the quantity as TEXT, so Rust decides what "0.5" means', async () => {
    call.mockResolvedValue([]);
    render(<Combos rows={[dosa]} onFailed={vi.fn()} />);

    fireEvent.click(await screen.findByText('Add a combo'));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Thali' } });
    fireEvent.change(screen.getByLabelText('Combo price'), { target: { value: '199' } });
    fireEvent.click(screen.getByText('Add something'));
    fireEvent.change(screen.getByLabelText('Item'), { target: { value: 'itm_dosa' } });
    fireEvent.change(screen.getByLabelText('How many'), { target: { value: '0.5' } });
    fireEvent.click(screen.getByText('Save'));

    const sent = call.mock.calls.find((c) => c[0] === 'save_combo');
    expect(sent).toBeTruthy();
    const combo = (sent?.[1] as { combo: { price: string; parts: [string, string][] } }).combo;
    expect(combo.price).toBe('199');
    expect(combo.parts).toEqual([['itm_dosa', '0.5']]);
  });
});
