/**
 * **The floor** — scope 14.1 the plan, 14.2 the timers, 14.3 the occupancy
 * line, and the three operations (1.21, 1.22, 1.23).
 *
 * The billing screen's grid stays exactly where it is and keeps working. This
 * is the room: an owner's own layout, the two timers that say which table
 * needs somebody, and the arranging of both.
 *
 * # Why this is its own rail item rather than a mode of the billing grid
 *
 * Because it answers a different question. Billing asks *"which table am I
 * putting this dosa on"*; the floor asks *"which table needs me"*, and — since
 * 2026-08-22 — *"what is my room made of"*. A mode toggle on a screen a cashier
 * is mid-bill on would make those questions ones they have to close a bill to
 * ask.
 *
 * # The room is arranged HERE, not in a dialog
 *
 * The owner, 2026-08-22: *"No need for popup for setup room. Redesign the Floor
 * section page to have a adding tables section in one side (at the starting side
 * of the screen)… no need to show table list as it will already be visible in
 * the screen in proper square format. just add small edit symbol and delete
 * symbol, icon on top of the table squares when hovered. make the tables
 * selectable… and then i should be able to delete them."*
 *
 * So: a panel down the left holds the three things that MAKE a room — sections,
 * a run of tables, and the two timers — and the room itself fills the rest of
 * the screen. **The squares are the table list.** The old dialog carried a
 * seven-column table of every table beside a grid drawing the same tables,
 * which is the same information twice and the reason the dialog needed its own
 * scrollbar.
 *
 * What each square can do lives on the square: hover for the pencil and the
 * bin, press to tick it, and the bar above the room acts on everything ticked.
 *
 * # Ticked is not selected
 *
 * `table.selected` means *"the billing cart is on this table"* and is **always
 * false here** — this screen has no cart (owner, same day: *"why is the table i
 * selected in the billing section is highlighted in floor section also? it makes
 * no sense"*). `picked` means *"I have ticked this one"*. Two facts, two props,
 * two marks. See `TableView::selected` and `Room::cart_is_on`.
 *
 * # Nothing here decides anything
 *
 * The tile states arrive decided (Rust compared the minutes to the shop's own
 * thresholds), the occupancy line arrives as sentences, whether this person may
 * arrange the room arrives as `canArrange`, and a dragged tile is a square
 * reported to Rust which accepts or refuses it. R8, and the drag is the
 * interesting case: following the mouse is not a business rule; deciding whether
 * two tables may share a square is.
 */

import { useCallback, useEffect, useMemo, useState } from 'react';

