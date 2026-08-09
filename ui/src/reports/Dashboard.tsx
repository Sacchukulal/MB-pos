/**
 * **Today, and what needs you** — audit G1.
 *
 * > *"The owner's questions — how did today go, what is unusual, what needs me
 * > — are answered by opening four screens and doing arithmetic in your head."*
 *
 * Thirteen reports do not answer the third question, because you have to know
 * to ask them. This is the panel that speaks first.
 *
 * # Nothing here is worked out on this screen
 *
 * The figures are formatted, the comparison is a sentence, and **every line of
 * the attention list comes from the thing that already knows** — the backup
 * screen's own headline, the print queue's own snapshot, the Spends screen's
 * own reminders. A dashboard that recomputed any of them would be a fifth place
 * for a figure to disagree, which is G1 with more steps.
 */

import { useEffect, useState } from 'react';

import { Badge, Card, Spinner, StatCard } from '../kit';
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
    <div className="mb-dash">
      <h2 className="mb-dash__title">{view.title}</h2>

      <div className="mb-dash__stats">
        {view.stats.map((stat) => (
          <StatCard
            key={stat.label}
            label={stat.label}
            value={
              <>
                <span className="mb-numeric">{stat.value}</span>
                {stat.note ? <small className="mb-dash__note">{stat.note}</small> : null}
              </>
            }
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
            {view.compare.direction === 'up' ? '▲' : view.compare.direction === 'down' ? '▼' : '='}
          </Badge>
          {/* The whole sentence, written in Rust. */}
          {view.compare.summary}
        </p>
      ) : null}

      <h3 className="mb-dash__heading">What needs you</h3>
      {view.attention.length === 0 ? (
        // **Empty is the good case and it says so.** A blank panel reads as
        // broken; this reads as "nothing is wrong", which is the news.
        <Card className="mb-dash__quiet">{view.quiet}</Card>
      ) : (
        <div className="mb-dash__list">
          {view.attention.map((item: AttentionView) => (
            <Card key={item.title} className={`mb-dash__item mb-dash__item--${item.tone}`}>
              <Badge tone={item.tone === 'danger' ? 'danger' : item.tone === 'warn' ? 'warn' : 'info'}>
                {item.tone === 'danger' ? '!' : item.tone === 'warn' ? '▲' : 'i'}
              </Badge>
              <div>
                <strong>{item.title}</strong>
                <p className="mb-dash__detail">{item.detail}</p>
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
