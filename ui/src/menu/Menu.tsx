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
  Input,
  Modal,
  SearchField,
  Select,
  Table,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { CategoryView } from '../ipc/generated/CategoryView';
import type { MenuRowView } from '../ipc/generated/MenuRowView';
import type { TaxClassView } from '../ipc/generated/TaxClassView';
import type { ImportPlanView } from '../ipc/generated/ImportPlanView';

import './menu.css';

export function Menu() {
  const [categories, setCategories] = useState<readonly CategoryView[]>([]);
  const [rows, setRows] = useState<readonly MenuRowView[]>([]);
  const [classes, setClasses] = useState<readonly TaxClassView[]>([]);
  const [chosen, setChosen] = useState<string | null>(null);
  const [find, setFind] = useState('');
  const [editing, setEditing] = useState<MenuRowView | null>(null);
  const [bulkOpen, setBulkOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
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
    { key: 'name', header: 'Item', render: (r) => r.name },
    { key: 'code', header: 'Code', render: (r) => r.shortCode ?? '—' },
    {
      key: 'price',
      header: 'Price',
      numeric: true,
      render: (r) => <span className="mb-mono">{r.price.text}</span>,
    },
    // The whole point of this session, on every row.
    { key: 'tax', header: 'Tax', render: (r) => r.rate },
    { key: 'hsn', header: 'HSN', render: (r) => r.hsn ?? '—' },
    ...(showsMargin
      ? [
          {
            key: 'margin',
            header: 'Margin',
            numeric: true,
            render: (r: MenuRowView) => (
              <span className="mb-mono">{r.margin ?? '—'}</span>
            ),
          },
        ]
      : []),
    {
      key: 'available',
      header: 'On the menu',
      render: (r) =>
        r.isAvailable ? (
          <Badge tone="ok">Yes</Badge>
        ) : (
          <Badge tone="warn">86&rsquo;d</Badge>
        ),
    },
    {
      key: 'do',
      header: '',
      render: (r) => (
        <div className="mb-row">
          <Button small onClick={() => setEditing(r)}>
            Edit
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
            {r.isAvailable ? '86 it' : 'Put back'}
          </Button>
        </div>
      ),
    },
  ];

  return (
    <div className="mb-menu">
      <aside className="mb-menu__categories">
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
      </aside>

      <section className="mb-menu__items">
        <div className="mb-row">
          <SearchField
            value={find}
            placeholder="Find an item"
            onChange={(event) => setFind(event.target.value)}
          />
          <Button variant="quiet" onClick={() => setBulkOpen(true)}>
            Change prices
          </Button>
          <Button variant="quiet" onClick={() => setImportOpen(true)}>
            Import
          </Button>
          <Button
            variant="quiet"
            onClick={() => {
              call('export_menu')
                .then((text) => {
                  void navigator.clipboard.writeText(text);
                  toast.show('ok', 'The menu is on the clipboard — paste it into a spreadsheet.');
                })
                .catch(report);
            }}
          >
            Export
          </Button>
          <Button
            variant="primary"
            onClick={() =>
              setEditing({
                id: `itm_${Date.now()}`,
                name: '',
                categoryId: chosen,
                price: { paise: 0n, text: '0.00' },
                taxClassId: classes[0]?.id ?? null,
                rate: '',
                hsn: null,
                shortCode: null,
                cost: null,
                margin: null,
                isOpenPrice: false,
                isAvailable: true,
                variants: 0n,
              })
            }
          >
            Add an item
          </Button>
        </div>

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
      </section>

      {editing ? (
        <EditItem
          row={editing}
          categories={categories}
          classes={classes}
          onClose={() => setEditing(null)}
          onSaved={(saved) => {
            setRows(saved);
            setEditing(null);
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
    </div>
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
  // **The name is editable too, and it has to be.**
  //
  // The seeded names carry their rate — "Restaurant food 5%" — because that is
  // how an owner recognises one in a list. So a dialog that changed only the
  // rate left the shop with a class called "Restaurant food 5%" that charges
  // 12%, which is worse than either. Found by changing a rate and reading the
  // tile afterwards.
  const [name, setName] = useState('');
  const toast = useToast();

  return (
    <div className="mb-menu__classes">
      <h2 className="mb-menu__heading">Tax classes</h2>
      <p className="mb-muted">
        A class is your name for a rate. Change one and every item on it
        follows — bills already printed never move.
      </p>
      <div className="mb-menu__classlist">
        {classes.map((klass) => (
          <div key={klass.id} className="mb-menu__class">
            <div className="mb-stack">
              <strong>{klass.name}</strong>
              <span className="mb-muted">
                {klass.rate} · {klass.treatment} · {klass.itemsUsing} item(s)
              </span>
            </div>
            <Button
              small
              onClick={() => {
                setEditing(klass);
                setRate(klass.rate.replace('%', ''));
                setName(klass.name);
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
              : `${editing.itemsUsing} item(s) will change with it.`}
          </p>
          <Input
            label="Name"
            hint="What you call it. Most shops put the rate in the name, so change both together."
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <Input
            label="Rate"
            hint="Per cent. Liquor and exempt items stay at 0."
            value={rate}
            autoFocus
            onChange={(event) => setRate(event.target.value.replace(/[^0-9.]/g, ''))}
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
                  rate,
                  // The treatment is not editable here: changing a class from
                  // "tax added on top" to "outside GST" is a different act from
                  // changing its rate, and P17's settings screen owns it.
                  treatment: editing.treatment.includes('Outside')
                    ? 'non_gst'
                    : editing.treatment.includes('Exempt')
                      ? 'exempt'
                      : editing.treatment.includes('included')
                        ? 'inclusive'
                        : 'exclusive',
                })
                  .then(async (said) => {
                    setEditing(null);
                    toast.show('ok', said);
                    await onChanged();
                  })
                  .catch(onFailed);
              }}
            >
              Save the rate
            </Button>
          </div>
        </Modal>
      ) : null}
    </div>
  );
}

function EditItem({
  row,
  categories,
  classes,
  onClose,
  onSaved,
  onFailed,
}: {
  row: MenuRowView;
  categories: readonly CategoryView[];
  classes: readonly TaxClassView[];
  onClose: () => void;
  onSaved: (rows: readonly MenuRowView[]) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [name, setName] = useState(row.name);
  // The price arrives preformatted from Rust and goes back as text; TypeScript
  // never turns it into a number (R8, D39).
  const [price, setPrice] = useState(row.price.text);
  const [cost, setCost] = useState(row.cost?.text ?? '');
  const [categoryId, setCategoryId] = useState(row.categoryId ?? '');
  const [taxClassId, setTaxClassId] = useState(row.taxClassId ?? '');
  const [hsn, setHsn] = useState(row.hsn ?? '');
  const [shortCode, setShortCode] = useState(row.shortCode ?? '');
  const [openPrice, setOpenPrice] = useState(row.isOpenPrice);
  const [available, setAvailable] = useState(row.isAvailable);

  const save = () => {
    call('save_menu_item', {
      edit: {
        id: row.id,
        name,
        categoryId: categoryId === '' ? null : categoryId,
        price,
        taxClassId: taxClassId === '' ? null : taxClassId,
        hsn: hsn.trim() === '' ? null : hsn.trim(),
        shortCode: shortCode.trim() === '' ? null : shortCode.trim(),
        cost: cost.trim() === '' ? null : cost.trim(),
        isOpenPrice: openPrice,
        isAvailable: available,
      },
    })
      .then(onSaved)
      .catch(onFailed);
  };

  return (
    <Modal open title={row.name === '' ? 'Add an item' : row.name} onClose={onClose} wide>
      <Input label="Name" value={name} autoFocus onChange={(e) => setName(e.target.value)} />
      <Input
        label="Price"
        hint="What the customer pays, before tax is added — unless the class says tax is included."
        value={price}
        onChange={(e) => setPrice(e.target.value)}
      />
      <Select
        label="Category"
        value={categoryId}
        onChange={(e) => setCategoryId(e.target.value)}
        options={[
          { value: '', label: 'No category' },
          ...categories.map((c) => ({ value: c.id, label: c.name })),
        ]}
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
        hint="Printed on the bill. Not needed for liquor or a shop below the threshold."
        value={hsn}
        onChange={(e) => setHsn(e.target.value)}
      />
      <Input
        label="Short code"
        hint="Typed at the counter instead of the name."
        value={shortCode}
        onChange={(e) => setShortCode(e.target.value)}
      />
      {row.cost !== null || row.margin !== null || cost !== '' ? (
        <Input
          label="What it costs you"
          hint="Only you see this. It is what makes a margin report possible."
          value={cost}
          onChange={(e) => setCost(e.target.value)}
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
      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" onClick={save}>
          Save
        </Button>
      </div>
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
