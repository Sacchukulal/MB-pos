/** The version this counter runs, and the way back off it. */

import { useCallback, useEffect, useState } from 'react';

import { Button, Card, Notice, plural, SectionHeader, Spinner, useToast } from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { UpdateState } from '../ipc/generated/UpdateState';

export function Updates() {
  const [view, setView] = useState<UpdateState | null>(null);
  const [busy, setBusy] = useState(false);
  /** The sentence `go_back_a_version` answered with, once it has. */
  const [goingBack, setGoingBack] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  useEffect(() => {
    if (!inApp()) return;
    // On open, not on a timer.
    call('look_for_an_update').then(setView).catch(complain);
  }, [complain]);

  if (!view) return <Spinner label="Looking at this version" />;

  return (
    <Card className="mb-updates">
      <SectionHeader
        title="This version"
        note="What this counter is running, and how to get off it if a new one goes wrong."
      />

      <dl className="mb-updates__facts">
        <dt>Running</dt>
        <dd className="mb-mono">
          {view.running}
          {view.isDevBuild ? ' (a development build)' : ''}
        </dd>
        <dt>Installed</dt>
        <dd>
          {view.daysOnThisVersion === 0
            ? 'today'
            : `${plural(view.daysOnThisVersion, 'day')} ago`}
        </dd>
      </dl>

      {view.available ? (
        <Notice tone="info">
          <strong>Version {view.available} is available.</strong>
          {view.notes ? <p>{view.notes}</p> : null}
        </Notice>
      ) : (
        <p className="mb-muted">
          {view.isDevBuild
            ? 'This is a development build, so it is not checked against the released ones.'
            : 'This is the newest version we know about.'}
        </p>
      )}

      <div className="mb-row mb-row--end">
        <Button
          variant="quiet"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            call('look_for_an_update')
              .then((fresh) => {
                setView(fresh);
                toast.show(
                  'ok',
                  fresh.available
                    ? `Version ${fresh.available} is available.`
                    : 'You are on the newest version.',
                );
              })
              .catch(complain)
              .finally(() => setBusy(false));
          }}
        >
          Check for an update
        </Button>


        <Button variant="danger" disabled={busy} onClick={() => setConfirming(true)}>
          Go back a version
        </Button>
      </div>

      {confirming ? (
        <Notice tone="warn" icon="warning">
          <strong>Go back to the version you had before?</strong>
          <p>
            Your shop&rsquo;s data is not touched — only the program is. Do this
            if a new version will not start, or has broken something you need
            tonight.
          </p>
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setConfirming(false)}>
              Stay on this one
            </Button>
            <Button
              variant="danger"
              disabled={busy}
              onClick={() => {
                setBusy(true);
                call('go_back_a_version')
                  .then((says) => {
                    setConfirming(false);
                    setGoingBack(says);
                  })
                  .catch(complain)
                  .finally(() => setBusy(false));
              }}
            >
              Go back
            </Button>
          </div>
        </Notice>
      ) : null}

      {/* The sentence Rust wrote, with the installer's path in it. */}
      {goingBack ? (
        <Notice tone="info">
          <p>{goingBack}</p>
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setGoingBack(null)}>
              Close
            </Button>
          </div>
        </Notice>
      ) : null}
    </Card>
  );
}
