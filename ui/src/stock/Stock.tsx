/** The stock book. */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  Checkbox,
  EmptyState,
  freshId,
  Icon,
  Input,
  Locked,
  Modal,
  MoneyInput,
  Page,
  PageHeader,
  SearchField,
  Select,
  Table,
  Tabs,
  Toolbar,
  useToast,
  type Column,
} from '../kit';
import { Count } from '../buying/Count';
import { call, isLicenceRefusal, isUiError } from '../ipc/call';
import type { BuyGroupView } from '../ipc/generated/BuyGroupView';
import type { DishCostView } from '../ipc/generated/DishCostView';
import type { InventoryView } from '../ipc/generated/InventoryView';
import type { VarianceView } from '../ipc/generated/VarianceView';
import type { MaterialView } from '../ipc/generated/MaterialView';
import type { StockMovementView } from '../ipc/generated/StockMovementView';
import type { ProblemView } from '../ipc/generated/ProblemView';
import type { RecipeLineView } from '../ipc/generated/RecipeLineView';
import type { RecipeView } from '../ipc/generated/RecipeView';
import type { UnitView } from '../ipc/generated/UnitView';

import './stock.css';

/** What a material is measured in. */
const DIMENSIONS = [
  { value: 'weight', label: 'Weight — grams and kilos' },
  { value: 'volume', label: 'Volume — millilitres and litres' },
  { value: 'count', label: 'Count — pieces' },
];

/** The movements a person types. */
const KINDS = [
  { value: 'purchase', label: 'Bought' },
  { value: 'opening', label: 'Opening stock' },
  { value: 'wastage', label: 'Wasted' },
  { value: 'production_in', label: 'Made a batch' },
  { value: 'adjustment', label: 'Adjustment' },
];

type Editing = { material: MaterialView | null; isNew: boolean };

