/** The stock count. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  ConfirmDialog,
  EmptyState,
  Input,
  Modal,
  Select,
  Table,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { CountLineView } from '../ipc/generated/CountLineView';
import type { StockCountView } from '../ipc/generated/StockCountView';

import './buying.css';

export function Count() {
  const [view, setView] = useState<StockCountView | null>(null);
  const [material, setMaterial] = useState('');
  const [counted, setCounted] = useState('');
  const [unit, setUnit] = useState('');
  const [sheet, setSheet] = useState<string | null>(null);
  const [approving, setApproving] = useState(false);
  const [explaining, setExplaining] = useState<CountLineView | null>(null);
  /** The same finding as Buying's: a screen that cannot load says so here. */
  const [refused, setRefused] = useState<string | null>(null);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    call('stock_count', { id: null })
      .then((fresh) => {
        setView(fresh);
        setRefused(null);
      })
      .catch((cause) => {
        if (isUiError(cause)) setRefused(cause.message);
        report(cause);
      });
  }, [report]);

  useEffect(load, [load]);

  if (refused !== null) {
    return (
      <div className="mb-count">
        <EmptyState title="Counting is not open on this counter" body={refused} />
      </div>
    );
  }
  if (!view) return <div className="mb-count" />;

  const chosen = view.remaining.find((m) => m.materialId === material);
  /** Whether anything on this sheet may still be changed. */
  const open = view.id !== null && view.stateTag === 'draft';

  const write = () => {
    if (!view.id || material === '' || counted.trim() === '') return;
    call('record_count_line', {
      edit: {
        countId: view.id,
        materialId: material,
        counted,
        unit: unit === '' ? (chosen?.defaultUnit ?? '') : unit,
      },
    })
      .then((fresh) => {
        setView(fresh);
        setMaterial('');
        setCounted('');
        setUnit('');
      })
      .catch(report);
  };

  const columns: Column<CountLineView>[] = [
    { key: 'material', header: 'Material', render: (l) => l.material },
    { key: 'counted', header: 'Counted', render: (l) => l.counted },
    { key: 'book', header: 'Book said', render: (l) => l.book },
    {
      key: 'variance',
      header: 'Difference',
      render: (l) => (
        <span className={l.isShort ? 'mb-count__short' : l.isOver ? 'mb-count__over' : ''}>
          {l.variance}
        </span>
      ),
    },
    {
      key: 'value',
      header: 'Worth',
      numeric: true,
      render: (l) => <span className="mb-mono">{l.varianceValue.text}</span>,
    },
    {
      key: 'why',
      header: 'Why',
      render: (l) =>
        l.needsReason && open ? (
          <Button small variant="quiet" onClick={() => setExplaining(l)}>
            Say why
          </Button>
        ) : (
          <span>{l.note ?? (l.reasonId ? 'explained' : '—')}</span>
        ),
    },
  ];

  // An approved count is SEALED, so the screen must not offer to change it.
  if (open) {
    columns.push({
      key: 'do',
      header: '',
      render: (l) => (
        <Button
          small
          variant="quiet"
          onClick={() =>
            view.id
              ? call('remove_count_line', { countId: view.id, materialId: l.materialId })
                  .then(setView)
                  .catch(report)
              : undefined
          }
        >
          Remove
        </Button>
      ),
    });
  }

  return (
    <div className="mb-count">
      {view.note === '' ? null : <div className="mb-count__attention">{view.note}</div>}

      <div className="mb-row">
        <Button
          variant="quiet"
          onClick={() =>
            call('count_sheet', { location: view.location }).then(setSheet).catch(report)
          }
        >
          Print a count sheet
        </Button>
        {open ? <Badge tone="neutral">{view.state}</Badge> : null}
        {open ? null : (
          <Button
            onClick={() =>
              call('open_stock_count', { location: '' }).then(setView).catch(report)
            }
          >
            {view.id === null ? 'Start counting' : 'Start a new count'}
          </Button>
        )}
        {/* A sealed count says so, and the only thing offered is a NEW one. */}
        {view.id !== null && !open ? <Badge tone="neutral">{view.state}</Badge> : null}
      </div>

      {view.id === null ? (
        <EmptyState
          title="Nothing is being counted"
          body="Print the sheet, walk the store and write down what is actually there. Then type it in here — the difference in rupees is what tells you where the food is going."
        />
      ) : (
        <>
          {open ? (
          <div className="mb-row">
            <Select
              label="Material"
              value={material}
              onChange={(e) => {
                setMaterial(e.target.value);
                const found = view.remaining.find((m) => m.materialId === e.target.value);
                setUnit(found?.defaultUnit ?? '');
              }}
              options={[
                { value: '', label: 'Pick one' },
                ...view.remaining.map((m) => ({ value: m.materialId, label: m.material })),
              ]}
            />
            <Input
              label="Counted"
              value={counted}
              onChange={(e) => setCounted(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') write();
              }}
            />
            <Select
              label="Unit"
              value={unit}
              onChange={(e) => setUnit(e.target.value)}
              options={
                chosen
                  ? [
                      { value: chosen.baseUnit, label: chosen.baseUnit },
                      ...chosen.units.map((u) => ({ value: u, label: u })),
                    ]
                  : [{ value: '', label: '—' }]
              }
            />
            <Button onClick={write}>Write it down</Button>
          </div>
          ) : null}

          {view.lines.length === 0 ? (
            <EmptyState
              title="Nothing written down yet"
              body="Pick a material, type what is on the shelf, press Enter. You do not have to count everything — only what you counted is adjusted."
            />
          ) : (
            <>
              <Table rows={view.lines} columns={columns} rowKey={(l) => l.materialId} />
              {/* One sentence, composed in Rust. */}
              <p className="mb-count__says">{view.totalsSays}</p>
            </>
          )}

          {/*
            What approving will do, said before anybody presses it — and only while there is
            still something to press.
          */}
          {open ? <p className="mb-count__says">{view.effect}</p> : null}

          {open ? (
            <div className="mb-row mb-row--end">
              <Button
                variant="quiet"
                onClick={() =>
                  view.id
                    ? call('abandon_stock_count', { id: view.id, reason: 'Given up on' })
                        .then(setView)
                        .catch(report)
                    : undefined
                }
              >
                Give up on this count
              </Button>
              {view.mayApprove ? (
                <Button onClick={() => setApproving(true)} disabled={view.lines.length === 0}>
                  Approve
                </Button>
              ) : null}
            </div>
          ) : (
            <p className="mb-count__says">
              This count is finished and cannot be changed. Its adjustments are in the
              movements list, with who approved them.
            </p>
          )}
        </>
      )}

      {view.history.length > 0 ? (
        <Table
          rows={view.history}
          columns={[
            { key: 'date', header: 'Day', render: (h) => h.date },
            { key: 'where', header: 'Where', render: (h) => h.location },
            { key: 'state', header: '', render: (h) => <Badge tone="neutral">{h.state}</Badge> },
            { key: 'materials', header: 'Materials', numeric: true, render: (h) => String(h.materials) },
            {
              key: 'value',
              header: 'Out by',
              numeric: true,
              render: (h) => <span className="mb-mono">{h.value.text}</span>,
            },
          ]}
          rowKey={(h) => h.id}
        />
      ) : null}

      {sheet !== null ? (
        <Modal open title="Count sheet" onClose={() => setSheet(null)} wide>
          {/* No book quantity on it, on purpose. */}
          <pre className="mb-count__sheet">{sheet}</pre>
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => navigator.clipboard.writeText(sheet)}>
              Copy
            </Button>
            <Button onClick={() => window.print()}>Print</Button>
          </div>
        </Modal>
      ) : null}

      {approving && view.id ? (
        <ConfirmDialog
          open
          title="Approve this count?"
          body={`${view.effect} It cannot be changed afterwards, and every adjustment says who approved it.`}
          confirmLabel="Approve"
          onConfirm={() => {
            call('approve_stock_count', { id: view.id as string })
              .then((fresh) => {
                setView(fresh);
                setApproving(false);
                toast.show('ok', 'The book now matches what you counted.');
              })
              .catch((cause) => {
                setApproving(false);
                report(cause);
              });
          }}
          onCancel={() => setApproving(false)}
        />
      ) : null}

      {explaining && view.id ? (
        <Explain
          line={explaining}
          reasons={view.reasons}
          onClose={() => setExplaining(null)}
          onSaved={(fresh) => {
            setView(fresh);
            setExplaining(null);
          }}
          countId={view.id}
          onError={report}
        />
      ) : null}
    </div>
  );
}

