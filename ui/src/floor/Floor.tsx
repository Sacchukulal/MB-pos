/**
 * **The floor** — scope 14.1 the plan, 14.2 the timers, 14.3 the occupancy
 * line, and the three operations (1.21, 1.22, 1.23).
 *
 * The billing screen's grid stays exactly where it is and keeps working. This
 * is the room: an owner's own layout, the two timers that say which table
 * needs somebody, and the master behind both.
 *
 * # Why this is its own rail item rather than a mode of the billing grid
 *
 * Because it answers a different question. Billing asks *"which table am I
 * putting this dosa on"*; the floor asks *"which table needs me"*. Audit F5 is
 * the second one going unanswered — *"no search or filter on the Processing
 * orders list; with 20 tables open it becomes a scrolling exercise"* — and a
 * mode toggle on a screen a cashier is mid-bill on would make it a question
 * they have to close a bill to ask.
 *
 * # Nothing here decides anything
 *
 * The tile states arrive decided (Rust compared the minutes to the shop's own
 * thresholds), the occupancy line arrives as sentences, and a dragged tile is
 * a square reported to Rust which accepts or refuses it. R8, and the drag is
 * the interesting case: following the mouse is not a business rule; deciding
 * whether two tables may share a square is.
 */

import { useCallback, useEffect, useMemo, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  EmptyState,
  Icon,
  Input,
  Modal,
  Select,
  Table,
  Toolbar,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
/* **The one table tile in the product.** This screen used to have a second
   copy — see the note where it was. */
import { Tile } from '../billing/TableGrid';
import type { FloorView } from '../ipc/generated/FloorView';
import type { TableRowView } from '../ipc/generated/TableRowView';
import type { TableView } from '../ipc/generated/TableView';

import './floor.css';

/** What the floor is being asked to show. Audit F5. */
type Filter = 'all' | 'busy' | 'attention';

export function Floor() {
  const [floor, setFloor] = useState<FloorView | null>(null);
  const [section, setSection] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>('all');
  const [editing, setEditing] = useState<TableRowView | null>(null);
  const [master, setMaster] = useState(false);
  const [moving, setMoving] = useState<TableView | null>(null);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    call('floor_plan').then(setFloor).catch(report);
  }, [report]);

  /**
   * **Carry the bill to this table, from the Floor screen too.**
   *
   * The owner, 2026-08-17: *"the users are keeps open that floor page for
   * billing also, so print button inside table cards also wil appeare here."*
   *
   * The same command the billing grid's tiles call, doing the same nothing to
   * the cart: it reads the order off disk, prints it marked NOT PAID, and
   * leaves the table open. Nothing about this screen's state changes, which is
   * why it does not reload the floor afterwards.
   */
  const printTheBill = useCallback(
    async (tile: TableView) => {
      if (!tile.orderId) return;
      try {
        toast.show('ok', await call('print_open_bill', { orderId: tile.orderId }));
      } catch (cause) {
        report(cause);
      }
    },
    [report, toast],
  );

  useEffect(load, [load]);

  const shown = useMemo(() => {
    if (!floor) return [];
    return floor.tiles.filter((tile) => {
      if (section !== null && tile.section !== section) return false;
      switch (filter) {
        case 'busy':
          return tile.orderId !== null;
        case 'attention':
          // The two things worth walking over for: a table that has been
          // waiting, and food the kitchen has not heard about.
          return (
            tile.state === 'late' ||
            tile.state === 'waiting' ||
            (tile.orderId !== null && !tile.kitchenTold)
          );
        default:
          return true;
      }
    });
  }, [floor, section, filter]);

  if (!floor) {
    return <div className="mb-floor" />;
  }

  const sections = ['All', ...floor.sections.map((s) => s.name)];

  return (
    <div className="mb-floor">
      {/*
        **Two choices, and they must not look like one choice.**

        WHICH ROOM you are looking at, and WHICH TABLES within it. Before P27.5
        both were rendered as `Button`s with the selected one filled solid
        accent, side by side on one row — so the screen showed two identical
        highlighted pills, four inches apart, meaning completely different
        things. That is the specific failure UI_GUIDELINES §5 is about, and it
        was on the second-most-used screen in the product.

        Now: the ROOM is a segmented control (one of these, always exactly one,
        like the order type on the billing screen) and the VIEW is a set of
        tabs (a lens over what the segment chose). Different shapes, because
        they are different questions.
      */}
      <Toolbar
        end={
          <>
            <div className="mb-tabs" role="tablist" aria-label="Which tables">
              {(['all', 'busy', 'attention'] as const).map((which) => (
                <button
                  key={which}
                  type="button"
                  role="tab"
                  className="mb-tab"
                  aria-selected={filter === which}
                  onClick={() => setFilter(which)}
                >
                  {which === 'all' ? 'Everything' : which === 'busy' ? 'Busy' : 'Needs attention'}
                </button>
              ))}
            </div>
            <Button small variant="secondary" onClick={() => setMaster(true)}>
              <Icon name="settings" size="sm" />
              Set up the room
            </Button>
          </>
        }
      >
        {/* **No rooms, no room picker** (P30.5). A shop with no sections got a
            segmented control holding the single word "All" — a tall empty box
            in the corner of the screen offering a choice between one thing.
            One section is the same: "All" and "Main hall" are the same set of
            tables under two names. */}
        {floor.sections.length > 1 ? (
          <div className="mb-segment" role="group" aria-label="Which room">
            {sections.map((name) => (
              <button
                key={name}
                type="button"
                className="mb-segment__option"
                aria-pressed={(name === 'All' && section === null) || section === name}
                onClick={() => setSection(name === 'All' ? null : name)}
              >
                {name}
              </button>
            ))}
          </div>
        ) : null}
      </Toolbar>

      {/* Scope 14.3, and it is the line an owner glances at. */}
      <div className="mb-floor__numbers">
        <span>{floor.occupancy.busy}</span>
        <span>{floor.occupancy.covers}</span>
        <span>{floor.occupancy.turns}</span>
        <span>{floor.occupancy.average}</span>
      </div>

      {floor.hasLayout ? (
        <Plan
          floor={floor}
          tiles={shown}
          onFailed={report}
          onChanged={setFloor}
          onMove={setMoving}
          onPrintBill={printTheBill}
        />
      ) : (
        <Grid
          tiles={shown}
          onMove={setMoving}
          onPrintBill={printTheBill}
          none={floor.tables.length === 0}
          onSetUp={() => setMaster(true)}
        />
      )}

      {master ? (
        <RoomSetup
          floor={floor}
          onClose={() => setMaster(false)}
          onChanged={setFloor}
          onEdit={setEditing}
          onFailed={report}
        />
      ) : null}

      {editing ? (
        <EditTable
          row={editing}
          floor={floor}
          onClose={() => setEditing(null)}
          onSaved={(fresh) => {
            setFloor(fresh);
            setEditing(null);
          }}
          onFailed={report}
        />
      ) : null}

      {moving ? (
        <TableActions
          tile={moving}
          floor={floor}
          onClose={() => setMoving(null)}
          onDone={(fresh, said) => {
            setFloor(fresh);
            setMoving(null);
            toast.show('ok', said);
          }}
          onFailed={report}
        />
      ) : null}
    </div>
  );
}