export function Stock({ onGoTo }: { onGoTo?: (screen: string) => void }) {
  const [view, setView] = useState<InventoryView | null>(null);
  /** The licence saying no, held rather than flashed. */
  const [locked, setLocked] = useState<string>('');
  const [tab, setTab] = useState('shelf');
  const [search, setSearch] = useState('');
  const [editing, setEditing] = useState<Editing | null>(null);
  const [moving, setMoving] = useState<MaterialView | null>(null);
  const [recipeFor, setRecipeFor] = useState<MaterialView | null>(null);
  const [dishRecipe, setDishRecipe] = useState<DishCostView | null>(null);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      // A refusal is an answer, not a fault — it goes on the screen.
      if (isLicenceRefusal(cause)) {
        setLocked(cause.message);
        return;
      }
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    call('inventory', { material: null }).then(setView).catch(report);
  }, [report]);

  useEffect(load, [load]);

  const shown = useMemo(() => {
    if (!view) return [];
    const needle = search.trim().toLowerCase();
    if (needle === '') return view.materials;
    return view.materials.filter(
      (m) =>
        m.name.toLowerCase().includes(needle) ||
        m.category.toLowerCase().includes(needle) ||
        m.buyFrom.toLowerCase().includes(needle),
    );
  }, [view, search]);

  if (locked) {
    return <Locked says={locked} onOpenAccount={onGoTo ? () => onGoTo('account') : undefined} />;
  }
  if (!view) return <div className="mb-stock" />;

  const columns: Column<MaterialView>[] = [
    {
      key: 'name',
      header: 'Material',
      render: (m) => (
        <div className="mb-stock__name">
          <span>{m.name}</span>
          {m.isMade ? <Badge tone="info">Made here</Badge> : null}
          {!m.isActive ? <Badge tone="neutral">Retired</Badge> : null}
        </div>
      ),
    },
    { key: 'category', header: 'Group', render: (m) => m.category || '—' },
    { key: 'buyFrom', header: 'Buy from', render: (m) => m.buyFrom || '—' },
    {
      key: 'onHand',
      header: 'On the shelf',
      numeric: true,
      render: (m) => (
        <span className={m.isNegative ? 'mb-stock__negative' : 'mb-mono'}>{m.onHand}</span>
      ),
    },
    {
      key: 'low',
      header: '',
      render: (m) =>
        m.isNegative ? (
          <Badge tone="danger">Below zero</Badge>
        ) : m.isLow ? (
          <Badge tone="warn">Buy {m.buy}</Badge>
        ) : null,
    },
    { key: 'cost', header: 'Costs', render: (m) => m.cost || '—' },
    {
      key: 'value',
      header: 'Worth',
      numeric: true,
      render: (m) => <span className="mb-mono">{m.value.text}</span>,
    },
    {
      key: 'do',
      header: '',
      render: (m) => (
        <div className="mb-row">
          <Button small variant="quiet" onClick={() => setMoving(m)}>
            Move
          </Button>
          <Button small variant="quiet" onClick={() => setRecipeFor(m)}>
            Recipe
          </Button>
          <Button small variant="quiet" onClick={() => setEditing({ material: m, isNew: false })}>
            Edit
          </Button>
        </div>
      ),
    },
  ];

  return (
    <Page className="mb-stock">
      <PageHeader
        title="Stock"
        subtitle={view.summary}
        actions={
          view.mayManage ? (
            <Button variant="primary" onClick={() => setEditing({ material: null, isNew: true })}>
              <Icon name="plus" size="sm" />
              Add a material
            </Button>
          ) : undefined
        }
      />

      <Toolbar
        end={
          <div className="mb-stock__worth">
            <span className="mb-muted">Stock worth</span>{' '}
            <span className="mb-mono">{view.totalValue.text}</span>
          </div>
        }
      >
        <div className="mb-stock__find">
          <SearchField
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            what="Find a material"
          />
        </div>
      </Toolbar>

      {/*
        The cache disagreed with the ledger, and the sentence carries the button that fixes it.
      */}
      {view.cacheWarning !== '' ? (
        <div className="mb-stock__drift">
          <span>{view.cacheWarning}</span>
          {view.mayAdjust ? (
            <Button
              small
              onClick={() =>
                call('rebuild_stock_balances')
                  .then((fresh) => {
                    setView(fresh);
                    toast.show('ok', 'The balances were worked out again from the movements.');
                  })
                  .catch(report)
              }
            >
              Rebuild
            </Button>
          ) : null}
        </div>
      ) : null}

      <Tabs
        active={tab}
        onChange={setTab}
        tabs={[
          { id: 'shelf', label: 'On the shelf' },
          { id: 'dishes', label: 'What each dish costs' },
          { id: 'buy', label: 'What to buy' },
          { id: 'moves', label: 'Movements' },
          { id: 'problems', label: `Needs a look${view.problems.length > 0 ? ` (${view.problems.length})` : ''}` },
          { id: 'variance', label: 'What went missing' },
          { id: 'count', label: 'Count' },
        ]}
      />

      {tab === 'count' ? <Count /> : null}

      {tab === 'shelf' ? (
        shown.length === 0 ? (
          <EmptyState
            title="No materials yet"
            body="Add what the kitchen uses — rice, oil, paneer — then say what each dish is made of."
          />
        ) : (
          <>
            <Table rows={shown} columns={columns} rowKey={(m) => m.id} />
            {shown
              .filter((m) => m.warning !== '')
              .map((m) => (
                <div key={`warn-${m.id}`} className="mb-stock__warning">
                  {m.warning}
                </div>
              ))}
          </>
        )
      ) : null}

      {tab === 'dishes' ? <Dishes view={view} onOpen={setDishRecipe} /> : null}
      {tab === 'buy' ? <BuyList view={view} onReport={report} /> : null}
      {tab === 'moves' ? <Movements rows={view.movements} /> : null}
      {tab === 'problems' ? (
        <Problems view={view} onChange={setView} onReport={report} />
      ) : null}
      {tab === 'variance' ? <Variance onReport={report} /> : null}

      {editing ? (
        <MaterialForm
          editing={editing}
          onClose={() => setEditing(null)}
          onSaved={(fresh) => {
            setView(fresh);
            setEditing(null);
          }}
          onReport={report}
        />
      ) : null}

      {moving ? (
        <MoveForm
          material={moving}
          reasons={view.wastageReasons}
          onClose={() => setMoving(null)}
          onSaved={(fresh) => {
            setView(fresh);
            setMoving(null);
          }}
          onReport={report}
        />
      ) : null}

      {recipeFor ? (
        <RecipeEditor
          ownerKind="material"
          ownerId={recipeFor.id}
          materials={view.materials}
          onClose={() => {
            setRecipeFor(null);
            load();
          }}
          onReport={report}
        />
      ) : null}

      {dishRecipe ? (
        <RecipeEditor
          ownerKind="item"
          ownerId={dishRecipe.itemId}
          materials={view.materials}
          onClose={() => {
            setDishRecipe(null);
            load();
          }}
          onReport={report}
        />
      ) : null}
    </Page>
  );
}

