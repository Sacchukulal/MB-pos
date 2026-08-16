/**
 * **The employment side of the Staff screen** — P28.
 *
 * Attendance, leave and salary. P11's People and Roles tabs are next to these
 * and stay what they are: identity, and what somebody may do. This is what they
 * are paid, when they worked, and when they were away.
 *
 * # The first screen built against UI_GUIDELINES §8
 *
 * P27.5 wrote the spacing, type, elevation, icon and layout contracts. This is
 * the first session that did not write them, so it is also the test of whether
 * they survive contact with somebody following them rather than making them:
 *
 * * every gap is named from the contract (`--gap-field` and friends), never a
 *   raw step off the scale;
 * * `Panel`, `Toolbar` and the kit's tabs, so nothing here re-invents a page;
 * * `<Icon>` for every icon — `check-layout.mjs` fails the build on a glyph;
 * * money is mono with `tabular-nums`, and **a column header is a label, not a
 *   figure** (P27.5 fixed that twice; it will come back if nobody says it).
 *
 * # Nothing here computes anything
 *
 * Not one figure on these screens is worked out in TypeScript. The balance is a
 * sum of a ledger in Rust, the payslip arithmetic is `mb_core::employment`, and
 * even "7½ days" is a sentence Rust wrote — because that same sentence appears
 * on a payslip and inside a refusal, and three copies is three chances for one
 * of them to say something different (§6).
 */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  EmptyState,
  Icon,
  Input,
  Modal,
  Panel,
  Row,
  Select,
  Stack,
  Table,
  Toolbar,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { AttendanceView } from '../ipc/generated/AttendanceView';
import type { EmployeeView } from '../ipc/generated/EmployeeView';
import type { LeaveView } from '../ipc/generated/LeaveView';
import type { PayrollView } from '../ipc/generated/PayrollView';
import type { PayrollListView } from '../ipc/generated/PayrollListView';
import type { SalaryView } from '../ipc/generated/SalaryView';
import type { ShiftView } from '../ipc/generated/ShiftView';

/** A day, as the product writes one everywhere: 2026-08-16. */
function dayText(date: Date): string {
  const month = `${date.getMonth() + 1}`.padStart(2, '0');
  const day = `${date.getDate()}`.padStart(2, '0');
  return `${date.getFullYear()}-${month}-${day}`;
}

function todayText(): string {
  return dayText(new Date());
}

/**
 * **A fortnight back — what the attendance and payroll screens open on.**
 *
 * Not today. "Who is in right now" is already answered by the dot beside a name
 * on the People tab; the question these two screens exist for is "what happened
 * over a period", and a screen that opens on a single day answers it with an
 * empty table on every shop that is not mid-shift.
 */
function fortnightAgoText(): string {
  const then = new Date();
  then.setDate(then.getDate() - 14);
  return dayText(then);
}

function useReport() {
  const toast = useToast();
  return useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      else toast.show('danger', String(cause));
    },
    [toast],
  );
}

// ===========================================================================
// Attendance
// ===========================================================================

/**
 * **Who was here, and who was not.**
 *
 * The missed clock-outs are their own panel above the day rather than a row in
 * the middle of it, because they are the only thing on this screen somebody has
 * to *do* something about: until a shift has an end, nobody's hours can be
 * worked out and payroll silently leaves them out.
 */
