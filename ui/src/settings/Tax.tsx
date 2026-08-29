/** Settings › Tax — the slabs, and which item is on which. The one screen for tax. */

import { useCallback, useEffect, useMemo, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  Checkbox,
  ConfirmDialog,
  EmptyState,
  freshId,
  Icon,
  InfoTip,
  Input,
  Modal,
  Notice,
  plural,
  SectionHeader,
  Select,
  Spinner,
  Table,
  useToast,
  type Column,
} from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { TaxCategoryView } from '../ipc/generated/TaxCategoryView';
import type { TaxItemView } from '../ipc/generated/TaxItemView';
import type { TaxPageView } from '../ipc/generated/TaxPageView';
import type { TaxSlabView } from '../ipc/generated/TaxSlabView';

/** The three answers to "is the tax inside the price?", in the words the screen uses. */
const PRICE_WORDS = [
  { value: 'shop', label: 'Shop default' },
  { value: 'inclusive', label: 'In the price' },
  { value: 'exclusive', label: 'Added on top' },
];

const KINDS: readonly { value: TaxSlabView['kind']; label: string }[] = [
  { value: 'gst', label: 'GST' },
  { value: 'exempt', label: 'Exempt' },
  { value: 'outside_gst', label: 'State VAT (liquor)' },
  { value: 'untaxed', label: 'No tax' },
];

function kindWords(kind: TaxSlabView['kind']): string {
  return KINDS.find((k) => k.value === kind)?.label ?? kind;
}

function priceWords(basis: string): string {
  return PRICE_WORDS.find((p) => p.value === basis)?.label ?? basis;
}

