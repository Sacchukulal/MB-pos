/** The roles tab: sections you can tick whole, boxes with an explanation, and an owner nobody edits. */

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

import type { PermissionGroupView } from '../src/ipc/generated/PermissionGroupView';
import type { RoleView } from '../src/ipc/generated/RoleView';

const call = vi.fn();
vi.mock('../src/ipc/call', () => ({
  call: (...args: unknown[]) => call(...args),
  inApp: () => false,
  isUiError: () => false,
  subscribe: () => Promise.resolve(() => undefined),
}));

const { Roles } = await import('../src/auth/Roles');
const { ToastProvider } = await import('../src/kit');

const GROUPS: PermissionGroupView[] = [
  {
    name: 'Billing',
    permissions: [
      { code: 'bill.create', label: 'Take orders and settle bills', hint: 'Take an order.' },
      { code: 'bill.reprint', label: 'Reprint a bill', hint: 'Print a settled bill again.' },
    ],
  },
  {
    name: 'Reports and history',
    permissions: [
      { code: 'reports.view', label: 'See reports', hint: 'Open the reports.' },
      { code: 'reports.export', label: 'Export and share reports', hint: 'Save a report.' },
      { code: 'audit.view', label: 'Read the history', hint: 'Read the audit trail.' },
    ],
  },
];

const OWNER: RoleView = {
  id: 'role_owner',
  name: 'Owner',
  isBuiltin: true,
  isOwner: true,
  permissions: GROUPS.flatMap((g) => g.permissions.map((p) => p.code)),
  maxDiscountPercent: null,
  maxDiscount: null,
  maxDiscountRupees: null,
};

const CASHIER: RoleView = {
  id: 'role_cashier',
  name: 'Cashier',
  isBuiltin: true,
  isOwner: false,
  permissions: ['bill.create'],
  maxDiscountPercent: '10%',
  maxDiscount: { paise: 20_000n, text: '200.00' },
  maxDiscountRupees: '200.00',
};

beforeEach(() => {
  call.mockReset();
  call.mockImplementation((command: string, args?: { role?: RoleView }) => {
    switch (command) {
      case 'list_roles':
        return Promise.resolve([OWNER, CASHIER]);
      case 'list_permissions':
        return Promise.resolve(GROUPS);
      case 'save_role':
        return Promise.resolve([OWNER, args?.role]);
      default:
        return Promise.resolve(null);
    }
  });
});
afterEach(cleanup);

function show() {
  return render(
    <ToastProvider>
      <Roles />
    </ToastProvider>,
  );
}

it('lists what each role may do, and locks the owner', async () => {
  show();
  await screen.findByText('Cashier');
  // Counted against the boxes on offer, and the caps read as words.
  expect(screen.getByText('1 of 5 allowed · discounts up to 10% or ₹200.00')).toBeTruthy();
  expect(screen.getByText('Everything, always')).toBeTruthy();
  expect(screen.getByText('Cannot be changed')).toBeTruthy();
  // One Edit button: the cashier's.
  expect(screen.getAllByRole('button', { name: 'Edit' })).toHaveLength(1);
});

it('ticks a whole section at once, and one box at a time', async () => {
  show();
  fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
  const dialog = await screen.findByRole('dialog');

  // The sections are there, in Rust's order, each with its count.
  const billing = within(dialog).getByRole('region', { name: 'Billing' });
  const reports = within(dialog).getByRole('region', { name: 'Reports and history' });
  expect(within(billing).getByText('1 of 2')).toBeTruthy();
  expect(within(reports).getByText('0 of 3')).toBeTruthy();

  // Every box has its explanation behind an info button, not under it.
  expect(within(billing).getByRole('button', { name: 'About Reprint a bill' })).toBeTruthy();

  // The section's own box is half-ticked while only some of it is on.
  const billingTick = within(billing).getByRole('checkbox', { name: 'Billing' }) as HTMLInputElement;
  expect(billingTick.indeterminate).toBe(true);
  expect(billingTick.checked).toBe(false);

  // One press on the section ticks all of it.
  fireEvent.click(within(reports).getByRole('checkbox', { name: 'Reports and history' }));
  expect(within(reports).getByText('3 of 3')).toBeTruthy();
  expect(
    (within(reports).getByRole('checkbox', { name: 'Read the history' }) as HTMLInputElement)
      .checked,
  ).toBe(true);

  // One box off again, and the section says so.
  fireEvent.click(within(reports).getByRole('checkbox', { name: 'Read the history' }));
  expect(within(reports).getByText('2 of 3')).toBeTruthy();

  // The rupee cap is editable beside the percentage.
  fireEvent.change(within(dialog).getByLabelText('Biggest discount, rupees'), {
    target: { value: '500' },
  });

  fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith('save_role', {
      role: expect.objectContaining({
        id: 'role_cashier',
        permissions: ['bill.create', 'reports.view', 'reports.export'],
        maxDiscountPercent: '10',
        maxDiscountRupees: '500',
      }),
    }),
  );
});