/** What each dish costs to make. */
function Dishes({
  view,
  onOpen,
}: {
  view: InventoryView;
  onOpen: (dish: DishCostView) => void;
}) {
  if (view.dishes.length === 0) {
    return <EmptyState title="No dishes yet" body="Add items to the menu first." />;
  }
  const columns: Column<DishCostView>[] = [
    { key: 'name', header: 'Dish', render: (d) => d.name },
    {
      key: 'sells',
      header: 'Sells for',
      numeric: true,
      render: (d) => <span className="mb-mono">{d.sellsFor.text}</span>,
    },
    {
      key: 'cost',
      header: 'The recipe says',
      numeric: true,
      // A dish nobody has costed is not a dish that costs nothing, so the two cases must not
      // look the same.
      render: (d) =>
        d.hasRecipe ? <span className="mb-mono">{d.recipeCost.text}</span> : <span>—</span>,
    },
    {
      key: 'typed',
      header: 'You had it down as',
      numeric: true,
      render: (d) => (d.typedCost ? <span className="mb-mono">{d.typedCost.text}</span> : '—'),
    },
    { key: 'gap', header: '', render: (d) => (d.gap === '' ? null : <Badge tone="warn">{d.gap}</Badge>) },
    { key: 'margin', header: 'Margin', render: (d) => d.margin || '—' },
    {
      key: 'do',
      header: '',
      render: (d) => (
        <div className="mb-row">
          {d.isIncomplete ? <Badge tone="warn">Not fully priced</Badge> : null}
          {view.mayManage ? (
            <Button small variant="quiet" onClick={() => onOpen(d)}>
              {d.hasRecipe ? 'Recipe' : 'Add a recipe'}
            </Button>
          ) : null}
        </div>
      ),
    },
  ];
  return <Table rows={view.dishes} columns={columns} rowKey={(d) => d.itemId} />;
}

/** Grouped by where you buy it, and sendable as text. */
function BuyList({ view, onReport }: { view: InventoryView; onReport: (cause: unknown) => void }) {
  const toast = useToast();
  if (view.buyList.length === 0) {
    return (
      <EmptyState
        title="Nothing to buy"
        body="Set a reorder level on a material and it appears here when it runs low."
      />
    );
  }
  return (
    <div className="mb-stack">
      <div className="mb-row mb-row--end">
        <Button
          small
          variant="quiet"
          onClick={() =>
            call('buy_list_text')
              .then((text) => {
                void navigator.clipboard?.writeText(text);
                toast.show('ok', 'The list was copied. Paste it into a message.');
              })
              .catch(onReport)
          }
        >
          Copy the list
        </Button>
      </div>
      {view.buyList.map((group: BuyGroupView) => (
        <Card key={group.buyFrom}>
          <div className="mb-stack">
            <strong>{group.buyFrom}</strong>
            {group.lines.map((line) => (
              <div key={line.materialId} className="mb-stock__buy">
                <span>{line.material}</span>
                <span className="mb-muted">have {line.have}</span>
                <span className="mb-mono">buy {line.buy}</span>
              </div>
            ))}
          </div>
        </Card>
      ))}
    </div>
  );
}

