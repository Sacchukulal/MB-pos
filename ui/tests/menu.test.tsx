/** The composition screens. */

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
import type { CategoryView } from '../src/ipc/generated/CategoryView';

function money(paise: number, text: string): MoneyView {
  return { paise: BigInt(paise), text };
}

const dosa: MenuRowView = {
  id: 'itm_dosa',
  name: 'Masala dosa',
  categoryId: null,
  price: money(12_000, '120.00'),
  taxClassId: 'tax_food_5',
  priceBasis: 'shop',
  rate: '5% · added on top',
  hsn: null,
  shortCode: null,
  cost: null,
  margin: null,
  isOpenPrice: false,
  isAvailable: true,
  // A dish with no course and no target, which is what a shop that has not set up its kitchen
  // screen has, and must keep working with.
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
    // Plain numbers, deliberately: `JSON.stringify` throws on a BigInt, so a count that crosses
    // the wire is a `u32` in Rust and a `number` here.
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
    // Four shapes, every one of them satisfiable.
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

/** The menu screen: categories down the left, items on the right, both typed in a run. */
describe('the menu screen', () => {
  const tiffin: CategoryView = {
    id: 'cat_tiffin',
    name: 'Tiffin',
    sortOrder: 0n,
    isActive: true,
    itemCount: 1n,
    defaultSlabId: null,
  };

  function open(rows: MenuRowView[] = [dosa]) {
    call.mockImplementation((name: string) => {
      switch (name) {
        case 'menu_categories':
        case 'save_menu_category':
        case 'delete_menu_category':
          return Promise.resolve([tiffin]);
        case 'menu_rows':
        case 'save_menu_item':
        case 'edit_menu_item_field':
        case 'delete_menu_item':
          return Promise.resolve(rows);
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

  it('adds items with short codes and keeps the form ready for the next item', async () => {
    open([]);
    const name = await screen.findByLabelText('Item name');
    expect(screen.getByLabelText('New category')).toBeVisible();
    expect(screen.queryByRole('dialog')).toBeNull();
    fireEvent.change(name, { target: { value: 'Idli' } });
    fireEvent.change(screen.getByLabelText('Price'), { target: { value: '40' } });
    fireEvent.change(screen.getByLabelText('Short code'), { target: { value: ' ID1 ' } });
    fireEvent.submit(name.closest('form')!);

    await waitFor(() =>
      expect(call.mock.calls.filter(([n]) => n === 'save_menu_item')).toHaveLength(1),
    );
    const sent = (call.mock.calls.find(([n]) => n === 'save_menu_item')![1] as {
      edit: { name: string; price: string; shortCode: string | null; taxClassId: string | null; categoryId: string | null };
    }).edit;
    expect(sent.name).toBe('Idli');
    expect(sent.price).toBe('40');
    expect(sent.shortCode).toBe('ID1');
    // Rust decides the tax from the category and the shop.
    expect(sent.taxClassId).toBeNull();
    expect(screen.queryByLabelText('Tax slab')).toBeNull();

    // Empty and ready for the next one; the second item is a NEW item.
    await waitFor(() => expect((name as HTMLInputElement).value).toBe(''));
    expect(screen.getByLabelText('Short code')).toHaveValue('');
    fireEvent.change(name, { target: { value: 'Vada' } });
    fireEvent.submit(name.closest('form')!);
    await waitFor(() =>
      expect(call.mock.calls.filter(([n]) => n === 'save_menu_item')).toHaveLength(2),
    );
    const ids = call.mock.calls
      .filter(([n]) => n === 'save_menu_item')
      .map(([, args]) => (args as { edit: { id: string } }).edit.id);
    expect(ids[0]).not.toBe(ids[1]);
  });

  it('keeps available dishes first and offers a direct availability switch', async () => {
    open([{ ...dosa, id: 'off', name: 'Apple juice', isAvailable: false }, dosa]);
    await screen.findByRole('switch', { name: 'Available: Masala dosa' });
    const rows = screen.getAllByRole('row');
    expect(rows[1]).toHaveTextContent('Masala dosa');
    expect(rows[2]).toHaveTextContent('Apple juice');
    expect(screen.queryByText('Sold out')).toBeNull();
    fireEvent.click(screen.getByRole('switch', { name: 'Available: Apple juice' }));
    await waitFor(() => expect(call).toHaveBeenCalledWith('set_item_available', { itemId: 'off', available: true }));
  });

  it('hides GST in the list and keeps it in the full editor', async () => {
    open();
    await screen.findByText(dosa.name);
    expect(screen.queryByText(dosa.rate)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
    expect(within(screen.getByRole('dialog')).getByText(`GST ${dosa.rate}`)).toBeVisible();
  });

  it('edits a price and a missing short code directly without resending other item fields', async () => {
    open();
    const price = await screen.findByLabelText(`Price for ${dosa.name}`);
    expect(screen.queryByRole('button', { name: 'Save' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull();
    fireEvent.change(price, { target: { value: '125.50' } });
    fireEvent.keyDown(price, { key: 'Enter' });
    fireEvent.blur(price);
    await waitFor(() => expect(call).toHaveBeenCalledWith('edit_menu_item_field', { itemId: dosa.id, field: 'price', value: '125.50' }));
    expect(call.mock.calls.filter(([name]) => name === 'edit_menu_item_field')).toHaveLength(1);
    const code = screen.getByLabelText(`Short code for ${dosa.name}`);
    expect(code.closest('td')).not.toBe(screen.getByText(dosa.name).closest('td'));
    fireEvent.change(code, { target: { value: ' MD ' } });
    fireEvent.blur(code);
    await waitFor(() => expect(call).toHaveBeenCalledWith('edit_menu_item_field', { itemId: dosa.id, field: 'shortCode', value: 'MD' }));
    expect(call.mock.calls.some(([name]) => name === 'save_menu_item')).toBe(false);
  });

  it('cancels inline changes and keeps a failed edit open for correction', async () => {
    open();
    const input = await screen.findByLabelText(`Price for ${dosa.name}`);
    fireEvent.change(input, { target: { value: '132' } });
    fireEvent.keyDown(input, { key: 'Escape' });
    fireEvent.blur(input);
    expect(input).toHaveValue('120.00');
    expect(call.mock.calls.some(([name]) => name === 'edit_menu_item_field')).toBe(false);
    fireEvent.change(input, { target: { value: '131' } });
    call.mockRejectedValueOnce(new Error('save failed'));
    fireEvent.blur(input);
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not save');
    expect(input).toHaveValue('131');
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(input).toHaveValue('120.00');
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('leaves unchanged cells alone and refuses an empty price without silently changing it to zero', async () => {
    open();
    const input = await screen.findByLabelText(`Price for ${dosa.name}`);
    fireEvent.blur(input);
    expect(call.mock.calls.some(([name]) => name === 'edit_menu_item_field')).toBe(false);
    fireEvent.change(input, { target: { value: '' } });
    fireEvent.blur(input);
    expect(await screen.findByRole('alert')).toHaveTextContent('Enter a price');
    expect(call.mock.calls.some(([name]) => name === 'edit_menu_item_field')).toBe(false);
  });

  it('queues quick edits without disabling the next cell or losing its draft', async () => {
    open();
    const price = await screen.findByLabelText(`Price for ${dosa.name}`);
    let finish!: (rows: MenuRowView[]) => void;
    call.mockImplementationOnce(() => new Promise<MenuRowView[]>((resolve) => { finish = resolve; }));
    fireEvent.change(price, { target: { value: '130' } });
    fireEvent.blur(price);
    await waitFor(() => expect(finish).toBeDefined());
    const code = screen.getByLabelText(`Short code for ${dosa.name}`);
    expect(code).not.toHaveAttribute('readonly');
    fireEvent.change(code, { target: { value: 'MD' } });
    fireEvent.blur(code);
    expect(call.mock.calls.filter(([name]) => name === 'edit_menu_item_field')).toHaveLength(1);
    finish([{ ...dosa, price: money(13000, '130.00') }]);
    await waitFor(() => expect(call).toHaveBeenCalledWith('edit_menu_item_field', { itemId: dosa.id, field: 'shortCode', value: 'MD' }));
  });

  it('keeps deleted dishes separate and puts them back through the restore command', async () => {
    let active = [dosa];
    let deleted: MenuRowView[] = [];
    call.mockImplementation((name: string) => {
      if (name === 'delete_menu_item') { active = []; deleted = [{ ...dosa, isAvailable: false }]; }
      if (name === 'restore_menu_item') { active = [dosa]; deleted = []; }
      return Promise.resolve(name === 'menu_categories' ? [tiffin] :
        name === 'menu_deleted_rows' || name === 'restore_menu_item' ? deleted : active);
    });
    render(<ToastProvider><Menu /></ToastProvider>);
    fireEvent.click(await screen.findByRole('button', { name: `More for ${dosa.name}` }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveTextContent('You can put it back from Deleted items');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(screen.queryByRole('switch', { name: `Available: ${dosa.name}` })).toBeNull());
    fireEvent.click(screen.getByRole('button', { name: 'More for the menu' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Deleted items (1)' }));
    expect(await screen.findByText(dosa.name)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Put back' }));
    await waitFor(() => expect(call).toHaveBeenCalledWith('restore_menu_item', { itemId: dosa.id }));
    await screen.findByText('No deleted items here');
    fireEvent.click(screen.getByRole('button', { name: 'Back to menu' }));
    expect(await screen.findByRole('switch', { name: `Available: ${dosa.name}` })).toBeChecked();
  });

  it('puts a new item in the selected category', async () => {
    open([]);
    fireEvent.click(await screen.findByRole('button', { name: /^Tiffin/ }));
    const name = screen.getByLabelText('Item name');
    fireEvent.change(name, { target: { value: 'Upma' } });
    fireEvent.submit(name.closest('form')!);
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith(
        'save_menu_item',
        expect.objectContaining({ edit: expect.objectContaining({ categoryId: 'cat_tiffin' }) }),
      ),
    );
  });

  it('adds a category from its own row', async () => {
    open([]);
    const box = await screen.findByLabelText('New category');
    fireEvent.change(box, { target: { value: 'Drinks' } });
    fireEvent.submit(box.closest('form')!);
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith(
        'save_menu_category',
        expect.objectContaining({ name: 'Drinks', isActive: true }),
      ),
    );
  });

  it('renames and deletes a category in place', async () => {
    open([]);
    fireEvent.click(await screen.findByRole('button', { name: 'More for category Tiffin' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Rename Tiffin' }));
    const box = screen.getByLabelText('Category name');
    fireEvent.change(box, { target: { value: 'Breakfast' } });
    fireEvent.submit(box.closest('form')!);
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('save_menu_category', {
        id: 'cat_tiffin',
        name: 'Breakfast',
        isActive: true,
      }),
    );

    fireEvent.click(screen.getByRole('button', { name: 'More for category Tiffin' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete Tiffin' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('delete_menu_category', { categoryId: 'cat_tiffin' }),
    );
  });

  it('edits the rare fields in a dialog, and deletes with a confirmation', async () => {
    open();
    fireEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0]!);
    const dialog = screen.getByRole('dialog');
    fireEvent.change(within(dialog).getByLabelText('Minutes to cook'), { target: { value: '12' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith(
        'save_menu_item',
        expect.objectContaining({
          edit: expect.objectContaining({ id: 'itm_dosa', prepMinutes: '12', taxClassId: null }),
        }),
      ),
    );

    fireEvent.click(await screen.findByRole('button', { name: `More for ${dosa.name}` }));
    fireEvent.click(screen.getAllByRole('button', { name: 'Delete' })[0]!);
    const ask = screen.getByRole('dialog');
    fireEvent.click(within(ask).getByRole('button', { name: 'Delete' }));
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('delete_menu_item', { itemId: 'itm_dosa' }),
    );
  });

  const plan = {
    path: 'C:\Users\owner\Downloads\magic-bill-menu.csv',
    mode: 'update',
    summary: '240 new item(s) and 1 change(s). 9 new categories will be added.',
    newItems: 240n,
    updatedItems: 1n,
    unchanged: 0n,
    already: ['Tea (TEA)'],
    newCategories: ['CHINEES', 'JUICE'],
    removed: [],
    takenOff: [],
    retiredCategories: [],
    refused: [],
    isClean: true,
    isEmpty: false,
  };

  function openWithFile(answers: Record<string, unknown>) {
    open();
    call.mockImplementation((name: string) => {
      if (name in answers) return Promise.resolve(answers[name]);
      switch (name) {
        case 'menu_categories':
          return Promise.resolve([tiffin]);
        case 'menu_rows':
          return Promise.resolve([dosa]);
        default:
          return Promise.resolve([]);
      }
    });
  }

  it('imports through the counter’s own file dialog: pick, choose what the file does, read the plan, one Import', async () => {
    openWithFile({
      pick_menu_file: plan.path,
      plan_menu_import: plan,
      run_menu_import: '241 items imported. 9 categories were added.',
    });

    fireEvent.click(await screen.findByLabelText('More for the menu'));
    fireEvent.click(screen.getByText('Import a file'));
    // Rust opened the dialog: no file input on the page.
    expect(document.querySelector('input[type=file]')).toBeNull();
    await waitFor(() =>
      expect(call.mock.calls.filter(([n]) => n === 'plan_menu_import')).toHaveLength(1),
    );
    // Update is the default; the plan is asked for the mode chosen.
    expect(call.mock.calls.find(([n]) => n === 'plan_menu_import')![1]).toEqual({
      path: plan.path,
      mode: 'update',
    });

    expect(await screen.findByText(plan.summary)).toBeTruthy();
    expect(screen.getByText(/New categories: CHINEES, JUICE/)).toBeTruthy();
    // What the file is about to overwrite is named before anything is written.
    expect(screen.getByText(/Already on the menu.*Tea \(TEA\)/)).toBeTruthy();
    expect(screen.getByText(plan.path)).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Import' }));
    await waitFor(() =>
      expect(call.mock.calls.filter(([n]) => n === 'run_menu_import')).toHaveLength(1),
    );
    expect(call.mock.calls.find(([n]) => n === 'run_menu_import')![1]).toEqual({
      path: plan.path,
      mode: 'update',
    });
    await waitFor(() => expect(screen.queryByText(plan.summary)).toBeNull());
  });

  it('replacing the whole menu is its own choice, planned again, named in red, and pressed in red', async () => {
    const replacing = {
      ...plan,
      mode: 'replace',
      summary: '240 new item(s), 1 change(s), 0 already the same, and 2 item(s) not in the file would go.',
      removed: ['Masala dosa'],
      takenOff: ['Idli'],
      retiredCategories: ['Tiffin'],
    };
    openWithFile({
      pick_menu_file: plan.path,
      plan_menu_import: plan,
      run_menu_import: '241 items imported and 2 not in the file removed.',
    });
    fireEvent.click(await screen.findByLabelText('More for the menu'));
    fireEvent.click(screen.getByText('Import a file'));
    expect(await screen.findByText(plan.summary)).toBeTruthy();

    call.mockImplementation((name: string) =>
      Promise.resolve(
        name === 'plan_menu_import' ? replacing : name === 'run_menu_import' ? 'done' : [],
      ),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Replace the whole menu' }));
    await waitFor(() =>
      expect(call.mock.calls.filter(([n]) => n === 'plan_menu_import')).toHaveLength(2),
    );
    expect(call.mock.calls.filter(([n]) => n === 'plan_menu_import')[1]![1]).toEqual({
      path: plan.path,
      mode: 'replace',
    });
    expect(await screen.findByText(replacing.summary)).toBeTruthy();
    expect(screen.getByText(/Moved to Deleted items: Masala dosa, Idli/)).toBeTruthy();
    expect(screen.getByText(/Categories left empty and removed: Tiffin/)).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Replace the menu' }));
    await waitFor(() =>
      expect(call.mock.calls.filter(([n]) => n === 'run_menu_import')).toHaveLength(1),
    );
    expect(call.mock.calls.find(([n]) => n === 'run_menu_import')![1]).toEqual({
      path: plan.path,
      mode: 'replace',
    });
  });

  it('a file that would change nothing has no Import to press, and a cancelled dialog opens nothing', async () => {
    openWithFile({
      pick_menu_file: 'menu.csv',
      plan_menu_import: {
        ...plan,
        path: 'menu.csv',
        summary: 'Nothing would change — the menu already has all 3 of these.',
        newItems: 0n,
        updatedItems: 0n,
        unchanged: 3n,
        already: ['Tea', 'Masala dosa', 'Water bottle'],
        newCategories: [],
        isEmpty: true,
      },
    });
    fireEvent.click(await screen.findByLabelText('More for the menu'));
    fireEvent.click(screen.getByText('Import a file'));
    expect(await screen.findByText(/Nothing would change/)).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Import' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByText(/Nothing would change/)).toBeNull());

    // Cancelled in the dialog: nothing comes back, nothing opens.
    call.mockImplementation((name: string) =>
      Promise.resolve(name === 'pick_menu_file' ? null : name === 'menu_rows' ? [dosa] : [tiffin]),
    );
    fireEvent.click(screen.getByLabelText('More for the menu'));
    fireEvent.click(screen.getByText('Import a file'));
    await waitFor(() =>
      expect(call.mock.calls.filter(([n]) => n === 'pick_menu_file')).toHaveLength(2),
    );
    expect(screen.queryByText('Import a menu')).toBeNull();
  });
});
