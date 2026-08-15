/**
 * **Today's bills — the way in to every correction.**
 *
 * P09's grid shows open orders only, so before this screen a settled bill was
 * unreachable: void, reprint and refund had no door. That is why it is here
 * rather than in P18's reports, and why it is deliberately *not* a report — it
 * is today, newest first, with four buttons on it.
 *
 * # A voided bill stays in the list
 *
 * Badged **Voided**, with its reason under it, and never removed. A bill that
 * vanishes is exactly what audit **B5** is complaining about — *"a CA will ask
 * where bill 1042 went"* — and the footer says gross, voids and net so the
 * three always tie on screen as well as in the database.
 *
 * The state is carried by the word, the badge and the reason rather than by
 * colour alone (§2) — checked by looking at it, which is also how the raw JSON
 * on the History screen was found.
 */

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

  // Who could approve a void. Needs `staff.manage`, so a cashier simply gets an
  // empty list and the dialog says a manager is needed — which is true.
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
        subtitle="Every bill settled today — and where a wrong one is voided or reprinted."
        count={bills.length}
      />

      {totals ? (
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

      <p className="mb-muted">
        A voided bill keeps its number and stays on this list. A gap in the bill
        book is evidence; a missing bill is a question nobody can answer.
      </p>

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
  // **Rust decides whether a manager is needed; the screen finds out by
  // asking.**
  //
  // The first version passed `needsApproval={false}` always, which was a dead
  // end: over the shop's threshold Rust refuses with `void.needs_approval`, and
  // a dialog with no PIN box gives the cashier no way to comply. So the refusal
  // turns the fields on and the same dialog is used again — nobody is asked for
  // a manager's PIN who did not need one, and nobody is stuck who did.
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
          // The paise integer Rust sent, handed straight back. TypeScript does
          // not compute it, which is R8 and the reason MoneyView has both
          // halves.
          amountPaise: Number(bill.total.paise),
          mode: 'cash',
          reason,
        });
        await onDone(`${bill.total.text} recorded as given back.`);
      }
    } catch (cause) {
      if (isUiError(cause) && cause.code === 'void.needs_approval') {
        // Not a failure the cashier caused: the dialog stays open and grows a
        // PIN box.
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
