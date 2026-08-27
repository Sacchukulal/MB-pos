/** Today's bills — the way in to every correction. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  EmptyState,
  Page,
  PageHeader,
  Panel,
  StatCard,
  Table,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { BillRowView } from '../ipc/generated/BillRowView';
import type { DayTotalsView } from '../ipc/generated/DayTotalsView';
import type { PersonView } from '../ipc/generated/PersonView';
import { ReasonDialog, type ReasonKind } from './Reason';

import './corrections.css';

type Pending =
  | { kind: 'void'; bill: BillRowView }
  | { kind: 'reprint'; bill: BillRowView }
  | { kind: 'refund'; bill: BillRowView };

export function Bills() {
  const [bills, setBills] = useState<readonly BillRowView[]>([]);
  const [totals, setTotals] = useState<DayTotalsView | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [approvers, setApprovers] = useState<readonly PersonView[]>([]);
  const toast = useToast();

  const load = useCallback(async () => {
    try {
      setBills(await call('list_bills'));
      setTotals(await call('day_totals'));
    } catch (cause) {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    }
  }, [toast]);

  useEffect(() => {
    void load();
  }, [load]);

  // Who could approve a void.
  useEffect(() => {
    void (async () => {
      try {
        const staff = await call('list_staff');
        setApprovers(staff.filter((p) => p.permissions.includes('bill.void')));
      } catch {
        setApprovers([]);
      }
    })();
  }, []);

  const columns: Column<BillRowView>[] = [
    { key: 'number', header: 'Bill', render: (b) => b.number },
    { key: 'at', header: 'Time', render: (b) => b.at },
    { key: 'type', header: 'Type', render: (b) => b.table ?? b.orderType },
    {
      key: 'total',
      header: 'Total',
      numeric: true,
      render: (b) => <span className="mb-mono">{b.total.text}</span>,
    },
    { key: 'cashier', header: 'Taken by', render: (b) => b.cashier ?? '—' },
    {
      key: 'state',
      header: 'State',
      // Form as well as colour (§2): the word, then the tone, then the reason.
      render: (b) =>
        b.state === 'voided' ? (
          <span className="mb-stack">
            <Badge tone="danger">Voided</Badge>
            <span className="mb-muted">{b.voidReason}</span>
            {b.refunded ? (
              <span className="mb-muted">{b.refunded.text} given back</span>
            ) : null}
          </span>
        ) : b.state === 'cancelled' ? (
          <span className="mb-stack">
            <Badge tone="neutral">Cancelled</Badge>
            <span className="mb-muted">{b.voidReason}</span>
          </span>
        ) : (
          <Badge tone="ok">Paid</Badge>
        ),
    },
    {
      key: 'reprints',
      header: 'Copies',
      render: (b) => (b.reprints > 0 ? `${b.reprints + 1}` : '1'),
    },
    {
      key: 'do',
      header: '',
      render: (b) =>
        b.state === 'cancelled' ? null : (
          <div className="mb-row">
            <Button small onClick={() => setPending({ kind: 'reprint', bill: b })}>
              Reprint
            </Button>
            <Button
              small
              variant="quiet"
              onClick={() => {
                call('bill_pdf', { orderId: b.orderId })
                  .then((saved) => toast.show('ok', saved.message))
                  .catch((cause) => {
                    if (isUiError(cause)) {
                      toast.show('danger', cause.message, cause.detail ?? undefined);
                    }
                  });
              }}
            >
              Invoice PDF
            </Button>
            {b.state === 'settled' ? (
              <Button
                small
                variant="danger"
                onClick={() => setPending({ kind: 'void', bill: b })}
              >
                Void
              </Button>
            ) : (
              <Button small onClick={() => setPending({ kind: 'refund', bill: b })}>
                Give money back
              </Button>
            )}
          </div>
        ),
    },
  ];

  return (
    <Page className="mb-screen">
      <PageHeader
        title="Bills"
        count={bills.length}
        note="A voided bill keeps its number and stays on this list. A gap in the bill book is evidence; a missing bill is a question nobody can answer."
      />

      {totals && bills.length > 0 ? (
        <div className="mb-bills__stats">
          <StatCard label="Taken today" value={totals.gross.text} />
          <StatCard label="Voided" value={totals.voids.text} />
          <StatCard label="Net" value={totals.net.text} />
          {totals.refunded.paise > 0 ? (
            <StatCard label="Given back" value={totals.refunded.text} />
          ) : null}
        </div>
      ) : null}

      {bills.length === 0 ? (
        <EmptyState
          title="No bills yet today"
          body="Every bill you settle appears here, and this is where you void or reprint one."
        />
      ) : (
        <Panel flush>
          <Table rows={bills} columns={columns} rowKey={(b) => b.orderId} />
        </Panel>
      )}

      {pending ? (
        <Correction
          pending={pending}
          approvers={approvers}
          onClose={() => setPending(null)}
          onDone={async (message) => {
            setPending(null);
            toast.show('ok', message);
            await load();
          }}
          onFailed={(message, detail) => toast.show('danger', message, detail)}
        />
      ) : null}
    </Page>
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
  approvers: readonly PersonView[];
  onClose: () => void;
  onDone: (message: string) => void | Promise<void>;
  onFailed: (message: string, detail?: string) => void;
}) {
  const { bill } = pending;
  // Rust decides whether a manager is needed; the screen finds out by asking.
  const [needsApproval, setNeedsApproval] = useState(false);

  const kind: ReasonKind = pending.kind === 'reprint' ? 'reprint' : 'void';
  const what =
    pending.kind === 'void'
      ? `Void bill ${bill.number} — ${bill.total.text}`
      : pending.kind === 'reprint'
        ? `Reprint bill ${bill.number}`
        : `Give back ${bill.total.text} on bill ${bill.number}`;
  const confirmLabel =
    pending.kind === 'void'
      ? 'Void the bill'
      : pending.kind === 'reprint'
        ? 'Print another copy'
        : 'Record the money going back';

  const run = async (reason: string, approver?: { id: string; pin: string }) => {
    try {
      if (pending.kind === 'void') {
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
      approvers={approvers.map((p) => ({ id: p.id, name: p.name }))}
      onCancel={onClose}
      onConfirm={(reason, approver) => void run(reason, approver)}
    />
  );
}
