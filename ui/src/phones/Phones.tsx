/** The phones on this counter: add one, say whose it is, take one off. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  ConfirmDialog,
  EmptyState,
  Fact,
  Facts,
  Icon,
  Notice,
  Page,
  PageHeader,
  Panel,
  Select,
  Table,
  useToast,
  type Column,
} from '../kit';
import { call, inApp, isUiError, subscribe } from '../ipc/call';
import type { DeviceRowView } from '../ipc/generated/DeviceRowView';
import type { NetworkView } from '../ipc/generated/NetworkView';

import './phones.css';

/** Rust's tone words are the notice's own. */
const TONES: Record<string, 'ok' | 'warn' | 'danger'> = {
  ok: 'ok',
  warn: 'warn',
  danger: 'danger',
};

/** The choice that gives a phone to nobody: a tablet at the pass. */
const SHARED = 'shared';

export function Phones() {
  const [view, setView] = useState<NetworkView | null>(null);
  const [removing, setRemoving] = useState<DeviceRowView | null>(null);
  const [busy, setBusy] = useState(false);
  /** Whose phone each waiting one is, chosen before Allow. */
  const [owners, setOwners] = useState<Record<string, string>>({});
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  useEffect(() => {
    call('network').then(setView).catch(report);
  }, [report]);

  // Rust pushes; React subscribes. A phone coming on or off, asking to join, or using the code
  // (which moves the code on) all arrive as one kind.
  useEffect(() => {
    if (!inApp()) return undefined;
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (message.kind === 'phones') {
        call('network')
          .then(setView)
          .catch(() => undefined);
      }
    })
      .then((off) => {
        stop = off;
      })
      .catch(() => undefined);
    return () => stop?.();
  }, []);

  /** Run a phones command and take the whole view back. */
  const run = (work: () => Promise<NetworkView>, done?: string) => {
    setBusy(true);
    work()
      .then((fresh) => {
        setView(fresh);
        if (done) toast.show('ok', done);
      })
      .catch(report)
      .finally(() => setBusy(false));
  };

  const copy = (text: string) => {
    navigator.clipboard
      .writeText(text)
      .then(() => toast.show('ok', 'Copied.'))
      .catch(() => toast.show('danger', 'It could not be copied.'));
  };

  if (!view) return null;

  const tone = TONES[view.tone] ?? 'info';
  const phones = view.devices;
  const full = phones.length >= view.phonesAllowed;
  const subtitle = [
    `${phones.length} of ${view.phonesAllowed} phone${view.phonesAllowed === 1 ? '' : 's'}`,
    view.connected > 0 ? `${view.connected} live` : '',
  ]
    .filter(Boolean)
    .join(' · ');

  const columns: readonly Column<DeviceRowView>[] = [
    { key: 'name', header: 'Phone', render: (d) => <strong>{d.name}</strong> },
    { key: 'staff', header: 'Whose', render: (d) => d.staff },
    { key: 'seen', header: 'Last seen', nowrap: true, render: (d) => d.lastSeen },
    { key: 'ip', header: 'Address', optional: true, nowrap: true, render: (d) => d.lastIp },
    {
      key: 'remove',
      header: '',
      numeric: true,
      render: (d) => (
        <Button size="sm" variant="quiet" disabled={busy} onClick={() => setRemoving(d)}>
          Remove
        </Button>
      ),
    },
  ];

  return (
    <Page className="mb-phones-page">
      <PageHeader
        title="Phones"
        subtitle={subtitle}
        actions={
          view.code ? (
            <Button variant="secondary" disabled={busy} onClick={() => run(() => call('close_pairing'))}>
              Stop adding
            </Button>
          ) : (
            <Button
              variant="primary"
              disabled={busy || full}
              title={full ? 'Every phone the plan allows is on this counter' : undefined}
              onClick={() => run(() => call('open_pairing'))}
            >
              <Icon name="plus" size="sm" />
              Add phones
            </Button>
          )
        }
      />

      {/* The state of the road, in one sentence written in Rust. */}
      <Notice
        tone={tone}
        standing
        action={
          view.mayFixFirewall ? (
            <Button
              variant="primary"
              size="sm"
              disabled={busy}
              onClick={() =>
                run(() => call('allow_firewall'), 'Windows Firewall now lets phones in.')
              }
            >
              Allow through Windows Firewall
            </Button>
          ) : null
        }
      >
        {view.headline}
      </Notice>
      {view.certificateNote ? <Notice tone="warn">{view.certificateNote}</Notice> : null}

      {view.code ? (
        <Panel title="Add a phone" note="Scan the code with the Magic Bill app, or type it in.">
          <div className="mb-phones__pairing">
            <div
              className="mb-phones__qr"
              role="img"
              aria-label="Scan this with the Magic Bill app on the phone"
            >
              {view.qr.map((row, y) => (
                // The code is positional: two identical rows are two different places.
                <div className="mb-phones__qrrow" key={`${y}-${row}`}>
                  {[...row].map((cell, x) => (
                    <span
                      key={`${y}-${x}`}
                      className={cell === '#' ? 'mb-phones__dot mb-phones__dot--on' : 'mb-phones__dot'}
                    />
                  ))}
                </div>
              ))}
            </div>
            <div className="mb-phones__code">
              <span className="mb-phones__label">Code</span>
              <strong className="mb-code">{view.code}</strong>
              <span className="mb-phones__label">Changes after every phone</span>
            </div>
          </div>

          {view.waiting.length > 0 ? (
            <div className="mb-phones__waiting">
              {view.waiting.map((w) => {
                const owner = owners[w.requestId] ?? '';
                return (
                  <div className="mb-phones__ask" key={w.requestId}>
                    <div className="mb-phones__who">
                      <Icon name="phone" size="md" />
                      <div>
                        <strong>{w.name}</strong>
                        <span className="mb-phones__label">{w.ip}</span>
                      </div>
                    </div>
                    {/* Whose phone it is decides what it may do: a waiter's phone acts as the
                        waiter, a shared tablet acts as nobody. */}
                    <Select
                      aria-label="Whose phone is this?"
                      value={owner}
                      onChange={(e) => setOwners({ ...owners, [w.requestId]: e.target.value })}
                      options={[
                        { value: '', label: 'Whose phone?' },
                        { value: SHARED, label: 'Shared tablet' },
                        ...view.people.map((p) => ({ value: p.id, label: p.name })),
                      ]}
                    />
                    <div className="mb-phones__answer">
                      <Button
                        variant="quiet"
                        disabled={busy}
                        onClick={() => run(() => call('refuse_device', { requestId: w.requestId }))}
                      >
                        Refuse
                      </Button>
                      <Button
                        variant="primary"
                        disabled={busy || owner === ''}
                        onClick={() =>
                          run(
                            () =>
                              call('allow_device', {
                                requestId: w.requestId,
                                staffId: owner === SHARED ? null : owner,
                              }),
                            `${w.name} is on this counter.`,
                          )
                        }
                      >
                        Allow
                      </Button>
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <p className="mb-phones__label">Waiting for a phone to scan.</p>
          )}
        </Panel>
      ) : null}

      <Panel
        title="On this counter"
        actions={<Badge tone={full ? 'warn' : 'neutral'}>{`${phones.length} of ${view.phonesAllowed}`}</Badge>}
        flush
      >
        <Table
          columns={columns}
          rows={phones}
          rowKey={(d) => d.id}
          empty={<EmptyState small title="No phones yet" says="Press Add phones and scan the code." />}
        />
      </Panel>

      <Panel title="This counter">
        <Facts>
          <Fact label="Address">{view.address || '—'}</Fact>
          <Fact label="Port">{view.port || '—'}</Fact>
          <Fact label="Security code" code className="mb-phones__fingerprint-fact">
            {view.fingerprint ? (
              <span className="mb-phones__fingerprint">
                {view.fingerprint}
                <Button size="sm" variant="quiet" onClick={() => copy(view.fingerprint)}>
                  <Icon name="copy" size="sm" />
                  Copy
                </Button>
              </span>
            ) : (
              '—'
            )}
          </Fact>
        </Facts>
      </Panel>

      <ConfirmDialog
        open={removing !== null}
        title={`Remove ${removing?.name ?? 'this phone'}?`}
        body="It stops working on its next tap. It can be added again."
        confirmLabel="Remove"
        destructive
        onConfirm={() => {
          const device = removing;
          setRemoving(null);
          if (!device) return;
          run(() => call('revoke_device', { deviceId: device.id }), `${device.name} removed.`);
        }}
        onCancel={() => setRemoving(null)}
      />
    </Page>
  );
}
