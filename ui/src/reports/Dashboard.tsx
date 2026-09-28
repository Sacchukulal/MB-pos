/** The dashboard: the period's figures in tiles and charts, and what needs you. */

import { useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  Chart,
  DateRangePicker,
  Icon,
  Locked,
  PageHeader,
  Scroller,
  SectionHeader,
  Spinner,
  type IconName,
} from '../kit';
import { call, isLicenceRefusal, isUiError } from '../ipc/call';
import { useMay } from '../shell/permissions';
import { keep, remember } from '../remember';
import type { AttentionView } from '../ipc/generated/AttentionView';
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
  const [presets, setPresets] = useState<readonly PeriodChoiceView[] | null>(null);
  const [trouble, setTrouble] = useState('');
  const [locked, setLocked] = useState('');
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let current = true;
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
  }, [attempt]);
  if (locked) return <Locked says={locked} onOpenAccount={onGoTo ? () => onGoTo('account') : undefined} />;
  if (trouble) return <div role="alert"><p>{trouble}</p><Button onClick={() => setAttempt((n) => n + 1)}>Try again</Button></div>;
  if (!presets) return <Spinner label="Opening your dashboard" />;
  return <Dashboard presets={presets} onGoTo={onGoTo} />;
}

const METRIC_ICONS: Record<string, IconName> = {
  Takings: 'banknote', 'Average bill': 'receipt', 'In the drawer': 'wallet',
  Spent: 'card', Voided: 'x', 'Gross margin': 'chart',
};

export function Dashboard({ presets, onGoTo }: {
  presets: readonly PeriodChoiceView[];
  onGoTo?: (screen: string) => void;
}) {
  const [period, setPeriod] = useState(() => startingPeriod(presets));
  const [view, setView] = useState<DashboardView | null>(null);
  const [trouble, setTrouble] = useState('');
  const [refresh, setRefresh] = useState(0);
  const [busy, setBusy] = useState(true);
  const may = useMay();

  const choose = (from: string, to: string) => {
    setPeriod({ from, to });
    const preset = presets.find((choice) => choice.from === from && choice.to === to);
    keep(REMEMBERED, JSON.stringify(preset ? { label: preset.label } : { from, to }));
  };

  useEffect(() => {
    let current = true;
    setBusy(true);
    setTrouble('');
    setView(null);
    if (!period.from || !period.to) {
      setTrouble('Choose a start and end date to see your overview.');
      setBusy(false);
      return;
    }
    if (period.from > period.to) {
      setTrouble('Choose an end date on or after the start date.');
      setBusy(false);
      return;
    }
    call('dashboard', { period })
      .then((fresh) => {
        if (!current) return;
        setView(fresh);
        setTrouble('');
      })
      .catch((cause: unknown) => {
        if (current) setTrouble(isUiError(cause) ? cause.message : 'Your figures could not be loaded. Please try again.');
      }).finally(() => { if (current) setBusy(false); });
    return () => { current = false; };
  }, [period, refresh]);

  const when = (
    <div className="mb-dash__filters">
      <div className="mb-dash__presets" role="group" aria-label="Dashboard period">
        {presets.map((choice) => (
          <Button
            size="sm"
            key={choice.label}
            variant={choice.from === period.from && choice.to === period.to ? 'primary' : 'quiet'}
            aria-pressed={choice.from === period.from && choice.to === period.to}
            onClick={() => choose(choice.from, choice.to)}
          >
            {choice.label}
          </Button>
        ))}
      </div>
      <DateRangePicker from={period.from} to={period.to} onChange={choose} />
    </div>
  );

  return (
    <Scroller className="mb-dash">
      <div className="mb-dash__intro">
        <span className="mb-dash__eyebrow"><span className="mb-dash__dot" /> Your business at a glance</span>
        <PageHeader title="Business overview"
          actions={<>
            <Button variant="quiet" disabled={busy} onClick={() => setRefresh((n) => n + 1)}>
              <Icon name="refresh" size="sm" /> Refresh
            </Button>
            {onGoTo && may('bill.create') ? <Button className="mb-dash__primary" onClick={() => onGoTo('billing')}>
              <Icon name="plus" size="sm" /> New bill
            </Button> : null}
          </>}
        />
      </div>
      {when}
      {trouble ? <div className="mb-dash__error" role="alert"><Icon name="warning" /><p>{trouble}</p>
        <Button size="sm" onClick={() => setRefresh((n) => n + 1)}>Try again</Button></div> : null}
      {busy ? <div className="mb-dash__loading"><Spinner label="Updating your overview" /></div> : null}
      {view && !busy ? <div className="mb-dash__content">
        <div className="mb-dash__periodline">
          <h2>{view.title}</h2>
          {view.compare ? <span className="mb-dash__compare">
            <Icon name={view.compare.direction === 'up' ? 'chevron-up' : view.compare.direction === 'down' ? 'chevron-down' : 'minus'} size="sm" />
            {view.compare.summary}
          </span> : null}
        </div>
        <div className="mb-dash__metrics">
          {view.stats.map((stat, index) => <Card key={stat.label} className={`mb-dash__metric mb-dash__metric--${index}`}>
            <div className="mb-dash__metrichead"><span>{stat.label}</span><span className="mb-dash__metricicon"><Icon name={METRIC_ICONS[stat.label] ?? 'chart'} size="md" /></span></div>
            <strong className="mb-dash__value mb-numeric">{stat.value}</strong>
            <span className="mb-dash__metricnote">{stat.note}</span>
          </Card>)}
        </div>
        <div className="mb-dash__visuals">
          <div className="mb-dash__performance">
            {view.charts.filter((chart) => chart.kind === 'columns').map((chart) => <Chart key={chart.id} chart={chart} overview />)}
          </div>
          <Card className="mb-dash__attention">
            <div className="mb-dash__panelcaption"><Icon name="pulse" size="sm" /> On your radar</div>
            <SectionHeader title="What needs you" action={<Badge tone={view.attention.length ? 'warn' : 'ok'}>{view.attention.length ? `${view.attention.length} to review` : 'All clear'}</Badge>} />
            {view.attention.length === 0 ? <div className="mb-dash__quiet"><span className="mb-dash__allclear"><Icon name="check-circle" size="lg" /></span><strong>Room to focus on the good stuff.</strong><p>{view.quiet}</p></div> :
              <div className="mb-dash__list">{view.attention.map((item: AttentionView) => <div key={item.title} className={`mb-dash__item mb-dash__item--${item.tone}`}>
                <Icon name={item.tone === 'info' ? 'info' : 'warning'} size="sm" /><div><strong>{item.title}</strong><p>{item.detail}</p></div>
              </div>)}</div>}
            {onGoTo ? <Button variant="quiet" className="mb-dash__reportlink" onClick={() => onGoTo('reports')}>Explore reports <Icon name="arrow-right" size="sm" /></Button> : null}
          </Card>
        </div>
        <div className="mb-dash__breakdownhead"><SectionHeader title="Behind the numbers" /><span>Sales mix & best performers</span></div>
        <div className="mb-dash__breakdowns">
          {view.charts.filter((chart) => chart.kind !== 'columns').map((chart) => <Chart key={chart.id} chart={chart} overview />)}
        </div>
        <footer className="mb-dash__footer"><span><Icon name="check-circle" size="sm" /> Figures from your recorded transactions</span><span>Magic Bill · Made for your everyday</span></footer>
      </div> : null}
    </Scroller>
  );
}
