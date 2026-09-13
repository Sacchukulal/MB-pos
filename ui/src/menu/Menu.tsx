/** The menu: categories down the left, items on the right, both typed in a run. */

import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  Choice,
  ConfirmDialog,
  EmptyState,
  Foot,
  freshId,
  Icon,
  Input,
  Modal,
  MoneyInput,
  Page,
  PageHeader,
  Panel,
  Pick,
  plural,
  RowMenu,
  Scroller,
  SearchField,
  Select,
  Spinner,
  Table,
  Tabs,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError, type ImportMode } from '../ipc/call';
import type { CategoryView } from '../ipc/generated/CategoryView';
import type { MenuRowView } from '../ipc/generated/MenuRowView';
import type { ImportPlanView } from '../ipc/generated/ImportPlanView';
import { Combos, Composition, ModifierGroups } from './Composition';

import './menu.css';

/** The three kinds of thing on the menu, as three tabs of one shape. */
type Tab = 'items' | 'choices' | 'combos';

export function Menu() {
  const [categories, setCategories] = useState<readonly CategoryView[] | null>(null);
  const [rows, setRows] = useState<readonly MenuRowView[]>([]);
  const [chosen, setChosen] = useState<string | null>(null);
  const [find, setFind] = useState('');
  const [editing, setEditing] = useState<MenuRowView | null>(null);
  const [deleting, setDeleting] = useState<MenuRowView | null>(null);
  const [madeOf, setMadeOf] = useState<MenuRowView | null>(null);
  const [bulkOpen, setBulkOpen] = useState(false);
  /** The spreadsheet somebody chose, waiting to be looked at and agreed to. */
  const [importing, setImporting] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>('items');
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(async () => {
    try {
      setCategories(await call('menu_categories'));
      setRows(await call('menu_rows'));
    } catch (cause) {
      report(cause);
    }
  }, [report]);

  useEffect(() => {
    void load();
  }, [load]);

  const live = useMemo(() => (categories ?? []).filter((c) => c.isActive), [categories]);

  const shown = useMemo(() => {
    const wanted = find.trim().toLowerCase();
    return rows.filter((row) => {
      if (chosen && row.categoryId !== chosen) return false;
      if (wanted === '') return true;
      return (
        row.name.toLowerCase().includes(wanted) ||
        (row.shortCode ?? '').toLowerCase() === wanted
      );
    });
  }, [rows, chosen, find]);

  const columns: Column<MenuRowView>[] = [
    {
      key: 'name',
      header: 'Item',
      render: (r) =>
        r.isAvailable ? (
          r.name
        ) : (
          <span className="mb-row mb-row--gap-inline">
            {r.name}
            <Badge tone="warn">Sold out</Badge>
          </span>
        ),
    },
    { key: 'code', header: 'Code', optional: true, render: (r) => r.shortCode },
    {
      key: 'price',
      header: 'Price',
      numeric: true,
      render: (r) => <span className="mb-mono">{r.price.text}</span>,
    },
    { key: 'tax', header: 'GST', render: (r) => r.rate },
    {
      key: 'do',
      header: '',
      render: (r) => (
        <div className="mb-row">
          <Button size="sm" onClick={() => setEditing(r)}>
            Edit
          </Button>
          <Button size="sm" variant="quiet" onClick={() => setDeleting(r)}>
            Delete
          </Button>
          <RowMenu label={`More for ${r.name}`}>
            <Button size="sm" variant="quiet" onClick={() => setMadeOf(r)}>
              Sizes &amp; choices{r.variants > 0n ? ` (${r.variants})` : ''}
            </Button>
            <Button
              size="sm"
              variant="quiet"
              onClick={() => {
                call('set_item_available', { itemId: r.id, available: !r.isAvailable })
                  .then(setRows)
                  .catch(report);
              }}
            >
              {r.isAvailable ? 'Sold out' : 'Put back'}
            </Button>
          </RowMenu>
        </div>
      ),
    },
  ];

  if (categories === null) {
    return (
      <Page className="mb-menu">
        <Spinner label="Reading the menu" />
      </Page>
    );
  }

  return (
    <Page className="mb-menu">
      <PageHeader
        title="Menu"
        count={rows.length}
        actions={
          <>
            <div className="mb-menu__find">
              <SearchField
                value={find}
                placeholder="Find an item"
                onChange={(event) => setFind(event.target.value)}
              />
            </div>
            <RowMenu label="More for the menu" size="md">
              <Button variant="quiet" onClick={() => setBulkOpen(true)}>
                <Icon name="tag" size="sm" />
                Change prices
              </Button>
              {/* The counter's own file dialog; the file is read and planned in Rust. */}
              <Button
                variant="quiet"
                onClick={() => {
                  call('pick_menu_file')
                    .then((path) => {
                      if (path) setImporting(path);
                    })
                    .catch(report);
                }}
              >
                <Icon name="upload" size="sm" />
                Import a file
              </Button>
              <Button
                variant="quiet"
                onClick={() => {
                  call('export_menu')
                    .then((path) => {
                      if (path) toast.show('ok', 'The menu was saved.', path);
                    })
                    .catch(report);
                }}
              >
                <Icon name="download" size="sm" />
                Save as a file
              </Button>
            </RowMenu>
          </>
        }
      />

      <Tabs
        tabs={[
          { id: 'items', label: 'Items' },
          { id: 'choices', label: 'Choices' },
          { id: 'combos', label: 'Combos' },
        ]}
        active={tab}
        onChange={(id) => setTab(id as Tab)}
      />

      {tab === 'items' ? (
        <div className="mb-menu__body">
          <Categories
            categories={live}
            total={rows.length}
            chosen={chosen}
            onChoose={setChosen}
            onChanged={(fresh) => {
              setCategories(fresh);
              // A renamed or deleted category changes the rows too.
              void load();
            }}
            onFailed={report}
          />
          <Items
            categories={live}
            chosen={chosen}
            shown={shown}
            total={rows.length}
            columns={columns}
            onAdded={(fresh) => {
              setRows(fresh);
              // The count on the category rail moves too.
              void load();
            }}
            onFailed={report}
          />
        </div>
      ) : tab === 'choices' ? (
        <Scroller className="mb-menu__items">
          <ModifierGroups onFailed={report} />
        </Scroller>
      ) : (
        <Scroller className="mb-menu__items">
          <Combos rows={rows} onFailed={report} />
        </Scroller>
      )}

      {editing ? (
        <EditItem
          key={editing.id}
          row={editing}
          categories={live}
          onClose={() => setEditing(null)}
          onSaved={(saved) => {
            setRows(saved);
            setEditing(null);
            void load();
          }}
          onFailed={report}
        />
      ) : null}

      <ConfirmDialog
        open={deleting !== null}
        title={deleting ? `Delete ${deleting.name}?` : 'Delete this item?'}
        body="It goes off the menu for good. An item that has been sold is marked sold out instead."
        confirmLabel="Delete"
        cancelLabel="Keep it"
        destructive
        onConfirm={() => {
          const gone = deleting;
          setDeleting(null);
          if (!gone) return;
          call('delete_menu_item', { itemId: gone.id })
            .then((fresh) => {
              setRows(fresh);
              void load();
            })
            .catch(report);
        }}
        onCancel={() => setDeleting(null)}
      />

      {madeOf ? (
        <Composition
          row={madeOf}
          onClose={() => {
            setMadeOf(null);
            void load();
          }}
          onFailed={report}
        />
      ) : null}

      {importing !== null ? (
        <ImportMenu
          path={importing}
          onClose={() => setImporting(null)}
          onDone={async (said) => {
            setImporting(null);
            toast.show('ok', said);
            await load();
          }}
          onFailed={report}
        />
      ) : null}

      {bulkOpen ? (
        <BulkPrices
          categories={live}
          chosen={chosen}
          onClose={() => setBulkOpen(false)}
          onDone={async (said) => {
            setBulkOpen(false);
            toast.show('ok', said);
            await load();
          }}
          onFailed={report}
        />
      ) : null}
    </Page>
  );
}