/** The fallback: sections and tiles, exactly as the billing screen draws them. */
function Grid({
  tiles,
  onMove,
  onPrintBill,
  none,
  onSetUp,
}: {
  tiles: readonly TableView[];
  onMove: (tile: TableView) => void;
  onPrintBill: (tile: TableView) => void;
  /** True when the shop has no tables at all, rather than none in this view. */
  none?: boolean;
  onSetUp?: () => void;
}) {
  if (tiles.length === 0) {
    // **Two different emptinesses** (P30.5). "Try another section" is useless
    // advice to a shop that has never added a table, and it was the only thing
    // this screen said on a fresh install.
    return none ? (
      <EmptyState
        title="No tables yet"
        body="Add your rooms and tables and they appear here. A shop that does only parcel and counter sales never needs this."
        action={
          onSetUp ? (
            <Button variant="primary" onClick={onSetUp}>
              Set up the room
            </Button>
          ) : undefined
        }
      />
    ) : (
      <EmptyState
        title="Nothing here"
        body="Try another section, or show everything."
      />
    );
  }
  return (
    <div className="mb-floor__grid">
      {tiles.map((tile) => (
        <Tile
          key={tile.id}
          table={tile}
          onOpen={() => onMove(tile)}
          onPrintBill={() => onPrintBill(tile)}
        />
      ))}
    </div>
  );
}

/**
 * The plan — **a grid of squares, not free pixels.**
 *
 * Snapping is what makes a dragged layout look deliberate instead of drunk,
 * and it makes "two tables in the same place" a comparison of two integers
 * rather than a rectangle intersection. Rust holds both halves of that rule.
 */
