/** Settings › Tax — the slabs, and ticking items onto them. */

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

function item(id: string, name: string, slab: string): TaxItemView {
  return {
    id,
    name,
    price: { paise: 2_000n, text: '20.00' },
    slabId: slab,
    slabName: slab === 'tax_food_5' ? 'GST 5%' : 'GST 18%',
    basis: 'shop',
    words: slab === 'tax_food_5' ? '5% · added on top' : '18% · added on top',
    isAvailable: true,
  };
}

const page: TaxPageView = {
  shopBasis: 'exclusive',
  registrationNote: null,
  slabs: [
    {
      id: 'tax_food_5',
      name: 'GST 5%',
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
      name: 'GST 18%',
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
      id: 'cat_chats',
      name: 'Chats',
      defaultSlabId: 'tax_food_5',
      items: [
        item('itm_bhel', 'Bhel puri', 'tax_food_5'),
        item('itm_pani', 'Pani puri', 'tax_food_5'),
        item('itm_biscuit', 'Biscuit packet', 'tax_food_5'),
      ],
    },
  ],
};

beforeEach(() => {
  call.mockReset();
  call.mockImplementation((name: string) => {
    switch (name) {
      case 'tax_page':
      case 'set_items_tax':
      case 'set_category_tax':
        return Promise.resolve(page);
      case 'save_tax_slab':
      case 'remove_tax_slab':
        return Promise.resolve(page.slabs);
      default:
        return Promise.resolve([]);
    }
  });
});

afterEach(cleanup);

async function open() {
  render(
    <ToastProvider>
      <Tax />
    </ToastProvider>,
  );
  await screen.findByText('Bhel puri');
}

describe('the tick list', () => {
  it('ticks a whole category, lets one be unticked, and moves the rest to a slab', async () => {
    await open();

    // The biscuit is the counter-example: a 5% chats stall selling an 18% packet.
    fireEvent.click(screen.getByLabelText('Tick every item in Chats'));
    fireEvent.click(screen.getByLabelText('Tick Bhel puri'));
    fireEvent.click(screen.getByLabelText('Tick Pani puri'));
    expect(screen.getByText('1 item ticked')).toBeTruthy();

    fireEvent.change(screen.getByLabelText('Move the ticked items to'), {
      target: { value: 'tax_packaged_18' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('set_items_tax', {
        itemIds: ['itm_biscuit'],
        slabId: 'tax_packaged_18',
        basis: null,
      }),
    );
  });

  it('applies a price rule on its own, leaving the slab alone', async () => {
    await open();
    fireEvent.click(screen.getByLabelText('Tick Pani puri'));
    fireEvent.change(screen.getByLabelText('Price rule for the ticked items'), {
      target: { value: 'inclusive' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('set_items_tax', {
        itemIds: ['itm_pani'],
        slabId: null,
        basis: 'inclusive',
      }),
    );
  });

  it('will not apply nothing', async () => {
    await open();
    const apply = screen.getByRole('button', { name: 'Apply' }) as HTMLButtonElement;
    expect(apply.disabled, 'nothing ticked').toBe(true);
    fireEvent.click(screen.getByLabelText('Tick Pani puri'));
    expect(apply.disabled, 'ticked, but no slab or rule chosen').toBe(true);
  });

  it('sets what a new item in the category starts on', async () => {
    await open();
    fireEvent.change(screen.getByLabelText('New items in Chats start on'), {
      target: { value: 'tax_packaged_18' },
    });
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('set_category_tax', {
        categoryId: 'cat_chats',
        slabId: 'tax_packaged_18',
      }),
    );
  });
});

describe('the slabs', () => {
  it('adds a custom slab with the machine values, never the words', async () => {
    await open();
    fireEvent.click(screen.getByRole('button', { name: 'Add a slab' }));
    const dialog = screen.getByRole('dialog');
    fireEvent.change(within(dialog).getByLabelText('Name'), {
      target: { value: 'Sweets 12%' },
    });
    fireEvent.change(within(dialog).getByLabelText('Rate %'), { target: { value: '12' } });
    fireEvent.change(within(dialog).getByLabelText('Price'), {
      target: { value: 'inclusive' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add it' }));

    await waitFor(() =>
      expect(call).toHaveBeenCalledWith(
        'save_tax_slab',
        expect.objectContaining({
          edit: expect.objectContaining({
            name: 'Sweets 12%',
            rate: '12',
            kind: 'gst',
            basis: 'inclusive',
          }),
        }),
      ),
    );
  });

  it('shuts the rate box on a kind that cannot carry one', async () => {
    await open();
    fireEvent.click(screen.getByRole('button', { name: 'Add a slab' }));
    const dialog = screen.getByRole('dialog');
    fireEvent.change(within(dialog).getByLabelText('Kind'), { target: { value: 'exempt' } });
    const box = within(dialog).getByLabelText('Rate %') as HTMLInputElement;
    expect(box.disabled).toBe(true);
    expect(box.value).toBe('0');
  });

  it('will not remove a slab that items still use', async () => {
    await open();
    const removes = screen.getAllByRole('button', { name: 'Remove' }) as HTMLButtonElement[];
    expect(removes[0]!.disabled, 'GST 5% has three items on it').toBe(true);
    expect(removes[1]!.disabled, 'GST 18% has none').toBe(false);
  });
});
