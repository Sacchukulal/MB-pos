/**
 * The floor — scope 14.1 the plan, 14.2 the timers, 14.3 the occupancy line, and the three
 * operations (1.21, 1.22, 1.23).
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
  plural,
  Scroller,
  SectionHeader,
  Select,
  SideFold,
  Toolbar,
  useReport,
  useToast,
} from '../kit';
import { call } from '../ipc/call';
/* The one table tile in the product. */
import { Tile } from '../billing/TableGrid';
import type { FloorView } from '../ipc/generated/FloorView';
import type { SectionView } from '../ipc/generated/SectionView';
import type { TableRowView } from '../ipc/generated/TableRowView';
import type { TableView } from '../ipc/generated/TableView';

import './floor.css';

/** What the floor is being asked to show. */
type Filter = 'all' | 'busy' | 'attention';

export function Floor() {
  const [floor, setFloor] = useState<FloorView | null>(null);
  const [section, setSection] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>('all');
  const [editing, setEditing] = useState<TableRowView | null>(null);
  const [moving, setMoving] = useState<TableView | null>(null);
  /** The ticked tables, by id. */
  const [picked, setPicked] = useState<readonly string[]>([]);
  const [confirming, setConfirming] = useState<'delete' | 'hide' | null>(null);
  /** Is the arranging panel unfolded? */
  const [arrangeOpen, setArrangeOpen] = useState<boolean | null>(null);
  /** The one table the bin on a tile is about — see the note on `Tile`. */
  const [deletingOne, setDeletingOne] = useState<TableRowView | null>(null);
  const toast = useToast();

  // One reporter for the whole product, obeying the tone the engine set — so "the kitchen
  // already has this" is not shown in the colour of a real fault.
  const report = useReport();

  /** One place the floor comes back from Rust, and the one place a stale tick is dropped. */
  const arrived = useCallback((fresh: FloorView) => {
    setFloor(fresh);
    setPicked((was) => {
      const kept = was.filter((id) => fresh.tables.some((t) => t.id === id));
      // The SAME array back when nothing was dropped, so React can bail out of the update.
      return kept.length === was.length ? was : kept;
    });
  }, []);

  const load = useCallback(() => {
    call('floor_plan').then(arrived).catch(report);
  }, [arrived, report]);

  useEffect(load, [load]);

  /** Carry the bill to this table, from the Floor screen too. */
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
          // The two things worth walking over for: a table that has been waiting, and food the
          // kitchen has not heard about.
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

  /** A tile you can tick is a tile that IS a table. */
  const isATable = (id: string) => floor.tables.some((t) => t.id === id);
  const pickable = floor.canArrange;
  /** Only what is on screen and is a table — the bar acts on what you can see. */
  const ticked = picked.filter((id) => shown.some((t) => t.id === id) && isATable(id));
  /** The room is drawn room by room — see `roomsOf`. */
  const rooms = roomsOf(floor.sections, shown);

  /** What a tile can do, written once for the plan and for every room's grid. */
  const onTile = {
    picked,
    onPress: (tile: TableView) =>
      pickable && isATable(tile.id) ? undefined : setMoving(tile),
    onTick: (tile: TableView) => toggle(tile.id),
    onEdit: (tile: TableView) => setEditing(rowFor(tile.id)),
    onDelete: (tile: TableView) => setDeletingOne(rowFor(tile.id)),
    canTick: (tile: TableView) => pickable && isATable(tile.id),
    onPrintBill: printTheBill,
  };
  /** Open on a shop with no tables, folded once there are some. */
  const arranging = arrangeOpen ?? floor.tables.length === 0;

  /** What the ticked tables are about to have done to them. */
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
            ? `${plural(how, 'table')} deleted.`
            : what === 'hide'
              ? `${plural(how, 'table')} taken off the floor.`
              : `${plural(how, 'table')} put back.`,
        );
      })
      .catch((cause) => {
        setConfirming(null);
        report(cause);
      });
  };

  return (
    <div className="mb-floor">
      {/* Two choices, and they must not look like one choice. */}
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
          </>
        }
      >
        {/* No rooms, no room picker. */}
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

      {/*
        Hidden entirely from somebody who may not arrange the room: a panel of controls that can
        only answer "you do not have permission" is worse than no panel.
      */}
      <SideFold
        label="Rooms and tables"
        open={arranging}
        onOpen={() => setArrangeOpen(true)}
        onFold={() => setArrangeOpen(false)}
        allowed={pickable}
        panel={<Arrange floor={floor} onChanged={arrived} onFailed={report} />}
      >
        {/* Only when there is something ticked. */}
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

        {floor.hasLayout ? (
          <Plan floor={floor} tiles={shown} pickable={pickable} onFailed={report} onChanged={arrived} {...onTile} />
        ) : shown.length === 0 ? (
          // Nothing to draw, so nothing to group — `Grid` owns both of the empty things this
          // screen can be.
          <Grid
            tiles={shown}
            {...onTile}
            none={floor.tables.length === 0}
            canArrange={floor.canArrange}
          />
        ) : (
          <Scroller className="mb-floor__rooms">
            {rooms.map((room) => {
              const mine = room.tiles.filter((t) => isATable(t.id));
              const on = mine.filter((t) => picked.includes(t.id));
              const named = room.name === '' ? '' : ` in ${room.name}`;
              return (
                <section key={room.name} className="mb-floor__roomgroup">
                  <div className="mb-floor__roomhead">
                    {/*
                      The box first, then the room's name — the name is the label, so the box
                      needs no words of its own.
                    */}
                    {pickable && mine.length > 0 ? (
                      <Checkbox
                        aria-label={
                          on.length === mine.length
                            ? `All ${mine.length}${named} ticked`
                            : `Tick all ${mine.length}${named}`
                        }
                        checked={on.length === mine.length}
                        onChange={(event) =>
                          setPicked((was) => {
                            // Only this room's ticks change; another room's stay, so the bar
                            // can act on both at once.
                            const others = was.filter(
                              (id) => !mine.some((t) => t.id === id),
                            );
                            return event.target.checked
                              ? [...others, ...mine.map((t) => t.id)]
                              : others;
                          })
                        }
                      />
                    ) : null}
                    {/* A shop with no rooms at all is not told its tables are in "No room". */}
                    {rooms.length > 1 || room.name !== '' ? (
                      <h3 className="mb-floor__roomname">
                        {room.name === '' ? 'No room' : room.name}
                      </h3>
                    ) : null}
                  </div>
                  <Grid tiles={room.tiles} {...onTile} canArrange={floor.canArrange} />
                </section>
              );
            })}
          </Scroller>
        )}
      </SideFold>

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

      {/* The bin on a tile. */}
      {deletingOne ? (
        <ConfirmDialog
          open
          title={`Delete ${deletingOne.printed}?`}
          body={
            Number(deletingOne.history) > 0
              ? `This table has ${plural(Number(deletingOne.history), 'order')} against it, so it cannot be deleted — take it off the floor instead and it keeps its history.`
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
              ? `Delete ${plural(ticked.length, 'table')}?`
              : `Take ${plural(ticked.length, 'table')} off the floor?`
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

/** The tiles, split by room, in the shop's own room order. */
function roomsOf(
  sections: readonly SectionView[],
  tiles: readonly TableView[],
): { name: string; tiles: TableView[] }[] {
  // `tile.section` is a name taken from this same list (see `floor_view`), so every tile lands
  // in exactly one of these buckets.
  return [...sections.map((s) => s.name), '']
    .map((name) => ({ name, tiles: tiles.filter((t) => (t.section ?? '') === name) }))
    .filter((room) => room.tiles.length > 0);
}

/** The room, as three things you can add to it — sections, tables, timers. */
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
    <Scroller inset className="mb-arrange">
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
    </Scroller>
  );
}

/** What you can do to the tables you ticked. */
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

      {/* The order operations. */}
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
  onTick,
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
  onTick: (tile: TableView) => void;
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
    // Two different emptinesses.
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
          onOpen={canTick(tile) ? undefined : () => onPress(tile)}
          onTick={canTick(tile) ? () => onTick(tile) : undefined}
          onEdit={canTick(tile) ? () => onEdit(tile) : undefined}
          onDelete={canTick(tile) ? () => onDelete(tile) : undefined}
          onPrintBill={() => onPrintBill(tile)}
        />
      ))}
    </div>
  );
}

/** The plan — a grid of squares, not free pixels. */
function Plan({
  floor,
  tiles,
  picked,
  pickable,
  onFailed,
  onChanged,
  onPress,
  onTick,
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
  onTick: (tile: TableView) => void;
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
                onOpen={canTick(tile) ? undefined : () => onPress(tile)}
                onTick={canTick(tile) ? () => onTick(tile) : undefined}
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
      {/*
        `style={undefined}` was here — the fossil of an inline style somebody removed without
        removing the prop.
      */}
      <div className="mb-plan" data-grid={floor.grid}>
        {squares}
      </div>
      {placed.size === 0 || !pickable ? null : (
        <p className="mb-floor__hint">Drag a table onto an empty square to move it.</p>
      )}
    </>
  );
}

/** What you can do to the order on a table. */
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
                  // The NAME, never the id.
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

/** One table's own details. */
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