function Plan({
  floor,
  tiles,
  onFailed,
  onChanged,
  onMove,
  onPrintBill,
}: {
  floor: FloorView;
  tiles: readonly TableView[];
  onFailed: (cause: unknown) => void;
  onChanged: (floor: FloorView) => void;
  onMove: (tile: TableView) => void;
  onPrintBill: (tile: TableView) => void;
}) {
  const [dragging, setDragging] = useState<string | null>(null);
  const placed = useMemo(
    () => new Map(floor.tables.filter((t) => t.x !== null).map((t) => [t.id, t])),
    [floor.tables],
  );

  const drop = (x: number, y: number) => {
    if (dragging === null) return;
    call('place_dining_table', { tableId: dragging, x, y })
      .then(onChanged)
      .catch(onFailed);
    setDragging(null);
  };

  const squares = [];
  for (let y = 0; y < floor.grid; y += 1) {
    for (let x = 0; x < floor.grid; x += 1) {
      const here = floor.tables.find((t) => t.x === x && t.y === y);
      const tile = here ? tiles.find((t) => t.id === here.id) : undefined;
      squares.push(
        <div
          key={`${x}-${y}`}
          className="mb-plan__cell"
          onDragOver={(event) => event.preventDefault()}
          onDrop={() => drop(x, y)}
        >
          {tile ? (
            <div draggable onDragStart={() => setDragging(tile.id)}>
              <Tile
                table={tile}
                onOpen={() => onMove(tile)}
                onPrintBill={() => onPrintBill(tile)}
              />
            </div>
          ) : null}
        </div>,
      );
    }
  }

  return (
    <>
      {/* `style={undefined}` was here — the fossil of an inline style somebody
          removed without removing the prop. The grid size travels as a data
          attribute and the CSS reads it. */}
      <div className="mb-plan" data-grid={floor.grid}>
        {squares}
      </div>
      {placed.size === 0 ? null : (
        <p className="mb-floor__hint">
          Drag a table onto an empty square to move it. Tables you have not
          placed stay in the section grid.
        </p>
      )}
    </>
  );
}

/*
  **THE FLOOR'S OWN `Tile` USED TO BE HERE, AND IT IS THE BUG.**

  The owner, 2026-08-17: *"in floor page the table icons differently showing.
  As already i told you from starting to till, dont hardcode any styling
  themes, that must be global theme follow… if anything hardcoded, remove
  hardcode immediately, that is the very very strict instruction forever."*

  This file had a second table tile. It reached for the same `mb-tile` classes
  but drew different markup — a `<button>` rather than the box-and-face the
  billing grid uses, its own meta line, its own `mb-floor__kitchen` class for
  the food timer, and no print mark. So the two screens had never actually
  matched, and when the billing tile was restructured on 2026-08-17 this copy
  kept the old shape, lost the padding that now lives on `.mb-tile__face`, and
  every busy table on this screen collapsed into overlapping text.

  **That is the real cost of a duplicate**: not that the two look different,
  but that fixing one silently breaks the other, and nothing fails to tell you.

  There is one tile now — `billing/TableGrid.tsx` — and both screens import it.
  The food timer went with it, because it belongs to a tile rather than to a
  screen.
*/

