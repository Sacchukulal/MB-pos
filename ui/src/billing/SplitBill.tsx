/** Split bill: some of this order becomes its own bill, at this table or another. */

import { useEffect, useState } from 'react';

import { Button, Hint, Modal, Notice, NumberInput, Numeric, onlyAmount, Select } from '../kit';
import { call } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';
import type { TableRowView } from '../ipc/generated/TableRowView';

export function SplitBill({
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
  /** The free tables the new bill could go to; empty means "this table". */
  const [free, setFree] = useState<readonly TableRowView[]>([]);
  const [toTable, setToTable] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    call('floor_plan')
      .then((floor) => setFree(floor.tables.filter((t) => t.isActive && !t.isBusy)))
      .catch(onFailed);
  }, [onFailed]);

  const picked = Object.entries(moving).filter(([, qty]) => qty.trim() !== '');
  const destination = free.find((t) => t.id === toTable);

  return (
    <Modal open title="Split bill" onClose={onClose} wide>
      {cart.orderId === null ? (
        <Notice tone="warn">
          This order has not been sent to the kitchen or put on a table yet, so
          there is nothing to move off it. Print the kitchen ticket first, or
          open the table.
        </Notice>
      ) : (
        <>
          <Hint>
            Type how many of each item move to the new bill. The kitchen is not
            told again — the food is already cooking.
          </Hint>

          <ul className="mb-split__lines">
            {cart.lines.map((line) => (
              <li key={line.index} className="mb-split__line">
                <span className="mb-split__name">{line.name}</span>
                <Numeric>{line.qty}</Numeric>
                <NumberInput
                  label="Move"
                  value={moving[line.index] ?? ''}
                  placeholder="0"
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

          {/* Where the new bill sits: here, or on a free table. */}
          {free.length > 0 ? (
            <Select
              label="New bill goes to"
              value={toTable}
              onChange={(event) => setToTable(event.target.value)}
              options={[
                { value: '', label: cart.table ? `This table (${cart.table})` : 'Stays here' },
                ...free.map((t) => ({ value: t.id, label: t.printed })),
              ]}
            />
          ) : null}

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
                    toTable: toTable === '' ? null : toTable,
                    seat: null,
                  },
                })
                  // Rust clears the cart when the order it was holding has been split — there
                  // are two orders now and it must not guess which one the cashier meant.
                  .then(() =>
                    onSplit(
                      destination
                        ? `Done. The new bill is on ${destination.printed} — open the one you are settling from the floor.`
                        : 'Done. There are two bills on that table now — open the one you are settling from the floor.',
                    ),
                  )
                  .catch(onFailed)
                  .finally(() => setBusy(false));
              }}
            >
              Split
            </Button>
          </div>
        </>
      )}
    </Modal>
  );
}
