/**
 * The gate: a day was left open, and billing waits until it is closed or called a holiday.
 * Every sentence and the button's words come from Rust; this file draws rows and sends choices.
 */

import { useState } from 'react';

import { Button, Modal, Money, Numeric, Table, useToast, type Column } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { DayStateView } from '../ipc/generated/DayStateView';
import type { PendingDayView } from '../ipc/generated/PendingDayView';

export interface DayGateProps {
  state: DayStateView;
  /** Rust answered again — after a switch, or after the press. */
  onChange: (fresh: DayStateView) => void;
  /** The way past the gate when an open order has to be settled first. */
  onEscape: () => void;
  onSignOut: () => void;
}

/** The days Rust currently has switched to Holiday. */
function holidaysIn(state: DayStateView): string[] {
  return state.pending.filter((row) => row.suggested === 'holiday').map((row) => row.day);
}

export function DayGate({ state, onChange, onEscape, onSignOut }: DayGateProps) {
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const complain = (cause: unknown) => {
    if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
  };

  /** One switch moved: ask Rust again, so the button's words follow the choice. */
  const choose = (day: string, choice: 'close' | 'holiday') => {
    const holidays = holidaysIn(state).filter((d) => d !== day);
    if (choice === 'holiday') holidays.push(day);
    call('day_state', { holidays }).then(onChange).catch(complain);
  };

  const press = () => {
    setBusy(true);
    call('close_pending', { holidays: holidaysIn(state) })
      .then((fresh) => {
        onChange(fresh);
        if (fresh.pending.length === 0) toast.show('ok', 'Done. The counter is open.');
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const columns: readonly Column<PendingDayView>[] = [
    { key: 'day', header: 'Day', nowrap: true, render: (row) => row.daySays },
    { key: 'bills', header: 'Bills', numeric: true, render: (row) => <Numeric>{row.bills}</Numeric> },
    { key: 'net', header: 'Net', numeric: true, render: (row) => <Money value={row.net} /> },
    { key: 'cash', header: 'Cash', numeric: true, render: (row) => <Money value={row.cash} /> },
    {
      key: 'upi',
      header: 'UPI / card',
      numeric: true,
      render: (row) => <Money value={row.upiAndCard} />,
    },
    {
      key: 'spent',
      header: 'Spent',
      numeric: true,
      render: (row) => <Money value={row.expenses} />,
    },
    {
      key: 'what',
      header: 'What to do',
      render: (row) =>
        row.openSays ? (
          // A day with an open order is not a choice; it is a sentence.
          <span className="mb-muted mb-daygate__says">{row.openSays}</span>
        ) : (
          <div className="mb-segment" role="group" aria-label={`What to do with ${row.daySays}`}>
            {(['close', 'holiday'] as const).map((choice) => (
              <button
                key={choice}
                type="button"
                className="mb-segment__option"
                aria-pressed={row.suggested === choice}
                // Only a day with nothing on it can be a holiday; Rust decided that.
                disabled={!state.mayAct || (choice === 'holiday' && !row.looksLikeHoliday)}
                onClick={() => choose(row.day, choice)}
              >
                {choice === 'close' ? 'Close' : 'Holiday'}
              </button>
            ))}
          </div>
        ),
    },
  ];

  return (
    <Modal
      open
      wide
      title={state.pendingSays}
      // No way out for somebody who can close the day: it is closed, called a holiday, or they
      // sign out. Somebody who cannot gets "Carry on", which Rust decides.
      onClose={() => undefined}
      actions={
        state.mayAct ? (
          <>
            {state.escapeLabel ? <Button onClick={onEscape}>{state.escapeLabel}</Button> : null}
            <Button
              variant="primary"
              size="lg"
              disabled={busy || state.actionLabel === ''}
              onClick={press}
            >
              {state.actionLabel || 'Nothing to close yet'}
            </Button>
          </>
        ) : (
          // Told, then let through: holding a waiter here stops the shop taking orders. The
          // days stay pending, and whoever can close them meets this same modal.
          <>
            <Button onClick={onSignOut}>Sign out</Button>
            {state.escapeLabel ? (
              <Button variant="primary" onClick={onEscape}>
                {state.escapeLabel}
              </Button>
            ) : null}
          </>
        )
      }
    >
      {state.mayAct ? null : <p className="mb-muted">{state.blockedSays}</p>}
      <Table columns={columns} rows={state.pending} rowKey={(row) => row.day} />
    </Modal>
  );
}
