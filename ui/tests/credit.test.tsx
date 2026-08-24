/**
 * **The credit screens** — scope 5.1–5.4.
 *
 * Rust proves the ledger (`credit_tests.rs`); this proves what the screen does
 * with it:
 *
 * 1. the default view is **who owes me money**, not an alphabetical list;
 * 2. "no limit" is shown as no limit, never as 0.00 — the difference between
 *    "may owe anything" and "may owe nothing";
 * 3. the statement's running column is Rust's, printed as it arrived;
 * 4. putting a bill on an account asks what it would do FIRST.
 */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  isUiError: () => false,
}));

const { Credit, PutOnAccount } = await import('../src/credit/Credit');
const { ToastProvider } = await import('../src/kit');

import type { AccountView } from '../src/ipc/generated/AccountView';
import type { CustomerView } from '../src/ipc/generated/CustomerView';

function money(paise: number, text: string) {
  return { paise: BigInt(paise), text };
}

function customer(over: Partial<CustomerView> & Pick<CustomerView, 'id' | 'name'>): CustomerView {
  return {
    phone: '9876543210',
    gstin: null,
    address: null,
    creditLimit: money(500_000, '5,000.00'),
    isActive: true,
    balance: money(420_000, '4,200.00'),
    oldest: '74 days',
    ...over,
  };
}

const account: AccountView = {
  customer: customer({ id: 'cus_rekha', name: 'Rekha' }),
  ageing: {
    current: money(0, '0.00'),
    days30: money(0, '0.00'),
    days60: money(420_000, '4,200.00'),
    days90: money(0, '0.00'),
    oldest: '74 days',
  },
  movements: [
    {
      date: '2026-05-27',
      kind: 'Bill',
      note: 'BIR/1207',
      amount: money(500_000, '5,000.00'),
      adds: true,
      running: money(500_000, '5,000.00'),
    },
    {
      date: '2026-06-04',
      kind: 'Repayment',
      note: 'cash',
      amount: money(80_000, '800.00'),
      adds: false,
      running: money(420_000, '4,200.00'),
    },
  ],
  statement: 'Statement — Rekha\n\nOutstanding: 4,200.00\n',
};

function show() {
  return render(
    <ToastProvider>
      <Credit />
    </ToastProvider>,
  );
}

beforeEach(() => {
  call.mockReset();
});
afterEach(cleanup);

describe('who owes me money (scope 5.1)', () => {
  /**
   * **One list, and everybody is on it** — the owner, 2026-08-24: *"when i add
   * a credit customer, it seemed like it disappeared but the thing was it was
   * in everybody section."*
   *
   * The screen used to open on "who owes me", and a customer added a minute ago
   * owes nothing — so adding one looked exactly like losing one.
   */
  it('asks for everybody, and has no second view to lose a customer in', async () => {
    call.mockResolvedValue([customer({ id: 'cus_rekha', name: 'Rekha' })]);
    show();
    await screen.findByText('Rekha');

    expect(call.mock.calls[0]?.[0]).toBe('customers');
    expect(call.mock.calls.some((c) => c[0] === 'who_owes')).toBe(false);
    expect(screen.queryByText('Everybody')).toBeNull();
    expect(screen.queryByText('Who owes me')).toBeNull();
  });

  /**
   * **The customer form is the side panel** — the owner, same day: *"i want add
   * customer panel in the side (foldable)"*, and the screen's own "Credit 0"
   * title and sub-line went with the dialog.
   */
  it('adds and edits in one folding side panel, not a dialog', async () => {
    call.mockResolvedValue([customer({ id: 'cus_rekha', name: 'Rekha' })]);
    show();
    await screen.findByText('Rekha');

    // Folded: one button, and no repeated screen title.
    expect(screen.queryByRole('complementary', { name: 'Add a customer' })).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Credit' })).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Add a customer' }));
    const panel = screen.getByRole('complementary', { name: 'Add a customer' });
    expect(panel).toBeTruthy();
    expect(screen.getByLabelText('Name')).toBeTruthy();

    // Edit opens the SAME panel, holding that customer.
    fireEvent.click(screen.getByRole('button', { name: 'Close Add a customer' }));
    fireEvent.click(screen.getByText('Edit'));
    expect((screen.getByLabelText('Name') as HTMLInputElement).value).toBe('Rekha');
  });

  it('shows how long it has been owed, which is the number an owner acts on', async () => {
    call.mockResolvedValue([customer({ id: 'cus_rekha', name: 'Rekha' })]);
    show();
    expect(await screen.findByText('74 days')).toBeTruthy();
    expect(screen.getByText('4,200.00')).toBeTruthy();
  });

  /** Blank is no limit. A screen that renders it as 0.00 says the opposite. */
  it('says "No limit" rather than zero', async () => {
    call.mockResolvedValue([
      customer({ id: 'cus_free', name: 'Anand', creditLimit: null }),
    ]);
    show();
    expect(await screen.findByText('No limit')).toBeTruthy();
    expect(screen.queryByText('0.00')).toBeNull();
  });
});

