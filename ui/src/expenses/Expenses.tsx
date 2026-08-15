/**
 * **Money going out, and what should be in the drawer** — scope 10.6.
 *
 * # Quick add is the whole feature
 *
 * A cashier records a ₹40 milk purchase mid-service or does not record it at
 * all, and the second one is exactly how v1's owner ended up with an inflated
 * net profit on their phone every single day (audit A2 / ANDROID-D1). So the
 * top of this screen is two fields and Enter, and everything else — the
 * category, the vendor, the GST split — is optional detail on a row that
 * already exists.
 *
 * # Nothing here is arithmetic
 *
 * The cash position arrives as figures AND as a sentence, the category totals
 * are summed in Rust, and the input credit arrives as "18% · 180.00". R8.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  EmptyState,
  Icon,
  Input,
  Modal,
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
        id: `exp_${Date.now().toString(36)}`,
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
        subtitle="What left the shop as money today, and what is in the drawer."
        count={view.rows.length}
        actions={
          <Button variant="secondary" onClick={() => setDrawer(true)}>
            <Icon name="cash" size="sm" />
            Move cash
          </Button>
        }
      />

      {/* Two fields and Enter. Everything else is optional detail on a row
          that already exists. */}
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
        <Input
          label="Amount"
          value={amount}
          onChange={(e) => setAmount(e.target.value)}
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
      </div>

      {/* Scope 10.6's cash position — the number a drawer is counted against,
          said as a sentence Rust wrote. */}
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
    <Modal open title="Move cash" onClose={onClose}>
      <p className="mb-expenses__note">
        The float you started with, a top-up from the owner, a payout, or money
        taken to the bank. A purchase is not one of these — record that as an
        expense and the drawer follows it.
      </p>
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
      <Input label="Amount" value={amount} onChange={(e) => setAmount(e.target.value)} />
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
      <Input label="Amount" value={amount} onChange={(e) => setAmount(e.target.value)} />
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
                    id: `rec_${Date.now().toString(36)}`,
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