/** The left pane: a box to add a category, and the list under it. */
function Categories({
  categories,
  total,
  chosen,
  onChoose,
  onChanged,
  onFailed,
}: {
  categories: readonly CategoryView[];
  total: number;
  chosen: string | null;
  onChoose: (id: string | null) => void;
  onChanged: (fresh: readonly CategoryView[]) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [name, setName] = useState('');
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null);
  const [deleting, setDeleting] = useState<CategoryView | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async (id: string, wanted: string, isActive: boolean) => {
    setBusy(true);
    try {
      onChanged(await call('save_menu_category', { id, name: wanted, isActive }));
      return true;
    } catch (cause) {
      onFailed(cause);
      return false;
    } finally {
      setBusy(false);
    }
  };

  const add = async (event: FormEvent) => {
    event.preventDefault();
    const wanted = name.trim();
    if (wanted === '') return;
    // The id is ours to make and never shown.
    const id = freshId('cat');
    if (await save(id, wanted, true)) {
      setName('');
      onChoose(id);
    }
  };

  return (
    <Panel title="Categories" className="mb-menu__pane">
      <form className="mb-menu__add" onSubmit={(event) => void add(event)}>
        <Input
          aria-label="New category"
          placeholder="New category"
          value={name}
          onChange={(event) => setName(event.target.value)}
        />
        <Button type="submit" variant="primary" disabled={busy || name.trim() === ''}>
          <Icon name="plus" size="sm" />
          Add
        </Button>
      </form>

      <Scroller inset className="mb-menu__list">
        <div className="mb-menu__categories">
          <Pick className="mb-menu__category" current={chosen === null} onClick={() => onChoose(null)}>
            <span className="mb-menu__catname">Everything</span>
            <span className="mb-menu__catcount">{total}</span>
          </Pick>
          {categories.map((category) =>
            renaming?.id === category.id ? (
              <form
                key={category.id}
                className="mb-menu__rename"
                onSubmit={(event) => {
                  event.preventDefault();
                  const wanted = renaming.name.trim();
                  setRenaming(null);
                  if (wanted !== '' && wanted !== category.name) {
                    void save(category.id, wanted, true);
                  }
                }}
              >
                <Input
                  aria-label="Category name"
                  value={renaming.name}
                  autoFocus
                  onChange={(event) => setRenaming({ id: category.id, name: event.target.value })}
                  onKeyDown={(event) => {
                    if (event.key === 'Escape') setRenaming(null);
                  }}
                />
                <Button size="sm" type="submit" variant="primary">
                  Save
                </Button>
                <Button size="sm" variant="quiet" onClick={() => setRenaming(null)}>
                  Cancel
                </Button>
              </form>
            ) : (
              <div
                key={category.id}
                className="mb-pick mb-menu__catrow"
                aria-current={chosen === category.id ? 'page' : undefined}
              >
                <button
                  type="button"
                  className="mb-menu__category"
                  onClick={() => onChoose(category.id)}
                >
                  <span className="mb-menu__catname">{category.name}</span>
                  <span className="mb-menu__catcount">{category.itemCount}</span>
                </button>
                <div className="mb-menu__catdo">
                  <Button
                    size="sm"
                    variant="quiet"
                    iconOnly
                    title={`Rename ${category.name}`}
                    disabled={busy}
                    onClick={() => setRenaming({ id: category.id, name: category.name })}
                  >
                    <Icon name="pencil" size="sm" />
                  </Button>
                  <Button
                    size="sm"
                    variant="quiet"
                    iconOnly
                    title={`Delete ${category.name}`}
                    disabled={busy}
                    onClick={() => setDeleting(category)}
                  >
                    <Icon name="trash" size="sm" />
                  </Button>
                </div>
              </div>
            ),
          )}
        </div>
      </Scroller>

      <ConfirmDialog
        open={deleting !== null}
        title={deleting ? `Delete ${deleting.name}?` : 'Delete this category?'}
        body={
          deleting && deleting.itemCount > 0n
            ? `Its ${deleting.itemCount} items stay on the menu with no category.`
            : undefined
        }
        confirmLabel="Delete"
        cancelLabel="Keep it"
        destructive
        onConfirm={() => {
          const gone = deleting;
          setDeleting(null);
          if (!gone) return;
          if (chosen === gone.id) onChoose(null);
          call('delete_menu_category', { categoryId: gone.id }).then(onChanged).catch(onFailed);
        }}
        onCancel={() => setDeleting(null)}
      />
    </Panel>
  );
}

