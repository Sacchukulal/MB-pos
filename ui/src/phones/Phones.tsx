/** The phones on this counter: add one, say whose it is, take one off. */

import { useCallback, useEffect, useState } from 'react';

import {
  Button,
  ConfirmDialog,
  EmptyState,
  Icon,
  Modal,
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
  /** Whose the phone at the desk is, chosen before Allow. */
  const [picked, setPicked] = useState<{ id: string; owner: string } | null>(null);
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

  // The rules are read at start and remembered, so a counter blocked since would still say the
  // phones can get in. The screen paints from the remembered answer above and this replaces it
  // with a fresh one — asking takes a moment, and nobody waits for it.
  useEffect(() => {
    call('check_firewall')
      .then(setView)
      .catch(() => undefined);
  }, []);

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
  // The desk is open while Rust shows a code or holds a phone at it; the dialog mirrors that.
  // Phones are answered one at a time, first come first served.
  const asking = view.waiting[0];
  const adding = view.code !== '' || asking !== undefined;
  // The choice belongs to one request: the next phone starts with nobody chosen.
  const owner = picked !== null && picked.id === asking?.requestId ? picked.owner : '';

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
          <Button
            variant="primary"
            disabled={busy || full}
            title={full ? 'Every phone the plan allows is on this counter' : undefined}
            onClick={() => run(() => call('open_pairing'))}
          >
            <Icon name="plus" size="sm" />
            Add phone
          </Button>
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

      <Panel title="On this counter" flush>
        <Table
          columns={columns}
          rows={phones}
          rowKey={(d) => d.id}
          empty={<EmptyState small title="No phones yet" says="Press Add phone and scan the code." />}
        />
      </Panel>

      <Modal
        open={adding}
        title="Add phone"
        onClose={() => run(() => call('close_pairing'))}
        actions={
          asking ? (
            <>
              <Button
                variant="quiet"
                disabled={busy}
                onClick={() => run(() => call('refuse_device', { requestId: asking.requestId }))}
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
                        requestId: asking.requestId,
                        staffId: owner === SHARED ? null : owner,
                      }),
                    `${asking.name} is on this counter.`,
                  )
                }
              >
                Allow
              </Button>
            </>
          ) : (
            <Button disabled={busy} onClick={() => run(() => call('close_pairing'))}>
              Cancel
            </Button>
          )
        }
      >
        {asking ? (
          <>
            <div className="mb-phones__asking">
              <Icon name="phone" size="lg" />
              <div>
                <strong>{asking.name}</strong>
                <span className="mb-phones__label">{asking.ip}</span>
              </div>
            </div>
            {/* Whose phone it is decides what it may do: a waiter's phone acts as the waiter,
                a shared tablet acts as nobody. */}
            <Select
              label="Whose phone"
              value={owner}
              onChange={(e) => setPicked({ id: asking.requestId, owner: e.target.value })}
              options={[
                { value: '', label: 'Choose' },
                { value: SHARED, label: 'Shared tablet' },
                ...view.people.map((p) => ({ value: p.id, label: p.name })),
              ]}
            />
          </>
        ) : (
          <ScanCode qr={view.qr} code={view.code} />
        )}
      </Modal>

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

/** The QR with the short code under it, for the phone that cannot scan. */
function ScanCode({ qr, code }: { qr: readonly string[]; code: string }) {
  return (
    <div className="mb-phones__scan">
      <div className="mb-phones__qr" role="img" aria-label="Scan this with the Magic Bill app">
        {qr.map((row, y) => (
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
      <span className="mb-phones__label">Or type the code in the app</span>
      <strong className="mb-phones__code">{code}</strong>
      <span className="mb-phones__label">Waiting for a phone to scan</span>
    </div>
  );
}
