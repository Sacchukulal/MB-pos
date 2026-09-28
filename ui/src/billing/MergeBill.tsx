/** Merge processing orders into one payable bill; service stays at each table. */
import { useEffect, useState } from 'react';
import { Button, EmptyState, Hint, Modal, Notice, Numeric } from '../kit';
import { call } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';
import type { TableView } from '../ipc/generated/TableView';
import type { MergePreview } from '../ipc/generated/MergePreview';
import type { FloorView } from '../ipc/generated/FloorView';

export function mergeCandidates(orders: readonly TableView[], orderId: string | null): TableView[] {
  return orders.filter((t) => t.orderId !== null && t.orderId !== orderId && !t.billedInto && !t.billNumber);
}

/** Both entry points use the same Rust preview and confirmation. */
export function MergeConfirmation({ fromOrder, intoOrder, onBack, onMerged, onFailed }: {
  fromOrder: string;
  intoOrder: string;
  onBack: () => void;
  onMerged: (floor: FloorView) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [preview, setPreview] = useState<MergePreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let active = true;
    setPreview(null);
    setFailed(false);
    call('preview_merge', { fromOrder, intoOrder })
      .then((value) => { if (active) setPreview(value); })
      .catch((cause) => { if (active) { setFailed(true); onFailed(cause); } });
    return () => { active = false; };
  }, [fromOrder, intoOrder, onFailed]);
  return <>
    {preview ? <>
      <p>Merge {preview.fromLabel} into {preview.intoLabel}?</p>
      <div className="mb-totals">
        <div className="mb-totals__row"><span>{preview.intoLabel}</span><Numeric>{preview.intoTotal.text}</Numeric></div>
        <div className="mb-totals__row"><span>{preview.fromLabel}</span><Numeric>{preview.fromTotal.text}</Numeric></div>
        <div className="mb-totals__row mb-totals__row--grand"><span>Combined total</span><Numeric>{preview.total.text}</Numeric></div>
      </div>
      <Hint>One bill stays with {preview.intoLabel}. Each table stays occupied until its guests leave. Existing kitchen tickets stay with their orders.</Hint>
    </> : <Hint>{failed ? 'These orders could not be prepared for merging. Go back and choose again.' : 'Checking orders…'}</Hint>}
    <div className="mb-row mb-row--end">
      <Button variant="quiet" disabled={busy} onClick={onBack}>Back</Button>
      <Button variant="primary" disabled={busy || !preview} onClick={() => {
        if (!preview || busy) return;
        setBusy(true);
        call('merge_orders', { fromOrder, intoOrder, previewKey: preview.previewKey }).then(onMerged)
          .catch((cause) => { setPreview(null); setFailed(true); onFailed(cause); })
          .finally(() => setBusy(false));
      }}>Confirm merge</Button>
    </div>
  </>;
}

export function MergeBill({ cart, orders, onClose, onMerged, onFailed }: {
  cart: CartView;
  orders: readonly TableView[];
  onClose: () => void;
  onMerged: (said: string) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [available, setAvailable] = useState<readonly TableView[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [picked, setPicked] = useState<TableView | null>(null);
  const ready = !!cart.orderId && !cart.isEmpty && !cart.billNumber && (cart.kitchenTicketOff || cart.kitchenTold);
  useEffect(() => {
    if (!ready) return;
    let active = true;
    call('combine_candidates').then((value) => { if (active) setAvailable(value); })
      .catch((cause) => { if (active) { setFailed(true); onFailed(cause); } });
    return () => { active = false; };
  }, [ready, onFailed]);
  // Use fresh backend eligibility as well as the counter's current Processing list.
  const others = mergeCandidates(available ?? [], cart.orderId)
    .filter((candidate) => orders.some((order) => order.orderId === candidate.orderId));
  const destination = cart.table ? `Table ${cart.table}` : cart.orderType;
  return <Modal open title="Merge bill" onClose={onClose}>
    {!ready ? <Notice tone="warn">Choose a non-empty, unbilled processing order. Print the kitchen ticket first, or save the bill when kitchen tickets are off.</Notice>
      : picked?.orderId && cart.orderId ? <MergeConfirmation
          fromOrder={picked.orderId} intoOrder={cart.orderId} onBack={() => setPicked(null)}
          onMerged={() => onMerged(`${picked.section === null ? picked.label : `Table ${picked.label}`} joined this bill.`)} onFailed={onFailed} />
      : <>
        <Hint>Merge into {destination}{cart.token ? ` · #${cart.token}` : ''}. Choose another processing order to pay together.</Hint>
        {failed ? <Notice tone="warn">Could not load orders. Close this window and try again.</Notice>
          : available === null ? <Hint>Loading processing orders…</Hint>
          : others.length === 0 ? <EmptyState title="Nothing to merge" hint="No other eligible processing order is available." />
          : <ul className="mb-merge__list" aria-label="Orders that can join this bill">
            {others.map((other) => <li key={other.orderId} className="mb-merge__row">
              <span className="mb-merge__name">{other.section === null ? other.label : `Table ${other.label}`}
                {other.token ? <span className="mb-merge__no"> · #{other.token}</span> : null}</span>
              <Numeric>{other.total?.text ?? ''}</Numeric>
              <Button size="sm" onClick={() => setPicked(other)}>Select</Button>
            </li>)}
          </ul>}
      </>}
    <div className="mb-row mb-row--end"><Button variant="quiet" onClick={onClose}>Close</Button></div>
  </Modal>;
}
