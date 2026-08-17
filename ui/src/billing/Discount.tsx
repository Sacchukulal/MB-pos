/**
 * **Money off a bill** — scope 1.12, audit B7, and the owner's ask of
 * 2026-08-17: *"where is discount option?? i want to give discount to customer,
 * here no option showing."*
 *
 * # Everything for this existed except the door
 *
 * `mb_core::Discount` has spread a bill-level discount across the lines
 * *before* tax since P02 — which is what lets a bill mixing 5% food and 18%
 * packaged goods still tie rate by rate (audit B11). `DiscountPolicy` decides
 * who may give how much and when a reason is compulsory. The permission has
 * been a checkbox on the roles screen since P11. `Totals` already draws the
 * line when there is one. **`CartState.bill_discount` was `None` at birth and
 * never written again**, so no cashier could give a customer a rupee off, ever,
 * by any route.
 *
 * # Percent or rupees, and this file does no arithmetic
 *
 * The value is sent as TEXT and Rust turns it into basis points or paise (R8,
 * D39) — `parseFloat` here is how `0.30000000000000004` gets onto a bill, and
 * `check-no-money.mjs` fails the build if this file so much as multiplies.
 * What the customer will pay comes back in the recomputed cart, so the number
 * on screen is always the one the bill will print.
 *
 * # Why the refusal is shown here rather than as a toast
 *
 * A discount can be refused for a reason the cashier can fix in this box —
 * over their limit, or needing a reason they have not typed. A toast floats
 * away from the field it is about; this keeps it next to the number.
 */

import { useState } from 'react';

import { Button, Input, Modal, Select } from '../kit';
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
  /** The whole recomputed cart, straight from Rust (D4). */
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
      // Rust's sentence, verbatim — "that is 30% — you can give up to 10%" is
      // already what a cashier needs to read (audit F8).
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
          {/* Only when there is one to take off. A "Remove" that removes
              nothing is a button that does nothing, which is worse than one
              that is not there. */}
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
      {/* What the bill is before anything comes off it — the number the
          percentage is a percentage OF, so a cashier can check the answer. */}
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
      <Input
        label={kind === 'percent' ? 'Per cent' : 'Rupees'}
        hint={
          kind === 'percent'
            ? 'Like 10, or 12.5. It comes off before tax, so the tax on the bill stays correct.'
            : 'Like 50, or 50.00. It is spread across the lines before tax.'
        }
        value={value}
        autoFocus
        inputMode="decimal"
        error={problem ?? undefined}
        onChange={(event) => {
          setValue(event.target.value);
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
