/**
 * **The expenses screen** — scope 10.6.
 *
 * Rust proves the arithmetic (`expense_tests.rs`); this proves the screen's
 * two claims:
 *
 * 1. **quick add is two fields and Enter**, because a cashier records a ₹40
 *    milk purchase mid-service or does not record it at all — and not
 *    recording it is exactly how v1's owner got an inflated profit every day;
 * 2. the cash position is shown as Rust's sentence, not assembled here.
 */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  isUiError: () => false,
}));

const { Expenses } = await import('../src/expenses/Expenses');
const { ToastProvider } = await import('../src/kit');

import type { ExpensesView } from '../src/ipc/generated/ExpensesView';

function money(paise: number, text: string) {
  return { paise: BigInt(paise), text };
}

const view: ExpensesView = {
  rows: [
    {
      id: 'exp_veg',
      categoryId: 'exc_vegetables',
      category: 'Vegetables',
      description: 'Mandi run',
      amount: money(40_000, '400.00'),
      mode: 'Cash',
      modeTag: 'cash',
      paidTo: 'Mandi',
      reference: null,
      inputCredit: null,
      note: null,
    },
    {
      id: 'exp_gas',
      categoryId: 'exc_gas',
      category: 'Gas',
      description: 'Cylinder',
      amount: money(118_000, '1,180.00'),
      mode: 'Bank',
      modeTag: 'bank',
      paidTo: 'HP',
      reference: 'INV-9',
      inputCredit: '18% · 180.00',
      note: null,
    },
  ],
  categories: [
    { id: 'exc_vegetables', name: 'Vegetables', total: money(40_000, '400.00'), count: 1 },
    { id: 'exc_gas', name: 'Gas', total: money(118_000, '1,180.00'), count: 1 },
  ],
  allCategories: [
    { id: 'exc_vegetables', name: 'Vegetables', total: money(0, '0.00'), count: 0 },
    { id: 'exc_gas', name: 'Gas', total: money(0, '0.00'), count: 0 },
  ],
  movements: [
    {
      id: 'cm_1',
      kind: 'Payout',
      kindTag: 'payout',
      amount: money(30_000, '300.00'),
      reason: 'to the boy',
      takesOut: true,
    },
  ],
  cash: {
    openingFloat: money(200_000, '2,000.00'),
    cashSales: money(12_600, '126.00'),
    topUps: money(0, '0.00'),
    cashExpenses: money(40_000, '400.00'),
    payouts: money(30_000, '300.00'),
    bankDrops: money(100_000, '1,000.00'),
    expected: money(42_600, '426.00'),
    says: '2,000.00 float + 126.00 cash sales + 0.00 top-ups − 400.00 expenses − 300.00 payouts − 1,000.00 to the bank',
  },
  total: money(158_000, '1,580.00'),
  thisMonth: money(158_000, '1,580.00'),
  lastMonth: money(0, '0.00'),
  due: [
    {
      id: 'rec_rent',
      description: 'Shop rent',
      amount: money(2_500_000, '25,000.00'),
      paidTo: 'Landlord',
      when: 'due today',
    },
  ],
};

function show() {
  return render(
    <ToastProvider>
      <Expenses />
    </ToastProvider>,
  );
}

beforeEach(() => {
  call.mockReset();
  call.mockResolvedValue(view);
});
afterEach(cleanup);

describe('recording what goes out (scope 10.6)', () => {
  it('records from two fields and the Enter key', async () => {
    show();
    await screen.findByText('Mandi run');

    fireEvent.change(screen.getByLabelText('What'), { target: { value: 'Milk' } });
    fireEvent.change(screen.getByLabelText('Amount'), { target: { value: '40' } });
    fireEvent.keyDown(screen.getByLabelText('Amount'), { key: 'Enter' });

    const sent = call.mock.calls.find((c) => c[0] === 'save_expense');
    expect(sent).toBeTruthy();
    const edit = (sent?.[1] as { edit: { description: string; amount: string; mode: string } }).edit;
    expect(edit.description).toBe('Milk');
    expect(edit.amount).toBe('40');
    expect(edit.mode).toBe('cash');
  });

  it('does not record an empty row', async () => {
    show();
    await screen.findByText('Mandi run');

    // Two buttons say "Record it" — quick add and the reminder. The first is
    // quick add.
    fireEvent.click(screen.getAllByText('Record it')[0] as HTMLElement);
    expect(call.mock.calls.some((c) => c[0] === 'save_expense')).toBe(false);
  });

  it('shows the input credit as Rust wrote it', async () => {
    show();
    expect(await screen.findByText('18% · 180.00')).toBeTruthy();
  });

  /** Cash carries a badge as well as a word — §2 rule 2. */
  it('marks what came out of the drawer', async () => {
    const { container } = show();
    await screen.findByText('Mandi run');
    expect(container.querySelector('.mb-badge--warn')?.textContent).toBe('Cash');
  });
});

describe('the drawer (scope 10.6)', () => {
  it('shows the sum as one sentence Rust assembled', async () => {
    show();
    expect(await screen.findByText(/2,000.00 float \+ 126.00 cash sales/)).toBeTruthy();
    expect(screen.getByText('426.00')).toBeTruthy();
  });

  it('a purchase is not a cash movement, and the dialog says so', async () => {
    show();
    await screen.findByText('Mandi run');
    fireEvent.click(screen.getByText('Move cash'));

    expect(await screen.findByText(/A purchase is not one of these/)).toBeTruthy();
    const kinds = screen.getByLabelText('What happened') as HTMLSelectElement;
    expect([...kinds.options].map((o) => o.value)).toEqual([
      'float',
      'top_up',
      'payout',
      'bank_drop',
    ]);
  });
});

describe('reminders', () => {
  it('shows what is due and posts nothing until it is confirmed', async () => {
    show();
    await screen.findByText(/Shop rent/);
    // It is a prompt, not a posting: nothing has been sent.
    expect(call.mock.calls.some((c) => c[0] === 'confirm_recurring_expense')).toBe(false);

    fireEvent.click(screen.getAllByText('Record it')[1] ?? screen.getByText('Record it'));
    const sent = call.mock.calls.find((c) => c[0] === 'confirm_recurring_expense');
    expect(sent?.[1]).toEqual({ id: 'rec_rent' });
  });
});
