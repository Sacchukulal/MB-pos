/** Every bill, one at a time — and the ways one is taken back. */

import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';

import {
  Badge,
  Button,
  Caption,
  DateRangePicker,
  EmptyState,
  Fact,
  Facts,
  Modal,
  Money,
  Notice,
  Numeric,
  PageHeader,
  Panel,
  RowMenu,
  Scroller,
  SearchField,
  Select,
  Spinner,
  Table,
  Toolbar,
  useToast,
  type BadgeTone,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { BeforeLineView } from '../ipc/generated/BeforeLineView';
import type { BillDetailView } from '../ipc/generated/BillDetailView';
import type { BillRowView } from '../ipc/generated/BillRowView';
import type { BillsView } from '../ipc/generated/BillsView';
import type { CartLineView } from '../ipc/generated/CartLineView';
import type { HistoryView } from '../ipc/generated/HistoryView';
import { ReasonDialog, type ReasonKind } from '../corrections/Reason';
import { useMay } from '../shell/permissions';

type Pending =
  | { kind: 'revert'; bill: BillRowView }
  | { kind: 'void'; bill: BillRowView }
  | { kind: 'reprint'; bill: BillRowView }
  | { kind: 'refund'; bill: BillRowView };

/** The chip for a bill's state, in a colour that is never the only signal. */
const TONES: Record<string, BadgeTone> = {
  settled: 'ok',
  voided: 'danger',
  cancelled: 'neutral',
};

const STATES = [
  { value: '', label: 'Every state' },
  { value: 'settled', label: 'Paid' },
  { value: 'edited', label: 'Edited' },
  { value: 'voided', label: 'Voided' },
  { value: 'cancelled', label: 'Cancelled' },
];

const MODES = [
  { value: '', label: 'Any payment' },
  { value: 'Cash', label: 'Cash' },
  { value: 'Card', label: 'Card' },
  { value: 'UPI', label: 'UPI' },
  { value: 'Credit', label: 'Credit' },
];

/** How long typing settles before Rust is asked again. */
const SEARCH_SETTLE_MS = 150;

export function Bills({ onGoTo }: { onGoTo?: (screen: string) => void }) {
  const [view, setView] = useState<BillsView | null>(null);
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [query, setQuery] = useState('');
  const [cashier, setCashier] = useState('');
  const [state, setState] = useState('');
  const [mode, setMode] = useState('');
  /** The bill that is open, by order id. */
  const [opened, setOpened] = useState<string | null>(null);
  const [detail, setDetail] = useState<BillDetailView | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const toast = useToast();
  const mayExport = useMay()('reports.export');

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  // One filter, asked for and saved: a file that does not match the list on screen is worse
  // than no file.
  const filter = useMemo(
    () => ({
      period: from && to ? { from, to } : null,
      query: query || null,
      cashier: cashier || null,
      state: state || null,
      mode: mode || null,
    }),
    [from, to, query, cashier, state, mode],
  );

  const load = useCallback(async () => {
    try {
      const fresh = await call('bills', { filter });
      setView(fresh);
      // The first answer carries the presets; the screen opens on today.
      const today = fresh.periods[0];
      if (!from && today) {
        setFrom(today.from);
        setTo(today.to);
      }
    } catch (cause) {
      complain(cause);
    }
  }, [filter, from, complain]);

  /** The list on screen, as a file in the Downloads folder that opens itself. */
  const save = (command: 'bills_csv' | 'bills_pdf') => {
    call(command, { filter })
      .then((saved) => toast.show('ok', saved.message, saved.path))
      .catch(complain);
  };

  useEffect(() => {
    const timer = setTimeout(() => void load(), SEARCH_SETTLE_MS);
    return () => clearTimeout(timer);
  }, [load]);

  const open = useCallback(
    async (orderId: string) => {
      setOpened(orderId);
      try {
        setDetail(await call('bill_detail', { orderId }));
      } catch (cause) {
        setOpened(null);
        complain(cause);
      }
    },
    [complain],
  );

  const close = () => {
    setOpened(null);
    setDetail(null);
  };

  /** After anything changes a bill: the list again, and the open one again. */
  const refresh = async () => {
    await load();
    if (opened) await open(opened);
  };

  const approve = async (revertId: string) => {
    try {
      await call('approve_revert', { revertId });
      toast.show('ok', 'Approved.');
      await refresh();
    } catch (cause) {
      complain(cause);
    }
  };

  const invoice = (bill: BillRowView) => {
    call('bill_pdf', { orderId: bill.orderId })
      .then((saved) => toast.show('ok', saved.message))
      .catch(complain);
  };

  /** The buttons for one bill: on its row, and again in its dialog. */
  const actionsFor = (bill: BillRowView, size: 'sm' | 'md') => {
    if (!view || bill.state === 'cancelled') return null;
    const paid = bill.state === 'settled';
    return (
      <>
        {paid && view.canRevert ? (
          <Button
            size={size}
            variant={size === 'md' ? 'primary' : 'secondary'}
            onClick={() => setPending({ kind: 'revert', bill })}
          >
            Edit bill
          </Button>
        ) : null}
        {view.canReprint ? (
          <Button size={size} variant="quiet" onClick={() => setPending({ kind: 'reprint', bill })}>
            Reprint
          </Button>
        ) : null}
        <RowMenu label={`More for bill ${bill.number}`} size={size}>
          <Button size="sm" variant="quiet" onClick={() => invoice(bill)}>
            Invoice PDF
          </Button>
          {paid && view.canVoid ? (
            <Button size="sm" variant="danger" onClick={() => setPending({ kind: 'void', bill })}>
              Void
            </Button>
          ) : null}
          {!paid && view.canVoid ? (
            <Button size="sm" onClick={() => setPending({ kind: 'refund', bill })}>
              Give money back
            </Button>
          ) : null}
        </RowMenu>
      </>
    );
  };

  const columns: Column<BillRowView>[] = [
    {
      key: 'number',
      header: 'Bill',
      nowrap: true,
      render: (b) => <strong>{b.number}</strong>,
    },
    { key: 'at', header: 'When', nowrap: true, render: (b) => b.at },
    { key: 'type', header: 'Table', render: (b) => b.table ?? b.orderType },
    { key: 'items', header: 'Items', numeric: true, render: (b) => <Numeric>{b.items}</Numeric> },
    {
      key: 'total',
      header: 'Total',
      numeric: true,
      render: (b) => (b.state === 'cancelled' ? '—' : <Money value={b.total} />),
    },
    { key: 'paidBy', header: 'Paid by', optional: true, render: (b) => b.paidBy },
    { key: 'cashier', header: 'Taken by', render: (b) => b.cashier ?? '—' },
    {
      key: 'state',
      header: 'State',
      render: (b) => (
        <span className="mb-stack mb-stack--gap-inline">
          <span className="mb-row mb-row--gap-inline">
            <Badge tone={TONES[b.state] ?? 'neutral'}>{b.stateWord}</Badge>
            {b.edited ? <Badge tone="accent">Edited</Badge> : null}
            {b.approval === 'waiting' ? <Badge tone="warn">Needs approval</Badge> : null}
            {b.approval === 'approved' ? <Badge tone="ok">Approved</Badge> : null}
          </span>
          {b.voidReason ? <span className="mb-bills__second">{b.voidReason}</span> : null}
          {b.refunded ? (
            <span className="mb-bills__second">{b.refunded.text} given back</span>
          ) : null}
        </span>
      ),
    },
    {
      key: 'copies',
      header: 'Copies',
      numeric: true,
      optional: true,
      render: (b) => (b.reprints > 0 ? <Numeric>{b.reprints + 1}</Numeric> : ''),
    },
    {
      key: 'do',
      header: '',
      render: (b) => (
        // The buttons act on the bill; only the rest of the row opens it.
        <div className="mb-row mb-row--gap-inline" onClick={(event) => event.stopPropagation()}>
          {actionsFor(b, 'sm')}
        </div>
      ),
    },
  ];

  if (!view) return <Spinner label="Opening the bills" />;

  const totals = view.totals;
  const subtitle =
    view.rows.length > 0
      ? [
          `Taken ${totals.gross.text}`,
          totals.voids.paise > 0 ? `Voided ${totals.voids.text}` : '',
          `Net ${totals.net.text}`,
          totals.refunded.paise > 0 ? `Given back ${totals.refunded.text}` : '',
          view.waiting > 0 ? `${view.waiting} waiting for approval` : '',
        ]
          .filter(Boolean)
          .join(' · ')
      : view.waiting > 0
        ? `${view.waiting} waiting for approval`
        : undefined;

  return (
    <div className="mb-bills">
      <PageHeader
        title="Bills"
        count={view.rows.length}
        subtitle={subtitle}
        actions={
          mayExport ? (
            <>
              <Button size="sm" variant="quiet" onClick={() => save('bills_csv')}>
                Save as CSV
              </Button>
              <Button size="sm" variant="quiet" onClick={() => save('bills_pdf')}>
                Save as PDF
              </Button>
            </>
          ) : undefined
        }
      />

      <Toolbar
        end={
          <>
            <div className="mb-reports__presets">
              {view.periods.map((choice) => (
                <Button
                  size="sm"
                  key={choice.label}
                  variant={choice.from === from && choice.to === to ? 'primary' : 'quiet'}
                  onClick={() => {
                    setFrom(choice.from);
                    setTo(choice.to);
                  }}
                >
                  {choice.label}
                </Button>
              ))}
            </div>
            <DateRangePicker
              from={from}
              to={to}
              onChange={(nextFrom, nextTo) => {
                setFrom(nextFrom);
                setTo(nextTo);
              }}
            />
          </>
        }
      >
        <SearchField
          what="Bill number, table, item"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <Select
          size="sm"
          aria-label="Taken by"
          value={cashier}
          onChange={(event) => setCashier(event.target.value)}
          options={[
            { value: '', label: 'Everybody' },
            ...view.cashiers.map((p) => ({ value: p.id, label: p.name })),
          ]}
        />
        <Select
          size="sm"
          aria-label="State"
          value={state}
          onChange={(event) => setState(event.target.value)}
          options={STATES}
        />
        <Select
          size="sm"
          aria-label="Paid by"
          value={mode}
          onChange={(event) => setMode(event.target.value)}
          options={MODES}
        />
      </Toolbar>

      {view.rows.length === 0 ? (
        <EmptyState
          title="No bills here"
          hint="Pick another day, or clear the search."
        />
      ) : (
        <Scroller wide className="mb-bills__sheet">
          <Panel flush>
            <Table
              rows={view.rows}
              columns={columns}
              rowKey={(b) => b.orderId}
              onRow={(b) => void open(b.orderId)}
            />
          </Panel>
        </Scroller>
      )}

      {opened ? (
        <BillDialog
          detail={detail}
          actions={detail ? actionsFor(detail.row, 'md') : null}
          onApprove={approve}
          onClose={close}
        />
      ) : null}

      {pending ? (
        <Correction
          pending={pending}
          approvers={view.approvers}
          onClose={() => setPending(null)}
          onDone={async (message) => {
            setPending(null);
            toast.show('ok', message);
            if (pending.kind === 'revert') {
              // The lines are on the counter now; that is where the person is going.
              close();
              onGoTo?.('billing');
              return;
            }
            await refresh();
          }}
          onFailed={(message, detail) => toast.show('danger', message, detail)}
        />
      ) : null}
    </div>
  );
}

/** One bill, opened: what was on it, how it was paid, and everything done to it since. */
function BillDialog({
  detail,
  actions,
  onApprove,
  onClose,
}: {
  detail: BillDetailView | null;
  actions: ReactNode;
  onApprove: (revertId: string) => void | Promise<void>;
  onClose: () => void;
}) {
  const lineColumns: Column<CartLineView>[] = [
    {
      key: 'name',
      header: 'Item',
      render: (l) => (
        <span className="mb-stack mb-stack--gap-inline">
          <span>{l.name}</span>
          {l.modifiers.length > 0 ? (
            <span className="mb-bills__second">{l.modifiers.join(', ')}</span>
          ) : null}
          {l.note ? <span className="mb-bills__second">{l.note}</span> : null}
        </span>
      ),
    },
    { key: 'qty', header: 'Qty', numeric: true, render: (l) => <Numeric>{l.qty}</Numeric> },
    { key: 'price', header: 'Price', numeric: true, render: (l) => <Money value={l.unitPrice} /> },
    {
      key: 'discount',
      header: 'Discount',
      numeric: true,
      optional: true,
      render: (l) => (l.discount.paise > 0 ? <Money value={l.discount} /> : ''),
    },
    { key: 'amount', header: 'Amount', numeric: true, render: (l) => <Money value={l.amount} /> },
  ];
  const historyColumns: Column<HistoryView>[] = [
    { key: 'when', header: 'When', nowrap: true, render: (h) => h.when },
    { key: 'who', header: 'Who', render: (h) => h.who },
    { key: 'what', header: 'What', render: (h) => h.what },
  ];

  const beforeColumns: Column<BeforeLineView>[] = [
    { key: 'name', header: 'Item', render: (l) => l.name },
    { key: 'qty', header: 'Qty', numeric: true, render: (l) => <Numeric>{l.qty}</Numeric> },
    { key: 'price', header: 'Price', numeric: true, render: (l) => <Money value={l.unitPrice} /> },
    { key: 'amount', header: 'Amount', numeric: true, render: (l) => <Money value={l.amount} /> },
  ];

  const row = detail?.row;
  const bill = detail?.bill;
  const waiting = detail?.edits.filter((edit) => !edit.approvedAt) ?? [];

  return (
    <Modal
      open
      wide
      title={row ? `Bill ${row.number}` : 'Bill'}
      onClose={onClose}
      actions={
        <>
          {detail?.canApprove && waiting.length > 0 ? (
            <Button
              variant="primary"
              onClick={() => {
                for (const edit of waiting) void onApprove(edit.id);
              }}
            >
              Approve
            </Button>
          ) : null}
          {actions}
          <Button variant="quiet" onClick={onClose}>
            Close
          </Button>
        </>
      }
    >
      {!detail || !row ? (
        <Spinner label="Opening the bill" />
      ) : (
        <>
          <Facts>
            <Fact label="When">{row.at}</Fact>
            <Fact label={row.table ? 'Table' : 'Type'}>{row.table ?? row.orderType}</Fact>
            <Fact label="Taken by">{row.cashier ?? detail.takenBy ?? '—'}</Fact>
            {row.paidBy ? <Fact label="Paid by">{row.paidBy}</Fact> : null}
            <Fact label="State">
              <span className="mb-row mb-row--gap-inline">
                <Badge tone={TONES[row.state] ?? 'neutral'}>{row.stateWord}</Badge>
                {row.edited ? <Badge tone="accent">Edited</Badge> : null}
                {row.reprints > 0 ? <Badge tone="neutral">{row.reprints + 1} copies</Badge> : null}
              </span>
            </Fact>
          </Facts>

          {detail.edits.map((edit) => (
            <div className="mb-stack mb-stack--gap-inline" key={edit.id}>
              <Notice tone={edit.approvedAt ? 'ok' : 'warn'}>
                <span className="mb-stack mb-stack--gap-inline">
                  <strong>
                    Taken back by {edit.by}, {edit.at}: {edit.reason}
                  </strong>
                  <span>
                    Was {edit.beforeTotal.text}, paid by {edit.beforePaidBy}, {edit.beforePaidAt}
                  </span>
                  <ul className="mb-bills__changes">
                    {edit.changes.map((change) => (
                      <li key={change}>{change}</li>
                    ))}
                  </ul>
                  <span>
                    {edit.approvedAt
                      ? `Approved by ${edit.approvedBy ?? ''}, ${edit.approvedAt}`
                      : 'Waiting for approval'}
                  </span>
                </span>
              </Notice>
              <Caption>Before</Caption>
              <Table
                rows={edit.beforeLines}
                columns={beforeColumns}
                rowKey={(l) => l.name}
                dense
              />
            </div>
          ))}

          {row.state === 'voided' && row.voidReason ? (
            <Notice tone="danger">Voided: {row.voidReason}</Notice>
          ) : null}
          {row.state === 'cancelled' && row.voidReason ? (
            <Notice tone="info">Cancelled: {row.voidReason}</Notice>
          ) : null}
          {detail.note ? <Notice tone="info">{detail.note}</Notice> : null}

          <Caption>Items</Caption>
          <Table rows={detail.lines} columns={lineColumns} rowKey={(l) => `${l.index}`} dense />

          {bill ? (
            <div className="mb-bills__totals">
              <TotalLine label="Subtotal" value={bill.subtotal.text} />
              {bill.totalDiscount.paise > 0 ? (
                <TotalLine label="Discount" value={bill.totalDiscount.text} />
              ) : null}
              {bill.charges.map((charge) => (
                <TotalLine key={charge.name} label={charge.name} value={charge.amount.text} />
              ))}
              {bill.taxTotal.paise > 0 ? <TotalLine label="Tax" value={bill.taxTotal.text} /> : null}
              {Number(bill.roundOff.paise) !== 0 ? (
                <TotalLine label="Round off" value={bill.roundOff.text} />
              ) : null}
              <TotalLine label="Total" value={bill.grandTotal.text} strong />
              {detail.payments.map((payment) => (
                <TotalLine
                  key={payment.index}
                  label={payment.reference ? `${payment.mode} ${payment.reference}` : payment.mode}
                  value={payment.amount.text}
                />
              ))}
              {detail.tip.paise > 0 ? <TotalLine label="Tip" value={detail.tip.text} /> : null}
              {detail.change.paise > 0 ? (
                <TotalLine label="Change" value={detail.change.text} />
              ) : null}
            </div>
          ) : null}

          {detail.history.length > 0 ? (
            <>
              <Caption>History</Caption>
              <Table
                rows={detail.history}
                columns={historyColumns}
                rowKey={(h) => `${h.when} ${h.what}`}
                dense
              />
            </>
          ) : null}
        </>
      )}
    </Modal>
  );
}

function TotalLine({ label, value, strong }: { label: string; value: string; strong?: boolean }) {
  return (
    <div className={strong ? 'mb-bills__line mb-bills__line--strong' : 'mb-bills__line'}>
      <span>{label}</span>
      <span className="mb-numeric">{value}</span>
    </div>
  );
}

function Correction({
  pending,
  approvers,
  onClose,
  onDone,
  onFailed,
}: {
  pending: Pending;
  approvers: readonly { id: string; name: string }[];
  onClose: () => void;
  onDone: (message: string) => void | Promise<void>;
  onFailed: (message: string, detail?: string) => void;
}) {
  const { bill } = pending;
  // Rust decides whether a manager is needed; the screen finds out by asking.
  const [needsApproval, setNeedsApproval] = useState(false);

  // A revert is a void and a fresh bill, so it offers the void reasons.
  const kind: ReasonKind = pending.kind === 'reprint' ? 'reprint' : 'void';
  const what = {
    revert: `Edit bill ${bill.number} — ${bill.total.text}. It goes back to the counter under the same number, to be changed and billed again.`,
    void: `Void bill ${bill.number} — ${bill.total.text}`,
    reprint: `Reprint bill ${bill.number}`,
    refund: `Give back ${bill.total.text} on bill ${bill.number}`,
  }[pending.kind];
  const confirmLabel = {
    revert: 'Take it back to the counter',
    void: 'Void the bill',
    reprint: 'Print another copy',
    refund: 'Record the money going back',
  }[pending.kind];

  const run = async (reason: string, approver?: { id: string; pin: string }) => {
    try {
      if (pending.kind === 'revert') {
        const said = await call('revert_bill', {
          orderId: bill.orderId,
          reason,
          approverStaffId: approver?.id ?? null,
          approverPin: approver?.pin ?? null,
        });
        await onDone(said);
      } else if (pending.kind === 'void') {
        await call('void_bill', {
          orderId: bill.orderId,
          reason,
          approverStaffId: approver?.id ?? null,
          approverPin: approver?.pin ?? null,
        });
        await onDone(`Bill ${bill.number} is voided.`);
      } else if (pending.kind === 'reprint') {
        const said = await call('reprint_bill', { orderId: bill.orderId, reason });
        await onDone(said);
      } else {
        await call('refund_bill', {
          orderId: bill.orderId,
          // The paise integer Rust sent, handed straight back.
          amountPaise: Number(bill.total.paise),
          mode: 'cash',
          reason,
        });
        await onDone(`${bill.total.text} recorded as given back.`);
      }
    } catch (cause) {
      if (isUiError(cause) && cause.code === 'void.needs_approval') {
        // Not a failure the cashier caused: the dialog stays open and grows a PIN box.
        setNeedsApproval(true);
        onFailed(cause.message);
        return;
      }
      if (isUiError(cause)) onFailed(cause.message, cause.detail ?? undefined);
      else onFailed('That could not be done.');
    }
  };

  return (
    <ReasonDialog
      kind={kind}
      what={what}
      confirmLabel={confirmLabel}
      needsApproval={needsApproval}
      approvers={approvers}
      onCancel={onClose}
      onConfirm={(reason, approver) => void run(reason, approver)}
    />
  );
}