export function Tax() {
  const [page, setPage] = useState<TaxPageView | null>(null);
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set());
  const [slabPick, setSlabPick] = useState('');
  const [pricePick, setPricePick] = useState('');
  const [editing, setEditing] = useState<TaxSlabView | null>(null);
  const [removing, setRemoving] = useState<TaxSlabView | null>(null);
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    if (!inApp()) return;
    call('tax_page').then(setPage).catch(complain);
  }, [complain]);

  useEffect(() => {
    load();
  }, [load]);

  /** The slabs somebody may put an item on today. */
  const live = useMemo(() => page?.slabs.filter((s) => s.isActive) ?? [], [page]);
  const slabOptions = useMemo(
    () => live.map((s) => ({ value: s.id, label: s.name })),
    [live],
  );

  const toggleItem = (id: string, on: boolean) =>
    setTicked((was) => {
      const next = new Set(was);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });

  const toggleGroup = (group: TaxCategoryView, on: boolean) =>
    setTicked((was) => {
      const next = new Set(was);
      for (const item of group.items) {
        if (on) next.add(item.id);
        else next.delete(item.id);
      }
      return next;
    });

  /** Put the ticked items on the chosen slab and/or price basis. */
  const apply = () => {
    if (ticked.size === 0) return;
    setBusy(true);
    call('set_items_tax', {
      itemIds: [...ticked],
      slabId: slabPick === '' ? null : slabPick,
      basis: pricePick === '' ? null : pricePick,
    })
      .then((next) => {
        setPage(next);
        setTicked(new Set());
        setSlabPick('');
        setPricePick('');
        toast.show('ok', `${plural(ticked.size, 'item')} moved.`);
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const setCategoryDefault = (group: TaxCategoryView, slabId: string) => {
    if (!group.id) return;
    call('set_category_tax', { categoryId: group.id, slabId: slabId === '' ? null : slabId })
      .then(setPage)
      .catch(complain);
  };

  if (!page) return <Spinner label="Reading the tax book" />;

  const slabColumns: Column<TaxSlabView>[] = [
    {
      key: 'name',
      header: 'Slab',
      render: (s) =>
        s.isActive ? (
          s.name
        ) : (
          <span className="mb-row mb-row--gap-inline">
            {s.name}
            <Badge tone="warn">Removed</Badge>
          </span>
        ),
    },
    {
      key: 'rate',
      header: 'Rate',
      numeric: true,
      render: (s) =>
        s.kind === 'exempt' || s.kind === 'untaxed' ? null : (
          <span className="mb-mono">{s.rate}</span>
        ),
    },
    { key: 'kind', header: 'Kind', render: (s) => kindWords(s.kind) },
    { key: 'price', header: 'Price', render: (s) => s.priceWords },
    {
      key: 'items',
      header: 'Items',
      numeric: true,
      render: (s) => <span className="mb-mono">{s.itemsUsing}</span>,
    },
    {
      key: 'do',
      header: '',
      render: (s) => (
        <div className="mb-row">
          <Button size="sm" onClick={() => setEditing(s)}>
            Edit
          </Button>
          <Button
            size="sm"
            variant="quiet"
            disabled={s.itemsUsing > 0}
            onClick={() => setRemoving(s)}
          >
            Remove
          </Button>
        </div>
      ),
    },
  ];

  const itemColumns = (group: TaxCategoryView): Column<TaxItemView>[] => [
    {
      key: 'tick',
      header: '',
      nowrap: true,
      render: (item) => (
        <Checkbox
          aria-label={`Tick ${item.name}`}
          checked={ticked.has(item.id)}
          onChange={(event) => toggleItem(item.id, event.currentTarget.checked)}
        />
      ),
    },
    {
      key: 'name',
      header: group.name,
      render: (item) =>
        item.isAvailable ? (
          item.name
        ) : (
          <span className="mb-row mb-row--gap-inline">
            {item.name}
            <Badge tone="warn">Sold out</Badge>
          </span>
        ),
    },
    {
      key: 'price',
      header: 'Price',
      numeric: true,
      render: (item) => <span className="mb-mono">{item.price.text}</span>,
    },
    { key: 'slab', header: 'Slab', render: (item) => item.slabName },
    { key: 'tax', header: 'Taxed at', render: (item) => item.words },
    {
      key: 'own',
      header: 'Own price rule',
      optional: true,
      render: (item) => (item.basis === 'shop' ? null : priceWords(item.basis)),
    },
  ];

  const total = page.categories.reduce((n, g) => n + g.items.length, 0);

  return (
    <div className="mb-tax">
      {page.registrationNote ? <Notice tone="warn">{page.registrationNote}</Notice> : null}
      <Card>
        <SectionHeader
          title="Tax slabs"
          sticky
          action={
            <Button
              size="sm"
              variant="primary"
              onClick={() =>
                setEditing({
                  id: freshId('tax'),
                  name: '',
                  rate: '',
                  rateBp: 0,
                  kind: 'gst',
                  basis: 'shop',
                  priceWords: '',
                  isActive: true,
                  itemsUsing: 0,
                })
              }
            >
              <Icon name="plus" size="sm" />
              Add a slab
            </Button>
          }
        />
        <Table dense rows={page.slabs} columns={slabColumns} rowKey={(s) => s.id} />
      </Card>

      <Card>
        <SectionHeader
          title="Which slab each item is on"
          sticky
          action={
            <InfoTip label="About ticking items">
              Tick items, or a whole category, then choose a slab or a price rule below and
              press Apply. A category's own choice is what a new item in it starts on.
            </InfoTip>
          }
        />
        {total === 0 ? (
          <EmptyState small title="No items yet" hint="Add items on the Menu screen first." />
        ) : (
          page.categories.map((group) => {
            const inGroup = group.items.filter((i) => ticked.has(i.id)).length;
            const all = group.items.length > 0 && inGroup === group.items.length;
            return (
              <section key={group.id ?? 'none'} className="mb-tax__group">
                <div className="mb-tax__grouphead">
                  <Checkbox
                    aria-label={`Tick every item in ${group.name}`}
                    checked={all}
                    ref={(box: HTMLInputElement | null) => {
                      if (box) box.indeterminate = inGroup > 0 && !all;
                    }}
                    onChange={(event) => toggleGroup(group, event.currentTarget.checked)}
                  />
                  <strong className="mb-tax__groupname">{group.name}</strong>
                  <span className="mb-muted">{plural(group.items.length, 'item')}</span>
                  {group.id ? (
                    <div className="mb-tax__groupdefault">
                      <Select
                        aria-label={`New items in ${group.name} start on`}
                        value={group.defaultSlabId ?? ''}
                        options={[{ value: '', label: 'New items: pick each time' }, ...slabOptions]}
                        onChange={(event) => setCategoryDefault(group, event.currentTarget.value)}
                      />
                    </div>
                  ) : null}
                </div>
                <Table
                  dense
                  rows={group.items}
                  columns={itemColumns(group)}
                  rowKey={(item) => item.id}
                />
              </section>
            );
          })
        )}

        {/* The apply bar: what the ticks are for. */}
        <div className="mb-tax__apply" role="group" aria-label="Apply to the ticked items">
          <span className="mb-tax__count">{plural(ticked.size, 'item')} ticked</span>
          <Select
            aria-label="Move the ticked items to"
            value={slabPick}
            disabled={ticked.size === 0}
            options={[{ value: '', label: 'Slab: leave as it is' }, ...slabOptions]}
            onChange={(event) => setSlabPick(event.currentTarget.value)}
          />
          <Select
            aria-label="Price rule for the ticked items"
            value={pricePick}
            disabled={ticked.size === 0}
            options={[{ value: '', label: 'Price: leave as it is' }, ...PRICE_WORDS]}
            onChange={(event) => setPricePick(event.currentTarget.value)}
          />
          <Button
            variant="primary"
            disabled={busy || ticked.size === 0 || (slabPick === '' && pricePick === '')}
            onClick={apply}
          >
            Apply
          </Button>
          <Button
            variant="quiet"
            disabled={ticked.size === 0}
            onClick={() => setTicked(new Set())}
          >
            Clear
          </Button>
        </div>
      </Card>

      {editing ? (
        <EditSlab
          key={editing.id}
          slab={editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            load();
          }}
          onFailed={complain}
        />
      ) : null}

      <ConfirmDialog
        open={removing !== null}
        title={removing ? `Remove ${removing.name}?` : 'Remove this slab?'}
        body="Bills already printed keep the tax they were printed with."
        confirmLabel="Remove"
        cancelLabel="Keep it"
        onConfirm={() => {
          const gone = removing;
          setRemoving(null);
          if (!gone) return;
          call('remove_tax_slab', { id: gone.id })
            .then(() => {
              toast.show('ok', `${gone.name} removed.`);
              load();
            })
            .catch(complain);
        }}
        onCancel={() => setRemoving(null)}
      />
    </div>
  );
}

/** One slab's form. The machine values go back exactly as Rust sent them. */
function EditSlab({
  slab,
  onClose,
  onSaved,
  onFailed,
}: {
  slab: TaxSlabView;
  onClose: () => void;
  onSaved: () => void;
  onFailed: (cause: unknown) => void;
}) {
  const adding = slab.name === '';
  const [name, setName] = useState(slab.name);
  const [rate, setRate] = useState(slab.rate.replace('%', ''));
  const [kind, setKind] = useState<TaxSlabView['kind']>(slab.kind);
  const [basis, setBasis] = useState(slab.basis);
  const [busy, setBusy] = useState(false);
  const toast = useToast();
  // Exempt and no-tax have no rate at all, so the box is shut rather than hinted at.
  const rateless = kind === 'exempt' || kind === 'untaxed';

  const save = () => {
    setBusy(true);
    call('save_tax_slab', {
      edit: { id: slab.id, name, rate: rateless ? '0' : rate, kind, basis },
    })
      .then(() => {
        toast.show('ok', `${name.trim()} saved.`);
        onSaved();
      })
      .catch(onFailed)
      .finally(() => setBusy(false));
  };

  return (
    <Modal
      open
      title={adding ? 'Add a slab' : slab.name}
      onClose={onClose}
      actions={
        <>
          <Button variant="quiet" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy || name.trim() === ''} onClick={save}>
            {adding ? 'Add it' : 'Save'}
          </Button>
        </>
      }
    >
      <Input
        label="Name"
        value={name}
        autoFocus
        placeholder="GST 5%"
        onChange={(event) => setName(event.target.value)}
      />
      <Select
        label="Kind"
        value={kind}
        onChange={(event) => setKind(event.target.value as TaxSlabView['kind'])}
        options={KINDS}
      />
      {/* field-lint-ok: a percentage, not money */}
      <Input
        label={kind === 'outside_gst' ? 'State VAT %' : 'Rate %'}
        value={rateless ? '0' : rate}
        disabled={rateless}
        inputMode="decimal"
        onChange={(event) => setRate(event.target.value.replace(/[^0-9.]/g, ''))}
      />
      <Select
        label="Price"
        hint="Shop default follows Settings › Tax › Menu prices. Liquor is usually in the price."
        value={basis}
        onChange={(event) => setBasis(event.target.value)}
        options={PRICE_WORDS}
      />
    </Modal>
  );
}