export function Attendance({ staffId }: { staffId?: string }) {
  const [view, setView] = useState<AttendanceView | null>(null);
  const [from, setFrom] = useState(fortnightAgoText());
  const [to, setTo] = useState(todayText());
  const [fixing, setFixing] = useState<ShiftView | null>(null);
  const report = useReport();

  const load = useCallback(() => {
    call('attendance', { staffId: staffId ?? null, from, to })
      .then(setView)
      .catch(report);
  }, [staffId, from, to, report]);

  useEffect(load, [load]);

  const columns: Column<ShiftView>[] = [
    { key: 'who', header: 'Who', render: (s) => s.staffName },
    { key: 'day', header: 'Day', render: (s) => s.day },
    { key: 'in', header: 'In', render: (s) => <span className="mb-mono">{s.started}</span> },
    {
      key: 'out',
      header: 'Out',
      render: (s) => <span className="mb-mono">{s.ended || '—'}</span>,
    },
    {
      key: 'worked',
      header: 'Worked',
      numeric: true,
      render: (s) => <span className="mb-mono">{s.worked}</span>,
    },
    {
      key: 'verdict',
      header: 'How it went',
      render: (s) => (
        <Row gap="inline">
          <Badge
            tone={
              s.tone === 'ok'
                ? 'ok'
                : s.tone === 'warn'
                  ? 'warn'
                  : s.tone === 'danger'
                    ? 'danger'
                    : 'neutral'
            }
          >
            {s.verdict}
          </Badge>
          {/* A corrected row SAYS it was corrected (D47) — a correction
              nobody can see is indistinguishable from the original. */}
          {s.corrected ? (
            <span title={s.correctionReason ?? undefined}>
              <Badge tone="info">Changed</Badge>
            </span>
          ) : null}
        </Row>
      ),
    },
    {
      key: 'do',
      header: '',
      render: (s) =>
        view?.mayCorrect ? (
          <div className="mb-row">
            <Button small variant="secondary" onClick={() => setFixing(s)}>
              Fix the hours
            </Button>
          </div>
        ) : null,
    },
  ];

  return (
    <Stack gap="section">
      <Toolbar
        end={
          <Row gap="inline">
            <Button variant="secondary" onClick={() => void call('clock_in', { terminalId: null }).then(load).catch(report)}>
              <Icon name="clock" size="sm" />
              Clock in
            </Button>
            <Button variant="secondary" onClick={() => void call('clock_out').then(load).catch(report)}>
              Clock out
            </Button>
          </Row>
        }
      >
        <Row gap="field">
          <Input label="From" value={from} onChange={(e) => setFrom(e.target.value)} />
          <Input label="To" value={to} onChange={(e) => setTo(e.target.value)} />
        </Row>
      </Toolbar>

      {view && view.missed.length > 0 ? (
        <Panel
          title="Nobody clocked out of these"
          note="Their hours cannot be worked out until somebody says when they left."
        >
          <Table
            rows={[...view.missed]}
            columns={columns}
            rowKey={(s) => s.id}
          />
        </Panel>
      ) : null}

      {view && view.shifts.length === 0 ? (
        <EmptyState
          title="Nobody worked on these days"
          body="Clocking in is the same PIN as signing in — there is no second thing to learn."
        />
      ) : (
        <Panel flush>
          <Table rows={[...(view?.shifts ?? [])]} columns={columns} rowKey={(s) => s.id} />
        </Panel>
      )}

      {fixing ? (
        <FixHours
          shift={fixing}
          onClose={() => setFixing(null)}
          onDone={() => {
            setFixing(null);
            load();
          }}
          onFailed={report}
        />
      ) : null}
    </Stack>
  );
}

function FixHours({
  shift,
  onClose,
  onDone,
  onFailed,
}: {
  shift: ShiftView;
  onClose: () => void;
  onDone: () => void;
  onFailed: (cause: unknown) => void;
}) {
  const [started, setStarted] = useState(shift.started);
  const [ended, setEnded] = useState(shift.ended);
  const [reason, setReason] = useState('');

  return (
    <Modal
      open
      title={`${shift.staffName} — ${shift.day}`}
      onClose={onClose}
      actions={
        <>
          <Button variant="quiet" onClick={onClose}>
            Leave it
          </Button>
          <Button
            variant="primary"
            onClick={() =>
              void call('correct_attendance', { id: shift.id, started, ended, reason })
                .then(onDone)
                .catch(onFailed)
            }
          >
            Save the change
          </Button>
        </>
      }
    >
      <Stack gap="field">
        {/* The sentence says WHY this is watched, in the place somebody is
            about to do it. §6: written from the shop's side of the screen. */}
        <p className="mb-muted">
          Changing somebody&rsquo;s hours is recorded with your name, the old
          times and the new ones. You cannot change your own.
        </p>
        <Row gap="field">
          <Input label="In" value={started} onChange={(e) => setStarted(e.target.value)} />
          <Input
            label="Out"
            hint="Leave it empty if they are still here."
            value={ended}
            onChange={(e) => setEnded(e.target.value)}
          />
        </Row>
        <Input
          label="Why"
          hint="Required. A correction nobody can explain looks the same as a mistake."
          value={reason}
          onChange={(e) => setReason(e.target.value)}
        />
      </Stack>
    </Modal>
  );
}

