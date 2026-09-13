/** Every report, on one screen. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  DateRangePicker,
  EmptyState,
  Icon,
  Locked,
  Pick,
  Scroller,
  SectionHeader,
  Spinner,
  Table,
  useToast,
  type Column,
} from '../kit';
import { call, isLicenceRefusal, isUiError } from '../ipc/call';
import { keep, remember } from '../remember';
import { useMay } from '../shell/permissions';
import { Bills } from './Bills';
import { Dashboard } from './Dashboard';
import { Days } from './Days';
import type { PeriodChoiceView } from '../ipc/generated/PeriodChoiceView';
import type { ReportEntryView } from '../ipc/generated/ReportEntryView';
import type { ReportListView } from '../ipc/generated/ReportListView';
import type { ReportView } from '../ipc/generated/ReportView';

import './reports.css';

/** A row, as the table sees it: the cells, plus an index to key on. */
interface Line {
  at: number;
  cells: readonly string[];
}

/** `initial` opens a part straight away — the alert for a closed day lands on Day open/close. */
export function Reports({
  onGoTo,
  initial,
}: {
  onGoTo?: (screen: string) => void;
  initial?: string | null;
}) {
  const [list, setList] = useState<ReportListView | null>(null);
  /** Taking a report out of the building is its own permission on top of reading it. */
  const mayExport = useMay()('reports.export');
  /** The licence saying no, held rather than flashed. */
  const [locked, setLocked] = useState<string>('');
  /**
   * Whether this person reads reports at all. A cashier who may close the day but not read
   * reports comes here for Day open/close and sees nothing else.
   */
  const [mayReport, setMayReport] = useState(true);
  // The dashboard is what this screen opens on.
  const [chosen, setChosen] = useState<string>(initial === DAYS ? DAYS : TODAY);
  useEffect(() => {
    if (initial === DAYS) setChosen(DAYS);
  }, [initial]);
  // Which groups are unfolded. The rail is nine groups long; folded is how a person finds
  // anything in it, and which ones are open is a look preference, not a fact about the shop.
  const [unfolded, setUnfolded] = useState<readonly string[]>(() => opened());
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [report, setReport] = useState<ReportView | null>(null);
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const fold = (group: string) => {
    const next = unfolded.includes(group)
      ? unfolded.filter((name) => name !== group)
      : [...unfolded, group];
    setUnfolded(next);
    keep(FOLDS, next.join('\n'));
  };

  const complain = useCallback(
    (cause: unknown) => {
      // A refusal is an answer, not a fault: it belongs on the screen, and a toast on top of it
      // would say the same thing twice and then vanish.
      if (isLicenceRefusal(cause)) {
        setLocked(cause.message);
        return;
      }
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  // The list, and with it the period presets — which come from Rust because "today" is the
  // shop's business day and only Rust knows when that starts.
  useEffect(() => {
    call('report_list')
      .then((fresh) => {
        setList(fresh);
        const today = fresh.periods[0];
        if (today) {
          setFrom(today.from);
          setTo(today.to);
        }
      })
      .catch((cause) => {
        // No permission is an answer too: the rail keeps the one entry this person may open.
        if (isUiError(cause) && cause.code === 'auth.denied') {
          setMayReport(false);
          setChosen(DAYS);
          return;
        }
        complain(cause);
      });
  }, [complain]);

  // One effect, one call: whenever the report or the period changes, ask again.
  useEffect(() => {
    // The dashboard, the bills and the days are not reports.
    if (!from || !to || chosen === DAYS || chosen === TODAY || chosen === BILLS) return;
    setBusy(true);
    call('report', { id: chosen, period: { from, to } })
      .then(setReport)
      .catch((cause) => {
        setReport(null);
        complain(cause);
      })
      .finally(() => setBusy(false));
  }, [chosen, from, to, complain]);

  const save = (command: 'report_csv' | 'report_pdf') => {
    call(command, { id: chosen, period: { from, to } })
      .then((saved) => toast.show('ok', saved.message, saved.path))
      .catch(complain);
  };

  /** The same figures, on the roll the bills come off. */
  const print = () => {
    call('report_print', { id: chosen, period: { from, to } })
      .then((says) => toast.show('ok', says))
      .catch(complain);
  };

  /** Send the figures to somebody. */
  const share = (channel: 'copy' | 'whats_app' | 'email' | 'folder') => {
    call('share_report', { id: chosen, period: { from, to }, channel })
      .then((shared) => {
        if (channel === 'copy') void navigator.clipboard.writeText(shared.text);
        toast.show('ok', shared.says, shared.caveat === '' ? undefined : shared.caveat);
      })
      .catch(complain);
  };

  // A licence refusal closes the reports, never the bills or the days: voiding, reprinting
  // and closing the day are billing, and billing is never behind the plan.
  if (!list && !locked && mayReport) return <Spinner label="Opening the reports" />;

  const columns: readonly Column<Line>[] =
    report?.columns.map((spec, index) => ({
      key: `${index}`,
      header: spec.header,
      numeric: spec.numeric,
      render: (line: Line) => line.cells[index] ?? '',
    })) ?? [];
  const lines: readonly Line[] = report?.rows.map((cells, at) => ({ at, cells })) ?? [];

  return (
    <div className="mb-railpage">
      <Scroller inset className="mb-reports__rail">
        {/* At the top and on their own: the dashboard, the bills, and the day itself. */}
        <div className="mb-reports__group">
          {[
            { id: TODAY, label: 'Dashboard', shown: mayReport },
            { id: BILLS, label: 'Bills', shown: mayReport },
            { id: DAYS, label: 'Day open/close', shown: true },
          ]
            .filter((entry) => entry.shown)
            .map((entry) => (
              <Pick
                key={entry.id}
                className="mb-reports__pick"
                current={chosen === entry.id}
                onClick={() => setChosen(entry.id)}
              >
                {entry.label}
              </Pick>
            ))}
        </div>
        {groups(list?.reports ?? []).map(([group, entries]) => {
          const open = unfolded.includes(group);
          return (
            <div className="mb-reports__group" key={group}>
              {/* The heading IS the switch — a screen reader gets both the level and the state. */}
              <h2 className="mb-reports__grouphead">
                <Pick
                  className="mb-reports__grouptitle"
                  aria-expanded={open}
                  aria-controls={`reports-${group}`}
                  title={open ? `Fold ${group} away` : `Show ${group}`}
                  onClick={() => fold(group)}
                >
                  <Icon name={open ? 'chevron-down' : 'chevron-right'} size="sm" />
                  {group}
                </Pick>
              </h2>
              <div className="mb-reports__list" id={`reports-${group}`} hidden={!open}>
                {entries.map((entry) => (
                  <Pick
                    key={entry.id}
                    className="mb-reports__pick"
                    current={entry.id === chosen}
                    onClick={() => setChosen(entry.id)}
                  >
                    {entry.title}
                  </Pick>
                ))}
              </div>
            </div>
          );
        })}
      </Scroller>

      <div className="mb-reports__body">
        {chosen === BILLS ? (
          <Bills onGoTo={onGoTo} />
        ) : chosen === DAYS ? (
          <Days />
        ) : locked || !list ? (
          <Locked says={locked} onOpenAccount={onGoTo ? () => onGoTo('account') : undefined} />
        ) : chosen === TODAY ? (
          <Dashboard presets={list.periods} />
        ) : (
          <>
        <div className="mb-reports__when">
          <div className="mb-reports__presets">
            {list.periods.map((choice: PeriodChoiceView) => (
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
        </div>

        {report ? (
          <>
            <SectionHeader
              title={report.title}
              note={report.subtitle}
              action={
                mayExport ? (
                  <div className="mb-reports__exports">
                    {/*
                      Paper first, because a shop with a printer on the counter asks for it
                      before it asks for a file; then sending it, then saving it.
                    */}
                    <Button size="sm" variant="quiet" onClick={print}>
                      <Icon name="printer" size="sm" />
                      Print
                    </Button>
                    <Button size="sm" variant="quiet" onClick={() => share('copy')}>
                      Copy
                    </Button>
                    <Button size="sm" variant="quiet" onClick={() => share('whats_app')}>
                      WhatsApp
                    </Button>
                    <Button size="sm" variant="quiet" onClick={() => share('email')}>
                      Email
                    </Button>
                    <Button size="sm" variant="quiet" onClick={() => save('report_csv')}>
                      Save as CSV
                    </Button>
                    <Button size="sm" variant="quiet" onClick={() => save('report_pdf')}>
                      Save as PDF
                    </Button>
                  </div>
                ) : undefined
              }
            />

            {report.compare ? (
              <div className="mb-reports__compare">
                <Badge
                  tone={
                    report.compare.direction === 'up'
                      ? 'ok'
                      : report.compare.direction === 'down'
                        ? 'warn'
                        : 'neutral'
                  }
                >
                  <Icon
                    name={
                      report.compare.direction === 'up'
                        ? 'chevron-up'
                        : report.compare.direction === 'down'
                          ? 'chevron-down'
                          : 'minus'
                    }
                    size="sm"
                  />
                </Badge>
                {/* The whole sentence, written in Rust. */}
                <span>{report.compare.summary}</span>
                <span className="mb-reports__against">
                  Compared against {report.compare.period}
                </span>
              </div>
            ) : null}

            <Scroller wide className="mb-reports__sheet">
              <Table
                columns={columns}
                rows={lines}
                rowKey={(line) => `${line.at}`}
                // The totals belong to the table: a second table underneath could not agree
                // with it about column widths, and a column of rupees that does not line up
                // looks broken (§3).
                footer={report.totals ?? undefined}
                empty={
                  <EmptyState
                    title="Nothing in this period"
                    hint="Pick a different date range, or a different report."
                  />
                }
              />
            </Scroller>

            {report.notes.map((note) => (
              <p className="mb-reports__note" key={note}>
                {note}
              </p>
            ))}
          </>
        ) : busy ? (
          <Spinner label="Adding it up" />
        ) : (
          <EmptyState title="Pick a report" hint="Choose one from the list on the left." />
        )}
          </>
        )}
      </div>
    </div>
  );
}

/** Not a report id — the day, opened and closed. */
const DAYS = 'days';

/** Nor are the bills: one at a time, with the ways to take one back. */
const BILLS = 'bills';

/** Nor is the dashboard: it is the answer to a question, not a report. */
const TODAY = 'today';

/** Which report groups are open, on this computer. */
const FOLDS = 'reports.unfolded';

/** The groups this computer last had open — none, the first time, so the rail opens short. */
function opened(): readonly string[] {
  return remember(FOLDS, '')
    .split('\n')
    .filter((name) => name !== '');
}

/** The reports in their groups, in the order Rust listed them. */
function groups(entries: readonly ReportEntryView[]): [string, ReportEntryView[]][] {
  const out: [string, ReportEntryView[]][] = [];
  for (const entry of entries) {
    const last = out.at(-1);
    if (last && last[0] === entry.group) last[1].push(entry);
    else out.push([entry.group, [entry]]);
  }
  return out;
}
