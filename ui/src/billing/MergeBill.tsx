/** Merge bill: another open order's food joins the bill on the counter. */

import { useEffect, useState } from 'react';

import { Button, EmptyState, Hint, Modal, Notice, Numeric } from '../kit';
import { call } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';
import type { TableView } from '../ipc/generated/TableView';

/** The open orders that could join this one: every other one the floor holds. */
export function mergeCandidates(
  orders: readonly TableView[],
  orderId: string | null,
): TableView[] {
  return orders.filter((t) => t.orderId !== null && t.orderId !== orderId && !t.billedInto);
}

export function MergeBill({
  cart,
  orders,
  onClose,
  onMerged,
  onFailed,
}: {
  cart: CartView;
  /** The floor's open orders, as the counter already has them. */
  orders: readonly TableView[];
  onClose: () => void;
  /** The other order joined this one; the cart must be re-read. */
  onMerged: (said: string) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [available, setAvailable] = useState(orders);
  useEffect(() => { call('combine_candidates').then(setAvailable).catch(onFailed); }, [onFailed]);
  const others = mergeCandidates(available, cart.orderId);

  return (
    <Modal open title="Merge bill" onClose={onClose}>
      {cart.orderId === null ? (
        <Notice tone="warn">
          This order has not been sent to the kitchen or put on a table yet.
          Print the kitchen ticket first, then merge.
        </Notice>
      ) : others.length === 0 ? (
        <EmptyState title="Nothing to merge" hint="No other order is open right now." />
      ) : (
        <>
          <Hint>
            Choose an order or paid bill to include. Payments stay with the combined bill;
            each table stays occupied until its guests leave.
          </Hint>
          <ul className="mb-merge__list" aria-label="Orders that can join this bill">
            {others.map((other) => (
              <li key={other.orderId} className="mb-merge__row">
                <span className="mb-merge__name">
                  <span>{other.section === null ? other.label : `Table ${other.label}`}</span>
                  {other.token ? (
                    <span className="mb-merge__no"> · #{other.token}</span>
                  ) : other.billNumber ? (
                    <span className="mb-merge__no"> · {other.billNumber}</span>
                  ) : null}
                </span>
                <Numeric>{other.total?.text ?? ''}</Numeric>
                <Button
                  size="sm"
                  disabled={busy}
                  onClick={() => {
                    setBusy(true);
                    call('merge_orders', {
                      fromOrder: other.orderId ?? '',
                      intoOrder: cart.orderId ?? '',
                    })
                      .then(() =>
                        onMerged(
                          `${other.section === null ? other.label : `Table ${other.label}`} joined this bill.`,
                        ),
                      )
                      .catch(onFailed)
                      .finally(() => setBusy(false));
                  }}
                >
                  Merge
                </Button>
              </li>
            ))}
          </ul>
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={onClose}>
              Close
            </Button>
          </div>
        </>
      )}
    </Modal>
  );
}
