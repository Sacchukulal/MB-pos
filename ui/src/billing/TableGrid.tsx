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
 * a stripe*, waiting and late *emphasise the timer*.
 *
 * # Being selected is drawn ON TOP of the state, not instead of it
 *
 * "Which table am I on" used to be a fifth state, `loaded`. It could not be
 * one: a state field holds a single fact, so selecting a late table turned the
 * late signal off — and an **empty** table could not be selected at all,
 * because that state was decided by matching the cart's ORDER and a table with
 * nothing typed on it has no order yet.
 *
 * The owner found the second one on 2026-08-22: *"selected table is not
 * highlighted. user should know which table he selected right?"*
 *
 * It is `table.selected` now — a separate flag from Rust, drawn as a ring
 * around whatever the tile already is. See `TableView::selected`.
 */

import { useMemo } from 'react';

import { EmptyState, Icon } from '../kit';
import type { TableView } from '../ipc/generated/TableView';

/* **The tile brings its own styling.** `Tile` is imported by the Floor screen
   as well as by this grid, and a component whose appearance depends on which
   OTHER screen happened to be loaded first is the same class of bug as the
   duplicate this replaced. The bundler dedupes it. */
import './billing.css';

/** Past this many tables the grid steps down a density. */
export const DENSE_ABOVE = 24;

