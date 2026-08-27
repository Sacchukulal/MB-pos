/** "Put some of this on its own bill.". */

import { useState } from 'react';

import { Button, Modal, Notice, Numeric, onlyAmount, Input } from '../kit';
import { call } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';

export function SeparateBill({
  cart,
  onClose,
  onSplit,
  onFailed,
}: {
  cart: CartView;
  onClose: () => void;
  /** The order really changed, so the cart must be re-read. */
  onSplit: (said: string) => void;
  onFailed: (cause: unknown) => void;
}) {
  /** How much of each line moves to the new bill, by line index, as typed. */
  const [moving, setMoving] = useState<Record<number, string>>({});
  const [busy, setBusy] = useState(false);

  const picked = Object.entries(moving).filter(([, qty]) => qty.trim() !== '');

  return (
    <Modal open title="Put some of this on its own bill" onClose={onClose} wide>
      {cart.orderId === null ? (
        <Notice tone="warn">
          This order has not been sent to the kitchen or put on a table yet, so
          there is nothing to move off it. Print the kitchen ticket first, or
          open the table.
        </Notice>
      ) : (
        <>
          <p className="mb-field__hint">
            Type how many of each item move across. The new bill gets its own
            number and sits at the same table. The kitchen is not told again —
            the food is already cooking.
          </p>

          <ul className="mb-split__lines">
            {cart.lines.map((line) => (
              <li key={line.index} className="mb-split__line">
                <span className="mb-split__name">{line.name}</span>
                <Numeric>{line.qty}</Numeric>
                <Input
                  label="Move"
                  value={moving[line.index] ?? ''}
                  inputMode="decimal"
                  placeholder="0"
                  className="mb-input--number"
                  onChange={(event) =>
                    setMoving({
                      ...moving,
                      [line.index]: onlyAmount(event.target.value),
                    })
                  }
                />
              </li>
            ))}
          </ul>

          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={onClose}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={busy || picked.length === 0}
              onClick={() => {
                setBusy(true);
                call('split_order', {
                  request: {
                    orderId: cart.orderId ?? '',
                    lines: picked.map(([index, qty]) => [Number(index), qty.trim()]),
                    toTable: null,
                    seat: null,
                  },
                })
                  // Rust clears the cart when the order it was holding has been split — there
                  // are two orders now and it must not guess which one the cashier meant.
                  .then(() =>
                    onSplit(
                      'Done. There are two bills on that table now — open the one you are settling from the floor.',
                    ),
                  )
                  .catch(onFailed)
                  .finally(() => setBusy(false));
              }}
            >
              Move them
            </Button>
          </div>
        </>
      )}
    </Modal>
  );
}
