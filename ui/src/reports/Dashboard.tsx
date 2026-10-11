/** A compact business overview. Figures and chart shares are supplied by Rust. */

import { useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Chart,
  DateRangePicker,
  Icon,
  Locked,
  Modal,
  Scroller,
  Spinner,
  StatCard,
} from '../kit';
import { call, isLicenceRefusal, isUiError } from '../ipc/call';
import { useMay } from '../shell/permissions';
import { useLicenceRevision } from '../shell/licence';
import { keep, remember } from '../remember';
import { Backup } from '../account/Backup';
import type { DashboardAmountView } from '../ipc/generated/DashboardAmountView';
import type { DashboardView } from '../ipc/generated/DashboardView';
import type { PeriodChoiceView } from '../ipc/generated/PeriodChoiceView';
import './dashboard.css';

/** The period the dashboard was last left on, kept on this computer. */
const REMEMBERED = 'reports.dashboard.period';

/** A preset is kept by its name, so "Today" is still today tomorrow; a typed range by its dates. */
type Kept = { label: string } | { from: string; to: string };

function keptPeriod(): Kept | null {
  const raw = remember(REMEMBERED, '');
  if (raw === '') return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === 'object') {
      if ('label' in parsed && typeof parsed.label === 'string') return { label: parsed.label };
      if (
        'from' in parsed &&
        'to' in parsed &&
        typeof parsed.from === 'string' &&
        typeof parsed.to === 'string'
      ) {
        return { from: parsed.from, to: parsed.to };
      }
    }
  } catch {
    // A preference that cannot be read is no preference.
  }
  return null;
}

/** The dates to open on: what was kept, resolved against today's presets, else today. */
function startingPeriod(presets: readonly PeriodChoiceView[]): { from: string; to: string } {
  const kept = keptPeriod();
  if (kept && 'label' in kept) {
    const preset = presets.find((choice) => choice.label === kept.label);
    if (preset) return { from: preset.from, to: preset.to };
  } else if (kept) {
    return kept;
  }
  const today = presets[0];
  return today ? { from: today.from, to: today.to } : { from: '', to: '' };
}

/** A standalone destination, with the same permission and licence checks as Reports. */
export function DashboardPage({ onGoTo }: { onGoTo?: (screen: string) => void }) {
  const licenceRevision = useLicenceRevision();
  const [presets, setPresets] = useState<readonly PeriodChoiceView[] | null>(null);
  const [trouble, setTrouble] = useState('');
  const [locked, setLocked] = useState('');
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let current = true;
    setPresets(null);
    setLocked('');
    setTrouble('');
    call('report_list').then((list) => {
      if (!current) return;
      if (!list.periods.length) setTrouble('No reporting periods are available. Please try again.');
      else setPresets(list.periods);
    }).catch((cause: unknown) => {
      if (!current) return;
      if (isLicenceRefusal(cause)) setLocked(cause.message);
      else setTrouble(isUiError(cause) ? cause.message : 'The dashboard could not be opened. Please try again.');
    });
    return () => { current = false; };
  }, [attempt, licenceRevision]);
  if (locked) return <Locked says={locked} onOpenAccount={onGoTo ? () => onGoTo('account') : undefined} />;
  if (trouble) return <div role="alert"><p>{trouble}</p><Button onClick={() => setAttempt((n) => n + 1)}>Try again</Button></div>;
  if (!presets) return <Spinner label="Loading dashboard" />;
  return <Dashboard presets={presets} onGoTo={onGoTo} />;
}