function Explain({
  line,
  reasons,
  countId,
  onClose,
  onSaved,
  onError,
}: {
  line: CountLineView;
  reasons: { id: string; text: string }[];
  countId: string;
  onClose: () => void;
  onSaved: (fresh: StockCountView) => void;
  onError: (cause: unknown) => void;
}) {
  const [reason, setReason] = useState(reasons[0]?.id ?? '');
  const [note, setNote] = useState('');

  return (
    <Modal open title={`${line.material} is ${line.variance}`} onClose={onClose}>
      <div className="mb-buying__form">
        <p className="mb-count__says">
          That is {line.varianceValue.text}. Saying why is what turns a number into something
          somebody can fix.
        </p>
        <Select
          label="Why"
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          options={reasons.map((r) => ({ value: r.id, label: r.text }))}
        />
        <Input label="Anything else" value={note} onChange={(e) => setNote(e.target.value)} />
        <div className="mb-row mb-row--end">
          <Button variant="quiet" onClick={onClose}>
            Not now
          </Button>
          <Button
            onClick={() =>
              call('explain_count_line', {
                countId,
                materialId: line.materialId,
                reasonId: reason === '' ? null : reason,
                note,
              })
                .then(onSaved)
                .catch(onError)
            }
          >
            Save
          </Button>
        </div>
      </div>
    </Modal>
  );
}
