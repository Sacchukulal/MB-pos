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

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  isUiError: () => false,
}));

const { Combos, Composition, ModifierGroups } = await import('../src/menu/Composition');
const { Menu } = await import('../src/menu/Menu');
const { ToastProvider } = await import('../src/kit');

import type { ComboView } from '../src/ipc/generated/ComboView';
import type { ItemComposition } from '../src/ipc/generated/ItemComposition';
import type { MenuRowView } from '../src/ipc/generated/MenuRowView';
import type { MoneyView } from '../src/ipc/generated/MoneyView';
import type { TaxClassView } from '../src/ipc/generated/TaxClassView';

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
  // P24 — a dish with no course and no target, which is what a shop that has
  // not set up its kitchen screen has, and must keep working with.
  course: null,
  prepMinutes: null,
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

/**
 * **The tax class editor** — P33 §5.1, the round trip that used to go through
 * prose. The screen read its own display words back (`.includes('Outside')`) to
 * decide what to save, so rewording a label made liquor GST-taxable.
 */
describe('the tax class editor', () => {
  const liquor: TaxClassView = {
    id: 'tax_liquor',
    name: 'Liquor — state VAT',
    rate: '20%',
    rateBp: 2000,
    kind: 'outside_gst',
    basis: 'inclusive',
    treatment: 'Outside GST',
    isActive: true,
    itemsUsing: 4n,
  };

  function wire() {
    call.mockImplementation((name: string) => {
      switch (name) {
        case 'menu_tax_classes':
          return Promise.resolve([liquor]);
        case 'save_tax_class':
          return Promise.resolve('Saved.');
        default:
          return Promise.resolve([]);
      }
    });
  }

  const open = async () => {
    wire();
    render(
      <ToastProvider>
        <Menu />
      </ToastProvider>,
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
  };

  it('sends the machine values back, never the words on the tile', async () => {
    await open();

    // Outside GST asks for a VAT rate by name, because it is not GST.
    expect(screen.getByLabelText('State VAT %')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Bar list' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(call).toHaveBeenCalledWith('save_tax_class', {
      id: 'tax_liquor',
      name: 'Bar list',
      rate: '20',
      kind: 'outside_gst',
      basis: 'inclusive',
    });
  });

  it('shuts the rate box on a kind that cannot carry one', async () => {
    await open();

    fireEvent.change(screen.getByLabelText('Kind'), { target: { value: 'exempt' } });
    const box = screen.getByLabelText('Rate') as HTMLInputElement;
    expect(box.disabled, 'exempt has no rate to type').toBe(true);
    expect(box.value).toBe('0');

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(call).toHaveBeenCalledWith(
      'save_tax_class',
      expect.objectContaining({ kind: 'exempt', rate: '0' }),
    );
  });
});

/**
 * **Typing a menu is a run, not a dialog reopened per dish** — the owner,
 * 2026-08-24: *"If the user has to add many items, he has to click add buton
 * and it pops up many times, it is tedious."*
 */
describe('adding items (2026-08-24)', () => {
  const classes: TaxClassView[] = [
    {
      id: 'tax_food_5',
      name: 'Food 5%',
      rate: '5%',
      rateBp: 500,
      kind: 'gst',
      basis: 'exclusive',
      treatment: 'Tax added on top',
      isActive: true,
      itemsUsing: 1n,
    },
  ];

  function open() {
    call.mockImplementation((name: string) => {
      switch (name) {
        case 'menu_tax_classes':
          return Promise.resolve(classes);
        case 'menu_rows':
          return Promise.resolve([]);
        case 'save_menu_item':
          return Promise.resolve([]);
        default:
          return Promise.resolve([]);
      }
    });
    return render(
      <ToastProvider>
        <Menu />
      </ToastProvider>,
    );
  }

  it('keeps the panel open and empties it after each item', async () => {
    open();
    fireEvent.click(await screen.findByRole('button', { name: 'Add an item' }));
    expect(screen.getByRole('complementary', { name: 'Add an item' })).toBeTruthy();

    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Idli' } });
    fireEvent.change(screen.getByLabelText('Price'), { target: { value: '40' } });
    // Enter saves, which is how a menu is typed: name, price, Enter.
    fireEvent.keyDown(screen.getByLabelText('Name'), { key: 'Enter' });

    await waitFor(() =>
      expect(
        call.mock.calls.filter(([name]) => name === 'save_menu_item'),
      ).toHaveLength(1),
    );
    const first = call.mock.calls.find(([name]) => name === 'save_menu_item')!;
    expect((first[1] as { edit: { name: string } }).edit.name).toBe('Idli');

    // Still open, and empty, ready for the next one.
    expect(screen.getByRole('complementary', { name: 'Add an item' })).toBeTruthy();
    await waitFor(() =>
      expect((screen.getByLabelText('Name') as HTMLInputElement).value).toBe(''),
    );
    expect((screen.getByLabelText('Price') as HTMLInputElement).value).toBe('');

    // And the second item is a NEW item, not an edit of the first.
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Vada' } });
    fireEvent.keyDown(screen.getByLabelText('Name'), { key: 'Enter' });
    await waitFor(() =>
      expect(
        call.mock.calls.filter(([name]) => name === 'save_menu_item'),
      ).toHaveLength(2),
    );
    const saves = call.mock.calls.filter(([name]) => name === 'save_menu_item');
    const ids = saves.map(([, args]) => (args as { edit: { id: string } }).edit.id);
    expect(ids[0], 'the second item overwrote the first').not.toBe(ids[1]);
  });

  it('closes when an existing item is saved, because there is nothing to type next', async () => {
    call.mockImplementation((name: string) => {
      switch (name) {
        case 'menu_tax_classes':
          return Promise.resolve(classes);
        case 'menu_rows':
          return Promise.resolve([dosa]);
        default:
          return Promise.resolve([]);
      }
    });
    render(
      <ToastProvider>
        <Menu />
      </ToastProvider>,
    );

    fireEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0]!);
    // Editing names the panel after what is in it; adding names it "Add an item".
    const panel = screen.getByRole('complementary', { name: dosa.name });
    // Scoped: the tax class block below the list has a Save of its own.
    fireEvent.click(within(panel).getByRole('button', { name: 'Save' }));

    await waitFor(() =>
      expect(screen.queryByRole('complementary', { name: dosa.name })).toBeNull(),
    );
  });
});
