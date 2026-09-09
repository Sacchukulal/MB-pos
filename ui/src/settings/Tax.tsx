/** Settings › Tax — GST on or off, the shop's rate, and the categories or items on their own. */

import { useCallback, useEffect, useMemo, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  Checkbox,
  Choice,
  Input,
  Notice,
  plural,
  SectionHeader,
  Select,
  Spinner,
  Table,
  Toolbar,
  useToast,
  type Column,
} from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { TaxItemView } from '../ipc/generated/TaxItemView';
import type { TaxPageView } from '../ipc/generated/TaxPageView';

/** The three answers to "is the tax inside the price?", in the words the screen uses. */
const PRICE_WORDS = [
  { value: 'shop', label: 'Shop rule' },
  { value: 'inclusive', label: 'Tax inside the price' },
  { value: 'exclusive', label: 'Tax added on top' },
];

const REGISTRATIONS = [
  { value: 'unregistered', label: 'No GST' },
  { value: 'regular', label: 'Regular GST' },
  { value: 'composition', label: 'Composition scheme' },
];

const BASES = [
  { value: 'exclusive', label: 'Added on top' },
  { value: 'inclusive', label: 'Inside the price' },
];

/** The word in the "Set by" column. */
const FROM_WORDS: Record<string, string> = {
  shop: 'Shop',
  category: 'Category',
  item: 'This item',
};

/** The GST card, as a person fills it in. */
interface Shop {
  registration: string;
  gstin: string;
  stateCode: string;
  basis: string;
  rate: string;
}

function shopOf(page: TaxPageView): Shop {
  return {
    registration: page.registration,
    gstin: page.gstin,
    stateCode: page.stateCode,
    basis: page.shopBasis,
    rate: page.shopRate,
  };
}