describe('the account (scope 5.3)', () => {
  it('prints the running balance Rust computed, and ends at the balance', async () => {
    call.mockImplementation((name: string) =>
      Promise.resolve(name === 'customer_account' ? account : [customer({ id: 'cus_rekha', name: 'Rekha' })]),
    );
    show();
    fireEvent.click(await screen.findByText('Open'));

    // Both movements, both running figures, as they arrived.
    expect(await screen.findByText('BIR/1207')).toBeTruthy();
    expect(screen.getByText('-800.00')).toBeTruthy();
    // The last running figure is the balance — the property of a statement.
    const rows = [...document.querySelectorAll('.mb-ledger tbody tr')];
    const last = rows[rows.length - 1]?.textContent ?? '';
    expect(last).toContain('4,200.00');
  });

  it('takes a repayment in a real payment mode and nothing else', async () => {
    call.mockImplementation((name: string) =>
      Promise.resolve(name === 'customer_account' ? account : [customer({ id: 'cus_rekha', name: 'Rekha' })]),
    );
    show();
    fireEvent.click(await screen.findByText('Open'));

    const how = (await screen.findByLabelText('How')) as HTMLSelectElement;
    expect([...how.options].map((o) => o.value)).toEqual(['cash', 'card', 'upi']);

    fireEvent.change(screen.getByLabelText('Amount'), { target: { value: '500' } });
    fireEvent.click(screen.getByText('Take it'));

    const sent = call.mock.calls.find((c) => c[0] === 'record_repayment');
    expect(sent?.[1]).toEqual({
      customerId: 'cus_rekha',
      amount: '500',
      mode: 'cash',
      reference: '',
    });
  });
});

describe('putting a bill on an account (scope 5.2)', () => {
  it('asks what it would do before it does it', async () => {
    call.mockImplementation((name: string) => {
      if (name === 'customers') return Promise.resolve([customer({ id: 'cus_rekha', name: 'Rekha' })]);
      if (name === 'credit_headroom') {
        return Promise.resolve({
          customer: 'Rekha',
          balance: money(420_000, '4,200.00'),
          after: money(534_000, '5,340.00'),
          limit: money(500_000, '5,000.00'),
          verdict: 'over',
          says: 'Rekha owes 4,200.00 and the limit is 5,000.00. This bill takes them to 5,340.00.',
        });
      }
      return Promise.resolve(null);
    });

    render(
      <ToastProvider>
        <PutOnAccount onClose={vi.fn()} onDone={vi.fn()} onFailed={vi.fn()} />
      </ToastProvider>,
    );

    fireEvent.click(await screen.findByText('Rekha'));

    // The sentence Rust wrote, and the fact that it needs approval.
    expect(await screen.findByText(/takes them to 5,340.00/)).toBeTruthy();
    expect(screen.getByText(/Past the limit/)).toBeTruthy();

    // The button says what will happen — §6.
    fireEvent.click(screen.getByText('Approve and put it on'));
    const sent = call.mock.calls.find((c) => c[0] === 'put_on_account');
    expect(sent?.[1]).toEqual({ customerId: 'cus_rekha', overrideLimit: true });
  });
});
