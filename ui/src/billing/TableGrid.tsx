/** The table grid — the only view of open orders. */

import { useMemo, type ReactNode } from 'react';

import { cx, EmptyState, Icon, Scroller } from '../kit';
import type { TableView } from '../ipc/generated/TableView';

/* The tile brings its own styling. */
import './billing.css';

/** Past this many tables the grid steps down a density. */
export const DENSE_ABOVE = 24;

export function TableGrid({
  tables,
  filter,
  onOpen,
  onPrintBill,
  onSplit,
}: {
  tables: readonly TableView[];
  filter: string;
  onOpen: (table: TableView) => void;
  /** Carry the bill to this table. */
  onPrintBill: (table: TableView) => void;
  /** A second party beside a busy table — the + on its tile. */
  onSplit?: (table: TableView) => void;
}) {
  const shown = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return tables;
    // "with 20 tables open it becomes a scrolling exercise.".
    return tables.filter(
      (t) =>
        t.label.toLowerCase().includes(needle) ||
        (t.section ?? '').toLowerCase().includes(needle),
    );
  }, [tables, filter]);

  const dense = shown.length > DENSE_ABOVE;

  const sections = useMemo(() => {
    const groups = new Map<string, TableView[]>();
    for (const table of shown) {
      // `null` section is the "No table" group, and it is forced to the end below — §4: "so no
      // order is ever invisible".
      const key = table.section ?? '';
      const found = groups.get(key);
      if (found) found.push(table);
      else groups.set(key, [table]);
    }
    return [...groups.entries()].sort(([a], [b]) => {
      if (a === '') return 1;
      if (b === '') return -1;
      return a.localeCompare(b);
    });
  }, [shown]);

  if (shown.length === 0) {
    /* Nothing to show and nothing to say — so nothing. */
    if (!filter) return null;
    return (
      <EmptyState
        title="No table matches that"
        hint="Clear the filter to see the whole floor."
      />
    );
  }

  return (
    <div className={dense ? 'mb-floor--dense' : undefined} data-dense={dense}>
      {sections.map(([name, group]) => (
        <div className="mb-floor__section" key={name || 'no-table'}>
          <span className="mb-floor__heading">{name || 'No table'}</span>
          <Scroller className="mb-floor__grid">
            {group.map((table) => (
              <Tile
                key={table.id}
                table={table}
                dense={dense}
                onOpen={() => onOpen(table)}
                onPrintBill={() => onPrintBill(table)}
                onSplit={onSplit ? () => onSplit(table) : undefined}
              />
            ))}
          </Scroller>
        </div>
      ))}
    </div>
  );
}