import {
  Button,
  Checkbox,
  ConfirmDialog,
  EmptyState,
  freshId,
  Icon,
  Input,
  Modal,
  SectionHeader,
  Select,
  Toolbar,
  useToast,
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
  const [moving, setMoving] = useState<TableView | null>(null);
  /**
   * **The ticked tables, by id.**
   *
   * Ids and not rows, so a floor that reloads under a selection does not leave
   * stale copies of rows behind — everything is read back out of `floor` when
   * it is needed. Ids that no longer exist are dropped on every reload.
   */
  const [picked, setPicked] = useState<readonly string[]>([]);
  const [confirming, setConfirming] = useState<'delete' | 'hide' | null>(null);
  /** The one table the bin on a tile is about — see the note on `Tile`. */
  const [deletingOne, setDeletingOne] = useState<TableRowView | null>(null);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  /**
   * **One place the floor comes back from Rust**, and the one place a stale
   * tick is dropped.
   *
   * Every command on this screen returns the whole `FloorView`, so a table
   * deleted by one of them is gone from `fresh.tables` — and a tick still
   * pointing at it would be a bulk action that fails on a row nobody can see.
   */
  const arrived = useCallback((fresh: FloorView) => {
    setFloor(fresh);
    setPicked((was) => {
      const kept = was.filter((id) => fresh.tables.some((t) => t.id === id));
      // **The SAME array back when nothing was dropped**, so React can bail out
      // of the update. `filter` always builds a new one, and a new one is a
      // render — every reload of the floor would have forced one whether or not
      // a tick had actually gone. Found by a test hanging: the reload effect
      // re-runs on a render, so a render it caused is a reload it causes.
      return kept.length === was.length ? was : kept;
    });
  }, []);

  const load = useCallback(() => {
    call('floor_plan').then(arrived).catch(report);
  }, [arrived, report]);

  useEffect(load, [load]);

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

  const toggle = useCallback((id: string) => {
    setPicked((was) => (was.includes(id) ? was.filter((x) => x !== id) : [...was, id]));
  }, []);

  const rowFor = useCallback(
    (tileId: string) => floor?.tables.find((t) => t.id === tileId) ?? null,
    [floor],
  );

  if (!floor) {
    return <div className="mb-floor" />;
  }

  const sections = ['All', ...floor.sections.map((s) => s.name)];

  /**
   * **A tile you can tick is a tile that IS a table.**
   *
   * The floor also shows parcel and self-service orders — §4's *"so no order is
   * ever invisible"* — and those are tiles with an order and no table behind
   * them. There is nothing to hide or delete, so there is nothing to tick.
   *
   * Getting this wrong made the bar lie: it counted `floor.tables` rows while
   * the ticks came from `floor.tiles`, so ticking four tiles on a floor with
   * two tables and two parcel orders read *"2 ticked"*. Found by driving it.
   */
  const isATable = (id: string) => floor.tables.some((t) => t.id === id);
  const pickable = floor.canArrange;
  /** Only what is on screen and is a table — the bar acts on what you can see. */
  const ticked = picked.filter((id) => shown.some((t) => t.id === id) && isATable(id));
  const tickable = shown.filter((t) => isATable(t.id));

  /**
   * **What the ticked tables are about to have done to them.**
   *
   * Each of these is ONE command over the whole set — see
   * `floor::delete_tables_on`. A loop here would be a room that can end up half
   * changed, and a message that has to explain which half.
   */
  const act = (what: 'delete' | 'hide' | 'show') => {
    const how = ticked.length;
    const sent =
      what === 'delete'
        ? call('delete_dining_tables', { tableIds: ticked })
        : call('set_dining_tables_active', { tableIds: ticked, active: what === 'show' });

    sent
      .then((fresh) => {
        arrived(fresh);
        setPicked([]);
        setConfirming(null);
        toast.show(
          'ok',
          what === 'delete'
            ? `${how} table(s) deleted.`
            : what === 'hide'
              ? `${how} table(s) taken off the floor.`
              : `${how} table(s) put back.`,
        );
      })
      .catch((cause) => {
        setConfirming(null);
        report(cause);
      });
  };

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

        The "Set up the room" button that used to sit at the end is gone with
        the dialog it opened.
      */}
      <Toolbar
        end={
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

      <div className="mb-floor__body">
        {/* **The starting side of the screen** (owner, 2026-08-22). Hidden
            entirely from somebody who may not arrange the room — a panel of
            controls that can only answer "you do not have permission" is worse
            than no panel. `guard::require` is still the control; see
            `FloorView::can_arrange`. */}
        {pickable ? (
          <Arrange floor={floor} onChanged={arrived} onFailed={report} />
        ) : null}

        <div className="mb-floor__room">
          {/* Only when there is something ticked. A bar that is always there is
              furniture; a bar that appears is an answer to what you just did. */}
          {pickable && ticked.length > 0 ? (
            <Picked
              floor={floor}
              ticked={ticked}
              onEdit={setEditing}
              onOrder={setMoving}
              onClear={() => setPicked([])}
              onDelete={() => setConfirming('delete')}
              onHide={() => setConfirming('hide')}
              onShow={() => act('show')}
              onPrint={printTheBill}
            />
          ) : null}

          {pickable && tickable.length > 0 ? (
            <div className="mb-floor__pickall">
              <Checkbox
                label={
                  ticked.length === tickable.length
                    ? `All ${tickable.length} ticked`
                    : `Tick all ${tickable.length}`
                }
                checked={ticked.length === tickable.length && tickable.length > 0}
                onChange={(event) =>
                  setPicked(event.target.checked ? tickable.map((t) => t.id) : [])
                }
              />
            </div>
          ) : null}

          {floor.hasLayout ? (
            <Plan
              floor={floor}
              tiles={shown}
              picked={picked}
              pickable={pickable}
              onFailed={report}
              onChanged={arrived}
              onPress={(tile) =>
                pickable && isATable(tile.id) ? toggle(tile.id) : setMoving(tile)
              }
              onEdit={(tile) => setEditing(rowFor(tile.id))}
              onDelete={(tile) => setDeletingOne(rowFor(tile.id))}
              canTick={(tile) => pickable && isATable(tile.id)}
              onPrintBill={printTheBill}
            />
          ) : (
            <Grid
              tiles={shown}
              picked={picked}
              onPress={(tile) =>
                pickable && isATable(tile.id) ? toggle(tile.id) : setMoving(tile)
              }
              onEdit={(tile) => setEditing(rowFor(tile.id))}
              onDelete={(tile) => setDeletingOne(rowFor(tile.id))}
              canTick={(tile) => pickable && isATable(tile.id)}
              onPrintBill={printTheBill}
              none={floor.tables.length === 0}
              canArrange={floor.canArrange}
            />
          )}
        </div>
      </div>

      {editing ? (
        <EditTable
          row={editing}
          floor={floor}
          onClose={() => setEditing(null)}
          onSaved={(fresh) => {
            arrived(fresh);
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
            arrived(fresh);
            setMoving(null);
            setPicked([]);
            toast.show('ok', said);
          }}
          onFailed={report}
        />
      ) : null}

      {/* **The bin on a tile.** One table, named, and still confirmed — Rust
          refuses a table with history, and this says so before the press
          rather than after it. */}
      {deletingOne ? (
        <ConfirmDialog
          open
          title={`Delete ${deletingOne.printed}?`}
          body={
            Number(deletingOne.history) > 0
              ? `This table has ${Number(deletingOne.history)} order(s) against it, so it cannot be deleted — take it off the floor instead and it keeps its history.`
              : 'It has no orders against it, so it can go for good.'
          }
          confirmLabel={Number(deletingOne.history) > 0 ? 'Take it off the floor' : 'Delete it'}
          destructive
          onCancel={() => setDeletingOne(null)}
          onConfirm={() => {
            const one = deletingOne;
            const sent =
              Number(one.history) > 0
                ? call('set_dining_tables_active', { tableIds: [one.id], active: false })
                : call('delete_dining_tables', { tableIds: [one.id] });
            sent
              .then((fresh) => {
                arrived(fresh);
                setDeletingOne(null);
                toast.show(
                  'ok',
                  Number(one.history) > 0
                    ? `${one.printed} is off the floor.`
                    : `${one.printed} deleted.`,
                );
              })
              .catch((cause) => {
                setDeletingOne(null);
                report(cause);
              });
          }}
        />
      ) : null}

      {confirming ? (
        <ConfirmDialog
          open
          title={
            confirming === 'delete'
              ? `Delete ${ticked.length} table(s)?`
              : `Take ${ticked.length} table(s) off the floor?`
          }
          body={
            confirming === 'delete'
              ? 'A table that has ever had an order on it cannot be deleted. If one of these has, none of them is deleted and nothing changes.'
              : 'They stay in the shop and keep their history. Put them back at any time.'
          }
          confirmLabel={confirming === 'delete' ? 'Delete them' : 'Take them off'}
          destructive={confirming === 'delete'}
          onCancel={() => setConfirming(null)}
          onConfirm={() => act(confirming)}
        />
      ) : null}
    </div>
  );
}

