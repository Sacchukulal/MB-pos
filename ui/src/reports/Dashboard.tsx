/** Today, and what needs you. */

import { useEffect, useState } from 'react';

import { Badge, Card, Icon, Scroller, SectionHeader, Spinner, StatCard } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { AttentionView } from '../ipc/generated/AttentionView';
import type { DashboardView } from '../ipc/generated/DashboardView';

export function Dashboard() {
  const [view, setView] = useState<DashboardView | null>(null);
  const [trouble, setTrouble] = useState('');

  useEffect(() => {
    call('dashboard')
      .then(setView)
      .catch((cause: unknown) => {
        if (isUiError(cause)) setTrouble(cause.message);
      });
  }, []);

  if (trouble) return <p className="mb-dash__trouble">{trouble}</p>;
  if (!view) return <Spinner label="Adding up today" />;

  return (
    <Scroller className="mb-dash">
      <SectionHeader title={view.title} />

      <div className="mb-dash__stats">
        {view.stats.map((stat) => (
          <StatCard
            key={stat.label}
            label={stat.label}
            value={stat.value}
            note={stat.note}
          />
        ))}
      </div>

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

      <SectionHeader title="What needs you" />
      {view.attention.length === 0 ? (
        // Empty is the good case and it says so.
        <Card className="mb-dash__quiet">{view.quiet}</Card>
      ) : (
        <div className="mb-dash__list">
          {view.attention.map((item: AttentionView) => (
            <Card key={item.title} className={`mb-dash__item mb-dash__item--${item.tone}`}>
              <Badge tone={item.tone === 'danger' ? 'danger' : item.tone === 'warn' ? 'warn' : 'info'}>
                <Icon
                  name={item.tone === 'info' ? 'info' : 'warning'}
                  size="sm"
                />
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
