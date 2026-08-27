/** Money going out, and what should be in the drawer — scope 10.6. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  EmptyState,
  freshId,
  Icon,
  Input,
  Modal,
  MoneyInput,
  Page,
  PageHeader,
  Select,
  Table,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { ExpenseRowView } from '../ipc/generated/ExpenseRowView';
import type { ExpensesView } from '../ipc/generated/ExpensesView';

import './expenses.css';

const MODES = [
  { value: 'cash', label: 'Cash' },
  { value: 'bank', label: 'Bank' },
  { value: 'upi', label: 'UPI' },
  { value: 'card', label: 'Card' },
];

export function Expenses() {
  const [view, setView] = useState<ExpensesView | null>(null);
  const [what, setWhat] = useState('');
  const [amount, setAmount] = useState('');
  const [category, setCategory] = useState('');
  const [mode, setMode] = useState('cash');
  const [editing, setEditing] = useState<ExpenseRowView | null>(null);
  const [drawer, setDrawer] = useState(false);
  /** The categories themselves, which nothing could add to. */
  const [editingCategories, setEditingCategories] = useState(false);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    call('expenses').then(setView).catch(report);
  }, [report]);

  useEffect(load, [load]);

  const quickAdd = () => {
    if (what.trim() === '' || amount.trim() === '') return;
    call('save_expense', {
      edit: {
        id: freshId('exp'),
        categoryId: category === '' ? null : category,
        description: what,
        amount,
        mode,
        paidTo: '',
        reference: '',
        gstPercent: '',
        note: '',
      },
    })
      .then((fresh) => {
        setView(fresh);
        setWhat('');
        setAmount('');
      })
      .catch(report);
  };

  if (!view) return <div className="mb-expenses" />;

  const columns: Column<ExpenseRowView>[] = [
    { key: 'what', header: 'What', render: (r) => r.description },
    { key: 'category', header: 'Category', render: (r) => r.category },
    { key: 'paidTo', header: 'Paid to', render: (r) => r.paidTo ?? '—' },
    {
      key: 'mode',
      header: 'How',
      render: (r) =>
        r.modeTag === 'cash' ? <Badge tone="warn">Cash</Badge> : <Badge tone="neutral">{r.mode}</Badge>,
    },
    {
      key: 'gst',
      header: 'Input credit',
      render: (r) => r.inputCredit ?? '—',
    },
    {
      key: 'amount',
      header: 'Amount',
      numeric: true,
      render: (r) => <span className="mb-mono">{r.amount.text}</span>,
    },
    {
      key: 'do',
      header: '',
      render: (r) => (
        <div className="mb-row">
          <Button small variant="quiet" onClick={() => setEditing(r)}>
            Edit
          </Button>
          <Button
            small
            variant="quiet"
            onClick={() => {
              call('delete_expense', { id: r.id }).then(setView).catch(report);
            }}
          >
            Delete
          </Button>
        </div>
      ),
    },
  ];

  return (
    <Page className="mb-expenses">
      <PageHeader
        title="Spends"
        count={view.rows.length}
        actions={
          <Button variant="secondary" onClick={() => setDrawer(true)}>
            <Icon name="cash" size="sm" />
            Move cash
          </Button>
        }
      />

      {/* Two fields and Enter. */}
      <div className="mb-row mb-expenses__quick">
        <Input
          label="What"
          value={what}
          autoFocus
          onChange={(e) => setWhat(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') quickAdd();
          }}
        />
        <MoneyInput
          label="Amount"
          value={amount}
          onChange={setAmount}
          onKeyDown={(e) => {
            if (e.key === 'Enter') quickAdd();
          }}
        />
        <Select
          label="Category"
          value={category}
          onChange={(e) => setCategory(e.target.value)}
          options={[
            { value: '', label: 'No category' },
            ...view.allCategories.map((c) => ({ value: c.id ?? '', label: c.name })),
          ]}
        />
        <Select
          label="How"
          value={mode}
          onChange={(e) => setMode(e.target.value)}
          options={MODES}
        />
        <Button variant="primary" onClick={quickAdd}>
          Record it
        </Button>
        {/*
          Beside the box that uses them, and this is a bug fixed by looking: it was first put in
          the totals row, which only draws once a shop has spent something TODAY — so on a quiet
          morning, and on every shop's first day, the button was not there at all.
        */}
        <Button variant="quiet" onClick={() => setEditingCategories(true)}>
          Categories
        </Button>
      </div>

      {/*
        6's cash position — the number a drawer is counted against, said as a sentence Rust
        wrote.
      */}
      <div className="mb-expenses__cash">
        <span>
          In the drawer{' '}
          <strong className="mb-mono">{view.cash.expected.text}</strong>
        </span>
        <span className="mb-expenses__sum">{view.cash.says}</span>
      </div>

      {view.due.length > 0 ? (
        <div className="mb-expenses__due">
          <strong>Due</strong>
          {view.due.map((due) => (
            <span key={due.id} className="mb-expenses__reminder">
              {due.description} {due.amount.text} ({due.when})
              <Button
                small
                onClick={() => {
                  call('confirm_recurring_expense', { id: due.id })
                    .then((fresh) => {
                      setView(fresh);
                      toast.show('ok', 'Recorded.');
                    })
                    .catch(report);
                }}
              >
                Record it
              </Button>
            </span>
          ))}
        </div>
      ) : null}

      {view.rows.length === 0 ? (
        <EmptyState
          title="Nothing spent today"
          body="Record what goes out as it goes out — that is what makes the profit figure true."
        />
      ) : (
        <>
          <Table rows={[...view.rows]} columns={columns} rowKey={(r) => r.id} />
          <div className="mb-expenses__totals">
            <span>
              Today <strong className="mb-mono">{view.total.text}</strong>
            </span>
            <span>
              This month <span className="mb-mono">{view.thisMonth.text}</span>
            </span>
            <span>
              Last month <span className="mb-mono">{view.lastMonth.text}</span>
            </span>
            {view.categories.map((c) => (
              <span key={c.name}>
                {c.name} <span className="mb-mono">{c.total.text}</span>
              </span>
            ))}
            <Button
              small
              variant="quiet"
              onClick={() => {
                call('export_expenses')
                  .then((csv) => {
                    void navigator.clipboard.writeText(csv);
                    toast.show('ok', "Today's expenses are on the clipboard.");
                  })
                  .catch(report);
              }}
            >
              Export
            </Button>
          </div>
        </>
      )}

      {view.movements.length > 0 ? (
        <div className="mb-expenses__movements">
          <strong>The drawer today</strong>
          {view.movements.map((m) => (
            <span key={m.id}>
              {m.kind} {m.takesOut ? '−' : '+'}
              <span className="mb-mono">{m.amount.text}</span> — {m.reason}
            </span>
          ))}
        </div>
      ) : null}

      {drawer ? (
        <MoveCash
          onClose={() => setDrawer(false)}
          onDone={(fresh) => {
            setView(fresh);
            setDrawer(false);
          }}
          onFailed={report}
        />
      ) : null}

      {editingCategories && view ? (
        <SpendCategories
          view={view}
          onClose={() => setEditingCategories(false)}
          onChanged={setView}
          onFailed={report}
        />
      ) : null}

      {editing ? (
        <EditExpense
          row={editing}
          view={view}
          onClose={() => setEditing(null)}
          onSaved={(fresh) => {
            setView(fresh);
            setEditing(null);
          }}
          onFailed={report}
        />
      ) : null}
    </Page>
  );
}