// ===========================================================================
// Leave
// ===========================================================================

export function Leave({ staffId }: { staffId?: string }) {
  const [view, setView] = useState<LeaveView | null>(null);
  const [asking, setAsking] = useState(false);
  const report = useReport();

  const load = useCallback(() => {
    call('leave', { staffId: staffId ?? null })
      .then(setView)
      .catch(report);
  }, [staffId, report]);

  useEffect(load, [load]);

  return (
    <Stack gap="section">
      <Toolbar
        end={
          <Button variant="primary" onClick={() => setAsking(true)}>
            <Icon name="plus" size="sm" />
            Ask for leave
          </Button>
        }
      >
        <span className="mb-muted">
          {view?.mayApprove
            ? 'Everybody’s leave, and what is waiting on you.'
            : 'Your leave.'}
        </span>
      </Toolbar>

      {/* **The balance is a ledger, and the screen says so.** Accrued and taken
          are both shown beside what is left, because a bare number is a number
          somebody will argue with and nobody can check. */}
      <Panel title="What is left" flush>
        <Table
          rows={[...(view?.balances ?? [])]}
          columns={[
            { key: 'type', header: 'Leave', render: (b) => b.leaveType },
            {
              key: 'paid',
              header: '',
              render: (b) =>
                b.isPaid ? null : <Badge tone="warn">Unpaid</Badge>,
            },
            { key: 'accrued', header: 'Granted', render: (b) => b.accruedSays },
            { key: 'taken', header: 'Taken', render: (b) => b.takenSays },
            {
              key: 'left',
              header: 'Left',
              render: (b) => (
                <strong className={b.leftHalves < 0 ? 'mb-leave--over' : undefined}>
                  {b.leftSays}
                </strong>
              ),
            },
          ]}
          rowKey={(b) => b.leaveTypeId}
        />
      </Panel>

      {view && view.pending.length > 0 ? (
        <Panel title="Waiting on you" note="Nobody is away until you say so.">
          <Stack gap="field">
            {view.pending.map((r) => (
              <div key={r.id} className="mb-leave__pending">
                <div className="mb-stack">
                  <strong>
                    {r.staffName} — {r.leaveType}
                  </strong>
                  <span className="mb-muted">
                    {r.from} to {r.to} · {r.daysSays} · {r.reason}
                  </span>
                </div>
                <Decide id={r.id} onDone={load} onFailed={report} />
              </div>
            ))}
          </Stack>
        </Panel>
      ) : null}

      <Panel title="Asked for" flush>
        <Table
          rows={[...(view?.requests ?? [])]}
          columns={[
            { key: 'type', header: 'Leave', render: (r) => r.leaveType },
            { key: 'from', header: 'From', render: (r) => r.from },
            { key: 'to', header: 'To', render: (r) => r.to },
            { key: 'days', header: 'How long', render: (r) => r.daysSays },
            { key: 'reason', header: 'Why', render: (r) => r.reason },
            {
              key: 'state',
              header: 'What happened',
              render: (r) => (
                <Badge
                  tone={
                    r.state === 'approved'
                      ? 'ok'
                      : r.state === 'rejected'
                        ? 'danger'
                        : 'neutral'
                  }
                >
                  {r.state === 'pending' ? 'Waiting' : r.state}
                </Badge>
              ),
            },
          ]}
          rowKey={(r) => r.id}
        />
      </Panel>

      {asking && view ? (
        <AskForLeave
          view={view}
          staffId={staffId ?? view.staffId}
          onClose={() => setAsking(false)}
          onDone={() => {
            setAsking(false);
            load();
          }}
          onFailed={report}
        />
      ) : null}
    </Stack>
  );
}

