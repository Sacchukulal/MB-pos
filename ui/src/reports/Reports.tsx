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
import { BILL_LOOKUP_PERMISSIONS, useMay } from '../shell/permissions';
import { useLicenceRevision } from '../shell/licence';
import { Bills } from './Bills';
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
  const licenceRevision = useLicenceRevision();
  const may = useMay();
  const mayBills = BILL_LOOKUP_PERMISSIONS.some(may);
  const mayDays = may('reports.view') || may('day.close');
  const [list, setList] = useState<ReportListView | null>(null);
  /** Taking a report out of the building is its own permission on top of reading it. */
  const mayExport = may('reports.export');
  /** The licence saying no, held rather than flashed. */
  const [locked, setLocked] = useState<string>('');
  /** The whole Reports area waits for Rust's current licence decision. */
  const [checkedRevision, setCheckedRevision] = useState<number | null>(null);
  const [allowed, setAllowed] = useState(false);
  const initialOrderId = initial?.startsWith('bills/') ? initial.slice('bills/'.length) : undefined;
  // The dashboard has its own home in the logo; Reports opens on the bill history.
  const [chosen, setChosen] = useState<string>(initial === DAYS || !mayBills ? DAYS : BILLS);
  useEffect(() => {
    if (initial === DAYS) setChosen(DAYS);
    else if (initial === BILLS || initialOrderId) setChosen(BILLS);
  }, [initial, initialOrderId]);
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
        setAllowed(false);
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
    let current = true;
    setList(null);
    setReport(null);
    setLocked('');
    call('report_list')
      .then((fresh) => {
        if (!current) return;
        setCheckedRevision(licenceRevision);
        setAllowed(true);
        setList(fresh);
        const today = fresh.periods[0];
        if (today) {
          setFrom(today.from);
          setTo(today.to);
        }
      })
      .catch((cause) => {
        if (!current) return;
        // Rust checks the licence before report permissions. A denied report list may still
        // expose licensed bill work or day closing according to this person's permissions.
        if (isUiError(cause) && cause.code === 'auth.denied') {
          setCheckedRevision(licenceRevision);
          setAllowed(true);
          if (!mayBills) setChosen(DAYS);
          return;
        }
        if (isLicenceRefusal(cause)) setCheckedRevision(licenceRevision);
        complain(cause);
      });
    return () => { current = false; };
  }, [complain, licenceRevision, mayBills]);

  // One effect, one call: whenever the report or the period changes, ask again.
  useEffect(() => {
    if (checkedRevision !== licenceRevision || !from || !to || chosen === DAYS || chosen === BILLS || !list || locked) return;
    let current = true;
    setBusy(true);
    setReport(null);
    call('report', { id: chosen, period: { from, to } })
      .then((fresh) => { if (current) setReport(fresh); })
      .catch((cause) => {
        if (!current) return;
        setReport(null);
        complain(cause);
      })
      .finally(() => { if (current) setBusy(false); });
    return () => { current = false; };
  }, [chosen, from, to, complain, list, locked, licenceRevision, checkedRevision]);

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

  const checking = checkedRevision !== licenceRevision;
  if (!allowed) return locked && !checking
    ? <Locked says={locked} onOpenAccount={onGoTo ? () => onGoTo('account') : undefined} />
    : <Spinner label="Opening the reports" />;

  const columns: readonly Column<Line>[] =
    report?.columns.map((spec, index) => ({
      key: `${index}`,
      header: spec.header,
      numeric: spec.numeric,
      render: (line: Line) => line.cells[index] ?? '',
    })) ?? [];
  const lines: readonly Line[] = report?.rows.map((cells, at) => ({ at, cells })) ?? [];

  return (
    <>
    {checking ? <Spinner label="Opening the reports" /> : null}
    <div className="mb-railpage mb-reports__gate" hidden={checking} inert={checking}>
      <Scroller inset className="mb-reports__rail">
        {/* At the top and on their own: the bills and the day itself. */}
        <div className="mb-reports__group">
          {[
            { id: BILLS, label: 'Bills', shown: mayBills },
            { id: DAYS, label: 'Day open/close', shown: mayDays },
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
          <Bills onGoTo={onGoTo} initialOrderId={initialOrderId} />
        ) : chosen === DAYS ? (
          <Days />
        ) : locked || !list ? (
          <Locked says={locked} onOpenAccount={onGoTo ? () => onGoTo('account') : undefined} />
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
    </>
  );
}

/** Not a report id — the day, opened and closed. */
const DAYS = 'days';

/** Nor are the bills: one at a time, with the ways to take one back. */
const BILLS = 'bills';

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
