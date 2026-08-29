/** The menu — two panes, and the screen that makes a bar billable. */

import { useCallback, useEffect, useMemo, useState } from 'react';

import { Badge, Button, Checkbox, EmptyState, Foot, freshId, Icon, Input, Modal, MoneyInput, Page, Scroller, SearchField, Select, SideFold, Toolbar, Table, useToast, type Column, Spinner } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { CategoryView } from '../ipc/generated/CategoryView';
import type { MenuRowView } from '../ipc/generated/MenuRowView';
import type { TaxSlabView } from '../ipc/generated/TaxSlabView';
import type { ImportPlanView } from '../ipc/generated/ImportPlanView';
import { Combos, Composition, ModifierGroups } from './Composition';
import { Groups } from './Groups';

import './menu.css';

export function Menu() {
  const [categories, setCategories] = useState<readonly CategoryView[]>([]);
  const [rows, setRows] = useState<readonly MenuRowView[]>([]);
  const [classes, setClasses] = useState<readonly TaxSlabView[]>([]);
  const [chosen, setChosen] = useState<string | null>(null);
  const [find, setFind] = useState('');
  const [editing, setEditing] = useState<MenuRowView | null>(null);
  const [madeOf, setMadeOf] = useState<MenuRowView | null>(null);
  const [bulkOpen, setBulkOpen] = useState(false);
  /** The spreadsheet somebody chose, waiting to be looked at. */
  const [importing, setImporting] = useState<string | null>(null);
  const [groupsOpen, setGroupsOpen] = useState(false);
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
      setClasses(await call('tax_slabs'));
    } catch (cause) {
      report(cause);
    }
  }, [report]);

  useEffect(() => {
    void load();
  }, [load]);

  // By item 300 the list is the problem, so there is a search box.
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

  const showsMargin = rows.some((row) => row.margin !== null);

  const columns: Column<MenuRowView>[] = [
    {
      key: 'name',
      header: 'Item',
      // Sold out sits on the item, not in a column of its own — that column was a green "Yes"
      // on all twelve rows and 90px wide.
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
    // `optional`: a shop with no short codes and no HSN numbers is not given two columns of
    // em-dashes.
    { key: 'code', header: 'Code', optional: true, render: (r) => r.shortCode },
    {
      key: 'price',
      header: 'Price',
      numeric: true,
      render: (r) => <span className="mb-mono">{r.price.text}</span>,
    },
    { key: 'tax', header: 'Tax', render: (r) => r.rate },
    { key: 'hsn', header: 'HSN', optional: true, render: (r) => r.hsn },
    ...(showsMargin
      ? [
          {
            key: 'margin',
            header: 'Margin',
            numeric: true,
            optional: true,
            render: (r: MenuRowView) =>
              r.margin === null ? null : <span className="mb-mono">{r.margin}</span>,
          },
        ]
      : []),
    {
      key: 'do',
      header: '',
      render: (r) => (
        <div className="mb-row">
          <Button small onClick={() => setEditing(r)}>
            Edit
          </Button>
          <Button small variant="quiet" onClick={() => setMadeOf(r)}>
            Sizes &amp; choices{r.variants > 0n ? ` (${r.variants})` : ''}
          </Button>
          <Button
            small
            variant="quiet"
            onClick={() => {
              call('set_item_available', { itemId: r.id, available: !r.isAvailable })
                .then(setRows)
                .catch(report);
            }}
          >
            {/* "Sold out", not "86 it". */}
            {r.isAvailable ? 'Sold out' : 'Put back'}
          </Button>
        </div>
      ),
    },
  ];

  const addItem = () =>
    setEditing({
      id: freshId('itm'),
      name: '',
      categoryId: chosen,
      // Empty, not "0.00".
      price: { paise: 0n, text: '' },
      // The category's starting slab, else the first live one — Settings › Tax decides both.
      taxClassId:
        categories.find((c) => c.id === chosen)?.defaultSlabId ??
        classes.find((c) => c.isActive)?.id ??
        '',
      priceBasis: 'shop',
      rate: '',
      hsn: null,
      shortCode: null,
      cost: null,
      margin: null,
      isOpenPrice: false,
      isAvailable: true,
      course: null,
      prepMinutes: null,
      variants: 0n,
    });

  return (
    <Page className="mb-menu">
      <Toolbar
        end={
          <>
            {/*
              First, because it is what a shop does before it has items: decide what the
              categories are.
            */}
            <Button variant="secondary" onClick={() => setGroupsOpen(true)}>
              <Icon name="plus" size="sm" />
              Categories
            </Button>
            <Button variant="secondary" onClick={() => setBulkOpen(true)}>
              Change prices
            </Button>
            {/* A label wearing the kit's button, over a hidden file input — one way in. */}
            <label className="mb-button mb-button--secondary">
              <Icon name="upload" size="sm" />
              Import a file
              <input
                className="mb-visually-hidden"
                type="file"
                accept="text/csv,.csv,.txt"
                onChange={(event) => {
                  const file = event.currentTarget.files?.[0];
                  event.currentTarget.value = '';
                  if (file) file.text().then(setImporting).catch(report);
                }}
              />
            </label>
            <Button
              variant="secondary"
              onClick={() => {
                call('export_menu')
                  .then((path) => toast.show('ok', 'The menu was saved as a spreadsheet.', path))
                  .catch(report);
              }}
            >
              <Icon name="download" size="sm" />
              Save as a file
            </Button>
          </>
        }
      >
        <div className="mb-menu__find">
          <SearchField
            value={find}
            placeholder="Find an item"
            onChange={(event) => setFind(event.target.value)}
          />
        </div>
      </Toolbar>

      {/*
        Adding an item is a panel, not a dialog that comes back: it stays open and empties
        itself after each item, keeping the category and the tax class, so a menu is typed in a
        run.
      */}
      <SideFold
        label={editing && editing.name !== '' ? editing.name : 'Add an item'}
        open={editing !== null}
        onOpen={addItem}
        onFold={() => setEditing(null)}
        panel={
          editing ? (
            <EditItem
              // A different item is a different form.
              key={editing.id}
              row={editing}
              categories={categories}
              classes={classes}
              onClose={() => setEditing(null)}
              onSaved={(saved) => {
                setRows(saved);
                void load();
              }}
              onCategoriesChanged={setCategories}
              onFailed={report}
            />
          ) : null
        }
      >
      <div
        className={[
          'mb-menu__body',
          categories.length === 0 ? 'mb-menu__body--nogroups' : '',
        ]
          .filter(Boolean)
          .join(' ')}
      >
      {/* No groups, no group rail. */}
      {categories.length > 0 ? (
        <Scroller inset className="mb-menu__categories">
          <Button
            variant={chosen === null ? 'primary' : 'quiet'}
            wide
            onClick={() => setChosen(null)}
          >
            Everything ({rows.length})
          </Button>
          {categories.map((category) => (
            <Button
              key={category.id}
              variant={chosen === category.id ? 'primary' : 'quiet'}
              wide
              onClick={() => setChosen(category.id)}
            >
              {category.name} ({category.itemCount})
            </Button>
          ))}
          {/*
            At the bottom of the rail as well as in the header, because this is where somebody
            is looking when they notice a group is wrong.
          */}
          <Button variant="quiet" wide onClick={() => setGroupsOpen(true)}>
            <Icon name="settings" size="sm" />
            Edit categories
          </Button>
        </Scroller>
      ) : null}

      <Scroller className="mb-menu__items">
        {shown.length === 0 ? (
          <EmptyState
            title={rows.length === 0 ? 'No items yet' : 'Nothing matches'}
            body={
              rows.length === 0
                ? 'Add what you sell, and give each thing its tax class — that is what lets this bill a bar as well as a restaurant.'
                : 'Try a different word, or pick another category.'
            }
          />
        ) : (
          <Table dense rows={shown} columns={columns} rowKey={(r) => r.id} />
        )}

        <ModifierGroups onFailed={report} />
        <Combos rows={rows} onFailed={report} />
      </Scroller>
      </div>
      </SideFold>

      {groupsOpen ? (
        <Groups
          categories={categories}
          onChanged={(fresh) => {
            setCategories(fresh);
            // The item counts on the rail come from the same list, but the ROWS are what a
            // renamed group changes on screen — so re-read rather than patch a second copy.
            void load();
          }}
          onClose={() => setGroupsOpen(false)}
          onFailed={report}
        />
      ) : null}

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
          csv={importing}
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
          categories={categories}
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

/**
 * One item's form, in the side panel — and the thing that makes typing a whole menu bearable.
 */
function EditItem({
  row,
  categories,
  classes,
  onClose,
  onSaved,
  onCategoriesChanged,
  onFailed,
}: {
  row: MenuRowView;
  categories: readonly CategoryView[];
  classes: readonly TaxSlabView[];
  onClose: () => void;
  /** The fresh list. It does not close the panel — see above. */
  onSaved: (rows: readonly MenuRowView[]) => void;
  /** The whole new list, straight from Rust — see `AddCategory`. */
  onCategoriesChanged: (fresh: readonly CategoryView[]) => void;
  onFailed: (cause: unknown) => void;
}) {
  /** True for the whole life of this form: it was opened by the + button. */
  const adding = row.name === '';
  /** The id being saved to. */
  const [id, setId] = useState(row.id);
  const toast = useToast();
  const [name, setName] = useState(row.name);
  // The price arrives preformatted from Rust and goes back as text; TypeScript never turns it
  // into a number.
  const [price, setPrice] = useState(row.price.text);
  const [cost, setCost] = useState(row.cost?.text ?? '');
  const [categoryId, setCategoryId] = useState(row.categoryId ?? '');
  const [taxClassId, setTaxClassId] = useState(row.taxClassId);
  const [priceBasis, setPriceBasis] = useState(row.priceBasis);
  const [hsn, setHsn] = useState(row.hsn ?? '');
  const [shortCode, setShortCode] = useState(row.shortCode ?? '');
  // What the kitchen screen needs to know about this dish.
  const [course, setCourse] = useState(row.course ?? '');
  const [prepMinutes, setPrepMinutes] = useState(row.prepMinutes ?? '');
  const [openPrice, setOpenPrice] = useState(row.isOpenPrice);
  const [available, setAvailable] = useState(row.isAvailable);

  const save = () => {
    call('save_menu_item', {
      edit: {
        id,
        name,
        categoryId: categoryId === '' ? null : categoryId,
        price,
        taxClassId: taxClassId === '' ? null : taxClassId,
        priceBasis,
        hsn: hsn.trim() === '' ? null : hsn.trim(),
        shortCode: shortCode.trim() === '' ? null : shortCode.trim(),
        cost: cost.trim() === '' ? null : cost.trim(),
        isOpenPrice: openPrice,
        isAvailable: available,
        course: course.trim() === '' ? null : course.trim(),
        prepMinutes: prepMinutes.trim() === '' ? null : prepMinutes.trim(),
      },
    })
      .then((saved) => {
        onSaved(saved);
        if (!adding) {
          onClose();
          return;
        }
        // Ready for the next one.
        toast.show('ok', `${name} added. Type the next one.`);
        setId(freshId('itm'));
        setName('');
        setPrice('');
        setShortCode('');
        setHsn('');
        setCost('');
      })
      .catch(onFailed);
  };

  return (
    <div
      className="mb-menu__form"
      // Enter is how a run of items is typed — see the note on this component.
      onKeyDown={(event) => {
        if (event.key === 'Enter' && name.trim() !== '') save();
      }}
    >
      {/* No heading here — the panel draws it, from the same `label`. */}
      {/*
        Keyed on the id, so it empties AND takes the cursor back after each item added — the
        autofocus runs again on the new box.
      */}
      <Input
        key={id}
        label="Name"
        value={name}
        autoFocus
        onChange={(e) => setName(e.target.value)}
      />
      <MoneyInput
        label="Price"
        hint="The menu price. Whether tax is inside it is the slab's and the shop's rule, unless this item says otherwise below."
        value={price}
        onChange={setPrice}
      />
      {/* The category, and a way to MAKE one. */}
      <AddCategory
        categories={categories}
        chosen={categoryId}
        onChoose={setCategoryId}
        onChanged={onCategoriesChanged}
        onFailed={onFailed}
      />
      {/* A slab, never a rate: Settings › Tax decides what "GST 5%" means. */}
      <Select
        label="Tax slab"
        hint="Defined under Settings › Tax. Liquor sits outside GST entirely."
        value={taxClassId}
        onChange={(e) => setTaxClassId(e.target.value)}
        options={[
          ...(adding ? [] : [{ value: '', label: 'Leave as it is' }]),
          ...classes
            .filter((c) => c.isActive || c.id === row.taxClassId)
            .map((c) => ({ value: c.id, label: c.isActive ? c.name : `${c.name} (removed)` })),
        ]}
      />
      <Select
        label="Tax in the price"
        hint="Shop default follows Settings › Tax. Say otherwise only for this item — an MRP bottle on a shop that adds tax on top."
        value={priceBasis}
        onChange={(e) => setPriceBasis(e.target.value)}
        options={[
          { value: 'shop', label: 'Shop default' },
          { value: 'inclusive', label: 'In the price' },
          { value: 'exclusive', label: 'Added on top' },
        ]}
      />
      <Input
        label="HSN / SAC"
        hint="Printed on the bill. 2, 4, 6 or 8 digits, or leave it blank."
        value={hsn}
        onChange={(e) => setHsn(e.target.value)}
      />
      <Input
        label="Short code"
        hint="Typed at the counter instead of the name."
        value={shortCode}
        onChange={(e) => setShortCode(e.target.value)}
      />
      {/* The kitchen screen. */}
      <Input
        label="Course"
        hint="Starter, Main, Dessert — or leave blank if you send the whole order together."
        value={course}
        onChange={(e) => setCourse(e.target.value)}
      />
      <Input
        label="Minutes to cook"
        hint="The kitchen screen turns this ticket red after this long. Leave blank for no target."
        value={prepMinutes}
        onChange={(e) => setPrepMinutes(e.target.value)}
      />
      {row.cost !== null || row.margin !== null || cost !== '' ? (
        <MoneyInput
          label="What it costs you"
          hint="Only you see this. It is what makes a margin report possible."
          value={cost}
          onChange={setCost}
        />
      ) : null}
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
          {adding ? 'Done' : 'Cancel'}
        </Button>
        <Button variant="primary" onClick={save} disabled={name.trim() === ''}>
          {adding ? 'Add it' : 'Save'}
        </Button>
      </Foot>
    </div>
  );
}

/** The category selector, with a way to make one. */
function AddCategory({
  categories,
  chosen,
  onChoose,
  onChanged,
  onFailed,
}: {
  categories: readonly CategoryView[];
  chosen: string;
  onChoose: (id: string) => void;
  onChanged: (fresh: readonly CategoryView[]) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);

  const add = async () => {
    const wanted = name.trim();
    if (wanted === '') return;
    setBusy(true);
    try {
      // The id is ours to make and never shown — the same shape `Groups` uses, because
      // `save_menu_category` upserts on it and a fresh one is what makes this an add rather
      // than a rename of whatever was there.
      const id = freshId('cat');
      onChanged(await call('save_menu_category', { id, name: wanted, isActive: true }));
      onChoose(id);
      setName('');
      setNaming(false);
    } catch (cause) {
      onFailed(cause);
    } finally {
      setBusy(false);
    }
  };

  if (naming) {
    return (
      <div className="mb-catadd">
        <Input
          label="New category"
          hint="Tiffin, Drinks, Tandoor — how this shop arranges its menu."
          value={name}
          autoFocus
          placeholder="Tiffin"
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') void add();
            if (event.key === 'Escape') setNaming(false);
          }}
        />
        <Button variant="primary" disabled={busy || name.trim() === ''} onClick={() => void add()}>
          <Icon name="plus" size="sm" />
          Add
        </Button>
        <Button variant="quiet" disabled={busy} onClick={() => setNaming(false)}>
          Cancel
        </Button>
      </div>
    );
  }

  return (
    <div className="mb-catadd">
      <Select
        label="Category"
        value={chosen}
        onChange={(e) => onChoose(e.target.value)}
        options={[
          { value: '', label: 'No category' },
          ...categories.map((c) => ({ value: c.id, label: c.name })),
        ]}
      />
      <Button onClick={() => setNaming(true)}>
        <Icon name="plus" size="sm" />
        New
      </Button>
    </div>
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

/** The spreadsheet. */
function ImportMenu({
  csv,
  onClose,
  onDone,
  onFailed,
}: {
  csv: string;
  onClose: () => void;
  onDone: (said: string) => void | Promise<void>;
  onFailed: (cause: unknown) => void;
}) {
  const [plan, setPlan] = useState<ImportPlanView | null>(null);

  // Looked at as soon as it is chosen; nothing is written until Import is pressed.
  useEffect(() => {
    call('plan_menu_import', { csv }).then(setPlan).catch(onFailed);
  }, [csv, onFailed]);

  return (
    <Modal open title="Import a menu" onClose={onClose} wide>
      {plan ? (
        <div className="mb-import__plan">
          <strong>{plan.summary}</strong>
          {plan.refused.length > 0 ? (
            <ul className="mb-import__refused">
              {plan.refused.map((line) => (
                <li key={line}>{line}</li>
              ))}
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
          variant="primary"
          disabled={!plan?.isClean || plan.newItems + plan.updatedItems === 0n}
          onClick={() => {
            call('run_menu_import', { csv })
              .then(onDone)
              .catch(onFailed);
          }}
        >
          Import
        </Button>
      </div>
    </Modal>
  );
}