function Decide({
  id,
  onDone,
  onFailed,
}: {
  id: string;
  onDone: () => void;
  onFailed: (cause: unknown) => void;
}) {
  const [refusing, setRefusing] = useState(false);
  const [note, setNote] = useState('');

  if (refusing) {
    return (
      <Row gap="inline">
        <Input
          label="Why not"
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
        <Button
          variant="danger"
          small
          onClick={() =>
            void call('decide_leave', { id, approve: false, note })
              .then(onDone)
              .catch(onFailed)
          }
        >
          Refuse it
        </Button>
        <Button variant="quiet" small onClick={() => setRefusing(false)}>
          Back
        </Button>
      </Row>
    );
  }

  return (
    <Row gap="inline">
      <Button
        variant="primary"
        small
        onClick={() =>
          void call('decide_leave', { id, approve: true, note: '' })
            .then(onDone)
            .catch(onFailed)
        }
      >
        Approve
      </Button>
      {/* Refusing opens a box for the reason rather than refusing outright —
          a rejection with no reason is one nobody can appeal, and Rust refuses
          it anyway, so asking here is kinder than a toast. */}
      <Button variant="secondary" small onClick={() => setRefusing(true)}>
        Refuse
      </Button>
    </Row>
  );
}

function AskForLeave({
  view,
  staffId,
  onClose,
  onDone,
  onFailed,
}: {
  view: LeaveView;
  staffId: string;
  onClose: () => void;
  onDone: () => void;
  onFailed: (cause: unknown) => void;
}) {
  const [type, setType] = useState(view.balances[0]?.leaveTypeId ?? '');
  const [from, setFrom] = useState(todayText());
  const [to, setTo] = useState(todayText());
  const [halves, setHalves] = useState('2');
  const [reason, setReason] = useState('');

  return (
    <Modal
      open
      title="Ask for leave"
      onClose={onClose}
      actions={
        <>
          <Button variant="quiet" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() =>
              void call('request_leave', {
                staffId,
                leaveTypeId: type,
                from,
                to,
                halfDays: Number(halves) || 0,
                reason,
              })
                .then(onDone)
                .catch(onFailed)
            }
          >
            Ask
          </Button>
        </>
      }
    >
      <Stack gap="field">
        <Select
          label="What kind"
          value={type}
          onChange={(e) => setType(e.target.value)}
          options={view.balances.map((b) => ({
            value: b.leaveTypeId,
            label: `${b.leaveType} — ${b.leftSays} left`,
          }))}
        />
        <Row gap="field">
          <Input label="From" value={from} onChange={(e) => setFrom(e.target.value)} />
          <Input label="To" value={to} onChange={(e) => setTo(e.target.value)} />
        </Row>
        <Input
          label="How many half-days"
          hint="2 is a full day, 1 is half a day. Days are counted in halves so they always add up."
          value={halves}
          onChange={(e) => setHalves(e.target.value.replace(/[^0-9]/g, ''))}
        />
        <Input label="Why" value={reason} onChange={(e) => setReason(e.target.value)} />
      </Stack>
    </Modal>
  );
}

// ===========================================================================
// Salary and payroll
// ===========================================================================

