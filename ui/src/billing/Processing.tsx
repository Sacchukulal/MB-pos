/** The orders the kitchen has, until they are billed. Drawn from the same list as the grid. */

import { useEffect, useRef } from 'react';

import { Badge, cx, EmptyState, Icon } from '../kit';
import type { TableView } from '../ipc/generated/TableView';
import { formatMinutes } from './TableGrid';

/** The orders being cooked or served — sent to the kitchen and not yet billed, oldest first. */
export function processingOrders(
  tables: readonly TableView[],
  /** A shop with no kitchen ticket: every open order counts. */
  kitchenOff: boolean,
): TableView[] {
  return tables
    .filter((t) => t.orderId !== null && (kitchenOff || t.kitchenTold))
    .sort((a, b) => (b.minutes ?? -1) - (a.minutes ?? -1));
}

/** The panel's head: always in the top row, with the count, whether the list is open or not. */
export function ProcessingHead({
  count,
  open,
  controls,
  onToggle,
}: {
  count: number;
  open: boolean;
  /** The id of the list it folds. */
  controls: string;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      className="mb-processing__head"
      aria-expanded={open}
      aria-controls={controls}
      title={open ? 'Fold the processing orders away' : 'Show the processing orders'}
      onClick={onToggle}
    >
      <Icon name="flame" size="sm" />
      <span className="mb-processing__title">Processing orders</span>
      <Badge tone="accent">{count}</Badge>
      <Icon name={open ? 'chevron-up' : 'chevron-down'} size="sm" />
    </button>
  );
}

export function Processing({
  orders,
  highlighted = -1,
  onOpen,
}: {
  orders: readonly TableView[];
  /** The row the arrow keys are on, or none. */
  highlighted?: number;
  /** Put it in the cart — the same press as the tile. */
  onOpen: (order: TableView) => void;
}) {
  const list = useRef<HTMLUListElement>(null);
  // The arrows never leave the highlighted row out of sight.
  useEffect(() => {
    if (highlighted < 0) return;
    const row = list.current?.querySelectorAll('.mb-processing__order')[highlighted];
    // A test's DOM has no scrolling to do.
    if (row && typeof row.scrollIntoView === 'function') row.scrollIntoView({ block: 'nearest' });
  }, [highlighted]);

  if (orders.length === 0) {
    return (
      <EmptyState
        small
        title="Nothing cooking"
        body="Orders sent to the kitchen show here until they are billed."
      />
    );
  }
  return (
    <ul className="mb-processing" ref={list}>
      {orders.map((order, index) => (
        <li key={order.id}>
          <button
            type="button"
            className={cx(
              'mb-processing__order',
              order.state === 'waiting' && 'mb-processing__order--waiting',
              order.state === 'late' && 'mb-processing__order--late',
              order.selected && 'mb-processing__order--on',
              index === highlighted && 'mb-processing__order--highlighted',
            )}
            aria-pressed={order.selected}
            aria-current={index === highlighted ? 'true' : undefined}
            onClick={() => onOpen(order)}
          >
            {/* A real table says so; parcel and self service already name themselves. */}
            <span className="mb-processing__where">
              {order.section === null ? order.label : `Table ${order.label}`}
            </span>
            <span
              className={cx(
                'mb-processing__timer',
                order.state !== 'occupied' && 'mb-processing__timer--late',
              )}
            >
              {order.minutes === null ? '' : formatMinutes(order.minutes)}
            </span>
            <span className="mb-processing__no">
              {order.billNumber ?? ''}
              {order.kitchenMinutes === null ? null : (
                <span
                  className="mb-processing__food"
                  title="Since the kitchen was last told"
                  aria-label={`Kitchen ${formatMinutes(order.kitchenMinutes)}`}
                >
                  <Icon name="flame" size="sm" />
                  {formatMinutes(order.kitchenMinutes)}
                </span>
              )}
            </span>
            <span className="mb-processing__amount">{order.total ? order.total.text : ''}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}