function MoveCash({
  onClose,
  onDone,
  onFailed,
}: {
  onClose: () => void;
  onDone: (view: ExpensesView) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [kind, setKind] = useState('payout');
  const [amount, setAmount] = useState('');
  const [reason, setReason] = useState('');

  return (
    <Modal
      open
      title="Move cash"
      note="The float you started with, a top-up from the owner, a payout, or money taken to the bank. A purchase is not one of these — record that as an expense and the drawer follows it."
      onClose={onClose}
    >
      <Select
        label="What happened"
        value={kind}
        onChange={(e) => setKind(e.target.value)}
        options={[
          { value: 'float', label: 'Opening float' },
          { value: 'top_up', label: 'Top-up into the drawer' },
          { value: 'payout', label: 'Payout from the drawer' },
          { value: 'bank_drop', label: 'Taken to the bank' },
        ]}
      />
      <MoneyInput label="Amount" value={amount} onChange={setAmount} />
      <Input
        label="Why"
        hint="Money leaving a drawer without a reason is how a shortfall becomes an argument."
        value={reason}
        onChange={(e) => setReason(e.target.value)}
      />
      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="primary"
          onClick={() => {
            call('save_cash_movement', { kind, amount, reason })
              .then(onDone)
              .catch(onFailed);
          }}
        >
          Record it
        </Button>
      </div>
    </Modal>
  );
}

function EditExpense({
  row,
  view,
  onClose,
  onSaved,
  onFailed,
}: {
  row: ExpenseRowView;
  view: ExpensesView;
  onClose: () => void;
  onSaved: (view: ExpensesView) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [what, setWhat] = useState(row.description);
  const [amount, setAmount] = useState(row.amount.text);
  const [category, setCategory] = useState(row.categoryId ?? '');
  const [mode, setMode] = useState(row.modeTag);
  const [paidTo, setPaidTo] = useState(row.paidTo ?? '');
  const [reference, setReference] = useState(row.reference ?? '');
  const [gst, setGst] = useState('');
  const [note, setNote] = useState(row.note ?? '');
  const [recurring, setRecurring] = useState(false);

  return (
    <Modal open title={row.description} onClose={onClose} wide>
      <Input label="What" value={what} autoFocus onChange={(e) => setWhat(e.target.value)} />
      <MoneyInput label="Amount" value={amount} onChange={setAmount} />
      <Select
        label="Category"
        value={category}
        onChange={(e) => setCategory(e.target.value)}
        options={[
          { value: '', label: 'No category' },
          ...view.allCategories.map((c) => ({ value: c.id ?? '', label: c.name })),
        ]}
      />
      <Select label="How" value={mode} onChange={(e) => setMode(e.target.value)} options={MODES} />
      <Input label="Paid to" value={paidTo} onChange={(e) => setPaidTo(e.target.value)} />
      <Input
        label="Their bill number"
        value={reference}
        onChange={(e) => setReference(e.target.value)}
      />
      <Input
        label="GST %"
        hint="The tax INSIDE what you paid — 1,180 at 18% contains 180 you can claim. Blank for none."
        value={gst}
        onChange={(e) => setGst(e.target.value)}
      />
      <Input label="Note" value={note} onChange={(e) => setNote(e.target.value)} />
      <Checkbox
        label="This happens every month — remind me"
        checked={recurring}
        onChange={(e) => setRecurring(e.target.checked)}
      />
      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="primary"
          onClick={() => {
            const save = call('save_expense', {
              edit: {
                id: row.id,
                categoryId: category === '' ? null : category,
                description: what,
                amount,
                mode,
                paidTo,
                reference,
                gstPercent: gst,
                note,
              },
            });
            (recurring
              ? save.then(() =>
                  call('save_recurring_expense', {
                    id: freshId('rec'),
                    description: what,
                    amount,
                    mode,
                    every: 'month',
                    categoryId: category === '' ? null : category,
                  }),
                )
              : save
            )
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

/** What a shop sorts its spending into. */
function SpendCategories({
  view,
  onClose,
  onChanged,
  onFailed,
}: {
  view: ExpensesView;
  onClose: () => void;
  onChanged: (fresh: ExpensesView) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [adding, setAdding] = useState('');
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async (id: string, name: string, isActive: boolean) => {
    setBusy(true);
    try {
      onChanged(await call('save_expense_category', { id, name, isActive }));
    } catch (cause) {
      onFailed(cause);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal open title="What you spend on" onClose={onClose}>
      <p className="mb-muted">
        These are the headings your spending is sorted under, and what the
        totals at the bottom of the screen are grouped by.
      </p>

      <div className="mb-row">
        <Input
          label="Add a heading"
          value={adding}
          autoFocus
          placeholder="Gas"
          onChange={(e) => setAdding(e.target.value)}
          onKeyDown={(e) => {
            if (e.key !== 'Enter' || adding.trim() === '') return;
            void save(freshId('exc'), adding.trim(), true);
            setAdding('');
          }}
        />
        <Button
          variant="primary"
          disabled={busy || adding.trim() === ''}
          onClick={() => {
            void save(freshId('exc'), adding.trim(), true);
            setAdding('');
          }}
        >
          Add
        </Button>
      </div>

      <div className="mb-stack">
        {view.allCategories
          // The `null` id is "no category" — a real bucket on the totals and not a row anybody
          // can rename.
          .filter((c) => c.id !== null)
          .map((c) => (
            <div key={c.id} className="mb-row">
              {renaming?.id === c.id ? (
                <Input
                  label="Name"
                  value={renaming.name}
                  autoFocus
                  onChange={(e) => setRenaming({ id: c.id ?? '', name: e.target.value })}
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') setRenaming(null);
                    if (e.key !== 'Enter') return;
                    const name = renaming.name.trim();
                    setRenaming(null);
                    if (name !== '' && name !== c.name) void save(c.id ?? '', name, true);
                  }}
                />
              ) : (
                <>
                  <strong>{c.name}</strong>
                  <span className="mb-muted">
                    {c.count === 0 ? 'nothing filed here yet' : `${c.count} entries`}
                  </span>
                </>
              )}
              <Button
                small
                disabled={busy}
                onClick={() => setRenaming({ id: c.id ?? '', name: c.name })}
              >
                Rename
              </Button>
              <Button
                small
                variant="quiet"
                disabled={busy}
                onClick={() => void save(c.id ?? '', c.name, false)}
              >
                Stop using it
              </Button>
            </div>
          ))}
      </div>

      <div className="mb-row mb-row--end">
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </div>
    </Modal>
  );
}
