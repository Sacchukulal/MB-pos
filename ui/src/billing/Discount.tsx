/** Money off a bill. */

import { useState } from 'react';

import { Button, Input, Modal, Select, onlyAmount } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';

const KINDS = [
  { value: 'percent', label: 'A percentage' },
  { value: 'amount', label: 'Rupees off' },
];

export function DiscountDialog({
  cart,
  onClose,
  onChanged,
}: {
  cart: CartView;
  onClose: () => void;
  /** The whole recomputed cart, straight from Rust. */
  onChanged: (cart: CartView) => void;
}) {
  const [kind, setKind] = useState('percent');
  const [value, setValue] = useState('');
  const [reason, setReason] = useState('');
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const has = cart.bill.billDiscount.paise > 0n;

  const apply = async () => {
    setBusy(true);
    setProblem(null);
    try {
      onChanged(
        await call('cart_set_discount', {
          kind,
          value,
          reason: reason.trim() === '' ? null : reason.trim(),
        }),
      );
      onClose();
    } catch (cause) {
      // Rust's sentence, verbatim — "that is 30% — you can give up to 10%" is already what a
      // cashier needs to read.
      setProblem(isUiError(cause) ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const clear = async () => {
    setBusy(true);
    setProblem(null);
    try {
      onChanged(await call('cart_clear_discount'));
      onClose();
    } catch (cause) {
      setProblem(isUiError(cause) ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open
      title="Money off this bill"
      onClose={onClose}
      actions={
        <>
          {/* Only when there is one to take off. */}
          {has ? (
            <Button variant="danger" disabled={busy} onClick={() => void clear()}>
              Remove the discount
            </Button>
          ) : null}
          <Button variant="quiet" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={busy || value.trim() === ''}
            onClick={() => void apply()}
          >
            Take it off
          </Button>
        </>
      }
    >
      {/*
        What the bill is before anything comes off it — the number the percentage is a
        percentage OF, so a cashier can check the answer.
      */}
      <p className="mb-muted">
        This bill is {cart.bill.subtotal.text} before tax
        {has ? `, with ${cart.bill.billDiscount.text} already off` : ''}.
      </p>

      <Select
        label="How much"
        value={kind}
        options={KINDS}
        onChange={(event) => {
          setKind(event.currentTarget.value);
          setProblem(null);
        }}
      />
{/* Money and a percentage are the same characters and not the same thing. */}
      <Input
        label={kind === 'percent' ? 'Per cent' : 'Rupees'}
        prefix={kind === 'percent' ? undefined : '₹'}
        hint={
          kind === 'percent'
            ? 'Like 10, or 12.5. It comes off before tax, so the tax on the bill stays correct.'
            : 'Like 50, or 50.00. It is spread across the lines before tax.'
        }
        value={value}
        autoFocus
        inputMode="decimal"
        className="mb-input--money"
        error={problem ?? undefined}
        onChange={(event) => {
          setValue(onlyAmount(event.target.value));
          setProblem(null);
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter' && value.trim() !== '') void apply();
        }}
      />
      <Input
        label="Why (optional)"
        hint="Some discounts need one — the shop decides how big, on the roles screen."
        value={reason}
        onChange={(event) => setReason(event.target.value)}
      />
    </Modal>
  );
}
