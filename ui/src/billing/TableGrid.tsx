/** The table grid — the only view of open orders. */

import { useMemo, type ReactNode } from 'react';

import { cx, EmptyState, Icon, Scroller } from '../kit';
import type { TableView } from '../ipc/generated/TableView';

/* The tile brings its own styling. */
import './billing.css';

/** How many person colours the theme has — `--person-1` … `--person-8` in tokens.css. */
export const PERSON_COLOURS = 8;

/**
 * Which of the eight colours a person wears. The phone does the same sum over the same id
 * (`personSlot` in MB-android's Palette.kt), so one waiter is one colour everywhere.
 */
export function personSlot(id: string): number {
  let sum = 0;
  for (let i = 0; i < id.length; i += 1) sum += id.charCodeAt(i);
  return sum % PERSON_COLOURS;
}

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
    <div>
      {sections.map(([name, group]) => (
        <div className="mb-floor__section" key={name || 'no-table'}>
          <span className="mb-floor__heading">{name || 'No table'}</span>
          <Scroller className="mb-floor__grid">
            {group.map((table) => (
              <Tile
                key={table.id}
                table={table}
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

/**
 * THE table tile. There is one, and this is it — the billing grid, the floor plan and the
 * phone's Orders screen all draw the same card:
 *
 *   number ……………… print
 *   amount ……………… who opened it        (a free table: "4 seats")
 *   timer · chips ……… seats
 *
 * A busy table wears its PERSON's colour on the border, so a room reads at a glance whose
 * tables are whose. Late and waiting stay in the timer: bold red, or amber — a state is a
 * form as well as a colour, and the person's colour is not overwritten by it. Every floor,
 * however big, draws the tile at the one size.
 */
export function Tile({
  table,
  onOpen,
  onPrintBill,
  onSplit,
  picked,
  onTick,
  onEdit,
  onDelete,
}: {
  table: TableView;
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
  const waiting = table.state === 'waiting';
  const busy = table.state !== 'free';
  // Two classes, never one.
  const classes = [
    'mb-tile',
    `mb-tile--${table.state}`,
    busy ? 'mb-tile--busy' : '',
    busy && table.byId ? `mb-tile--person-${personSlot(table.byId)}` : '',
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

        {/* The second row: the money and whose it is, or how big the table is. */}
        {busy ? (
          <span className="mb-tile__row">
            {table.total ? <span className="mb-tile__amount">{table.total.text}</span> : null}
            {table.by ? (
              <span className="mb-tile__by" title={`Opened by ${table.by}`}>
                {table.by}
              </span>
            ) : null}
          </span>
        ) : (
          <span className="mb-tile__seatsline">
            {table.seats > 0 ? `${table.seats} seats` : ''}
          </span>
        )}

        {/* The third row: the timers and the chips, and the seats at the far end. */}
        <span className="mb-tile__meta">
            {table.minutes === null ? null : (
              <span
                className={cx(
                  'mb-tile__timer',
                  late && 'mb-tile__timer--late',
                  waiting && 'mb-tile__timer--waiting',
                )}
              >
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
            {/* A waiter asked, from a phone, for this table to be settled: the desk has it. */}
            {table.settleAsked ? (
              <span className="mb-tile__settle" title="A phone asked for this bill to be settled">
                <Icon name="cash" size="sm" />
                Settle
              </span>
            ) : table.billAsked ? (
              /* A waiter asked for this table's bill from a phone; it printed. */
              <span className="mb-tile__bill" title="The bill was asked for from a phone">
                <Icon name="file" size="sm" />
                Bill
              </span>
            ) : null}
            {table.seats > 0 ? (
              <span className="mb-tile__seats" title={`${table.seats} seats`}>
                <Icon name="users" size="sm" />
                {table.seats}
              </span>
            ) : null}
        </span>
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

/** The dashed card after the last table in a room: the way in to adding one. */
export function AddTile({ onAdd }: { onAdd: () => void }) {
  return (
    <button
      type="button"
      className="mb-tile-add"
      onClick={onAdd}
      aria-label="Add table"
      title="Add a table to this room"
    >
      <Icon name="plus" size="md" />
      <span>Add table</span>
    </button>
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
  if (table.by) parts.push(`opened by ${table.by}`);
  if (table.total) parts.push(table.total.text);
  if (table.minutes !== null) parts.push(formatMinutes(table.minutes));
  if (table.settleAsked) parts.push('asked to be settled');
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
