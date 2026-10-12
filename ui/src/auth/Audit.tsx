/** The history. */

import { useCallback, useEffect, useState } from 'react';

import { Badge, Button, EmptyState, Select, Table, useToast, type Column,
  Page,
  PageHeader,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { AuditEntryView } from '../ipc/generated/AuditEntryView';
import type { AuditView } from '../ipc/generated/AuditView';
import type { PersonView } from '../ipc/generated/PersonView';

import './auth.css';

export function Audit() {
  const [view, setView] = useState<AuditView | null>(null);
  const [people, setPeople] = useState<readonly PersonView[]>([]);
  const [who, setWho] = useState('');
  const [what, setWhat] = useState('');
  const [days, setDays] = useState('7');
  const toast = useToast();

  const load = useCallback(async () => {
    try {
      setView(
        await call('audit_trail', {
          staffId: who === '' ? null : who,
          actionCode: what === '' ? null : what,
          days: days === '' ? null : Number(days),
        }),
      );
    } catch (cause) {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    }
  }, [who, what, days, toast]);

  useEffect(() => {
    void load();
  }, [load]);

  // The names for the filter.
  useEffect(() => {
    void (async () => {
      try {
        setPeople(await call('list_staff'));
      } catch {
        setPeople([]);
      }
    })();
  }, []);

  const columns: Column<AuditEntryView>[] = [
    { key: 'when', header: 'When', nowrap: true, render: (e) => e.when },
    { key: 'who', header: 'Staff member', render: (e) => e.who },
    { key: 'what', header: 'Action', render: (e) => e.what },
    { key: 'about', header: 'Record', render: (e) => e.about ?? '—' },
    {
      key: 'change',
      header: 'Change',
      render: (e) =>
        e.before || e.after ? (
          <span className="mb-audit__change">
            {e.before ? <code className="mb-audit__before">{e.before}</code> : null}
            {e.before && e.after ? <span aria-hidden="true"> → </span> : null}
            {e.after ? <code className="mb-audit__after">{e.after}</code> : null}
          </span>
        ) : (
          '—'
        ),
    },
  ];

  return (
    <Page className="mb-screen">
      <PageHeader
        title="History"
        subtitle="Recorded staff activity"
      />

      {view?.tampered ? (
        <div className="mb-audit__alarm" role="alert">
          <Badge tone="danger">Check this</Badge>
          <span>{view.tampered}</span>
        </div>
      ) : null}

      <div className="mb-row">
        <Select
          label="Staff member"
          value={who}
          onChange={(e) => setWho(e.target.value)}
          options={[
            { value: '', label: 'All staff' },
            ...people.map((p) => ({ value: p.id, label: p.name })),
          ]}
        />
        <Select
          label="Action"
          value={what}
          onChange={(e) => setWhat(e.target.value)}
          options={[
            { value: '', label: 'All actions' },
            ...(view?.actions ?? []).map(([code, words]) => ({
              value: code,
              label: words,
            })),
          ]}
        />
        <Select
          label="When"
          value={days}
          onChange={(e) => setDays(e.target.value)}
          options={[
            { value: '0', label: 'Today' },
            { value: '7', label: 'This week' },
            { value: '30', label: 'This month' },
            { value: '', label: 'All time' },
          ]}
        />
        <Button variant="quiet" onClick={() => void load()}>
          Refresh
        </Button>
      </div>

      {view && view.entries.length === 0 ? (
        <EmptyState
          title="No activity recorded"
          hint="Sign-ins, bill changes, price changes and day closing appear here."
        />
      ) : (
        <Table
          rows={view?.entries ?? []}
          columns={columns}
          rowKey={(e) => String(e.seq)}
        />
      )}
    </Page>
  );
}