function Movements({ rows }: { rows: StockMovementView[] }) {
  if (rows.length === 0) {
    return <EmptyState title="Nothing has moved yet" body="Buying, selling and wasting all show up here." />;
  }
  const columns: Column<StockMovementView>[] = [
    { key: 'when', header: 'When', render: (m) => m.when },
    { key: 'material', header: 'Material', render: (m) => m.material },
    {
      key: 'kind',
      header: 'What happened',
      render: (m) => (
        <div className="mb-stock__name">
          <span>{m.kind}</span>
          {/* Never hidden. */}
          {m.wasAutomatic ? <Badge tone="info">Automatic</Badge> : null}
        </div>
      ),
    },
    {
      key: 'qty',
      header: 'How much',
      numeric: true,
      render: (m) => (
        <span className={m.takesOut ? 'mb-stock__negative' : 'mb-mono'}>{m.qty}</span>
      ),
    },
    {
      key: 'value',
      header: 'Worth',
      numeric: true,
      render: (m) => <span className="mb-mono">{m.value.text}</span>,
    },
    { key: 'why', header: 'Why', render: (m) => m.reason || m.note || '—' },
  ];
  return <Table rows={rows} columns={columns} rowKey={(m) => m.id} />;
}

/** What the stock book could not do, which did not stop a sale. */
function Problems({
  view,
  onChange,
  onReport,
}: {
  view: InventoryView;
  onChange: (fresh: InventoryView) => void;
  onReport: (cause: unknown) => void;
}) {
  if (view.problems.length === 0) {
    return (
      <EmptyState
        title="Nothing needs a look"
        body="A bill that could not take something off the shelf is listed here. The bill always goes through."
      />
    );
  }
  return (
    <div className="mb-stack">
      {view.problems.map((problem: ProblemView) => (
        <div key={problem.id} className="mb-stock__problem">
          <div className="mb-stock__problem-text">
            <div>{problem.sentence}</div>
            <div className="mb-muted">
              {problem.times === 1 ? 'once' : `${problem.times} times`}, last {problem.when}
            </div>
          </div>
          {view.mayManage ? (
            <Button
              small
              variant="quiet"
              onClick={() =>
                call('resolve_stock_problem', { id: problem.id }).then(onChange).catch(onReport)
              }
            >
              Done
            </Button>
          ) : null}
        </div>
      ))}
    </div>
  );
}