/**
 * **The room, as three things you can add to it** — sections, tables, timers.
 *
 * This was a modal called *Set up the room* holding all of this plus a
 * seven-column table of every table in the shop. The owner asked for the dialog
 * to go and for the list to go with it: the squares to the right of this panel
 * ARE the list, and a room drawn twice is a room that can disagree with itself.
 *
 * Everything the dialog could do is still here. Hiding and deleting moved to
 * the tiles and the ticked-bar, which is where you can see what you are acting
 * on — and they act on a set, so the one-table-at-a-time `set_dining_table_active`
 * was left with no caller and deleted. `audit-wiring.mjs` is what said so.
 */
function Arrange({
  floor,
  onChanged,
  onFailed,
}: {
  floor: FloorView;
  onChanged: (floor: FloorView) => void;
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

  const addSection = () => {
    if (sectionName.trim() === '') return;
    call('save_floor_section', {
      id: freshId('sec'),
      name: sectionName.trim(),
      sortOrder: floor.sections.length,
      isActive: true,
    })
      .then((fresh) => {
        onChanged(fresh);
        setSectionName('');
      })
      .catch(onFailed);
  };

  return (
    <aside className="mb-arrange" aria-label="Arrange the room">
      <SectionHeader
        title="Rooms"
        note="A room is what a table prints under — AC 1, Garden 4. A shop with one dining area does not need any."
      />
      <div className="mb-arrange__add">
        <Input
          label="Room name"
          value={sectionName}
          placeholder="AC"
          onChange={(event) => setSectionName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') addSection();
          }}
        />
        <Button onClick={addSection}>Add</Button>
      </div>
      {floor.sections.length > 0 ? (
        <ul className="mb-arrange__rooms">
          {floor.sections.map((s) => (
            <li key={s.id} className="mb-arrange__room">
              <span className="mb-arrange__roomname">{s.name}</span>
              <span className="mb-arrange__count">{Number(s.tableCount)}</span>
              <button
                type="button"
                className="mb-arrange__drop"
                title={`Delete the room ${s.name}`}
                aria-label={`Delete the room ${s.name}`}
                onClick={() => {
                  call('delete_floor_section', { id: s.id }).then(onChanged).catch(onFailed);
                }}
              >
                <Icon name="trash" size="sm" />
              </button>
            </li>
          ))}
        </ul>
      ) : null}

      <SectionHeader
        title="Tables"
        note="Added as a run, because a room of twenty is twenty forms otherwise. If any name in the range is already taken, none of them is created."
      />
      <div className="mb-arrange__fields">
        <Select
          label="Room"
          value={rangeSection}
          onChange={(event) => setRangeSection(event.target.value)}
          options={[
            { value: '', label: 'No room' },
            ...floor.sections.map((s) => ({ value: s.id, label: s.name })),
          ]}
        />
        <Input label="Before the number" value={prefix} onChange={(e) => setPrefix(e.target.value)} />
        <div className="mb-arrange__pair">
          <Input label="From" value={from} onChange={(e) => setFrom(e.target.value)} />
          <Input label="To" value={to} onChange={(e) => setTo(e.target.value)} />
        </div>
        <Input label="Seats" value={seats} onChange={(e) => setSeats(e.target.value)} />
        <Button
          variant="primary"
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

      <SectionHeader
        title="Timers"
        note="A dosa counter turns a table in eight minutes and a dining room takes ninety. These are yours, and the floor colours itself by them."
      />
      <div className="mb-arrange__fields">
        <Input label="Keep an eye after (min)" value={warn} onChange={(e) => setWarn(e.target.value)} />
        <Input label="Late after (min)" value={late} onChange={(e) => setLate(e.target.value)} />
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
    </aside>
  );
}

