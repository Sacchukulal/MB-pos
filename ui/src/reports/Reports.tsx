/**
 * **Every report, on one screen** — scope 10, audit G1–G7.
 *
 * # There is one component here and there will only ever be one
 *
 * P17 proved the shape: the settings screen is one component over one
 * catalogue, and adding a setting touches no `.tsx` file. A report is the same
 * kind of thing — a title, some columns and some rows — so it gets the same
 * treatment. Thirteen reports, one table, one export path.
 *
 * Adding a report is a function in `mb-db/src/repo/reports.rs` and a line in
 * `src-tauri/src/reports.rs`'s `CATALOGUE`. **Nothing in this file changes**,
 * and a Rust test walks the list in both directions to keep that true.
 *
 * # Nothing on this page is a number
 *
 * Every cell arrives as a string that `Money::to_plain_string` produced, the
 * comparison arrives as a finished sentence, and the notes arrive written.
 * R8 and §6: this file has no arithmetic and no sentence assembly in it, which
 * is exactly what audit E3 found wrong with v1's reports.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  DateRangePicker,
  EmptyState,
  SectionHeader,
  Spinner,
  Table,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import { DayClose } from './DayClose';
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

export function Reports() {
  const [list, setList] = useState<ReportListView | null>(null);
  const [chosen, setChosen] = useState<string>('sales_day');
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [report, setReport] = useState<ReportView | null>(null);
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  // The list, and with it the period presets — which come from Rust because
  // "today" is the shop's business day and only Rust knows when that starts.
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
      .catch(complain);
  }, [complain]);

  // One effect, one call: whenever the report or the period changes, ask
  // again. No caching — a report is a question about right now, and a stale
  // answer with today's date on it is worse than a spinner.
  useEffect(() => {
    // The day close is not a report and there is nothing to ask for.
    if (!from || !to || chosen === DAY_CLOSE) return;
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

  if (!list) return <Spinner label="Opening the reports" />;

  const columns: readonly Column<Line>[] =
    report?.columns.map((spec, index) => ({
      key: `${index}`,
      header: spec.header,
      numeric: spec.numeric,
      render: (line: Line) => line.cells[index] ?? '',
    })) ?? [];
  const lines: readonly Line[] = report?.rows.map((cells, at) => ({ at, cells })) ?? [];

  return (
    <div className="mb-reports">
      <nav className="mb-reports__rail" aria-label="Reports">
        {/* At the top and on its own, because it is the one thing on this
            screen a shop does every single night. */}
        <div className="mb-reports__group">
          <button
            type="button"
            className={
              chosen === DAY_CLOSE
                ? 'mb-reports__pick mb-reports__pick--on'
                : 'mb-reports__pick'
            }
            aria-current={chosen === DAY_CLOSE}
            onClick={() => setChosen(DAY_CLOSE)}
          >
            Close the day
          </button>
        </div>
        {groups(list.reports).map(([group, entries]) => (
          <div className="mb-reports__group" key={group}>
            <h2 className="mb-reports__grouptitle">{group}</h2>
            {entries.map((entry) => (
              <button
                type="button"
                key={entry.id}
                className={
                  entry.id === chosen
                    ? 'mb-reports__pick mb-reports__pick--on'
                    : 'mb-reports__pick'
                }
                aria-current={entry.id === chosen}
                onClick={() => setChosen(entry.id)}
              >
                {entry.title}
              </button>
            ))}
          </div>
        ))}
      </nav>

      <div className="mb-reports__body">
        {chosen === DAY_CLOSE ? (
          <DayClose />
        ) : (
          <>
        <div className="mb-reports__when">
          <div className="mb-reports__presets">
            {list.periods.map((choice: PeriodChoiceView) => (
              <Button
                small
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
                <div className="mb-reports__exports">
                  <Button small variant="quiet" onClick={() => save('report_csv')}>
                    Save as CSV
                  </Button>
                  <Button small variant="quiet" onClick={() => save('report_pdf')}>
                    Save as PDF
                  </Button>
                </div>
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
                  {report.compare.direction === 'up'
                    ? '▲'
                    : report.compare.direction === 'down'
                      ? '▼'
                      : '='}
                </Badge>
                {/* The whole sentence, written in Rust. */}
                <span>{report.compare.summary}</span>
                <span className="mb-reports__against">
                  Compared against {report.compare.period}
                </span>
              </div>
            ) : null}

            <div className="mb-reports__sheet">
              <Table
                columns={columns}
                rows={lines}
                rowKey={(line) => `${line.at}`}
                empty={
                  <EmptyState
                    title="Nothing in this period"
                    body="Pick a different date range, or a different report."
                  />
                }
              />
              {report.totals ? (
                <table className="mb-reports__totals">
                  <tbody>
                    <tr>
                      {report.totals.map((cell, index) => (
                        <td
                          // The cells are positional and can repeat ("" twice),
                          // so the column index IS the identity here.
                          key={`${index}-${cell}`}
                          className={
                            report.columns[index]?.numeric
                              ? 'mb-reports__total mb-reports__total--numeric'
                              : 'mb-reports__total'
                          }
                        >
                          {cell}
                        </td>
                      ))}
                    </tr>
                  </tbody>
                </table>
              ) : null}
            </div>

            {report.notes.map((note) => (
              <p className="mb-reports__note" key={note}>
                {note}
              </p>
            ))}
          </>
        ) : busy ? (
          <Spinner label="Adding it up" />
        ) : (
          <EmptyState title="Pick a report" body="Choose one from the list on the left." />
        )}
          </>
        )}
      </div>
    </div>
  );
}

/** Not a report id — the one entry on this screen that is a thing to DO. */
const DAY_CLOSE = 'day_close';

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