/** Add or change a material, and the packs the shop buys it in. */
function MaterialForm({
  editing,
  onClose,
  onSaved,
  onReport,
}: {
  editing: Editing;
  onClose: () => void;
  onSaved: (fresh: InventoryView) => void;
  onReport: (cause: unknown) => void;
}) {
  const m = editing.material;
  const [name, setName] = useState(m?.name ?? '');
  const [dimension, setDimension] = useState(m?.dimension ?? 'weight');
  const [category, setCategory] = useState(m?.category ?? '');
  const [buyFrom, setBuyFrom] = useState(m?.buyFrom ?? '');
  const [packs, setPacks] = useState<{ name: string; size: string; unit: string }[]>(
    m ? m.units.filter((u) => !u.isStandard).map((u) => ({ name: u.name, size: u.basePerUnit, unit: baseOf(m.dimension) })) : [],
  );
  const [reorderLevel, setReorderLevel] = useState(numberOf(m?.reorderLevel));
  const [reorderQty, setReorderQty] = useState(numberOf(m?.reorderQty));
  const [reorderUnit, setReorderUnit] = useState(unitOf(m?.reorderLevel) || baseOf(m?.dimension ?? 'weight'));
  const [perishable, setPerishable] = useState(m?.isPerishable ?? false);
  const [shelfLife, setShelfLife] = useState(m?.shelfLifeDays ? String(m.shelfLifeDays) : '');
  const [active, setActive] = useState(m?.isActive ?? true);

  // Every unit the reorder figure can be typed in: the base, the dimension's standard, and
  // whatever packs are on the form right now.
  const units = [
    baseOf(dimension),
    ...standardOf(dimension),
    ...packs.map((p) => p.name).filter((n) => n.trim() !== ''),
  ];

  const save = () => {
    call('save_material', {
      edit: {
        id: m?.id ?? freshId('mat'),
        name,
        dimension,
        category,
        buyFrom,
        reorderLevel,
        reorderQty,
        reorderUnit,
        isPerishable: perishable,
        shelfLifeDays: shelfLife.trim() === '' ? null : Number(shelfLife),
        isActive: active,
        packs: packs.filter((p) => p.name.trim() !== ''),
        purchaseUnit: m?.purchaseUnit ?? '',
        recipeUnit: m?.recipeUnit ?? '',
      },
    })
      .then(onSaved)
      .catch(onReport);
  };

  return (
    <Modal
      open
      onClose={onClose}
      title={editing.isNew ? 'A new material' : name}
      actions={
        <div className="mb-row mb-row--end">
          <Button variant="quiet" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={save}>Save</Button>
        </div>
      }
    >
      <div className="mb-stack">
        <Input label="Name" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        <Select
          label="Measured by"
          value={dimension}
          onChange={(e) => setDimension(e.target.value)}
          options={DIMENSIONS}
          hint={`Kept in ${baseOf(dimension)}. This cannot be changed once stock has moved.`}
          disabled={!editing.isNew}
        />
        <Input label="Group" value={category} onChange={(e) => setCategory(e.target.value)} hint="Vegetables, dry goods, dairy — your own words." />
        <Input
          label="Where you buy it"
          value={buyFrom}
          onChange={(e) => setBuyFrom(e.target.value)}
          hint="The buy list is grouped by this. A place is fine: the vegetable market, the milk van."
        />

        {/* The shop's own packs. */}
        <div className="mb-stack">
          <div className="mb-row">
            <strong>How you buy it</strong>
            <Button
              small
              variant="quiet"
              onClick={() => setPacks([...packs, { name: '', size: '', unit: baseOf(dimension) }])}
            >
              Add a pack
            </Button>
          </div>
          <div className="mb-muted mb-stock__hint">
            A bag, a tin, a tray. {baseOf(dimension)}
            {standardOf(dimension).length > 0 ? ` and ${standardOf(dimension).join(', ')}` : ''} are
            already there.
          </div>
          {packs.map((pack, n) => (
            <div key={n} className="mb-stock__pack">
              <Input
                label="Called"
                value={pack.name}
                onChange={(e) => setPacks(packs.map((p, i) => (i === n ? { ...p, name: e.target.value } : p)))}
              />
              <Input
                label="Holds"
                value={pack.size}
                onChange={(e) => setPacks(packs.map((p, i) => (i === n ? { ...p, size: e.target.value } : p)))}
              />
              <Select
                label="of"
                value={pack.unit}
                onChange={(e) => setPacks(packs.map((p, i) => (i === n ? { ...p, unit: e.target.value } : p)))}
                options={[baseOf(dimension), ...standardOf(dimension)].map((u) => ({ value: u, label: u }))}
              />
              <Button small variant="quiet" onClick={() => setPacks(packs.filter((_, i) => i !== n))}>
                Remove
              </Button>
            </div>
          ))}
        </div>

        <div className="mb-stock__pack">
          <Input label="Buy more when it drops to" value={reorderLevel} onChange={(e) => setReorderLevel(e.target.value)} />
          <Input label="and then buy" value={reorderQty} onChange={(e) => setReorderQty(e.target.value)} />
          <Select
            label="counted in"
            value={reorderUnit}
            onChange={(e) => setReorderUnit(e.target.value)}
            options={units.map((u) => ({ value: u, label: u }))}
          />
        </div>

        <Checkbox label="It goes bad" checked={perishable} onChange={(e) => setPerishable(e.target.checked)} />
        {perishable ? (
          <Input
            label="Keeps for (days)"
            value={shelfLife}
            onChange={(e) => setShelfLife(e.target.value)}
            hint="You will be told when some has been sitting longer than this. Batches are not tracked."
          />
        ) : null}
        {!editing.isNew ? (
          <Checkbox label="Still in use" checked={active} onChange={(e) => setActive(e.target.checked)} />
        ) : null}
      </div>
    </Modal>
  );
}

