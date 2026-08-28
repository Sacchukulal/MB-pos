/** The version this counter runs, the one waiting, and the way back off it. */

import { useCallback, useEffect, useState } from 'react';

import { Button, Card, Notice, plural, SectionHeader, Spinner, useToast } from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { UpdateState } from '../ipc/generated/UpdateState';

export function Updates() {
  const [view, setView] = useState<UpdateState | null>(null);
  const [busy, setBusy] = useState(false);
  /** The sentence Rust answered with once an installer was handed to Windows. */
  const [closing, setClosing] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<'install' | 'back' | null>(null);
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

  /** Download, check, keep the way back, hand over to the installer. */
  const install = () => {
    setBusy(true);
    call('install_update')
      .then((says) => {
        setConfirming(null);
        setClosing(says);
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const goBack = () => {
    setBusy(true);
    call('go_back_a_version')
      .then((says) => {
        setConfirming(null);
        setClosing(says);
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  return (
    <Card className="mb-updates">
      <SectionHeader
        title="This version"
        note="What this counter is running, what is waiting, and how to get off a new one if it goes wrong."
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
        {view.previous ? (
          <>
            <dt>Before this</dt>
            <dd className="mb-mono">{view.previous}</dd>
          </>
        ) : null}
      </dl>

      {view.available ? (
        <Notice tone="info">
          <strong>Version {view.available} is available.</strong>
          {view.notes ? <p>{view.notes}</p> : null}
          {view.downloaded ? <p>It is downloaded and checked, waiting to be installed.</p> : null}
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

        {view.available ? (
          <Button variant="primary" disabled={busy} onClick={() => setConfirming('install')}>
            Install {view.available}
          </Button>
        ) : null}

        <Button variant="danger" disabled={busy} onClick={() => setConfirming('back')}>
          Go back a version
        </Button>
      </div>

      {confirming === 'install' ? (
        <Notice tone="info" icon="download">
          <strong>Install version {view.available} now?</strong>
          <p>
            Magic Bill downloads it, checks it, and then closes so the installer can
            put it in place. Do this after the last bill of the day — your
            shop&rsquo;s data is not touched, and the version you have now is kept so
            you can go back.
          </p>
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setConfirming(null)}>
              Not now
            </Button>
            <Button variant="primary" disabled={busy} onClick={install}>
              Install and close
            </Button>
          </div>
        </Notice>
      ) : null}

      {confirming === 'back' ? (
        <Notice tone="warn" icon="warning">
          <strong>Go back to the version you had before?</strong>
          <p>
            Your shop&rsquo;s data is not touched — only the program is. Do this
            if a new version will not start, or has broken something you need
            tonight.
          </p>
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setConfirming(null)}>
              Stay on this one
            </Button>
            <Button variant="danger" disabled={busy} onClick={goBack}>
              Go back
            </Button>
          </div>
        </Notice>
      ) : null}

      {/* The sentence Rust wrote. The counter is about to close. */}
      {closing ? (
        <Notice tone="info">
          <p>{closing}</p>
        </Notice>
      ) : null}
    </Card>
  );
}