export function Tax() {
  const [page, setPage] = useState<TaxPageView | null>(null);
  const [shop, setShop] = useState<Shop | null>(null);
  const [filter, setFilter] = useState('');
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set());
  const [slabPick, setSlabPick] = useState('');
  const [pricePick, setPricePick] = useState('');
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  /** A fresh page from Rust, and the card follows it. */
  const take = useCallback((next: TaxPageView) => {
    setPage(next);
    setShop(shopOf(next));
  }, []);

  const load = useCallback(() => {
    if (!inApp()) return;
    call('tax_page').then(take).catch(complain);
  }, [complain, take]);

  useEffect(() => {
    load();
  }, [load]);

  const slabOptions = useMemo(
    () => (page?.slabs ?? []).map((s) => ({ value: s.id, label: s.name })),
    [page],
  );

  /** The rows on screen: one category, or everything. */
  const rows = useMemo(() => {
    if (!page) return [];
    const groups = filter === '' ? page.categories : page.categories.filter((g) => (g.id ?? '') === filter);
    return groups.flatMap((g) => g.items.map((item) => ({ item, category: g.name })));
  }, [page, filter]);

  const group = page?.categories.find((g) => g.id === filter) ?? null;

  const toggleItem = (id: string, on: boolean) =>
    setTicked((was) => {
      const next = new Set(was);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });

  const allShown = rows.length > 0 && rows.every(({ item }) => ticked.has(item.id));
  const tickAll = () =>
    setTicked((was) => {
      const next = new Set(was);
      for (const { item } of rows) {
        if (allShown) next.delete(item.id);
        else next.add(item.id);
      }
      return next;
    });

  /** The GST card. The rate goes first: it may move items, and the page is re-read after. */
  const saveShop = async () => {
    if (!page || !shop) return;
    setBusy(true);
    try {
      let next = page;
      if (shop.rate.trim() !== page.shopRate) {
        next = await call('set_shop_tax_rate', { percent: shop.rate });
      }
      const edits = [
        ['store.registration', shop.registration, page.registration],
        ['store.gstin', shop.gstin, page.gstin],
        ['store.state_code', shop.stateCode, page.stateCode],
        ['store.price_basis', shop.basis, page.shopBasis],
      ]
        .filter(([, now, was]) => now !== was)
        .map(([key, value]) => ({ key: key ?? '', value: value ?? '' }));
      if (edits.length > 0) {
        await call('save_settings', { edits });
        next = await call('tax_page');
      }
      take(next);
      toast.show('ok', 'Saved.');
    } catch (cause) {
      complain(cause);
    } finally {
      setBusy(false);
    }
  };

  /** Put the ticked items on the chosen rate and/or price rule. */
  const apply = () => {
    if (ticked.size === 0) return;
    setBusy(true);
    call('set_items_tax', {
      itemIds: [...ticked],
      slabId: slabPick === '' ? null : slabPick,
      basis: pricePick === '' ? null : pricePick,
    })
      .then((next) => {
        take(next);
        setTicked(new Set());
        setSlabPick('');
        setPricePick('');
        toast.show('ok', `${plural(ticked.size, 'item')} changed.`);
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const setCategoryRate = (slabId: string) => {
    if (!group?.id) return;
    call('set_category_tax', { categoryId: group.id, slabId: slabId === 'follow' ? null : slabId })
      .then(take)
      .catch(complain);
  };

  if (!page || !shop) return <Spinner label="Reading the tax book" />;

  const dirty =
    shop.registration !== page.registration ||
    shop.gstin !== page.gstin ||
    shop.stateCode !== page.stateCode ||
    shop.basis !== page.shopBasis ||
    shop.rate.trim() !== page.shopRate;
  const registered = shop.registration !== 'unregistered';
  const regular = shop.registration === 'regular';

  const columns: Column<{ item: TaxItemView; category: string }>[] = [
    {
      key: 'tick',
      header: '',
      nowrap: true,
      render: ({ item }) => (
        <Checkbox
          aria-label={`Tick ${item.name}`}
          checked={ticked.has(item.id)}
          onChange={(event) => toggleItem(item.id, event.currentTarget.checked)}
        />
      ),
    },
    {
      key: 'name',
      header: 'Item',
      render: ({ item }) =>
        item.isAvailable ? (
          item.name
        ) : (
          <span className="mb-row mb-row--gap-inline">
            {item.name}
            <Badge tone="warn">Sold out</Badge>
          </span>
        ),
    },
    ...(filter === ''
      ? [{ key: 'category', header: 'Category', render: (r: { category: string }) => r.category }]
      : []),
    {
      key: 'price',
      header: 'Price',
      numeric: true,
      render: ({ item }) => <span className="mb-mono">{item.price.text}</span>,
    },
    { key: 'tax', header: 'GST', render: ({ item }) => item.words },
    { key: 'from', header: 'Set by', render: ({ item }) => FROM_WORDS[item.from] ?? item.from },
  ];

  return (
    <div className="mb-tax">
      {page.registrationNote ? <Notice tone="warn">{page.registrationNote}</Notice> : null}

      <Card>
        <SectionHeader title="GST" />
        <Choice
          label="GST"
          value={shop.registration}
          options={REGISTRATIONS}
          onPick={(registration) => setShop({ ...shop, registration })}
        />
        {registered ? (
          <div className="mb-tax__fields">
            <Input
              label="GST number"
              value={shop.gstin}
              maxLength={32}
              onChange={(event) => setShop({ ...shop, gstin: event.target.value })}
            />
            <Select
              label="State"
              value={shop.stateCode}
              options={page.states}
              onChange={(event) => setShop({ ...shop, stateCode: event.target.value })}
            />
          </div>
        ) : null}
        {regular ? (
          <>
            <Choice
              label="Prices"
              value={shop.basis}
              options={BASES}
              onPick={(basis) => setShop({ ...shop, basis })}
            />
            <div className="mb-tax__fields">
              {/* field-lint-ok: a percentage, not money */}
              <Input
                label="Shop GST rate %"
                hint="Every item is taxed at this unless its category or the item says otherwise."
                value={shop.rate}
                inputMode="decimal"
                onChange={(event) =>
                  setShop({ ...shop, rate: event.target.value.replace(/[^0-9.]/g, '') })
                }
              />
            </div>
          </>
        ) : null}
        <div className="mb-row mb-row--end">
          <Button variant="quiet" disabled={!dirty || busy} onClick={() => setShop(shopOf(page))}>
            Cancel
          </Button>
          <Button variant="primary" disabled={!dirty || busy} onClick={() => void saveShop()}>
            Save
          </Button>
        </div>
      </Card>

      {page.registration === 'regular' ? (
        <Card>
          <SectionHeader title="Item GST" />
          <Toolbar
            end={
              rows.length > 0 ? (
                <Button size="sm" variant="quiet" onClick={tickAll}>
                  {allShown ? 'Untick all' : 'Tick all'}
                </Button>
              ) : null
            }
          >
            <Select
              aria-label="Category"
              value={filter}
              options={[
                { value: '', label: 'Every category' },
                ...page.categories
                  .filter((g) => g.id !== null)
                  .map((g) => ({ value: g.id ?? '', label: g.name })),
              ]}
              onChange={(event) => {
                setFilter(event.currentTarget.value);
                setTicked(new Set());
              }}
            />
            {group?.id ? (
              <Select
                aria-label={`Rate for ${group.name}`}
                value={group.ownSlabId ?? 'follow'}
                options={[{ value: 'follow', label: 'Shop rate' }, ...slabOptions]}
                onChange={(event) => setCategoryRate(event.currentTarget.value)}
              />
            ) : null}
          </Toolbar>

          {rows.length === 0 ? (
            <p className="mb-muted">No items here yet.</p>
          ) : (
            <Table dense rows={rows} columns={columns} rowKey={({ item }) => item.id} />
          )}

          {/* The apply bar: what the ticks are for. */}
          <div className="mb-tax__apply" role="group" aria-label="Apply to the ticked items">
            <span className="mb-tax__count">{plural(ticked.size, 'item')} ticked</span>
            <Select
              aria-label="Rate for the ticked items"
              value={slabPick}
              disabled={ticked.size === 0}
              options={[
                { value: '', label: 'Rate: leave as it is' },
                { value: 'follow', label: group?.id ? 'Category rate' : 'Shop rate' },
                ...slabOptions,
              ]}
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
            <Button variant="quiet" disabled={ticked.size === 0} onClick={() => setTicked(new Set())}>
              Clear
            </Button>
          </div>
        </Card>
      ) : null}
    </div>
  );
}