/** One movement a person types: bought, wasted, adjusted. */
function MoveForm({
  material,
  reasons,
  onClose,
  onSaved,
  onReport,
}: {
  material: MaterialView;
  reasons: { id: string; text: string }[];
  onClose: () => void;
  onSaved: (fresh: InventoryView) => void;
  onReport: (cause: unknown) => void;
}) {
  const [kind, setKind] = useState('purchase');
  const [qty, setQty] = useState('');
  const [unit, setUnit] = useState(material.purchaseUnit);
  const [cost, setCost] = useState('');
  const [reason, setReason] = useState(reasons[0]?.id ?? '');
  const [note, setNote] = useState('');

  const wasting = kind === 'wastage';
  const buying = kind === 'purchase' || kind === 'opening';

  return (
    <Modal
      open
      onClose={onClose}
      title={material.name}
      actions={
        <div className="mb-row mb-row--end">
          <Button variant="quiet" onClick={onClose}>
            Cancel
          </Button>
          <Button
            onClick={() =>
              call('record_stock_movement', {
                edit: {
                  materialId: material.id,
                  kind,
                  qty,
                  unit,
                  reasonId: wasting ? reason : null,
                  note: note.trim() === '' ? null : note,
                  cost: buying && cost.trim() !== '' ? cost : null,
                },
              })
                .then(onSaved)
                .catch(onReport)
            }
          >
            Record it
          </Button>
        </div>
      }
    >
      <div className="mb-stack">
        <div className="mb-muted">On the shelf now: {material.onHand}</div>
        <Select label="What happened" value={kind} onChange={(e) => setKind(e.target.value)} options={KINDS} />
        <div className="mb-stock__pack">
          <Input label="How much" value={qty} onChange={(e) => setQty(e.target.value)} autoFocus />
          <Select
            label="in"
            value={unit}
            onChange={(e) => setUnit(e.target.value)}
            options={material.units.map((u: UnitView) => ({ value: u.name, label: u.name }))}
          />
        </div>
        {buying ? (
          <MoneyInput
            label={`Price for one ${unit}`}
            value={cost}
            onChange={setCost}
            hint="What you paid. The material's cost is the average of what actually came in."
          />
        ) : null}
        {wasting ? (
          <Select
            label="Why"
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            options={reasons.map((r) => ({ value: r.id, label: r.text }))}
          />
        ) : null}
        <Input label="Note" value={note} onChange={(e) => setNote(e.target.value)} />
        {kind === 'adjustment' ? (
          <div className="mb-muted mb-stock__hint">
            Put a minus in front to take stock away. Every adjustment is recorded against your name.
          </div>
        ) : null}
      </div>
    </Modal>
  );
}

