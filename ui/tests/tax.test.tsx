/** Settings › Tax — GST on or off, the shop rate, and the items on their own. */

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => true,
  isUiError: () => false,
}));

const { Tax } = await import('../src/settings/Tax');
const { ToastProvider } = await import('../src/kit');

import type { TaxPageView } from '../src/ipc/generated/TaxPageView';
import type { TaxItemView } from '../src/ipc/generated/TaxItemView';

function item(id: string, name: string, slab: string, from: string): TaxItemView {
  return {
    id,
    name,
    price: { paise: 2_000n, text: '20.00' },
    slabId: slab,
    basis: 'shop',
    words: slab === 'tax_food_5' ? '5% · added on top' : '18% · added on top',
    from,
    isAvailable: true,
  };
}

const page: TaxPageView = {
  registration: 'regular',
  gstin: '29ABCDE1234F1Z5',
  stateCode: '29',
  states: [
    { value: '', label: 'Not chosen yet' },
    { value: '29', label: 'Karnataka' },
  ],
  shopBasis: 'exclusive',
  shopRate: '5',
  shopSlabId: 'tax_food_5',
  chargesGst: true,
  registrationNote: null,
  slabs: [
    {
      id: 'tax_food_5',
      name: '5%',
      rate: '5%',
      rateBp: 500,
      kind: 'gst',
      basis: 'shop',
      priceWords: 'Shop default (added on top)',
      isActive: true,
      itemsUsing: 3,
    },
    {
      id: 'tax_packaged_18',
      name: '18%',
      rate: '18%',
      rateBp: 1800,
      kind: 'gst',
      basis: 'shop',
      priceWords: 'Shop default (added on top)',
      isActive: true,
      itemsUsing: 0,
    },
  ],
  categories: [
    {
      id: 'cat_tiffin',
      name: 'Tiffin',
      ownSlabId: null,
      rateWords: 'Shop rate',
      items: [
        item('itm_idli', 'Idli', 'tax_food_5', 'shop'),
        item('itm_dosa', 'Dosa', 'tax_food_5', 'shop'),
      ],
    },
    {
      id: 'cat_drinks',
      name: 'Drinks',
      ownSlabId: 'tax_packaged_18',
      rateWords: '18%',
      items: [item('itm_water', 'Water', 'tax_packaged_18', 'category')],
    },
  ],
};

function open(view: TaxPageView = page) {
  call.mockImplementation((name: string) => {
    switch (name) {
      case 'tax_page':
      case 'set_shop_tax_rate':
      case 'set_items_tax':
      case 'set_category_tax':
        return Promise.resolve(view);
      case 'save_settings':
        return Promise.resolve({ changed: [], settings: { groups: [], hasShop: true, trouble: null } });
      default:
        return Promise.resolve([]);
    }
  });
  return render(
    <ToastProvider>
      <Tax />
    </ToastProvider>,
  );
}

beforeEach(() => {
  call.mockReset();
});
afterEach(cleanup);

describe('the GST card', () => {
  it('saves the shop rate as a typed percentage, before the rest', async () => {
    open();
    const box = (await screen.findByLabelText('Shop GST rate %')) as HTMLInputElement;
    expect(box.value).toBe('5');
    fireEvent.change(box, { target: { value: '18' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(call).toHaveBeenCalledWith('set_shop_tax_rate', { percent: '18' }));
    // Nothing else changed, so the settings were not written.
    expect(call.mock.calls.some(([name]) => name === 'save_settings')).toBe(false);
  });

  it('writes the registration, number, state and price rule as settings', async () => {
    open();
    await screen.findByLabelText('Shop GST rate %');
    fireEvent.click(screen.getByRole('button', { name: 'Inside the price' }));
    fireEvent.change(screen.getByLabelText('GST number'), { target: { value: '29XYZ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('save_settings', {
        edits: [
          { key: 'store.gstin', value: '29XYZ' },
          { key: 'store.price_basis', value: 'inclusive' },
        ],
      }),
    );
    expect(call.mock.calls.some(([name]) => name === 'set_shop_tax_rate')).toBe(false);
  });

  it('hides the number, the price rule and the rate when GST is off', async () => {
    open();
    await screen.findByLabelText('Shop GST rate %');
    fireEvent.click(screen.getByRole('button', { name: 'No GST' }));
    expect(screen.queryByLabelText('GST number')).toBeNull();
    expect(screen.queryByLabelText('Shop GST rate %')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Inside the price' })).toBeNull();
  });

  it('keeps the number for a composition shop, which has one but charges nothing', async () => {
    open();
    await screen.findByLabelText('Shop GST rate %');
    fireEvent.click(screen.getByRole('button', { name: 'Composition scheme' }));
    expect(screen.getByLabelText('GST number')).toBeTruthy();
    expect(screen.queryByLabelText('Shop GST rate %')).toBeNull();
  });
});

describe('item GST', () => {
  it('says which rung each item is on', async () => {
    open();
    await screen.findByText('Idli');
    const water = screen.getByText('Water').closest('tr')!;
    expect(within(water).getByText('Category')).toBeTruthy();
    const idli = screen.getByText('Idli').closest('tr')!;
    expect(within(idli).getByText('Shop')).toBeTruthy();
  });

  it('ticks items and puts them on a rate', async () => {
    open();
    await screen.findByText('Idli');
    fireEvent.click(screen.getByLabelText('Tick Idli'));
    fireEvent.click(screen.getByLabelText('Tick Dosa'));
    fireEvent.change(screen.getByLabelText('Rate for the ticked items'), {
      target: { value: 'tax_packaged_18' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('set_items_tax', {
        itemIds: ['itm_idli', 'itm_dosa'],
        slabId: 'tax_packaged_18',
        basis: null,
      }),
    );
  });

  it('sends a ticked item back up the ladder with "follow"', async () => {
    open();
    await screen.findByText('Water');
    fireEvent.click(screen.getByLabelText('Tick Water'));
    fireEvent.change(screen.getByLabelText('Rate for the ticked items'), {
      target: { value: 'follow' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('set_items_tax', {
        itemIds: ['itm_water'],
        slabId: 'follow',
        basis: null,
      }),
    );
  });

  it('will not apply nothing', async () => {
    open();
    await screen.findByText('Idli');
    fireEvent.click(screen.getByLabelText('Tick Idli'));
    expect((screen.getByRole('button', { name: 'Apply' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('sets a category rate once a category is chosen', async () => {
    open();
    await screen.findByText('Idli');
    fireEvent.change(screen.getByLabelText('Category'), { target: { value: 'cat_drinks' } });
    const rate = (await screen.findByLabelText('Rate for Drinks')) as HTMLSelectElement;
    expect(rate.value).toBe('tax_packaged_18');
    fireEvent.change(rate, { target: { value: 'follow' } });

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('set_category_tax', {
        categoryId: 'cat_drinks',
        slabId: null,
      }),
    );
  });

  it('is not drawn at all for a shop with no GST', async () => {
    open({ ...page, registration: 'unregistered', chargesGst: false });
    await screen.findByRole('button', { name: 'No GST' });
    expect(screen.queryByText('Item GST')).toBeNull();
  });
});
