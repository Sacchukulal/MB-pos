/**
 * **The table grid — the only view of open orders** (scope 1.4).
 *
 * There is no list view. The owner dropped it; the grid replaces it
 * (UI_GUIDELINES §1).
 *
 * # Density, decided
 *
 * §4: *"a busy shop has 40+ tables. Tiles must stay readable at that count
 * without scrolling becoming the interaction."*
 *
 * At 1366 × 768 the permanent cart takes 24rem, leaving roughly 900 px for the
 * floor. The solution is two things, and both are tokens:
 *
 * 1. **the grid packs, it does not count.** `auto-fill` with a minimum tile
 *    width means the number of columns follows the room available, so the same
 *    code is right at 1366 and at 1920 — which is also §1's "fluid".
 * 2. **past [`DENSE_ABOVE`] tables it steps down**: a smaller minimum, tighter
 *    gaps, shorter tiles, and **the tile drops its second line** — seats and
 *    the kitchen flag go, the label, the amount and the timer stay. Those three
 *    are what the floor is read for; the rest is what a tile can afford to lose.
 *
 * Twenty-four is the threshold because that is roughly where seven columns of
 * 7.5rem stop fitting two rows on screen at 768 px. T3 asserts the step
 * actually engages.
 *
 * # State is carried in form as well as colour
 *
 * §2 rule 2, and it is not decoration — bright rooms, cheap monitors, tired
 * eyes, colour-blind cashiers. Grey-scale the screen and all four states are
 * still distinguishable: free is *dashed with no fill*, occupied is *solid with
 * a stripe*, late *emphasises the timer*, loaded *has a ring*.
 */

import { useMemo } from 'react';

import { EmptyState } from '../kit';
import type { TableView } from '../ipc/generated/TableView';

/** Past this many tables the grid steps down a density. */
export const DENSE_ABOVE = 24;

export function TableGrid({
  tables,
  filter,
  onOpen,
}: {
  tables: readonly TableView[];
  filter: string;
  onOpen: (table: TableView) => void;
}) {
  const shown = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return tables;
    // Audit F5: "with 20 tables open it becomes a scrolling exercise."
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
      // `null` section is the "No table" group, and it is forced to the end
      // below — §4: "so no order is ever invisible".
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
    return (
      <EmptyState
        title={filter ? 'No table matches that' : 'No tables set up yet'}
        body={
          filter
            ? 'Clear the filter to see the whole floor.'
            : 'Tables are added in Settings. Parcel and self-service orders appear here too.'
        }
      />
    );
  }

  return (
    <div className={dense ? 'mb-floor--dense' : undefined} data-dense={dense}>
      {sections.map(([name, group]) => (
        <div className="mb-floor__section" key={name || 'no-table'}>
          <span className="mb-floor__heading">{name || 'No table'}</span>
          <div className="mb-floor__grid">
            {group.map((table) => (
              <Tile
                key={table.id}
                table={table}
                dense={dense}
                onOpen={() => onOpen(table)}
              />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function Tile({
  table,
  dense,
  onOpen,
}: {
  table: TableView;
  dense: boolean;
  onOpen: () => void;
}) {
  const late = table.state === 'late';
  // Amber and red are two states, not one — see TableState::Waiting. The
  // timer is emphasised for both, because a table somebody should look at and
  // a table somebody must look at are both read off the timer.
  const overdue = late || table.state === 'waiting';
  return (
    <button
      type="button"
      className={`mb-tile mb-tile--${table.state}`}
      onClick={onOpen}
      aria-label={describe(table)}
    >
      <span className="mb-tile__label">{table.label}</span>

      {table.total ? (
        <span className="mb-tile__amount">{table.total.text}</span>
      ) : null}

      {/* The second line is what the dense step drops. The label, the amount
          and the timer are what the floor is actually read for. */}
      {dense ? null : (
        <span className="mb-tile__meta">
          {table.minutes === null ? (
            <span>{table.seats > 0 ? `${table.seats} seats` : ''}</span>
          ) : (
            <span className={overdue ? 'mb-tile__timer--late' : undefined}>
              {formatMinutes(table.minutes)}
            </span>
          )}
          {table.orderId && !table.kitchenTold ? (
            <span className="mb-tile__kitchen" title="The kitchen has not been told">
              ●
            </span>
          ) : null}
        </span>
      )}

      {/* Dense tiles keep the timer, because a late table is the one thing
          worth interrupting somebody for. */}
      {dense && table.minutes !== null ? (
        <span
          className={`mb-tile__meta ${overdue ? 'mb-tile__timer--late' : ''}`.trim()}
        >
          {formatMinutes(table.minutes)}
        </span>
      ) : null}
    </button>
  );
}

/**
 * What a screen reader says, and what the aria-label test checks.
 *
 * A tile is a button with a number on it; without this it announces as "6",
 * which tells a blind cashier nothing about whether the table is busy.
 */
function describe(table: TableView): string {
  const parts = [`Table ${table.label}`];
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
    case 'loaded':
      parts.push('open in the cart');
      break;
  }
  if (table.total) parts.push(table.total.text);
  if (table.minutes !== null) parts.push(formatMinutes(table.minutes));
  return parts.join(', ');
}

/**
 * "12m", "1h 05m". Not a duration library — this is the only place in the
 * product that formats one, and it is nine lines.
 */
function formatMinutes(minutes: number): string {
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return `${hours}h ${String(rest).padStart(2, '0')}m`;
}