export function Salary({ people }: { people: readonly EmployeeView[] }) {
  const [who, setWho] = useState(people[0]?.id ?? '');
  const [view, setView] = useState<SalaryView | null>(null);
  const [setting, setSetting] = useState(false);
  const [advancing, setAdvancing] = useState(false);
  const report = useReport();

  const load = useCallback(() => {
    if (!who) return;
    call('salary', { staffId: who }).then(setView).catch(report);
  }, [who, report]);

  useEffect(load, [load]);

  return (
    <Stack gap="section">
      <Toolbar
        end={
          view?.mayManage ? (
            <Row gap="inline">
              <Button variant="secondary" onClick={() => setAdvancing(true)}>
                <Icon name="cash" size="sm" />
                Give an advance
              </Button>
              <Button variant="primary" onClick={() => setSetting(true)}>
                Set the salary
              </Button>
            </Row>
          ) : undefined
        }
      >
        <Select
          label="Who"
          value={who}
          onChange={(e) => setWho(e.target.value)}
          options={people.map((p) => ({ value: p.id, label: p.name }))}
        />
      </Toolbar>

      {/* **The whole history, oldest first.** A raise is a new row, never an
          edit — which is what lets last month's payslip recompute to the figure
          it printed, for ever. */}
      <Panel title="What they are paid" note="A raise adds a row. Nothing is ever overwritten." flush>
        <Table
          rows={[...(view?.structures ?? [])]}
          columns={[
            { key: 'from', header: 'From', render: (s) => s.effectiveFrom },
            { key: 'says', header: 'Pay', render: (s) => s.says },
            {
              key: 'parts',
              header: 'Allowances and deductions',
              render: (s) =>
                s.components.length === 0
                  ? '—'
                  : s.components
                      .map(
                        (c) =>
                          `${c.name} ${c.kind === 'deduction' ? '−' : '+'}${c.amount.text}`,
                      )
                      .join(' · '),
            },
          ]}
          rowKey={(s) => s.effectiveFrom}
        />
      </Panel>

      <Panel
        title="Advances"
        note={
          view && view.outstanding.paise > 0n
            ? `${view.outstanding.text} still to come back`
            : 'Nothing outstanding'
        }
        flush
      >
        {view && view.advances.length === 0 ? (
          <EmptyState
            title="No advances"
            body="An advance comes out of the drawer the day it is given, and off the next payroll run."
          />
        ) : (
          <Table
            rows={[...(view?.advances ?? [])]}
            columns={[
              { key: 'given', header: 'Given', render: (a) => a.given },
              {
                key: 'amount',
                header: 'Amount',
                numeric: true,
                render: (a) => <span className="mb-mono">{a.amount.text}</span>,
              },
              {
                key: 'recovered',
                header: 'Come back',
                numeric: true,
                render: (a) => <span className="mb-mono">{a.recovered.text}</span>,
              },
              {
                key: 'outstanding',
                header: 'Still owed',
                numeric: true,
                render: (a) => (
                  <strong className="mb-mono">{a.outstanding.text}</strong>
                ),
              },
              {
                key: 'instalments',
                header: 'Over',
                render: (a) =>
                  a.instalments === 1 ? 'One run' : `${a.instalments} runs`,
              },
            ]}
            rowKey={(a) => a.id}
          />
        )}
      </Panel>

      {setting && view ? (
        <SetSalary
          staffId={who}
          onClose={() => setSetting(false)}
          onDone={() => {
            setSetting(false);
            load();
          }}
          onFailed={report}
        />
      ) : null}

      {advancing ? (
        <GiveAdvance
          staffId={who}
          onClose={() => setAdvancing(false)}
          onDone={() => {
            setAdvancing(false);
            load();
          }}
          onFailed={report}
        />
      ) : null}
    </Stack>
  );
}

function SetSalary({
  staffId,
  onClose,
  onDone,
  onFailed,
}: {
  staffId: string;
  onClose: () => void;
  onDone: () => void;
  onFailed: (cause: unknown) => void;
}) {
  const [from, setFrom] = useState(todayText());
  const [basis, setBasis] = useState('monthly');
  const [amount, setAmount] = useState('');

  return (
    <Modal
      open
      title="Set the salary"
      onClose={onClose}
      actions={
        <>
          <Button variant="quiet" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() =>
              void call('save_salary', {
                edit: { staffId, effectiveFrom: from, basis, amount, components: [] },
              })
                .then(onDone)
                .catch(onFailed)
            }
          >
            Save
          </Button>
        </>
      }
    >
      <Stack gap="field">
        <p className="mb-muted">
          This starts from the date you give and does not change anything before
          it. Last month&rsquo;s payslip will still say what it said.
        </p>
        <Input label="From" value={from} onChange={(e) => setFrom(e.target.value)} />
        <Select
          label="Paid by the"
          value={basis}
          onChange={(e) => setBasis(e.target.value)}
          options={[
            { value: 'monthly', label: 'Month' },
            { value: 'daily', label: 'Day worked' },
            { value: 'hourly', label: 'Hour worked' },
          ]}
        />
        <Input
          label="How much"
          value={amount}
          onChange={(e) => setAmount(e.target.value)}
        />
      </Stack>
    </Modal>
  );
}