/** The right pane: a row to add an item, and the table under it. */
function Items({
  categories,
  chosen,
  shown,
  total,
  columns,
  onAdded,
  onFailed,
}: {
  categories: readonly CategoryView[];
  chosen: string | null;
  shown: readonly MenuRowView[];
  total: number;
  columns: readonly Column<MenuRowView>[];
  onAdded: (rows: readonly MenuRowView[]) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [name, setName] = useState('');
  const [price, setPrice] = useState('');
  const [categoryId, setCategoryId] = useState(chosen ?? '');
  const [busy, setBusy] = useState(false);
  const nameBox = useRef<HTMLInputElement>(null);

  // The pane follows the category rail, until somebody picks otherwise for one item.
  useEffect(() => {
    setCategoryId(chosen ?? '');
  }, [chosen]);

  const add = async (event: FormEvent) => {
    event.preventDefault();
    if (name.trim() === '') return;
    setBusy(true);
    try {
      onAdded(
        await call('save_menu_item', {
          edit: {
            id: freshId('itm'),
            name,
            categoryId: categoryId === '' ? null : categoryId,
            price,
            // Rust decides the tax: the category's rate, else the shop's.
            taxClassId: null,
            priceBasis: null,
            hsn: null,
            shortCode: null,
            cost: null,
            isOpenPrice: false,
            isAvailable: true,
            course: null,
            prepMinutes: null,
          },
        }),
      );
      setName('');
      setPrice('');
      nameBox.current?.focus();
    } catch (cause) {
      onFailed(cause);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Panel title="Items" className="mb-menu__pane">
      <form className="mb-menu__add mb-menu__add--item" onSubmit={(event) => void add(event)}>
        <Input
          ref={nameBox}
          aria-label="Item name"
          placeholder="Item name"
          value={name}
          onChange={(event) => setName(event.target.value)}
        />
        <MoneyInput
          aria-label="Price"
          placeholder="Price"
          value={price}
          onChange={setPrice}
        />
        <Select
          aria-label="Category"
          value={categoryId}
          onChange={(event) => setCategoryId(event.target.value)}
          options={[
            { value: '', label: 'No category' },
            ...categories.map((c) => ({ value: c.id, label: c.name })),
          ]}
        />
        <Button type="submit" variant="primary" disabled={busy || name.trim() === ''}>
          <Icon name="plus" size="sm" />
          Add
        </Button>
      </form>

      <Scroller className="mb-menu__list">
        {shown.length === 0 ? (
          <EmptyState
            small
            title={total === 0 ? 'No items yet' : 'Nothing here'}
            hint={total === 0 ? 'Type a name and a price above.' : 'Try another word or category.'}
          />
        ) : (
          <Table dense rows={shown} columns={columns} rowKey={(r) => r.id} />
        )}
      </Scroller>
    </Panel>
  );
}

/** One item's form: the rare fields, in a dialog. */
function EditItem({
  row,
  categories,
  onClose,
  onSaved,
  onFailed,
}: {
  row: MenuRowView;
  categories: readonly CategoryView[];
  onClose: () => void;
  onSaved: (rows: readonly MenuRowView[]) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [name, setName] = useState(row.name);
  // The price arrives preformatted from Rust and goes back as text.
  const [price, setPrice] = useState(row.price.text);
  const [cost, setCost] = useState(row.cost?.text ?? '');
  const [categoryId, setCategoryId] = useState(row.categoryId ?? '');
  const [hsn, setHsn] = useState(row.hsn ?? '');
  const [shortCode, setShortCode] = useState(row.shortCode ?? '');
  const [course, setCourse] = useState(row.course ?? '');
  const [prepMinutes, setPrepMinutes] = useState(row.prepMinutes ?? '');
  const [openPrice, setOpenPrice] = useState(row.isOpenPrice);
  const [available, setAvailable] = useState(row.isAvailable);
  const [busy, setBusy] = useState(false);

  const save = () => {
    setBusy(true);
    call('save_menu_item', {
      edit: {
        id: row.id,
        name,
        categoryId: categoryId === '' ? null : categoryId,
        price,
        taxClassId: null,
        priceBasis: null,
        hsn: hsn.trim() === '' ? null : hsn.trim(),
        shortCode: shortCode.trim() === '' ? null : shortCode.trim(),
        cost: cost.trim() === '' ? null : cost.trim(),
        isOpenPrice: openPrice,
        isAvailable: available,
        course: course.trim() === '' ? null : course.trim(),
        prepMinutes: prepMinutes.trim() === '' ? null : prepMinutes.trim(),
      },
    })
      .then(onSaved)
      .catch(onFailed)
      .finally(() => setBusy(false));
  };

  return (
    <Modal open title={row.name} onClose={onClose}>
      <form
        className="mb-menu__form"
        onSubmit={(event) => {
          event.preventDefault();
          if (name.trim() !== '') save();
        }}
      >
        <Input label="Name" value={name} autoFocus onChange={(e) => setName(e.target.value)} />
        <div className="mb-menu__pair">
          <MoneyInput label="Price" value={price} onChange={setPrice} />
          <Select
            label="Category"
            value={categoryId}
            onChange={(e) => setCategoryId(e.target.value)}
            options={[
              { value: '', label: 'No category' },
              ...categories.map((c) => ({ value: c.id, label: c.name })),
            ]}
          />
        </div>
        <p className="mb-menu__tax">GST {row.rate}</p>
        <div className="mb-menu__pair">
          <Input
            label="Short code"
            hint="Typed at the counter instead of the name."
            value={shortCode}
            onChange={(e) => setShortCode(e.target.value)}
          />
          <Input
            label="HSN / SAC"
            hint="2, 4, 6 or 8 digits, or blank."
            value={hsn}
            onChange={(e) => setHsn(e.target.value)}
          />
        </div>
        <div className="mb-menu__pair">
          <Input
            label="Minutes to cook"
            inputMode="numeric"
            value={prepMinutes}
            onChange={(e) => setPrepMinutes(e.target.value.replace(/[^0-9]/g, ''))}
          />
          <Input
            label="Course"
            hint="Starter, Main, Dessert."
            value={course}
            onChange={(e) => setCourse(e.target.value)}
          />
        </div>
        <MoneyInput
          label="What it costs you"
          hint="Only you see this; it is what makes a margin report possible."
          value={cost}
          onChange={setCost}
        />
        <Checkbox
          label="Price typed at the counter (sold by weight)"
          checked={openPrice}
          onChange={(e) => setOpenPrice(e.target.checked)}
        />
        <Checkbox
          label="On the menu"
          checked={available}
          onChange={(e) => setAvailable(e.target.checked)}
        />
        <Foot>
          <Button variant="quiet" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={busy || name.trim() === ''}>
            Save
          </Button>
        </Foot>
      </form>
    </Modal>
  );
}

/** A percentage across a category — exact to the paisa, in Rust. */
function BulkPrices({
  categories,
  chosen,
  onClose,
  onDone,
  onFailed,
}: {
  categories: readonly CategoryView[];
  chosen: string | null;
  onClose: () => void;
  onDone: (said: string) => void | Promise<void>;
  onFailed: (cause: unknown) => void;
}) {
  const [categoryId, setCategoryId] = useState(chosen ?? '');
  const [percent, setPercent] = useState('');

  return (
    <Modal open title="Change prices" onClose={onClose}>
      <Select
        label="Which items"
        value={categoryId}
        onChange={(e) => setCategoryId(e.target.value)}
        options={[
          { value: '', label: 'Everything on the menu' },
          ...categories.map((c) => ({ value: c.id, label: `${c.name} (${c.itemCount})` })),
        ]}
      />
      <Input
        label="By how much"
        hint="Per cent. Put a minus in front to bring prices down — 10, or -5."
        value={percent}
        autoFocus
        onChange={(e) => setPercent(e.target.value.replace(/[^0-9.-]/g, ''))}
      />
      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="danger"
          onClick={() => {
            call('change_menu_prices', {
              categoryId: categoryId === '' ? null : categoryId,
              percent,
            })
              .then(onDone)
              .catch(onFailed);
          }}
        >
          Change them
        </Button>
      </div>
    </Modal>
  );
}

/** How many refused rows the dialog names before it starts counting them instead. */
const REFUSALS_SHOWN = 10;

/** A list of names, the first few in full and the rest as a count. */
function Named({ label, names, tone }: { label: string; names: readonly string[]; tone?: 'danger' }) {
  if (names.length === 0) return null;
  const shown = names.slice(0, REFUSALS_SHOWN);
  const hidden = names.length - shown.length;
  return (
    <p className={tone === 'danger' ? 'mb-import__gone' : 'mb-import__added'}>
      {label}: {shown.join(', ')}
      {hidden > 0 ? ` and ${hidden} more` : ''}
    </p>
  );
}

/** The spreadsheet: what the owner wants it to do, what that would do, and one button. */
function ImportMenu({
  path,
  onClose,
  onDone,
  onFailed,
}: {
  path: string;
  onClose: () => void;
  onDone: (said: string) => void | Promise<void>;
  onFailed: (cause: unknown) => void;
}) {
  const [mode, setMode] = useState<ImportMode>('update');
  const [plan, setPlan] = useState<ImportPlanView | null>(null);
  const [busy, setBusy] = useState(false);

  // Planned in Rust for the mode chosen; nothing is written until the button is pressed.
  useEffect(() => {
    setPlan(null);
    call('plan_menu_import', { path, mode }).then(setPlan).catch(onFailed);
  }, [path, mode, onFailed]);

  // A bad file is a diagnosis, not a wall: the first few lines say what is wrong, and the
  // count says how far it goes.
  const shown = plan?.refused.slice(0, REFUSALS_SHOWN) ?? [];
  const hidden = (plan?.refused.length ?? 0) - shown.length;
  const replacing = mode === 'replace';

  return (
    <Modal open title="Import a menu" onClose={onClose} wide>
      <p className="mb-import__file">{path}</p>
      <Choice
        label="What should this file do?"
        value={mode}
        options={[
          { value: 'update', label: 'Update the menu' },
          { value: 'replace', label: 'Replace the whole menu' },
        ]}
        onPick={(picked) => setMode(picked as ImportMode)}
      />
      <p className="mb-import__shape">
        {replacing
          ? 'The file becomes the menu. Anything not in it is removed — or taken off the menu when old bills still need it.'
          : 'New items are added and items already on the menu take the file’s price. Nothing is removed.'}
      </p>

      {plan ? (
        <div className="mb-import__plan">
          <strong>{plan.summary}</strong>
          <Named label="New categories" names={plan.newCategories} />
          <Named label="Already on the menu, the file’s values win" names={plan.already} />
          <Named label="Removed" names={plan.removed} tone="danger" />
          <Named label="Taken off the menu, kept for old bills" names={plan.takenOff} tone="danger" />
          <Named label="Categories left empty and removed" names={plan.retiredCategories} tone="danger" />
          {shown.length > 0 ? (
            <ul className="mb-import__refused">
              {shown.map((line) => (
                <li key={line}>{line}</li>
              ))}
              {hidden > 0 ? <li>and {plural(hidden, 'more row')}</li> : null}
            </ul>
          ) : null}
        </div>
      ) : (
        <Spinner label="Reading the file" />
      )}

      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant={replacing ? 'danger' : 'primary'}
          disabled={busy || !plan || !plan.isClean || plan.isEmpty}
          onClick={() => {
            setBusy(true);
            call('run_menu_import', { path, mode })
              .then(onDone)
              .catch(onFailed)
              .finally(() => setBusy(false));
          }}
        >
          {replacing ? 'Replace the menu' : 'Import'}
        </Button>
      </div>
    </Modal>
  );
}