/** THE table tile. There is one, and this is it. */
export function Tile({
  table,
  dense = false,
  onOpen,
  onPrintBill,
  onSplit,
  picked,
  onTick,
  onEdit,
  onDelete,
}: {
  table: TableView;
  /** The floor's dense step past `DENSE_ABOVE` tables. */
  dense?: boolean;
  /** What pressing the tile does. */
  onOpen?: () => void;
  /** Carry the bill to this table. */
  onPrintBill?: () => void;
  /** A second party beside this table, with the next free letter. */
  onSplit?: () => void;
  /**
   * Ticked for a bulk action on the Floor screen — a different fact from `table.selected`, and
   * a separate prop on purpose.
   */
  picked?: boolean;
  /** Tick or untick. The circle top-left; without it there is no circle. */
  onTick?: () => void;
  /** The pencil, on hover. */
  onEdit?: () => void;
  /** The bin, on hover. */
  onDelete?: () => void;
}) {
  const late = table.state === 'late';
  // Amber and red are two states, not one — see TableState::Waiting.
  const overdue = late || table.state === 'waiting';
  // Two classes, never one.
  const classes = [
    'mb-tile',
    `mb-tile--${table.state}`,
    table.selected ? 'mb-tile--selected' : '',
    picked ? 'mb-tile--picked' : '',
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <div className={classes}>
      <Face onOpen={onOpen} label={describe(table, picked)}>
        {/* Table numbers are short; "Parcel" and "Self service" are not. */}
        <span
          className={cx('mb-tile__label', table.label.length > 3 && 'mb-tile__label--long')}
        >
          {table.label}
        </span>

        {table.total ? (
          <span className="mb-tile__amount">{table.total.text}</span>
        ) : null}

        {/* The second line is what the dense step drops. */}
        {dense ? null : (
          <span className="mb-tile__meta">
            {table.minutes === null ? (
              <span>{table.seats > 0 ? `${table.seats} seats` : ''}</span>
            ) : (
              <span className={overdue ? 'mb-tile__timer--late' : undefined}>
                {formatMinutes(table.minutes)}
              </span>
            )}
            {/*
              2's second timer — food went to the kitchen and nothing has since, which is the
              number that catches a forgotten table.
            */}
            {table.kitchenMinutes === null ? null : (
              <span
                className="mb-tile__food"
                title="Since the kitchen was last told"
                aria-label={`Kitchen ${formatMinutes(table.kitchenMinutes)}`}
              >
                <Icon name="flame" size="sm" />
                {formatMinutes(table.kitchenMinutes)}
              </span>
            )}
            {table.orderId && !table.kitchenTold ? (
              <span
                className="mb-tile__kitchen"
                title="The kitchen has not been told"
                aria-label="The kitchen has not been told"
              />
            ) : null}
            {/* A waiter asked for this table's bill from a phone; it printed. */}
            {table.billAsked ? (
              <span className="mb-tile__bill" title="The bill was asked for from a phone">
                <Icon name="file" size="sm" />
                Bill
              </span>
            ) : null}
          </span>
        )}

        {/*
          Dense tiles keep the timer, because a late table is the one thing worth interrupting
          somebody for.
        */}
        {dense && table.minutes !== null ? (
          <span
            className={`mb-tile__meta ${overdue ? 'mb-tile__timer--late' : ''}`.trim()}
          >
            {formatMinutes(table.minutes)}
          </span>
        ) : null}
      </Face>

      {/* The tick, top left: an empty circle on hover, filled once ticked. */}
      {onTick ? (
        <button
          type="button"
          className="mb-tile__tick"
          onClick={onTick}
          role="checkbox"
          aria-checked={picked === true}
          title={`Tick table ${table.label}`}
          aria-label={`Tick table ${table.label}`}
        />
      ) : null}

      {/* The corner, and only what this caller actually offers. */}
      {table.orderId && onPrintBill ? (
        <button
          type="button"
          className="mb-tile__print"
          onClick={onPrintBill}
          title={`Print the bill for table ${table.label}`}
          aria-label={`Print the bill for table ${table.label}`}
        >
          <Icon name="printer" size="sm" />
        </button>
      ) : null}

      {/*
        The + : another party on the same table. Only on the table's own tile — a second
        party's tile is its order, not the table — and only once the table is busy.
      */}
      {onSplit && table.orderId && table.id !== table.orderId ? (
        <button
          type="button"
          className="mb-tile__split"
          onClick={onSplit}
          title={`Another party on table ${table.label}`}
          aria-label={`Another party on table ${table.label}`}
        >
          <Icon name="plus" size="sm" />
        </button>
      ) : null}

      {/* On hover, and on keyboard focus. */}
      {onEdit || onDelete ? (
        <div className="mb-tile__tools">
          {onEdit ? (
            <button
              type="button"
              className="mb-tile__tool"
              onClick={onEdit}
              title={`Edit table ${table.label}`}
              aria-label={`Edit table ${table.label}`}
            >
              <Icon name="pencil" size="sm" />
            </button>
          ) : null}
          {onDelete ? (
            <button
              type="button"
              className="mb-tile__tool mb-tile__tool--danger"
              onClick={onDelete}
              title={`Delete table ${table.label}`}
              aria-label={`Delete table ${table.label}`}
            >
              <Icon name="trash" size="sm" />
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

/** The tile's own surface. */
function Face({
  onOpen,
  label,
  children,
}: {
  onOpen?: () => void;
  label: string;
  children: ReactNode;
}) {
  if (!onOpen) return <div className="mb-tile__face">{children}</div>;
  return (
    <button type="button" className="mb-tile__face" onClick={onOpen} aria-label={label}>
      {children}
    </button>
  );
}

/** What a screen reader says, and what the aria-label test checks. */
function describe(table: TableView, picked?: boolean): string {
  const parts = [`Table ${table.label}`];
  // First, because it is the thing that changes when you press.
  if (picked) parts.push('ticked');
  if (table.section) parts.push(table.section);
  switch (table.state) {
    case 'free':
      parts.push('free');
      break;
    case 'occupied':
      parts.push('busy');
      break;
    case 'waiting':
      parts.push('busy, waiting a while');
      break;
    case 'late':
      parts.push('busy, waiting a long time');
      break;
  }
  // Said as well as the state, not instead of it — the same reason the ring is drawn on top of
  // the colour rather than replacing it.
  if (table.selected) parts.push('open in the cart');
  if (table.total) parts.push(table.total.text);
  if (table.minutes !== null) parts.push(formatMinutes(table.minutes));
  return parts.join(', ');
}

/**
 * "12m", "1h 05m". Not a duration library — this is the only place in the product that formats
 * one, and it is nine lines.
 */
export function formatMinutes(minutes: number): string {
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return `${hours}h ${String(rest).padStart(2, '0')}m`;
}
