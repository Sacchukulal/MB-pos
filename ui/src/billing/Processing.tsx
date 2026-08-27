/** The orders the kitchen has, until they are billed. Drawn from the same list as the grid. */

import { cx, EmptyState, Icon } from '../kit';
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

export function Processing({
  orders,
  onOpen,
  onPrintBill,
}: {
  orders: readonly TableView[];
  /** Put it in the cart — the same press as the tile. */
  onOpen: (order: TableView) => void;
  /** Carry the bill to the table — the same press as the tile's corner. */
  onPrintBill: (order: TableView) => void;
}) {
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
    <ul className="mb-processing">
      {orders.map((order) => (
        <li className="mb-processing__item" key={order.id}>
          <button
            type="button"
            className={cx(
              'mb-processing__order',
              order.state === 'waiting' && 'mb-processing__order--waiting',
              order.state === 'late' && 'mb-processing__order--late',
              order.selected && 'mb-processing__order--on',
            )}
            aria-pressed={order.selected}
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
          <button
            type="button"
            className="mb-processing__print"
            onClick={() => onPrintBill(order)}
            title={`Print the bill for ${order.label}`}
            aria-label={`Print the bill for ${order.label}`}
          >
            <Icon name="printer" size="sm" />
          </button>
        </li>
      ))}
    </ul>
  );
}
