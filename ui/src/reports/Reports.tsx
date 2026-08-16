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
  Icon,
  Locked,
  SectionHeader,
  Spinner,
  Table,
  useToast,
  type Column,
} from '../kit';
import { call, isLicenceRefusal, isUiError } from '../ipc/call';
import { Dashboard } from './Dashboard';
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

export function Reports({ onGoTo }: { onGoTo?: (screen: string) => void }) {
  const [list, setList] = useState<ReportListView | null>(null);
  /**
   * **The licence saying no, held rather than flashed** (P30.5).
   *
   * Reports are behind the licence (D86). On a shop without one this screen
   * used to open on a spinner that never stopped and a red toast that slid
   * away after four seconds — permanently blank, with the only explanation
   * already gone. Now the refusal IS the screen, and it says how to fix it.
   */
  const [locked, setLocked] = useState<string>('');
  // **The dashboard is what this screen opens on.** Audit G1: the owner's
  // first question is "what needs me", and a screen that opens on a list of
  // reports makes them ask it themselves.
  const [chosen, setChosen] = useState<string>(TODAY);
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [report, setReport] = useState<ReportView | null>(null);
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      // A refusal is an answer, not a fault: it belongs on the screen, and a
      // toast on top of it would say the same thing twice and then vanish.
      if (isLicenceRefusal(cause)) {
        setLocked(cause.message);
        return;
      }
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
    // Neither the dashboard nor the day close is a report.
    if (!from || !to || chosen === DAY_CLOSE || chosen === TODAY) return;
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

  /**
   * **Send the figures to somebody** — scope 10.13, `share_report`, which was
   * written at P26 and never called.
   *
   * Rust composes the summary and, crucially, **says what each way of sending
   * it cannot do** (D134): WhatsApp will not carry a table, e-mail opens a
   * draft it cannot attach to. The screen shows that sentence rather than
   * inventing a cheerful one, because a summary that arrives mangled is worse
   * than one that was never sent.
   *
   * "Copy" is first and is the default, because it is the only one that cannot
   * fail: a shopkeeper pastes it wherever they were going to send it anyway.
   */
  const share = (channel: 'copy' | 'whats_app' | 'email' | 'folder') => {
    call('share_report', { id: chosen, period: { from, to }, channel })
      .then((shared) => {
        if (channel === 'copy') void navigator.clipboard.writeText(shared.text);
        toast.show('ok', shared.says, shared.caveat === '' ? undefined : shared.caveat);
      })
      .catch(complain);
  };

  if (locked) {
    return <Locked says={locked} onOpenAccount={onGoTo ? () => onGoTo('account') : undefined} />;
  }
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
        {/* At the top and on their own: one is the question an owner opens
            this screen to ask, the other is the thing a shop does every single
            night. Neither is a report. */}
        <div className="mb-reports__group">
          {[
            // Not just "Today": there is a period preset by that name three
            // inches to the right, and two buttons saying the same word that
            // do different things is how a screen teaches somebody to distrust
            // it. Found by a test that could not tell them apart either.
            { id: TODAY, label: 'Today at a glance' },
            { id: DAY_CLOSE, label: 'Close the day' },
          ].map((entry) => (
            <button
              type="button"
              key={entry.id}
              className={
                chosen === entry.id
                  ? 'mb-reports__pick mb-reports__pick--on'
                  : 'mb-reports__pick'
              }
              aria-current={chosen === entry.id}
              onClick={() => setChosen(entry.id)}
            >
              {entry.label}
            </button>
          ))}
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
        {chosen === TODAY ? (
          <Dashboard />
        ) : chosen === DAY_CLOSE ? (
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
                  {/* Sending it first, saving it second: an owner looking at
                      this at 11 p.m. wants it on their phone far more often
                      than they want a file on the till. */}
                  <Button small variant="quiet" onClick={() => share('copy')}>
                    Copy
                  </Button>
                  <Button small variant="quiet" onClick={() => share('whats_app')}>
                    WhatsApp
                  </Button>
                  <Button small variant="quiet" onClick={() => share('email')}>
                    Email
                  </Button>
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

            <div className="mb-reports__sheet">
              <Table
                columns={columns}
                rows={lines}
                rowKey={(line) => `${line.at}`}
                // The totals belong to the table: a second table underneath
                // could not agree with it about column widths, and a column of
                // rupees that does not line up looks broken (§3).
                footer={report.totals ?? undefined}
                empty={
                  <EmptyState
                    title="Nothing in this period"
                    body="Pick a different date range, or a different report."
                  />
                }
              />
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

/** Nor is the dashboard: it is the answer to a question, not a report. */
const TODAY = 'today';

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