export function Dashboard({ presets, onGoTo }: {
  presets: readonly PeriodChoiceView[];
  onGoTo?: (screen: string) => void;
}) {
  const licenceRevision = useLicenceRevision();
  const [period, setPeriod] = useState(() => startingPeriod(presets));
  const [view, setView] = useState<DashboardView | null>(null);
  const [trouble, setTrouble] = useState('');
  const [refresh, setRefresh] = useState(0);
  const [busy, setBusy] = useState(true);
  const [trendId, setTrendId] = useState('trend');
  const [detail, setDetail] = useState<'cash' | 'sales' | 'backup' | null>(null);
  const may = useMay();
  const trends = view?.charts.filter((chart) => chart.kind === 'columns') ?? [];
  const trend = trends.find((chart) => chart.id === trendId) ?? trends[0];

  const choose = (from: string, to: string) => {
    setPeriod({ from, to });
    setDetail(null);
    const preset = presets.find((choice) => choice.from === from && choice.to === to);
    keep(REMEMBERED, JSON.stringify(preset ? { label: preset.label } : { from, to }));
  };

  useEffect(() => { setView(null); setDetail(null); }, [licenceRevision]);
  useEffect(() => {
    let current = true;
    setBusy(true);
    setTrouble('');
    setView((previous) => previous?.from === period.from && previous?.to === period.to ? previous : null);
    if (!period.from || !period.to) {
      setTrouble('Choose a start and end date.');
      setBusy(false);
      return;
    }
    if (period.from > period.to) {
      setTrouble('Choose an end date on or after the start date.');
      setBusy(false);
      return;
    }
    call('dashboard', { period })
      .then((fresh) => { if (current) { setView(fresh); setTrouble(''); } })
      .catch((cause: unknown) => {
        if (current) {
          setView(null);
          setDetail(null);
          setTrouble(isUiError(cause) ? cause.message : 'Figures could not be loaded. Please try again.');
        }
      }).finally(() => { if (current) setBusy(false); });
    return () => { current = false; };
  }, [period, refresh, licenceRevision]);

  const summary = view?.summary;
  const go = (target: string) => {
    setDetail(null);
    const report = target === 'reports/sales_day' || target === 'reports/profit';
    onGoTo?.(report ? `${target}?from=${period.from}&to=${period.to}` : target);
  };

  return (
    <Scroller className="mb-dash">
      <div className="mb-dash__toolbar">
        <div className="mb-dash__presets" role="group" aria-label="Dashboard period">
          {presets.map((choice) => <Button size="sm" key={choice.label}
            variant="quiet" aria-pressed={choice.from === period.from && choice.to === period.to}
            onClick={() => choose(choice.from, choice.to)}>{choice.label}</Button>)}
        </div>
        <div className="mb-dash__tools">
          <DateRangePicker size="sm" from={period.from} to={period.to} onChange={choose} />
          <Button size="sm" variant="quiet" disabled={busy} onClick={() => setRefresh((n) => n + 1)}>
            <Icon name="refresh" size="sm" /> Refresh
          </Button>
          {onGoTo && may('bill.create') ? <Button size="sm" variant="primary" onClick={() => go('billing')}>
            <Icon name="plus" size="sm" /> New bill
          </Button> : null}
        </div>
      </div>
      {trouble ? <div className="mb-dash__error" role="alert"><Icon name="warning" /><p>{trouble}</p>
        <Button size="sm" onClick={() => setRefresh((n) => n + 1)}>Try again</Button></div> : null}
      {busy && !view ? <div className="mb-dash__loading"><Spinner label="Updating figures" /></div> : null}
      {view && summary ? <div className="mb-dash__content" aria-busy={busy}>
        <section className="mb-dash__summary" aria-label="Sales and payment breakdown">
          <div className="mb-dash__money">
            <div className="mb-dash__total">
              <StatCard plain label="Sales" value={summary.sales} note={summary.bills} />
              {view.compare?.summary ? <span className={`mb-dash__compare mb-dash__compare--${view.compare.direction}`}>
                {view.compare.summary}
              </span> : null}
            </div>
            <div className="mb-dash__payments" role="group" aria-label="Sales by payment mode">
              {summary.payments.map((amount) => <StatCard plain key={amount.id}
                className={`mb-dash__payment${amount.id === 'credit' ? ' mb-dash__payment--credit' : ''}`}
                label={amount.label} value={amount.value} />)}
              {summary.paymentNote ? <span className="mb-dash__paymentnote">{summary.paymentNote}</span> : null}
              {!summary.payments.length && !summary.paymentNote ? <span className="mb-dash__paymentnote">No sales in this period</span> : null}
            </div>
            <div className="mb-dash__actions">
              <Button size="sm" variant="quiet" onClick={() => setDetail('sales')}><Icon name="info" size="sm" /> Details</Button>
              <Button size="sm" variant="quiet" onClick={() => setDetail('cash')}><Icon name="wallet" size="sm" /> Check cash</Button>
            </div>
          </div>
          <div className="mb-dash__secondary">
            <div className="mb-dash__facts">
              {summary.secondary.map((amount) => <span key={amount.id} title={amount.note}>
                {amount.label} <strong className="mb-numeric">{amount.value}</strong>
              </span>)}
            </div>
            <div className="mb-dash__status">
          {busy ? <span role="status">Updating figures…</span> : view.attention.length ?
            <Badge tone="warn">{view.attention.length} to review</Badge> :
            <span className="mb-dash__clear"><Icon name="check-circle" size="sm" /> No pending issues</span>}
          {onGoTo ? <Button size="sm" variant="quiet" onClick={() => go('reports')}>
            <Icon name="chart" size="sm" /> Reports
          </Button> : null}
            </div>
          </div>
        </section>
        {view.attention.length ? <section className="mb-dash__attention" aria-label="Needs attention">
          {view.attention.map((item, index) => <div key={`${item.target}-${index}`} className={`mb-dash__item mb-dash__item--${item.tone}`}>
            <Icon name={item.tone === 'info' ? 'info' : 'warning'} size="sm" />
            <div><strong>{item.title}</strong><p>{item.detail}</p></div>
            {item.target === 'backup' ? <Button size="sm" variant="quiet" onClick={() => setDetail('backup')}>{item.action}</Button> :
              onGoTo && item.target ? <Button size="sm" variant="quiet" onClick={() => go(item.target)}>{item.action}</Button> : null}
          </div>)}
        </section> : null}
        <div className="mb-dash__analytics">
          <div className="mb-dash__performance">
            {trend ? <Chart key={trend.id} chart={trend} overview action={trends.length > 1 ?
              <div className="mb-dash__trendchoices" role="group" aria-label="Sales breakdown">
                {trends.map((chart) => <Button key={chart.id} size="sm" variant="quiet" aria-pressed={chart.id === trend.id} onClick={() => setTrendId(chart.id)}>{chart.title}</Button>)}
              </div> : undefined} /> : null}
          </div>
          <div className="mb-dash__breakdowns">
            {view.charts.filter((chart) => chart.kind !== 'columns').map((chart) => <Chart key={chart.id} chart={chart} overview />)}
          </div>
        </div>
        <Modal open={detail === 'cash'} title="Check cash" onClose={() => setDetail(null)}
          note="Expected physical cash across all tills for the selected day, before counting. This is not a bank balance."
          actions={<><Button variant="quiet" onClick={() => setDetail(null)}>Close</Button>
            {summary.drawer && onGoTo && may('day.close') ? <Button onClick={() => go('reports/days')}>Count cash</Button> : null}</>}>
          {summary.drawer ? <>
            <AmountRows rows={summary.drawer.rows} />
            <div className="mb-dash__drawer-total"><span>Expected drawer cash</span><strong className="mb-numeric">{summary.drawer.expected}</strong></div>
            <p className="mb-dash__detailnote">All tills · Before counting</p>
          </> : <p>{summary.drawerUnavailable}</p>}
        </Modal>
        <Modal open={detail === 'backup'} title="Backup" onClose={() => setDetail(null)}
          actions={<Button variant="quiet" onClick={() => setDetail(null)}>Close</Button>}>
          {detail === 'backup' ? <Backup /> : null}
        </Modal>
        <Modal open={detail === 'sales'} title="Sales details" onClose={() => setDetail(null)}
          note="Sales include bill taxes and discounts, with voids and returns deducted. Payment modes include tips; any tips are deducted separately to reconcile with sales. Credit sales are not cash received."
          actions={<><Button variant="quiet" onClick={() => setDetail(null)}>Close</Button>
            {onGoTo ? <Button onClick={() => go('reports/sales_day')}>Sales report</Button> : null}</>}>
          <AmountRows rows={summary.payments} />
          {summary.paymentNote ? <p>{summary.paymentNote}</p> : null}
          <AmountRows rows={summary.secondary} />
          <AmountRows rows={summary.details} />
          {onGoTo ? <Button variant="quiet" onClick={() => go('reports/profit')}>Profit report <Icon name="arrow-right" size="sm" /></Button> : null}
        </Modal>
      </div> : null}
    </Scroller>
  );
}

function AmountRows({ rows }: { rows: readonly DashboardAmountView[] }) {
  return <dl className="mb-dash__amounts">{rows.map((row) => <div key={row.id}>
    <dt>{row.label}{row.note ? <small>{row.note}</small> : null}</dt>
    <dd className="mb-numeric">{row.value}</dd>
  </div>)}</dl>;
}
