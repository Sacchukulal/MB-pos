/**
 * **What an item is made of** — scope 6.1 sizes, 6.2 choices, 6.3 combos.
 *
 * The rules and the storage were built earlier in P13 and are tested on their
 * own; this is the way in. Three ideas, kept apart on purpose because a
 * shopkeeper keeps them apart:
 *
 * * a **size** is its own price (a half plate is not a discounted full plate);
 * * a **group of choices** is made once and offered on many items — a shop has
 *   one "Spice level", not one per curry;
 * * a **combo** is a price that gets shared across what is in it, which is the
 *   only way a dosa at 5% and a bottle of water at 18% can be sold as one deal
 *   and still produce a correct rate summary.
 *
 * # Nothing here decides anything
 *
 * Every number is parsed in Rust and comes back preformatted (R8, D39). The
 * shares under a combo are recomputed from today's prices on every read (D53),
 * so this screen never holds arithmetic that could go stale.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  EmptyState,
  freshId,
  InfoTip,
  Input,
  Modal,
  MoneyInput,
  SectionHeader,
  Select,
  Table,
  type Column,
} from '../kit';
import { call } from '../ipc/call';
import type { ComboView } from '../ipc/generated/ComboView';
import type { ItemComposition } from '../ipc/generated/ItemComposition';
import type { MenuRowView } from '../ipc/generated/MenuRowView';
import type { ModifierEdit } from '../ipc/generated/ModifierEdit';
import type { ModifierGroupView } from '../ipc/generated/ModifierGroupView';
import type { VariantView } from '../ipc/generated/VariantView';

/* **This screen had its own `freshId` and it was nearly right.**

   It was `Date.now() + Math.floor(Math.random() * 1000)` — the only id in the
   product with any randomness at all, which is why sizes and choices never hit
   the collision the rest of the app did. A thousand values is not enough
   though: by the birthday rule you would expect a repeat inside one
   millisecond after about forty of them, and `Math.random` is not a source
   anybody should be counting on.

   It is the kit's `freshId` now, like everything else. */

// ---------------------------------------------------------------------------
// One item: its sizes, and which groups it offers.
// ---------------------------------------------------------------------------

export function Composition({
  row,
  onClose,
  onFailed,
}: {
  row: MenuRowView;
  onClose: () => void;
  onFailed: (cause: unknown) => void;
}) {
  const [made, setMade] = useState<ItemComposition | null>(null);
  const [size, setSize] = useState<VariantView | null>(null);

  const load = useCallback(() => {
    call('item_composition', { itemId: row.id }).then(setMade).catch(onFailed);
  }, [row.id, onFailed]);

  useEffect(load, [load]);

  return (
    <Modal open title={`${row.name} — sizes and choices`} onClose={onClose} wide>
      <SectionHeader
        title="Sizes"
        note="Each size carries its own price. A half plate is a different thing to cook, not a discount off the full one."
      />

      {made === null || made.variants.length === 0 ? (
        <EmptyState
          title="One size"
          body="Most things are sold one way. Add a size only if you charge differently for it."
        />
      ) : (
        <ul className="mb-comp__list">
          {made.variants.map((variant) => (
            <li key={variant.id} className="mb-comp__row">
              <span>{variant.name}</span>
              <span className="mb-mono">{variant.price.text}</span>
              {variant.isActive ? null : <Badge tone="warn">Off</Badge>}
              <Button small variant="quiet" onClick={() => setSize(variant)}>
                Edit
              </Button>
            </li>
          ))}
        </ul>
      )}

      <div className="mb-row mb-row--end">
        <Button
          small
          onClick={() =>
            setSize({
              id: freshId('var'),
              name: '',
              price: { paise: 0n, text: '' },
              isActive: true,
            })
          }
        >
          Add a size
        </Button>
      </div>

      <SectionHeader
        title="Choices this item offers"
        note="Groups are shared across the menu. Tick the ones this item should ask about at the counter."
      />

      {made === null || made.groups.length === 0 ? (
        <EmptyState
          title="No groups yet"
          body="Make a group of choices below the menu — Spice level, Add-ons — and it becomes tickable here."
        />
      ) : (
        <ul className="mb-comp__list">
          {made.groups.map((group) => (
            <li key={group.id} className="mb-comp__row">
              <Checkbox
                label={group.name}
                checked={group.attached}
                onChange={(event) => {
                  call('attach_modifier_group', {
                    itemId: row.id,
                    groupId: group.id,
                    attach: event.target.checked,
                  })
                    .then(setMade)
                    .catch(onFailed);
                }}
              />
              <span className="mb-comp__rule">{group.rule}</span>
              <span className="mb-comp__rule">
                {group.modifiers.map((m) => m.name).join(', ')}
              </span>
            </li>
          ))}
        </ul>
      )}

      <div className="mb-row mb-row--end">
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </div>

      {size ? (
        <EditSize
          itemId={row.id}
          variant={size}
          onClose={() => setSize(null)}
          onSaved={(fresh) => {
            setMade(fresh);
            setSize(null);
          }}
          onFailed={onFailed}
        />
      ) : null}
    </Modal>
  );
}