/** What you can do to the order on a table — scope 1.21, 1.22, 1.23. */
function TableActions({
  tile,
  floor,
  onClose,
  onDone,
  onFailed,
}: {
  tile: TableView;
  floor: FloorView;
  onClose: () => void;
  onDone: (floor: FloorView, said: string) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [target, setTarget] = useState('');
  const busy = floor.tiles.filter((t) => t.orderId !== null && t.id !== tile.id);
  const free = floor.tables.filter(
    (t) => t.isActive && !t.isBusy && t.id !== tile.id,
  );

  if (tile.orderId === null) {
    return (
      <Modal open title={`Table ${tile.label}`} onClose={onClose}>
        <p>This table is free. Open it from the billing screen to start an order.</p>
        <div className="mb-row mb-row--end">
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal open title={`Table ${tile.label}`} onClose={onClose} wide>
      <h3 className="mb-floor__heading">Move this order</h3>
      <p className="mb-floor__note">
        The party changed seats. Nothing else changes — same bill number, same
        food, and the kitchen is not told again.
      </p>
      <div className="mb-comp__choice">
        <Select
          label="To table"
          value={target}
          onChange={(event) => setTarget(event.target.value)}
          options={[
            { value: '', label: 'Pick a free table' },
            ...free.map((t) => ({ value: t.id, label: t.printed })),
          ]}
        />
        <Button
          disabled={target === ''}
          onClick={() => {
            call('move_order', { orderId: tile.orderId ?? '', toTable: target })
              .then((fresh) =>
                onDone(
                  fresh,
                  // The NAME, never the id. A toast reading "moved to tbl_1"
                  // is audit F8 — a shopkeeper reading our key — and it was
                  // found the only way it could be: by moving a table and
                  // looking at the screen (P14).
                  `The order moved to ${
                    free.find((t) => t.id === target)?.printed ?? 'the new table'
                  }.`,
                ),
              )
              .catch(onFailed);
          }}
        >
          Move
        </Button>
      </div>

      <h3 className="mb-floor__heading">Merge into another table</h3>
      <p className="mb-floor__note">
        Both parties pay together. This table&rsquo;s food joins the other
        bill; its own order is kept and marked as merged, never deleted.
      </p>
      {busy.length === 0 ? (
        <EmptyState title="Nothing to merge with" body="No other table has an order on it." />
      ) : (
        <div className="mb-floor__mergelist">
          {busy.map((other) => (
            <Button
              key={other.id}
              small
              variant="quiet"
              onClick={() => {
                call('merge_orders', {
                  fromOrder: tile.orderId ?? '',
                  intoOrder: other.orderId ?? '',
                })
                  .then((fresh) =>
                    onDone(fresh, `Table ${tile.label} joined table ${other.label}.`),
                  )
                  .catch(onFailed);
              }}
            >
              Into table {other.label}
              {other.total ? ` (${other.total.text})` : ''}
            </Button>
          ))}
        </div>
      )}

      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Close
        </Button>
      </div>
    </Modal>
  );
}

/** The master: sections, tables, a numbered range, and the two thresholds. */
function RoomSetup({
  floor,
  onClose,
  onChanged,
  onEdit,
  onFailed,
}: {
  floor: FloorView;
  onClose: () => void;
  onChanged: (floor: FloorView) => void;
  onEdit: (row: TableRowView) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [sectionName, setSectionName] = useState('');
  const [rangeSection, setRangeSection] = useState('');
  const [from, setFrom] = useState('1');
  const [to, setTo] = useState('10');
  const [prefix, setPrefix] = useState('');
  const [seats, setSeats] = useState('4');
  const [warn, setWarn] = useState(String(floor.warnMinutes));
  const [late, setLate] = useState(String(floor.lateMinutes));

  const columns: Column<TableRowView>[] = [
    { key: 'printed', header: 'Prints as', render: (r) => r.printed },
    {
      key: 'section',
      header: 'Section',
      render: (r) =>
        floor.sections.find((s) => s.id === r.sectionId)?.name ?? 'No section',
    },
    { key: 'seats', header: 'Seats', numeric: true, render: (r) => String(r.seats) },
    {
      key: 'where',
      header: 'On the plan',
      render: (r) => (r.x === null ? 'Not placed' : `${r.x + 1}, ${(r.y ?? 0) + 1}`),
    },
    {
      key: 'state',
      header: '',
      render: (r) =>
        r.isBusy ? (
          <Badge tone="warn">In use</Badge>
        ) : r.isActive ? (
          <Badge tone="ok">On the floor</Badge>
        ) : (
          <Badge tone="neutral">Hidden</Badge>
        ),
    },
    {
      key: 'do',
      header: '',
      render: (r) => (
        <div className="mb-row">
          <Button small onClick={() => onEdit(r)}>
            Edit
          </Button>
          <Button
            small
            variant="quiet"
            onClick={() => {
              call('set_dining_table_active', { tableId: r.id, active: !r.isActive })
                .then(onChanged)
                .catch(onFailed);
            }}
          >
            {r.isActive ? 'Hide' : 'Put back'}
          </Button>
        </div>
      ),
    },
  ];

  return (
    <Modal open title="Set up the room" onClose={onClose} wide>
      <h3 className="mb-floor__heading">Sections</h3>
      <div className="mb-comp__choice">
        <Input
          label="Add a section"
          hint="AC, Garden, Rooftop."
          value={sectionName}
          onChange={(event) => setSectionName(event.target.value)}
        />
        <Button
          onClick={() => {
            call('save_floor_section', {
              id: `sec_${Date.now().toString(36)}`,
              name: sectionName,
              sortOrder: floor.sections.length,
              isActive: true,
            })
              .then((fresh) => {
                onChanged(fresh);
                setSectionName('');
              })
              .catch(onFailed);
          }}
        >
          Add
        </Button>
      </div>
      <ul className="mb-comp__list">
        {floor.sections.map((s) => (
          <li key={s.id} className="mb-comp__row">
            <span>{s.name}</span>
            <span className="mb-comp__rule">{Number(s.tableCount)} table(s)</span>
            <Button
              small
              variant="quiet"
              onClick={() => {
                call('delete_floor_section', { id: s.id }).then(onChanged).catch(onFailed);
              }}
            >
              Remove
            </Button>
          </li>
        ))}
      </ul>

      <h3 className="mb-floor__heading">Add a run of tables</h3>
      <p className="mb-floor__note">
        A room of twenty is twenty forms otherwise. If any name in the range is
        already taken, none of them is created.
      </p>
      <div className="mb-comp__choice">
        <Select
          label="Section"
          value={rangeSection}
          onChange={(event) => setRangeSection(event.target.value)}
          options={[
            { value: '', label: 'No section' },
            ...floor.sections.map((s) => ({ value: s.id, label: s.name })),
          ]}
        />
        <Input label="Before the number" value={prefix} onChange={(e) => setPrefix(e.target.value)} />
        <Input label="From" value={from} onChange={(e) => setFrom(e.target.value)} />
        <Input label="To" value={to} onChange={(e) => setTo(e.target.value)} />
        <Input label="Seats" value={seats} onChange={(e) => setSeats(e.target.value)} />
        <Button
          onClick={() => {
            call('add_dining_tables', {
              sectionId: rangeSection === '' ? null : rangeSection,
              prefix,
              from: Number(from),
              to: Number(to),
              seats: Number(seats),
            })
              .then(onChanged)
              .catch(onFailed);
          }}
        >
          Add them
        </Button>
      </div>

      <h3 className="mb-floor__heading">The tables</h3>
      <Table rows={[...floor.tables]} columns={columns} rowKey={(r) => r.id} />

      <h3 className="mb-floor__heading">When a table has been waiting</h3>
      <p className="mb-floor__note">
        A dosa counter turns a table in eight minutes and a dining room takes
        ninety. These are yours, and the floor colours itself by them.
      </p>
      <div className="mb-comp__choice">
        <Input label="Keep an eye after (minutes)" value={warn} onChange={(e) => setWarn(e.target.value)} />
        <Input label="Late after (minutes)" value={late} onChange={(e) => setLate(e.target.value)} />
        <Button
          onClick={() => {
            call('save_floor_thresholds', { warn: Number(warn), late: Number(late) })
              .then(onChanged)
              .catch(onFailed);
          }}
        >
          Save
        </Button>
      </div>

      <div className="mb-row mb-row--end">
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </div>
    </Modal>
  );
}

function EditTable({
  row,
  floor,
  onClose,
  onSaved,
  onFailed,
}: {
  row: TableRowView;
  floor: FloorView;
  onClose: () => void;
  onSaved: (floor: FloorView) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [label, setLabel] = useState(row.label);
  const [sectionId, setSectionId] = useState(row.sectionId ?? '');
  const [seats, setSeats] = useState(String(row.seats));
  const [active, setActive] = useState(row.isActive);

  return (
    <Modal open title={row.printed} onClose={onClose}>
      <Input label="Name" value={label} autoFocus onChange={(e) => setLabel(e.target.value)} />
      <Select
        label="Section"
        hint="The section is part of what the table prints as — AC 1."
        value={sectionId}
        onChange={(e) => setSectionId(e.target.value)}
        options={[
          { value: '', label: 'No section' },
          ...floor.sections.map((s) => ({ value: s.id, label: s.name })),
        ]}
      />
      <Input label="Seats" value={seats} onChange={(e) => setSeats(e.target.value)} />
      <Checkbox
        label="On the floor"
        checked={active}
        onChange={(e) => setActive(e.target.checked)}
      />
      {Number(row.history) > 0 ? (
        <p className="mb-floor__note">
          This table has {Number(row.history)} order(s) against it, so it can be
          hidden but never deleted — hiding takes it off the floor and keeps its
          history.
        </p>
      ) : null}
      <div className="mb-row mb-row--end">
        {Number(row.history) === 0 && !row.isBusy ? (
          <Button
            variant="danger"
            onClick={() => {
              call('delete_dining_table', { tableId: row.id })
                .then(onSaved)
                .catch(onFailed);
            }}
          >
            Delete
          </Button>
        ) : null}
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="primary"
          onClick={() => {
            call('save_dining_table', {
              edit: {
                id: row.id,
                label,
                sectionId: sectionId === '' ? null : sectionId,
                seats: Number(seats),
                isActive: active,
              },
            })
              .then(onSaved)
              .catch(onFailed);
          }}
        >
          Save
        </Button>
      </div>
    </Modal>
  );
}
