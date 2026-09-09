/** The dashboard: the period's figures in tiles and charts, and what needs you. */

import { useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  Chart,
  DateRangePicker,
  Icon,
  Scroller,
  SectionHeader,
  Spinner,
  StatCard,
  Stats,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import { keep, remember } from '../remember';
import type { AttentionView } from '../ipc/generated/AttentionView';
import type { DashboardView } from '../ipc/generated/DashboardView';
import type { PeriodChoiceView } from '../ipc/generated/PeriodChoiceView';

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

export function Dashboard({ presets }: { presets: readonly PeriodChoiceView[] }) {
  const [period, setPeriod] = useState(() => startingPeriod(presets));
  const [view, setView] = useState<DashboardView | null>(null);
  const [trouble, setTrouble] = useState('');

  const choose = (from: string, to: string) => {
    setPeriod({ from, to });
    const preset = presets.find((choice) => choice.from === from && choice.to === to);
    keep(REMEMBERED, JSON.stringify(preset ? { label: preset.label } : { from, to }));
  };

  useEffect(() => {
    if (!period.from || !period.to) return;
    call('dashboard', { period })
      .then((fresh) => {
        setView(fresh);
        setTrouble('');
      })
      .catch((cause: unknown) => {
        if (isUiError(cause)) setTrouble(cause.message);
      });
  }, [period]);

  const when = (
    <div className="mb-reports__when">
      <div className="mb-reports__presets">
        {presets.map((choice) => (
          <Button
            size="sm"
            key={choice.label}
            variant={choice.from === period.from && choice.to === period.to ? 'primary' : 'quiet'}
            onClick={() => choose(choice.from, choice.to)}
          >
            {choice.label}
          </Button>
        ))}
      </div>
      <DateRangePicker from={period.from} to={period.to} onChange={choose} />
    </div>
  );

  if (trouble) {
    return (
      <div className="mb-dash">
        {when}
        <p className="mb-dash__trouble">{trouble}</p>
      </div>
    );
  }
  if (!view) return <Spinner label="Adding it up" />;

  return (
    <Scroller className="mb-dash">
      {when}
      <SectionHeader title={view.title} />

      <Stats>
        {view.stats.map((stat) => (
          <StatCard key={stat.label} label={stat.label} value={stat.value} note={stat.note} />
        ))}
      </Stats>

      {view.compare ? (
        <p className="mb-dash__compare">
          <Badge
            tone={
              view.compare.direction === 'up'
                ? 'ok'
                : view.compare.direction === 'down'
                  ? 'warn'
                  : 'neutral'
            }
          >
            <Icon
              name={
                view.compare.direction === 'up'
                  ? 'chevron-up'
                  : view.compare.direction === 'down'
                    ? 'chevron-down'
                    : 'minus'
              }
              size="sm"
            />
          </Badge>
          {/* The whole sentence, written in Rust. */}
          {view.compare.summary}
        </p>
      ) : null}

      <div className="mb-dash__charts">
        {view.charts.map((chart) => (
          <Chart
            key={chart.id}
            chart={chart}
            // The run over time reads across the whole row.
            className={chart.kind === 'columns' ? 'mb-dash__chart--wide' : undefined}
          />
        ))}
      </div>

      <SectionHeader title="What needs you" />
      {view.attention.length === 0 ? (
        // Empty is the good case and it says so.
        <Card className="mb-dash__quiet">{view.quiet}</Card>
      ) : (
        <div className="mb-dash__list">
          {view.attention.map((item: AttentionView) => (
            <Card key={item.title} className={`mb-dash__item mb-dash__item--${item.tone}`}>
              <Badge tone={item.tone === 'danger' ? 'danger' : item.tone === 'warn' ? 'warn' : 'info'}>
                <Icon name={item.tone === 'info' ? 'info' : 'warning'} size="sm" />
              </Badge>
              <div>
                <strong>{item.title}</strong>
                <p className="mb-dash__detail">{item.detail}</p>
              </div>
            </Card>
          ))}
        </div>
      )}
    </Scroller>
  );
}