/**
 * **What you can do to the tables you ticked.**
 *
 * One bar rather than a menu per tile, because everything here is about a SET —
 * and because the number in it is the confirmation that you ticked what you
 * meant to. With exactly one table ticked it also carries what used to need a
 * click on the tile: editing it, and the order operations.
 */
function Picked({
  floor,
  ticked,
  onEdit,
  onOrder,
  onClear,
  onDelete,
  onHide,
  onShow,
  onPrint,
}: {
  floor: FloorView;
  ticked: readonly string[];
  onEdit: (row: TableRowView) => void;
  onOrder: (tile: TableView) => void;
  onClear: () => void;
  onDelete: () => void;
  onHide: () => void;
  onShow: () => void;
  onPrint: (tile: TableView) => void;
}) {
  const rows = floor.tables.filter((t) => ticked.includes(t.id));
  const one = rows.length === 1 ? rows[0] : null;
  const tile = one ? floor.tiles.find((t) => t.id === one.id) : undefined;
  const hidden = rows.filter((r) => !r.isActive).length;

  return (
    <div className="mb-picked" role="group" aria-label="What to do with the ticked tables">
      <span className="mb-picked__count">
        {rows.length} ticked
        {hidden > 0 ? ` · ${hidden} off the floor` : ''}
      </span>

      {one ? (
        <Button small onClick={() => onEdit(one)}>
          <Icon name="pencil" size="sm" />
          Edit
        </Button>
      ) : null}

      {/* The order operations — scope 1.21 and 1.22. They belong to one table
          with one order on it, so they appear exactly then. */}
      {tile && tile.orderId ? (
        <>
          <Button small onClick={() => onPrint(tile)}>
            <Icon name="printer" size="sm" />
            Print the bill
          </Button>
          <Button small onClick={() => onOrder(tile)}>
            Move or merge
          </Button>
        </>
      ) : null}

      {hidden < rows.length ? (
        <Button small onClick={onHide}>
          Take off the floor
        </Button>
      ) : null}
      {hidden > 0 ? (
        <Button small onClick={onShow}>
          Put back
        </Button>
      ) : null}

      <Button small variant="danger" onClick={onDelete}>
        <Icon name="trash" size="sm" />
        Delete
      </Button>

      <Button small variant="quiet" onClick={onClear}>
        Clear
      </Button>
    </div>
  );
}