function GiveAdvance({
  staffId,
  onClose,
  onDone,
  onFailed,
}: {
  staffId: string;
  onClose: () => void;
  onDone: () => void;
  onFailed: (cause: unknown) => void;
}) {
  const [amount, setAmount] = useState('');
  const [instalments, setInstalments] = useState('1');
  const [reason, setReason] = useState('');

  return (
    <Modal
      open
      title="Give an advance"
      onClose={onClose}
      actions={
        <>
          <Button variant="quiet" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() =>
              void call('give_advance', {
                staffId,
                amount,
                instalments: Number(instalments) || 1,
                reason,
              })
                .then(onDone)
                .catch(onFailed)
            }
          >
            Hand it over
          </Button>
        </>
      }
    >
      <Stack gap="field">
        <p className="mb-muted">
          This comes out of the drawer today and off the next payroll run.
        </p>
        <Input label="How much" value={amount} onChange={(e) => setAmount(e.target.value)} />
        <Input
          label="Over how many runs"
          hint="1 is all of it next month."
          value={instalments}
          onChange={(e) => setInstalments(e.target.value.replace(/[^0-9]/g, ''))}
        />
        <Input label="Why" value={reason} onChange={(e) => setReason(e.target.value)} />
      </Stack>
    </Modal>
  );
}

/**
 * **The payroll screen.**
 *
 * A run is computed, reviewed, then approved — and the three are different
 * states on purpose, because the first thing an owner does with a payroll
 * figure is disagree with one line of it.
 */
export function Payroll() {
  const [list, setList] = useState<PayrollListView | null>(null);
  const [run, setRun] = useState<PayrollView | null>(null);
  const [from, setFrom] = useState(fortnightAgoText());
  const [to, setTo] = useState(todayText());
  const report = useReport();

  const load = useCallback(() => {
    call('payroll_runs').then(setList).catch(report);
  }, [report]);

  useEffect(load, [load]);

  return (
    <Stack gap="section">
      <Toolbar
        end={
          list?.mayManage ? (
            <Button
              variant="primary"
              onClick={() =>
                void call('compute_payroll', { from, to })
                  .then((r) => {
                    setRun(r);
                    load();
                  })
                  .catch(report)
              }
            >
              <Icon name="refresh" size="sm" />
              Work out this period
            </Button>
          ) : undefined
        }
      >
        <Row gap="field">
          <Input label="From" value={from} onChange={(e) => setFrom(e.target.value)} />
          <Input label="To" value={to} onChange={(e) => setTo(e.target.value)} />
        </Row>
      </Toolbar>

      <Panel title="Runs" flush>
        {list && list.runs.length === 0 ? (
          <EmptyState
            title="No payroll yet"
            body="Pick a period and work it out. Nothing moves until you approve it."
          />
        ) : (
          <Table
            rows={[...(list?.runs ?? [])]}
            columns={[
              { key: 'from', header: 'From', render: (r) => r.from },
              { key: 'to', header: 'To', render: (r) => r.to },
              { key: 'people', header: 'People', render: (r) => `${r.people}` },
              {
                key: 'total',
                header: 'Total',
                numeric: true,
                render: (r) => <span className="mb-mono">{r.total.text}</span>,
              },
              {
                key: 'state',
                header: 'Where it is',
                render: (r) => (
                  <Badge
                    tone={
                      r.state === 'approved'
                        ? 'ok'
                        : r.state === 'reversed'
                          ? 'danger'
                          : 'neutral'
                    }
                  >
                    {r.state === 'draft' ? 'Draft' : r.state}
                  </Badge>
                ),
              },
              {
                key: 'do',
                header: '',
                render: (r) => (
                  <div className="mb-row">
                    <Button
                      small
                      onClick={() =>
                        void call('payroll', { runId: r.id }).then(setRun).catch(report)
                      }
                    >
                      Open
                    </Button>
                  </div>
                ),
              },
            ]}
            rowKey={(r) => r.id}
          />
        )}
      </Panel>

      {run ? (
        <RunSheet
          run={run}
          onChanged={(fresh) => {
            setRun(fresh);
            load();
          }}
          onClose={() => setRun(null)}
          onFailed={report}
        />
      ) : null}
    </Stack>
  );
}

