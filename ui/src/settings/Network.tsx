/** The phones this counter serves. */

import { useCallback, useEffect, useState } from 'react';

import { Badge, Button, Card, ConfirmDialog, EmptyState, Scroller, SectionHeader, Spinner, useToast } from '../kit';
import { call, inApp, isUiError, subscribe } from '../ipc/call';
import type { DeviceRowView } from '../ipc/generated/DeviceRowView';
import type { NetworkView } from '../ipc/generated/NetworkView';

export function Network() {
  const [view, setView] = useState<NetworkView | null>(null);
  const [removing, setRemoving] = useState<DeviceRowView | null>(null);
  const [fixing, setFixing] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  useEffect(() => {
    call('network').then(setView).catch(complain);
  }, [complain]);

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

  // Whose phone each waiting one is: chosen before Allow. '' = a shared tablet, nobody's.
  const [owners, setOwners] = useState<Record<string, string>>({});
  const act = (
    command: 'open_pairing' | 'close_pairing' | 'allow_device' | 'refuse_device',
    args?: { requestId: string; staffId?: string | null },
  ) => {
    call(command, args as never)
      .then(setView)
      .catch(complain);
  };

  if (!view) return <Spinner label="Looking at the network" />;

  const live = view.devices;

  return (
    <Scroller className="mb-network">
      <SectionHeader
        title="Phones"
        note="Waiters take orders on a phone, over this shop's own WiFi."
        action={
          <span className="mb-network__badges">
            <Badge tone={view.connected > 0 ? 'ok' : 'neutral'}>
              {view.connected === 1 ? '1 live' : `${view.connected} live`}
            </Badge>
            <Badge tone={view.tone === 'ok' ? 'ok' : view.tone === 'warn' ? 'warn' : 'danger'}>
              {view.tone === 'ok' ? 'On' : 'Off'}
            </Badge>
          </span>
        }
      />

      {/* The sentence at the top, written in Rust. */}
      <Card className={`mb-network__headline mb-network__headline--${view.tone}`}>
        <strong>{view.headline}</strong>
        {view.mayFixFirewall ? (
          <div className="mb-row">
            <Button
              variant="primary"
              disabled={fixing}
              onClick={() => {
                setFixing(true);
                call('allow_firewall')
                  .then((fresh) => {
                    setView(fresh);
                    toast.show('ok', 'Windows Firewall now lets phones reach this counter.');
                  })
                  .catch(complain)
                  .finally(() => setFixing(false));
              }}
            >
              Allow Magic Bill through Windows Firewall
            </Button>
          </div>
        ) : null}
        {view.certificateNote ? (
          <p className="mb-network__note">{view.certificateNote}</p>
        ) : null}
        {view.fingerprint ? (
          <p className="mb-network__fingerprint">
            <span>This counter's security code</span>
            <code className="mb-mono">{view.fingerprint}</code>
          </p>
        ) : null}
      </Card>

      {view.mayPair ? (
        <Card>
          <div className="mb-row--end">
            {view.code ? (
              <Button variant="quiet" onClick={() => act('close_pairing')}>
                Stop showing the code
              </Button>
            ) : (
              <Button variant="primary" onClick={() => act('open_pairing')}>
                Add phones
              </Button>
            )}
          </div>

          {view.code ? (
            <div className="mb-network__pairing">
              {/* A row of flex rows, not one grid. */}
              <div
                className="mb-network__qr"
                role="img"
                aria-label="Scan this with the Magic Bill app on the phone"
              >
                {view.qr.map((row, y) => (
                  // The code is positional: two identical rows are two different places, so the
                  // coordinates ARE the identity.
                  <div className="mb-network__qrrow" key={`${y}-${row}`}>
                    {[...row].map((cell, x) => (
                      <span
                        key={`${y}-${x}`}
                        className={
                          cell === '#' ? 'mb-network__dot mb-network__dot--on' : 'mb-network__dot'
                        }
                      />
                    ))}
                  </div>
                ))}
              </div>
              <div className="mb-network__code">
                <p>On the phone: scan this. The phone then shows up below — pick whose it is and press Allow.</p>
                <strong className="mb-mono">{view.code}</strong>
                <p className="mb-network__note">
                  Or type the code. It changes after every phone, so the next one scans a fresh
                  one — add as many as you like, then stop showing it.
                </p>
              </div>
            </div>
          ) : null}

          {view.waiting.length > 0 ? (
            <div className="mb-network__waiting">
              {view.waiting.map((w) => (
                <div className="mb-network__ask" key={w.requestId}>
                  {/* The whole sentence, written in Rust. Every phone waits here for Allow. */}
                  <span>{w.says}</span>
                  <div className="mb-row--end">
                    {/* Whose phone it is decides what it may do: a waiter's phone acts as the
                        waiter, a shared tablet at the pass acts as nobody. */}
                    <select
                      className="mb-select"
                      aria-label="Whose phone is this?"
                      value={owners[w.requestId] ?? ''}
                      onChange={(e) => setOwners({ ...owners, [w.requestId]: e.target.value })}
                    >
                      <option value="">Whose phone? (or: shared, nobody's)</option>
                      {view.people.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.name}
                        </option>
                      ))}
                    </select>
                    <Button
                      size="sm"
                      variant="quiet"
                      onClick={() => act('refuse_device', { requestId: w.requestId })}
                    >
                      Refuse
                    </Button>
                    <Button
                      size="sm"
                      variant="primary"
                      onClick={() =>
                        act('allow_device', {
                          requestId: w.requestId,
                          staffId: owners[w.requestId] || null,
                        })
                      }
                    >
                      Allow
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          ) : null}
        </Card>
      ) : null}

      <Card>
        <SectionHeader
          title="Phones on this counter"
          action={
            live.length > 0 ? <Badge tone="neutral">{live.length}</Badge> : null
          }
        />
        {live.length === 0 ? (
          <EmptyState
            small
            title="No phones yet"
            hint='Press "Add phones" and scan the code with the Magic Bill app.'
          />
        ) : (
          live.map((device) => (
            <div className="mb-network__device" key={device.id}>
              <div>
                <strong>{device.name}</strong>
                <p className="mb-network__note">
                  {device.staff} · last seen {device.lastSeen}
                  {device.lastIp ? ` · ${device.lastIp}` : ''}
                </p>
              </div>
              {view.mayPair ? (
                <Button size="sm" variant="danger" onClick={() => setRemoving(device)}>
                  Remove
                </Button>
              ) : null}
            </div>
          ))
        )}
      </Card>

      <ConfirmDialog
        open={removing !== null}
        title={`Remove ${removing?.name ?? 'this phone'}?`}
        body="It stops working immediately — on its very next tap, not the next time somebody signs in. It can be added again."
        confirmLabel="Remove it"
        destructive
        onConfirm={() => {
          const device = removing;
          setRemoving(null);
          if (!device) return;
          call('revoke_device', { deviceId: device.id })
            .then((fresh) => {
              setView(fresh);
              toast.show('ok', `${device.name} has been taken off this counter.`);
            })
            .catch(complain);
        }}
        onCancel={() => setRemoving(null)}
      />
    </Scroller>
  );
}