/** The fallback: tiles, exactly as the billing screen draws them. */
function Grid({
  tiles,
  picked,
  onPress,
  onEdit,
  onDelete,
  canTick,
  onPrintBill,
  none,
  canArrange,
}: {
  tiles: readonly TableView[];
  picked: readonly string[];
  onPress: (tile: TableView) => void;
  onEdit: (tile: TableView) => void;
  onDelete: (tile: TableView) => void;
  /** A parcel order is a tile with no table behind it — nothing to tick. */
  canTick: (tile: TableView) => boolean;
  onPrintBill: (tile: TableView) => void;
  /** True when the shop has no tables at all, rather than none in this view. */
  none?: boolean;
  canArrange: boolean;
}) {
  if (tiles.length === 0) {
    // **Two different emptinesses** (P30.5). "Try another section" is useless
    // advice to a shop that has never added a table, and it was the only thing
    // this screen said on a fresh install. The advice changed with the layout:
    // the panel that adds a table is now on this screen, not behind a button.
    return none ? (
      <EmptyState
        title="No tables yet"
        body={
          canArrange
            ? 'Add a room and a run of tables on the left, and they appear here.'
            : 'Somebody who manages tables can add them.'
        }
      />
    ) : (
      <EmptyState title="Nothing here" body="Try another room, or show everything." />
    );
  }
  return (
    <div className="mb-floor__grid">
      {tiles.map((tile) => (
        <Tile
          key={tile.id}
          table={tile}
          picked={canTick(tile) ? picked.includes(tile.id) : undefined}
          onOpen={() => onPress(tile)}
          onEdit={canTick(tile) ? () => onEdit(tile) : undefined}
          onDelete={canTick(tile) ? () => onDelete(tile) : undefined}
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
  picked,
  pickable,
  onFailed,
  onChanged,
  onPress,
  onEdit,
  onDelete,
  canTick,
  onPrintBill,
}: {
  floor: FloorView;
  tiles: readonly TableView[];
  picked: readonly string[];
  pickable: boolean;
  onFailed: (cause: unknown) => void;
  onChanged: (floor: FloorView) => void;
  onPress: (tile: TableView) => void;
  onEdit: (tile: TableView) => void;
  onDelete: (tile: TableView) => void;
  canTick: (tile: TableView) => boolean;
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
                picked={canTick(tile) ? picked.includes(tile.id) : undefined}
                onOpen={() => onPress(tile)}
                onEdit={canTick(tile) ? () => onEdit(tile) : undefined}
                onDelete={canTick(tile) ? () => onDelete(tile) : undefined}
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
      {placed.size === 0 || !pickable ? null : (
        <p className="mb-floor__hint">Drag a table onto an empty square to move it.</p>
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
  screen. The pencil and the bin were added there for the same reason.
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
      <SectionHeader
        title="Move this order"
        note="The party changed seats. Nothing else changes — same bill number, same food, and the kitchen is not told again."
      />
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

      <SectionHeader
        title="Merge into another table"
        note="Both parties pay together. This table's food joins the other bill; its own order is kept and marked as merged, never deleted."
      />
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

/**
 * One table's own details.
 *
 * Still a dialog, and deliberately: this is a form about a single thing, opened
 * on purpose, with a Save. The dialog the owner asked to remove was the one
 * that held the whole room behind a button.
 */
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
        label="Room"
        hint="Part of what the table prints as — AC 1."
        value={sectionId}
        onChange={(e) => setSectionId(e.target.value)}
        options={[
          { value: '', label: 'No room' },
          ...floor.sections.map((s) => ({ value: s.id, label: s.name })),
        ]}
      />
      <Input label="Seats" value={seats} onChange={(e) => setSeats(e.target.value)} />
      <Checkbox
        label="On the floor"
        checked={active}
        onChange={(e) => setActive(e.target.checked)}
      />
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