function RunSheet({
  run,
  onChanged,
  onClose,
  onFailed,
}: {
  run: PayrollView;
  onChanged: (run: PayrollView) => void;
  onClose: () => void;
  onFailed: (cause: unknown) => void;
}) {
  const [reversing, setReversing] = useState(false);
  const [reason, setReason] = useState('');

  return (
    <Modal open title={`Payroll ${run.from} to ${run.to}`} onClose={onClose} wide>
      <Stack gap="group">
        {/* Rust's sentence, not one built here. §6. */}
        <p className="mb-muted">{run.says}</p>

        {/* **Every step of the arithmetic**, so an owner can add it up by
            hand. Payroll somebody cannot check by hand is payroll they keep
            doing in a notebook. */}
        <Table
          rows={[...run.lines]}
          columns={[
            { key: 'who', header: 'Who', render: (l) => l.staffName },
            { key: 'basis', header: 'Paid by', render: (l) => l.basis },
            { key: 'days', header: 'Worked', render: (l) => l.daysSays },
            {
              key: 'earned',
              header: 'Earned',
              numeric: true,
              render: (l) => <span className="mb-mono">{l.earned.text}</span>,
            },
            {
              key: 'allow',
              header: 'Allowances',
              numeric: true,
              render: (l) => <span className="mb-mono">{l.allowances.text}</span>,
            },
            {
              key: 'unpaid',
              header: 'Unpaid leave',
              numeric: true,
              render: (l) => (
                <span className="mb-mono">{l.unpaidLeaveDeduction.text}</span>
              ),
            },
            {
              key: 'advance',
              header: 'Advance',
              numeric: true,
              render: (l) => <span className="mb-mono">{l.advanceRecovered.text}</span>,
            },
            {
              key: 'net',
              header: 'To hand over',
              numeric: true,
              render: (l) => (
                <strong className="mb-mono">
                  {l.net.text}
                  {l.edited ? ' *' : ''}
                </strong>
              ),
            },
            {
              // **P30 — the paper the person being paid holds** (scope 9.14).
              // Only for somebody who may manage payroll: handing the slip
              // over is the same authority as approving the run.
              key: 'slip',
              header: '',
              render: (l) =>
                run.mayManage ? (
                  <Button
                    small
                    variant="quiet"
                    onClick={() => {
                      // This sheet has no toast of its own; a failure goes the
                      // same way every other failure on it does.
                      call('print_payslip', { runId: run.id, staffId: l.staffId }).catch(
                        onFailed,
                      );
                    }}
                  >
                    <Icon name="printer" size="sm" />
                    Payslip
                  </Button>
                ) : null,
            },
          ]}
          rowKey={(l) => l.id}
        />

        <Row end gap="field">
          <strong className="mb-mono">{run.total.text}</strong>
        </Row>

        {run.state === 'draft' && run.mayManage ? (
          <Row end gap="field">
            <Button
              variant="primary"
              onClick={() =>
                void call('approve_payroll', { runId: run.id, paidBy: 'cash' })
                  .then(onChanged)
                  .catch(onFailed)
              }
            >
              Approve and pay in cash
            </Button>
          </Row>
        ) : null}

        {run.state === 'approved' && run.mayManage ? (
          reversing ? (
            <Row end gap="field">
              <Input
                label="Why"
                value={reason}
                onChange={(e) => setReason(e.target.value)}
              />
              <Button
                variant="danger"
                onClick={() =>
                  void call('reverse_payroll', { runId: run.id, reason })
                    .then(onChanged)
                    .catch(onFailed)
                }
              >
                Reverse it
              </Button>
            </Row>
          ) : (
            <Row end gap="field">
              <Button variant="secondary" onClick={() => setReversing(true)}>
                Reverse this run
              </Button>
            </Row>
          )
        ) : null}
      </Stack>
    </Modal>
  );
}
