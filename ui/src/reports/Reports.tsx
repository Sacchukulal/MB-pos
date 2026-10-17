/** Every report, on one screen. */

import { useCallback, useEffect, useMemo, useState } from 'react';

import {
  Button,
  EmptyState,
  Icon,
  Locked,
  Pick,
  Panel,
  Scroller,
  RowMenu,
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
import { ReportExport, ReportHeader, ReportPeriod } from './ReportHeader';
import type { ReportEntryView } from '../ipc/generated/ReportEntryView';
import type { ReportListView } from '../ipc/generated/ReportListView';
import type { ReportView } from '../ipc/generated/ReportView';
import type { ReportSelection } from '../ipc/generated/ReportSelection';
import { SelectionPicker } from './SelectionPicker';

const ALL_SELECTION: ReportSelection = { keys: [], categoryIds: [] };

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
  const [initialId, initialQuery] = (initial ?? '').split('?');
  const initialDates = new URLSearchParams(initialQuery);
  const requestedFrom = initialDates.get('from') ?? '';
  const requestedTo = initialDates.get('to') ?? '';
  const validRequestedPeriod = /^\d{4}-\d{2}-\d{2}$/.test(requestedFrom) && /^\d{4}-\d{2}-\d{2}$/.test(requestedTo) && requestedFrom <= requestedTo;
  const initialOrderId = initialId?.startsWith('bills/') ? initialId.slice('bills/'.length) : undefined;
  // The dashboard has its own home in the logo; Reports opens on the bill history.
  const [chosen, setChosen] = useState<string>(initialId === DAYS || !mayBills ? DAYS : BILLS);
  useEffect(() => {
    if (initialId === DAYS) setChosen(DAYS);
    else if (initialId === BILLS || initialOrderId) setChosen(BILLS);
    else if (initialId && list?.reports.some((entry) => entry.id === initialId)) setChosen(initialId);
  }, [initialId, initialOrderId, list]);
  // Which groups are unfolded. The rail is nine groups long; folded is how a person finds
  // anything in it, and which ones are open is a look preference, not a fact about the shop.
  const [unfolded, setUnfolded] = useState<readonly string[]>(() => opened());
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [report, setReport] = useState<ReportView | null>(null);
  const [loadedFor, setLoadedFor] = useState('');
  const [busy, setBusy] = useState(false);
  const [selections, setSelections] = useState<Record<string, ReportSelection>>({});
  const selection = selections[chosen] ?? ALL_SELECTION;
  const filtered = selection.keys.length > 0 || selection.categoryIds.length > 0;
  const request = useMemo(() => ({ id: chosen, period: { from, to }, ...(filtered ? { selection } : {}) }),
    [chosen, from, to, selection, filtered]);
  const requestKey = JSON.stringify(request);
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
          setFrom(validRequestedPeriod ? requestedFrom : today.from);
          setTo(validRequestedPeriod ? requestedTo : today.to);
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
  }, [complain, licenceRevision, mayBills, requestedFrom, requestedTo, validRequestedPeriod]);

  // One effect, one call: whenever the report or the period changes, ask again.
  useEffect(() => {
    if (checkedRevision !== licenceRevision || !from || !to || chosen === DAYS || chosen === BILLS || !list || locked) return;
    let current = true;
    setBusy(true);
    call('report', request)
      .then((fresh) => {
        if (current) {
          setReport(fresh);
          setLoadedFor(requestKey);
        }
      })
      .catch((cause) => {
        if (!current) return;
        setReport(null);
        complain(cause);
      })
      .finally(() => { if (current) setBusy(false); });
    return () => { current = false; };
  }, [chosen, from, to, complain, list, locked, licenceRevision, checkedRevision, request, requestKey]);

  const save = (command: 'report_csv' | 'report_pdf') => {
    if (!currentReport) return;
    call(command, request)
      .then((saved) => toast.show('ok', saved.message, saved.path))
      .catch(complain);
  };

  /** The same figures, on the roll the bills come off. */
  const print = () => {
    if (!currentReport) return;
    call('report_print', request)
      .then((says) => toast.show('ok', says))
      .catch(complain);
  };

  /** Send the figures to somebody. */
  const share = (channel: 'copy' | 'whats_app' | 'email' | 'folder') => {
    if (!currentReport) return;
    call('share_report', { ...request, channel })
      .then((shared) => {
        if (channel === 'copy') void navigator.clipboard.writeText(shared.text);
        toast.show('ok', shared.says, shared.caveat === '' ? undefined : shared.caveat);
      })
      .catch(complain);
  };

  const checking = checkedRevision !== licenceRevision;
  if (!allowed) return locked && !checking
    ? <Locked says={locked} onOpenAccount={onGoTo ? () => onGoTo('account') : undefined} />
    : <Spinner label="Loading reports" />;

  const currentReport = loadedFor === requestKey ? report : null;
  const filters = report?.id === chosen ? report.filters : null;
  const selected = list?.reports.find((entry) => entry.id === chosen);
  const columns: readonly Column<Line>[] =
    currentReport?.columns.map((spec, index) => ({
      key: `${index}`,
      header: spec.header,
      numeric: spec.numeric,
      render: (line: Line) => line.cells[index] ?? '',
    })) ?? [];
  const lines: readonly Line[] = currentReport?.rows.map((cells, at) => ({ at, cells })) ?? [];

  return (
    <>
    {checking ? <Spinner label="Loading reports" /> : null}
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
          <Bills onGoTo={onGoTo} initialOrderId={initialOrderId} periods={list?.periods} />
        ) : chosen === DAYS ? (
          <Days />
        ) : locked || !list ? (
          <Locked says={locked} onOpenAccount={onGoTo ? () => onGoTo('account') : undefined} />
        ) : (
          <>
        <ReportHeader title={selected?.title ?? 'Reports'}
          info={selected?.description}
          summary={currentReport?.compare ? <div className="mb-reports__compare">
            <span>{currentReport.compare.summary}</span>
            <span className="mb-reports__against">· Compared with {currentReport.compare.period}</span>
          </div> : undefined}
          actions={mayExport ? <>
            <Button size="sm" variant="secondary" disabled={!currentReport} onClick={print}>Print</Button>
            <RowMenu text="Share" label="Share" icon="chevron-down" keyboardNavigation disabled={!currentReport}>
              <Button role="menuitem" size="sm" variant="quiet" onClick={() => share('copy')}>Copy</Button>
              <Button role="menuitem" size="sm" variant="quiet" onClick={() => share('whats_app')}>WhatsApp</Button>
              <Button role="menuitem" size="sm" variant="quiet" onClick={() => share('email')}>Email</Button>
            </RowMenu>
            <ReportExport disabled={!currentReport} onExport={(format) => save(format === 'csv' ? 'report_csv' : 'report_pdf')} />
          </> : undefined}>
          <ReportPeriod periods={list.periods} from={from} to={to} onChange={(nextFrom, nextTo) => {
            setFrom(nextFrom);
            setTo(nextTo);
          }} />
          {filters ? <div className="mb-row">
            {filters.categories ? <SelectionPicker label="Categories" choices={filters.categories} selected={selection.categoryIds}
              onChange={(categoryIds) => setSelections((prior) => ({ ...prior, [chosen]: { ...selection, categoryIds } }))} /> : null}
            <SelectionPicker label={filters.label === 'Category' ? 'Categories' : filters.label === 'Paid by' ? 'Payment modes' : `${filters.label}s`}
              choices={filters.choices} selected={selection.keys}
              onChange={(keys) => setSelections((prior) => ({ ...prior, [chosen]: { ...selection, keys } }))} />
            {filtered ? <Button size="sm" variant="quiet" onClick={() => setSelections((prior) => ({ ...prior, [chosen]: ALL_SELECTION }))}>Clear filters</Button> : null}
          </div> : null}
          {filtered && currentReport ? <p className="mb-muted">{currentReport.subtitle}</p> : null}
        </ReportHeader>

        {currentReport ? (
            <Scroller wide className="mb-reports__sheet">
              <Panel flush>
              <Table
                columns={columns}
                rows={lines}
                rowKey={(line) => `${line.at}`}
                // The totals belong to the table: a second table underneath could not agree
                // with it about column widths, and a column of rupees that does not line up
                // looks broken (§3).
                footer={currentReport.totals ?? undefined}
                empty={
                  <EmptyState
                    title="No records in this period"
                    hint="Choose another date range or report."
                  />
                }
              />
              </Panel>
            {currentReport.notes.map((note) => (
              <p className="mb-reports__note" key={note}>
                {note}
              </p>
            ))}
            </Scroller>
        ) : busy ? (
          <Spinner label="Loading report" />
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