function EditSize({
  itemId,
  variant,
  onClose,
  onSaved,
  onFailed,
}: {
  itemId: string;
  variant: VariantView;
  onClose: () => void;
  onSaved: (made: ItemComposition) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [name, setName] = useState(variant.name);
  const [price, setPrice] = useState(variant.price.text);
  const [active, setActive] = useState(variant.isActive);

  return (
    <Modal open title={variant.name === '' ? 'Add a size' : variant.name} onClose={onClose}>
      <Input
        label="Name"
        hint="Half, Full, 500g, Large."
        value={name}
        autoFocus
        onChange={(event) => setName(event.target.value)}
      />
      <MoneyInput label="Price" value={price} onChange={setPrice} />
      <Checkbox
        label="On the menu"
        checked={active}
        onChange={(event) => setActive(event.target.checked)}
      />
      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="primary"
          onClick={() => {
            call('save_item_variant', {
              itemId,
              variantId: variant.id,
              name,
              price,
              isActive: active,
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

// ---------------------------------------------------------------------------
// The shop's groups of choices.
// ---------------------------------------------------------------------------

export function ModifierGroups({ onFailed }: { onFailed: (cause: unknown) => void }) {
  const [groups, setGroups] = useState<readonly ModifierGroupView[]>([]);
  const [editing, setEditing] = useState<ModifierGroupView | null>(null);

  useEffect(() => {
    call('list_modifier_groups').then(setGroups).catch(onFailed);
  }, [onFailed]);

  return (
    <section className="mb-menu__classes">
      <div className="mb-row">
        <h2 className="mb-menu__heading">Choices</h2>
        <InfoTip label="About choices">
          Made once, offered on as many items as you like — open an item&rsquo;s
          sizes and choices to tick it on.
        </InfoTip>
        <Button
          small
          variant="quiet"
          onClick={() =>
            setEditing({
              id: freshId('grp'),
              name: '',
              minSelect: 0,
              maxSelect: 1,
              rule: '',
              modifiers: [],
              attached: false,
            })
          }
        >
          Add a group
        </Button>
      </div>

      {groups.length === 0 ? (
        <EmptyState
          title="No groups yet"
          body="Spice level, Add-ons, How would you like it cooked — anything the counter has to ask."
        />
      ) : (
        <div className="mb-menu__classlist">
          {groups.map((group) => (
            <div key={group.id} className="mb-menu__class">
              <div>
                <div>{group.name}</div>
                <div className="mb-comp__rule">
                  {group.rule} · {group.modifiers.length} choice
                  {group.modifiers.length === 1 ? '' : 's'}
                </div>
              </div>
              <Button small variant="quiet" onClick={() => setEditing(group)}>
                Edit
              </Button>
            </div>
          ))}
        </div>
      )}

      {editing ? (
        <EditGroup
          group={editing}
          onClose={() => setEditing(null)}
          onSaved={(fresh) => {
            setGroups(fresh);
            setEditing(null);
          }}
          onFailed={onFailed}
        />
      ) : null}
    </section>
  );
}

function EditGroup({
  group,
  onClose,
  onSaved,
  onFailed,
}: {
  group: ModifierGroupView;
  onClose: () => void;
  onSaved: (groups: readonly ModifierGroupView[]) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [name, setName] = useState(group.name);
  // "How many may they pick" as one choice rather than two numbers, because
  // min and max as separate boxes is how you end up with "at least 3 of 2" —
  // which Rust refuses, but nobody should have been able to type.
  const [shape, setShape] = useState(() => {
    if (group.minSelect === 1 && group.maxSelect === 1) return 'one';
    if (group.maxSelect === 1) return 'atMostOne';
    if (group.maxSelect === null && group.minSelect === 0) return 'any';
    return 'atLeastOne';
  });
  const [choices, setChoices] = useState<readonly ModifierEdit[]>(
    group.modifiers.map((m) => ({
      id: m.id,
      name: m.name,
      priceDelta: m.priceDelta.text,
    })),
  );

  const shaped = (): { minSelect: number; maxSelect: number | null } => {
    switch (shape) {
      case 'one':
        return { minSelect: 1, maxSelect: 1 };
      case 'atMostOne':
        return { minSelect: 0, maxSelect: 1 };
      case 'atLeastOne':
        return { minSelect: 1, maxSelect: null };
      default:
        return { minSelect: 0, maxSelect: null };
    }
  };

  const change = (index: number, patch: Partial<ModifierEdit>) => {
    setChoices((was) => was.map((c, i) => (i === index ? { ...c, ...patch } : c)));
  };

  return (
    <Modal open title={group.name === '' ? 'A group of choices' : group.name} onClose={onClose} wide>
      <Input
        label="Name"
        hint="Spice level, Add-ons — what the counter is being asked."
        value={name}
        autoFocus
        onChange={(event) => setName(event.target.value)}
      />
      <Select
        label="How many may they pick"
        value={shape}
        onChange={(event) => setShape(event.target.value)}
        options={[
          { value: 'one', label: 'Exactly one' },
          { value: 'atMostOne', label: 'One at most' },
          { value: 'any', label: 'Any number' },
          { value: 'atLeastOne', label: 'At least one' },
        ]}
      />

      <SectionHeader
        title="The choices"
        note={
          <>
            Leave the price blank when a choice is free. It may be a minus —
            &ldquo;No onion, &minus;5&rdquo; takes money off.
          </>
        }
      />
      {choices.map((choice, index) => (
        <div key={choice.id} className="mb-comp__choice">
          <Input
            label="Choice"
            value={choice.name}
            onChange={(event) => change(index, { name: event.target.value })}
          />
          <MoneyInput
            label="Price difference"
            value={choice.priceDelta}
            onChange={(next) => change(index, { priceDelta: next })}
          />
          <Button
            small
            variant="quiet"
            onClick={() => setChoices((was) => was.filter((_, i) => i !== index))}
          >
            Remove
          </Button>
        </div>
      ))}
      <div className="mb-row mb-row--end">
        <Button
          small
          onClick={() =>
            setChoices((was) => [...was, { id: freshId('mod'), name: '', priceDelta: '' }])
          }
        >
          Add a choice
        </Button>
      </div>

      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="primary"
          onClick={() => {
            call('save_modifier_group', {
              group: { id: group.id, name, ...shaped(), modifiers: [...choices] },
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

// ---------------------------------------------------------------------------
// Combos.
// ---------------------------------------------------------------------------

export function Combos({
  rows,
  onFailed,
}: {
  rows: readonly MenuRowView[];
  onFailed: (cause: unknown) => void;
}) {
  const [combos, setCombos] = useState<readonly ComboView[]>([]);
  const [editing, setEditing] = useState<ComboView | null>(null);

  useEffect(() => {
    call('list_combos').then(setCombos).catch(onFailed);
  }, [onFailed]);

  const columns: Column<ComboView>[] = [
    { key: 'name', header: 'Combo', render: (c) => c.name },
    {
      key: 'parts',
      header: "What's in it",
      // The share and the rate on every part, because a combo across two rates
      // is the whole reason apportioning exists.
      render: (c) => (
        <span className="mb-comp__rule">
          {c.parts
            .map((p) => `${p.qty} × ${p.itemName} (${p.share.text} at ${p.rate})`)
            .join(', ')}
        </span>
      ),
    },
    {
      key: 'separately',
      header: 'Separately',
      numeric: true,
      render: (c) => <span className="mb-mono">{c.separately.text}</span>,
    },
    {
      key: 'price',
      header: 'Combo price',
      numeric: true,
      render: (c) => <span className="mb-mono">{c.price.text}</span>,
    },
    {
      key: 'do',
      header: '',
      render: (c) => (
        <Button small variant="quiet" onClick={() => setEditing(c)}>
          Edit
        </Button>
      ),
    },
  ];

  return (
    <section className="mb-menu__classes">
      <div className="mb-row">
        <h2 className="mb-menu__heading">Combos</h2>
        {/* A real rule and not a reassurance, so it is kept — just asked for
            rather than given. */}
        <InfoTip label="About combos">
          The combo price is shared across what is in it, in proportion to what
          each part sells for on its own — so a meal that mixes a 5% dish with
          an 18% bottle still adds up correctly on the rate summary.
        </InfoTip>
        <Button
          small
          variant="quiet"
          onClick={() =>
            setEditing({
              id: freshId('cmb'),
              name: '',
              price: { paise: 0n, text: '' },
              isActive: true,
              parts: [],
              separately: { paise: 0n, text: '' },
            })
          }
        >
          Add a combo
        </Button>
      </div>

      {combos.length === 0 ? (
        <EmptyState
          title="No combos yet"
          body="A thali, a meal deal, a happy-hour pair — anything you sell at one price that is really two things."
        />
      ) : (
        <Table rows={[...combos]} columns={columns} rowKey={(c) => c.id} />
      )}

      {editing ? (
        <EditCombo
          combo={editing}
          rows={rows}
          onClose={() => setEditing(null)}
          onSaved={(fresh) => {
            setCombos(fresh);
            setEditing(null);
          }}
          onFailed={onFailed}
        />
      ) : null}
    </section>
  );
}

function EditCombo({
  combo,
  rows,
  onClose,
  onSaved,
  onFailed,
}: {
  combo: ComboView;
  rows: readonly MenuRowView[];
  onClose: () => void;
  onSaved: (combos: readonly ComboView[]) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [name, setName] = useState(combo.name);
  const [price, setPrice] = useState(combo.price.text);
  const [active, setActive] = useState(combo.isActive);
  const [parts, setParts] = useState<readonly [string, string][]>(
    combo.parts.map((p) => [p.itemId, p.qty] as [string, string]),
  );

  const change = (index: number, part: [string, string]) => {
    setParts((was) => was.map((p, i) => (i === index ? part : p)));
  };

  return (
    <Modal open title={combo.name === '' ? 'A combo' : combo.name} onClose={onClose} wide>
      <Input
        label="Name"
        hint="What it is called on the bill — Thali, Meal for two."
        value={name}
        autoFocus
        onChange={(event) => setName(event.target.value)}
      />
      <MoneyInput
        label="Combo price"
        hint="What the customer pays for the lot."
        value={price}
        onChange={setPrice}
      />

      <h3 className="mb-comp__heading">What is in it</h3>
      {parts.map(([itemId, qty], index) => (
        <div key={`${itemId}-${index}`} className="mb-comp__choice">
          <Select
            label="Item"
            value={itemId}
            onChange={(event) => change(index, [event.target.value, qty])}
            options={[
              { value: '', label: 'Pick an item' },
              ...rows.map((r) => ({ value: r.id, label: `${r.name} — ${r.price.text}` })),
            ]}
          />
          <Input
            label="How many"
            value={qty}
            onChange={(event) => change(index, [itemId, event.target.value])}
          />
          <Button
            small
            variant="quiet"
            onClick={() => setParts((was) => was.filter((_, i) => i !== index))}
          >
            Remove
          </Button>
        </div>
      ))}
      <div className="mb-row mb-row--end">
        <Button small onClick={() => setParts((was) => [...was, ['', '1']])}>
          Add something
        </Button>
      </div>

      <Checkbox
        label="On the menu"
        checked={active}
        onChange={(event) => setActive(event.target.checked)}
      />
      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="primary"
          onClick={() => {
            call('save_combo', {
              combo: { id: combo.id, name, price, isActive: active, parts: [...parts] },
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
