import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

import type { CartView } from '../src/ipc/generated/CartView';

const { call } = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock('../src/ipc/call', () => ({ call, isUiError: () => false }));
import { OrderConflict } from '../src/billing/OrderConflict';

beforeEach(() => { call.mockReset(); });
afterEach(cleanup);

it('keeps the draft and makes no command when the cashier chooses to keep editing', () => {
  const close = vi.fn();
  const reloaded = vi.fn();
  render(<OrderConflict onClose={close} onReloaded={reloaded} onFailed={vi.fn()} />);
  expect(screen.getByText(/discards unsaved local item changes/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Keep editing' }));
  expect(close).toHaveBeenCalledTimes(1);
  expect(call).not.toHaveBeenCalled();
  expect(reloaded).not.toHaveBeenCalled();
});

it('reloads once after confirmation and never retries the failed billing action', async () => {
  let finish!: (cart: CartView) => void;
  call.mockReturnValue(new Promise<CartView>((resolve) => { finish = resolve; }));
  const reloaded = vi.fn();
  render(<OrderConflict onClose={vi.fn()} onReloaded={reloaded} onFailed={vi.fn()} />);
  const button = screen.getByRole('button', { name: 'Reload saved order' });
  expect(call).not.toHaveBeenCalled();
  fireEvent.click(button);
  fireEvent.click(button);
  expect(button).toBeDisabled();
  expect(reloaded).not.toHaveBeenCalled();
  const cart = { orderId: 'table_4' } as CartView;
  await act(async () => { finish(cart); });
  expect(call).toHaveBeenCalledExactlyOnceWith('reload_current_order');
  expect(reloaded).toHaveBeenCalledExactlyOnceWith(cart);
});

it('keeps the current draft when Rust refuses to discard an unsaved receipt', async () => {
  const refusal = { code: 'order.unsaved_payment', message: 'Keep the receipt.' };
  call.mockRejectedValue(refusal);
  const reloaded = vi.fn();
  const failed = vi.fn();
  render(<OrderConflict onClose={vi.fn()} onReloaded={reloaded} onFailed={failed} />);
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Reload saved order' })); });
  expect(failed).toHaveBeenCalledWith(refusal);
  expect(reloaded).not.toHaveBeenCalled();
  expect(screen.getByRole('dialog')).toBeInTheDocument();
});
