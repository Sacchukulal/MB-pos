/** Reports › Days: today's state, the last fourteen days, holidays ahead, and the drawer count. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  ConfirmDialog,
  Input,
  Modal,
  Money,
  Numeric,
  PageHeader,
  Panel,
  Row,
  Scroller,
  Sections,
  Spinner,
  Table,
  useToast,
  type BadgeTone,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { CountArg } from '../ipc/generated/CountArg';
import type { DayRowView } from '../ipc/generated/DayRowView';
import type { DaysView } from '../ipc/generated/DaysView';
import type { DrawerView } from '../ipc/generated/DrawerView';
import type { UnconfirmedView } from '../ipc/generated/UnconfirmedView';

/** The chip for a day's state, in a colour that is never the only signal. */
const TONES: Record<string, BadgeTone> = {
  open: 'info',
  closed: 'ok',
  holiday: 'neutral',
  pending: 'warn',
};
const STATE_WORDS: Record<string, string> = {
  open: 'Open',
  closed: 'Closed',
  holiday: 'Holiday',
  pending: 'Not closed',
};

export function Days() {
  const [view, setView] = useState<DaysView | null>(null);
  const [drawer, setDrawer] = useState<DrawerView | null>(null);
  const [counts, setCounts] = useState<Record<number, number>>({});
  const [reason, setReason] = useState('');
  const [confirming, setConfirming] = useState(false);
  /** The day being opened again, and why. */
  const [reopening, setReopening] = useState<string | null>(null);
  const [why, setWhy] = useState('');
  /** A day that has not come yet, typed as a date. */
  const [ahead, setAhead] = useState('');
  /** Every electronic payment nobody has said arrived yet. */
  const [unconfirmed, setUnconfirmed] = useState<readonly UnconfirmedView[]>([]);
  const [waiting, setWaiting] = useState('');
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const drawerArrived = useCallback((fresh: DrawerView) => {
    setDrawer(fresh);
    setReason(fresh.reason);
    // The stored count, when there is one, becomes what the boxes show.
    setCounts(
      Object.fromEntries(
        fresh.denominations.filter((d) => d.count > 0).map((d) => [d.value, d.count]),
      ),
    );
  }, []);

  useEffect(() => {
    call('days').then(setView).catch(complain);
    // The drawer is optional: a shop that never counts still closes its days.
    call('count_cash', { counts: null })
      .then((fresh) => {
        if (fresh) drawerArrived(fresh);
      })
      .catch(() => setDrawer(null));
    // Silent on failure: a person who may not read payments still closes the day.
    call('payments')
      .then((fresh) => {
        if (fresh && Array.isArray(fresh.unconfirmed)) {
          setUnconfirmed(fresh.unconfirmed);
          setWaiting(fresh.says);
        }
      })
      .catch(() => setUnconfirmed([]));
  }, [complain, drawerArrived]);

  /** Every write answers with the whole screen. */
  const act = (promise: Promise<DaysView>, said: string) => {
    promise
      .then((fresh) => {
        setView(fresh);
        toast.show('ok', said);
        // Closing today changes what the drawer expects tomorrow; read it again.
        call('count_cash', { counts: asCounts(counts) })
          .then((fresh) => {
            if (fresh) setDrawer(fresh);
          })
          .catch(() => undefined);
      })
      .catch(complain);
  };

  const recount = (value: number, text: string) => {
    const digits = text.replace(/[^0-9]/g, '');
    const next = { ...counts, [value]: digits === '' ? 0 : Number(digits) };
    setCounts(next);
    call('count_cash', { counts: asCounts(next) }).then(setDrawer).catch(complain);
  };

  const writeCount = (print: boolean) => {
    setConfirming(false);
    call('count_drawer', { counts: asCounts(counts), reason, print })
      .then((fresh) => {
        drawerArrived(fresh);
        toast.show('ok', print ? 'The count is written and the slip is printing.' : 'The count is written.');
      })
      .catch(complain);
  };

  if (!view) return <Spinner label="Reading the days" />;

  const columns: readonly Column<DayRowView>[] = [
    { key: 'day', header: 'Day', nowrap: true, render: (row) => row.daySays },
    {
      key: 'state',
      header: 'State',
      render: (row) => <Badge tone={TONES[row.state] ?? 'neutral'}>{STATE_WORDS[row.state] ?? row.state}</Badge>,
    },
    { key: 'bills', header: 'Bills', numeric: true, render: (row) => <Numeric>{row.bills}</Numeric> },
    { key: 'net', header: 'Net', numeric: true, render: (row) => <Money value={row.net} /> },
    { key: 'says', header: '', render: (row) => <span className="mb-muted">{row.closedSays}</span> },
    {
      key: 'act',
      header: '',
      render: (row) => (
        <Row gap="inline" wrap={false}>
          {view.mayAct && row.state === 'pending' ? (
            <Button size="sm" onClick={() => act(call('close_day', { day: row.day }), `${row.daySays} is closed.`)}>
              Close
            </Button>
          ) : null}
          {view.mayAct && row.isLocked ? (
            <Button size="sm" variant="quiet" onClick={() => setReopening(row.day)}>
              Open again
            </Button>
          ) : null}
          {row.state === 'holiday' && view.mayAct ? (
            <Button
              size="sm"
              variant="quiet"
              onClick={() => act(call('unmark_holiday', { days: [row.day] }), `${row.daySays} is not a holiday.`)}
            >
              Not a holiday
            </Button>
          ) : row.mayBeHoliday ? (
            <Button
              size="sm"
              variant="quiet"
              onClick={() => act(call('mark_holiday', { days: [row.day] }), `${row.daySays} is a holiday.`)}
            >
              Holiday
            </Button>
          ) : null}
        </Row>
      ),
    },
  ];

  const todayActions = view.mayAct ? (
    view.todayState === 'open' ? (
      <>
        <Button onClick={() => act(call('mark_holiday', { days: [view.today] }), 'Today is a holiday.')}>
          Mark today a holiday
        </Button>
        <Button variant="primary" onClick={() => act(call('close_day', { day: view.today }), 'Today is closed.')}>
          Close today
        </Button>
      </>
    ) : (
      <Button onClick={() => setReopening(view.today)}>Open today again</Button>
    )
  ) : null;

  return (
    <Scroller className="mb-days">
      <PageHeader
        title="Days"
        subtitle={view.todayClosedSays || view.todaySays}
        note={view.carrySays || undefined}
        actions={todayActions}
      />

      <Sections>
        <Panel title="The last 14 days" flush>
          <Table columns={columns} rows={view.days} rowKey={(row) => row.day} />
        </Panel>

        {view.mayPlanHoliday ? (
          <Panel
            title="Holidays ahead"
            actions={
              <Row gap="inline" wrap={false}>
                <Input
                  aria-label="A day the shop will be shut"
                  type="date"
                  value={ahead}
                  onChange={(event) => setAhead(event.target.value)}
                />
                <Button
                  disabled={ahead === ''}
                  onClick={() => {
                    act(call('mark_holiday', { days: [ahead] }), 'Marked as a holiday.');
                    setAhead('');
                  }}
                >
                  Mark a holiday
                </Button>
              </Row>
            }
            flush={view.upcoming.length > 0}
          >
            {view.upcoming.length > 0 ? (
              <Table
                columns={[
                  { key: 'day', header: 'Day', nowrap: true, render: (row: DayRowView) => row.daySays },
                  { key: 'says', header: '', render: (row: DayRowView) => <span className="mb-muted">{row.closedSays}</span> },
                  {
                    key: 'act',
                    header: '',
                    render: (row: DayRowView) => (
                      <Button
                        size="sm"
                        variant="quiet"
                        onClick={() => act(call('unmark_holiday', { days: [row.day] }), `${row.daySays} is not a holiday.`)}
                      >
                        Not a holiday
                      </Button>
                    ),
                  },
                ]}
                rows={view.upcoming}
                rowKey={(row) => row.day}
              />
            ) : (
              <p className="mb-muted">None yet.</p>
            )}
          </Panel>
        ) : null}

        {drawer ? (
          <Panel
            title="Count the drawer"
            note="Optional. Counting the box under this till records what was in it; it does not close the day."
            actions={drawer.countedSays ? <span className="mb-muted">{drawer.countedSays}</span> : null}
          >
            {drawer.tillsSay ? <p className="mb-muted">{drawer.tillsSay}</p> : null}
            <div className="mb-dayclose__columns">
              <div>
                {drawer.takings.map((row) => (
                  <div className="mb-dayclose__line" key={row.label}>
                    <span>{row.label}</span>
                    <Money value={row.amount} />
                  </div>
                ))}
                {drawer.drawer.map((row) => (
                  <div className="mb-dayclose__line" key={row.label}>
                    <span>{row.label}</span>
                    <Money value={row.amount} />
                  </div>
                ))}
                <div className="mb-dayclose__line mb-dayclose__line--strong">
                  <span>Should be in the drawer</span>
                  <Money value={drawer.expected} />
                </div>
              </div>

              <div>
                <div className="mb-dayclose__grid">
                  {drawer.denominations.map((row) => (
                    <div className="mb-dayclose__note" key={row.value}>
                      <span className="mb-dayclose__face">{row.label}</span>
                      <Input
                        aria-label={`How many ${row.label} notes`}
                        inputMode="numeric"
                        disabled={!drawer.mayCount}
                        value={counts[row.value] ? String(counts[row.value]) : ''}
                        placeholder="0"
                        onChange={(event) => recount(row.value, event.target.value)}
                      />
                      <span className="mb-dayclose__rowtotal">{row.total.text}</span>
                    </div>
                  ))}
                </div>
                <div className="mb-dayclose__line mb-dayclose__line--strong">
                  <span>Counted</span>
                  <Money value={drawer.counted} />
                </div>
              </div>
            </div>

            <Row>
              <Badge
                tone={
                  drawer.varianceKind === 'exact' ? 'ok' : drawer.varianceKind === 'short' ? 'danger' : 'warn'
                }
              >
                {drawer.varianceKind === 'exact' ? 'Exact' : drawer.varianceKind === 'short' ? 'Short' : 'Over'}
              </Badge>
              {/* The sentence, written in Rust. */}
              <strong>{drawer.varianceSays}</strong>
            </Row>

            {unconfirmed.length > 0 ? (
              <div className="mb-dayclose__grid">
                <p className="mb-muted">{waiting}</p>
                {unconfirmed.map((row) => (
                  <div className="mb-dayclose__line" key={`${row.orderId}-${row.seq}`}>
                    <span>
                      {row.bill} · {row.mode}
                      {row.reference ? ` · ${row.reference}` : ''}
                    </span>
                    <Money value={row.amount} />
                    <Button
                      size="sm"
                      onClick={() => {
                        call('confirm_payment', { orderId: row.orderId, seq: row.seq, reference: '' })
                          .then((fresh) => {
                            setUnconfirmed(fresh.unconfirmed);
                            setWaiting(fresh.says);
                            toast.show('ok', 'Marked as arrived.');
                          })
                          .catch(complain);
                      }}
                    >
                      It arrived
                    </Button>
                  </div>
                ))}
              </div>
            ) : null}

            {drawer.needsReason ? (
              <Input
                label="Why is the drawer out?"
                hint={drawer.reasonSays}
                value={reason}
                maxLength={200}
                onChange={(event) => setReason(event.target.value)}
              />
            ) : null}

            {drawer.mayCount ? (
              <Row end>
                <Button variant="primary" onClick={() => setConfirming(true)}>
                  Write the count
                </Button>
              </Row>
            ) : null}
          </Panel>
        ) : null}
      </Sections>

      {confirming && drawer ? (
        <ConfirmDialog
          open
          title="Write the count?"
          body={drawer.varianceSays}
          confirmLabel="Write it and print the slip"
          otherLabel="Write it without printing"
          onConfirm={() => writeCount(true)}
          onOther={() => writeCount(false)}
          onCancel={() => setConfirming(false)}
        />
      ) : null}

      {/* A Modal and not a ConfirmDialog: this one asks for something. */}
      <Modal
        open={reopening !== null}
        title="Open this day again?"
        note="A bill from a closed day cannot change. Opening the day again is recorded against your name, with the reason you give here."
        onClose={() => setReopening(null)}
        actions={
          <>
            <Button onClick={() => setReopening(null)}>Cancel</Button>
            <Button
              variant="primary"
              onClick={() => {
                const day = reopening ?? '';
                setReopening(null);
                act(call('reopen_day', { day, reason: why }), 'The day is open again.');
                setWhy('');
              }}
            >
              Open it
            </Button>
          </>
        }
      >
        <Input label="Why?" value={why} maxLength={200} onChange={(event) => setWhy(event.target.value)} />
      </Modal>
    </Scroller>
  );
}

/** The boxes, as Rust wants them. */
function asCounts(counts: Record<number, number>): CountArg[] {
  return Object.entries(counts)
    .filter(([, count]) => count > 0)
    .map(([value, count]) => ({ value: Number(value), count }));
}