/** What a dish is made of, with the cost as you type. */
export function RecipeEditor({
  ownerKind,
  ownerId,
  materials,
  onClose,
  onReport,
}: {
  ownerKind: string;
  ownerId: string;
  materials: MaterialView[];
  onClose: () => void;
  onReport: (cause: unknown) => void;
}) {
  const [recipe, setRecipe] = useState<RecipeView | null>(null);
  const [lines, setLines] = useState<{ materialId: string; qty: string; unit: string; yieldPercent: number }[]>([]);
  const [batchQty, setBatchQty] = useState('1');
  const [batchUnit, setBatchUnit] = useState('');
  const [copyFrom, setCopyFrom] = useState('');
  const toast = useToast();
  const loaded = useRef(false);

  useEffect(() => {
    call('recipe', { ownerKind, ownerId })
      .then((fresh) => {
        setRecipe(fresh);
        if (!loaded.current) {
          loaded.current = true;
          setLines(
            fresh.lines.map((l: RecipeLineView) => ({
              materialId: l.materialId,
              qty: l.qty,
              unit: l.unit,
              yieldPercent: l.yieldPercent,
            })),
          );
          setBatchQty(fresh.batchQty === '' ? '1' : fresh.batchQty);
          setBatchUnit(fresh.batchUnit);
        }
      })
      .catch(onReport);
  }, [ownerKind, ownerId, onReport]);

  const save = () => {
    call('save_recipe', {
      edit: { ownerKind, ownerId, batchQty, batchUnit, lines },
    })
      .then((fresh) => {
        setRecipe(fresh);
        toast.show('ok', 'Saved.');
      })
      .catch(onReport);
  };

  const addLine = () =>
    setLines([...lines, { materialId: '', qty: '', unit: '', yieldPercent: 100 }]);

  const unitsFor = (id: string) => materials.find((m) => m.id === id)?.units ?? [];
  const defaultUnitFor = (id: string) => materials.find((m) => m.id === id)?.recipeUnit ?? '';

  if (!recipe) return null;

  const isMade = ownerKind === 'material';

  return (
    <Modal
      open
      onClose={onClose}
      title={`What ${recipe.owner} is made of`}
      wide
      actions={
        <div className="mb-row mb-row--end">
          {recipe.exists ? (
            <Button
              variant="quiet"
              onClick={() =>
                call('delete_recipe', { ownerKind, ownerId })
                  .then(() => {
                    toast.show('ok', 'The recipe was removed. Selling this will no longer take anything off the shelf.');
                    onClose();
                  })
                  .catch(onReport)
              }
            >
              Remove the recipe
            </Button>
          ) : null}
          <Button variant="quiet" onClick={onClose}>
            Close
          </Button>
          <Button onClick={save}>Save</Button>
        </div>
      }
    >
      <div className="mb-stack">
        {/*
          Copy from an existing recipe: half plate is full plate scaled, and a shop has thirty
          of those.
        */}
        <div className="mb-stock__pack">
          <Select
            label="Copy an existing recipe"
            value={copyFrom}
            onChange={(e) => {
              const id = e.target.value;
              setCopyFrom(id);
              if (id === '') return;
              call('recipe', { ownerKind: 'material', ownerId: id })
                .then((other) => {
                  setLines(
                    other.lines.map((l: RecipeLineView) => ({
                      materialId: l.materialId,
                      qty: l.qty,
                      unit: l.unit,
                      yieldPercent: l.yieldPercent,
                    })),
                  );
                  toast.show('ok', 'Copied. Change the amounts and save.');
                })
                .catch(onReport);
            }}
            options={[
              { value: '', label: '—' },
              ...materials.filter((m) => m.isMade).map((m) => ({ value: m.id, label: m.name })),
            ]}
          />
        </div>

        {isMade ? (
          <div className="mb-stock__pack">
            <Input label="One batch makes" value={batchQty} onChange={(e) => setBatchQty(e.target.value)} />
            <Select
              label="of"
              value={batchUnit}
              onChange={(e) => setBatchUnit(e.target.value)}
              options={unitsFor(ownerId).map((u) => ({ value: u.name, label: u.name }))}
            />
          </div>
        ) : null}

        {lines.map((line, n) => (
          <div key={n} className="mb-stock__line">
            <Select
              label="Material"
              value={line.materialId}
              onChange={(e) =>
                setLines(
                  lines.map((l, i) =>
                    i === n
                      ? { ...l, materialId: e.target.value, unit: l.unit || defaultUnitFor(e.target.value) }
                      : l,
                  ),
                )
              }
              options={[
                { value: '', label: 'Choose' },
                ...materials
                  .filter((m) => m.isActive && m.id !== ownerId)
                  .map((m) => ({ value: m.id, label: m.name })),
              ]}
            />
            <Input
              label="How much"
              value={line.qty}
              onChange={(e) => setLines(lines.map((l, i) => (i === n ? { ...l, qty: e.target.value } : l)))}
            />
            <Select
              label="in"
              value={line.unit}
              onChange={(e) => setLines(lines.map((l, i) => (i === n ? { ...l, unit: e.target.value } : l)))}
              options={unitsFor(line.materialId).map((u) => ({ value: u.name, label: u.name }))}
            />
            <Input
              label="% kept"
              value={String(line.yieldPercent)}
              onChange={(e) =>
                setLines(
                  lines.map((l, i) =>
                    i === n ? { ...l, yieldPercent: Number(e.target.value) || 0 } : l,
                  ),
                )
              }
            />
            <div className="mb-stock__linecost mb-mono">{recipe.lines[n]?.cost.text ?? '—'}</div>
            <Button small variant="quiet" onClick={() => setLines(lines.filter((_, i) => i !== n))}>
              Remove
            </Button>
          </div>
        ))}

        <div className="mb-row">
          <Button small variant="quiet" onClick={addLine}>
            Add a material
          </Button>
        </div>

        {/* The cost, and. */}
        <div className="mb-stock__cost">
          <div>
            <span className="mb-muted">This recipe costs</span>{' '}
            <span className="mb-mono">{recipe.cost.text}</span>
          </div>
          {recipe.typedCost ? (
            <div>
              <span className="mb-muted">You had it down as</span>{' '}
              <span className="mb-mono">{recipe.typedCost.text}</span>
            </div>
          ) : null}
          {recipe.sellsFor ? (
            <div>
              <span className="mb-muted">Sells for</span>{' '}
              <span className="mb-mono">{recipe.sellsFor.text}</span>
            </div>
          ) : null}
          {recipe.margin !== '' ? <Badge tone="ok">{recipe.margin}</Badge> : null}
        </div>

        {recipe.unpriced.map((said) => (
          <div key={said} className="mb-stock__warning">
            {said}
          </div>
        ))}
      </div>
    </Modal>
  );
}

