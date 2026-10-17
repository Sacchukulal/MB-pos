/** Every bill, one at a time — and the ways one is taken back. */

import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

import {
  Badge,
  Button,
  Caption,
  EmptyState,
  Fact,
  Facts,
  Icon,
  Modal,
  Money,
  MoneyInput,
  Notice,
  Numeric,
  Panel,
  Scroller,
  SearchField,
  Select,
  Spinner,
  Table,
  plural,
  useToast,
  freshId,
  type BadgeTone,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { BeforeLineView } from '../ipc/generated/BeforeLineView';
import type { BillDetailView } from '../ipc/generated/BillDetailView';
import type { BillRowView } from '../ipc/generated/BillRowView';
import type { BillsView } from '../ipc/generated/BillsView';
import type { BillFilter } from '../ipc/generated/BillFilter';
import type { PeriodChoiceView } from '../ipc/generated/PeriodChoiceView';
import type { BillCorrectionOffer } from '../ipc/generated/BillCorrectionOffer';
import type { CartLineView } from '../ipc/generated/CartLineView';
import type { HistoryView } from '../ipc/generated/HistoryView';
import { ReasonDialog, type ReasonKind } from '../corrections/Reason';
import { useLicenceRevision } from '../shell/licence';
import { useMay } from '../shell/permissions';
import { ReportExport, ReportHeader, ReportPeriod } from './ReportHeader';

type Pending =
  | { kind: 'revert'; bill: BillRowView }
  | { kind: 'void'; bill: BillRowView }
  | { kind: 'return'; bill: BillRowView }
  | { kind: 'reprint'; bill: BillRowView }
  | { kind: 'refund'; bill: BillRowView };

/** The chip for a bill's state, in a colour that is never the only signal. */
const TONES: Record<string, BadgeTone> = {
  settled: 'ok',
  voided: 'danger',
  cancelled: 'neutral',
};

const STATES = [
  { value: '', label: 'All states' },
  { value: 'settled', label: 'Paid' },
  { value: 'edited', label: 'Edited' },
  { value: 'voided', label: 'Voided' },
  { value: 'cancelled', label: 'Cancelled' },
];

const MODES = [
  { value: '', label: 'All payment modes' },
  { value: 'Cash', label: 'Cash' },
  { value: 'Card', label: 'Card' },
  { value: 'UPI', label: 'UPI' },
  { value: 'Credit', label: 'Credit' },
];

/** How long typing settles before Rust is asked again. */
const SEARCH_SETTLE_MS = 150;

export function Bills({ onGoTo, initialOrderId, periods = [] }: {
  onGoTo?: (screen: string) => void;
  initialOrderId?: string;
  periods?: readonly PeriodChoiceView[];
}) {
  const may = useMay();
  const [view, setView] = useState<BillsView | null>(null);
  const initialPeriod = useRef<BillFilter['period']>(periods[0] ? { from: periods[0].from, to: periods[0].to } : null);
  // null asks Rust for its current business day. Display its returned dates without
  // converting that first response into another request for the same day.
  const [period, setPeriod] = useState<BillFilter['period']>(null);
  const [query, setQuery] = useState('');
  const [settledQuery, setSettledQuery] = useState('');
  const [loadedFilter, setLoadedFilter] = useState<BillFilter | null>(null);
  const [busy, setBusy] = useState(true);
  const [failed, setFailed] = useState(false);
  const [cashier, setCashier] = useState('');
  const [state, setState] = useState('');
  const [mode, setMode] = useState('');
  /** The bill that is open, by order id. */
  const [opened, setOpened] = useState<string | null>(null);
  const [detail, setDetail] = useState<BillDetailView | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const toast = useToast();
  const licenceRevision = useLicenceRevision();
  const listRequest = useRef(0);
  const detailRequest = useRef(0);
  const initialHandled = useRef<string | undefined>(undefined);
  const initialReturn = useRef<string | null>(null);
  const [returnNotice, setReturnNotice] = useState<string | null>(null);

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  // One filter, asked for and saved: a file that does not match the list on screen is worse
  // than no file.
  const filter = useMemo<BillFilter>(
    () => ({
      period: period ?? initialPeriod.current,
      query: settledQuery || null,
      cashier: cashier || null,
      state: state || null,
      mode: mode || null,
    }),
    [period, settledQuery, cashier, state, mode],
  );

  useEffect(() => {
    if (query === settledQuery) return;
    const timer = setTimeout(() => setSettledQuery(query), SEARCH_SETTLE_MS);
    return () => clearTimeout(timer);
  }, [query, settledQuery]);

  const load = useCallback(async () => {
    const request = ++listRequest.current;
    setFailed(false);
    if (query !== settledQuery) { setBusy(true); return; }
    if (filter.period && (!filter.period.from || !filter.period.to)) { setBusy(false); return; }
    setBusy(true);
    try {
      const fresh = await call('bills', { filter });
      if (request !== listRequest.current) return;
      const today = fresh.periods[0];
      if (!initialPeriod.current && today) initialPeriod.current = { from: today.from, to: today.to };
      setView(fresh);
      setLoadedFilter(filter);
    } catch (cause) {
      if (request !== listRequest.current) return;
      setFailed(true);
      complain(cause);
    } finally {
      if (request === listRequest.current) setBusy(false);
    }
  }, [filter, query, settledQuery, complain]);

  const current = loadedFilter === filter && query === settledQuery && !busy && !failed;

  /** The list on screen, as a file in the Downloads folder that opens itself. */
  const save = (command: 'bills_csv' | 'bills_pdf') => {
    if (!current || !view?.canExport) return;
    call(command, { filter: { ...filter, period: filter.period ?? initialPeriod.current } })
      .then((saved) => toast.show('ok', saved.message, saved.path))
      .catch(complain);
  };

  useEffect(() => {
    void load();
    return () => { ++listRequest.current; };
  }, [load, licenceRevision]);

  const open = useCallback(
    async (orderId: string) => {
      const request = ++detailRequest.current;
      setOpened(orderId);
      setDetail(null);
      setReturnNotice(null);
      try {
        const fresh = await call('bill_detail', { orderId });
        if (request === detailRequest.current) setDetail(fresh);
      } catch (cause) {
        if (request !== detailRequest.current) return;
        setOpened(null);
        complain(cause);
      }
    },
    [complain],
  );

  const close = () => {
    ++detailRequest.current;
    initialReturn.current = null;
    setOpened(null);
    setDetail(null);
  };

  useEffect(() => {
    if (!initialOrderId) {
      initialHandled.current = undefined;
      initialReturn.current = null;
      return;
    }
    if (!view || initialHandled.current === initialOrderId) return;
    initialHandled.current = initialOrderId;
    initialReturn.current = initialOrderId;
    void open(initialOrderId);
  }, [initialOrderId, view, open]);

  useEffect(() => {
    if (!view || !detail || initialReturn.current !== opened || opened !== initialOrderId) return;
    initialReturn.current = null;
    if (view.canVoid && detail.row.state === 'settled' && !detail.row.returned) {
      setPending({ kind: 'return', bill: detail.row });
    } else if (!view.canVoid) {
      setReturnNotice('A manager with bill-return permission must sign in to return the whole bill. Your changes are saved.');
    }
  }, [detail, opened, initialOrderId, view]);

  /** After anything changes a bill: the list again, and the open one again. */
  const refresh = async () => {
    await load();
    if (opened) await open(opened);
  };

  const approvingRef = useRef(false);
  const [approving, setApproving] = useState(false);
  const approve = async (revertIds: string[]) => {
    if (approvingRef.current) return;
    approvingRef.current = true;
    setApproving(true);
    try {
      for (const revertId of revertIds) await call('approve_revert', { revertId });
      toast.show('ok', 'Approved.');
    } catch (cause) {
      complain(cause);
    } finally {
      try { await refresh(); } finally {
        approvingRef.current = false;
        setApproving(false);
      }
    }
  };

  /** Keep the row compact; refunds remain available inside the bill details. */
  const actionsFor = (bill: BillRowView, size: 'sm' | 'md') => {
    if (!view || bill.state === 'cancelled') return null;
    const paid = bill.state === 'settled' && !bill.returned;
    const inRow = size === 'sm';
    return (
      <>
        {view.canReprint ? (
          <Button size="md" variant="secondary" iconOnly
            title="Reprint bill" aria-label="Reprint bill" icon={<Icon name="printer" size="lg" />}
            onClick={() => setPending({ kind: 'reprint', bill })} />
        ) : null}
        {paid && view.canRevert && !bill.dayClosed ? (
          <Button size={size} onClick={() => setPending({ kind: 'revert', bill })}
            title="Open in Billing and complete with the same bill number">
            {bill.correctionOpen ? 'Resume bill' : 'Revert bill'}
          </Button>
        ) : null}
        {paid && view.canVoid && !bill.dayClosed ? (
          <Button size="md" variant="danger" iconOnly
            title="Delete bill" aria-label="Delete bill" icon={<Icon name="trash" size="lg" />}
            onClick={() => setPending({ kind: 'void', bill })} />
        ) : null}
        {!inRow && paid && view.canVoid ? (
          <Button size={size} variant="quiet" onClick={() => setPending({ kind: 'return', bill })}>
            Refund entire bill
          </Button>
        ) : null}
        {!inRow && !paid && view.canVoid && detail?.canRefund ? (
          <Button size={size} variant="quiet" onClick={() => setPending({ kind: 'refund', bill })}>
            Return money
          </Button>
        ) : null}
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
    {
      key: 'do',
      header: '',
      render: (b) => (
        // Keep bill controls ahead of secondary details on narrow screens.
        <div className="mb-row mb-bills__actions" onClick={(event) => event.stopPropagation()}>
          {actionsFor(b, 'sm')}
        </div>
      ),
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
            {b.returned ? <Badge tone="warn">Returned</Badge> : null}
            {b.edited ? <Badge tone="accent">Edited</Badge> : null}
            {b.correctionOpen ? <Badge tone="accent">Editing</Badge> : b.approval === 'waiting' ? <Badge tone="warn">Needs approval</Badge> : null}
            {!b.correctionOpen && b.approval === 'approved' ? <Badge tone="ok">Approved</Badge> : null}
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
  ];

  const choices = view?.periods ?? periods;
  const from = period?.from ?? initialPeriod.current?.from ?? choices[0]?.from ?? '';
  const to = period?.to ?? initialPeriod.current?.to ?? choices[0]?.to ?? '';
  const totals = current ? view?.totals : null;
  const subtitle =
    totals && view && view.rows.length > 0
      ? [
          `Taken ${totals.gross.text}`,
          totals.voids.paise > 0 ? `Voided ${totals.voids.text}` : '',
          `Net ${totals.net.text}`,
          totals.refunded.paise > 0 ? `Given back ${totals.refunded.text}` : '',
          view.waiting > 0 ? `${view.waiting} waiting for approval` : '',
        ]
          .filter(Boolean)
          .join(' · ')
      : current && view && view.waiting > 0
        ? `${view.waiting} waiting for approval`
        : undefined;

  return (
    <div className="mb-bills">
      <ReportHeader
        title="Bills"
        summary={totals || subtitle ? <span>{[totals ? plural(view?.rows.length ?? 0, 'bill') : '', subtitle].filter(Boolean).join(' · ')}</span> : undefined}
        actions={
          (view ? view.canExport : may('reports.view') && may('reports.export')) ? (
            <ReportExport disabled={!current} onExport={(format) => save(format === 'csv' ? 'bills_csv' : 'bills_pdf')} />
          ) : undefined
        }
      >
        <ReportPeriod periods={choices} from={from} to={to} onChange={(nextFrom, nextTo) => setPeriod({ from: nextFrom, to: nextTo })} />
        <div className="mb-bills__filters">
        <SearchField
          what="Bill number, table, item"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <div className="mb-bills__filter-picks">
        <Select
          size="sm"
          aria-label="Taken by"
          value={cashier}
          onChange={(event) => setCashier(event.target.value)}
          options={[
            { value: '', label: 'All cashiers' },
            ...(view?.cashiers ?? []).map((p) => ({ value: p.id, label: p.name })),
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
        </div>
        </div>
      </ReportHeader>

      {failed ? <EmptyState title="Could not load bills" hint="Try again or change the filters."
        action={<Button onClick={() => void load()}>Retry</Button>} /> :
      period && (!period.from || !period.to) ? <EmptyState title="Choose a valid date range" /> :
      !current || !view ? <Spinner label="Loading bills" /> : view.rows.length === 0 ? (
        <EmptyState
          title="No matching bills"
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
          notice={returnNotice}
          actions={detail ? actionsFor(detail.row, 'md') : null}
          onApprove={approve}
          approving={approving}
          onClose={close}
        />
      ) : null}

      {pending && view ? (
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
  notice,
  actions,
  onApprove,
  approving,
  onClose,
}: {
  detail: BillDetailView | null;
  notice: string | null;
  actions: ReactNode;
  onApprove: (revertIds: string[]) => void | Promise<void>;
  approving: boolean;
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
              disabled={approving}
              onClick={() => { void onApprove(waiting.map((edit) => edit.id)); }}
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
      {notice ? <Notice tone="warn">{notice}</Notice> : null}
      {!detail || !row ? (
        <Spinner label="Loading bill" />
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
                {row.returned ? <Badge tone="warn">Returned on a later day</Badge> : null}
                {row.edited ? <Badge tone="accent">Edited</Badge> : null}
                {row.correctionOpen ? <Badge tone="accent">Editing</Badge> : null}
                {row.reprints > 0 ? <Badge tone="neutral">{row.reprints + 1} copies</Badge> : null}
              </span>
            </Fact>
          </Facts>

          {row.correctionOpen ? <p className="mb-muted">An edit is still open. Choose Resume bill, then Complete bill or Discard changes in Billing.</p> : null}
          {detail.edits.map((edit, index) => (
            <div className="mb-stack mb-stack--gap-inline" key={edit.id}>
              <Notice tone={edit.approvedAt ? 'ok' : row.state !== 'settled' || row.returned ? 'info' : 'warn'}>
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
                      : row.state !== 'settled' || row.returned ? 'Bill cancelled or returned; no edit approval required.'
                      : row.correctionOpen && index === detail.edits.length - 1 ? 'Edit in progress — complete the bill to finish.' : 'Waiting for approval'}
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

export function Correction({
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
  const [offer, setOffer] = useState<BillCorrectionOffer | null>(null);
  const [amounts, setAmounts] = useState<Record<string, string>>({});
  const [requestId] = useState(() => freshId('refund'));
  const [problem, setProblem] = useState<string | null>(null);
  const returnsMoney = pending.kind === 'return' || pending.kind === 'refund';
  useEffect(() => {
    if (pending.kind === 'reprint') return;
    let active = true;
    call('bill_correction_offer', { orderId: bill.orderId }).then((fresh) => {
      if (!active) return;
      setOffer(fresh);
      setNeedsApproval((pending.kind === 'void' || pending.kind === 'return') && fresh.needsApproval);
      setAmounts(Object.fromEntries(fresh.tenders.map((tender) => [tender.mode, tender.input])));
    }).catch((cause) => { if (active) setProblem(isUiError(cause) ? cause.message : 'Could not load this bill. Reopen it and try again.'); });
    return () => { active = false; };
  }, [bill.orderId, pending.kind]);

  // Editing a settled bill uses the shop's existing correction reasons.
  const kind: ReasonKind = pending.kind === 'reprint' ? 'reprint' : 'void';
  const what = {
    revert: `Revert bill ${bill.number} — ${bill.total.text}`,
    void: `Delete bill ${bill.number} — ${bill.total.text}`,
    return: `Return bill ${bill.number}`,
    reprint: `Reprint bill ${bill.number}`,
    refund: `Return money on bill ${bill.number}`,
  }[pending.kind];
  const confirmLabel = {
    revert: 'Revert bill',
    void: 'Delete bill',
    return: 'Confirm return',
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
      } else if (pending.kind === 'return') {
        const args = {
          orderId: bill.orderId, reason,
          amounts: (offer?.tenders ?? []).map((tender): [string, string] => [tender.mode, amounts[tender.mode] ?? '']),
          approverStaffId: approver?.id ?? null, approverPin: approver?.pin ?? null,
        };
        if (offer?.closedDay) await call('return_closed_bill', { ...args, requestId });
        else await call('void_and_return_bill', args);
        await onDone(`Bill ${bill.number} returned.`);
      } else if (pending.kind === 'void') {
        await call('void_bill', {
          orderId: bill.orderId,
          reason,
          approverStaffId: approver?.id ?? null,
          approverPin: approver?.pin ?? null,
        });
        await onDone(`Bill ${bill.number} cancelled without a refund.`);
      } else if (pending.kind === 'reprint') {
        const said = await call('reprint_bill', { orderId: bill.orderId, reason });
        await onDone(said);
      } else {
        await call('return_bill_money', {
          orderId: bill.orderId,
          amounts: (offer?.tenders ?? []).map((tender) => [tender.mode, amounts[tender.mode] ?? '']),
          reason,
          requestId,
        });
        await onDone('The money returned is recorded.');
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

  if (pending.kind === 'refund' && offer && offer.tenders.length === 0) {
    return <Modal open title={`Bill ${bill.number}`} onClose={onClose}
      actions={<Button onClick={onClose}>Close</Button>}>
      <Notice tone="info">There is no payment left to refund. Any refunds already recorded remain in this bill’s history.</Notice>
    </Modal>;
  }

  return (
    <ReasonDialog
      wide={returnsMoney}
      kind={kind}
      what={what}
      description={pending.kind === 'revert'
        ? 'Change the items on the billing screen, then Complete bill. The bill keeps its number.'
        : pending.kind === 'void' ? 'This cancels the bill without recording a refund and keeps its audit history. To refund the customer, open the bill details and choose Refund entire bill.'
        : returnsMoney ? 'Enter refunds by original payment method. This records the refund; send Card/UPI refunds through your provider separately. Prepared food is not returned to stock.' : undefined}
      confirmLabel={confirmLabel}
      needsApproval={needsApproval}
      approvers={approvers}
      onCancel={onClose}
      disabled={pending.kind !== 'reprint' && (!offer || (pending.kind === 'refund' && offer.tenders.length === 0))}
      onConfirm={run}
    >
      {problem ? <Notice tone="danger">{problem}</Notice> : null}
      {pending.kind !== 'reprint' && !offer && !problem ? <Spinner label="Checking this bill" /> : null}
      {returnsMoney && offer ? <>
        {pending.kind === 'return' && offer.closedDay ? <Notice tone="info">The whole bill will be returned today. The original closed bill stays unchanged. Any customer-account amount will be credited. Prepared food will not be added back to stock.</Notice> : null}
        <p>Still available to return: <Money value={offer.remaining} /></p>
        {offer.tenders.length === 0 ? <Notice tone="info">There is no received payment left to return.</Notice> : null}
        {offer.tenders.map((tender) => <MoneyInput key={tender.mode}
          label={`Return by ${tender.mode.startsWith('other:') ? tender.mode.slice(6) : tender.mode}`} value={amounts[tender.mode] ?? ''}
          hint={`Available: ${tender.remaining.text}`}
          onChange={(value) => setAmounts((prior) => ({ ...prior, [tender.mode]: value }))} />)}
      </> : null}
    </ReasonDialog>
  );
}
