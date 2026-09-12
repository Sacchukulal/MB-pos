/** Money off a bill, or off one line of it. */

import { useState } from 'react';

import { Button, Input, Modal, Select, onlyAmount } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { CartLineView } from '../ipc/generated/CartLineView';
import type { CartView } from '../ipc/generated/CartView';

const KINDS = [
  { value: 'percent', label: 'A percentage' },
  { value: 'amount', label: 'Rupees off' },
];

export function DiscountDialog({
  cart,
  line = null,
  onClose,
  onChanged,
}: {
  cart: CartView;
  /** One line of the bill, when the money comes off that line alone. */
  line?: CartLineView | null;
  onClose: () => void;
  /** The whole recomputed cart, straight from Rust. */
  onChanged: (cart: CartView) => void;
}) {
  const [kind, setKind] = useState('percent');
  const [value, setValue] = useState('');
  const [reason, setReason] = useState('');
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // What is already off: this line's own discount, or the bill's.
  const already = line ? line.lineDiscount : cart.bill.billDiscount;
  const has = already.paise > 0n;

  const apply = async () => {
    setBusy(true);
    setProblem(null);
    try {
      onChanged(
        await call('cart_set_discount', {
          kind,
          value,
          reason: reason.trim() === '' ? null : reason.trim(),
          line: line ? line.index : null,
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
      onChanged(await call('cart_clear_discount', { line: line ? line.index : null }));
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
      title={line ? `Money off ${line.name}` : 'Money off this bill'}
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
        What the money comes off before anything comes off it — the number the percentage is a
        percentage OF, so a cashier can check the answer.
      */}
      <p className="mb-muted">
        {line
          ? `${line.qty} × ${line.name} is ${line.gross.text} before tax`
          : `This bill is ${cart.bill.subtotal.text} before tax`}
        {has ? `, with ${already.text} already off` : ''}.
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
            : line
              ? 'Like 50, or 50.00. It comes off this line before tax.'
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
