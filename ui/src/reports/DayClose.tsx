/**
 * **Closing the day** — requirement 9 of the ten, audit B15.
 *
 * The nightly ritual: count the drawer, compare it against what the till
 * expected, say why if the two differ, print the slip, lock the day.
 *
 * # Everything on this screen was computed in Rust
 *
 * The grid sends `{ value, count }` pairs on every keystroke and gets the whole
 * screen back — including the running total, the difference, and the sentence
 * that describes it. **There is exactly one variance calculation in this
 * product**, which is why the number on screen while you type cannot disagree
 * with the number that gets saved. R8, and it is the rule that matters most on
 * the one screen that is entirely about money.
 *
 * # The difference is a sentence
 *
 * "Short by 340.00", not "-340.00". A minus sign in front of an amount on a
 * screen somebody reads at eleven at night, tired, is read wrong eventually.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  ConfirmDialog,
  Input,
  Modal,
  SectionHeader,
  Spinner,
  useToast,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { CountArg } from '../ipc/generated/CountArg';
import type { DayCloseView } from '../ipc/generated/DayCloseView';

export function DayClose() {
  const [view, setView] = useState<DayCloseView | null>(null);
  const [counts, setCounts] = useState<Record<number, number>>({});
  const [reason, setReason] = useState('');
  const [confirming, setConfirming] = useState(false);
  const [reopening, setReopening] = useState(false);
  const [reopenReason, setReopenReason] = useState('');
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const arrived = useCallback((fresh: DayCloseView) => {
    setView(fresh);
    setReason(fresh.reason);
    // The stored count, when there is one, becomes what the boxes show — so a
    // reopened day does not have to be counted again from scratch.
    setCounts(
      Object.fromEntries(fresh.denominations.filter((d) => d.count > 0).map((d) => [d.value, d.count])),
    );
  }, []);

  useEffect(() => {
    call('day_close').then(arrived).catch(complain);
  }, [arrived, complain]);

  /** Every keystroke goes to Rust, and Rust owns the arithmetic. */
  const recount = (value: number, text: string) => {
    const digits = text.replace(/[^0-9]/g, '');
    const next = { ...counts, [value]: digits === '' ? 0 : Number(digits) };
    setCounts(next);
    call('count_cash', { counts: asCounts(next) }).then(setView).catch(complain);
  };

  const close = (print: boolean) => {
    setConfirming(false);
    call('close_day', { counts: asCounts(counts), reason, print })
      .then((fresh) => {
        arrived(fresh);
        toast.show('ok', print ? 'The day is closed and the slip is printing.' : 'The day is closed.');
      })
      .catch(complain);
  };

  if (!view) return <Spinner label="Adding up the day" />;

  return (
    <div className="mb-dayclose">
      <SectionHeader
        title="Close the day"
        note={view.daySays}
        action={
          view.isClosed ? (
            <Badge tone="ok">Closed</Badge>
          ) : (
            <Badge tone="info">Open</Badge>
          )
        }
      />

      {view.isClosed ? (
        <Card>
          <p className="mb-dayclose__closed">{view.closedSays}</p>
          {view.mayClose ? (
            <Button variant="quiet" onClick={() => setReopening(true)}>
              Open this day again
            </Button>
          ) : null}
        </Card>
      ) : null}

      <div className="mb-dayclose__columns">
        <Card>
          <h3 className="mb-dayclose__title">The day</h3>
          {view.takings.map((row) => (
            <div className="mb-dayclose__line" key={row.label}>
              <span>{row.label}</span>
              <span className="mb-numeric">{row.amount.text}</span>
            </div>
          ))}
          <h3 className="mb-dayclose__title">The drawer</h3>
          {view.drawer.map((row) => (
            <div className="mb-dayclose__line" key={row.label}>
              <span>{row.label}</span>
              <span className="mb-numeric">{row.amount.text}</span>
            </div>
          ))}
          <div className="mb-dayclose__line mb-dayclose__line--strong">
            <span>Should be in the drawer</span>
            <span className="mb-numeric">{view.expected.text}</span>
          </div>
        </Card>

        <Card>
          <h3 className="mb-dayclose__title">Count the drawer</h3>
          <div className="mb-dayclose__grid">
            {view.denominations.map((row) => (
              <div className="mb-dayclose__note" key={row.value}>
                <span className="mb-dayclose__face">{row.label}</span>
                <Input
                  aria-label={`How many ${row.label} notes`}
                  inputMode="numeric"
                  disabled={view.isClosed}
                  value={counts[row.value] ? String(counts[row.value]) : ''}
                  placeholder="0"
                  onChange={(event) => recount(row.value, event.target.value)}
                />
                <span className="mb-numeric mb-dayclose__rowtotal">{row.total.text}</span>
              </div>
            ))}
          </div>
          <div className="mb-dayclose__line mb-dayclose__line--strong">
            <span>Counted</span>
            <span className="mb-numeric">{view.counted.text}</span>
          </div>
        </Card>
      </div>

      <Card className={`mb-dayclose__verdict mb-dayclose__verdict--${view.varianceKind}`}>
        <Badge
          tone={
            view.varianceKind === 'exact' ? 'ok' : view.varianceKind === 'short' ? 'danger' : 'warn'
          }
        >
          {view.varianceKind === 'exact' ? '✓' : view.varianceKind === 'short' ? '▼' : '▲'}
        </Badge>
        {/* The sentence, written in Rust. */}
        <strong>{view.varianceSays}</strong>
      </Card>

      {view.needsReason && !view.isClosed ? (
        <Card>
          <p className="mb-dayclose__why">{view.reasonSays}</p>
          <Input
            label="Why is the drawer out?"
            value={reason}
            maxLength={200}
            onChange={(event) => setReason(event.target.value)}
          />
        </Card>
      ) : null}

      {view.carrySays ? <p className="mb-dayclose__carry">{view.carrySays}</p> : null}

      {!view.isClosed && view.mayClose ? (
        <div className="mb-dayclose__actions">
          <Button variant="primary" onClick={() => setConfirming(true)}>
            Close the day
          </Button>
        </div>
      ) : null}
      {!view.mayClose ? (
        <p className="mb-dayclose__why">
          You can see the count, but closing the day needs permission. Ask somebody who can.
        </p>
      ) : null}

      {confirming ? (
        <ConfirmDialog
          open
          title="Close the day?"
          body={`${view.varianceSays} Once it is closed, a bill from today cannot be voided until somebody opens the day again.`}
          confirmLabel="Close and print the slip"
          otherLabel="Close without printing"
          onConfirm={() => close(true)}
          onOther={() => close(false)}
          onCancel={() => setConfirming(false)}
        />
      ) : null}

      {/* A Modal and not a ConfirmDialog: this one asks for something, and a
          confirm dialog that grew a text box would stop being a confirmation. */}
      <Modal
        open={reopening}
        title="Open this day again?"
        onClose={() => setReopening(false)}
        actions={
          <>
            <Button onClick={() => setReopening(false)}>Cancel</Button>
            <Button
              variant="primary"
              onClick={() => {
                setReopening(false);
                call('reopen_day', { reason: reopenReason })
                  .then((fresh) => {
                    arrived(fresh);
                    toast.show('ok', 'The day is open again.');
                  })
                  .catch(complain);
              }}
            >
              Open it
            </Button>
          </>
        }
      >
        <p>
          A bill from a closed day cannot be voided. Opening the day again is
          recorded against your name, with the reason you give here.
        </p>
        <Input
          label="Why?"
          value={reopenReason}
          maxLength={200}
          onChange={(event) => setReopenReason(event.target.value)}
        />
      </Modal>
    </div>
  );
}

/** The boxes, as Rust wants them. */
function asCounts(counts: Record<number, number>): CountArg[] {
  return Object.entries(counts)
    .filter(([, count]) => count > 0)
    .map(([value, count]) => ({ value: Number(value), count }));
}