// Small readers. None of these is arithmetic — they split a phrase Rust already wrote ("1 bag"
// → "1", "bag") so a form can show it back in two boxes.

function numberOf(said: string | undefined): string {
  const first = (said ?? '').split(' ')[0] ?? '';
  return first === '0' ? '' : first;
}

function unitOf(said: string | undefined): string {
  return (said ?? '').split(' ')[1] ?? '';
}

function baseOf(dimension: string): string {
  if (dimension === 'volume') return 'ml';
  if (dimension === 'count') return 'piece';
  return 'g';
}

function standardOf(dimension: string): string[] {
  if (dimension === 'volume') return ['l'];
  if (dimension === 'count') return ['dozen'];
  return ['kg'];
}

/** What should be on the shelf, against what is. */
function Variance({ onReport }: { onReport: (cause: unknown) => void }) {
  const [rows, setRows] = useState<readonly VarianceView[] | null>(null);
  const [from, setFrom] = useState(() => {
    const then = new Date();
    then.setDate(then.getDate() - 30);
    return then.toISOString().slice(0, 10);
  });
  const [to, setTo] = useState(() => new Date().toISOString().slice(0, 10));

  useEffect(() => {
    call('stock_variance', { from, to })
      .then(setRows)
      .catch((cause) => {
        setRows([]);
        onReport(cause);
      });
  }, [from, to, onReport]);

  return (
    <div className="mb-stack">
      <div className="mb-row">
        <Input label="From" value={from} onChange={(e) => setFrom(e.target.value)} />
        <Input label="To" value={to} onChange={(e) => setTo(e.target.value)} />
      </div>

      {rows !== null && rows.length === 0 ? (
        <EmptyState
          title="Nothing to compare yet"
          body="Needs recipes and a stock count. Do a count and come back."
        />
      ) : (
        <Table
          rows={[...(rows ?? [])]}
          columns={[
            { key: 'material', header: 'Material', render: (v) => v.material },
            { key: 'theoretical', header: 'Should have used', render: (v) => v.theoretical },
            {
              key: 'actual',
              header: 'Really used',
              render: (v) => (v.isUnchecked ? <span className="mb-muted">{v.counted}</span> : v.actual),
            },
            {
              key: 'variance',
              header: 'Difference',
              render: (v) =>
                v.isUnchecked ? (
                  '—'
                ) : (
                  <strong className={v.isOver ? 'mb-stock__over' : undefined}>{v.variance}</strong>
                ),
            },
            { key: 'percent', header: 'As a share', render: (v) => (v.isUnchecked ? '—' : v.percent) },
            {
              key: 'value',
              header: 'What it cost',
              numeric: true,
              render: (v) => (
                <span className="mb-mono">{v.isUnchecked ? '—' : v.value.text}</span>
              ),
            },
          ]}
          rowKey={(v) => v.material}
        />
      )}
    </div>
  );
}
