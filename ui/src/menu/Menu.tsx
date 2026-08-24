/**
 * **The menu — two panes, and the screen that makes a bar billable.**
 *
 * > *"v1's menu was: category, name, price. That is all… so it could not bill a
 * > bar, an AC/non-AC outlet or anyone selling packaged goods, and it could
 * > never compute a real margin."*
 *
 * P00 built the tax engine and P13's earlier parts gave a shop a vocabulary for
 * it. This is where an owner can reach it: categories on the left, items on the
 * right, and a tax class on every row.
 *
 * # Nothing here is a control
 *
 * Every button calls a command that checks `menu.manage` in Rust — and the tax
 * classes need `settings.tax`, because a rate is what the shop owes the
 * government rather than what it charges. The rail item being hidden is a
 * courtesy; `guard::require` is the control.
 *
 * # The margin column is not always there
 *
 * Cost price is scope 4.1 and it is the owner's business. Rust does not send it
 * without `reports.view`, so this screen cannot show it even by accident —
 * hiding a column in React would have sent it anyway.
 */

import { useCallback, useEffect, useMemo, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  EmptyState,
  Foot,
  freshId,
  Icon,
  Input,
  Modal,
  MoneyInput,
  Page,
  plural,
  Scroller,
  SearchField,
  SectionHeader,
  Select,
  SideFold,
  Toolbar,
  Table,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { CategoryView } from '../ipc/generated/CategoryView';
import type { MenuRowView } from '../ipc/generated/MenuRowView';
import type { TaxClassView } from '../ipc/generated/TaxClassView';
import type { ImportPlanView } from '../ipc/generated/ImportPlanView';
import { Combos, Composition, ModifierGroups } from './Composition';
import { Groups } from './Groups';

import './menu.css';

export function Menu() {
  const [categories, setCategories] = useState<readonly CategoryView[]>([]);
  const [rows, setRows] = useState<readonly MenuRowView[]>([]);
  const [classes, setClasses] = useState<readonly TaxClassView[]>([]);
  const [chosen, setChosen] = useState<string | null>(null);
  const [find, setFind] = useState('');
  const [editing, setEditing] = useState<MenuRowView | null>(null);
  const [madeOf, setMadeOf] = useState<MenuRowView | null>(null);
  const [bulkOpen, setBulkOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
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
      setClasses(await call('menu_tax_classes'));
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
      // Sold out sits on the item, not in a column of its own — that column was
      // a green "Yes" on all twelve rows and 90px wide.
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
    // `optional`: a shop with no short codes and no HSN numbers is not given
    // two columns of em-dashes. The table drops them and draws the dash.
    { key: 'code', header: 'Code', optional: true, render: (r) => r.shortCode },
    {
      key: 'price',
      header: 'Price',
      numeric: true,
      render: (r) => <span className="mb-mono">{r.price.text}</span>,
    },
    // The whole point of this session, on every row.
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
            {/* **"Sold out", not "86 it"** (P30.5). "86" is American kitchen
                slang; the shops this is for say sold out, finished, over. §6
                is written from the cashier's side of the screen, and a word
                somebody has to be taught is a word on the wrong side. */}
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
      // **Empty, not "0.00"** — the owner, 2026-08-22: *"in menu item adding,
      // there is 0.00, but in other places empty. make it look same, no need
      // for 0.00, just keep it empty."*
      //
      // A price nobody has typed is not zero rupees, it is nothing yet, and a
      // field that opens with a number in it is a field somebody has to clear
      // before they can type. Every other new-row seed in the product already
      // used `''` (see `Composition`); this was the one that did not.
      price: { paise: 0n, text: '' },
      taxClassId: classes[0]?.id ?? null,
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
            {/* First, because it is what a shop does before it has items:
                decide what the categories are. It was not possible at all
                until P31 — `save_menu_category` had no caller.

                **It said "Groups" and that was the whole of half a bug.** The
                owner reported on 2026-08-17 that there was *"no add catogory"*.
                There was: this button. The database calls them categories, the
                item dialog's field calls them Category, the report calls them
                categories — and the one button that makes one called them
                groups. A person looking for the word they were shown
                everywhere else had no reason to press it. */}
            <Button variant="secondary" onClick={() => setGroupsOpen(true)}>
              <Icon name="plus" size="sm" />
              Categories
            </Button>
            <Button variant="secondary" onClick={() => setBulkOpen(true)}>
              Change prices
            </Button>
            <Button variant="secondary" onClick={() => setImportOpen(true)}>
              <Icon name="upload" size="sm" />
              Import
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                call('export_menu')
                  .then((text) => {
                    void navigator.clipboard.writeText(text);
                    toast.show('ok', 'The menu is on the clipboard — paste it into a spreadsheet.');
                  })
                  .catch(report);
              }}
            >
              <Icon name="download" size="sm" />
              Export
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

      {/* Adding an item is a panel, not a dialog that comes back: it stays open
          and empties itself after each item, keeping the category and the tax
          class, so a menu is typed in a run. */}
      <SideFold
        label={editing && editing.name !== '' ? editing.name : 'Add an item'}
        open={editing !== null}
        onOpen={addItem}
        onFold={() => setEditing(null)}
        panel={
          editing ? (
            <EditItem
              // A different item is a different form. While ADDING, the id
              // changes inside the form, so it is not remounted between items.
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
      {/* **No groups, no group rail** (P30.5). A shop that has not put its items
          into groups yet — which is every shop on its first day — got a
          quarter of the screen taken by one solid accent button reading
          "Everything (2)", offering a choice between one thing. */}
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
          {/* At the bottom of the rail as well as in the header, because this
              is where somebody is looking when they notice a group is wrong. */}
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
          <Table rows={shown} columns={columns} rowKey={(r) => r.id} />
        )}

        <TaxClasses classes={classes} onChanged={load} onFailed={report} />
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
            // The item counts on the rail come from the same list, but the
            // ROWS are what a renamed group changes on screen — so re-read
            // rather than patch a second copy (D4).
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

      {importOpen ? (
        <ImportMenu
          onClose={() => setImportOpen(false)}
          onDone={async (said) => {
            setImportOpen(false);
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

/** The classes, and what each one would move. */
function TaxClasses({
  classes,
  onChanged,
  onFailed,
}: {
  classes: readonly TaxClassView[];
  onChanged: () => void | Promise<void>;
  onFailed: (cause: unknown) => void;
}) {
  const [editing, setEditing] = useState<TaxClassView | null>(null);
  const [rate, setRate] = useState('');
  // **The name is editable too, and it has to be.** The seeded names carry
  // their rate — "Restaurant food 5%" — so changing only the rate leaves a
  // class called 5% that charges 12%.
  const [name, setName] = useState('');
  // The machine values, straight from Rust and straight back (P33 §5.1). No
  // screen reads the words to work out what a class is.
  const [kind, setKind] = useState<TaxClassView['kind']>('gst');
  const [basis, setBasis] = useState<TaxClassView['basis']>('exclusive');
  const toast = useToast();

  // Exempt and no-tax have no rate at all, so the box is shut rather than hinted at.
  const rateless = kind === 'exempt' || kind === 'untaxed';

  return (
    <div className="mb-menu__classes">
      <SectionHeader
        title="Tax classes"
        note="A class is your name for a rate. Change one and every item on it follows — bills already printed never move."
      />
      <div className="mb-menu__classlist">
        {classes.map((klass) => (
          <div key={klass.id} className="mb-menu__class">
            <div className="mb-stack">
              <strong>{klass.name}</strong>
              <span className="mb-muted">
                {klass.rate} · {klass.treatment} · {plural(klass.itemsUsing, 'item')}
              </span>
            </div>
            <Button
              small
              onClick={() => {
                setEditing(klass);
                setRate(klass.rate.replace('%', ''));
                setName(klass.name);
                setKind(klass.kind);
                setBasis(klass.basis);
              }}
            >
              Edit
            </Button>
          </div>
        ))}
      </div>

      {editing ? (
        <Modal open title={editing.name} onClose={() => setEditing(null)}>
          <p className="mb-muted">
            {editing.itemsUsing === 0n
              ? 'Nothing uses this yet.'
              : `${plural(editing.itemsUsing, 'item')} will change with it.`}
          </p>
          <Input
            label="Name"
            hint="What you call it. Most shops put the rate in the name, so change both together."
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <Input
            label={kind === 'outside_gst' ? 'State VAT %' : 'Rate'}
            hint={
              kind === 'outside_gst'
                ? 'Your state VAT on liquor. Not GST, and it never goes in a GST return.'
                : 'Per cent. 5, 18, 2.5.'
            }
            value={rateless ? '0' : rate}
            disabled={rateless}
            autoFocus
            onChange={(event) => setRate(event.target.value.replace(/[^0-9.]/g, ''))}
          />
          <Select
            label="Kind"
            hint="What it is in the law. Liquor is outside GST and carries state VAT instead."
            value={kind}
            onChange={(event) => setKind(event.target.value as TaxClassView['kind'])}
            options={[
              { value: 'gst', label: 'GST' },
              { value: 'exempt', label: 'Exempt' },
              { value: 'outside_gst', label: 'Outside GST (VAT)' },
              { value: 'untaxed', label: 'No tax' },
            ]}
          />
          <Select
            label="Price basis"
            hint="Whether the price you type already contains the tax. Bar menus usually do."
            value={basis}
            onChange={(event) => setBasis(event.target.value as TaxClassView['basis'])}
            options={[
              { value: 'exclusive', label: 'Tax added on top' },
              { value: 'inclusive', label: 'Tax already in the price' },
            ]}
          />
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setEditing(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                call('save_tax_class', {
                  id: editing.id,
                  name,
                  rate: rateless ? '0' : rate,
                  kind,
                  basis,
                })
                  .then(async (said) => {
                    setEditing(null);
                    toast.show('ok', said);
                    await onChanged();
                  })
                  .catch(onFailed);
              }}
            >
              Save
            </Button>
          </div>
        </Modal>
      ) : null}
    </div>
  );
}

/**
 * **One item's form, in the side panel** — and the thing that makes typing a
 * whole menu bearable.
 *
 * The owner, 2026-08-24: *"If the user has to add many items, he has to click
 * add buton and it pops up many times, it is tedious."* So while it is ADDING,
 * saving does not close it: it takes a new id, empties the boxes that belong to
 * one dish (name, price, code, HSN, cost) and keeps the ones that belong to a
 * RUN of them (category, tax class, course, minutes, the two ticks). Enter
 * saves, so a menu goes in as name, price, Enter, name, price, Enter.
 *
 * Editing an existing item closes when it is saved, because there is nothing to
 * type next.
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
  classes: readonly TaxClassView[];
  onClose: () => void;
  /** The fresh list. **It does not close the panel** — see above. */
  onSaved: (rows: readonly MenuRowView[]) => void;
  /** The whole new list, straight from Rust — see `AddCategory`. */
  onCategoriesChanged: (fresh: readonly CategoryView[]) => void;
  onFailed: (cause: unknown) => void;
}) {
  /** True for the whole life of this form: it was opened by the + button. */
  const adding = row.name === '';
  /**
   * The id being saved to. It changes after each add, which is what makes the
   * next Save a new item rather than an edit of the one just made.
   */
  const [id, setId] = useState(row.id);
  const toast = useToast();
  const [name, setName] = useState(row.name);
  // The price arrives preformatted from Rust and goes back as text; TypeScript
  // never turns it into a number (R8, D39).
  const [price, setPrice] = useState(row.price.text);
  const [cost, setCost] = useState(row.cost?.text ?? '');
  const [categoryId, setCategoryId] = useState(row.categoryId ?? '');
  const [taxClassId, setTaxClassId] = useState(row.taxClassId ?? '');
  const [hsn, setHsn] = useState(row.hsn ?? '');
  const [shortCode, setShortCode] = useState(row.shortCode ?? '');
  // P24 — what the kitchen screen needs to know about this dish.
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
        // **Ready for the next one.** What belongs to this dish is cleared;
        // what belongs to the run it is part of stays.
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
      // Not a `<form>`: a form in a webview navigates on submit, and there is
      // nowhere to navigate to.
      onKeyDown={(event) => {
        if (event.key === 'Enter' && name.trim() !== '') save();
      }}
    >
      {/* No heading here — the panel draws it, from the same `label`. */}
      {/* Keyed on the id, so it empties AND takes the cursor back after each
          item added — the autofocus runs again on the new box. */}
      <Input
        key={id}
        label="Name"
        value={name}
        autoFocus
        onChange={(e) => setName(e.target.value)}
      />
      <MoneyInput
        label="Price"
        hint="What the customer pays, before tax is added — unless the class says tax is included."
        value={price}
        onChange={setPrice}
      />
      {/* **The category, and a way to MAKE one** — the owner, 2026-08-17:
          *"there is no catogory selection option while menu adding, and there
          is no add catogory."*

          Half of that was a naming problem and is fixed on the page behind
          this dialog: categories were called "Groups", so somebody looking for
          categories did not find the button that manages them.

          The other half was real and is fixed here. The selector existed, but
          on a new shop its only option is "No category" — and the one moment a
          person knows what their categories are is the moment they are typing
          their first item into this box. Sending them out of a half-filled
          dialog to find another screen is how the list ends up empty forever.
          `save_menu_category` is the same command the Categories dialog calls;
          this is a second door onto it, not a second implementation. */}
      <AddCategory
        categories={categories}
        chosen={categoryId}
        onChoose={setCategoryId}
        onChanged={onCategoriesChanged}
        onFailed={onFailed}
      />
      <Select
        label="Tax class"
        hint="What this is taxed at. Liquor sits outside GST entirely."
        value={taxClassId}
        onChange={(e) => setTaxClassId(e.target.value)}
        options={[
          { value: '', label: 'Leave as it is' },
          ...classes.map((c) => ({ value: c.id, label: `${c.name} — ${c.rate}` })),
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
      {/* P24 — the kitchen screen. Both are optional, and a shop that leaves
          them blank gets a screen that still works: no course means the whole
          order fires at once, and no target means the ticket never turns
          late. */}
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

/**
 * **The category selector, with a way to make one** — 2026-08-17.
 *
 * Two states in one field. Normally it is the `Select` that was always here,
 * with a "New" button beside it; press that and the select becomes a name box
 * and an Add button. Adding calls `save_menu_category`, hands the whole new
 * list back to the screen (D4 — one list, no second copy), and **selects what
 * was just made**, because a person who has stopped to name a category was
 * always going to put this item in it.
 *
 * It is a field rather than a dialog on purpose: a second modal over the item
 * modal, to type one word, is how a shopkeeper loses their place.
 */
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
      // The id is ours to make and never shown — the same shape `Groups` uses,
      // because `save_menu_category` upserts on it and a fresh one is what
      // makes this an add rather than a rename of whatever was there.
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
      <p className="mb-muted">
        Every price is worked out in Rust, to the paisa, and every change goes
        into the history with your name on it.
      </p>
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

/**
 * **The spreadsheet** — P13 item 7, and the reason an owner ever finishes
 * setting up.
 *
 * The dry run is the feature: paste or open a file, see *"312 new item(s) and
 * 88 change(s)"* — or every bad line by number — and only then decide. Rust
 * writes nothing until the second button.
 */
function ImportMenu({
  onClose,
  onDone,
  onFailed,
}: {
  onClose: () => void;
  onDone: (said: string) => void | Promise<void>;
  onFailed: (cause: unknown) => void;
}) {
  const [csv, setCsv] = useState('');
  const [plan, setPlan] = useState<ImportPlanView | null>(null);

  const look = () => {
    setPlan(null);
    call('plan_menu_import', { csv })
      .then(setPlan)
      .catch(onFailed);
  };

  return (
    <Modal open title="Import a menu" onClose={onClose} wide>
      <p className="mb-muted">
        Paste a spreadsheet here — the first line names the columns. Export the
        menu first if you want the shape. Nothing is written until you have seen
        what it would do.
      </p>
      <textarea
        className="mb-import__box"
        value={csv}
        onChange={(event) => {
          setCsv(event.target.value);
          setPlan(null);
        }}
        placeholder="name,price_paise,tax_class"
        aria-label="The spreadsheet"
      />

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
      ) : null}

      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button onClick={look} disabled={csv.trim() === ''}>
          See what it would do
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