export function TableGrid({
  tables,
  filter,
  onOpen,
  onPrintBill,
}: {
  tables: readonly TableView[];
  filter: string;
  onOpen: (table: TableView) => void;
  /**
   * **Carry the bill to this table** — the owner's ask of 2026-08-17.
   *
   * Only ever called for a tile that has an order on it, because the button
   * only exists on those. It settles nothing; see `flows::print_open_bill_on`.
   */
  onPrintBill: (table: TableView) => void;
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
    /*
      **Nothing to show and nothing to say — so nothing** (P30.5).

      This used to answer an empty floor with a card in the middle of the
      counter: "No tables set up yet · Tables are added in Settings." Two
      things are wrong with that. A tea stall, a bakery and a parcel counter
      have no tables and never will, so it is permanent furniture explaining a
      feature they do not want; and the one screen a cashier lives on is the
      worst place in the product to spend half a pane on it.

      Nothing is hidden: an open parcel or self-service order arrives here as a
      "No table" entry (§4, "so no order is ever invisible"), so the moment
      there is something to see the grid comes back on its own. The Floor
      screen is where a shop that DOES want tables is told how to add them,
      because that is a screen somebody opened on purpose.

      A filter that matches nothing is the opposite case and keeps its answer:
      there IS something, and the reason it is not on screen is the filter.
    */
    if (!filter) return null;
    return (
      <EmptyState
        title="No table matches that"
        body="Clear the filter to see the whole floor."
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
                onPrintBill={() => onPrintBill(table)}
              />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

/**
 * **THE table tile. There is one, and this is it.**
 *
 * # Why it is exported
 *
 * The owner, 2026-08-17: *"in floor page the table icons differently showing.
 * As already i told you from starting to till, dont hardcode any styling
 * themes, that must be global theme follow… that is the very very strict
 * instruction forever."*
 *
 * They were looking at the result of **a second copy of this component** living
 * in `Floor.tsx`. It drew the same `mb-tile` classes with different markup and
 * a different meta line, so the two screens had never quite matched — and when
 * this one was restructured into a box-plus-face on 2026-08-17, the copy kept
 * the old shape, lost its padding, and the Floor screen's tiles collapsed into
 * overlapping text. That is what a duplicate costs: not that it looks
 * different, but that fixing one silently breaks the other.
 *
 * So `Floor.tsx` imports this. **A tile is drawn in exactly one place in this
 * product**, and a change to it reaches both screens or neither.
 *
 * # A tile is a box with two things you can press, so it is not a button
 *
 * It was one — a `<button>` with the label inside it. The owner asked for a
 * print mark *"inside"* the tile, and a button inside a button is not markup a
 * browser will render: the inner one is dropped, and what you get is a print
 * icon that opens the table. So the tile is a positioned box holding a face
 * (which fills it, and is the press that opens the table) and, when there is an
 * order, a small print button in its corner.
 *
 * The face is still the whole tile to a mouse and to a finger. Nothing about
 * pressing a table changed; there is simply somewhere else to press as well.
 */
export function Tile({
  table,
  dense = false,
  onOpen,
  onPrintBill,
  picked,
  onEdit,
  onDelete,
}: {
  table: TableView;
  /** The floor's dense step past `DENSE_ABOVE` tables. Off unless asked for. */
  dense?: boolean;
  onOpen: () => void;
  /**
   * Carry the bill to this table.
   *
   * **Both screens pass it**, and that was the owner's point on 2026-08-17:
   * *"the users keeps open that floor page for billing also, so print button
   * inside table cards also wil appeare here."* A waiter working off the Floor
   * screen wants the same press a cashier has.
   *
   * Optional only so that a caller with genuinely nowhere to print from gets a
   * tile without a dead button — a mark that does nothing is worse than no
   * mark. There is no such caller today.
   */
  onPrintBill?: () => void;
  /**
   * **Ticked for a bulk action on the Floor screen** — a different fact from
   * `table.selected`, and a separate prop on purpose.
   *
   * `table.selected` means *"your billing cart is on this table"*. This means
   * *"I have ticked this one to hide or delete"*. They are two questions, they
   * belong to two screens, and putting them in one field is the mistake that
   * cost round 6 (`TableState::Loaded`, which hid `Late`). Never merge them.
   */
  picked?: boolean;
  /** The pencil, on hover. Only where the caller can actually edit. */
  onEdit?: () => void;
  /** The bin, on hover. Rust still decides whether it may go. */
  onDelete?: () => void;
}) {
  const late = table.state === 'late';
  // Amber and red are two states, not one — see TableState::Waiting. The
  // timer is emphasised for both, because a table somebody should look at and
  // a table somebody must look at are both read off the timer.
  const overdue = late || table.state === 'waiting';
  // **Two classes, never one.** The state says what the table is doing; the
  // modifier says the cashier is standing on it. See the note above this
  // component for what conflating them cost.
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
      <button
        type="button"
        className="mb-tile__face"
        onClick={onOpen}
        aria-label={describe(table, picked)}
        // Only where pressing it means "tick this", which is the Floor screen.
        // On the billing grid a press opens the table and this would be a
        // toggle that never toggles.
        aria-pressed={picked === undefined ? undefined : picked}
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
            {/* **Scope 14.2's second timer** — food went to the kitchen and
                nothing has since, which is the number that catches a
                forgotten table. It came from the Floor screen's own copy of
                this tile; it belongs to every tile, so it lives here now. */}
            {table.kitchenMinutes === null ? null : (
              <span className="mb-tile__food">
                food {formatMinutes(table.kitchenMinutes)}
              </span>
            )}
            {table.orderId && !table.kitchenTold ? (
              <span
                className="mb-tile__kitchen"
                title="The kitchen has not been told"
                aria-label="The kitchen has not been told"
              />
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

      {/* **The corner, and only what this caller actually offers.**

          Each of these is a real capability rather than decoration, which is
          why they are all optional props: the billing grid passes only the
          printer, so nothing about that screen changed when the Floor screen
          grew a pencil and a bin.

          A free table shows no printer at all — a print button on an empty
          table is a button whose only possible outcome is an error message,
          and forty of them on the floor is forty invitations to get one. */}
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

      {/* **On hover, and on keyboard focus.** The owner, 2026-08-22: *"just add
          small edit symbol and delete symbol, icon on top of the table squares
          when hovered."* CSS reveals them; they are in the markup the whole
          time so a keyboard reaches them and a screen reader lists them. */}
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

/**
 * What a screen reader says, and what the aria-label test checks.
 *
 * A tile is a button with a number on it; without this it announces as "6",
 * which tells a blind cashier nothing about whether the table is busy.
 */
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
  // **Said as well as the state, not instead of it** — the same reason the ring
  // is drawn on top of the colour rather than replacing it. A blind cashier
  // needs to hear that a table is late AND that it is the one they are on.
  if (table.selected) parts.push('open in the cart');
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
