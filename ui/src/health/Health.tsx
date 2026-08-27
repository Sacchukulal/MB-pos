/** "Is this counter healthy?". */

import { useCallback, useEffect, useState } from 'react';

import { Badge, Button, Card, SectionHeader, Table, useToast, type BadgeTone, type Column,
  Icon,
  Page,
  PageHeader,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { BundlePlanView } from '../ipc/generated/BundlePlanView';
import type { HealthRow } from '../ipc/generated/HealthRow';
import type { HealthView } from '../ipc/generated/HealthView';

import './health.css';

const TONES: Record<string, BadgeTone> = {
  ok: 'ok',
  warn: 'warn',
  danger: 'danger',
};

/** What the chip says. */
const CHIPS: Record<string, string> = {
  ok: 'Fine',
  warn: 'Look at this',
  danger: 'Needs you',
};

export function Health({ onGoTo }: { onGoTo?: (screen: string) => void }) {
  const [view, setView] = useState<HealthView | null>(null);
  const [plan, setPlan] = useState<BundlePlanView | null>(null);
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    call('health').then(setView).catch(report);
  }, [report]);

  useEffect(load, [load]);

  const columns: readonly Column<HealthRow>[] = [
    {
      key: 'name',
      header: 'What',
      render: (row) => <span className="mb-health__name">{row.name}</span>,
    },
    {
      key: 'tone',
      header: 'State',
      render: (row) => (
        <Badge tone={TONES[row.tone] ?? 'neutral'}>{CHIPS[row.tone] ?? row.tone}</Badge>
      ),
    },
    {
      key: 'says',
      header: 'Detail',
      render: (row) => (
        <div className="mb-health__says">
          <span>{row.says}</span>
          {row.goTo && onGoTo ? (
            <button
              type="button"
              className="mb-health__go"
              onClick={() => onGoTo(row.goTo as string)}
            >
              Open {row.goTo}
            </button>
          ) : null}
        </div>
      ),
    },
  ];

  if (!view) return null;

  return (
    <Page className="mb-health">
      <PageHeader
        title="Health"
        actions={
          <Button variant="secondary" onClick={load}>
            <Icon name="refresh" size="sm" />
            Check again
          </Button>
        }
      />

      <Card>
        <div className="mb-health__top mb-row">
          <p className="mb-health__headline">{view.headline}</p>
          <Badge tone={TONES[view.tone] ?? 'neutral'}>{CHIPS[view.tone] ?? view.tone}</Badge>
        </div>
      </Card>

      <Card>
        <Table columns={columns} rows={[...view.rows]} rowKey={(row) => row.id} />
      </Card>

      {/* The manifest before the zip. */}
      <Card>
        <SectionHeader
          title="Send us what is happening"
          note="If something above will not come right, this is what we need to look at it."
        />
        {plan ? (
          <>
            <ul className="mb-health__bundle">
              {plan.items.map((item) => (
                <li key={item.name}>
                  <span className="mb-health__file">{item.name}</span>
                  <span className="mb-health__what">{item.what}</span>
                  <span className="mb-health__size">{item.size}</span>
                </li>
              ))}
            </ul>
            <p className="mb-health__excludes">{plan.excludes}</p>
            <p className="mb-health__where">
              It will be saved in {plan.folder} — {plan.total} in all.
            </p>
            <div className="mb-health__actions mb-row mb-row--end">
              <Button variant="quiet" onClick={() => setPlan(null)}>
                Not now
              </Button>
              <Button
                variant="primary"
                disabled={busy}
                onClick={() => {
                  setBusy(true);
                  call('write_diagnostics')
                    .then((where) => {
                      setPlan(null);
                      toast.show('ok', 'Saved. Email it to us and we will look.', where);
                    })
                    .catch(report)
                    .finally(() => setBusy(false));
                }}
              >
                Save it
              </Button>
            </div>
          </>
        ) : (
          <div className="mb-health__actions mb-row mb-row--end">
            <Button
              variant="quiet"
              onClick={() => {
                call('reveal_logs')
                  .then((where) => toast.show('ok', 'The log folder is open.', where))
                  .catch(report);
              }}
            >
              Open the log folder
            </Button>
            <Button
              variant="quiet"
              onClick={() => {
                call('diagnostics_plan').then(setPlan).catch(report);
              }}
            >
              Copy diagnostics
            </Button>
          </div>
        )}
      </Card>
    </Page>
  );
}
